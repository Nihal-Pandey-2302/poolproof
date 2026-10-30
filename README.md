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
behavior broke or got fixed. [RESULTS.md](RESULTS.md) has the same data as a table. CI
regenerates both.

A failure is one of three kinds:

- **silent**: the pooler returned a result, but not the one Postgres returned. The
  application gets wrong data and no error. These are the dangerous ones.
- **error**: the pooler refused with an error where Postgres didn't. Loud, so you notice.
- **hang**: no reply within 10 seconds, or replies that stop short of what Postgres sent.

ERROR means there was nothing to compare: a target URL didn't answer before the run, a
step on the Postgres side got no reply (timeout, connection failure), or setup, teardown
or reset failed. It only stops that scenario, and `run` exits 1 as for a FAIL.

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
BREAKS  session-advisory-lock       calls=48211    SELECT pg_advisory_lock($1)
        pg_advisory_lock() holds the lock for the whole session. ...
WARN    session-set                 calls=1022     SET search_path TO app
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

To test your own pooler, point poolproof at it and at the Postgres server behind it:

```sh
export PGPASSWORD=...   # keeps the password out of the command line and `ps`
poolproof run --pooler postgres://user@pooler-host:6432/db \
              --direct postgres://user@postgres-host:5432/db
```

A URL without a password gets one the way psql does: from `PGPASSWORD`, else from the
password file (`PGPASSFILE`, default `~/.pgpass`; ignored unless only you can read it).
In a URL, percent-encode special characters in the password (`@` is `%40`). Output
never shows passwords.

For several poolers, or to reset state between scenarios, use a targets file
(`--targets`, default `targets.toml`):

```toml
[[target]]
name = "prod-like-pgbouncer"
# ${NAME} is replaced with the environment variable NAME ($${ for a literal ${).
pooler = "postgres://user:${POOLER_PASSWORD}@pooler-host:6432/db"
direct = "postgres://user@postgres-host:5432/db"   # password from PGPASSWORD or ~/.pgpass
mode = "transaction"
# Run on `direct` before every run so one scenario's leaked state can't change the next
# one's result. This terminates every other session on the database.
reset = ["SELECT pg_terminate_backend(pid, 5000) FROM pg_stat_activity WHERE datname = current_database() AND pid <> pg_backend_pid() AND backend_type = 'client backend'"]
```

To share the outcome with your team, `--format html` writes one self-contained page with
the grid and every step's trace. `--format json` saves the raw results;
`poolproof report a.json b.json --format html` (or `markdown`) combines saved runs.

Scenarios create and drop their own objects (prefixed `pp_`), so use a scratch database.
That matters even more with `reset`, which kills every session on it. Without `reset`,
leaks carry over between scenarios and `run` prints a warning. `--reverse` runs the
scenarios in reverse order; verdicts should not change.

## Scanning your queries

From pg_stat_statements:

```sh
psql -c "\copy (SELECT query, calls FROM pg_stat_statements) TO 'queries.csv' CSV HEADER"
poolproof scan queries.csv
```

From Postgres logs, with `log_statement = 'all'` or `log_min_duration_statement` set:
`poolproof scan postgresql.log` (stderr format) or `poolproof scan postgresql.json`
(jsonlog). Logs keep multi-statement query strings whole; pg_stat_statements splits them,
so some problems only show up in logs. A `.sql` file works too, e.g. your migrations.

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
`--pooler`, only BREAKS and REJECTED fail the exit code. `--postgres 17.11` picks the
Postgres version (default: newest), and `--results my-run.json` uses a run against your
own setup instead. An unknown name lists the available ones.

Without `--pooler`, the exit code is 1 if anything would break under transaction pooling
(`--strict` also fails on warnings), so it can run in CI. `--format json` gives machine-readable output and
`--format sarif` works with GitHub code scanning:

```yaml
- run: poolproof scan migrations/all.sql --format sarif > poolproof.sarif || true
- uses: github/codeql-action/upload-sarif@v4
  with:
    sarif_file: poolproof.sarif
```

Rules today: session `SET`/`RESET` (including `SET ROLE`) and `set_config(..., false)`,
session advisory locks, `LISTEN`, temp tables without `ON COMMIT DROP`, `WITH HOLD`
cursors, SQL-level `PREPARE`, `DISCARD`, and query strings that start a transaction and
run more statements in it.

## Writing a scenario

A scenario is a TOML file in `scenarios/`. Clients are named; each connects on its first
step. `direct = true` connects to Postgres directly, which is how scenarios check server
state behind the pooler.

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
- `prepare` + `name`, then `execute`: extended protocol. Cast columns to `text`.
- `disconnect`: close the connection cleanly.
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

Options on a `simple` step: `background = true` starts the query and moves on, so other
clients can act while it runs. `abandon_after_ms` drops the connection without a goodbye
if the query is still running after that long, like a client that crashed or timed out.

Options on any step: `delay_ms` sleeps first, e.g. so a cancel lands while a query runs.
`compare` can be `full` (default), `status` or `ignore`.

Scenario options: `setup` and `teardown` run directly on Postgres around each run;
`modes` limits which pooling modes a scenario applies to; `repeat = 5` runs the pooler side
five times and fails if any run fails. Use `repeat` when the outcome depends on timing
inside the pooler, as with pipelines: reports then show how many runs failed, and CI
doesn't treat a change in those verdicts as a regression, because the worst case can differ
between machines.

`poolproof run -v --filter <name>` prints every step's result on Postgres and through the
pooler. Use it on a new scenario: a PASS only means something if Postgres did what you
expected.

The best scenarios come from real bug reports. If your pooler bit you, a scenario with a
link to the issue is the most useful contribution.

## Status

Not covered yet: RDS Proxy and PgCat in the test bed, session-mode targets, TLS, and
COPY TO STDOUT.

## License

MIT OR Apache-2.0
