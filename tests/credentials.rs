//! Passwords must never reach output: not in any report format, not in warnings or
//! errors. Each target here carries a password through a different route, at a closed
//! port so every scenario ends in a connection error that could echo it.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output};

const INLINE: &str = "canary-inline-7f3a";
const FROM_VAR: &str = "canary-var-91bc";
const FROM_PGPASSWORD: &str = "canary-pgpassword-4d2e";
/// Would point the URL at another host if inserted unencoded.
const TRICKY: &str = "canary-tricky@127.0.0.1:2/";
const CANARIES: [&str; 4] = [INLINE, FROM_VAR, FROM_PGPASSWORD, "canary-tricky"];

fn targets_file(name: &str, text: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn poolproof(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poolproof"))
        .args(args)
        .env("PP_TEST_PASSWORD", FROM_VAR)
        .env("PP_TEST_TRICKY", TRICKY)
        .env("PGPASSWORD", FROM_PGPASSWORD)
        .env("PGPASSFILE", "/nonexistent")
        .output()
        .unwrap()
}

fn assert_no_canary(what: &str, out: &Output) {
    let all = [out.stdout.as_slice(), out.stderr.as_slice()].concat();
    let all = String::from_utf8_lossy(&all);
    for canary in CANARIES {
        assert!(!all.contains(canary), "{what} printed a password:\n{all}");
    }
}

#[test]
fn no_output_contains_a_password() {
    let targets = targets_file(
        "credentials-targets.toml",
        &format!(
            r#"
[[target]]
name = "inline"
pooler = "postgres://u:{INLINE}@127.0.0.1:1/app"
direct = "postgres://u:{INLINE}@127.0.0.1:1/app"
mode = "transaction"

[[target]]
name = "variable"
pooler = "postgres://u:${{PP_TEST_PASSWORD}}@127.0.0.1:1/app"
direct = "host=127.0.0.1 port=1 user=u password='${{PP_TEST_PASSWORD}}' dbname=app"
mode = "transaction"

[[target]]
name = "tricky"
pooler = "postgres://u:${{PP_TEST_TRICKY}}@127.0.0.1:1/app"
direct = "postgres://u:${{PP_TEST_TRICKY}}@127.0.0.1:1/app"
mode = "transaction"

[[target]]
name = "pgpassword"
pooler = "postgres://u@127.0.0.1:1/app"
direct = "postgres://u@127.0.0.1:1/app"
mode = "transaction"
"#
        ),
    );
    let targets = targets.to_str().unwrap();
    for format in ["text", "json", "markdown", "html"] {
        let out = poolproof(&["run", "--targets", targets, "--format", format, "--verbose"]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{format}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_no_canary(format, &out);
    }

    let url = format!("postgres://u:{INLINE}@127.0.0.1:1/app");
    let out = poolproof(&["run", "--pooler", &url, "--direct", &url, "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_no_canary("run --pooler", &out);
}

#[test]
fn bad_targets_are_reported_without_the_password() {
    let unset = targets_file(
        "credentials-unset.toml",
        &format!(
            "[[target]]\nname = 't'\nmode = 'transaction'\npooler = 'postgres://u:{INLINE}@h:${{PP_UNSET_VAR}}/app'\ndirect = 'postgres://u:{INLINE}@h/app'\n"
        ),
    );
    let out = poolproof(&["run", "--targets", unset.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("PP_UNSET_VAR is not set"));
    assert_no_canary("unset variable", &out);

    let invalid = targets_file(
        "credentials-invalid.toml",
        &format!(
            "[[target]]\nname = 't'\nmode = 'transaction'\npooler = 'postgres://u:{INLINE}@h:notaport/app'\ndirect = 'postgres://u@h/app'\n"
        ),
    );
    let out = poolproof(&["run", "--targets", invalid.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert_no_canary("invalid URL", &out);
}

/// A Postgres stand-in that accepts any login, answers `SELECT 1`, and fails every
/// other query with an error quoting it, as Postgres does for many errors. Returns
/// its port.
fn echoing_server() -> u16 {
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

#[test]
fn failing_reset_does_not_show_substituted_values() {
    let port = echoing_server();
    let targets = targets_file(
        "credentials-reset.toml",
        &format!(
            r#"
[[target]]
name = "reset"
pooler = "postgres://u:{INLINE}@127.0.0.1:{port}/app"
direct = "postgres://u:{INLINE}@127.0.0.1:{port}/app"
mode = "transaction"
reset = ["ALTER ROLE app PASSWORD '${{PP_TEST_PASSWORD}}'"]
"#
        ),
    );
    let targets = targets.to_str().unwrap();
    for format in ["text", "json", "markdown", "html"] {
        let out = poolproof(&["run", "--targets", targets, "--format", format, "--verbose"]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{format}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_no_canary(format, &out);
        // The reset failed, and the error names it as written.
        if format == "json" {
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("PASSWORD '${PP_TEST_PASSWORD}'"), "{stdout}");
            assert!(stdout.contains("42601 (details hidden"), "{stdout}");
        }
    }
}
