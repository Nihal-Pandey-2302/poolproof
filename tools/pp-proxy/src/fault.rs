//! The faults, as filters on single messages. Each one models a bug class real poolers
//! have had, and each one should make specific scenarios fail (selftest/expected.toml).

use crate::wire::{Message, tag_text};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Copy bytes, one server connection per client. Must pass every scenario.
    None,
    /// Accept, then close at once.
    Refuse,
    /// Drop client messages sent after a Sync until that Sync's ReadyForQuery.
    DropPipelined,
    /// Drop ErrorResponse from the server once the session is up.
    SwallowError,
    /// Drop the client's CopyDone.
    NoCopyDone,
    /// Report one row more in every INSERT command tag.
    WrongTag,
    /// One server connection for all clients, held from a client's first message to
    /// ReadyForQuery idle, never reset.
    SharedServer,
}

impl Fault {
    pub fn parse(s: &str) -> Option<Fault> {
        Some(match s {
            "none" => Fault::None,
            "refuse" => Fault::Refuse,
            "drop-pipelined" => Fault::DropPipelined,
            "swallow-error" => Fault::SwallowError,
            "no-copydone" => Fault::NoCopyDone,
            "wrong-tag" => Fault::WrongTag,
            "shared-server" => Fault::SharedServer,
            _ => return None,
        })
    }
}

/// State one client connection's two directions share.
#[derive(Default)]
pub struct Session {
    /// A Sync went to the server and its ReadyForQuery hasn't come back.
    sync_pending: AtomicBool,
    /// The first ReadyForQuery has been seen: startup is over.
    ready: AtomicBool,
}

impl Session {
    /// Whether a client message goes to the server.
    pub fn client_to_server(&self, fault: Fault, m: &Message) -> bool {
        match fault {
            Fault::DropPipelined => {
                if self.sync_pending.load(Ordering::SeqCst) {
                    return false;
                }
                if m.tag == b'S' {
                    self.sync_pending.store(true, Ordering::SeqCst);
                }
                true
            }
            Fault::NoCopyDone => m.tag != b'c',
            _ => true,
        }
    }

    /// What reaches the client for a server message, if anything.
    pub fn server_to_client(&self, fault: Fault, m: Message) -> Option<Message> {
        if m.tag == b'Z' {
            // Before the client can see it, so its next message isn't dropped.
            self.sync_pending.store(false, Ordering::SeqCst);
            self.ready.store(true, Ordering::SeqCst);
            return Some(m);
        }
        match fault {
            Fault::SwallowError if m.tag == b'E' && self.ready.load(Ordering::SeqCst) => None,
            Fault::WrongTag if m.tag == b'C' => Some(wrong_tag(m)),
            _ => Some(m),
        }
    }
}

fn wrong_tag(m: Message) -> Message {
    let tag = tag_text(m.body());
    match tag.strip_prefix("INSERT 0 ").and_then(|n| n.parse::<u64>().ok()) {
        Some(n) => Message::new(b'C', format!("INSERT 0 {}\0", n + 1).as_bytes()),
        None => m,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(tag: u8, body: &[u8]) -> Message {
        Message::new(tag, body)
    }

    #[test]
    fn drop_pipelined_drops_until_ready() {
        let s = Session::default();
        let f = Fault::DropPipelined;
        assert!(s.client_to_server(f, &msg(b'P', b"")));
        assert!(s.client_to_server(f, &msg(b'S', b"")));
        assert!(!s.client_to_server(f, &msg(b'P', b"")));
        assert!(!s.client_to_server(f, &msg(b'S', b"")));
        assert!(s.server_to_client(f, msg(b'Z', b"I")).is_some());
        assert!(s.client_to_server(f, &msg(b'Q', b"")));
    }

    #[test]
    fn swallow_error_keeps_startup_errors() {
        let s = Session::default();
        let f = Fault::SwallowError;
        assert!(s.server_to_client(f, msg(b'E', b"")).is_some());
        s.server_to_client(f, msg(b'Z', b"I"));
        assert!(s.server_to_client(f, msg(b'E', b"")).is_none());
        assert!(s.server_to_client(f, msg(b'C', b"SELECT 1\0")).is_some());
    }

    #[test]
    fn no_copydone_drops_only_copydone() {
        let s = Session::default();
        assert!(!s.client_to_server(Fault::NoCopyDone, &msg(b'c', b"")));
        assert!(s.client_to_server(Fault::NoCopyDone, &msg(b'd', b"1\n")));
    }

    #[test]
    fn wrong_tag_changes_insert_only() {
        let s = Session::default();
        let out = s.server_to_client(Fault::WrongTag, msg(b'C', b"INSERT 0 1\0")).unwrap();
        assert_eq!(tag_text(out.body()), "INSERT 0 2");
        let out = s.server_to_client(Fault::WrongTag, msg(b'C', b"SELECT 1\0")).unwrap();
        assert_eq!(tag_text(out.body()), "SELECT 1");
    }

    #[test]
    fn parses_every_fault_name() {
        for name in [
            "none",
            "refuse",
            "drop-pipelined",
            "swallow-error",
            "no-copydone",
            "wrong-tag",
            "shared-server",
        ] {
            assert!(Fault::parse(name).is_some(), "{name}");
        }
        assert!(Fault::parse("nope").is_none());
    }
}
