//! The `shared-server` fault: a tiny transaction pooler with one server connection
//! and no reset between clients. Every client gets that connection's BackendKeyData,
//! so a cancel from one client can stop another client's query.

use crate::wire::{self, CANCEL_REQUEST, GSSENC_REQUEST, Message, Reader, SSL_REQUEST};
use bytes::{Bytes, BytesMut};
use postgres_protocol::authentication::md5_hash;
use postgres_protocol::authentication::sasl::{ChannelBinding, ScramSha256};
use postgres_protocol::message::backend;
use postgres_protocol::message::frontend;
use std::io;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Login {
    pub upstream: String,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
}

struct Server {
    reader: Reader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

pub struct Pool {
    login: Login,
    server: Mutex<Option<Server>>,
    /// ParameterStatus and BackendKeyData of the current server connection, sent to
    /// each client at startup without waiting for the server.
    hello: std::sync::Mutex<Bytes>,
}

fn other(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

/// Number of ReadyForQuery messages a client message asks for.
fn cycles(m: &Message) -> usize {
    usize::from(matches!(m.tag, b'S' | b'Q'))
}

impl Pool {
    pub fn new(login: Login) -> Self {
        Pool {
            login,
            server: Mutex::new(None),
            hello: std::sync::Mutex::new(Bytes::new()),
        }
    }

    pub async fn serve(&self, client: TcpStream) -> io::Result<()> {
        let (cr, mut cw) = client.into_split();
        let mut cr = Reader::new(cr);
        loop {
            match cr.startup().await? {
                None => return Ok(()),
                Some((SSL_REQUEST | GSSENC_REQUEST, _)) => cw.write_all(b"N").await?,
                Some((CANCEL_REQUEST, raw)) => {
                    let mut s = wire::connect(&self.login.upstream).await?;
                    return s.write_all(&raw).await;
                }
                Some(_) => break,
            }
        }

        let hello = self.hello().await?;
        let mut out = BytesMut::new();
        out.extend_from_slice(&Message::new(b'R', &0i32.to_be_bytes()).raw);
        out.extend_from_slice(&hello);
        out.extend_from_slice(&Message::new(b'Z', b"I").raw);
        cw.write_all(&out).await?;

        let mut held = None;
        let mut outstanding = 0usize;
        loop {
            let Some(guard) = held.as_mut() else {
                let Some(m) = cr.next().await? else {
                    return Ok(());
                };
                if m.tag == b'X' {
                    return Ok(());
                }
                let mut guard = self.server.lock().await;
                self.ensure(&mut guard).await?;
                let server = guard.as_mut().ok_or_else(|| other("no server"))?;
                outstanding += cycles(&m);
                server.writer.write_all(&m.raw).await?;
                held = Some(guard);
                continue;
            };
            let Some(server) = guard.as_mut() else {
                return Err(other("server gone"));
            };
            let mut release = false;
            tokio::select! {
                m = cr.next() => match m {
                    Ok(Some(m)) if m.tag != b'X' => {
                        outstanding += cycles(&m);
                        server.writer.write_all(&m.raw).await?;
                    }
                    // The client left while holding the server: let its queries
                    // finish before anyone else gets the connection.
                    _ => {
                        if drain(server, outstanding).await.is_err() {
                            **guard = None;
                        }
                        return Ok(());
                    }
                },
                m = server.reader.next() => match m {
                    Ok(Some(m)) => {
                        cw.write_all(&m.raw).await?;
                        if m.tag == b'Z' {
                            outstanding = outstanding.saturating_sub(1);
                            release = outstanding == 0 && m.body() == b"I";
                        }
                    }
                    _ => {
                        **guard = None;
                        return Err(other("server connection closed"));
                    }
                },
            }
            if release {
                held = None;
            }
        }
    }

    /// The startup messages for a new client. If the server connection is free, check
    /// it first, so the client gets the BackendKeyData of the connection it will use.
    /// If another client holds it, it's in use and the cached hello is current.
    async fn hello(&self) -> io::Result<Bytes> {
        let cached = self.hello.lock().map_err(|_| other("poisoned"))?.clone();
        let mut guard = match self.server.try_lock() {
            Ok(guard) => guard,
            Err(_) if !cached.is_empty() => return Ok(cached),
            Err(_) => self.server.lock().await,
        };
        self.ensure(&mut guard).await?;
        Ok(self.hello.lock().map_err(|_| other("poisoned"))?.clone())
    }

    /// Connect if there's no server connection, it has closed, or it sent a FATAL
    /// error while idle (e.g. 57P01 after reset terminated the backend). Any other
    /// leftovers (notifications, or replies to a client that left mid-request) stay
    /// buffered and go to the next client, as in a pooler that doesn't clean up.
    async fn ensure(&self, slot: &mut Option<Server>) -> io::Result<()> {
        if let Some(s) = slot.as_mut()
            && s.reader.poll_alive()
            && !s
                .reader
                .buffered()
                .iter()
                .any(|&(tag, body)| tag == b'E' && wire::is_fatal(body))
        {
            return Ok(());
        }
        *slot = None;
        let (server, hello) = connect(&self.login).await?;
        *self.hello.lock().map_err(|_| other("poisoned"))? = hello;
        *slot = Some(server);
        Ok(())
    }
}

async fn drain(server: &mut Server, mut outstanding: usize) -> io::Result<()> {
    while outstanding > 0 {
        match server.reader.next().await? {
            Some(m) if m.tag == b'Z' => outstanding -= 1,
            Some(_) => {}
            None => return Err(other("server connection closed")),
        }
    }
    Ok(())
}

async fn connect(login: &Login) -> io::Result<(Server, Bytes)> {
    let stream = wire::connect(&login.upstream).await?;
    let (r, mut writer) = stream.into_split();
    let mut reader = Reader::new(r);
    let database = login.database.as_deref().unwrap_or(&login.user);
    let mut out = BytesMut::new();
    frontend::startup_message([("user", login.user.as_str()), ("database", database)], &mut out)?;
    writer.write_all(&out).await?;

    let password = login.password.as_bytes();
    let mut scram: Option<ScramSha256> = None;
    let mut hello = BytesMut::new();
    loop {
        let m = reader
            .next()
            .await?
            .ok_or_else(|| other("server closed during startup"))?;
        match m.tag {
            b'S' | b'K' => {
                hello.extend_from_slice(&m.raw);
                continue;
            }
            b'Z' => return Ok((Server { reader, writer }, hello.freeze())),
            b'N' => continue,
            b'E' => return Err(other("server rejected startup")),
            b'R' => {}
            _ => return Err(other("unexpected message during startup")),
        }
        let mut buf = BytesMut::from(&m.raw[..]);
        let parsed = backend::Message::parse(&mut buf)?.ok_or_else(|| other("short message"))?;
        let mut out = BytesMut::new();
        match parsed {
            backend::Message::AuthenticationOk => continue,
            backend::Message::AuthenticationCleartextPassword => frontend::password_message(password, &mut out)?,
            backend::Message::AuthenticationMd5Password(body) => {
                let hash = md5_hash(login.user.as_bytes(), password, body.salt());
                frontend::password_message(hash.as_bytes(), &mut out)?;
            }
            backend::Message::AuthenticationSasl(_) => {
                let s = ScramSha256::new(password, ChannelBinding::unsupported());
                frontend::sasl_initial_response("SCRAM-SHA-256", s.message(), &mut out)?;
                scram = Some(s);
            }
            backend::Message::AuthenticationSaslContinue(body) => {
                let s = scram.as_mut().ok_or_else(|| other("SCRAM continue before start"))?;
                s.update(body.data())?;
                frontend::sasl_response(s.message(), &mut out)?;
            }
            backend::Message::AuthenticationSaslFinal(body) => {
                scram
                    .as_mut()
                    .ok_or_else(|| other("SCRAM final before start"))?
                    .finish(body.data())?;
                continue;
            }
            _ => return Err(other("unsupported authentication")),
        }
        writer.write_all(&out).await?;
    }
}
