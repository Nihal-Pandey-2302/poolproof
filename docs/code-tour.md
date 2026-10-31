# Code tour

A reading order through the core of `run`, for someone who will maintain it. Each stop
says what to read and why. [architecture.md](architecture.md) is the map to come back to
later; this is the walk to take first. It fits in an afternoon: about four and a half
hours, the longest stops being `check.rs`, `steps.rs` and `pg.rs`.

Before reading, watch one scenario run, so the names below match something you've seen:

```sh
docker compose -f docker/compose.yml up -d --wait
cargo run -- run --target pgbouncer-1.26.1 --filter advisory-lock-released -v
```

`-v` prints every step's outcome on Postgres and through the pooler. Keep that output
open; most of the tour explains how it came about.

## 1. A scenario file, then `scenario.rs` (20 min)

Start with `scenarios/advisory-lock-released-on-disconnect.toml`: client `a` takes a lock
and disconnects, client `check` connects straight to Postgres (`direct = true`) and
counts locks. Nothing in the file says what the answer should be. Postgres decides that,
and everything else in the code follows from it.

Then `src/scenario.rs`. Read `Step` (one field per thing a step can do) and
`Step::action`, which turns the TOML's loose fields into one `Action` and rejects steps
that set two. `Scenario::check_clients` and the checks in `parse` are why a broken file
fails at load instead of halfway through a run. `Compare` (`full`, `status`, `ignore`)
is how a scenario says how much of a step's outcome matters; you'll see it used in
stop 4.

## 2. `runner/mod.rs`: the vocabulary (10 min)

Two types. `Outcome` is what a client saw for one step: rows and command tags, an error
(SQLSTATE and message), `Abandoned`, or the reply lines of a raw protocol step. Every
comparison in the project is between two `Outcome`s. `Timeouts` holds every limit the
harness uses, with a comment on each; read it once so later code that picks
`t.step` or `t.housekeeping` makes sense. The distinction that matters: `step` is how
long the system under test may take, `housekeeping` is how long the harness's own waits
may take, and the second is never shortened.

## 3. `runner/check.rs`: one scenario on one target (45 min)

This is the heart. Read `check` first: it runs the scenario on plain Postgres
(`run_once` with `direct` for both endpoints), refuses to compare if that baseline
itself failed (`baseline_failure`), then runs it through the pooler `repeat` times and
keeps the first failing run. Then `run_once`: reset, setup, steps, teardown, with
teardown running even after a failure. `Run` keeps teardown apart from the steps, and
`after_teardown` explains why: on the pooler run, a teardown that fails because of a
leaked lock must not hide the FAIL that found the leak.

Then the private `compare` at the bottom: it walks the steps in order and stops at the
first mismatch. Note the special case for a `direct` step that couldn't connect; that
says nothing about the pooler, so it's an ERROR unless the pooler already went wrong.
Finish with `deterministic`, which `--check-scenarios` uses: two baselines that differ
mean the scenario tests timing, not the pooler. `preflight` and `run_direct` are small
helpers; skim them.

## 4. `runner/compare.rs`: when two outcomes are "the same" (30 min)

`Outcome::matches` is short and worth reading slowly. Error messages never count, only
codes, because wording changes between Postgres versions and poolers. With
`compare = "status"` only success vs error and the code matter. For raw protocol replies,
ParameterStatus lines are compared as a set (`split_parameter_status`), since poolers
move them around harmlessly, and everything else in order.

`FailKind::of` decides how a failure shows up to an application: `hang` (TIMEOUT, or
replies that stop short of Postgres's with nothing wrong in them), `error` (an error
Postgres didn't give) or `silent` (a different result and no error, the dangerous kind).
`HARNESS_CODES` and `baseline_failure` are the other half of stop 3: they are the
pseudo-codes the harness itself produces (`CONNECT`, `TIMEOUT`, `CLIENT`, `CLOSED`), and
a baseline containing one can't be the oracle. The unit tests at the bottom are a good
summary of the rules.

## 5. `runner/steps.rs`: running the steps (40 min)

`run_steps(scenario, pooled, direct)` is called for both runs. For the baseline,
`pooled` and `direct` are the same Postgres URL, which is the whole trick: the same code
produces the expected and the actual outcomes. In `run_until_done`, `on_postgres` is
true for every client in the baseline and for `direct` clients in the pooler run. It
decides whether closing a client waits for its backend to exit.

Each client connects on its first step and keeps its connection until it disconnects.
Clients with a `send` step get a raw connection (stop 7) and are handled at the top of
the loop. Then one arm per `Action`: `Abandon` drops the socket without Terminate,
`Background` spawns the query and `Wait` collects it, `Execute` records `NO_STATEMENT`
when its `prepare` failed through the pooler instead of aborting. `close_all` closes
whatever is left, even when a step stopped the run early. `connect_failure` at the top
explains why a server refusing a connection (53300) compares like any other outcome on
the pooler side.

## 6. `runner/pg.rs`: connections, reset, readiness (45 min)

The biggest file; read it in three parts and skip the rest.

1. Connections: `connect_with`, then `open` and `tcp_connect`. poolproof opens every
   socket itself, with the URL's socket options, then adds TLS (`tls.rs`) and hands the
   stream to tokio-postgres. Read the comment on `DrainOnShutdown` carefully: a backend
   releases its locks and temp tables before Postgres closes the socket, so reading to
   EOF after Terminate is how the harness knows a baseline client's state is gone.
   Without it, a scenario like the one from stop 1 is flaky. `cancel` sends a
   CancelRequest over the same TLS settings.
2. Reset: `resolve_reset`, `check_reach` and `cannot_end`. A reset turns into
   statements once per target. `auto` asks the pooler which address and user Postgres
   sees, `all` ends every other session the direct user may end, and both refuse
   (`ResetRefusal`) when they couldn't end the pooler's sessions, rather than reset half
   of them and produce results that depend on luck. `RESET_ALL` and `reset_auto_sql`
   are the statements; values from the system under test get into the SQL only after
   parsing (the address as an IP) or quoting (the user as a literal).
3. Readiness: `wait_ready` and `settle`. After a reset kills its server connections, a
   pooler needs a moment to notice and reconnect. `settle` waits until the same backend
   answers a few times in a row, and `ROTATING` remembers poolers seen rotating their
   server connections, so they aren't waited for again.

`privileges` and `Privileges::missing` are what turn a scenario's `requires` into SKIP.

## 7. `runner/raw.rs`: the raw protocol client (30 min)

Some scenarios test what drivers hide: statement names, Sync vs Flush, pipelining. A
`send` step lists frontend messages as text (`parse s1 SELECT 1`, `bind - s1`, `sync`);
`RawMessage`'s `FromStr` parses them and `RawMessage::write` encodes them.
`RawConn::connect` does startup and authentication itself; `negotiate_tls` reads exactly
one byte after SSLRequest, so nothing a server sends early is taken as plaintext.
`exchange` writes the messages and reads until one ReadyForQuery per Sync, Query or
CopyDone, or until the server goes quiet when there was none (a Flush), ending with a
`TIMEOUT` or `CLOSED` line if replies stop. `summary` turns each backend message into
the line you saw in `-v` output. `MAX_MESSAGE` and `check_header` keep a hostile server
from making poolproof allocate gigabytes; the fuzz targets in `fuzz/` exercise this
file.

## 8. `runner/suite.rs`: the loop behind `run` (20 min)

`run` is the outer loop: for each target, preflight (both URLs must answer), resolve the
reset, read the direct user's privileges, then for each scenario `before_run` (SKIP for
another mode or missing privileges, ERROR for an unreachable target) and `check`. The
first three only happen when some scenario applies to the target, so a target filtered
down to nothing is never reset (`reset_is_not_resolved_when_no_scenario_applies`); only
its server version is read, for the report. The
`determinism` list caches `--check-scenarios` results per scenario, direct URL and reset,
so a scenario is checked once even with many targets. `FailOn` is the exit code ladder.
`cli.rs` (the `Command::Run` arm of `run`) is where the result becomes a `Report` and
gets printed; read that arm next, it's short.

## 9. `report/`: what a run leaves behind (30 min)

`report/mod.rs` is the JSON report: `Report`, `TargetInfo`, `ResultRow`. It's a stable
format with `SCHEMA_VERSION`; read the versioning section of CHANGELOG.md alongside it.
`bundled` loads the results in `results/`, built into the binary, which `scan --pooler`
uses through `find_target` and `check_finding`. `compare.rs` is `run --compare-to`:
`classify` decides whether a change between two runs is a regression, using the same
ladder as `--fail-on`. `markdown.rs` renders `RESULTS.md`; `html.rs` fills
`report.html`, the self-contained results page. `schema.rs` is only tests: they fail
when `schemas/v1` no longer matches the Rust types.

## After the tour

What's left is reference rather than core: `targets.rs` (URL parsing, passwords,
`${VAR}`), `tls.rs` (sslmode as in libpq), `doctor.rs` and `runner/doctor.rs`, and
`scan/`, which shares nothing with `run` but the report. architecture.md's last table
says which file to open for each kind of change.

Two ways to check your understanding:

- Pick a FAIL in `RESULTS.md`, open its trace (`-v`, or the trace dialog in the HTML
  report), and find the line in `compare.rs` that made it a FAIL and gave it its kind.
- Run the harness self-test (see `tools/pp-proxy/README.md`, and the `selftest` job in
  `.github/workflows/ci.yml` for the commands). Each fault injector breaks the protocol
  in one way, and `selftest/expected.toml` says which scenarios must notice. Predicting
  the verdicts before reading that file is a good test of stops 3 to 7.
