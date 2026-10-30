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
src/targets.rs           targets TOML format, and the single target from --pooler/--direct
src/runner/
  mod.rs                 Outcome (what a client saw for one step), timeouts
  suite.rs               the loop over targets and scenarios behind `run`
  check.rs               one scenario on one target: reset, setup, baseline, pooler, compare
                         (run_direct runs reset, setup and teardown statements)
  steps.rs               runs a scenario's steps, one connection per client
  pg.rs                  normal clients on tokio-postgres: connect, wait_ready after a
                         reset, waiting for a disconnected backend to exit, server version
  raw.rs                 raw protocol clients for `send` steps
  compare.rs             outcome matching, Verdict, Mismatch, FailKind (silent/error/hang)
src/report/
  mod.rs                 the JSON report, bundled results, lookups for scan --pooler
  markdown.rs            RESULTS.md-style table
  html.rs, report.html   the self-contained results page
src/scan/
  mod.rs                 Finding, Severity, rule list, rule -> scenario mapping
  input.rs               pg_stat_statements CSV, stderr and jsonlog logs, .sql files
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
```

## How `run` works

```text
cli ─ targets.toml or --pooler/--direct ─┐
    ─ scenarios (built in or --scenarios)┴─> runner::suite::run
                                               for each target, for each scenario:
                                               runner::check
                                                 1. reset, setup        (direct)
                                                 2. run_steps baseline  (every client direct)
                                                 3. teardown            (direct)
                                                 4. for each pooler run (repeat times):
                                                    reset, wait for the pooler to answer
                                                    (only with a reset), setup, run_steps
                                                    through the pooler, teardown
                                                 5. first step whose outcomes differ
                                                    -> Verdict (first failing run kept)
                                               -> text as it goes, or report::Report
                                                  -> JSON, markdown or HTML
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
- Every step produces an `Outcome`: rows and command tags, an error (SQLSTATE plus
  message), `abandoned`, or reply lines. Harness-side problems are errors with a
  pseudo-code: `TIMEOUT` (no reply in time), `CONNECT` (couldn't connect), `CLOSED` (the
  connection closed), `CLIENT` (any other driver error) and `NO_STATEMENT` (an `execute`
  whose `prepare` failed). A raw `send` client records these as a final reply line
  (`TIMEOUT`, `CLOSED`) instead of an error outcome.
- `compare.rs` decides whether two outcomes match, per the step's `compare` setting.
  Error messages never count, only codes, since wording differs between versions. The
  first mismatch becomes a `Fail` with a kind: `silent` (a different result), `error`
  (an error Postgres didn't give) or `hang` (timeout).
- `reset` exists because poolers keep server connections. Without it, state one
  scenario leaked can change the next scenario's verdict.
- Closing a direct connection returns before Postgres has ended the backend, so the
  runner waits for the backend to exit after a direct client disconnects, before the
  next step looks at server state.

## How `scan` works

```text
input file -> scan::input::load_queries -> Vec<Query> (SQL, calls, line)
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
| A step action or option | `scenario.rs` (format and validation), `runner/steps.rs`, README's step list |
| A protocol message for `send` | `runner/raw.rs` (parsing the `send` line, encoding, reply lines) |
| How outcomes are compared or classified | `runner/compare.rs`, with unit tests there |
| Reset, setup, repeat or timing behavior | `runner/check.rs`, `runner/pg.rs`, timeouts in `runner/mod.rs` |
| A targets file field | `targets.rs`, README's targets example |
| A field in the JSON report | `report/mod.rs`; it's a stable format, see the versioning policy in CHANGELOG.md |
| The results table or page | `report/markdown.rs`, `report/html.rs` and `report/report.html` |
| A `scan` rule | `scan/rules.rs`, `RULES` and the rule -> scenario map in `scan/mod.rs`, README's rule list |
| A `scan` input format | `scan/input.rs` |
| Terminal or SARIF output | `output/` |
| A command or flag | `cli.rs`, README |
| A pooler or version in the matrix | `docker/compose.yml`, `targets.toml` (with `product`, `version`, `config`), the version list in README.md, the Docker ignore list in `.github/dependabot.yml`, then regenerate `results/` |
| A harness self-test fault | `tools/pp-proxy/src/fault.rs` (the fault and its name), `main.rs` (`PROXY_LISTEN` parsing) and `shared.rs` (the shared-server fault) there, the table in `tools/pp-proxy/README.md`, `selftest/expected.toml` |
| CI or releases | `.github/workflows/`, `docs/releasing.md` |
