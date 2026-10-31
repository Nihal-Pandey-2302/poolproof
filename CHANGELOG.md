# Changelog

All notable changes to poolproof are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow the policy
below.

## Versioning

poolproof uses [Semantic Versioning](https://semver.org/). Before 1.0, a breaking change
bumps the minor version (0.1 to 0.2) and everything else bumps the patch version.

These are the stable interfaces. Breaking one is a breaking change:

- **CLI**: command names, flags and exit codes. The text and markdown output is for
  people and can change in any release. Exit codes:
  - `run`: 1 when a result matches `--fail-on` (default `any`: any FAIL or ERROR), else
    0. `--fail-on none` gives 0. `--ignore-expected` leaves out FAILs that a
    documented limitation explains.
  - `scan`: 1 on a BREAKS finding, with `--strict` also on WARN, else 0. With
    `--pooler`: 1 on a finding that pooler's results show as BREAKS or REJECTED;
    findings those results don't cover (no related scenario, or only SKIP or ERROR
    verdicts) count as they would without `--pooler`.
  - `doctor`: 1 when a check fails, with `--strict` also on a warning, else 0.
  - Any command: 2 when poolproof itself failed (bad arguments, unreadable files).
- **Scenario files** (`scenarios/*.toml`) and **targets files** (`targets.toml`): a file
  that works in one version works the same way in every later version with the same
  major version (minor, before 1.0). New optional keys are not breaking.
- **JSON report** (`run --format json`, read by `report` and `scan --results`): fields
  can be added in any release; removing, renaming or changing the meaning of a field is
  breaking. A newer poolproof reads reports from older versions of the same major.
  Reports carry `schema_version`, which goes up only for a change that older readers
  would misread.
- **`scan --format json`**: same rules as the JSON report.
- **SARIF output** follows SARIF 2.1.0. Rule IDs only change in a breaking release.

Not breaking, but always listed here: new or changed scenarios, a harness fix that changes
a verdict, new `scan` rules, and new pooler versions in the published results.

## [Unreleased]

First release.

### Added

- `poolproof run`: replays scenarios against Postgres and through each pooler, and reports
  every step where the results differ (silent, error or hang). Text, JSON, markdown and
  HTML output; `--pooler`/`--direct` for a single target, `--filter`, `--reverse`, `-v`.
- 37 scenarios: session state leaks (SET, SET ROLE, search_path, temp tables, advisory
  locks, cursors, prepared statements, LISTEN/NOTIFY, DEALLOCATE ALL, LOAD), session
  state that must survive the client's own transactions, multi-statement transactions,
  cancel requests, abandoned queries, ParameterStatus after SET, and protocol-level
  messages (statement names, Flush vs Sync, pipelines, COPY FROM STDIN and TO STDOUT)
  with raw `send` steps.
- `poolproof scan`: flags transaction-pooling-unsafe queries in pg_stat_statements CSV,
  Postgres stderr and jsonlog logs, and `.sql` files. Text, JSON and SARIF output.
  `--pooler` checks each finding against what that pooler version did in poolproof's
  own runs.
- `poolproof report`: renders saved JSON runs as markdown or one self-contained HTML page.
- Target probes before the first scenario: what a second client gets while the first is
  idle and while it is inside a transaction. A probe that contradicts the declared mode
  makes the target ERROR. Scenarios can require `one_server_connection` or
  `several_server_connections`, and session pools of one skip scenarios that need two
  connections at once. `doctor` shows the second probe as "server connections".
  `ignore_server_requirements = true` on a target (`run --ignore-server-requirements`
  with `--pooler`) runs those scenarios anyway, for Postgres standing in for its own
  pooler.
- `reset = "exact"` (with `pool_size`, 1 to 16): ends only the pooler's own server
  connections, found through the pooler before each run, and leaves neighbours with the
  same user and address alone. Before the first scenario it checks that the pool really
  is that size; when it isn't, or a reset can't account for every server connection,
  the target is an ERROR. `doctor` runs the same check and ends nothing. With `--pooler`:
  `--reset exact --pool-size N`. The report records each target's `pool_size`.
- Scenario steps can be `evidence` of which backend a client is on, and `same_backend`
  pairs make a run count only when the clients really shared a server connection; no
  counted run is a SKIP. `same_backend_or_closed` also counts a run where the first
  client's backend had exited.
- Scenario steps can be a `precondition`: a boolean query that decides whether a run
  through the pooler counts. Reports say in how many runs it held ("pass (precondition
  1/5)"), and the HTML trace marks steps that never ran and why. The HTML report shows
  what the pool probes saw.
- `poolproof drivers`: runs small programs for real drivers (psycopg 3, pgx,
  node-postgres, pgjdbc) through each target, with each driver's defaults and with the
  settings its docs give for poolers, and compares every step with the same program
  against Postgres. Each driver runs in a pinned container (needs Docker). It takes
  run's target, reset and output flags, plus `--driver NAME`. Results on Postgres 17.11
  are in DRIVERS.md.
- Documented limitations (`limitations/<product>.toml`): a FAIL that exactly matches a
  limitation in the pooler's own docs is marked expected, with the doc link and quote.
  It stays a FAIL. Entries that no longer match are reported as stale.
  `run --ignore-expected` leaves those FAILs out of the exit code. The HTML report shows
  expected FAILs apart from bugs (an outlined chip, separate counts for passes, expected
  failures and bugs, an optional filter that is off on every load, the doc quoted in the
  trace), and RESULTS.md marks them "(expected)" in their own section. Both list stale
  entries.
- Version notes in the HTML report name both tested versions, e.g. "fails from 0.1.56
  (passed in 0.1.50)", since the tested versions skip releases.
- Results for PgBouncer 1.24.1, 1.25.2, 1.26.1 (also with TLS between client and
  pooler), PgDog 0.1.50, 0.1.56, 0.1.62 (also with pub/sub), Supavisor 2.9.13,
  2.10.0-rc.1 (also in strict mode), PgCat 1.2.0 and Odyssey 1.5.2 on Postgres 17.11 and
  18.4, built into the binary.
- Session-mode targets for PgBouncer 1.26.1, PgDog 0.1.62 and Supavisor 2.10.0-rc.1 in
  `targets-variants.toml`, started with the test bed's `variants` profile
  (`COMPOSE_PROFILES=variants`). Their results are not published yet.
  `scripts/testbed.sh targets OFFSET FILE` shifts the ports in any targets file.
- `run --compare-to BASE.json`: compares the run with a saved JSON report and fails only
  on regressions (PASS to FAIL or ERROR, a FAIL getting worse, FAIL to ERROR or back),
  as far as `--fail-on` counts them. Fixed, milder and one-sided results are listed but
  don't fail the run. Text and markdown show the comparison; JSON has it under
  `comparison`.
- Release binaries for Linux (x86_64, arm64), macOS (x86_64, arm64) and Windows (x86_64),
  with SHA256SUMS and build provenance attestations. `cargo binstall poolproof` installs
  them. CI also runs the Windows build: every scenario, with PostgreSQL as its own
  pooler, on Windows, and the error paths and exit codes of `run`, `scan` and `doctor`.
- Guides: testing a managed pooler such as RDS Proxy or hosted Supavisor
  (`docs/managed-poolers.md`), and a code tour for new contributors (`docs/code-tour.md`).
- A harness self-test: a small proxy (`tools/pp-proxy`) puts known faults between
  poolproof and Postgres, and CI checks that every scenario gets the verdicts listed in
  `selftest/expected.toml`.
- Shell completions (bash, zsh, fish, PowerShell, elvish) and man pages, in the release
  archives and from `poolproof completions <shell>` and `poolproof man [command]`.
- Release binaries are built with thin LTO and stripped: 9.2 MB to 6.5 MB on Linux x86_64.
- A GitHub Action (`uses: Nihal-Pandey-2302/poolproof@<version>`): downloads and verifies
  the release binary, runs `scan` (with an optional SARIF upload to code scanning) or
  `run` (results table in the job summary), and fails with poolproof's exit code.
- ERROR verdict when nothing could be compared, instead of a PASS or an aborted run: a
  target URL that doesn't answer before the run (checked once per target), a step on the
  Postgres side that fails in the harness (timeout, connection failure), a pooler that
  doesn't answer again after a reset, or setup, teardown or reset failing. It stops that
  scenario only; the run goes on.
- A `send` reply from the pooler that stops short of what Postgres sent counts as a hang.
- `run --check-scenarios`: runs each scenario's Postgres side twice and gives ERROR if
  the two runs differ, e.g. a step that returns a random value, a time or a pid.
- `run --fail-on silent|error|any|none` picks which failures set exit code 1, by kind.
  `--target NAME` runs only the named targets from the targets file.
- A progress line per result on stderr with JSON, markdown and HTML output, and colored
  labels in text output on a terminal (`NO_COLOR` turns it off, `FORCE_COLOR` or
  `CLICOLOR_FORCE` on).
- `poolproof doctor`: checks a target before a run (both URLs answer and reach the same
  Postgres, TLS, privileges, what reset would end, the pooling mode). Only reads. Text and
  JSON output.
- `run --step-timeout` (e.g. `30s`, `500ms`; default `10s`) and a per-scenario
  `timeout_ms` that overrides it.
- Setup, teardown and reset statements run with `lock_timeout` 5s and
  `statement_timeout` 30s, so a lock a pooler leaked gives ERROR instead of hanging the
  run.
- Passwords as in psql: from the URL, else `PGPASSWORD`, else the password file
  (`PGPASSFILE`, default `~/.pgpass`, `%APPDATA%\postgresql\pgpass.conf` on Windows). On
  Unix a password file that group or others can read is ignored with a warning.
- `${NAME}` in a targets file's `pooler`, `direct` and `reset` is replaced by that
  environment variable; an unset one is an error. `$${` is a literal `${`.
- Passwords never appear in output, errors or reports: URLs are printed without them.
- TLS for every connection (normal clients, `send` clients, cancel requests, reset and
  setup), as in psql: `sslmode` (`disable`, `prefer` (the default), `require`,
  `verify-ca`, `verify-full`) and `sslrootcert` (a CA file or `system`) in the URL, or
  `PGSSLMODE` and `PGSSLROOTCERT`. `require` with a root certificate (`sslrootcert`, or
  `~/.postgresql/root.crt`, or `%APPDATA%\postgresql\root.crt` on Windows, if it
  exists) checks the chain. `verify-ca` and `verify-full` without one are an error when
  the targets load. `sslrootcert=system` needs `verify-full`. Client certificates are not
  supported yet.
- `reset = "auto"` ends only the sessions from the address and user the pooler connects to
  Postgres as, and refuses when that can't be told apart from poolproof's own sessions.
  `reset = "all"` ends every other session on the database that the `direct` user may end,
  and needs `run --yes` against a Postgres on another machine. Unless the `direct` user is
  a superuser, both check the role the pooler connects as and refuse to run when they
  couldn't end its sessions (when the pooler's role is another one and the direct user
  lacks `pg_signal_backend` or `pg_read_all_stats`, or when that role is a superuser),
  naming the `GRANT` that fixes it; `doctor` reports this as a failure. A list of
  statements works as before. Before the first scenario, `run` prints what each target's
  reset will end, and warns when your own reset statements terminate sessions without
  those privileges. `run --pooler ... --reset auto|all` for a single target. The JSON
  report records each target's reset kind, never the SQL or addresses.
- `requires` in scenario files: privileges the `direct` user needs (`createrole`,
  `superuser`, `pg_signal_backend`). A target without them SKIPs the scenario, with the
  reason; if the privileges can't be read, the scenario is ERROR.
- Socket options from the URL as in libpq: `connect_timeout`, `keepalives` (on unless
  `keepalives=0`) with `keepalives_idle`, `keepalives_interval` and `keepalives_retries`,
  and `tcp_user_timeout`. One that the system can't set is an error, not ignored.
- JSON Schemas for the report, scenario files and targets files, published at
  `https://nihal-pandey-2302.github.io/poolproof/schemas/v1/`. A report names its schema
  in `$schema`. A `#:schema` line in a TOML file lets editors with taplo check it.
- JSON report: `schema_version` (1; reports without it are 1, a newer one is refused with
  a clear error) and `generated_at` (UTC).
- `send` steps record COPY TO STDOUT replies: `CopyOutResponse`, one `CopyData` line per
  message (hex if it isn't UTF-8), `CopyDone`.
- `compare_parameter_status = true` on a `send` step records ParameterStatus messages
  and compares them as a set. A dropped or changed value is a silent failure.
- Scenario files are checked when loaded: two scenarios with the same name, a client
  that mixes `direct` and pooled steps, or an `execute` of a statement the client didn't
  prepare on that connection (since its last `disconnect` or abandoned query) are errors.
- `scan --input pgss|csvlog|log|jsonlog|sql`, and `-` for stdin. csvlog is new. Logs and
  CSV are read as a stream, so large logs use little memory, and invalid UTF-8 no longer
  fails the file. A log with no statements in it is an error that names `log_statement`
  and `lc_messages`.
- `scan` rules `deallocate-all` and `load` (warnings).
- `scan` checks function calls inside `PREPARE` and `DECLARE CURSOR`.
- `scan` checks the statements inside DO blocks and function bodies (PL/pgSQL and SQL,
  including trigger functions and `BEGIN ATOMIC`). A DO block runs right away, so its
  findings keep their severity. A function body runs whenever the function is called, so
  its findings are warnings that name the function. Parameters the function sets in its
  own `SET` clause are restored on return and not flagged. Dynamic `EXECUTE` strings are
  not inspected.
- `scan` counts the bodies the PL/pgSQL parser could not read, which are only checked for
  words: in the text summary, as `uninspected` in JSON, and as a SARIF notification.
- `scan` names the driver behind a log's prepared statements and, with `--pooler`, checks
  psycopg, pgjdbc and pgx against their driver results; a `scan --pooler` step on such
  logs can now fail.

### Changed

- **Breaking:** connections try TLS first (`sslmode=prefer`, as psql). Add
  `sslmode=disable` to a URL for plaintext only.
- **Breaking** for targets files from earlier git builds: `${` starts an environment
  variable. Write `$${` for a literal `${`.
- A URL names one host and port. Several hosts or ports, `hostaddr`,
  `target_session_attrs` and `load_balance_hosts` are refused, since poolproof would
  otherwise pick a host on its own.
- `sslnegotiation=direct` in a URL, or `PGSSLNEGOTIATION=direct`, is refused when the
  targets load: normal clients would start TLS directly while `send` clients send
  SSLRequest, so the two would connect differently. `postgres` is accepted, any other
  value is an error, and the URL's value wins over the environment, as in libpq.
- The `SET ROLE` scenarios declare `requires = ["createrole"]` and run
  `GRANT pp_role TO CURRENT_USER` in setup, so a non-superuser can run them and they test
  the pooler instead of passing on a permission error.
- The bundled targets file uses `reset = "all"`, the same statement as before.
- **Breaking:** `scan --format json` prints an object with `findings` and the counts
  `queries`, `unparsed` and `uninspected`, instead of an array of findings.

### Fixed

- `run`: without superuser or `pg_read_all_stats`, Postgres hides other roles' session
  details, so `reset = "all"` and `"auto"` matched none of the pooler's sessions, reported
  success, and leaked state carried over between scenarios. A pooler connecting as a
  superuser made the reset fail in every scenario. Both are now refused before the run,
  with the reason.
- `requires` and the reset checks count a privilege only if the user can use it
  (`pg_has_role(..., 'USAGE')`), so a `NOINHERIT` membership no longer counts.
- `run`: a `direct` user that inherits a superuser role (`GRANT postgres TO app`) passes
  that check for the superuser, so `reset = "all"` tried to end the superuser's sessions,
  Postgres refused, and every scenario was ERROR. Only a superuser may end a superuser's
  session, so the reset now leaves those alone unless the `direct` user is one.
- `run`: a backend releases its locks and temp tables only when it exits, so a step right
  after a disconnect could still see them. Every close of a connection to Postgres now
  reads until Postgres closes the socket, which it does after that cleanup. A backend
  that doesn't exit within 10 seconds makes the scenario ERROR.
- `run`: after a reset, a pooler can swap in a new server connection a moment later.
  poolproof now waits until the same backend answers through the pooler three times in a
  row (at most 2 seconds) before the scenario starts.
- `tcp_user_timeout` in a URL is read in milliseconds, as libpq does. The driver read it
  as seconds.
- `run`: an unreachable Postgres or pooler made scenarios pass, because both sides failed
  the same way. They are ERROR now.
- `run`: a failing setup, reset, step or teardown aborted the whole run. Teardown now
  always runs, and on the pooler side a teardown failure no longer hides a FAIL the steps
  found.
- `run`: a direct step that couldn't connect on the pooler run no longer hides a pooler
  failure at an earlier step.
- `run`: repeated scenarios waited for the pooler before each run even without a
  `reset`, up to 10s each when the pooler was slow to answer. They only wait after a
  reset now.
- `send` steps reject a server message over 64 MB, or with an impossible length, instead
  of allocating it.
- `scan`: the `set_config` rule reads each call's own third argument. `true` gives no
  finding, `false` a warning, and a parameter (how pg_stat_statements shows any literal)
  a warning to check the value. A `false` elsewhere in the query no longer triggers it.
  The value is read through casts, string booleans (`'true'`, `'on'`) and named
  arguments (`is_local => true`).
- `scan`: function calls in the body of `CREATE VIEW` and `CREATE RULE`, and in the
  `CREATE FUNCTION` statement itself, are no longer flagged, since they don't run when the
  statement does. `CREATE MATERIALIZED VIEW` still is, because it runs its query. The
  statements in a function body are checked, and give warnings that name the function.
- `scan`: in jsonlog files, only `LOG` messages count as statements.

### Security

- A `send` client reads exactly one byte after its SSLRequest before the TLS handshake,
  so a server or man in the middle can't inject plaintext replies ahead of TLS
  (CVE-2021-23222, the libpq side of it).
- HTML report: `refs` from a results file become links only for `http(s)://` URLs, with
  `rel="noopener noreferrer"`, and the page has a Content-Security-Policy that allows only
  its own inline script and style (by hash).

[Unreleased]: https://github.com/Nihal-Pandey-2302/poolproof/commits/main
