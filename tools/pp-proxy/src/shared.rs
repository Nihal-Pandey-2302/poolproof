//! The pooled faults: a tiny transaction pooler with a fixed number of server
//! connections and no reset between clients.
//!
//! - `shared-server`: one connection for every client.
//! - `pool-of-two`: two connections, handed out last in, first out, so a client that
//!   comes back after another one took its connection lands on the other one.
//!
//! A client holds a connection from its first message until ReadyForQuery idle with
//! nothing outstanding (so through a transaction, an aborted one and COPY). The second
//! connection opens only when a checkout finds the first one held, inside that checkout.
//! At startup a client gets the BackendKeyData of the connection it would get then:
//! the most recently released free one, or, when all are held, the last one opened.
//! So a cancel from one client can stop another client's query.

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
use tokio::sync::{Mutex, MutexGuard, Semaphore, SemaphorePermit};

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
    /// This connection's ParameterStatus and BackendKeyData, for a client's startup.
    hello: Bytes,
}

/// Which slot a checkout gets. Free slots form a stack: the most recently released one
/// goes out first. A slot that was never handed out is used only when none is free.
struct Lifo {
    free: Vec<usize>,
    opened: usize,
    size: usize,
}

impl Lifo {
    fn new(size: usize) -> Self {
        Lifo {
            free: Vec::new(),
            opened: 0,
            size,
        }
    }

    fn checkout(&mut self) -> Option<usize> {
        self.free.pop().or_else(|| {
            (self.opened < self.size).then(|| {
                self.opened += 1;
                self.opened - 1
            })
        })
    }

    fn release(&mut self, slot: usize) {
        self.free.push(slot);
    }
}

pub struct Pool {
    login: Login,
    slots: Vec<Mutex<Option<Server>>>,
    lifo: std::sync::Mutex<Lifo>,
    /// One permit per slot: a checkout with none free waits here.
    permits: Semaphore,
    /// The hello of the connection opened last, for a client that connects while every
    /// slot is held.
    hello: std::sync::Mutex<Bytes>,
}

/// A checked-out slot. Dropping it puts the slot back on top of the free stack.
struct Held<'a> {
    pool: &'a Pool,
    slot: usize,
    guard: MutexGuard<'a, Option<Server>>,
    _permit: SemaphorePermit<'a>,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        // Before the permit goes, so whoever gets the permit finds the slot.
        if let Ok(mut lifo) = self.pool.lifo.lock() {
            lifo.release(self.slot);
        }
    }
}

fn other(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

/// Number of ReadyForQuery messages a client message asks for.
fn cycles(m: &Message) -> usize {
    usize::from(matches!(m.tag, b'S' | b'Q'))
}

impl Pool {
    pub fn new(login: Login, size: usize) -> Self {
        Pool {
            login,
            slots: (0..size).map(|_| Mutex::new(None)).collect(),
            lifo: std::sync::Mutex::new(Lifo::new(size)),
            permits: Semaphore::new(size),
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

        let mut held: Option<Held> = None;
        let mut outstanding = 0usize;
        loop {
            let Some(h) = held.as_mut() else {
                let Some(m) = cr.next().await? else {
                    return Ok(());
                };
                if m.tag == b'X' {
                    return Ok(());
                }
                let mut h = self.checkout().await?;
                let server = h.guard.as_mut().ok_or_else(|| other("no server"))?;
                outstanding += cycles(&m);
                server.writer.write_all(&m.raw).await?;
                held = Some(h);
                continue;
            };
            let Some(server) = h.guard.as_mut() else {
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
                            *h.guard = None;
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
                        *h.guard = None;
                        return Err(other("server connection closed"));
                    }
                },
            }
            if release {
                held = None;
            }
        }
    }

    /// The most recently released free slot, connected; waits while all are held.
    async fn checkout(&self) -> io::Result<Held<'_>> {
        let permit = self.permits.acquire().await.map_err(|_| other("pool closed"))?;
        self.take(permit).await
    }

    /// Like `checkout`, but None at once when every slot is held.
    async fn try_checkout(&self) -> io::Result<Option<Held<'_>>> {
        match self.permits.try_acquire() {
            Ok(permit) => self.take(permit).await.map(Some),
            Err(_) => Ok(None),
        }
    }

    async fn take<'a>(&'a self, permit: SemaphorePermit<'a>) -> io::Result<Held<'a>> {
        let slot = self
            .lifo
            .lock()
            .map_err(|_| other("poisoned"))?
            .checkout()
            .ok_or_else(|| other("a permit but no free slot"))?;
        let guard = self.slots[slot].lock().await;
        let mut held = Held {
            pool: self,
            slot,
            guard,
            _permit: permit,
        };
        // Connecting here, not in the background, so the client that needs a second
        // connection waits for it and nothing else can race it.
        self.ensure(&mut held.guard).await?;
        Ok(held)
    }

    /// The startup messages for a new client: the hello of the slot it would get now
    /// (checked first, so it is current), or the last one opened when all are held.
    async fn hello(&self) -> io::Result<Bytes> {
        if let Some(held) = self.try_checkout().await? {
            return held
                .guard
                .as_ref()
                .map(|s| s.hello.clone())
                .ok_or_else(|| other("no server"));
        }
        let cached = self.hello.lock().map_err(|_| other("poisoned"))?.clone();
        if !cached.is_empty() {
            return Ok(cached);
        }
        let held = self.checkout().await?;
        held.guard
            .as_ref()
            .map(|s| s.hello.clone())
            .ok_or_else(|| other("no server"))
    }

    /// Connect if the slot has no server connection, it has closed, or it sent a FATAL
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
        let server = connect(&self.login).await?;
        *self.hello.lock().map_err(|_| other("poisoned"))? = server.hello.clone();
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

async fn connect(login: &Login) -> io::Result<Server> {
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
            b'Z' => {
                return Ok(Server {
                    reader,
                    writer,
                    hello: hello.freeze(),
                });
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::task::AbortHandle;

    #[test]
    fn lifo_hands_out_the_last_released_slot_and_opens_slots_in_order() {
        let mut lifo = Lifo::new(2);
        assert_eq!(lifo.checkout(), Some(0));
        // Slot 0 is held: the next checkout opens slot 1.
        assert_eq!(lifo.checkout(), Some(1));
        assert_eq!(lifo.checkout(), None);
        lifo.release(0);
        lifo.release(1);
        assert_eq!(lifo.checkout(), Some(1));
        assert_eq!(lifo.checkout(), Some(0));
        lifo.release(1);
        assert_eq!(lifo.checkout(), Some(1));

        // A pool of one never has a second slot.
        let mut one = Lifo::new(1);
        assert_eq!(one.checkout(), Some(0));
        assert_eq!(one.checkout(), None);
        one.release(0);
        assert_eq!(one.checkout(), Some(0));
    }

    /// A stand-in for Postgres. Connection n (from 1) sends BackendKeyData with pid n
    /// and answers every query with the command tag `pid n`; BEGIN opens a transaction
    /// and COMMIT ends it, as ReadyForQuery reports.
    struct Upstream {
        addr: String,
        connections: Arc<AtomicU32>,
        tasks: Arc<std::sync::Mutex<Vec<AbortHandle>>>,
    }

    impl Upstream {
        async fn start() -> Upstream {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            let connections = Arc::new(AtomicU32::new(0));
            let tasks = Arc::new(std::sync::Mutex::new(Vec::new()));
            let (count, list) = (connections.clone(), tasks.clone());
            tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let pid = count.fetch_add(1, Ordering::SeqCst) + 1;
                    let task = tokio::spawn(backend(stream, pid));
                    list.lock().unwrap().push(task.abort_handle());
                }
            });
            Upstream {
                addr,
                connections,
                tasks,
            }
        }

        /// Close every server connection, like a reset that terminates the backends.
        async fn kill_all(&self) {
            for t in self.tasks.lock().unwrap().drain(..) {
                t.abort();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        fn connections(&self) -> u32 {
            self.connections.load(Ordering::SeqCst)
        }
    }

    async fn backend(stream: TcpStream, pid: u32) -> io::Result<()> {
        let (r, mut w) = stream.into_split();
        let mut r = Reader::new(r);
        r.startup().await?;
        let mut key = pid.to_be_bytes().to_vec();
        key.extend(0u32.to_be_bytes());
        let mut out = BytesMut::new();
        out.extend_from_slice(&Message::new(b'R', &0i32.to_be_bytes()).raw);
        out.extend_from_slice(&Message::new(b'K', &key).raw);
        out.extend_from_slice(&Message::new(b'Z', b"I").raw);
        w.write_all(&out).await?;
        let mut status = b'I';
        while let Some(m) = r.next().await? {
            let sql = String::from_utf8_lossy(m.body()).trim_end_matches('\0').to_string();
            status = match sql.as_str() {
                "BEGIN" => b'T',
                "COMMIT" => b'I',
                _ => status,
            };
            let mut out = BytesMut::new();
            out.extend_from_slice(&Message::new(b'C', format!("pid {pid}\0").as_bytes()).raw);
            out.extend_from_slice(&Message::new(b'Z', &[status]).raw);
            w.write_all(&out).await?;
        }
        Ok(())
    }

    /// A proxy with a pool of `size` in front of `upstream`; returns its address.
    async fn proxy(upstream: &Upstream, size: usize) -> String {
        let login = Login {
            upstream: upstream.addr.clone(),
            user: "u".into(),
            password: String::new(),
            database: None,
        };
        let pool = Arc::new(Pool::new(login, size));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            loop {
                let (client, _) = listener.accept().await.unwrap();
                let pool = pool.clone();
                tokio::spawn(async move { pool.serve(client).await });
            }
        });
        addr
    }

    /// A client of the proxy, and the pid in the BackendKeyData it got at startup.
    struct Client {
        reader: Reader<OwnedReadHalf>,
        writer: OwnedWriteHalf,
        key_pid: u32,
    }

    impl Client {
        async fn connect(proxy: &str) -> Client {
            let (r, mut writer) = TcpStream::connect(proxy).await.unwrap().into_split();
            let mut out = BytesMut::new();
            frontend::startup_message([("user", "u")], &mut out).unwrap();
            writer.write_all(&out).await.unwrap();
            let mut reader = Reader::new(r);
            let mut key_pid = 0;
            loop {
                let m = reader.next().await.unwrap().unwrap();
                match m.tag {
                    b'K' => key_pid = u32::from_be_bytes(m.body()[..4].try_into().unwrap()),
                    b'Z' => break,
                    _ => {}
                }
            }
            Client {
                reader,
                writer,
                key_pid,
            }
        }

        /// Runs a query; returns the pid of the server connection that answered.
        async fn query(&mut self, sql: &str) -> u32 {
            let mut out = BytesMut::new();
            frontend::query(sql, &mut out).unwrap();
            self.writer.write_all(&out).await.unwrap();
            let mut pid = 0;
            loop {
                let m = self.reader.next().await.unwrap().unwrap();
                match m.tag {
                    b'C' => {
                        let tag = String::from_utf8_lossy(m.body()).trim_end_matches('\0').to_string();
                        pid = tag.strip_prefix("pid ").unwrap().parse().unwrap();
                    }
                    b'Z' => return pid,
                    _ => {}
                }
            }
        }
    }

    #[tokio::test]
    async fn the_second_slot_opens_inside_the_checkout_and_goes_out_lifo() {
        let up = Upstream::start().await;
        let proxy = proxy(&up, 2).await;
        let mut a = Client::connect(&proxy).await;
        let mut b = Client::connect(&proxy).await;
        assert_eq!(up.connections(), 1, "the second slot opens only when it's needed");
        assert_eq!(a.query("BEGIN").await, 1);
        // Slot 0 is held by a's transaction: b's checkout opens slot 1 and waits for it.
        assert_eq!(b.query("SELECT 1").await, 2);
        assert_eq!(up.connections(), 2);
        assert_eq!(a.query("SELECT 1").await, 1, "a keeps its slot inside the transaction");
        assert_eq!(a.query("COMMIT").await, 1);
        // Released last: slot 0 goes out first, then slot 1.
        assert_eq!(b.query("SELECT 1").await, 1);
        assert_eq!(a.query("BEGIN").await, 1);
        assert_eq!(b.query("SELECT 1").await, 2);
        assert_eq!(up.connections(), 2);
    }

    #[tokio::test]
    async fn a_pool_of_one_never_opens_a_second_connection() {
        let up = Upstream::start().await;
        let proxy = proxy(&up, 1).await;
        let mut a = Client::connect(&proxy).await;
        let mut b = Client::connect(&proxy).await;
        assert_eq!(a.query("BEGIN").await, 1);
        let waiting = tokio::spawn(async move { b.query("SELECT 1").await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!waiting.is_finished(), "b waits for a's connection");
        assert_eq!(a.query("COMMIT").await, 1);
        assert_eq!(waiting.await.unwrap(), 1);
        assert_eq!(up.connections(), 1);
    }

    #[tokio::test]
    async fn a_client_gets_the_key_of_the_top_free_slot_else_the_last_opened() {
        let up = Upstream::start().await;
        let proxy = proxy(&up, 2).await;
        let mut a = Client::connect(&proxy).await;
        assert_eq!(a.key_pid, 1);
        assert_eq!(a.query("BEGIN").await, 1);
        // Slot 0 is held, so the slot b would get is slot 1, opened for its key.
        let mut b = Client::connect(&proxy).await;
        assert_eq!(b.key_pid, 2);
        assert_eq!(b.query("BEGIN").await, 2);
        // Both held: the hello of the connection opened last.
        let c = Client::connect(&proxy).await;
        assert_eq!(c.key_pid, 2);
        assert_eq!(a.query("COMMIT").await, 1);
        assert_eq!(b.query("COMMIT").await, 2);
        // Slot 1 was released last, so a new client's key, and its first query, are slot 1's.
        let mut d = Client::connect(&proxy).await;
        assert_eq!(d.key_pid, 2);
        assert_eq!(d.query("SELECT 1").await, 2);
        drop(c);
    }

    #[tokio::test]
    async fn dead_slots_reconnect_at_checkout_in_lifo_order() {
        let up = Upstream::start().await;
        let proxy = proxy(&up, 2).await;
        let mut a = Client::connect(&proxy).await;
        let mut b = Client::connect(&proxy).await;
        assert_eq!(a.query("BEGIN").await, 1);
        assert_eq!(b.query("SELECT 1").await, 2);
        assert_eq!(a.query("COMMIT").await, 1);
        // Free stack, top first: slot 0, slot 1. A reset kills both connections.
        up.kill_all().await;
        assert_eq!(a.query("BEGIN").await, 3, "slot 0 reconnects first");
        assert_eq!(b.query("SELECT 1").await, 4, "then slot 1, while slot 0 is held");
        assert_eq!(a.query("COMMIT").await, 3);
        assert_eq!(up.connections(), 4);
    }
}
