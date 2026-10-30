# poolproof results

Each scenario runs against plain Postgres, then through the pooler. FAIL means the
pooler gave a different result than Postgres: **silent** means a wrong result with no
error (the dangerous kind), error means the pooler refused, hang means no reply. Not
every FAIL is a bug: some are documented limits of the pooling mode. Links point to the
report each scenario came from.

| Scenario | pgbouncer-1.24.1 (pg 17.11) | pgbouncer-1.25.2 (pg 17.11) | pgbouncer-1.26.1 (pg 17.11) | pgdog-0.1.50 (pg 17.11) | pgdog-0.1.56 (pg 17.11) | pgdog-0.1.62 (pg 17.11) | pgdog-0.1.62-pubsub (pg 17.11) | supavisor-2.9.13 (pg 17.11) | supavisor-2.10.0-rc.1 (pg 17.11) | supavisor-2.10.0-rc.1-strict (pg 17.11) | pgbouncer-1.24.1 (pg 18.4) | pgbouncer-1.25.2 (pg 18.4) | pgbouncer-1.26.1 (pg 18.4) | pgdog-0.1.50 (pg 18.4) | pgdog-0.1.56 (pg 18.4) | pgdog-0.1.62 (pg 18.4) | pgdog-0.1.62-pubsub (pg 18.4) | supavisor-2.9.13 (pg 18.4) | supavisor-2.10.0-rc.1 (pg 18.4) | supavisor-2.10.0-rc.1-strict (pg 18.4) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `abandoned-query-does-not-block-pool` | pass | pass | pass | FAIL error | FAIL error | FAIL error | FAIL error | pass | pass | pass | pass | pass | pass | FAIL error | FAIL error | FAIL error | FAIL error | pass | pass | pass |
| `aborted-transaction-not-reused` | pass | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass |
| `advisory-lock-released-on-disconnect` | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `cancel-from-idle-client-is-harmless` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `cancel-reaches-running-query` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `cancel-under-saturated-pool` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `cursor-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `discard-all-in-transaction-fails` | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error |
| `discard-all-resets-settings` | pass | pass | pass | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | FAIL error | pass | pass | pass | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | FAIL error |
| `empty-search-path-from-set-config` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `listen-notify-delivered` | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | FAIL error | FAIL error | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | FAIL error | FAIL error | pass | **FAIL** silent | **FAIL** silent | FAIL error |
| `multi-statement-begin` | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass |
| `prepare-in-aborted-transaction` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `prepared-statement-survives-other-clients` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `search-path-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error |
| `set-does-not-leak-after-disconnect` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error |
| `set-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error |
| `set-local-reverts-after-commit` | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | FAIL error | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | FAIL error |
| `set-role-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error |
| `set-role-reset-after-pinned-disconnect` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error |
| `sql-prepare-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent |
| `temp-table-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |

## Differences

### pgdog-0.1.50 (pg 17.11) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: checkout timeout`

### pgdog-0.1.56 (pg 17.11) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: checkout timeout`

### pgdog-0.1.62 (pg 17.11) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: checkout timeout`

### pgdog-0.1.62-pubsub (pg 17.11) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: checkout timeout`

### pgdog-0.1.50 (pg 18.4) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: checkout timeout`

### pgdog-0.1.56 (pg 18.4) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: checkout timeout`

### pgdog-0.1.62 (pg 18.4) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: checkout timeout`

### pgdog-0.1.62-pubsub (pg 18.4) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: checkout timeout`

### pgdog-0.1.50 (pg 17.11) / aborted-transaction-not-reused (error)

A client that disconnects inside an aborted transaction must not pass it to the next client.

- step 1: `[a] simple: BEGIN; SET LOCAL statement_timeout = 100; SELECT pg_sleep(1); COMMIT`
- postgres: `error 57014: canceling statement due to statement timeout`
- pooler: `error 42601: multi-statement queries cannot mix SET with other commands`
- from https://github.com/pgbouncer/pgbouncer/issues/1415

### pgdog-0.1.50 (pg 18.4) / aborted-transaction-not-reused (error)

A client that disconnects inside an aborted transaction must not pass it to the next client.

- step 1: `[a] simple: BEGIN; SET LOCAL statement_timeout = 100; SELECT pg_sleep(1); COMMIT`
- postgres: `error 57014: canceling statement due to statement timeout`
- pooler: `error 42601: multi-statement queries cannot mix SET with other commands`
- from https://github.com/pgbouncer/pgbouncer/issues/1415

### pgbouncer-1.24.1 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.25.2 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.26.1 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.50 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.56 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62-pubsub (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.9.13 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.10.0-rc.1 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.10.0-rc.1-strict (pg 17.11) / advisory-lock-released-on-disconnect (error)

A session advisory lock must be released when the client that took it disconnects.

- step 1: `[a] simple: SELECT pg_advisory_lock(424242)::text`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level advisory lock is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.24.1 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.25.2 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.26.1 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.50 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.56 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62-pubsub (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.9.13 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.10.0-rc.1 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 3: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.10.0-rc.1-strict (pg 18.4) / advisory-lock-released-on-disconnect (error)

A session advisory lock must be released when the client that took it disconnects.

- step 1: `[a] simple: SELECT pg_advisory_lock(424242)::text`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level advisory lock is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.24.1 (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgbouncer-1.25.2 (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgbouncer-1.26.1 (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgdog-0.1.50 (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgdog-0.1.56 (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgdog-0.1.62 (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgdog-0.1.62-pubsub (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### supavisor-2.9.13 (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### supavisor-2.10.0-rc.1 (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / cursor-isolated-between-clients (error)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 1: `[a] simple: DECLARE pp_cur CURSOR WITH HOLD FOR SELECT generate_series(1, 3)::text`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) WITH HOLD cursor is not allowed in transaction mode`

### pgbouncer-1.24.1 (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgbouncer-1.25.2 (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgbouncer-1.26.1 (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgdog-0.1.50 (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgdog-0.1.56 (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgdog-0.1.62 (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### pgdog-0.1.62-pubsub (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### supavisor-2.9.13 (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### supavisor-2.10.0-rc.1 (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / cursor-isolated-between-clients (error)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 1: `[a] simple: DECLARE pp_cur CURSOR WITH HOLD FOR SELECT generate_series(1, 3)::text`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) WITH HOLD cursor is not allowed in transaction mode`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / discard-all-in-transaction-fails (error)

DISCARD ALL inside a transaction block must fail with 25001, not be answered by the pooler.

- step 2: `[a] simple: DISCARD ALL`
- postgres: `error 25001: DISCARD ALL cannot run inside a transaction block`
- pooler: `error XX000: (ESESSIONLEAK) DISCARD statement is not allowed in transaction mode`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / discard-all-in-transaction-fails (error)

DISCARD ALL inside a transaction block must fail with 25001, not be answered by the pooler.

- step 2: `[a] simple: DISCARD ALL`
- postgres: `error 25001: DISCARD ALL cannot run inside a transaction block`
- pooler: `error XX000: (ESESSIONLEAK) DISCARD statement is not allowed in transaction mode`

### pgdog-0.1.50 (pg 17.11) / discard-all-resets-settings (silent)

DISCARD ALL must reset the client's settings to their defaults.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`

### pgdog-0.1.56 (pg 17.11) / discard-all-resets-settings (silent)

DISCARD ALL must reset the client's settings to their defaults.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / discard-all-resets-settings (error)

DISCARD ALL must reset the client's settings to their defaults.

- step 1: `[a] simple: SET work_mem = '7MB'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`

### pgdog-0.1.50 (pg 18.4) / discard-all-resets-settings (silent)

DISCARD ALL must reset the client's settings to their defaults.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`

### pgdog-0.1.56 (pg 18.4) / discard-all-resets-settings (silent)

DISCARD ALL must reset the client's settings to their defaults.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / discard-all-resets-settings (error)

DISCARD ALL must reset the client's settings to their defaults.

- step 1: `[a] simple: SET work_mem = '7MB'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`

### pgbouncer-1.24.1 (pg 17.11) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgbouncer-1.25.2 (pg 17.11) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgbouncer-1.26.1 (pg 17.11) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### supavisor-2.9.13 (pg 17.11) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### supavisor-2.10.0-rc.1 (pg 17.11) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### supavisor-2.10.0-rc.1-strict (pg 17.11) / empty-search-path-from-set-config (error)

An empty search_path set with set_config() must survive another client changing it.

- step 1: `[a] simple: SELECT set_config('search_path', '', false)`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level set_config() call is not allowed in transaction mode`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgbouncer-1.24.1 (pg 18.4) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgbouncer-1.25.2 (pg 18.4) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgbouncer-1.26.1 (pg 18.4) / empty-search-path-from-set-config (error)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `error 08P01: invalid server parameter`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgdog-0.1.50 (pg 18.4) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[("")] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgdog-0.1.56 (pg 18.4) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[("")] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgdog-0.1.62 (pg 18.4) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[("")] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgdog-0.1.62-pubsub (pg 18.4) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[("")] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### supavisor-2.9.13 (pg 18.4) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### supavisor-2.10.0-rc.1 (pg 18.4) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### supavisor-2.10.0-rc.1-strict (pg 18.4) / empty-search-path-from-set-config (error)

An empty search_path set with set_config() must survive another client changing it.

- step 1: `[a] simple: SELECT set_config('search_path', '', false)`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level set_config() call is not allowed in transaction mode`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### pgbouncer-1.24.1 (pg 17.11) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgbouncer-1.25.2 (pg 17.11) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgbouncer-1.26.1 (pg 17.11) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgdog-0.1.50 (pg 17.11) / listen-notify-delivered (error)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 1: `[a] simple: LISTEN pp_chan`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error 58000: pub/sub disabled`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgdog-0.1.56 (pg 17.11) / listen-notify-delivered (error)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 1: `[a] simple: LISTEN pp_chan`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error 58000: pub/sub disabled`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgdog-0.1.62 (pg 17.11) / listen-notify-delivered (error)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 1: `[a] simple: LISTEN pp_chan`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error 58000: pub/sub disabled`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### supavisor-2.9.13 (pg 17.11) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### supavisor-2.10.0-rc.1 (pg 17.11) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### supavisor-2.10.0-rc.1-strict (pg 17.11) / listen-notify-delivered (error)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 1: `[a] simple: LISTEN pp_chan`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) LISTEN statement is not allowed in transaction mode`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgbouncer-1.24.1 (pg 18.4) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgbouncer-1.25.2 (pg 18.4) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgbouncer-1.26.1 (pg 18.4) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgdog-0.1.50 (pg 18.4) / listen-notify-delivered (error)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 1: `[a] simple: LISTEN pp_chan`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error 58000: pub/sub disabled`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgdog-0.1.56 (pg 18.4) / listen-notify-delivered (error)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 1: `[a] simple: LISTEN pp_chan`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error 58000: pub/sub disabled`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgdog-0.1.62 (pg 18.4) / listen-notify-delivered (error)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 1: `[a] simple: LISTEN pp_chan`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error 58000: pub/sub disabled`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### supavisor-2.9.13 (pg 18.4) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### supavisor-2.10.0-rc.1 (pg 18.4) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### supavisor-2.10.0-rc.1-strict (pg 18.4) / listen-notify-delivered (error)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 1: `[a] simple: LISTEN pp_chan`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) LISTEN statement is not allowed in transaction mode`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgdog-0.1.50 (pg 17.11) / multi-statement-begin (silent)

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgdog-0.1.56 (pg 17.11) / multi-statement-begin (silent)

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgdog-0.1.62 (pg 17.11) / multi-statement-begin (silent)

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgdog-0.1.62-pubsub (pg 17.11) / multi-statement-begin (silent)

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgdog-0.1.50 (pg 18.4) / multi-statement-begin (silent)

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgdog-0.1.56 (pg 18.4) / multi-statement-begin (silent)

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgdog-0.1.62 (pg 18.4) / multi-statement-begin (silent)

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgdog-0.1.62-pubsub (pg 18.4) / multi-statement-begin (silent)

A simple query that starts with BEGIN must run every statement in the string.

- step 1: `[app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT`
- postgres: `ok rows=[] tags=[0, 1, 0]`
- pooler: `ok rows=[] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1849

### pgbouncer-1.24.1 (pg 17.11) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

### pgbouncer-1.25.2 (pg 17.11) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

### pgbouncer-1.26.1 (pg 17.11) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

### supavisor-2.9.13 (pg 17.11) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

### supavisor-2.10.0-rc.1 (pg 17.11) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / search-path-isolated-between-clients (error)

A client's empty search_path must still be empty after another client changed it.

- step 1: `[a] simple: SET search_path = ''`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`

### pgbouncer-1.24.1 (pg 18.4) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

### pgbouncer-1.25.2 (pg 18.4) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

### supavisor-2.9.13 (pg 18.4) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

### supavisor-2.10.0-rc.1 (pg 18.4) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / search-path-isolated-between-clients (error)

A client's empty search_path must still be empty after another client changed it.

- step 1: `[a] simple: SET search_path = ''`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`

### pgbouncer-1.24.1 (pg 17.11) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.25.2 (pg 17.11) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.26.1 (pg 17.11) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.9.13 (pg 17.11) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.10.0-rc.1 (pg 17.11) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.10.0-rc.1-strict (pg 17.11) / set-does-not-leak-after-disconnect (error)

Settings from a client that disconnected must not show up for the next client.

- step 1: `[a] simple: SET work_mem = '7MB'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.24.1 (pg 18.4) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.25.2 (pg 18.4) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.26.1 (pg 18.4) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.9.13 (pg 18.4) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.10.0-rc.1 (pg 18.4) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.10.0-rc.1-strict (pg 18.4) / set-does-not-leak-after-disconnect (error)

Settings from a client that disconnected must not show up for the next client.

- step 1: `[a] simple: SET work_mem = '7MB'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.24.1 (pg 17.11) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.25.2 (pg 17.11) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.26.1 (pg 17.11) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.9.13 (pg 17.11) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.10.0-rc.1 (pg 17.11) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.10.0-rc.1-strict (pg 17.11) / set-isolated-between-clients (error)

A plain SET from one client must not change the setting another client sees.

- step 1: `[a] simple: SET work_mem = '7MB'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.24.1 (pg 18.4) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.25.2 (pg 18.4) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgbouncer-1.26.1 (pg 18.4) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.9.13 (pg 18.4) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.10.0-rc.1 (pg 18.4) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### supavisor-2.10.0-rc.1-strict (pg 18.4) / set-isolated-between-clients (error)

A plain SET from one client must not change the setting another client sees.

- step 1: `[a] simple: SET work_mem = '7MB'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgdog-0.1.56 (pg 17.11) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 3: `[a] simple: SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[0]`
- pooler: `ok rows=[(0)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1770

### pgdog-0.1.62 (pg 17.11) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 7: `[a] simple: SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[0]`
- pooler: `ok rows=[(0)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1770

### pgdog-0.1.62-pubsub (pg 17.11) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 7: `[a] simple: SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[0]`
- pooler: `ok rows=[(0)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1770

### supavisor-2.10.0-rc.1-strict (pg 17.11) / set-local-reverts-after-commit (error)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 1: `[a] simple: SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1770

### pgdog-0.1.56 (pg 18.4) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 3: `[a] simple: SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[0]`
- pooler: `ok rows=[(0)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1770

### pgdog-0.1.62 (pg 18.4) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 7: `[a] simple: SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[0]`
- pooler: `ok rows=[(0)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1770

### pgdog-0.1.62-pubsub (pg 18.4) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 7: `[a] simple: SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[0]`
- pooler: `ok rows=[(0)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1770

### supavisor-2.10.0-rc.1-strict (pg 18.4) / set-local-reverts-after-commit (error)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 1: `[a] simple: SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1770

### pgbouncer-1.24.1 (pg 17.11) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### pgbouncer-1.25.2 (pg 17.11) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### pgbouncer-1.26.1 (pg 17.11) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### supavisor-2.9.13 (pg 17.11) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### supavisor-2.10.0-rc.1 (pg 17.11) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / set-role-isolated-between-clients (error)

SET ROLE from one client must not change the role another client runs as.

- step 1: `[a] simple: SET ROLE pp_role`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`

### pgbouncer-1.24.1 (pg 18.4) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### pgbouncer-1.25.2 (pg 18.4) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### pgbouncer-1.26.1 (pg 18.4) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### supavisor-2.9.13 (pg 18.4) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### supavisor-2.10.0-rc.1 (pg 18.4) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / set-role-isolated-between-clients (error)

SET ROLE from one client must not change the role another client runs as.

- step 1: `[a] simple: SET ROLE pp_role`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) session-level SET statement is not allowed in transaction mode`

### pgbouncer-1.24.1 (pg 17.11) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### pgbouncer-1.25.2 (pg 17.11) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### pgbouncer-1.26.1 (pg 17.11) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### supavisor-2.9.13 (pg 17.11) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### supavisor-2.10.0-rc.1 (pg 17.11) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### supavisor-2.10.0-rc.1-strict (pg 17.11) / set-role-reset-after-pinned-disconnect (error)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 1: `[a] simple: SELECT pg_advisory_lock(707519)::text`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level advisory lock is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1341

### pgbouncer-1.24.1 (pg 18.4) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### pgbouncer-1.25.2 (pg 18.4) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### pgbouncer-1.26.1 (pg 18.4) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### supavisor-2.9.13 (pg 18.4) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### supavisor-2.10.0-rc.1 (pg 18.4) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1341

### supavisor-2.10.0-rc.1-strict (pg 18.4) / set-role-reset-after-pinned-disconnect (error)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 1: `[a] simple: SELECT pg_advisory_lock(707519)::text`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level advisory lock is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1341

### pgbouncer-1.24.1 (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgbouncer-1.25.2 (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgbouncer-1.26.1 (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgdog-0.1.50 (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgdog-0.1.56 (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgdog-0.1.62 (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgdog-0.1.62-pubsub (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### supavisor-2.9.13 (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### supavisor-2.10.0-rc.1 (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgbouncer-1.24.1 (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgbouncer-1.25.2 (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgbouncer-1.26.1 (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgdog-0.1.50 (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgdog-0.1.56 (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgdog-0.1.62 (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgdog-0.1.62-pubsub (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### supavisor-2.9.13 (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### supavisor-2.10.0-rc.1 (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / sql-prepare-isolated-between-clients (silent)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 2: `[b] simple: EXECUTE pp_q`
- postgres: `error 26000: prepared statement "pp_q" does not exist`
- pooler: `ok rows=[(from a)] tags=[1]`

### pgbouncer-1.24.1 (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgbouncer-1.25.2 (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgbouncer-1.26.1 (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgdog-0.1.50 (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgdog-0.1.56 (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgdog-0.1.62 (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgdog-0.1.62-pubsub (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### supavisor-2.9.13 (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### supavisor-2.10.0-rc.1 (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### supavisor-2.10.0-rc.1-strict (pg 17.11) / temp-table-isolated-between-clients (error)

A temp table created by one client must not be visible to another client.

- step 1: `[a] simple: CREATE TEMP TABLE pp_tmp (x int)`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) temporary table creation is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgbouncer-1.24.1 (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgbouncer-1.25.2 (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgbouncer-1.26.1 (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgdog-0.1.50 (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgdog-0.1.56 (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgdog-0.1.62 (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### pgdog-0.1.62-pubsub (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### supavisor-2.9.13 (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### supavisor-2.10.0-rc.1 (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### supavisor-2.10.0-rc.1-strict (pg 18.4) / temp-table-isolated-between-clients (error)

A temp table created by one client must not be visible to another client.

- step 1: `[a] simple: CREATE TEMP TABLE pp_tmp (x int)`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) temporary table creation is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1733
