# Architecture

poolproof has two independent halves that share a report format:

- `run` is a differential tester. It plays each scenario against plain Postgres, then
  through a pooler, and compares what clients saw. Postgres is the oracle: scenarios
  never hardcode expected output.
- `drivers` is `run` with real drivers as the clients: psycopg, pgx, node-postgres and
  pgjdbc, each a small program in a Docker image.
- `scan` is a static checker. It parses queries with the Postgres parser
  (`pg_query`) and flags patterns that break under transaction pooling. With
  `--pooler`, it looks up what `run` found on that pooler.

`report` renders saved `run` results, and is how `RESULTS.md` and the results site are
made.

New to the code? [code-tour.md](code-tour.md) is a reading order through the core of
`run`, one file at a time.

## Layout

```text
build.rs                 embeds scenarios/*.toml, limitations/*.toml and
                         results/postgres-*.json into the binary, and the verdicts of
                         results/drivers-postgres-*.json
src/main.rs              entry point: SIGPIPE, exit code 2 on error
src/lib.rs               the modules below, as a library for tests/ (no stable API)
src/cli.rs               clap commands and flags; wires the modules together
src/scenario.rs          scenario TOML format: Scenario, Step, Action, Compare, PoolMode
src/doctor.rs            `doctor`: the checks, their order and output (text and JSON)
src/targets.rs           targets TOML format, and the single target from --pooler/--direct;
                         Endpoint (a parsed URL: ${VAR}, password sources, sslmode, one
                         host only; prints without the password), Reset
src/limitations.rs       documented pooler limitations (limitations/*.toml): loading, and
                         marking a FAIL that matches an entry exactly as expected (suite.rs
                         per row as it prints, cli.rs on the whole report, with stale
                         entries)
src/version.rs           version order (17.11, 0.1.62, 2.10.0-rc.1): limitation version
                         ranges, the newest Postgres for scan --pooler, and the newest
                         driver results
src/runner/
  mod.rs                 Outcome (what a client saw for one step), Timeouts (every
                         configurable limit and its default in one struct)
  suite.rs               the loop over targets and scenarios behind `run`: --target,
                         preflight, reset resolution, privileges and `requires`, probes,
                         the exact reset's start check, --check-scenarios, --fail-on
  check.rs               one scenario on one target: reset, setup, baseline, pooler, compare
                         (run_direct runs reset, setup and teardown statements)
  steps.rs               runs a scenario's steps, one connection per client
  pg.rs                  normal clients on tokio-postgres over a socket poolproof opens
                         (socket options, TLS, drain on close), cancel, wait_ready after a
                         reset, privileges, resolve_reset, server version
  raw.rs                 raw protocol clients for `send` steps
  doctor.rs              the live side of `doctor`: connects to one target, read-only
  probe.rs               before a target's scenarios: what a second client gets while the
                         first is idle and inside a transaction (probe_target)
  exact.rs               reset = "exact": finds the pooler's server connections by holding
                         pool_size transactions through it, and ends exactly those
  tls.rs                 sslmode and sslrootcert as in libpq (except prefer, which never
                         checks): one rustls config per Endpoint
  compare.rs             outcome matching, Verdict, Mismatch, FailKind (silent/error/hang)
src/report/
  mod.rs                 the JSON report (schema_version), bundled results, lookups for
                         scan --pooler
  compare.rs             run --compare-to: regressions against a saved report
  schema.rs              tests that keep schemas/v1 in sync with the Rust types
  markdown.rs            RESULTS.md-style table
  html.rs, report.html   the self-contained results page
src/scan/
  mod.rs                 Finding, Severity, rule list, rule -> scenario mapping
  input.rs               pg_stat_statements CSV, csvlog, stderr and jsonlog logs, .sql files;
                         --input, stdin, read as a stream and merged by query text; from
                         logs also prepared statement names
  rules.rs               the rules, on the parsed statement tree
  statements.rs          statement names: which driver they look like, which pooler renamed
                         them, and the driver results for --pooler
src/drivers/mod.rs       `drivers`: drivers.toml and cases.toml as scenarios whose steps a
                         driver program runs, the docker run, its output checked and read
src/output/
  text.rs                `run` results on the terminal
  scan.rs, sarif.rs      `scan` results as text and SARIF (cli.rs prints JSON directly)
scenarios/               one TOML file per scenario
limitations/             one TOML file per pooler product: its documented limitations
drivers/                 the driver programs, one directory and image each; drivers.toml
                         (versions, variants), cases.toml (the steps), targets.txt (where
                         CI runs them), docker-bake.hcl (builds the images), README.md
                         (the program contract)
results/                 committed `run --format json` reports: postgres-*.json (one per
                         Postgres version), drivers-postgres-*.json and
                         variants-postgres-*.json; known-flaky.txt (pairs CI compares
                         loosely)
schemas/v1/              JSON Schemas for reports, scenario and targets files, generated from
                         the Rust types and published with the site
docker/compose.yml       the test bed: Postgres and every pooler version in the matrix; the
                         `variants` profile adds non-default configs
targets.toml             the test bed's targets
targets-variants.toml    the variant targets (variants profile)
scripts/update-results.sh  regenerates results/ and RESULTS.md from a fresh test bed
scripts/update-driver-results.sh  regenerates results/drivers-*.json and DRIVERS.md
scripts/testbed.sh       a second test bed with shifted ports, to run several side by side
scripts/pgdog-ref.sh     the suite against PgDog built from a git ref, to test a fix or
                         bisect
.github/compare-verdicts.sh  compares a CI run's verdicts with the committed results: what
                         makes CI red
action.yml               the GitHub Action (scan or run in your own CI)
tools/pp-proxy/          self-test proxies (own crate, not shipped): a passthrough that must
                         pass every scenario, and fault injectors that must fail known ones
selftest/expected.toml   the verdict each self-test target must get
fuzz/                    cargo-fuzz targets for raw replies and `send` lines (own crate,
                         not shipped, not in CI)
```

## How `run` works

```text
cli ─ targets.toml or --pooler/--direct ─┐
    ─ scenarios (built in or --scenarios)┴─> runner::suite::run
                                               for each target (--target picks some):
                                                 preflight: both URLs must answer, else
                                                 every scenario of the target is ERROR
                                                 resolve reset: print what it will end;
                                                 a refusal is ERROR like preflight
                                                 privileges of the direct user: unmet
                                                 `requires` -> SKIP
                                                 probes: a second client while the first
                                                 is idle, then in a transaction; a
                                                 contradiction with the mode -> ERROR
                                                 exact reset: the pool is pool_size,
                                                 else ERROR
                                                 for each scenario:
                                                   [--check-scenarios: baseline twice,
                                                    once per scenario, direct URL and
                                                    reset]
                                                   runner::check_reuse
                                                     1. reset, setup        (direct)
                                                     2. run_steps_on        (baseline:
                                                                             all direct)
                                                     3. teardown            (direct)
                                                     4. for each pooler run (repeat
                                                        times): reset, wait for the
                                                        pooler to answer (only with a
                                                        statements reset), setup,
                                                        run_steps_on through the
                                                        pooler, teardown
                                                     5. first step whose outcomes differ
                                                        -> Verdict (first failing run
                                                        kept; runs with an unmet
                                                        precondition, or same_backend
                                                        evidence showing the clients
                                                        apart, don't count); anything
                                                        that stops it -> ERROR
                                               -> text as it goes, or report::Report
                                                  -> JSON, markdown or HTML (progress
                                                  on stderr)
                                               -> with --compare-to: report::compare
                                                  against the saved report
```

Details that matter when changing this:

- The baseline and the pooler run use the same code, `run_steps_on(scenario, pooled,
  direct, baseline, t)`, with `baseline` saying which run it is. For the baseline both
  URLs are Postgres. The flag is explicit because Postgres can stand in for its own
  pooler, and its evidence and preconditions must still run. Steps marked
  `direct = true` go to Postgres in both runs, which is how scenarios look behind the
  pooler.
- Each client gets its own connection on its first step. Normal clients use
  tokio-postgres (`pg.rs`). A client with any `send` step gets a raw connection
  (`raw.rs`) that writes frontend messages itself and records every reply as a line,
  because drivers hide exactly what those scenarios test (statement names, Sync vs
  Flush, pipelining).
- poolproof opens every TCP socket itself (`tcp_connect` in `pg.rs`, with the URL's
  socket options), for both client kinds, then does TLS per `tls.rs`. That is why a URL
  may name only one host: nothing picks among several. Raw clients read exactly one byte
  after SSLRequest, so nothing sent ahead of the handshake is read as plaintext.
  Cancel requests use the same TLS settings.
- Every step produces an `Outcome`: rows and affected-row counts (`tags`, from
  CommandComplete), an error (SQLSTATE plus message), `abandoned`, reply lines, or
  `not_run` (an evidence or precondition step on Postgres, or a step after an unmet
  precondition through the pooler). Harness-side problems are errors with a
  pseudo-code: `TIMEOUT` (no reply in time), `CONNECT` (couldn't connect), `CLOSED` (the
  connection closed), `CLIENT` (any other driver error) and `NO_STATEMENT` (an `execute`
  whose `prepare` failed). A raw `send` client records these as a final reply line
  (`TIMEOUT`, `CLOSED`) instead of an error outcome. If a baseline step ends in
  `CONNECT`, `TIMEOUT`, `CLIENT` or `CLOSED`, Postgres gave no real answer, so the
  scenario is ERROR rather than compared. Driver programs (`drivers`) add
  `DRIVER:<class>`, an error the driver raised without a SQLSTATE, which counts the
  same way.
- A `drivers` scenario has `program` set: `run_once` runs that driver program instead of
  `run_steps_on`, once with both of its connections on Postgres and once on the pooler.
  Everything around it (reset, setup, teardown, the comparison) is the same. The
  program's output must name the version `drivers.toml` pins and report its steps in
  order, or the scenario is ERROR. A program that stops early, fails on the way out or
  reconnects keeps the outcomes it reported (the rest are `DRIVER:Stopped`) and returns
  a `Broken` note: on Postgres it makes the scenario ERROR, and through the pooler
  `compare` adds it to the FAIL the steps found, or makes one at the last step.
- Anything that stops a comparison is `Verdict::Error` for that scenario only, and the
  run goes on: a failing reset, setup or teardown, a pooler that doesn't answer after a
  reset, or a direct step on the pooler run that can't connect (unless the pooler
  already went wrong at an earlier step).
  Teardown runs whenever setup was attempted (a failed reset or `wait_ready` returns
  before setup). On the pooler run a failing teardown only turns a PASS into
  ERROR, so a FAIL caused by, say, a leaked lock on the scenario's table is kept.
  Setup, teardown and reset run with `lock_timeout` and `statement_timeout` so they
  can't hang the run.
- `compare.rs` decides whether two outcomes match, per the step's `compare` setting.
  Error messages never count, only codes, since wording differs between versions. The
  first mismatch becomes a `Fail` with a kind: `silent` (a different result), `error`
  (an error Postgres didn't give) or `hang` (a timeout, or replies that stop short of
  Postgres's). ParameterStatus lines, recorded only with `compare_parameter_status`, are
  compared as a set; the other reply lines in order.
- `reset` exists because poolers keep server connections. Without it, state one
  scenario leaked can change the next scenario's verdict. `resolve_reset` turns a
  target's `reset` into statements once per target: `auto` asks the pooler for its
  address and user and builds a scoped `pg_terminate_backend` query, `all` needs
  `--yes` off this machine, and `exact` has no statements (see below). After a
  statements reset (`Isolation::needs_wait`; the exact reset waits on its own),
  `wait_ready` waits for the pooler to answer, then settles: it asks for the backend pid
  until the same one answers three times in a row. Settling is best effort: it gives up
  after `SETTLE_MAX` and goes on, and is skipped for poolers seen rotating their server
  connections (`ROTATING`). `wait_ready` is an error only when the pooler never
  answered.
- A backend releases its locks and temp tables only when it exits, and Postgres closes
  the socket only after that. So every close of a connection to Postgres sends
  Terminate, shuts the write side and reads until EOF (`DrainOnShutdown` in `pg.rs`,
  below TLS; raw clients do the same above it). Through a pooler the EOF says nothing
  about a backend, so pooled clients don't wait. An abandoned client isn't waited for.
  A backend that doesn't exit in time is an error, so the scenario is ERROR.

## Probes, evidence, preconditions, exact reset, limitations

These decide whether a result means anything, so read them before changing a verdict.

- **Probes** (`runner/probe.rs`, called from `suite.rs` once per target). A second
  client connects while the first is idle, then while the first is inside a
  transaction. The results check the declared mode (a contradiction makes the target
  ERROR) and decide the `one_server_connection` and `several_server_connections`
  requirements, which `before_run` turns into SKIPs; in session mode they also say
  whether the pool has room for a scenario's clients. `ignore_server_requirements`
  skips the server-connection requirements (not the session-mode room check) and the
  `same_backend` check (suite.rs passes `reuse = false` to `check_reuse`, so every run
  counts), for Postgres standing in for its own pooler.
- **Evidence** (run in `runner/steps.rs`: `evidence`, `closed_since`; judged by
  `evidence_of` and `compare_with` in `runner/check.rs`). An `evidence` step records
  which backend a client is on: its pid through the pooler, and that backend's start
  time read on direct. On the pooler run, a `same_backend` pair
  that doesn't match means the clients never shared a server connection, so the run
  doesn't count; `same_backend_or_closed` also counts one where the first backend had
  exited. A scenario with no counted run is a SKIP whose reason starts "passed, but".
  Evidence a scenario asks for mid-exchange (inside COPY IN) is the scenario's mistake.
- **Preconditions** (run in `runner/steps.rs`, judged in `runner/check.rs`). A
  `precondition` step runs only through the pooler. False stops that run, which then
  doesn't count; anything but true or false is a FAIL. With `repeat`, the row records
  how many runs met it.
- **Exact reset** (`runner/exact.rs`, built in `suite.rs` for `reset = "exact"`). It
  holds `pool_size` transactions through the pooler to learn each server connection's
  pid, start time, user and address, then ends exactly those. Before the first scenario
  it checks the pool really is that size; once a reset can't account for every server
  connection, the target's remaining scenarios ERROR. `doctor` runs the same check and
  ends nothing.
- **Limitations** (`limitations.rs`, data in `limitations/`). Applied in two places:
  `suite::run` marks each row as it prints it (`expected_for`), and `cli.rs` runs
  `apply` on every report it prints or renders, which also lists stale entries. Both use
  the same matching (`about`, `mismatch`), so a change to the rule covers both. A FAIL
  that matches an entry exactly (product, version range, config, Postgres, step,
  outcome) stays a FAIL, marked expected with the doc's link and quote. An entry that
  applies to a target but doesn't match its result is listed as stale, unless another
  entry explains that result or it is a SKIP or ERROR; entries that apply to no target
  are silent.

## How `scan` works

```text
file or stdin -> scan::input::load_queries -> Input: Vec<Query> (SQL, calls, line)
                                                   + statement names (logs only)
           -> scan::rules::scan -> findings (rule, severity, query), and from the
                                   names scan::statements: a finding per driver
           -> with --pooler: report::find_target + check_finding
                             (rule -> scenarios -> that pooler's verdicts);
                             driver findings: scan::check_driver against
                             report::driver_results, a list of its own
           -> output::scan (text), output::sarif, or JSON printed by cli.rs
```

The results `--pooler` uses come from `results/`, built into the binary, or from a
report given with `--results`.

## Where to change what

| To add or change | Edit |
|---|---|
| A scenario | `scenarios/<name>.toml`, then `scripts/update-results.sh` (see CONTRIBUTING.md) |
| A step action or option | `scenario.rs` (format, and the checks when a file loads: unique names, one side per client, `execute` after `prepare`), `runner/steps.rs`, README's step list |
| A protocol message for `send` | `runner/raw.rs` (parsing the `send` line, encoding, reply lines) |
| How outcomes are compared or classified | `runner/compare.rs`, with unit tests there |
| Reset, setup, repeat or timing behavior | `runner/check.rs`, `runner/pg.rs` (`resolve_reset`, `wait_ready`), `runner/exact.rs` (built in `runner/suite.rs`), `Timeouts` in `runner/mod.rs`; the step timeout is also set by `--step-timeout` in `cli.rs` and `timeout_ms` in `scenario.rs` |
| A targets file field | `targets.rs`, README's targets example |
| Connections, TLS or socket options | `runner/pg.rs` (`connect`, `tcp_connect`), `runner/raw.rs` (`RawConn::connect`), `runner/tls.rs`, URL checks in `targets.rs` |
| A privilege for `requires` | `scenario.rs` (the names), `privileges` in `runner/pg.rs`, `before_run` in `runner/suite.rs`; the server-connection requirements in `runner/probe.rs` |
| A field in the JSON report | `report/mod.rs`; it's a stable format, see the versioning policy in CHANGELOG.md. A change old readers would misread bumps `SCHEMA_VERSION` |
| What `--compare-to` counts as a regression | `report/compare.rs` |
| Any field of a report, scenario or targets file | also regenerate `schemas/v1` with `UPDATE_SCHEMAS=1 cargo test schema`; a test fails until it matches |
| The results table or page | `report/markdown.rs`, `report/html.rs` and `report/report.html` |
| A `scan` rule | `scan/rules.rs`, `RULES` and the rule -> scenario map in `scan/mod.rs`, README's rule list, expected findings in the corpus (`tests/fixtures/scan/*.toml`, run by `tests/scan_corpus.rs`) |
| A driver, variant or driver case | `drivers/` (the program and its lockfile, `drivers.toml`, `cases.toml`; the contract is in `drivers/README.md`), the Dependabot ignore list in `.github/dependabot.yml`, then `scripts/update-driver-results.sh` |
| A `scan` input format | `scan/input.rs` (`InputFormat` is also the list of `--input` values) |
| Terminal or SARIF output | `output/` |
| A command or flag | `cli.rs`, README |
| A pooler or version in the matrix | `docker/compose.yml`, `targets.toml` (with `product`, `version`, `config`), the version list in README.md, the Docker ignore list in `.github/dependabot.yml`, then regenerate `results/` |
| A harness self-test fault | `tools/pp-proxy/src/fault.rs` (the fault and its name), `main.rs` (`PROXY_LISTEN` parsing) and `shared.rs` (the shared-server fault) there, the table in `tools/pp-proxy/README.md`, `selftest/expected.toml` |
| A `doctor` check | `doctor.rs` (the check and its output), `runner/doctor.rs` (the queries); it must stay read-only |
| Known flaky pairs, or how CI compares verdicts | `results/known-flaky.txt`, `.github/compare-verdicts.sh` |
| CI or releases | `.github/workflows/`, `docs/releasing.md` |
