# poolproof

Find out what breaks when you put a connection pooler in front of Postgres.

Poolers like PgBouncer, PgDog, Supavisor and RDS Proxy are supposed to be invisible to the
application. In transaction mode they mostly are, except when they aren't: a `SET` leaks
to another client, a temp table shows up in someone else's session, an advisory lock
outlives the client that took it, or part of a multi-statement query silently never runs.
These bugs show up in production, one at a time.

poolproof does two things:

- **`poolproof run`** replays small scenarios against plain Postgres and then through your
  pooler, and reports every step where the results differ. Postgres is the oracle, so a
  scenario never hardcodes expected output.
- **`poolproof scan`** reads your real queries (a `pg_stat_statements` export, Postgres
  logs or a `.sql` file) and flags patterns that are unsafe under transaction pooling,
  using the actual Postgres parser.

## Install

```sh
cargo install --git https://github.com/Nihal-Pandey-2302/poolproof
```

Or download a binary for Linux, macOS (x86_64 and arm64) or Windows (x86_64) from the
releases page. The scenarios and the latest results are built into the binary.

## Results

**[Results site](https://nihal-pandey-2302.github.io/poolproof/)**: every scenario against
PgBouncer 1.24 to 1.26, PgDog 0.1.50 to 0.1.62 and Supavisor 2.9 and 2.10, on Postgres 17
and 18, with the full step-by-step trace for each cell. It also marks the version where a
behavior broke or got fixed.
[RESULTS.md](RESULTS.md) has the same data as a table. Both are built from `results/`,
which `scripts/update-results.sh` regenerates and CI checks.

A failure is one of three kinds:

- **silent**: the pooler returned a result, but not the one Postgres returned. The
  application gets wrong data and no error. These are the dangerous ones.
- **error**: the pooler refused with an error where Postgres didn't. Loud, so you notice.
- **hang**: no reply within the step timeout (10 seconds; `--step-timeout 30s` changes
  it, a scenario's `timeout_ms` overrides it), or replies that stop short of what
  Postgres sent. Keep the step timeout above the pooler's own checkout and query
  timeouts, so you see its error instead of a hang; 10 seconds is above PgDog's 5 second
  checkout timeout.

An ERROR verdict means nothing could be compared, so it says nothing about the pooler: a
target URL didn't answer before the run, a step on the Postgres side failed in the harness
(timeout, connection failure), the pooler didn't answer again after a reset, setup,
teardown or reset failed, or `--check-scenarios` saw Postgres give two different results.
A FAIL the steps already found is kept even if teardown fails after it. ERROR only stops
that scenario; the run goes on.

`--fail-on` picks which FAILs make the exit code 1, by kind: `silent` (only silent ones),
`error` (silent and error), `any` (the default, hangs too) or `none`. An ERROR verdict
(nothing compared) counts for all but `none`. A pooler with known hangs can still gate CI
on wrong results. Results it ignores are counted on stderr.

`--compare-to last.json` compares the run with a saved `run --format json` report, so a
pooler with known failures can gate CI on new ones. Results match by target and scenario.
Only regressions fail the run: PASS to FAIL or ERROR, a FAIL getting worse (hang, then
error, then silent), and FAIL to ERROR or back. `--fail-on` still decides which of those
count. Fixed results, milder failures, and scenarios or targets only one side has are
listed but never fail it. A repeated (timing-sensitive) scenario gates on PASS and FAIL
only, so one failing run in five against a baseline that passed all five fails it. A
target whose product, version, config or Postgres differs gets a warning; a different
pool mode is an error. With `--format json` the comparison is in the report under
`comparison`.

```text
$ poolproof run --filter multi-statement
FAIL  pgdog-0.1.62             multi-statement-begin (silent)
      A simple query that starts with BEGIN must run every statement in the string.
      step 1: [app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT
      postgres: ok rows=[] tags=[0, 1, 0]
      pooler:   ok rows=[] tags=[0]
      from https://github.com/pgdogdev/pgdog/issues/1849
```

```text
$ poolproof scan queries.csv
BREAKS   session-advisory-lock       calls=48211    SELECT pg_advisory_lock($1)
         pg_advisory_lock() holds the lock for the whole session. ...
WARN     session-set                 calls=1022     SET search_path TO app
         SET/RESET of `search_path` outside SET LOCAL lasts for the session. ...
```

## Try it locally

```sh
docker compose -f docker/compose.yml up -d --wait   # Postgres + 10 pooler configurations
cargo run -- run
```

Each pooler in the test bed has a pool of one server connection, so leaks between clients
happen every time instead of by luck. `POSTGRES_IMAGE=postgres:18.4` picks the server
version. `scripts/update-results.sh` rebuilds `results/` and `RESULTS.md` for Postgres 17
and 18 from a fresh test bed.

The test bed listens on 127.0.0.1 only. To run more than one at a time, give each its own
port offset:

```sh
scripts/testbed.sh up 100                    # project poolproof-100, every port + 100
scripts/testbed.sh targets 100 > t100.toml   # targets.toml with the ports shifted
cargo run -- run --targets t100.toml
scripts/testbed.sh down 100
```

To test your own pooler, point poolproof at it and at the Postgres server behind it:

```sh
export PGPASSWORD=...   # keeps the password out of the command line and `ps`
poolproof run --pooler postgres://user@pooler-host:6432/db \
              --direct postgres://user@postgres-host:5432/db
```

A URL without a password gets one the way psql does: from `PGPASSWORD`, else from the
password file (`PGPASSFILE`, default `~/.pgpass`; ignored unless only you can read it;
a Unix socket matches as `localhost`, and only the first of several hosts is looked up).
In a URL, percent-encode special characters in the password (`@` is `%40`). Output
never shows passwords.

TLS works as in psql: `sslmode` is `disable`, `prefer` (the default: TLS if the server
offers it, else plaintext without a word), `require`, `verify-ca` or `verify-full`, from
the URL or `PGSSLMODE`. `prefer` never checks certificates, even with a root certificate
present (psql checks the chain then, and falls back to plaintext if that fails), so it is
encrypted but not authenticated: use `verify-full` for anything not on localhost.
`sslrootcert` (or `PGSSLROOTCERT`) names the CA file, or `system` for the operating
system's store (which needs `verify-full`, and makes it the default). Without it,
`~/.postgresql/root.crt` (`%APPDATA%\postgresql\root.crt` on Windows) is used when it
exists, and `require` then checks the chain like `verify-ca`; `verify-ca` and
`verify-full` with no root certificate at all are an error. `send` steps, cancel requests
and the reset and setup connections use the same settings. A Unix socket never uses TLS,
as in psql. Not supported yet: client certificates and `sslnegotiation=direct`.

A URL names one host and port: several hosts or ports, `hostaddr`,
`target_session_attrs` and `load_balance_hosts` are refused, since poolproof would
otherwise pick a host on its own. Socket options work as in psql: `connect_timeout`,
`keepalives` (on unless `keepalives=0`) with `keepalives_idle`, `keepalives_interval` and
`keepalives_retries`, and `tcp_user_timeout` in milliseconds. One the system can't set
is an error rather than ignored.

For several poolers, or to reset state between scenarios, use a targets file
(`--targets`, default `targets.toml`):

```toml
[[target]]
name = "prod-like-pgbouncer"
# ${NAME} is replaced with the environment variable NAME ($${ for a literal ${). In a
# URL the value is percent-encoded (escaped in key=value strings), so any password works;
# in reset statements it goes in as is. An empty value is an error, except as a URL
# password (where it means no password). Postgres itself may log a failing reset
# statement with the values in it; keep secrets in URLs, not in reset.
pooler = "postgres://user:${POOLER_PASSWORD}@pooler-host:6432/db"
direct = "postgres://user@postgres-host:5432/db"   # password from PGPASSWORD or ~/.pgpass
mode = "transaction"
# Run on `direct` before every run so one scenario's leaked state can't change the next
# one's result: "auto", "all", or a list of your own statements.
reset = "auto"
```

`reset` decides how much each run is isolated from the one before:

- `"auto"` ends the sessions from the address and user Postgres sees the pooler connect
  as (asked once per target, through the pooler). That is every such session, not only
  the pooler's: behind a NAT (Kubernetes, a cloud NAT in front of RDS) or next to an app
  on the same host, other sessions share that address and user and end too. It refuses
  to run when the pooler is on a Unix socket or on the same address as poolproof. It
  assumes one pooler per Postgres: another pooler's leaked locks stay, and so do
  sessions the pooler opens from a second address (clusters, several nodes, a restart on
  a new IP). Other applications using the same pooler lose their server connections,
  which the pooler then reopens.
- `"all"` ends every other session on the database. Against a Postgres that isn't on
  this machine it needs `run --yes`.
- A list of statements runs as written.
- Without `reset`, nothing is reset: leaks carry over between scenarios, and `run`
  warns. With `--pooler` and `--direct`, `--reset auto` or `--reset all` picks one.

Ending other roles' sessions needs superuser or `pg_signal_backend` on the `direct` user;
`run` warns when a reset terminates sessions without either.

Before the first scenario, `run` prints what the reset will touch, e.g.
`reset: 'prod-like-pgbouncer': ends every session of user app from 10.0.0.7 (not only the
pooler's) on database db at postgres-host:5432 (auto)`. The JSON report records which
kind each target used.

`--target NAME` runs only that target from the file (repeat it for several).

To share the outcome with your team, `--format html` writes one self-contained page with
the grid and every step's trace. `--format json` saves the raw results;
`poolproof report a.json b.json --format html` (or `markdown`) combines saved runs. The
JSON carries `schema_version` and `generated_at`; poolproof refuses a report in a newer
format than it reads. In these formats, a progress line per result goes to stderr. Text
output is colored on a terminal unless `NO_COLOR` is set; `FORCE_COLOR` or
`CLICOLOR_FORCE` colors it anywhere.

Scenarios create and drop their own objects (prefixed `pp_`), so use a scratch database.
That matters even more with `reset = "all"`, which kills every session on it.
`--reverse` runs the scenarios in reverse order; verdicts should not change.

## Scanning your queries

From pg_stat_statements:

```sh
psql -c "\copy (SELECT query, calls FROM pg_stat_statements) TO 'queries.csv' CSV HEADER"
poolproof scan queries.csv
```

From Postgres logs, with `log_statement = 'all'` or `log_min_duration_statement` set:
`poolproof scan postgresql.log` (stderr format), `postgresql.csv` (csvlog) or
`postgresql.json` (jsonlog). Logs keep multi-statement query strings whole;
pg_stat_statements splits them, so some problems only show up in logs. Logs are read as a
stream, so multi-GB files are fine. poolproof reads the English log prefixes (`LOG:`,
`statement:`), so the server needs `lc_messages = 'C'` or another English locale. A `.sql`
file works too, e.g. your migrations.

`--input pgss|csvlog|log|jsonlog|sql` says what the input is when the extension doesn't
(a `.csv` is told apart by its first row). `-` reads stdin:
`pg_dump --schema-only mydb | poolproof scan - --input sql`.

### Against your pooler

Add `--pooler` and every finding is checked against what that pooler version actually did
in poolproof's own runs (built into the binary, no setup needed):

```text
$ poolproof scan postgresql.log --pooler pgdog-0.1.62
BREAKS   multi-statement-transaction calls=1        BEGIN; INSERT INTO t VALUES (1); COMMIT
         on pgdog-0.1.62: multi-statement-begin fails silently
         This one query string starts with BEGIN, COMMIT or ROLLBACK and has more ...
REJECTED listen                      calls=1        LISTEN jobs;
         on pgdog-0.1.62: listen-notify-delivered fails with an error
OK       session-set                 calls=1        SET search_path TO public;
         on pgdog-0.1.62: search-path-isolated-between-clients passes

9 queries scanned, 5 findings. On pgdog-0.1.62 (Postgres 18.4): 2 break silently, 1 rejected, 2 handled.
```

BREAKS means a related scenario fails silently on that pooler; REJECTED means the pooler
returns an error, so the query would fail loudly in production; OK means it passes. With
`--pooler`, BREAKS and REJECTED fail the exit code; a finding poolproof's results don't
cover (no related scenario, or only SKIP or ERROR verdicts) counts as it would without
`--pooler`. `--postgres 17.11` picks the Postgres version (default: newest), and
`--results my-run.json` uses a run against your own setup instead. An unknown name lists
the available ones.

Without `--pooler`, the exit code is 1 if anything would break under transaction pooling
(`--strict` also fails on warnings), so it can run in CI. `--format json` prints an object
with `findings` and the counts `queries`, `unparsed` and `uninspected` (function and DO
bodies only checked for words). `--format sarif` works with GitHub code scanning:

```yaml
- run: poolproof scan migrations/all.sql --format sarif > poolproof.sarif || true
- uses: github/codeql-action/upload-sarif@v4
  with:
    sarif_file: poolproof.sarif
```

Rules today: session `SET`/`RESET` (including `SET ROLE`) and `set_config(..., false)` (a
parameter there, as pg_stat_statements shows it, gives a warning to check the value),
session advisory locks, `LISTEN`, temp tables without `ON COMMIT DROP`, `WITH HOLD`
cursors, SQL-level `PREPARE`, `DISCARD`, `DEALLOCATE ALL`, `LOAD`, and query strings that
start a transaction and run more statements in it. Function calls are checked anywhere in a
statement, including the query inside `PREPARE` and `DECLARE CURSOR`, but not in the body
of `CREATE VIEW` or `CREATE RULE`, which doesn't run then (`CREATE MATERIALIZED VIEW` does
run its query, so it is checked). Statements inside DO blocks and function bodies
(PL/pgSQL and SQL) are checked too. Findings in a function body are warnings that name the
function, since it runs whenever the function is called. Parameters a function sets in its
own `SET` clause are restored on return, so setting them in its body is fine. Dynamic
`EXECUTE` strings are not inspected, and a body the PL/pgSQL parser can't read is only
checked for words, with a count at the end.

## Writing a scenario

A scenario is a TOML file in `scenarios/`. Clients are named; each connects on its first
step. `direct = true` connects to Postgres directly, which is how scenarios check server
state behind the pooler. A client is either direct or pooled for all of its steps.
Scenario names must be unique.

```toml
name = "temp-table-isolated-between-clients"
description = "A temp table created by one client must not be visible to another client."
refs = ["https://github.com/pgdogdev/pgdog/issues/1733"]

[[step]]
client = "a"
simple = "CREATE TEMP TABLE pp_tmp (x int)"

[[step]]
client = "b"
simple = "SELECT count(*) FROM pp_tmp"
```

Step actions:

- `simple`: simple query protocol, can hold several statements.
- `prepare` + `name`, then `execute`: extended protocol. Cast columns to `text`. An
  `execute` needs an earlier `prepare` of that name by the same client since its last
  `disconnect` or abandoned query, or the scenario fails to load.
- `disconnect`: close the connection cleanly. On Postgres it waits until the backend has
  exited, so the next step no longer sees its locks or temp tables.
- `wait`: collect the result of this client's `background` query.
- `cancel`: send a CancelRequest with this client's key, through the same pooler.
- `notifications`: the NOTIFY messages this client received since the last such step.
- `send`: write protocol messages directly and record every reply, for the things drivers
  hide: statement names, Flush vs Sync, pipelines, COPY. For example:

  ```toml
  [[step]]
  client = "a"
  send = ["parse s1 SELECT 'a'::text", "bind - s1", "execute -", "sync"]
  # replies: ParseComplete, BindComplete, DataRow a, CommandComplete SELECT 1, ReadyForQuery I
  ```

  Messages: `parse NAME SQL`, `bind PORTAL STATEMENT`, `execute PORTAL [MAX_ROWS]`,
  `describe statement|portal NAME`, `close statement|portal NAME`, `sync`, `flush`,
  `query SQL`, `copydata TEXT`, `copydone`. `-` is the unnamed statement or portal. Replies
  are read up to one ReadyForQuery per `sync`/`query`/`copydone`; without those, until the
  server is quiet for half a second. Errors are recorded by SQLSTATE only. A client that uses
  `send` has its own raw connection; `disconnect` is its only other step.
  ParameterStatus replies are skipped, since poolers add and drop them freely; with
  `compare_parameter_status = true` a `send` step records them and compares them as a
  set, wherever in the step's reply they come. A dropped or changed value is a silent
  failure; `compare = "status"` ignores them. The set is per step: one that arrives
  after the step's ReadyForQuery shows as missing there and extra in the next step.
  COPY TO STDOUT replies are `CopyOutResponse`, one `CopyData` line per message (hex if
  it isn't UTF-8) and `CopyDone`.

Options on a `simple` step: `background = true` starts the query and moves on, so other
clients can act while it runs. `abandon_after_ms` drops the connection without a goodbye
if the query is still running after that long, like a client that crashed or timed out.

Options on any step: `delay_ms` sleeps first, e.g. so a cancel lands while a query runs.
`compare` can be `full` (default), `status` or `ignore`.

Scenario options: `setup` and `teardown` run directly on Postgres around each run;
`timeout_ms` overrides the step timeout for this scenario;
`modes` limits which pooling modes a scenario applies to; `repeat = 5` runs the pooler side
five times and fails if any run fails. Use `repeat` when the outcome depends on timing
inside the pooler, as with pipelines: reports then show how many runs failed, and CI
doesn't treat a change in those verdicts as a regression, because the worst case can differ
between machines.
`requires` lists privileges the `direct` user needs (`createrole`, `superuser`,
`pg_signal_backend`); on a target without them the scenario is skipped, with the reason.
If the privileges can't be read, it is ERROR instead.

`poolproof run -v --filter <name>` prints every step's result on Postgres and through the
pooler. Use it on a new scenario: a PASS only means something if Postgres did what you
expected.
`poolproof run --check-scenarios --filter <name>` runs its Postgres side twice and gives
ERROR if the results differ, e.g. because a step returns a random value, a time or a pid.
Use `compare = "status"` or `"ignore"` on such steps.

The best scenarios come from real bug reports. If your pooler bit you, a scenario with a
link to the issue is the most useful contribution. [CONTRIBUTING.md](CONTRIBUTING.md) has
the workflow.

## Status

Not covered yet: RDS Proxy and PgCat in the test bed, session-mode targets, a TLS pooler
in the test bed, and COPY TO STDOUT (`send` steps record its replies, but no scenario
uses it yet).

## License

MIT OR Apache-2.0
