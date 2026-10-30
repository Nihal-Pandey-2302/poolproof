# poolproof results

Each scenario runs against plain Postgres, then through the pooler. FAIL means the
pooler gave a different result than Postgres. Not every FAIL is a bug: some are
documented limits of the pooling mode. Links point to the report each scenario came from.

| Scenario | pgbouncer-1.26-txn (transaction) | pgdog-0.1.62-txn (transaction) |
|---|---|---|
| `abandoned-query-does-not-block-pool` | pass | **FAIL** |
| `aborted-transaction-not-reused` | pass | pass |
| `advisory-lock-released-on-disconnect` | **FAIL** | **FAIL** |
| `cancel-from-idle-client-is-harmless` | pass | pass |
| `cancel-reaches-running-query` | pass | pass |
| `cancel-under-saturated-pool` | pass | pass |
| `cursor-isolated-between-clients` | **FAIL** | **FAIL** |
| `discard-all-in-transaction-fails` | pass | pass |
| `discard-all-resets-settings` | pass | pass |
| `listen-notify-delivered` | **FAIL** | **FAIL** |
| `multi-statement-begin` | pass | **FAIL** |
| `prepare-in-aborted-transaction` | pass | pass |
| `prepared-statement-survives-other-clients` | pass | pass |
| `search-path-isolated-between-clients` | **FAIL** | pass |
| `set-does-not-leak-after-disconnect` | **FAIL** | pass |
| `set-isolated-between-clients` | **FAIL** | pass |
| `set-local-reverts-after-commit` | pass | **FAIL** |
| `set-role-isolated-between-clients` | **FAIL** | pass |
| `set-role-reset-after-pinned-disconnect` | **FAIL** | pass |
| `sql-prepare-isolated-between-clients` | **FAIL** | **FAIL** |
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

### pgbouncer-1.26-txn / cursor-isolated-between-clients

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgdog-0.1.62-txn / cursor-isolated-between-clients

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgbouncer-1.26-txn / listen-notify-delivered

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgdog-0.1.62-txn / listen-notify-delivered

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 1: `[a] simple: LISTEN pp_chan`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error 58000: pub/sub disabled`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgdog-0.1.62-txn / multi-statement-begin

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgbouncer-1.26-txn / search-path-isolated-between-clients

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

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

### pgdog-0.1.62-txn / set-local-reverts-after-commit

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 7: `[a] simple: SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[0]`
- pooler: `ok rows=[(0)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1770

### pgbouncer-1.26-txn / set-role-isolated-between-clients

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### pgbouncer-1.26-txn / set-role-reset-after-pinned-disconnect

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### pgbouncer-1.26-txn / sql-prepare-isolated-between-clients

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgdog-0.1.62-txn / sql-prepare-isolated-between-clients

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

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
