//! A bare protocol connection for `send` steps. Drivers decide statement names,
//! when to Sync and what to pipeline; pooler bugs often hide exactly there, so
//! these steps write the frontend messages themselves and record every reply.

use std::io;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bytes::BytesMut;
use fallible_iterator::FallibleIterator;
use postgres_protocol::IsNull;
use postgres_protocol::authentication::md5_hash;
use postgres_protocol::authentication::sasl::{ChannelBinding, ScramSha256};
use postgres_protocol::message::backend::Message;
use postgres_protocol::message::frontend;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout_at};
use tokio_postgres::config::Host;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::pg::tcp_connect;
use super::tls::{SslMode, Tls, wants_client_certificate};
use crate::targets::Endpoint;

/// One frontend message, written in a scenario as e.g. `parse s1 SELECT 1`.
/// `-` stands for the unnamed statement or portal.
#[derive(Debug, Clone, PartialEq)]
pub enum RawMessage {
    Parse { name: String, sql: String },
    Bind { portal: String, statement: String },
    Describe { portal: bool, name: String },
    Execute { portal: String, max_rows: i32 },
    Close { portal: bool, name: String },
    Sync,
    Flush,
    Query(String),
    CopyData(String),
    CopyDone,
}

fn name(s: &str) -> String {
    if s == "-" { String::new() } else { s.to_string() }
}

fn kind(s: &str) -> Result<bool> {
    match s {
        "statement" => Ok(false),
        "portal" => Ok(true),
        other => bail!("expected `statement` or `portal`, got `{other}`"),
    }
}

impl FromStr for RawMessage {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let (command, rest) = s.split_once(' ').unwrap_or((s, ""));
        let words: Vec<&str> = rest.split_whitespace().collect();
        let message = match command {
            "parse" => {
                let (n, sql) = rest.split_once(' ').context("usage: parse NAME SQL")?;
                RawMessage::Parse {
                    name: name(n),
                    sql: sql.to_string(),
                }
            }
            "bind" => match words.as_slice() {
                [portal, statement] => RawMessage::Bind {
                    portal: name(portal),
                    statement: name(statement),
                },
                _ => bail!("usage: bind PORTAL STATEMENT"),
            },
            "describe" => match words.as_slice() {
                [k, n] => RawMessage::Describe {
                    portal: kind(k)?,
                    name: name(n),
                },
                _ => bail!("usage: describe statement|portal NAME"),
            },
            "execute" => match words.as_slice() {
                [portal] => RawMessage::Execute {
                    portal: name(portal),
                    max_rows: 0,
                },
                [portal, rows] => RawMessage::Execute {
                    portal: name(portal),
                    max_rows: rows.parse()?,
                },
                _ => bail!("usage: execute PORTAL [MAX_ROWS]"),
            },
            "close" => match words.as_slice() {
                [k, n] => RawMessage::Close {
                    portal: kind(k)?,
                    name: name(n),
                },
                _ => bail!("usage: close statement|portal NAME"),
            },
            "sync" => RawMessage::Sync,
            "flush" => RawMessage::Flush,
            "query" => RawMessage::Query(rest.to_string()),
            "copydata" => RawMessage::CopyData(rest.to_string()),
            "copydone" => RawMessage::CopyDone,
            other => bail!("unknown message `{other}`"),
        };
        Ok(message)
    }
}

impl RawMessage {
    fn write(&self, buf: &mut BytesMut) -> Result<()> {
        let variant = |portal: bool| if portal { b'P' } else { b'S' };
        match self {
            RawMessage::Parse { name, sql } => frontend::parse(name, sql, [], buf)?,
            RawMessage::Bind { portal, statement } => {
                // No parameters; ask for text results so rows are readable.
                frontend::bind(portal, statement, [], Vec::<()>::new(), |_, _| Ok(IsNull::No), [0], buf)
                    .map_err(|_| anyhow::anyhow!("encoding Bind"))?
            }
            RawMessage::Describe { portal, name } => frontend::describe(variant(*portal), name, buf)?,
            RawMessage::Execute { portal, max_rows } => frontend::execute(portal, *max_rows, buf)?,
            RawMessage::Close { portal, name } => frontend::close(variant(*portal), name, buf)?,
            RawMessage::Sync => frontend::sync(buf),
            RawMessage::Flush => frontend::flush(buf),
            RawMessage::Query(sql) => frontend::query(sql, buf)?,
            RawMessage::CopyData(data) => frontend::CopyData::new(data.as_bytes())?.write(buf),
            RawMessage::CopyDone => frontend::copy_done(buf),
        }
        Ok(())
    }

    /// Messages after which the server sends ReadyForQuery.
    fn ends_cycle(&self) -> bool {
        matches!(self, RawMessage::Sync | RawMessage::Query(_) | RawMessage::CopyDone)
    }
}

/// The largest backend message accepted on a `send` connection. A server can claim
/// any length up to 4 GB, and parsing reserves that much before reading it.
/// tokio-postgres clients use their own codec and are not covered by this cap.
const MAX_MESSAGE: usize = 64 << 20;

/// Rejects a message header whose length is impossible or above `MAX_MESSAGE`.
fn check_header(buf: &[u8]) -> Result<()> {
    let Some(len) = buf.get(1..5) else {
        return Ok(());
    };
    let len = u32::from_be_bytes([len[0], len[1], len[2], len[3]]) as usize;
    if len < 4 {
        bail!("server sent a message with invalid length {len}");
    }
    if len > MAX_MESSAGE {
        bail!("server sent a {len}-byte message, the limit is {MAX_MESSAGE} bytes");
    }
    Ok(())
}

/// How `exchange` waits for replies and what it records.
#[derive(Debug, Clone, Copy)]
pub struct ExchangeOpts {
    /// How long to wait for all replies.
    pub step: Duration,
    /// Without a Sync, how long the server must be quiet before the replies count as
    /// complete.
    pub quiet: Duration,
    /// Record each ParameterStatus as `ParameterStatus name=value`. Off by default,
    /// since poolers add, drop and reorder them.
    pub parameter_status: bool,
}

enum Next {
    Message(Message),
    Timeout,
    /// The connection ended, with the read error if there was one.
    Closed(Option<io::Error>),
}

/// The connection under a `send` client: plain TCP, or TLS after an SSLRequest.
enum RawStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for RawStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            RawStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            RawStream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for RawStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            RawStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            RawStream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            RawStream::Plain(s) => Pin::new(s).poll_flush(cx),
            RawStream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            RawStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            RawStream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Asks for TLS with an SSLRequest, as libpq does, and starts it if the server agrees.
/// A server that declines gets plaintext only with `prefer`.
async fn negotiate_tls(mut tcp: TcpStream, tls: &Tls, host: &str) -> Result<RawStream> {
    let Some(config) = tls.client_config()? else {
        return Ok(RawStream::Plain(tcp));
    };
    let mut out = BytesMut::new();
    frontend::ssl_request(&mut out);
    tcp.write_all(&out).await?;
    // Exactly one byte: anything the server sent after it must go through TLS, not be
    // read as plaintext (CVE-2021-23222).
    let mut answer = [0u8];
    tcp.read_exact(&mut answer)
        .await
        .context("reading the SSLRequest answer")?;
    match answer[0] {
        b'S' => {
            let name = ServerName::try_from(host.to_string()).context("server name for TLS")?;
            let stream = match TlsConnector::from(config.clone()).connect(name, tcp).await {
                Ok(stream) => stream,
                Err(e) if wants_client_certificate(&e) => {
                    return Err(anyhow::Error::from(e).context(
                        "TLS handshake: the server wants a client certificate, which poolproof doesn't support yet",
                    ));
                }
                Err(e) => return Err(anyhow::Error::from(e).context("TLS handshake")),
            };
            Ok(RawStream::Tls(Box::new(stream)))
        }
        b'N' if tls.mode == SslMode::Prefer => Ok(RawStream::Plain(tcp)),
        b'N' => bail!("the server does not support TLS (sslmode={})", tls.mode),
        other => bail!("unexpected answer to SSLRequest: {:?}", other as char),
    }
}

pub struct RawConn {
    stream: RawStream,
    buf: BytesMut,
}

impl RawConn {
    pub async fn connect(endpoint: &Endpoint, connect_timeout: Duration) -> Result<Self> {
        let config = endpoint.config();
        let host = match config.get_hosts().first() {
            Some(Host::Tcp(host)) => host.clone(),
            _ => bail!("send steps need a TCP host"),
        };
        let port = config.get_ports().first().copied().unwrap_or(5432);
        let user = config.get_user().context("no user in URL")?.to_string();
        let password = config.get_password().unwrap_or_default().to_vec();
        let dbname = config.get_dbname().unwrap_or(&user).to_string();

        // One deadline for TCP connect and startup, as for tokio-postgres clients.
        let deadline = Instant::now() + connect_timeout;
        let tcp = timeout_at(deadline, tcp_connect(config, &host, port))
            .await
            .context("connect timed out")??;
        let stream = timeout_at(deadline, negotiate_tls(tcp, endpoint.tls(), &host))
            .await
            .context("connect timed out")??;
        let mut conn = RawConn {
            stream,
            buf: BytesMut::new(),
        };
        let mut out = BytesMut::new();
        frontend::startup_message([("user", user.as_str()), ("database", dbname.as_str())], &mut out)?;
        conn.send(&out).await?;

        let mut scram: Option<ScramSha256> = None;
        loop {
            let message = match conn.next(deadline).await? {
                Next::Message(m) => m,
                Next::Timeout => bail!("startup timed out"),
                // With TLS 1.3 the server refuses a missing client certificate only after
                // the handshake, so it shows up here.
                Next::Closed(Some(e)) if wants_client_certificate(&e) => {
                    return Err(anyhow::Error::from(e)
                        .context("the server wants a client certificate, which poolproof doesn't support yet"));
                }
                Next::Closed(Some(e)) => return Err(anyhow::Error::from(e).context("connection closed during startup")),
                Next::Closed(None) => bail!("connection closed during startup"),
            };
            let mut out = BytesMut::new();
            match message {
                Message::BackendKeyData(_) => continue,
                Message::AuthenticationOk | Message::ParameterStatus(_) => continue,
                Message::NoticeResponse(_) => continue,
                Message::ReadyForQuery(_) => return Ok(conn),
                Message::AuthenticationCleartextPassword => frontend::password_message(&password, &mut out)?,
                Message::AuthenticationMd5Password(body) => {
                    let hash = md5_hash(user.as_bytes(), &password, body.salt());
                    frontend::password_message(hash.as_bytes(), &mut out)?;
                }
                Message::AuthenticationSasl(_) => {
                    // gs2 flag "n" (RFC 5802): no channel binding, also over TLS. Raw clients
                    // test protocol handling, not authentication.
                    let s = ScramSha256::new(&password, ChannelBinding::unsupported());
                    frontend::sasl_initial_response("SCRAM-SHA-256", s.message(), &mut out)?;
                    scram = Some(s);
                }
                Message::AuthenticationSaslContinue(body) => {
                    let s = scram.as_mut().context("SCRAM continue before start")?;
                    s.update(body.data())?;
                    frontend::sasl_response(s.message(), &mut out)?;
                }
                Message::AuthenticationSaslFinal(body) => {
                    scram
                        .as_mut()
                        .context("SCRAM final before start")?
                        .finish(body.data())?;
                    continue;
                }
                Message::ErrorResponse(body) => return Err(ServerError::from(&body).into()),
                _ => bail!("unexpected message during startup"),
            }
            conn.send(&out).await?;
        }
    }

    async fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.stream.write_all(bytes).await?;
        Ok(())
    }

    async fn next(&mut self, deadline: Instant) -> Result<Next> {
        loop {
            check_header(&self.buf)?;
            if let Some(message) = Message::parse(&mut self.buf)? {
                return Ok(Next::Message(message));
            }
            match tokio::time::timeout_at(deadline, self.stream.read_buf(&mut self.buf)).await {
                Err(_) => return Ok(Next::Timeout),
                Ok(Ok(0)) => return Ok(Next::Closed(None)),
                Ok(Err(e)) => return Ok(Next::Closed(Some(e))),
                Ok(Ok(_)) => {}
            }
        }
    }

    /// Send the messages and record the replies, one line each. Reads until one
    /// ReadyForQuery per Sync/Query/CopyDone. Without any of those (e.g. only Flush)
    /// it reads until the server has been quiet for `opts.quiet`. Ends with TIMEOUT or
    /// CLOSED if the replies stop early.
    pub async fn exchange(&mut self, messages: &[RawMessage], opts: &ExchangeOpts) -> Result<Vec<String>> {
        let mut out = BytesMut::new();
        for m in messages {
            m.write(&mut out)?;
        }
        self.send(&out).await?;

        let mut cycles = messages.iter().filter(|m| m.ends_cycle()).count();
        let wait_for_quiet = cycles == 0;
        let deadline = Instant::now() + opts.step;
        let mut lines = Vec::new();
        while wait_for_quiet || cycles > 0 {
            let until = if wait_for_quiet {
                (Instant::now() + opts.quiet).min(deadline)
            } else {
                deadline
            };
            let message = match self.next(until).await? {
                Next::Message(m) => m,
                // Quiet for long enough: everything that was coming has arrived.
                Next::Timeout if wait_for_quiet && Instant::now() < deadline => break,
                Next::Timeout => {
                    lines.push("TIMEOUT".to_string());
                    break;
                }
                Next::Closed(_) => {
                    lines.push("CLOSED".to_string());
                    break;
                }
            };
            // CopyInResponse pauses a COPY until the client sends its data in a later
            // step, which then waits for the ReadyForQuery.
            if let Message::ReadyForQuery(_) | Message::CopyInResponse(_) = message {
                cycles = cycles.saturating_sub(1);
            }
            if let Some(line) = summary(&message, opts.parameter_status)? {
                lines.push(line);
            }
        }
        Ok(lines)
    }

    /// Sends Terminate and closes the connection. With `wait_for_exit` (on Postgres),
    /// then reads until the server closes its end, which happens only after the
    /// backend's session state is gone (see `DrainOnShutdown` in pg.rs), and fails if
    /// that takes longer.
    pub async fn terminate(mut self, wait_for_exit: Option<Duration>) -> Result<()> {
        let mut out = BytesMut::new();
        frontend::terminate(&mut out);
        let _ = self.send(&out).await;
        let _ = self.stream.shutdown().await;
        let Some(limit) = wait_for_exit else {
            return Ok(());
        };
        let deadline = Instant::now() + limit;
        loop {
            match self.next(deadline).await {
                Ok(Next::Timeout) => bail!("the backend did not exit within {limit:?} of the disconnect"),
                // Close_notify or EOF (over TLS whichever comes first), or a broken
                // connection: either way the server is done with it.
                Ok(Next::Closed(_)) | Err(_) => return Ok(()),
                Ok(Next::Message(_)) => {}
            }
        }
    }
}

/// An ErrorResponse's SQLSTATE and message; as an error, the server refusing a connection
/// during startup.
#[derive(Debug)]
pub struct ServerError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "startup failed: {} {}", self.code, self.message)
    }
}

impl std::error::Error for ServerError {}

impl From<&postgres_protocol::message::backend::ErrorResponseBody> for ServerError {
    fn from(body: &postgres_protocol::message::backend::ErrorResponseBody) -> Self {
        let mut fields = body.fields();
        let (mut code, mut message) = (String::new(), String::new());
        while let Ok(Some(field)) = fields.next() {
            match field.type_() {
                b'C' => code = String::from_utf8_lossy(field.value_bytes()).into_owned(),
                b'M' => message = String::from_utf8_lossy(field.value_bytes()).into_owned(),
                _ => {}
            }
        }
        ServerError { code, message }
    }
}

/// One line per reply. Notices are skipped, and ParameterStatus unless asked for:
/// poolers add, drop and reorder those, and they rarely change what the client gets.
fn summary(message: &Message, parameter_status: bool) -> Result<Option<String>> {
    let line = match message {
        Message::ParseComplete => "ParseComplete".to_string(),
        Message::BindComplete => "BindComplete".to_string(),
        Message::CloseComplete => "CloseComplete".to_string(),
        Message::NoData => "NoData".to_string(),
        Message::PortalSuspended => "PortalSuspended".to_string(),
        Message::EmptyQueryResponse => "EmptyQueryResponse".to_string(),
        Message::CopyInResponse(_) => "CopyInResponse".to_string(),
        Message::CopyOutResponse(_) => "CopyOutResponse".to_string(),
        Message::CopyData(body) => match std::str::from_utf8(body.data()) {
            // Escaped so a row's trailing newline doesn't split the line.
            Ok(text) => format!("CopyData {}", text.escape_debug()),
            // Binary COPY: lossy text would hide differences.
            Err(_) => {
                let hex: String = body.data().iter().map(|b| format!("{b:02x}")).collect();
                format!("CopyData hex:{hex}")
            }
        },
        Message::CopyDone => "CopyDone".to_string(),
        Message::CommandComplete(body) => format!("CommandComplete {}", body.tag()?),
        Message::ParameterDescription(body) => format!("ParameterDescription {}", body.parameters().count()?),
        Message::RowDescription(body) => {
            let names: Vec<String> = body.fields().map(|f| Ok(f.name().to_string())).collect()?;
            format!("RowDescription {}", names.join(","))
        }
        Message::DataRow(body) => {
            let buffer = body.buffer();
            let values: Vec<String> = body
                .ranges()
                .map(|r| Ok(r.map_or("NULL".to_string(), |r| String::from_utf8_lossy(&buffer[r]).into_owned())))
                .collect()?;
            format!("DataRow {}", values.join("|"))
        }
        // The code is stable across servers; the wording is not.
        Message::ErrorResponse(body) => {
            format!("Error {}", ServerError::from(body).code)
        }
        Message::ReadyForQuery(body) => format!("ReadyForQuery {}", body.status() as char),
        Message::ParameterStatus(body) if parameter_status => {
            format!("ParameterStatus {}={}", body.name()?, body.value()?)
        }
        _ => return Ok(None),
    };
    Ok(Some(line))
}

#[cfg(fuzzing)]
pub(super) fn fuzz_replies(data: &[u8]) {
    let mut buf = BytesMut::from(data);
    while check_header(&buf).is_ok() {
        match Message::parse(&mut buf) {
            Ok(Some(message)) => {
                let _ = summary(&message, true);
            }
            _ => return,
        }
    }
}

#[cfg(fuzzing)]
pub(super) fn fuzz_send_messages(text: &str) {
    let mut buf = BytesMut::new();
    for line in text.lines() {
        if let Ok(message) = line.parse::<RawMessage>() {
            let _ = message.write(&mut buf);
            let _ = message.ends_cycle();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend message as the server would send it.
    fn backend(tag: u8, body: &[u8]) -> Message {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[tag]);
        buf.extend_from_slice(&(body.len() as u32 + 4).to_be_bytes());
        buf.extend_from_slice(body);
        Message::parse(&mut buf).unwrap().unwrap()
    }

    #[test]
    fn caps_message_size() {
        let header = |len: u32| [&b"D"[..], &len.to_be_bytes()].concat();
        assert!(check_header(b"").is_ok());
        assert!(check_header(&header(4)[..3]).is_ok());
        assert!(check_header(&header(4)).is_ok());
        assert!(check_header(&header(MAX_MESSAGE as u32)).is_ok());
        assert!(check_header(&header(MAX_MESSAGE as u32 + 1)).is_err());
        assert!(check_header(&header(u32::MAX)).is_err());
        assert!(check_header(&header(3)).is_err());
    }

    #[test]
    fn records_copy_out() {
        let line = |m: Message| summary(&m, false).unwrap().unwrap();
        assert_eq!(line(backend(b'H', b"\0\0\x02\0\0\0\0")), "CopyOutResponse");
        assert_eq!(line(backend(b'd', b"1\ta\n")), "CopyData 1\\ta\\n");
        assert_eq!(
            line(backend(b'd', b"PGCOPY\n\xff\r\n\0")),
            "CopyData hex:5047434f50590aff0d0a00"
        );
        assert_eq!(line(backend(b'c', b"")), "CopyDone");
    }

    #[test]
    fn parameter_status_only_when_asked() {
        let status = || backend(b'S', b"application_name\0app\0");
        assert_eq!(summary(&status(), false).unwrap(), None);
        assert_eq!(
            summary(&status(), true).unwrap().as_deref(),
            Some("ParameterStatus application_name=app")
        );
        assert_eq!(
            summary(&backend(b'Z', b"I"), true).unwrap().as_deref(),
            Some("ReadyForQuery I")
        );
    }

    #[test]
    fn parses_messages() {
        let parse = |s: &str| s.parse::<RawMessage>().unwrap();
        assert_eq!(
            parse("parse s1 SELECT 1, 2"),
            RawMessage::Parse {
                name: "s1".into(),
                sql: "SELECT 1, 2".into()
            }
        );
        assert_eq!(
            parse("parse - SELECT 1"),
            RawMessage::Parse {
                name: "".into(),
                sql: "SELECT 1".into()
            }
        );
        assert_eq!(
            parse("bind - s1"),
            RawMessage::Bind {
                portal: "".into(),
                statement: "s1".into()
            }
        );
        assert_eq!(
            parse("execute - 5"),
            RawMessage::Execute {
                portal: "".into(),
                max_rows: 5
            }
        );
        assert_eq!(
            parse("describe statement s1"),
            RawMessage::Describe {
                portal: false,
                name: "s1".into()
            }
        );
        assert_eq!(
            parse("close portal p"),
            RawMessage::Close {
                portal: true,
                name: "p".into()
            }
        );
        assert_eq!(parse("query SELECT 1"), RawMessage::Query("SELECT 1".into()));
        assert_eq!(parse("sync"), RawMessage::Sync);
        assert!("bind s1".parse::<RawMessage>().is_err());
        assert!("describe thing x".parse::<RawMessage>().is_err());
        assert!("frobnicate".parse::<RawMessage>().is_err());
    }
}
