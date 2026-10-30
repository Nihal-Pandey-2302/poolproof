//! Just enough of the Postgres wire protocol to split a byte stream into messages.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpStream;

pub const SSL_REQUEST: i32 = 80877103;
pub const GSSENC_REQUEST: i32 = 80877104;
pub const CANCEL_REQUEST: i32 = 80877102;

/// Larger messages are a broken peer, not something the self-test sends.
const MAX_LEN: usize = 64 << 20;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Connect to Postgres, giving up after 10s instead of waiting for the OS timeout.
pub async fn connect(addr: &str) -> io::Result<TcpStream> {
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, format!("connecting to {addr} timed out")))??;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// One typed message: tag byte plus body, `raw` is the whole message as sent.
pub struct Message {
    pub tag: u8,
    pub raw: Bytes,
}

impl Message {
    pub fn body(&self) -> &[u8] {
        &self.raw[5..]
    }

    pub fn new(tag: u8, body: &[u8]) -> Self {
        let mut raw = BytesMut::with_capacity(body.len() + 5);
        raw.put_u8(tag);
        raw.put_i32(body.len() as i32 + 4);
        raw.put_slice(body);
        Message { tag, raw: raw.freeze() }
    }
}

pub struct Reader<R> {
    inner: R,
    buf: BytesMut,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

impl<R: AsyncRead + Unpin> Reader<R> {
    pub fn new(inner: R) -> Self {
        Reader {
            inner,
            buf: BytesMut::new(),
        }
    }

    /// Fill the buffer to `n` bytes. False on a clean EOF before any byte of this read.
    async fn fill(&mut self, n: usize) -> io::Result<bool> {
        while self.buf.len() < n {
            if self.inner.read_buf(&mut self.buf).await? == 0 {
                if self.buf.is_empty() {
                    return Ok(false);
                }
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
        Ok(true)
    }

    /// The untyped first packet of a client connection: startup, SSLRequest or
    /// CancelRequest. Returns the request code and the whole packet.
    pub async fn startup(&mut self) -> io::Result<Option<(i32, Bytes)>> {
        if !self.fill(4).await? {
            return Ok(None);
        }
        let len = i32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if !(8..=MAX_LEN).contains(&len) {
            return Err(invalid("bad startup length"));
        }
        self.fill(len).await?;
        let raw = self.buf.split_to(len).freeze();
        let code = i32::from_be_bytes([raw[4], raw[5], raw[6], raw[7]]);
        Ok(Some((code, raw)))
    }

    /// The complete messages already buffered, as (tag, body), without consuming them.
    pub fn buffered(&self) -> Vec<(u8, &[u8])> {
        let mut out = Vec::new();
        let mut rest = &self.buf[..];
        while rest.len() >= 5 {
            let len = (&rest[1..5]).get_i32();
            let Ok(len) = usize::try_from(len) else { break };
            if !(4..=MAX_LEN).contains(&len) || rest.len() < len + 1 {
                break;
            }
            out.push((rest[0], &rest[5..len + 1]));
            rest = &rest[len + 1..];
        }
        out
    }

    /// The next typed message, or None on EOF between messages.
    pub async fn next(&mut self) -> io::Result<Option<Message>> {
        if !self.fill(5).await? {
            return Ok(None);
        }
        let len = (&self.buf[1..5]).get_i32() as usize;
        if !(4..=MAX_LEN).contains(&len) {
            return Err(invalid("bad message length"));
        }
        self.fill(len + 1).await?;
        let raw = self.buf.split_to(len + 1).freeze();
        Ok(Some(Message { tag: raw[0], raw }))
    }
}

impl Reader<tokio::net::tcp::OwnedReadHalf> {
    /// Take in whatever has arrived, without waiting. False if the peer has closed.
    pub fn poll_alive(&mut self) -> bool {
        let mut chunk = [0u8; 8192];
        loop {
            match self.inner.try_read(&mut chunk) {
                Ok(0) => return false,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return true,
                Err(_) => return false,
            }
        }
    }
}

/// Whether an ErrorResponse body has severity FATAL or PANIC: the session is over.
pub fn is_fatal(body: &[u8]) -> bool {
    let mut rest = body;
    while let Some((&field, tail)) = rest.split_first() {
        if field == 0 {
            break;
        }
        let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
        // 'V' is the severity that is never translated; 'S' may be.
        if field == b'V' || field == b'S' {
            let severity = &tail[..end];
            if severity == b"FATAL" || severity == b"PANIC" {
                return true;
            }
        }
        rest = tail.get(end + 1..).unwrap_or(&[]);
    }
    false
}

/// The command tag of a CommandComplete body (NUL-terminated string).
pub fn tag_text(body: &[u8]) -> &str {
    let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    std::str::from_utf8(&body[..end]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn reads_messages_split_across_reads() {
        let mut wire = Vec::new();
        wire.extend_from_slice(&Message::new(b'Q', b"SELECT 1\0").raw);
        wire.extend_from_slice(&Message::new(b'S', b"").raw);
        let (mut a, b) = tokio::io::duplex(3);
        let writer = tokio::spawn(async move { a.write_all(&wire).await });
        let mut r = Reader::new(b);
        let m = r.next().await.unwrap().unwrap();
        assert_eq!((m.tag, m.body()), (b'Q', &b"SELECT 1\0"[..]));
        let m = r.next().await.unwrap().unwrap();
        assert_eq!((m.tag, m.body()), (b'S', &b""[..]));
        writer.await.unwrap().unwrap();
        assert!(r.next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn buffered_skips_partial_and_bad_messages() {
        let (mut a, b) = tokio::io::duplex(64);
        a.write_all(&Message::new(b'A', b"x").raw).await.unwrap();
        a.write_all(&Message::new(b'E', b"y").raw).await.unwrap();
        a.write_all(&[b'Z', 0, 0, 0, 5]).await.unwrap();
        let mut r = Reader::new(b);
        r.fill(13).await.unwrap();
        assert_eq!(r.buffered(), vec![(b'A', &b"x"[..]), (b'E', &b"y"[..])]);
        assert_eq!(r.next().await.unwrap().unwrap().tag, b'A');

        // A negative length must not overflow.
        let (mut a, b) = tokio::io::duplex(64);
        a.write_all(&[b'E', 0xff, 0xff, 0xff, 0xff, 0]).await.unwrap();
        let mut r = Reader::new(b);
        r.fill(6).await.unwrap();
        assert!(r.buffered().is_empty());
    }

    #[test]
    fn fatal_severity() {
        assert!(is_fatal(b"SFATAL\0VFATAL\0C57P01\0Mterminating connection\0\0"));
        assert!(is_fatal(b"SPANIC\0\0"));
        assert!(!is_fatal(b"SERROR\0VERROR\0C42601\0\0"));
        assert!(!is_fatal(b""));
    }

    #[tokio::test]
    async fn eof_inside_a_message_is_an_error() {
        let (mut a, b) = tokio::io::duplex(64);
        a.write_all(&[b'Q', 0, 0, 0, 20, b'x']).await.unwrap();
        drop(a);
        assert!(Reader::new(b).next().await.is_err());
    }

    #[tokio::test]
    async fn startup_packet() {
        let (mut a, b) = tokio::io::duplex(64);
        let mut p = vec![0, 0, 0, 8];
        p.extend_from_slice(&SSL_REQUEST.to_be_bytes());
        a.write_all(&p).await.unwrap();
        let (code, raw) = Reader::new(b).startup().await.unwrap().unwrap();
        assert_eq!((code, raw.len()), (SSL_REQUEST, 8));
    }
}
