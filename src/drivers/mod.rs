//! `poolproof drivers`: real database drivers through each pooler. Each driver is a
//! small program in a Docker image (`drivers/<name>/`) that runs a case's steps on its
//! own two connections and prints each step's result. Every (driver, variant, case)
//! becomes a `Scenario` whose steps run that program, so reset, setup, the baseline and
//! the comparison are the same as in `run`.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio_postgres::config::Host;

use crate::runner::tls::SslMode;
use crate::runner::{Outcome, Timeouts};
use crate::scenario::{Compare, PoolMode, Scenario, Step};
use crate::targets::Endpoint;

const MANIFEST: &str = include_str!("../../drivers/drivers.toml");
const CASES: &str = include_str!("../../drivers/cases.toml");

/// Time on top of the steps' own timeouts for the container to start and stop. A JVM
/// takes a second or two; this is far above that.
const STARTUP_MARGIN: Duration = Duration::from_secs(30);

/// The most stderr kept for an error message.
const STDERR_TAIL: usize = 2000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    driver: Vec<Driver>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Driver {
    name: String,
    version: String,
    language: String,
    variant: Vec<Variant>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    name: String,
    settings: String,
    #[serde(default)]
    refs: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cases {
    case: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    description: String,
    #[serde(default)]
    refs: Vec<String>,
    #[serde(default)]
    setup: Vec<String>,
    #[serde(default)]
    teardown: Vec<String>,
    #[serde(default = "one")]
    rounds: u32,
    step: Vec<CaseStep>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseStep {
    client: String,
    sql: String,
    #[serde(default)]
    params: Vec<i64>,
    #[serde(default = "one")]
    times: u32,
}

fn one() -> u32 {
    1
}

/// How to run one case with one driver variant: the image, and what goes to the
/// program's stdin, one step per line (`client<TAB>sql<TAB>params`).
#[derive(Debug, Clone)]
pub struct Program {
    image: String,
    driver: String,
    version: String,
    variant: String,
    input: String,
    clients: usize,
    steps: usize,
}

/// The image a driver's program runs in.
pub fn image(driver: &str) -> String {
    format!("poolproof-driver-{driver}:dev")
}

/// The driver names, in manifest order.
pub fn names() -> Vec<String> {
    manifest()
        .map(|m| m.driver.into_iter().map(|d| d.name).collect())
        .unwrap_or_default()
}

fn manifest() -> Result<Manifest> {
    toml::from_str(MANIFEST).context("drivers/drivers.toml")
}

/// One scenario per driver, variant and case, for the drivers named in `only` (all
/// when empty). Named `driver/<driver>-<variant>/<case>`.
pub fn scenarios(only: &[String]) -> Result<Vec<Scenario>> {
    let manifest = manifest()?;
    let cases: Cases = toml::from_str(CASES).context("drivers/cases.toml")?;
    if let Some(unknown) = only.iter().find(|n| !manifest.driver.iter().any(|d| &d.name == *n)) {
        let known: Vec<&str> = manifest.driver.iter().map(|d| d.name.as_str()).collect();
        bail!("no driver named '{unknown}'; drivers: {}", known.join(", "));
    }
    for case in &cases.case {
        check_case(case).with_context(|| format!("drivers/cases.toml: case '{}'", case.name))?;
    }
    let mut out = Vec::new();
    for driver in manifest
        .driver
        .iter()
        .filter(|d| only.is_empty() || only.contains(&d.name))
    {
        for variant in &driver.variant {
            for case in &cases.case {
                out.push(scenario(driver, variant, case));
            }
        }
    }
    Ok(out)
}

fn check_case(case: &Case) -> Result<()> {
    if case.rounds == 0 || case.step.iter().any(|s| s.times == 0) {
        bail!("rounds and times must be at least 1");
    }
    // A program that stops early is marked on a step, so every case needs one.
    if case.step.is_empty() {
        bail!("a case needs at least one step");
    }
    for step in &case.step {
        if step.client.is_empty() || !step.client.chars().all(|c| c.is_ascii_lowercase()) {
            bail!("client '{}' must be lowercase letters", step.client);
        }
        // The programs read one step per line, split on tabs; psycopg uses %s and JDBC
        // uses ? as placeholders.
        if step.sql.contains(['\t', '\n', '\r', '%', '?']) {
            bail!("SQL can't contain tabs, newlines, % or ?: {}", step.sql);
        }
    }
    Ok(())
}

fn scenario(driver: &Driver, variant: &Variant, case: &Case) -> Scenario {
    let mut steps = Vec::new();
    let mut input = String::new();
    let mut clients: Vec<&str> = Vec::new();
    for _ in 0..case.rounds {
        for s in &case.step {
            for i in 1..=s.times {
                let params: Vec<String> = s.params.iter().map(i64::to_string).collect();
                input.push_str(&format!("{}\t{}\t{}\n", s.client, s.sql, params.join(",")));
                let mut label = s.sql.clone();
                if !params.is_empty() {
                    label.push_str(&format!(" with ({})", params.join(", ")));
                }
                if s.times > 1 {
                    label.push_str(&format!(" ({i} of {})", s.times));
                }
                steps.push(Step::labeled(&s.client, label, Compare::Full));
            }
            if !clients.contains(&s.client.as_str()) {
                clients.push(&s.client);
            }
        }
    }
    let mut refs = case.refs.clone();
    refs.extend(variant.refs.iter().cloned());
    Scenario {
        name: format!("driver/{}-{}/{}", driver.name, variant.name, case.name),
        description: format!(
            "{} {} ({}), {}: {}",
            driver.name, driver.version, driver.language, variant.settings, case.description
        ),
        refs,
        modes: vec![PoolMode::Session, PoolMode::Transaction, PoolMode::Statement],
        // Always 1. A broken program that fails with no differing step (see
        // `runner::check`) would make repeated runs look like they failed differently.
        repeat: 1,
        requires: vec![],
        timeout_ms: None,
        setup: case.setup.clone(),
        teardown: case.teardown.clone(),
        same_backend: vec![],
        same_backend_or_closed: vec![],
        program: Some(Program {
            image: image(&driver.name),
            driver: driver.name.clone(),
            version: driver.version.clone(),
            variant: variant.name.clone(),
            input,
            clients: clients.len(),
            steps: steps.len(),
        }),
        steps,
    }
}

/// Fails unless every image is there. `docker run` is told never to pull, so a missing
/// image would otherwise fail every scenario.
pub fn check_images(drivers: &[String]) -> Result<()> {
    let missing: Vec<String> = drivers
        .iter()
        .map(|d| image(d))
        .filter(|image| {
            !Command::new("docker")
                .args(["image", "inspect", "--format", "{{.Id}}", image])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
        .collect();
    if !missing.is_empty() {
        bail!(
            "missing driver images: {}. Build them with `docker buildx bake -f drivers/docker-bake.hcl --load`",
            missing.join(", ")
        );
    }
    Ok(())
}

/// The connection settings a program gets, as environment variables. Never as
/// arguments, which any local user sees in `ps`. The running container's environment
/// (PP_PASSWORD too) shows in `docker inspect`, but only to users with docker access,
/// which is root-equivalent anyway, and `--rm` removes the container when it ends.
fn connection_env(endpoint: &Endpoint) -> Result<Vec<(&'static str, String)>> {
    let config = endpoint.config();
    let host = match config.get_hosts().first() {
        Some(Host::Tcp(host)) => host.clone(),
        #[cfg(unix)]
        Some(Host::Unix(_)) => bail!("drivers connect over TCP only, not a Unix socket"),
        None => bail!("the URL has no host"),
    };
    let tls = endpoint.tls();
    let sslmode = match tls.mode {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
        SslMode::VerifyCa | SslMode::VerifyFull => {
            bail!("drivers support sslmode disable, prefer and require, not verify-ca or verify-full")
        }
    };
    if tls.roots.is_some() {
        bail!("drivers don't take sslrootcert: the CA file isn't inside the driver's container");
    }
    let user = config.get_user().context("the URL has no user")?.to_string();
    let password = match config.get_password() {
        Some(p) => String::from_utf8(p.to_vec()).context("the password isn't UTF-8")?,
        None => String::new(),
    };
    Ok(vec![
        ("PP_HOST", host),
        (
            "PP_PORT",
            config.get_ports().first().copied().unwrap_or(5432).to_string(),
        ),
        ("PP_DBNAME", config.get_dbname().unwrap_or(&user).to_string()),
        ("PP_USER", user),
        ("PP_PASSWORD", password),
        ("PP_SSLMODE", sslmode.to_string()),
    ])
}

/// Container names, unique per run of poolproof.
static RUNS: AtomicU32 = AtomicU32::new(0);

impl Program {
    /// Runs the program with both of its connections on `endpoint`: one outcome per step,
    /// and what went wrong with the program itself, if anything.
    pub(crate) async fn run(&self, endpoint: &Endpoint, t: &Timeouts) -> Result<(Vec<Outcome>, Option<Broken>)> {
        let env = connection_env(endpoint)?;
        let program = self.clone();
        let t = *t;
        tokio::task::spawn_blocking(move || program.run_blocking(env, &t))
            .await
            .context("the driver task panicked")?
    }

    /// Longest the whole program may take: every step and every connect at the step
    /// timeout, plus the startup margin. Past it the container is removed and the
    /// scenario is ERROR, never PASS.
    fn deadline(&self, t: &Timeouts) -> Duration {
        let waits = u32::try_from(self.steps + self.clients).unwrap_or(u32::MAX);
        t.step.saturating_mul(waits) + STARTUP_MARGIN
    }

    fn run_blocking(&self, env: Vec<(&'static str, String)>, t: &Timeouts) -> Result<(Vec<Outcome>, Option<Broken>)> {
        let name = format!(
            "poolproof-driver-{}-{}",
            std::process::id(),
            RUNS.fetch_add(1, Ordering::Relaxed)
        );
        let step_ms = t.step.as_millis().to_string();
        let mut cmd = Command::new("docker");
        cmd.args([
            "run",
            "--rm",
            "-i",
            "--pull",
            "never",
            "--network",
            "host",
            "--name",
            &name,
        ]);
        // `-e NAME` without a value passes the variable from docker's own environment.
        for (key, value) in &env {
            cmd.args(["-e", key]).env(key, value);
        }
        cmd.args(["-e", "PP_VARIANT", "-e", "PP_STEP_TIMEOUT_MS", &self.image])
            .env("PP_VARIANT", &self.variant)
            .env("PP_STEP_TIMEOUT_MS", &step_ms)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().context("starting docker")?;
        child
            .stdin
            .take()
            .context("no stdin")?
            .write_all(self.input.as_bytes())
            .context("writing the steps to the driver program")?;

        let (tx, rx) = mpsc::channel();
        let stdout = child.stdout.take().context("no stdout")?;
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut stderr = child.stderr.take().context("no stderr")?;
        let stderr = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });

        let deadline = Instant::now() + self.deadline(t);
        let mut lines = Vec::new();
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(line) => lines.push(line),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Killing the docker client would leave the container running.
                    let _ = Command::new("docker")
                        .args(["rm", "-fv", &name])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!(
                        "the {} program didn't finish within {:?}; it printed {}",
                        self.driver,
                        self.deadline(t),
                        printed(&lines)
                    );
                }
            }
        }
        let status = child.wait().context("waiting for docker")?;
        let stderr = stderr.join().unwrap_or_default();
        let (outcomes, mut broken) = self.parse(&lines, status.success()).with_context(|| {
            format!(
                "the {} program exited with {status} after printing {}: {}",
                self.driver,
                printed(&lines),
                tail(&stderr)
            )
        })?;
        if let Some(b) = &mut broken {
            if !status.success() {
                b.what.push_str(&format!(" ({status})"));
            }
            eprintln!("warning: the {} program {}: {}", self.driver, b.what, tail(&stderr));
        }
        Ok((outcomes, broken))
    }

    /// The program's output: a header with the driver and version it loaded, one line
    /// per step, then a line saying it finished and how many connections it opened.
    ///
    /// Output that can't be read, or that comes from another driver version, is the
    /// harness's problem: an error, so the scenario is ERROR. A program that stops early,
    /// exits with an error or opens more connections than it has clients was broken by
    /// what it talked to: every step it reported keeps its outcome, every step it didn't
    /// is `DRIVER:Stopped`, and the second value says what happened, for the comparison.
    fn parse(&self, lines: &[String], exited_ok: bool) -> Result<(Vec<Outcome>, Option<Broken>)> {
        let mut lines = lines
            .iter()
            .map(|l| serde_json::from_str::<Line>(l).with_context(|| format!("bad line: {l}")));
        match lines.next().transpose()? {
            Some(Line::Header(Header { driver, version })) => {
                if driver != self.driver || version != self.version {
                    bail!(
                        "the image runs {driver} {version}, but drivers/drivers.toml says {} {}; rebuild the image",
                        self.driver,
                        self.version
                    );
                }
            }
            _ => bail!("the {} program printed no header", self.driver),
        }
        let mut outcomes = Vec::new();
        let mut connections = None;
        for line in lines {
            match line? {
                Line::Step(step) => {
                    if connections.is_some() {
                        bail!("step {} came after the end", step.step);
                    }
                    if step.step != outcomes.len() + 1 || step.step > self.steps {
                        bail!(
                            "step {} came after step {} of {}",
                            step.step,
                            outcomes.len(),
                            self.steps
                        );
                    }
                    outcomes.push(step.outcome()?);
                }
                Line::Done(Done { connections: n }) => {
                    if connections.replace(n).is_some() {
                        bail!("a second end line");
                    }
                }
                Line::Header(_) => bail!("a second header"),
            }
        }
        let reported = outcomes.len();
        if exited_ok && connections.is_some() && reported == self.steps {
            return Ok(match connections {
                Some(n) if n < self.clients => bail!("the program opened {n} connections for {} clients", self.clients),
                // A driver that reconnected behind the program's back could hide a dropped
                // connection. Where it happened is unknown, so no step is marked.
                Some(n) if n > self.clients => (
                    outcomes,
                    Some(Broken {
                        code: RECONNECTED,
                        what: format!("opened {n} connections for {} clients", self.clients),
                    }),
                ),
                _ => (outcomes, None),
            });
        }
        outcomes.resize(self.steps, driver_error(STOPPED));
        let broken = Broken {
            code: STOPPED,
            what: format!("stopped after {reported} of {} steps", self.steps),
        };
        Ok((outcomes, Some(broken)))
    }
}

/// What went wrong with a driver program that its step outcomes don't show. The
/// comparison turns it into a FAIL when the steps alone would pass (see
/// `runner::check`), and on Postgres it makes the scenario ERROR.
#[derive(Debug, Clone, PartialEq)]
pub struct Broken {
    /// `DRIVER:Stopped` or `DRIVER:Reconnected`, for the FAIL's pooler outcome.
    pub code: &'static str,
    /// E.g. `stopped after 3 of 16 steps (exit status: 1)`.
    pub what: String,
}

/// A program that stopped before it reported every step and said it was done.
const STOPPED: &str = "DRIVER:Stopped";
/// A program that opened more connections than it has clients.
const RECONNECTED: &str = "DRIVER:Reconnected";

pub(crate) fn driver_error(code: &str) -> Outcome {
    Outcome::Error {
        code: code.to_string(),
        message: String::new(),
        connect: false,
    }
}

/// Each kind refuses fields it doesn't know, so a line can only be read one way.
#[derive(Deserialize)]
#[serde(untagged)]
enum Line {
    Header(Header),
    Done(Done),
    Step(StepLine),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    driver: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Done {
    connections: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepLine {
    step: usize,
    rows: Option<Vec<Vec<Option<String>>>>,
    /// A SQLSTATE, a pseudo-code (`TIMEOUT`, `CLOSED`, `CONNECT`), or `DRIVER:<class>`
    /// for a driver error without one. Never the message: wording isn't compared.
    error: Option<String>,
    /// The error came from opening the connection.
    #[serde(default)]
    connect: bool,
}

impl StepLine {
    fn outcome(self) -> Result<Outcome> {
        Ok(match (self.rows, self.error) {
            (Some(rows), None) => Outcome::Ok { rows, tags: vec![] },
            (None, Some(code)) => Outcome::Error {
                code,
                message: String::new(),
                connect: self.connect,
            },
            _ => bail!("step {} needs exactly one of rows and error", self.step),
        })
    }
}

/// The steps a program printed, for an error message.
fn printed(lines: &[String]) -> String {
    match lines.len() {
        0 => "nothing".to_string(),
        _ => format!("{} lines, the last: {}", lines.len(), lines[lines.len() - 1]),
    }
}

fn tail(text: &str) -> &str {
    let text = text.trim_end();
    let mut start = text.len().saturating_sub(STDERR_TAIL);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(steps: usize) -> Program {
        Program {
            image: image("psycopg"),
            driver: "psycopg".into(),
            version: "3.3.6".into(),
            variant: "default".into(),
            input: String::new(),
            clients: 2,
            steps,
        }
    }

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn every_driver_variant_and_case_is_a_scenario() {
        let all = scenarios(&[]).unwrap();
        let manifest = manifest().unwrap();
        let cases: Cases = toml::from_str(CASES).unwrap();
        let variants: usize = manifest.driver.iter().map(|d| d.variant.len()).sum();
        assert_eq!(all.len(), variants * cases.case.len());
        let mut names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len(), "scenario names must be unique");
        assert!(all.iter().all(|s| s.repeat == 1), "driver cases run once");
        // No driver step is a precondition, so every step is compared as it says.
        for s in &all {
            let own: Vec<Compare> = s.steps.iter().map(|step| step.compare).collect();
            assert_eq!(s.compare_modes(), own, "{}", s.name);
            assert!(s.steps.iter().all(|step| !step.precondition));
        }
        // Scenario metadata carries the driver version, the target name doesn't.
        let s = all
            .iter()
            .find(|s| s.name == "driver/psycopg-default/auto-prepare")
            .unwrap();
        assert!(s.description.starts_with("psycopg 3.3.6 (Python)"), "{}", s.description);
        let program = s.program.as_ref().unwrap();
        assert_eq!((program.clients, program.steps, s.steps.len()), (2, 16, 16));
        assert_eq!(
            program.input.lines().next(),
            Some("a\tSELECT ($1::int * 2)::text AS doubled\t21")
        );
        assert_eq!(
            s.steps[2].describe(),
            "[a] SELECT ($1::int * 2)::text AS doubled with (21) (3 of 8)"
        );

        let only = scenarios(&["pgx".into()]).unwrap();
        assert!(only.iter().all(|s| s.name.starts_with("driver/pgx-")));
        assert!(scenarios(&["jdbc".into()]).is_err());
    }

    #[test]
    fn rounds_interleave_the_clients() {
        let all = scenarios(&["pgx".into()]).unwrap();
        let s = all
            .iter()
            .find(|s| s.name == "driver/pgx-default/same-names-different-sql")
            .unwrap();
        let clients: String = s.steps.iter().map(|s| s.client.as_str()).collect();
        assert_eq!(clients, "abababababababab");
    }

    #[test]
    fn cases_reject_what_the_programs_cant_read() {
        let case = |sql: &str| Case {
            name: "c".into(),
            description: String::new(),
            refs: vec![],
            setup: vec![],
            teardown: vec![],
            rounds: 1,
            step: vec![CaseStep {
                client: "a".into(),
                sql: sql.into(),
                params: vec![],
                times: 1,
            }],
        };
        assert!(check_case(&case("SELECT 1")).is_ok());
        for bad in ["SELECT\t1", "SELECT 1\n", "SELECT '%'", "SELECT ?"] {
            assert!(check_case(&case(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn parses_rows_errors_and_driver_codes() {
        let (out, broken) = program(3)
            .parse(
                &lines(
                    r#"{"driver":"psycopg","version":"3.3.6"}
{"step":1,"rows":[["42",null]]}
{"step":2,"error":"26000"}
{"step":3,"error":"DRIVER:OperationalError"}
{"connections":2}"#,
                ),
                true,
            )
            .unwrap();
        assert!(broken.is_none());
        assert_eq!(
            out[0],
            Outcome::Ok {
                rows: vec![vec![Some("42".into()), None]],
                tags: vec![]
            }
        );
        assert!(matches!(&out[1], Outcome::Error { code, .. } if code == "26000"));
        assert!(matches!(&out[2], Outcome::Error { code, .. } if code == "DRIVER:OperationalError"));
    }

    #[test]
    fn unreadable_output_is_an_error() {
        let header = r#"{"driver":"psycopg","version":"3.3.6"}"#;
        let step1 = r#"{"step":1,"rows":[]}"#;
        let end = r#"{"connections":2}"#;
        let bad = [
            // Wrong version: the image and the manifest drifted apart.
            format!("{{\"driver\":\"psycopg\",\"version\":\"3.3.5\"}}\n{step1}\n{end}"),
            // No header.
            format!("{step1}\n{end}"),
            // Steps out of order, or more than the case has.
            format!("{header}\n{{\"step\":2,\"rows\":[]}}\n{end}"),
            format!("{header}\n{step1}\n{{\"step\":2,\"rows\":[]}}\n{end}"),
            // A step after the end.
            format!("{header}\n{end}\n{step1}"),
            // Both a result and an error.
            format!("{header}\n{{\"step\":1,\"rows\":[],\"error\":\"X\"}}\n{end}"),
            // Unknown field.
            format!("{header}\n{{\"step\":1,\"rows\":[],\"message\":\"m\"}}\n{end}"),
            // Fewer connections than clients: not something a pooler can cause.
            format!("{header}\n{step1}\n{{\"connections\":1}}"),
        ];
        for text in bad {
            assert!(program(1).parse(&lines(&text), true).is_err(), "{text}");
        }
        assert!(
            program(1)
                .parse(&lines(&format!("{header}\n{step1}\n{end}")), true)
                .is_ok()
        );
    }

    #[test]
    fn a_program_the_pooler_broke_keeps_what_it_reported() {
        use crate::runner::{FailKind, from_harness};
        let header = r#"{"driver":"psycopg","version":"3.3.6"}"#;
        let step1 = r#"{"step":1,"rows":[]}"#;
        let run = |text: &str, exited_ok| {
            let (out, broken) = program(3).parse(&lines(text), exited_ok).unwrap();
            let codes: Vec<String> = out
                .iter()
                .map(|o| match o {
                    Outcome::Error { code, .. } => code.clone(),
                    _ => "ok".to_string(),
                })
                .collect();
            (codes, broken.unwrap())
        };
        // Stopped after one step, with or without an error exit: only the steps it
        // didn't report are marked.
        for exited_ok in [true, false] {
            let (codes, broken) = run(&format!("{header}\n{step1}"), exited_ok);
            assert_eq!(codes, ["ok", STOPPED, STOPPED]);
            assert_eq!(
                (broken.code, broken.what.as_str()),
                (STOPPED, "stopped after 1 of 3 steps")
            );
        }
        // Every step reported, then it failed on the way out or reconnected: every
        // outcome stays as reported, and only the note says so.
        let all = format!("{header}\n{step1}\n{{\"step\":2,\"rows\":[]}}\n{{\"step\":3,\"rows\":[]}}");
        let (codes, broken) = run(&format!("{all}\n{{\"connections\":2}}"), false);
        assert_eq!((codes, broken.code), (vec!["ok".to_string(); 3], STOPPED));
        let (codes, broken) = run(&all, true);
        assert_eq!((codes, broken.code), (vec!["ok".to_string(); 3], STOPPED));
        let (codes, broken) = run(&format!("{all}\n{{\"connections\":3}}"), true);
        assert_eq!(codes, vec!["ok".to_string(); 3]);
        assert_eq!(
            (broken.code, broken.what.as_str()),
            (RECONNECTED, "opened 3 connections for 2 clients")
        );

        // An unreported step: ERROR on Postgres, a FAIL through the pooler.
        let ok = Outcome::Ok {
            rows: vec![],
            tags: vec![],
        };
        assert!(from_harness(&driver_error(STOPPED)));
        assert_eq!(FailKind::of(&ok, &driver_error(STOPPED)), FailKind::Error);
    }

    #[test]
    fn limitations_never_mark_a_driver_case() {
        use crate::runner::Verdict;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut report = crate::report::Report::load(&root.join("results/drivers-postgres-17.11.json")).unwrap();
        let cases = scenarios(&[]).unwrap();
        // PgBouncer's SET leak is a documented limitation of the built-in scenarios; the
        // driver cases fail the same way, but no entry is about them.
        assert!(
            report
                .results
                .iter()
                .any(|r| r.target.starts_with("pgbouncer") && matches!(r.verdict, Verdict::Fail { .. }))
        );
        let limitations = crate::limitations::load(None, &[]).unwrap();
        for row in &report.results {
            let target = report.targets.iter().find(|t| t.name == row.target).unwrap();
            let case = cases.iter().find(|s| s.name == row.scenario).unwrap();
            assert!(
                limitations.expected_for(target, case, row).is_none(),
                "{}",
                row.scenario
            );
        }
        limitations.apply(&mut report, &cases);
        assert!(report.results.iter().all(|r| r.expected.is_none()));
        assert!(report.stale_expectations.is_empty());

        // `drivers --limitations DIR` checks a directory against the built-in scenarios,
        // so a pooler's own set loads; against the driver cases it couldn't.
        let dir = root.join("limitations");
        assert!(crate::limitations::load(Some(&dir), &crate::scenario::bundled().unwrap()).is_ok());
        assert!(crate::limitations::load(Some(&dir), &cases).is_err());
    }

    #[test]
    fn deadline_covers_every_step_and_connect() {
        let t = Timeouts {
            step: Duration::from_secs(10),
            ..Default::default()
        };
        assert_eq!(program(16).deadline(&t), Duration::from_secs(18 * 10 + 30));
    }

    #[test]
    fn connection_settings_go_in_the_environment() {
        let env = connection_env(&Endpoint::parse("postgres://u:p%40ss@127.0.0.1:6432/app?sslmode=disable").unwrap())
            .unwrap();
        let get = |k: &str| env.iter().find(|(key, _)| *key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("PP_PASSWORD"), Some("p@ss"));
        assert_eq!(get("PP_PORT"), Some("6432"));
        assert_eq!(get("PP_DBNAME"), Some("app"));
        assert_eq!(get("PP_SSLMODE"), Some("disable"));
        assert!(
            connection_env(
                &Endpoint::parse("postgres://u@127.0.0.1/app?sslmode=verify-full&sslrootcert=system").unwrap()
            )
            .is_err()
        );
    }
}
