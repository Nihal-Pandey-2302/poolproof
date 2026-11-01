# Scanning your queries

From pg_stat_statements:

```sh
psql -c "\copy (SELECT query, calls FROM pg_stat_statements) TO 'queries.csv' CSV HEADER"
poolproof scan queries.csv
```

Export as a superuser or a role with `pg_read_all_stats`. For any other role,
pg_stat_statements returns `<insufficient privilege>` instead of other roles' queries,
and scan can't check them.

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

```text
$ poolproof scan queries.csv
BREAKS   session-advisory-lock       calls=48211    SELECT pg_advisory_lock($1)
         pg_advisory_lock() holds the lock for the whole session. ...
WARN     session-set                 calls=1022     SET search_path TO app
         SET/RESET of `search_path` outside SET LOCAL lasts for the session. ...
```

## Against your pooler

Add `--pooler` and every finding is checked against what that pooler version actually did
in poolproof's own runs (built into the binary, no setup needed). Abridged:

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

BREAKS means a related scenario fails silently on that pooler; REJECTED means a related
scenario fails with an error or hangs: the pooler refuses the query, or another client's
query fails because of it; OK means it passes. With
`--pooler`, BREAKS and REJECTED fail the exit code; a finding poolproof's results don't
cover (no related scenario, or only SKIP or ERROR verdicts) counts as it would without
`--pooler`. A finding whose failing scenarios are all documented limitations (see
[limitations.md](limitations.md)) still says BREAKS or REJECTED, with the docs after it: `(documented: <link>)`.
`--pooler pgbouncer` (a product) means its newest stable version in the results.
`--postgres 17.11` picks the Postgres version (default: newest; without it, a finding
whose verdict differs on another version says so), and `--results my-run.json` uses a
run against your own setup instead. An unknown name lists the available ones.

Most `SET` scenarios set `work_mem`, so a `SET` finding for another parameter says so.
When the pooler's docs say it keeps that parameter per client (PgBouncer:
`application_name`, `TimeZone` and others), the finding gets a note with the docs instead
of a verdict.

Without `--pooler`, the exit code is 1 if anything would break under transaction pooling
(`--strict` also fails on warnings), so it can run in CI. `--format json` prints an object
with `findings` and the counts `queries`, `unparsed`, `hidden` (pg_stat_statements rows
shown as `<insufficient privilege>`) and `uninspected` (function and DO bodies only checked
for words). `--format sarif` works with GitHub code scanning:

```yaml
- run: poolproof scan migrations/all.sql --format sarif > poolproof.sarif || true
- uses: github/codeql-action/upload-sarif@v4
  with:
    sarif_file: poolproof.sarif
```

Rules today: session `SET`/`RESET` (including `SET ROLE`) and `set_config(..., false)` (a
parameter there, as pg_stat_statements shows it, gives a warning to check the value),
session advisory locks, `LISTEN`, temp tables without `ON COMMIT DROP`, `WITH HOLD`
cursors, SQL-level `PREPARE`, `DISCARD`, `DEALLOCATE ALL`, `LOAD`, and query strings of
several statements with `BEGIN`, `COMMIT` or `ROLLBACK` among them, first or later. With
`--pooler`, a finding about session state counts the scenarios where other clients see it
and those where its own client loses it. Function calls are checked anywhere in
a statement, including the query inside `PREPARE` and `DECLARE CURSOR`, but not in the
body of `CREATE VIEW` or `CREATE RULE`, which doesn't run then (`CREATE MATERIALIZED VIEW`
does run its query, so it is checked). Statements inside DO blocks and function bodies
(PL/pgSQL and SQL) are checked too. Findings in a function body are warnings that name the
function, since it runs whenever the function is called. Parameters a function sets in its
own `SET` clause are restored on return, so setting them in its body is fine. Dynamic
`EXECUTE` strings are not inspected, and a body the PL/pgSQL parser can't read is only
checked for words, with a count at the end.

## Which driver prepares statements

From a log, scan also reads the names of prepared statements (`parse`, `bind` and
`execute` lines; pg_stat_statements doesn't keep them). Statement names like psycopg 3's
(`_pg3_0`), pgjdbc's (`S_1`), pgx v5's (`stmtcache_...`), asyncpg's
(`__asyncpg_stmt_1__`) and sqlx's (`sqlx_s_1`) give one `driver-prepared-statements`
warning per driver, with the driver's setting for transaction pooling. Any app could
use the same names, so it says the names look like that driver's. Other names, an app's
own or other drivers' (ActiveRecord's `a1`, ...), give no finding, which doesn't mean
they are safe. Behind PgBouncer, PgDog or Odyssey the server sees the pooler's own names
(`PGBOUNCER_1`, `__pgdog_1`, `odyssey_pstmt_1`), so scan notes that instead.

With `--pooler`, psycopg, pgjdbc and pgx findings are checked against that driver's
results on the pooler (see [Real drivers](drivers.md)), from the same Postgres version
when there are some, else the newest. The line under it says how the driver does with its
setting for poolers. asyncpg and sqlx aren't tested yet, so theirs say "no driver results"
and stay warnings. Like any finding, BREAKS or REJECTED makes the exit code 1, so a
`scan --pooler` step on a log with these names can fail where it passed before.
