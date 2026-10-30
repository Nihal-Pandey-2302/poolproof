//! Helpers shared by the integration tests.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

/// A Postgres stand-in that accepts any login, answers `SELECT 1`, and fails every
/// other query with an error quoting it, as Postgres does for many errors. Returns
/// its port.
pub fn echoing_server() -> u16 {
    fn message(tag: u8, body: &[u8]) -> Vec<u8> {
        [&[tag][..], &(body.len() as u32 + 4).to_be_bytes(), body].concat()
    }
    fn serve(mut s: TcpStream) -> std::io::Result<()> {
        let read_body = |s: &mut TcpStream, len: usize| -> std::io::Result<Vec<u8>> {
            let mut body = vec![0; len - 4];
            s.read_exact(&mut body)?;
            Ok(body)
        };
        loop {
            let mut len = [0; 4];
            s.read_exact(&mut len)?;
            let body = read_body(&mut s, u32::from_be_bytes(len) as usize)?;
            // SSLRequest: no TLS here; the startup message follows.
            if body == [0x04, 0xd2, 0x16, 0x2f] {
                s.write_all(b"N")?;
                continue;
            }
            break;
        }
        let ready = message(b'Z', b"I");
        s.write_all(&[message(b'R', &[0, 0, 0, 0]), message(b'K', &[0; 8]), ready.clone()].concat())?;
        loop {
            let mut head = [0; 5];
            s.read_exact(&mut head)?;
            let body = read_body(&mut s, u32::from_be_bytes(head[1..].try_into().unwrap()) as usize)?;
            match head[0] {
                b'X' => return Ok(()),
                b'Q' => {
                    let sql = String::from_utf8_lossy(&body[..body.len() - 1]).into_owned();
                    let reply = if sql == "SELECT 1" {
                        [
                            message(
                                b'T',
                                b"\0\x01?column?\0\0\0\0\0\0\0\0\0\0\x17\0\x04\xff\xff\xff\xff\0\0",
                            ),
                            message(b'D', b"\0\x01\0\0\0\x011"),
                            message(b'C', b"SELECT 1\0"),
                        ]
                        .concat()
                    } else {
                        let fields = format!("SERROR\0C42601\0Msyntax error in {sql}\0\0");
                        message(b'E', fields.as_bytes())
                    };
                    s.write_all(&[reply, ready.clone()].concat())?;
                }
                _ => s.write_all(&[message(b'E', b"SERROR\0C0A000\0Mnot supported\0\0"), ready.clone()].concat())?,
            }
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
    port
}
