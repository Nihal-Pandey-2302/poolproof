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

Or download a binary for Linux or macOS (x86_64 and arm64) from the releases page. The
scenarios are built into the binary.

## Example

[RESULTS.md](RESULTS.md) has the full table for PgBouncer, PgDog and Supavisor, regenerated
by CI.

```
$ poolproof run
FAIL  pgdog-0.1.62-txn         multi-statement-begin
      A simple query that starts with BEGIN must run every statement in the string.
      step 1: [app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT
      postgres: ok rows=[] tags=[0, 1, 0]
      pooler:   ok rows=[] tags=[0]
      from https://github.com/pgdogdev/pgdog/issues/1849
PASS  pgdog-0.1.62-txn         set-isolated-between-clients
FAIL  pgbouncer-1.26-txn       set-isolated-between-clients
      A plain SET from one client must not change the setting another client sees.
      step 3: [a] simple: SHOW work_mem
      postgres: ok rows=[(7MB)] tags=[0]
      pooler:   ok rows=[(9MB)] tags=[0]
```

```
$ poolproof scan queries.csv
BREAKS  session-advisory-lock  calls=48211    SELECT pg_advisory_lock($1)
        pg_advisory_lock() holds the lock for the whole session. ...
WARN    session-set            calls=1022     SET search_path TO app
        SET/RESET of `search_path` outside SET LOCAL lasts for the session. ...
```

## Try it locally

```sh
docker compose -f docker/compose.yml up -d --wait   # Postgres 17 + PgBouncer, PgDog, Supavisor
cargo run -- run
```

Each pooler in the test bed has a pool of one server connection, so leaks between clients
happen every time instead of by luck.

To test your own pooler, point poolproof at it and at the Postgres server behind it:

```sh
poolproof run --pooler postgres://user:pass@pooler-host:6432/db \
              --direct postgres://user:pass@postgres-host:5432/db
```

For several poolers, or to reset state between scenarios, use a targets file
(`--targets`, default `targets.toml`):

```toml
[[target]]
name = "prod-like-pgbouncer"
pooler = "postgres://user:pass@pooler-host:6432/db"
direct = "postgres://user:pass@postgres-host:5432/db"
mode = "transaction"
# Run on `direct` before every run so one scenario's leaked state can't change the next
# one's result. This terminates every other session on the database.
reset = ["SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = current_database() AND pid <> pg_backend_pid() AND backend_type = 'client backend'"]
```

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

Exit code is 1 if anything would break under transaction pooling (`--strict` also fails on
warnings), so it can run in CI. `--format json` gives machine-readable output and
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

Options on a `simple` step: `background = true` starts the query and moves on, so other
clients can act while it runs. `abandon_after_ms` drops the connection without a goodbye
if the query is still running after that long, like a client that crashed or timed out.

Options on any step: `delay_ms` sleeps first, e.g. so a cancel lands while a query runs.
`compare` can be `full` (default), `status` or `ignore`.

`poolproof run -v --filter <name>` prints every step's result on Postgres and through the
pooler. Use it on a new scenario: a PASS only means something if Postgres did what you
expected.

The best scenarios come from real bug reports. If your pooler bit you, a scenario with a
link to the issue is the most useful contribution.

## Status

Early. Not covered yet: RDS Proxy in the test bed, statement name
collisions at the protocol level, COPY, and TLS.

## License

MIT OR Apache-2.0
