# poolproof results

Each scenario runs against plain Postgres, then through the pooler. FAIL means the
pooler gave a different result than Postgres. Not every FAIL is a bug: some are
documented limits of the pooling mode. Links point to the report each scenario came from.

| Scenario | pgbouncer-1.26-txn (transaction) | pgdog-0.1.62-txn (transaction) |
|---|---|---|
| `abandoned-query-does-not-block-pool` | pass | **FAIL** |
| `advisory-lock-released-on-disconnect` | **FAIL** | **FAIL** |
| `multi-statement-begin` | pass | **FAIL** |
| `prepare-in-aborted-transaction` | pass | pass |
| `prepared-statement-survives-other-clients` | pass | pass |
| `set-does-not-leak-after-disconnect` | **FAIL** | pass |
| `set-isolated-between-clients` | **FAIL** | pass |
| `temp-table-isolated-between-clients` | **FAIL** | **FAIL** |

## Differences

### pgdog-0.1.62-txn / abandoned-query-does-not-block-pool

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: checkout timeout`

### pgbouncer-1.26-txn / advisory-lock-released-on-disconnect

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62-txn / advisory-lock-released-on-disconnect

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62-txn / multi-statement-begin

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgbouncer-1.26-txn / set-does-not-leak-after-disconnect

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.26-txn / set-isolated-between-clients

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.26-txn / temp-table-isolated-between-clients

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgdog-0.1.62-txn / temp-table-isolated-between-clients

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733
