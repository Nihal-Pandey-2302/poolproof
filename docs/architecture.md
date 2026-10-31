# Architecture

poolproof has two independent halves that share a report format:

- `run` is a differential tester. It plays each scenario against plain Postgres, then
  through a pooler, and compares what clients saw. Postgres is the oracle: scenarios
  never hardcode expected output.
- `scan` is a static checker. It parses queries with the Postgres parser
  (`pg_query`) and flags patterns that break under transaction pooling. With
  `--pooler`, it looks up what `run` found on that pooler.

`report` renders saved `run` results, and is how `RESULTS.md` and the results site are
made.

## Layout

```text
build.rs                 embeds scenarios/*.toml and results/*.json into the binary
src/main.rs              entry point: SIGPIPE, exit code 2 on error
src/lib.rs               the modules below, as a library for tests/ (no stable API)
src/cli.rs               clap commands and flags; wires the modules together
src/scenario.rs          scenario TOML format: Scenario, Step, Action, Compare, PoolMode
src/doctor.rs            `doctor`: the checks, their order and output (text and JSON)
src/targets.rs           targets TOML format, and the single target from --pooler/--direct;
                         Endpoint (a parsed URL: ${VAR}, password sources, sslmode, one
                         host only; prints without the password), Reset
src/runner/
  mod.rs                 Outcome (what a client saw for one step), Timeouts (every limit
                         and its default in one struct)
  suite.rs               the loop over targets and scenarios behind `run`: --target,
                         preflight, reset resolution, privileges and `requires`,
                         --check-scenarios, --fail-on
  check.rs               one scenario on one target: reset, setup, baseline, pooler, compare
                         (run_direct runs reset, setup and teardown statements)
  steps.rs               runs a scenario's steps, one connection per client
  pg.rs                  normal clients on tokio-postgres over a socket poolproof opens
                         (socket options, TLS, drain on close), cancel, wait_ready after a
                         reset, privileges, resolve_reset, server version
  raw.rs                 raw protocol clients for `send` steps
  doctor.rs              the live side of `doctor`: connects to one target, read-only
  tls.rs                 sslmode and sslrootcert as in libpq (except prefer, which never
                         checks): one rustls config per Endpoint
  compare.rs             outcome matching, Verdict, Mismatch, FailKind (silent/error/hang)
src/report/
  mod.rs                 the JSON report (schema_version), bundled results, lookups for
                         scan --pooler
  compare.rs             run --compare-to: regressions against a saved report
  markdown.rs            RESULTS.md-style table
  html.rs, report.html   the self-contained results page
src/scan/
  mod.rs                 Finding, Severity, rule list, rule -> scenario mapping
  input.rs               pg_stat_statements CSV, csvlog, stderr and jsonlog logs, .sql files;
                         --input, stdin, read as a stream and merged by query text
  rules.rs               the rules, on the parsed statement tree
src/output/
  text.rs                `run` results on the terminal
  scan.rs, sarif.rs      `scan` results as text and SARIF (cli.rs prints JSON directly)
scenarios/               one TOML file per scenario
results/                 committed `run --format json` reports, one per Postgres version
docker/compose.yml       the test bed: Postgres and every pooler version in the matrix
targets.toml             the test bed's targets
scripts/update-results.sh  regenerates results/ and RESULTS.md from a fresh test bed
scripts/testbed.sh       a second test bed with shifted ports, to run several side by side
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
                                                 for each scenario:
                                                   [--check-scenarios: baseline twice,
                                                    once per scenario, direct URL and
                                                    reset]
                                                   runner::check
                                                     1. reset, setup        (direct)
                                                     2. run_steps baseline  (all direct)
                                                     3. teardown            (direct)
                                                     4. for each pooler run (repeat
                                                        times): reset, wait for the
                                                        pooler to answer (only with a
                                                        reset), setup, run_steps through
                                                        the pooler, teardown
                                                     5. first step whose outcomes differ
                                                        -> Verdict (first failing run
                                                        kept); anything that stops it
                                                        -> ERROR
                                               -> text as it goes, or report::Report
                                                  -> JSON, markdown or HTML (progress
                                                  on stderr)
                                               -> with --compare-to: report::compare
                                                  against the saved report
```

Details that matter when changing this:

- The baseline and the pooler run use the same code, `run_steps(scenario, pooled,
  direct)`. For the baseline both URLs are Postgres. Steps marked `direct = true` go to
  Postgres in both runs, which is how scenarios look behind the pooler.
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
- Every step produces an `Outcome`: rows and command tags, an error (SQLSTATE plus
  message), `abandoned`, or reply lines. Harness-side problems are errors with a
  pseudo-code: `TIMEOUT` (no reply in time), `CONNECT` (couldn't connect), `CLOSED` (the
  connection closed), `CLIENT` (any other driver error) and `NO_STATEMENT` (an `execute`
  whose `prepare` failed). A raw `send` client records these as a final reply line
  (`TIMEOUT`, `CLOSED`) instead of an error outcome. If a baseline step ends in
  `CONNECT`, `TIMEOUT`, `CLIENT` or `CLOSED`, Postgres gave no real answer, so the
  scenario is ERROR rather than compared.
- Anything that stops a comparison is `Verdict::Error` for that scenario only, and the
  run goes on: a failing reset, setup or teardown, a pooler that doesn't answer after a
  reset, or a direct step on the pooler run that can't connect (unless the pooler
  already went wrong at an earlier step).
  Teardown always runs. On the pooler run a failing teardown only turns a PASS into
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
  `--yes` off this machine. After a reset, `wait_ready` waits until the same backend
  answers through the pooler three times in a row.
- A backend releases its locks and temp tables only when it exits, and Postgres closes
  the socket only after that. So every close of a connection to Postgres sends
  Terminate, shuts the write side and reads until EOF (`DrainOnShutdown` in `pg.rs`,
  below TLS; raw clients do the same above it). Through a pooler the EOF says nothing
  about a backend, so pooled clients don't wait. An abandoned client isn't waited for.
  A backend that doesn't exit in time is an error, so the scenario is ERROR.

## How `scan` works

```text
file or stdin -> scan::input::load_queries -> Vec<Query> (SQL, calls, line)
           -> scan::rules::scan -> findings (rule, severity, query)
           -> with --pooler: report::find_target + check_finding
                             (rule -> scenarios -> that pooler's verdicts)
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
| Reset, setup, repeat or timing behavior | `runner/check.rs`, `runner/pg.rs` (`resolve_reset`, `wait_ready`), `Timeouts` in `runner/mod.rs`; the step timeout is also set by `--step-timeout` in `cli.rs` and `timeout_ms` in `scenario.rs` |
| A targets file field | `targets.rs`, README's targets example |
| Connections, TLS or socket options | `runner/pg.rs` (`connect`, `tcp_connect`), `runner/raw.rs` (`RawConn::connect`), `runner/tls.rs`, URL checks in `targets.rs` |
| A privilege for `requires` | `scenario.rs` (the names), `privileges` in `runner/pg.rs`, `before_run` in `runner/suite.rs` |
| A field in the JSON report | `report/mod.rs`; it's a stable format, see the versioning policy in CHANGELOG.md. A change old readers would misread bumps `SCHEMA_VERSION` |
| What `--compare-to` counts as a regression | `report/compare.rs` |
| The results table or page | `report/markdown.rs`, `report/html.rs` and `report/report.html` |
| A `scan` rule | `scan/rules.rs`, `RULES` and the rule -> scenario map in `scan/mod.rs`, README's rule list, expected findings in the corpus (`tests/fixtures/scan/*.toml`, run by `tests/scan_corpus.rs`) |
| A `scan` input format | `scan/input.rs` (`InputFormat` is also the list of `--input` values) |
| Terminal or SARIF output | `output/` |
| A command or flag | `cli.rs`, README |
| A pooler or version in the matrix | `docker/compose.yml`, `targets.toml` (with `product`, `version`, `config`), the version list in README.md, the Docker ignore list in `.github/dependabot.yml`, then regenerate `results/` |
| A harness self-test fault | `tools/pp-proxy/src/fault.rs` (the fault and its name), `main.rs` (`PROXY_LISTEN` parsing) and `shared.rs` (the shared-server fault) there, the table in `tools/pp-proxy/README.md`, `selftest/expected.toml` |
| A `doctor` check | `doctor.rs` (the check and its output), `runner/doctor.rs` (the queries); it must stay read-only |
| CI or releases | `.github/workflows/`, `docs/releasing.md` |
