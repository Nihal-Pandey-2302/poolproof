//! pp-proxy: self-test targets for poolproof.
//!
//! PROXY_LISTEN="6501=none,6502=drop-pipelined,..." opens one listener per fault, all
//! forwarding to PROXY_UPSTREAM (host:port, default postgres:5432). `shared-server`
//! logs in to Postgres itself as PROXY_USER / PROXY_PASSWORD / PROXY_DATABASE.
//! PROXY_BIND picks the listen address (default 0.0.0.0, for a container).

mod fault;
mod shared;
mod wire;

use fault::{Fault, Session};
use std::io;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use wire::{CANCEL_REQUEST, GSSENC_REQUEST, Reader, SSL_REQUEST};

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn parse_listen(spec: &str) -> Result<Vec<(u16, Fault)>, String> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (port, fault) = item
                .split_once('=')
                .ok_or(format!("expected PORT=FAULT, got {item:?}"))?;
            let port = port.parse().map_err(|_| format!("bad port in {item:?}"))?;
            let fault = Fault::parse(fault).ok_or(format!("unknown fault in {item:?}"))?;
            Ok((port, fault))
        })
        .collect()
}

#[tokio::main]
async fn main() {
    let listen = match parse_listen(&env("PROXY_LISTEN", "6432=none")) {
        Ok(l) if !l.is_empty() => l,
        Ok(_) => fail("PROXY_LISTEN is empty"),
        Err(e) => fail(&e),
    };
    let upstream = env("PROXY_UPSTREAM", "postgres:5432");
    let login = shared::Login {
        upstream: upstream.clone(),
        user: env("PROXY_USER", "postgres"),
        password: env("PROXY_PASSWORD", ""),
        database: std::env::var("PROXY_DATABASE").ok(),
    };

    let bind = env("PROXY_BIND", "0.0.0.0");
    let mut tasks = Vec::new();
    for (port, fault) in listen {
        let listener = match TcpListener::bind((bind.as_str(), port)).await {
            Ok(l) => l,
            Err(e) => fail(&format!("listening on {port}: {e}")),
        };
        eprintln!("pp-proxy: :{port} {fault:?} -> {upstream}");
        let upstream = upstream.clone();
        let pool = (fault == Fault::SharedServer).then(|| Arc::new(shared::Pool::new(login.clone())));
        tasks.push(tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    continue;
                };
                let _ = client.set_nodelay(true);
                let upstream = upstream.clone();
                let pool = pool.clone();
                tokio::spawn(async move {
                    let result = match pool {
                        Some(pool) => pool.serve(client).await,
                        None => per_client(client, &upstream, fault).await,
                    };
                    if let Err(e) = result {
                        eprintln!("pp-proxy: :{port}: {e}");
                    }
                });
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("pp-proxy: {msg}");
    std::process::exit(2);
}

/// One server connection per client, with the fault applied to each message.
async fn per_client(client: TcpStream, upstream: &str, fault: Fault) -> io::Result<()> {
    if fault == Fault::Refuse {
        return Ok(());
    }
    let mut server = TcpStream::connect(upstream).await?;
    server.set_nodelay(true)?;
    if fault == Fault::None {
        let mut client = client;
        tokio::io::copy_bidirectional(&mut client, &mut server).await?;
        return Ok(());
    }

    let (cr, mut cw) = client.into_split();
    let (sr, mut sw) = server.into_split();
    let mut cr = Reader::new(cr);
    loop {
        match cr.startup().await? {
            None => return Ok(()),
            Some((SSL_REQUEST | GSSENC_REQUEST, _)) => cw.write_all(b"N").await?,
            Some((CANCEL_REQUEST, raw)) => return sw.write_all(&raw).await,
            Some((_, raw)) => {
                sw.write_all(&raw).await?;
                break;
            }
        }
    }

    let session = Session::default();
    let up = async {
        while let Some(m) = cr.next().await? {
            if session.client_to_server(fault, &m) {
                sw.write_all(&m.raw).await?;
            }
        }
        Ok::<_, io::Error>(())
    };
    let down = async {
        let mut sr = Reader::new(sr);
        while let Some(m) = sr.next().await? {
            if let Some(m) = session.server_to_client(fault, m) {
                cw.write_all(&m.raw).await?;
            }
        }
        Ok(())
    };
    // Either side closing ends the session; dropping the halves closes the other.
    tokio::select! {
        r = up => r,
        r = down => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_spec() {
        let l = parse_listen("6501=none, 6502=wrong-tag,").unwrap();
        assert_eq!(l, vec![(6501, Fault::None), (6502, Fault::WrongTag)]);
        assert!(parse_listen("6501").is_err());
        assert!(parse_listen("x=none").is_err());
        assert!(parse_listen("6501=nope").is_err());
    }
}
