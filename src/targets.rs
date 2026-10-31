//! The targets file: which poolers to test and the Postgres behind each.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio_postgres::Config;
use tokio_postgres::config::Host;

use crate::runner::tls::{RootCerts, SslMode, Tls, default_root};
use crate::scenario::PoolMode;

pub struct Targets {
    pub targets: Vec<Target>,
}

pub struct Target {
    pub name: String,
    /// Where clients connect through the pooler.
    pub pooler: Endpoint,
    /// The Postgres server behind it.
    pub direct: Endpoint,
    pub mode: PoolMode,
    /// What is being tested, for grouping in reports: e.g. `pgbouncer`, `1.26.1`, and a
    /// config label like `default` or `strict`.
    pub product: Option<String>,
    pub version: Option<String>,
    pub config: Option<String>,
    /// Statements run on `direct` before every run to drop state leaked on the
    /// pooler's server connections, usually by terminating them. Without it,
    /// one scenario's leak can change the next one's result.
    pub reset: Vec<Statement>,
}

/// A SQL statement to run on Postgres. Errors show it as written, with `${NAME}`
/// unexpanded, so a value substituted from the environment never reaches output.
#[derive(Clone)]
pub struct Statement {
    sql: String,
    written: String,
}

impl Statement {
    /// A statement without substitutions, e.g. a scenario's setup.
    pub fn plain(sql: &str) -> Self {
        Statement {
            sql: sql.to_string(),
            written: sql.to_string(),
        }
    }

    pub fn plain_all(sqls: &[String]) -> Vec<Self> {
        sqls.iter().map(|sql| Self::plain(sql)).collect()
    }

    /// A statement as if `written` had been expanded to `sql`.
    #[cfg(test)]
    pub(crate) fn substituted(sql: &str, written: &str) -> Self {
        Statement {
            sql: sql.to_string(),
            written: written.to_string(),
        }
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// Whether anything was substituted. A server error can quote the statement's
    /// text, so then only its SQLSTATE should be shown.
    pub fn has_substitutions(&self) -> bool {
        self.sql != self.written
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.written)
    }
}

impl fmt::Debug for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// A target as written in the file, before `${VAR}` substitution and URL parsing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetsFile {
    #[serde(rename = "target")]
    targets: Vec<TargetFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetFile {
    name: String,
    pooler: String,
    direct: String,
    mode: PoolMode,
    #[serde(default)]
    product: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    config: Option<String>,
    #[serde(default)]
    reset: Vec<String>,
}

impl Targets {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let file: TargetsFile = toml::from_str(&text).context("parsing targets")?;
        let env = |name: &str| std::env::var(name).ok();
        let targets = file
            .targets
            .into_iter()
            .map(|t| {
                let name = t.name.clone();
                Target::from_file(t, &env).with_context(|| format!("target '{name}'"))
            })
            .collect::<Result<_>>()?;
        Ok(Targets { targets })
    }

    /// The one target given with `run --pooler URL --direct URL`.
    pub fn single(pooler: &str, direct: &str, mode: PoolMode) -> Result<Self> {
        Ok(Targets {
            targets: vec![Target {
                name: "pooler".into(),
                pooler: Endpoint::parse(pooler).context("--pooler")?,
                direct: Endpoint::parse(direct).context("--direct")?,
                mode,
                product: None,
                version: None,
                config: None,
                reset: vec![],
            }],
        })
    }
}

impl Target {
    fn from_file(t: TargetFile, env: &dyn Fn(&str) -> Option<String>) -> Result<Self> {
        let endpoint = |field: &str, url: &str| -> Result<Endpoint> {
            // A value is data: encoded so it can't end the field it's in, e.g. a
            // password with `@` turning the rest into the host.
            let is_url = url.starts_with("postgres://") || url.starts_with("postgresql://");
            let quote = if is_url { percent_encode } else { backslash_escape };
            // An empty value would shift what follows into this field (`password= dbname=x`
            // makes "dbname=x" the password) or drop it. Only a URL's password may be
            // empty: that just means no password.
            let may_be_empty = |before: &str, after: &str| is_url && in_url_password(before, after);
            let url = expand_vars(url, env, quote, &may_be_empty).with_context(|| field.to_string())?;
            Endpoint::resolve(&url, env).with_context(|| field.to_string())
        };
        Ok(Target {
            pooler: endpoint("pooler", &t.pooler)?,
            direct: endpoint("direct", &t.direct)?,
            reset: t
                .reset
                .iter()
                .map(|sql| {
                    Ok(Statement {
                        sql: expand_vars(sql, env, |v| v.to_string(), &|_, _| true).context("reset")?,
                        written: sql.clone(),
                    })
                })
                .collect::<Result<_>>()?,
            name: t.name,
            mode: t.mode,
            product: t.product,
            version: t.version,
            config: t.config,
        })
    }
}

/// Whether a value between `before` and `after` is a URL's password: after
/// `scheme://user:` and before the `@`.
fn in_url_password(before: &str, after: &str) -> bool {
    // An `@` must follow in the same authority; without one, `host:` precedes a port.
    let rest_of_authority = after.split(['/', '?', '#']).next().unwrap_or_default();
    rest_of_authority.contains('@')
        && before
            .split_once("://")
            .is_some_and(|(_, authority)| authority.contains(':') && !authority.contains(['@', '/', '?']))
}

/// Replaces `${NAME}` with the environment variable NAME, passed through `quote`, so
/// passwords can stay out of the file. `$${` stands for a literal `${`; any other `$` is
/// left alone. An empty value is an error unless `may_be_empty` accepts the text around it.
fn expand_vars(
    text: &str,
    env: &dyn Fn(&str) -> Option<String>,
    quote: fn(&str) -> String,
    may_be_empty: &dyn Fn(&str, &str) -> bool,
) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if let Some(after) = rest.strip_prefix("$${") {
            out.push_str("${");
            rest = after;
        } else if let Some(after) = rest.strip_prefix("${") {
            let Some(end) = after.find('}') else {
                bail!("unclosed `${{`");
            };
            let name = &after[..end];
            let valid = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            // Not echoed: the text between the braces may be part of a literal password.
            if !valid {
                bail!("invalid variable name in `${{...}}`");
            }
            let Some(value) = env(name) else {
                bail!("environment variable {name} is not set");
            };
            if value.is_empty() && !may_be_empty(&text[..text.len() - rest.len()], &after[end + 1..]) {
                bail!("environment variable {name} is empty");
            }
            out.push_str(&quote(&value));
            rest = &after[end + 1..];
        } else {
            out.push('$');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// For URLs: every byte but the unreserved ones (RFC 3986) as `%XX`.
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// For `key=value` strings: a backslash before `\`, `'` and whitespace, which works
/// both inside and outside quotes.
fn backslash_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c == '\\' || c == '\'' || c.is_whitespace() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A server to connect to: the parsed URL, with a password from PGPASSWORD or the
/// password file filled in when the URL has none, and its TLS settings. Displays
/// without the password.
#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint {
    config: Config,
    tls: Tls,
}

impl Endpoint {
    /// Parses a connection URL (or `key=value` string) and fills in the password the
    /// way libpq does: the URL's own, else PGPASSWORD, else PGPASSFILE or ~/.pgpass.
    pub fn parse(url: &str) -> Result<Self> {
        Self::resolve(url, &|name: &str| std::env::var(name).ok())
    }

    fn resolve(url: &str, env: &dyn Fn(&str) -> Option<String>) -> Result<Self> {
        let (url, sslmode, sslrootcert) = take_tls_params(url)?;
        // The parse error never includes the URL, so it can't leak a password.
        let mut config = Config::from_str(&url).context("invalid connection URL")?;
        // libpq's default is prefer: TLS when the server offers it, without checks.
        let roots = sslrootcert.or_else(|| env("PGSSLROOTCERT")).map(|r| match r.as_str() {
            "system" => RootCerts::System,
            _ => RootCerts::File(r.into()),
        });
        let mode = match sslmode.or_else(|| env("PGSSLMODE")) {
            Some(mode) => SslMode::parse(&mode)?,
            // libpq 16+: the system store implies verify-full.
            None if roots == Some(RootCerts::System) => SslMode::VerifyFull,
            None => SslMode::Prefer,
        };
        config.ssl_mode(mode.driver_mode());
        let tls = Tls::new(mode, roots, default_root(env))?;
        // An empty password counts as none, as in libpq.
        if config.get_password().is_none_or(<[u8]>::is_empty) {
            if let Some(password) = env("PGPASSWORD").filter(|p| !p.is_empty()) {
                config.password(password);
            } else if let Some(password) = password_from_file(&config, env) {
                config.password(password);
            }
        }
        Ok(Endpoint { config, tls })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn tls(&self) -> &Tls {
        &self.tls
    }
}

/// Takes `sslmode` and `sslrootcert` out of a connection string, from either form.
/// tokio-postgres doesn't accept verify-ca, verify-full or sslrootcert, so poolproof
/// handles them itself. Errors don't quote the string: it can hold a password.
fn take_tls_params(url: &str) -> Result<(String, Option<String>, Option<String>)> {
    let (mut sslmode, mut sslrootcert) = (None, None);
    let mut take = |key: &str, value: String| match key {
        "sslmode" => {
            sslmode = Some(value);
            true
        }
        "sslrootcert" => {
            sslrootcert = Some(value);
            true
        }
        _ => false,
    };
    let rest = if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        // A `?` before the `@` is part of the password, not the start of the query.
        let after_userinfo = url.find('@').map_or(0, |i| i + 1);
        let Some(at) = url[after_userinfo..].find('?').map(|i| after_userinfo + i) else {
            return Ok((url.to_string(), None, None));
        };
        let (base, query) = (&url[..at], &url[at + 1..]);
        let kept: Vec<&str> = query
            .split('&')
            .filter(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                !take(&percent_decode(key), percent_decode(value))
            })
            .collect();
        if kept.is_empty() {
            base.to_string()
        } else {
            format!("{base}?{}", kept.join("&"))
        }
    } else {
        let kept: Vec<String> = parse_key_values(url)?
            .into_iter()
            .filter(|(key, value)| !take(key, value.clone()))
            .map(|(key, value)| format!("{key}='{}'", value.replace('\\', "\\\\").replace('\'', "\\'")))
            .collect();
        kept.join(" ")
    };
    Ok((rest, sslmode, sslrootcert))
}

/// `%XX` sequences to bytes; anything else is kept as it is.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `key = value` pairs as libpq and tokio-postgres read them: values optionally in
/// single quotes, `\` escaping the next character.
fn parse_key_values(s: &str) -> Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    let mut chars = s.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.peek().is_none() {
            return Ok(pairs);
        }
        let mut key = String::new();
        while let Some(c) = chars.next_if(|c| *c != '=' && !c.is_whitespace()) {
            key.push(c);
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.next() != Some('=') {
            bail!("invalid connection string: expected `=` after `{key}`");
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let mut value = String::new();
        if chars.next_if_eq(&'\'').is_some() {
            loop {
                match chars.next() {
                    None => bail!("invalid connection string: unterminated quoted value"),
                    Some('\'') => break,
                    Some('\\') => value.extend(chars.next()),
                    Some(c) => value.push(c),
                }
            }
        } else {
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                match c {
                    '\\' => value.extend(chars.next()),
                    c => value.push(c),
                }
            }
        }
        pairs.push((key, value));
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&redact(&self.config))?;
        if self.tls.mode != SslMode::Prefer {
            write!(f, "?sslmode={}", self.tls.mode)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// A connection as `postgres://user@host:port/db`, rebuilt from its parts so the
/// password can't be in it. The only way an endpoint is shown.
pub fn redact(config: &Config) -> String {
    let user = config.get_user().unwrap_or_default();
    let ports = config.get_ports();
    let hosts: Vec<String> = config
        .get_hosts()
        .iter()
        .enumerate()
        .map(|(i, host)| {
            let host = match host {
                Host::Tcp(h) if h.contains(':') => format!("[{h}]"),
                Host::Tcp(h) => h.clone(),
                #[cfg(unix)]
                Host::Unix(path) => path.display().to_string(),
            };
            // One port applies to every host.
            match ports.get(i).or(ports.first()) {
                Some(port) => format!("{host}:{port}"),
                None => host,
            }
        })
        .collect();
    let db = config.get_dbname().unwrap_or(user);
    format!("postgres://{user}@{}/{db}", hosts.join(","))
}

/// Files already warned about, so a bad password file is reported once per run.
static WARNED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// The password file libpq would read: PGPASSFILE, else ~/.pgpass
/// (%APPDATA%\postgresql\pgpass.conf on Windows).
fn password_file(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(path) = env("PGPASSFILE") {
        return Some(path.into());
    }
    if cfg!(windows) {
        env("APPDATA").map(|dir| Path::new(&dir).join("postgresql").join("pgpass.conf"))
    } else {
        env("HOME").map(|dir| Path::new(&dir).join(".pgpass"))
    }
}

fn password_from_file(config: &Config, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let path = password_file(env)?;
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
            if !warned.contains(&path) {
                eprintln!(
                    "warning: password file {} has group or world access; permissions should be u=rw (0600) or less",
                    path.display()
                );
                warned.push(path);
            }
            return None;
        }
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let user = config.get_user()?;
    // Simpler than libpq in two ways: a Unix socket always matches as "localhost"
    // (libpq uses the socket path unless it's the default), and only the first host
    // is looked up, even when the URL lists several.
    let host = match config.get_hosts().first() {
        Some(Host::Tcp(h)) => h.as_str(),
        _ => "localhost",
    };
    let port = config.get_ports().first().copied().unwrap_or(5432).to_string();
    let db = config.get_dbname().unwrap_or(user);
    pgpass_lookup(&text, [host, &port, db, user])
}

/// The password from the first `host:port:db:user:password` line that matches, as
/// libpq reads it: a bare `*` matches anything, `\` escapes `:` and `\`, and the
/// password ends at the next unescaped `:`.
fn pgpass_lookup(text: &str, wanted: [&str; 4]) -> Option<String> {
    'lines: for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut chars = line.chars();
        for want in wanted {
            // `raw` keeps the escapes, so an escaped `\*` is no wildcard.
            let (mut field, mut raw) = (String::new(), String::new());
            loop {
                match chars.next() {
                    None => continue 'lines,
                    Some(':') => break,
                    Some('\\') => {
                        raw.push('\\');
                        if let Some(c) = chars.next() {
                            field.push(c);
                            raw.push(c);
                        }
                    }
                    Some(c) => {
                        field.push(c);
                        raw.push(c);
                    }
                }
            }
            if raw != "*" && field != want {
                continue 'lines;
            }
        }
        let mut password = String::new();
        while let Some(c) = chars.next() {
            match c {
                ':' => break,
                // A trailing lone backslash stays, as in libpq.
                '\\' => password.push(chars.next().unwrap_or('\\')),
                c => password.push(c),
            }
        }
        // An empty password counts as none.
        return Some(password).filter(|p| !p.is_empty());
    }
    None
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let vars: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |name| vars.get(name).cloned()
    }

    fn password(e: &Endpoint) -> Option<String> {
        e.config()
            .get_password()
            .map(|p| String::from_utf8_lossy(p).into_owned())
    }

    #[test]
    fn bundled_targets_parse_with_reset() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("targets.toml");
        let targets = Targets::load(&path).unwrap();
        assert!(targets.targets.iter().all(|t| !t.reset.is_empty()));
    }

    #[test]
    fn expands_variables() {
        let env = env_of(&[("PW", "s3cret"), ("_X1", "x")]);
        let raw = |s: &str| expand_vars(s, &env, |v| v.to_string(), &|_, _| false);
        assert_eq!(raw("postgres://u:${PW}@h/${_X1}").unwrap(), "postgres://u:s3cret@h/x");
        assert_eq!(raw("$${PW} $1 $$ a$").unwrap(), "${PW} $1 $$ a$");
        let err = |s: &str| raw(s).unwrap_err().to_string();
        assert_eq!(err("${MISSING}"), "environment variable MISSING is not set");
        assert_eq!(err("${PW"), "unclosed `${`");
        assert_eq!(err("${1X}"), "invalid variable name in `${...}`");
        assert!(!err("${pa ss}").contains("pa ss"));
    }

    #[test]
    fn substituted_values_stay_inside_their_field() {
        // Through the targets file, as a user would write it.
        let resolve = |template: &str, value: &str| {
            let env = env_of(&[("PW", value), ("PGPASSFILE", "/nonexistent")]);
            let file: TargetFile = toml::from_str(&format!(
                "name = 't'\nmode = 'transaction'\npooler = \"\"\"{template}\"\"\"\ndirect = \"\"\"{template}\"\"\"\n"
            ))
            .unwrap();
            Target::from_file(file, &env).map(|t| t.pooler)
        };
        let templates = [
            "postgres://u:${PW}@127.0.0.1:1/app",
            "host=127.0.0.1 port=1 user=u password=${PW} dbname=app",
            "host=127.0.0.1 port=1 user=u password='${PW}' dbname=app",
            "host=127.0.0.1 user=u password=${PW} port=1 dbname=app",
        ];
        for value in ["x@127.0.0.1:2/", "ab%41", "a b'c\\d:e/f?g#h&i=j"] {
            for template in templates {
                let e = resolve(template, value).unwrap();
                assert_eq!(password(&e).as_deref(), Some(value), "{template}");
                assert_eq!(e.to_string(), "postgres://u@127.0.0.1:1/app", "{template}");
            }
        }

        // Empty: fine as a URL password (none), an error anywhere else, since it would
        // shift the next field into this one.
        let e = resolve(templates[0], "").unwrap();
        assert_eq!(password(&e).unwrap_or_default(), "");
        assert_eq!(e.to_string(), "postgres://u@127.0.0.1:1/app");
        for template in &templates[1..] {
            let err = format!("{:#}", resolve(template, "").unwrap_err());
            assert!(err.ends_with("environment variable PW is empty"), "{template}: {err}");
        }
        for template in [
            "postgres://u:x@127.0.0.1:${PW}/app",
            "postgres://${PW}:x@127.0.0.1:1/app",
            "postgres://u@${PW}/app",
            "postgres://u@127.0.0.1:1/${PW}",
            "postgres://127.0.0.1:${PW}/app?user=u",
            "postgres://[::1]:${PW}/app?user=u",
            "postgres://u:x@127.0.0.1:${PW}?dbname=app",
        ] {
            assert!(resolve(template, "").is_err(), "{template}");
        }
    }

    #[test]
    fn password_precedence() {
        let pw = |url: &str, vars: &[(&str, &str)]| password(&Endpoint::resolve(url, &env_of(vars)).unwrap());
        let env = [("PGPASSWORD", "from-env"), ("PGPASSFILE", "/nonexistent")];
        assert_eq!(pw("postgres://u:inline@h/db", &env).as_deref(), Some("inline"));
        assert_eq!(pw("postgres://u@h/db", &env).as_deref(), Some("from-env"));
        assert_eq!(pw("postgres://u:@h/db", &env).as_deref(), Some("from-env"));
        let empty = [("PGPASSWORD", ""), ("PGPASSFILE", "/nonexistent")];
        assert_eq!(pw("postgres://u@h/db", &empty).as_deref(), None);
        assert_eq!(pw("postgres://u@h/db", &[("PGPASSFILE", "/nonexistent")]), None);
    }

    #[test]
    fn reads_password_file() {
        let dir = std::env::temp_dir().join(format!("poolproof-pgpass-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pgpass");
        std::fs::write(&path, "# comment\nother:*:*:*:no\nh:5433:db:u:from-file\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let env = env_of(&[("PGPASSFILE", path.to_str().unwrap())]);
        let pw = |url: &str| password(&Endpoint::resolve(url, &env).unwrap());
        assert_eq!(pw("postgres://u@h:5433/db").as_deref(), Some("from-file"));
        assert_eq!(pw("postgres://u@h:5432/db"), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(
                pw("postgres://u@h:5433/db"),
                None,
                "readable by others: ignored like libpq"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pgpass_lines() {
        let text = "*:*:*:u\\:x:a\\\\b:c\n\nh:*:*:*:p\\:q\n";
        // The password ends at the first unescaped `:`, as in libpq.
        assert_eq!(pgpass_lookup(text, ["h", "5432", "db", "u:x"]).as_deref(), Some("a\\b"));
        assert_eq!(pgpass_lookup(text, ["h", "5432", "db", "v"]).as_deref(), Some("p:q"));
        assert_eq!(pgpass_lookup(text, ["g", "5432", "db", "v"]), None);
        assert_eq!(pgpass_lookup("h:5432:db", ["h", "5432", "db", "u"]), None);
        // An escaped `*` is a literal star, not a wildcard.
        let star = "\\*:*:*:*:literal\n*:*:*:*:any\n";
        assert_eq!(pgpass_lookup(star, ["h", "5432", "db", "u"]).as_deref(), Some("any"));
        assert_eq!(
            pgpass_lookup(star, ["*", "5432", "db", "u"]).as_deref(),
            Some("literal")
        );
        // A trailing lone backslash stays; an empty password is none.
        assert_eq!(
            pgpass_lookup("*:*:*:*:ab\\", ["h", "1", "d", "u"]).as_deref(),
            Some("ab\\")
        );
        assert_eq!(pgpass_lookup("*:*:*:*:", ["h", "1", "d", "u"]), None);
    }

    #[test]
    fn takes_tls_parameters_out() {
        let (rest, mode, root) = take_tls_params(
            "postgres://u:p@h/db?sslmode=verify-full&application_name=x&sslrootcert=%2Ftmp%2Fca%20x.pem",
        )
        .unwrap();
        assert_eq!(rest, "postgres://u:p@h/db?application_name=x");
        assert_eq!(mode.as_deref(), Some("verify-full"));
        assert_eq!(root.as_deref(), Some("/tmp/ca x.pem"));
        let (rest, ..) = take_tls_params("postgresql://u@h/db?sslmode=require").unwrap();
        assert_eq!(rest, "postgresql://u@h/db");
        // A `?` in the password is not the query.
        let (rest, mode, _) = take_tls_params("postgres://u:p?w@h/db?sslmode=require").unwrap();
        assert_eq!(rest, "postgres://u:p?w@h/db");
        assert_eq!(mode.as_deref(), Some("require"));

        let (rest, mode, root) =
            take_tls_params("host=h sslmode = 'verify-ca' password='a b\\'c\\\\' sslrootcert=/x dbname=d").unwrap();
        assert_eq!(mode.as_deref(), Some("verify-ca"));
        assert_eq!(root.as_deref(), Some("/x"));
        let config = Config::from_str(&rest).unwrap();
        assert_eq!(config.get_password(), Some(&b"a b'c\\"[..]));
        assert_eq!(config.get_dbname(), Some("d"));
        assert!(take_tls_params("host=h password='open").is_err());
    }

    #[test]
    fn sslmode_from_url_then_environment_then_prefer() {
        let mode = |url: &str, vars: &[(&str, &str)]| Endpoint::resolve(url, &env_of(vars)).map(|e| e.tls().mode);
        assert_eq!(mode("postgres://u@h/db", &[]).unwrap(), SslMode::Prefer);
        assert_eq!(
            mode("postgres://u@h/db", &[("PGSSLMODE", "disable")]).unwrap(),
            SslMode::Disable
        );
        assert_eq!(
            mode("postgres://u@h/db?sslmode=require", &[("PGSSLMODE", "disable")]).unwrap(),
            SslMode::Require
        );
        assert!(mode("postgres://u@h/db?sslmode=allow", &[]).is_err());
        // verify-full needs a root certificate, from the URL, PGSSLROOTCERT or ~/.postgresql.
        assert!(mode("postgres://u@h/db?sslmode=verify-full", &[("HOME", "/nonexistent")]).is_err());
        let shown = Endpoint::resolve("postgres://u:pw@h/db?sslmode=require", &env_of(&[])).unwrap();
        assert!(shown.to_string().ends_with("/db?sslmode=require"), "{shown}");
        assert!(!shown.to_string().contains("pw"), "{shown}");
    }

    #[test]
    fn redacts_passwords() {
        let shown = |url: &str| Endpoint::resolve(url, &env_of(&[])).unwrap().to_string();
        assert_eq!(shown("postgres://u:s3cret@h:5433/db"), "postgres://u@h:5433/db");
        assert_eq!(shown("host=h user=u password='s3 cret' dbname=db"), "postgres://u@h/db");
        assert_eq!(
            shown("postgres://u:pw@[::1]:5432,h2:5433/db"),
            "postgres://u@[::1]:5432,h2:5433/db"
        );
        let e = Endpoint::resolve("postgres://u:s3cret@h/db", &env_of(&[])).unwrap();
        assert!(!format!("{e:?}").contains("s3cret"));
        let err = Endpoint::resolve("postgres://u:s3cret@h:notaport/db", &env_of(&[])).unwrap_err();
        assert!(!format!("{err:#}").contains("s3cret"), "{err:#}");
    }
}
