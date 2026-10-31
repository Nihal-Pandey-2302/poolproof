# poolproof results

Each scenario runs against plain Postgres, then through the pooler. FAIL means the
pooler gave a different result than Postgres: **silent** means a wrong result with no
error (the dangerous kind), error means the pooler refused, hang means no reply. A FAIL
marked (expected) is a limitation the pooler's own documentation describes: not a bug, but
it still breaks apps that rely on the feature, and it fails the run unless
--ignore-expected is given. Links point to the report each scenario came from. A
fraction like 2/5 means the scenario is timing-sensitive and failed in 2 of 5 runs.
(precondition 1/5) means the scenario's precondition held in 1 of 5 runs, and only that
run counts.

| Scenario | pgbouncer-1.24.1 (pg 17.11) | pgbouncer-1.25.2 (pg 17.11) | pgbouncer-1.26.1 (pg 17.11) | pgdog-0.1.50 (pg 17.11) | pgdog-0.1.56 (pg 17.11) | pgdog-0.1.62 (pg 17.11) | pgdog-0.1.62-pubsub (pg 17.11) | pgcat-1.2.0 (pg 17.11) | odyssey-1.5.2 (pg 17.11) | supavisor-2.9.13 (pg 17.11) | supavisor-2.10.0-rc.1 (pg 17.11) | supavisor-2.10.0-rc.1-strict (pg 17.11) | pgbouncer-1.26.1-tls (pg 17.11) | pgbouncer-1.24.1 (pg 18.4) | pgbouncer-1.25.2 (pg 18.4) | pgbouncer-1.26.1 (pg 18.4) | pgdog-0.1.50 (pg 18.4) | pgdog-0.1.56 (pg 18.4) | pgdog-0.1.62 (pg 18.4) | pgdog-0.1.62-pubsub (pg 18.4) | pgcat-1.2.0 (pg 18.4) | odyssey-1.5.2 (pg 18.4) | supavisor-2.9.13 (pg 18.4) | supavisor-2.10.0-rc.1 (pg 18.4) | supavisor-2.10.0-rc.1-strict (pg 18.4) | pgbouncer-1.26.1-tls (pg 18.4) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `abandoned-query-does-not-block-pool` | pass | pass | pass | FAIL error | FAIL error | FAIL error | FAIL error | FAIL error | pass | skip | skip | skip | pass | pass | pass | pass | FAIL error | FAIL error | FAIL error | FAIL error | FAIL error | pass | skip | skip | skip | pass |
| `aborted-transaction-not-reused` | pass | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `advisory-lock-released-on-disconnect` | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent |
| `advisory-lock-survives-own-transactions` | pass | pass | pass | pass | pass | pass | pass | pass | **FAIL** silent | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | pass | **FAIL** silent | pass | pass | FAIL error | pass |
| `cancel-from-idle-client-is-harmless` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `cancel-reaches-running-query` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `cancel-under-saturated-pool` | pass | pass | pass | pass | pass | pass | pass | pass | pass | skip | skip | skip | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | skip | skip | skip | pass |
| `copy-from-stdin` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `copy-to-stdout` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `cursor-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent |
| `cursor-survives-own-transactions` | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | FAIL error | pass |
| `deallocate-all-isolated` | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | FAIL error | FAIL error | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | FAIL error | FAIL error | FAIL error | pass |
| `discard-all-in-transaction-fails` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass |
| `discard-all-resets-settings` | pass | pass | pass | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | FAIL error | pass |
| `empty-search-path-from-set-config` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | FAIL error |
| `listen-notify-delivered` | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | FAIL error | FAIL error | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | FAIL error | FAIL error | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent |
| `load-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent |
| `multi-statement-begin` | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass |
| `parameter-status-after-set` | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | FAIL error | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | FAIL error | pass |
| `prepare-in-aborted-transaction` | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass |
| `prepared-statement-survives-other-clients` | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | pass | pass | pass |
| `protocol-error-after-flush` | pass | pass | pass | pass | pass | pass | pass | FAIL hang | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL hang | pass | pass | pass | pass | pass |
| `protocol-four-pipelined-requests` | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL hang 5/5 | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL hang 4/5 | pass | pass | pass |
| `protocol-pipeline-recovers-after-error` | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL hang 5/5 | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL hang 5/5 | pass | pass | pass |
| `protocol-pipelined-statements` | pass | pass | pass | pass | pass | pass | pass | FAIL error 5/5 | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error 5/5 | pass | pass | pass | pass | pass |
| `protocol-statement-names-isolated` | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | FAIL error | FAIL error | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | FAIL error | FAIL error | FAIL error | pass |
| `protocol-statement-reparsed-after-close` | pass | pass | pass | pass | pass | pass | pass | FAIL error 5/5 | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error 5/5 | pass | pass | pass | pass | pass |
| `protocol-unnamed-statement-survives-other-clients` | FAIL error | FAIL error | FAIL error | pass | pass | pass | pass | FAIL error | pass | FAIL error | FAIL error | FAIL error | FAIL error | FAIL error | FAIL error | FAIL error | pass | pass | pass | pass | FAIL error | pass | FAIL error | FAIL error | FAIL error | FAIL error |
| `search-path-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | pass |
| `set-does-not-leak-after-disconnect` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent |
| `set-isolated-between-clients` | FAIL silent (expected) | FAIL silent (expected) | FAIL silent (expected) | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | FAIL silent (expected) | FAIL silent (expected) | FAIL silent (expected) | FAIL silent (expected) | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | FAIL silent (expected) |
| `set-local-reverts-after-commit` | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | FAIL error | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | FAIL error | pass |
| `set-role-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent |
| `set-role-reset-after-pinned-disconnect` | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | pass | pass | pass | pass | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent |
| `sql-prepare-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent |
| `temp-table-isolated-between-clients` | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | FAIL error | **FAIL** silent |
| `temp-table-survives-own-transactions` | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | FAIL error | pass | pass | pass | pass | pass | pass | pass | pass | pass | FAIL error | pass | pass | FAIL error | pass |

Documented limitations: built-in.

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

### pgcat-1.2.0 (pg 17.11) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: could not get connection from the pool - AllServersDown`

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

### pgcat-1.2.0 (pg 18.4) / abandoned-query-does-not-block-pool (error)

A client that drops its connection mid-query must not block the next client until the query ends.

- step 2: `[b] simple: SELECT 'b served'::text`
- postgres: `ok rows=[(b served)] tags=[1]`
- pooler: `error 58000: could not get connection from the pool - AllServersDown`

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

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.25.2 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.26.1 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.50 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.56 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62-pubsub (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgcat-1.2.0 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.9.13 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.10.0-rc.1 (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.10.0-rc.1-strict (pg 17.11) / advisory-lock-released-on-disconnect (error)

A session advisory lock must be released when the client that took it disconnects.

- step 1: `[a] simple: SELECT pg_advisory_lock(424242)::text`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level advisory lock is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.26.1-tls (pg 17.11) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.24.1 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.25.2 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.26.1 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.50 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.56 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgdog-0.1.62-pubsub (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgcat-1.2.0 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.9.13 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.10.0-rc.1 (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### supavisor-2.10.0-rc.1-strict (pg 18.4) / advisory-lock-released-on-disconnect (error)

A session advisory lock must be released when the client that took it disconnects.

- step 1: `[a] simple: SELECT pg_advisory_lock(424242)::text`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level advisory lock is not allowed in transaction mode`
- from https://github.com/pgdogdev/pgdog/issues/1407

### pgbouncer-1.26.1-tls (pg 18.4) / advisory-lock-released-on-disconnect (silent)

A session advisory lock must be released when the client that took it disconnects.

- step 4: `[check (direct)] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424242`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `ok rows=[(1)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1407

### odyssey-1.5.2 (pg 17.11) / advisory-lock-survives-own-transactions (silent)

A session advisory lock must stay held by the client that took it, across its later transactions.

- step 3: `[a] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424243 AND pid = pg_backend_pid()`
- postgres: `ok rows=[(1)] tags=[1]`
- pooler: `ok rows=[(0)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / advisory-lock-survives-own-transactions (error)

A session advisory lock must stay held by the client that took it, across its later transactions.

- step 1: `[a] simple: SELECT pg_advisory_lock(424243)::text`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level advisory lock is not allowed in transaction mode`

### odyssey-1.5.2 (pg 18.4) / advisory-lock-survives-own-transactions (silent)

A session advisory lock must stay held by the client that took it, across its later transactions.

- step 3: `[a] simple: SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND objid = 424243 AND pid = pg_backend_pid()`
- postgres: `ok rows=[(1)] tags=[1]`
- pooler: `ok rows=[(0)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / advisory-lock-survives-own-transactions (error)

A session advisory lock must stay held by the client that took it, across its later transactions.

- step 1: `[a] simple: SELECT pg_advisory_lock(424243)::text`
- postgres: `ok rows=[()] tags=[1]`
- pooler: `error XX000: (ESESSIONLEAK) session-level advisory lock is not allowed in transaction mode`

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

### pgcat-1.2.0 (pg 17.11) / cursor-isolated-between-clients (silent)

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

### pgbouncer-1.26.1-tls (pg 17.11) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

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

### pgcat-1.2.0 (pg 18.4) / cursor-isolated-between-clients (silent)

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

### pgbouncer-1.26.1-tls (pg 18.4) / cursor-isolated-between-clients (silent)

A WITH HOLD cursor declared by one client must not be visible to another client.

- step 2: `[b] simple: FETCH 1 FROM pp_cur`
- postgres: `error 34000: cursor "pp_cur" does not exist`
- pooler: `ok rows=[(1)] tags=[1]`

### odyssey-1.5.2 (pg 17.11) / cursor-survives-own-transactions (error)

A WITH HOLD cursor must stay usable by the client that declared it, across its later transactions.

- step 3: `[a] simple: FETCH 1 FROM pp_own_cur`
- postgres: `ok rows=[(1)] tags=[1]`
- pooler: `error 34000: cursor "pp_own_cur" does not exist`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / cursor-survives-own-transactions (error)

A WITH HOLD cursor must stay usable by the client that declared it, across its later transactions.

- step 1: `[a] simple: DECLARE pp_own_cur CURSOR WITH HOLD FOR SELECT generate_series(1, 3)::text`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) WITH HOLD cursor is not allowed in transaction mode`

### odyssey-1.5.2 (pg 18.4) / cursor-survives-own-transactions (error)

A WITH HOLD cursor must stay usable by the client that declared it, across its later transactions.

- step 3: `[a] simple: FETCH 1 FROM pp_own_cur`
- postgres: `ok rows=[(1)] tags=[1]`
- pooler: `error 34000: cursor "pp_own_cur" does not exist`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / cursor-survives-own-transactions (error)

A WITH HOLD cursor must stay usable by the client that declared it, across its later transactions.

- step 1: `[a] simple: DECLARE pp_own_cur CURSOR WITH HOLD FOR SELECT generate_series(1, 3)::text`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) WITH HOLD cursor is not allowed in transaction mode`

### pgcat-1.2.0 (pg 17.11) / deallocate-all-isolated (error)

DEALLOCATE ALL from one client must not remove another client's prepared statements.

- step 3: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s49" does not exist`

### supavisor-2.9.13 (pg 17.11) / deallocate-all-isolated (error)

DEALLOCATE ALL from one client must not remove another client's prepared statements.

- step 3: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s61" does not exist`

### supavisor-2.10.0-rc.1 (pg 17.11) / deallocate-all-isolated (error)

DEALLOCATE ALL from one client must not remove another client's prepared statements.

- step 3: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s67" does not exist`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / deallocate-all-isolated (error)

DEALLOCATE ALL from one client must not remove another client's prepared statements.

- step 3: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s73" does not exist`

### pgcat-1.2.0 (pg 18.4) / deallocate-all-isolated (error)

DEALLOCATE ALL from one client must not remove another client's prepared statements.

- step 3: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s49" does not exist`

### supavisor-2.9.13 (pg 18.4) / deallocate-all-isolated (error)

DEALLOCATE ALL from one client must not remove another client's prepared statements.

- step 3: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s61" does not exist`

### supavisor-2.10.0-rc.1 (pg 18.4) / deallocate-all-isolated (error)

DEALLOCATE ALL from one client must not remove another client's prepared statements.

- step 3: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s67" does not exist`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / deallocate-all-isolated (error)

DEALLOCATE ALL from one client must not remove another client's prepared statements.

- step 3: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s73" does not exist`

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

### pgcat-1.2.0 (pg 17.11) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[("$user", public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### odyssey-1.5.2 (pg 17.11) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[("$user", public)] tags=[0]`
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

### pgbouncer-1.26.1-tls (pg 17.11) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`
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

### pgcat-1.2.0 (pg 18.4) / empty-search-path-from-set-config (silent)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `ok rows=[("$user", public)] tags=[0]`
- from https://github.com/pgbouncer/pgbouncer/issues/1663

### odyssey-1.5.2 (pg 18.4) / empty-search-path-from-set-config (silent)

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

### pgbouncer-1.26.1-tls (pg 18.4) / empty-search-path-from-set-config (error)

An empty search_path set with set_config() must survive another client changing it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[()] tags=[0]`
- pooler: `error 08P01: invalid server parameter`
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

### pgcat-1.2.0 (pg 17.11) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### odyssey-1.5.2 (pg 17.11) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
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

### pgbouncer-1.26.1-tls (pg 17.11) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
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

### pgcat-1.2.0 (pg 18.4) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### odyssey-1.5.2 (pg 18.4) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
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

### pgbouncer-1.26.1-tls (pg 18.4) / listen-notify-delivered (silent)

A client that ran LISTEN must receive NOTIFY messages sent by another client.

- step 3: `[a] notifications`
- postgres: `ok rows=[(pp_chan, hello)] tags=[]`
- pooler: `ok rows=[] tags=[]`
- from https://github.com/pgbouncer/pgbouncer/issues/655

### pgbouncer-1.24.1 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgbouncer-1.25.2 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgbouncer-1.26.1 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgdog-0.1.50 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgdog-0.1.56 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgdog-0.1.62 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgdog-0.1.62-pubsub (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgcat-1.2.0 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### odyssey-1.5.2 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### supavisor-2.9.13 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### supavisor-2.10.0-rc.1 (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / load-isolated-between-clients (error)

A library one client loads with LOAD must not show up for another client.

- step 1: `[a] simple: LOAD 'auto_explain'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) LOAD statement is not allowed in transaction mode`

### pgbouncer-1.26.1-tls (pg 17.11) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgbouncer-1.24.1 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgbouncer-1.25.2 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgbouncer-1.26.1 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgdog-0.1.50 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgdog-0.1.56 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgdog-0.1.62 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgdog-0.1.62-pubsub (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### pgcat-1.2.0 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### odyssey-1.5.2 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### supavisor-2.9.13 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### supavisor-2.10.0-rc.1 (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / load-isolated-between-clients (error)

A library one client loads with LOAD must not show up for another client.

- step 1: `[a] simple: LOAD 'auto_explain'`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) LOAD statement is not allowed in transaction mode`

### pgbouncer-1.26.1-tls (pg 18.4) / load-isolated-between-clients (silent)

A library one client loads with LOAD must not show up for another client.

- step 3: `[b] simple: SELECT current_setting('auto_explain.log_min_duration', true) AS loaded_for_b`
- postgres: `ok rows=[(NULL)] tags=[1]`
- pooler: `ok rows=[(-1)] tags=[1]`

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

### pgdog-0.1.50 (pg 17.11) / parameter-status-after-set (silent)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `CommandComplete SET, ReadyForQuery I`

### pgdog-0.1.56 (pg 17.11) / parameter-status-after-set (silent)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `CommandComplete SET, ReadyForQuery I`

### pgdog-0.1.62 (pg 17.11) / parameter-status-after-set (silent)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `CommandComplete SET, ReadyForQuery I`

### pgdog-0.1.62-pubsub (pg 17.11) / parameter-status-after-set (silent)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `CommandComplete SET, ReadyForQuery I`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / parameter-status-after-set (error)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `Error XX000, CLOSED`

### pgdog-0.1.50 (pg 18.4) / parameter-status-after-set (silent)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `CommandComplete SET, ReadyForQuery I`

### pgdog-0.1.56 (pg 18.4) / parameter-status-after-set (silent)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `CommandComplete SET, ReadyForQuery I`

### pgdog-0.1.62 (pg 18.4) / parameter-status-after-set (silent)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `CommandComplete SET, ReadyForQuery I`

### pgdog-0.1.62-pubsub (pg 18.4) / parameter-status-after-set (silent)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `CommandComplete SET, ReadyForQuery I`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / parameter-status-after-set (error)

After a SET of a parameter Postgres reports, the client must get a ParameterStatus with the new value.

- step 1: `[a] send: query SET application_name = 'pp_status'`
- postgres: `CommandComplete SET, ParameterStatus application_name=pp_status, ReadyForQuery I`
- pooler: `Error XX000, CLOSED`

### pgcat-1.2.0 (pg 17.11) / prepared-statement-survives-other-clients (error)

A protocol-level prepared statement must still work after other clients used the pool.

- step 4: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s53" does not exist`

### pgcat-1.2.0 (pg 18.4) / prepared-statement-survives-other-clients (error)

A protocol-level prepared statement must still work after other clients used the pool.

- step 4: `[a] execute q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "s53" does not exist`

### pgcat-1.2.0 (pg 17.11) / protocol-error-after-flush (hang)

An error in a request that ends with Flush instead of Sync must be sent right away, and Sync must recover.

- step 1: `[a] send: parse - SELEC 1 / flush`
- postgres: `Error 42601`
- pooler: ``
- from https://github.com/pgdogdev/pgdog/issues/1873

### pgcat-1.2.0 (pg 18.4) / protocol-error-after-flush (hang)

An error in a request that ends with Flush instead of Sync must be sent right away, and Sync must recover.

- step 1: `[a] send: parse - SELEC 1 / flush`
- postgres: `Error 42601`
- pooler: ``
- from https://github.com/pgdogdev/pgdog/issues/1873

### supavisor-2.9.13 (pg 17.11) / protocol-four-pipelined-requests (hang)

Four requests pipelined on one connection, each ending in Sync, must all get their results.

- step 1: `[a] send: parse - SELECT 1 / bind - - / execute - / sync / parse - SELECT 2 / bind - - / execute - / sync / parse - SELECT 3 / bind - - / execute - / sync / parse - SELECT 4 / bind - - / execute - / sync`
- postgres: `ParseComplete, BindComplete, DataRow 1, CommandComplete SELECT 1, ReadyForQuery I, ParseComplete, BindComplete, DataRow 2, CommandComplete SELECT 1, ReadyForQuery I, ParseComplete, BindComplete, DataRow 3, CommandComplete SELECT 1, ReadyForQuery I, ParseComplete, BindComplete, DataRow 4, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `ParseComplete, BindComplete, DataRow 1, CommandComplete SELECT 1, ReadyForQuery I, TIMEOUT`
- from https://github.com/supabase/supavisor/issues/1239

### supavisor-2.9.13 (pg 18.4) / protocol-four-pipelined-requests (hang)

Four requests pipelined on one connection, each ending in Sync, must all get their results.

- step 1: `[a] send: parse - SELECT 1 / bind - - / execute - / sync / parse - SELECT 2 / bind - - / execute - / sync / parse - SELECT 3 / bind - - / execute - / sync / parse - SELECT 4 / bind - - / execute - / sync`
- postgres: `ParseComplete, BindComplete, DataRow 1, CommandComplete SELECT 1, ReadyForQuery I, ParseComplete, BindComplete, DataRow 2, CommandComplete SELECT 1, ReadyForQuery I, ParseComplete, BindComplete, DataRow 3, CommandComplete SELECT 1, ReadyForQuery I, ParseComplete, BindComplete, DataRow 4, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `ParseComplete, BindComplete, DataRow 1, CommandComplete SELECT 1, ReadyForQuery I, TIMEOUT`
- from https://github.com/supabase/supavisor/issues/1239

### supavisor-2.9.13 (pg 17.11) / protocol-pipeline-recovers-after-error (hang)

In a pipeline, a failing request must not affect the request after the next Sync.

- step 1: `[a] send: parse - SELECT 1/0 / bind - - / execute - / sync / parse - SELECT 'second'::text / bind - - / execute - / sync`
- postgres: `ParseComplete, Error 22012, ReadyForQuery I, ParseComplete, BindComplete, DataRow second, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `ParseComplete, Error 22012, ReadyForQuery I, TIMEOUT`

### supavisor-2.9.13 (pg 18.4) / protocol-pipeline-recovers-after-error (hang)

In a pipeline, a failing request must not affect the request after the next Sync.

- step 1: `[a] send: parse - SELECT 1/0 / bind - - / execute - / sync / parse - SELECT 'second'::text / bind - - / execute - / sync`
- postgres: `ParseComplete, Error 22012, ReadyForQuery I, ParseComplete, BindComplete, DataRow second, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `ParseComplete, Error 22012, ReadyForQuery I, TIMEOUT`

### pgcat-1.2.0 (pg 17.11) / protocol-pipelined-statements (error)

Several named statements prepared and run in one pipeline must all return their own results.

- step 2: `[a] send: describe statement p1 / sync`
- postgres: `ParameterDescription 0, RowDescription text, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgcat-1.2.0 (pg 18.4) / protocol-pipelined-statements (error)

Several named statements prepared and run in one pipeline must all return their own results.

- step 2: `[a] send: describe statement p1 / sync`
- postgres: `ParameterDescription 0, RowDescription text, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgcat-1.2.0 (pg 17.11) / protocol-statement-names-isolated (error)

Two clients can prepare protocol-level statements with the same name, and each runs its own.

- step 3: `[b] send: bind - s1 / execute - / sync`
- postgres: `BindComplete, DataRow from b, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### supavisor-2.9.13 (pg 17.11) / protocol-statement-names-isolated (error)

Two clients can prepare protocol-level statements with the same name, and each runs its own.

- step 2: `[b] send: parse s1 SELECT 'from b'::text / sync`
- postgres: `ParseComplete, ReadyForQuery I`
- pooler: `Error 42P05, ReadyForQuery I`

### supavisor-2.10.0-rc.1 (pg 17.11) / protocol-statement-names-isolated (error)

Two clients can prepare protocol-level statements with the same name, and each runs its own.

- step 2: `[b] send: parse s1 SELECT 'from b'::text / sync`
- postgres: `ParseComplete, ReadyForQuery I`
- pooler: `Error 42P05, ReadyForQuery I`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / protocol-statement-names-isolated (error)

Two clients can prepare protocol-level statements with the same name, and each runs its own.

- step 2: `[b] send: parse s1 SELECT 'from b'::text / sync`
- postgres: `ParseComplete, ReadyForQuery I`
- pooler: `Error 42P05, ReadyForQuery I`

### pgcat-1.2.0 (pg 18.4) / protocol-statement-names-isolated (error)

Two clients can prepare protocol-level statements with the same name, and each runs its own.

- step 3: `[b] send: bind - s1 / execute - / sync`
- postgres: `BindComplete, DataRow from b, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### supavisor-2.9.13 (pg 18.4) / protocol-statement-names-isolated (error)

Two clients can prepare protocol-level statements with the same name, and each runs its own.

- step 2: `[b] send: parse s1 SELECT 'from b'::text / sync`
- postgres: `ParseComplete, ReadyForQuery I`
- pooler: `Error 42P05, ReadyForQuery I`

### supavisor-2.10.0-rc.1 (pg 18.4) / protocol-statement-names-isolated (error)

Two clients can prepare protocol-level statements with the same name, and each runs its own.

- step 2: `[b] send: parse s1 SELECT 'from b'::text / sync`
- postgres: `ParseComplete, ReadyForQuery I`
- pooler: `Error 42P05, ReadyForQuery I`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / protocol-statement-names-isolated (error)

Two clients can prepare protocol-level statements with the same name, and each runs its own.

- step 2: `[b] send: parse s1 SELECT 'from b'::text / sync`
- postgres: `ParseComplete, ReadyForQuery I`
- pooler: `Error 42P05, ReadyForQuery I`

### pgcat-1.2.0 (pg 17.11) / protocol-statement-reparsed-after-close (error)

After Close, a statement name can be prepared again with different SQL, and the new SQL runs.

- step 6: `[a] send: bind - s1 / execute - / sync`
- postgres: `BindComplete, DataRow new, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgcat-1.2.0 (pg 18.4) / protocol-statement-reparsed-after-close (error)

After Close, a statement name can be prepared again with different SQL, and the new SQL runs.

- step 6: `[a] send: bind - s1 / execute - / sync`
- postgres: `BindComplete, DataRow new, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgbouncer-1.24.1 (pg 17.11) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgbouncer-1.25.2 (pg 17.11) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgbouncer-1.26.1 (pg 17.11) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgcat-1.2.0 (pg 17.11) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### supavisor-2.9.13 (pg 17.11) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### supavisor-2.10.0-rc.1 (pg 17.11) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgbouncer-1.26.1-tls (pg 17.11) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgbouncer-1.24.1 (pg 18.4) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgbouncer-1.25.2 (pg 18.4) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgbouncer-1.26.1 (pg 18.4) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgcat-1.2.0 (pg 18.4) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### supavisor-2.9.13 (pg 18.4) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### supavisor-2.10.0-rc.1 (pg 18.4) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

### pgbouncer-1.26.1-tls (pg 18.4) / protocol-unnamed-statement-survives-other-clients (error)

The unnamed statement stays usable until the same client replaces it, even if other clients ran queries.

- step 3: `[a] send: bind - - / execute - / sync`
- postgres: `BindComplete, DataRow from a, CommandComplete SELECT 1, ReadyForQuery I`
- pooler: `Error 26000, ReadyForQuery I`

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

### pgcat-1.2.0 (pg 17.11) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[("$user", public)] tags=[0]`

### odyssey-1.5.2 (pg 17.11) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[("$user", public)] tags=[0]`

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

### pgbouncer-1.26.1-tls (pg 17.11) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[(public)] tags=[0]`

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

### pgcat-1.2.0 (pg 18.4) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[("$user", public)] tags=[0]`

### odyssey-1.5.2 (pg 18.4) / search-path-isolated-between-clients (silent)

A client's empty search_path must still be empty after another client changed it.

- step 3: `[a] simple: SHOW search_path`
- postgres: `ok rows=[("")] tags=[0]`
- pooler: `ok rows=[("""""")] tags=[0]`

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

### pgbouncer-1.26.1-tls (pg 17.11) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
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

### pgbouncer-1.26.1-tls (pg 18.4) / set-does-not-leak-after-disconnect (silent)

Settings from a client that disconnected must not show up for the next client.

- step 3: `[b] simple: SHOW work_mem`
- postgres: `ok rows=[(4MB)] tags=[0]`
- pooler: `ok rows=[(7MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### pgcat-1.2.0 (pg 17.11) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(4MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### odyssey-1.5.2 (pg 17.11) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(4MB)] tags=[0]`
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

### pgcat-1.2.0 (pg 18.4) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(4MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850

### odyssey-1.5.2 (pg 18.4) / set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(4MB)] tags=[0]`
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

### pgcat-1.2.0 (pg 17.11) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 3: `[a] simple: SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[0]`
- pooler: `ok rows=[(0)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1770

### odyssey-1.5.2 (pg 17.11) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 3: `[a] simple: SHOW statement_timeout`
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

### pgcat-1.2.0 (pg 18.4) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 3: `[a] simple: SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[0]`
- pooler: `ok rows=[(0)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1770

### odyssey-1.5.2 (pg 18.4) / set-local-reverts-after-commit (silent)

SET LOCAL ... TO DEFAULT must not change the session value, outside or inside a transaction.

- step 3: `[a] simple: SHOW statement_timeout`
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

### pgbouncer-1.26.1-tls (pg 17.11) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

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

### pgbouncer-1.26.1-tls (pg 18.4) / set-role-isolated-between-clients (silent)

SET ROLE from one client must not change the role another client runs as.

- step 2: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`

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

### pgbouncer-1.26.1-tls (pg 17.11) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
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

### pgbouncer-1.26.1-tls (pg 18.4) / set-role-reset-after-pinned-disconnect (silent)

A client that held an advisory lock and ran SET ROLE must not pass its role to the next client.

- step 4: `[b] simple: SELECT current_user::text`
- postgres: `ok rows=[(postgres)] tags=[1]`
- pooler: `ok rows=[(pp_role)] tags=[1]`
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

### pgcat-1.2.0 (pg 17.11) / sql-prepare-isolated-between-clients (error)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 3: `[a] simple: EXECUTE pp_q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "pp_q" does not exist`

### odyssey-1.5.2 (pg 17.11) / sql-prepare-isolated-between-clients (silent)

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

### pgbouncer-1.26.1-tls (pg 17.11) / sql-prepare-isolated-between-clients (silent)

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

### pgcat-1.2.0 (pg 18.4) / sql-prepare-isolated-between-clients (error)

A statement made with SQL PREPARE must be usable by the client that made it, and only by it.

- step 3: `[a] simple: EXECUTE pp_q`
- postgres: `ok rows=[(from a)] tags=[1]`
- pooler: `error 26000: prepared statement "pp_q" does not exist`

### odyssey-1.5.2 (pg 18.4) / sql-prepare-isolated-between-clients (silent)

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

### pgbouncer-1.26.1-tls (pg 18.4) / sql-prepare-isolated-between-clients (silent)

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

### pgcat-1.2.0 (pg 17.11) / temp-table-isolated-between-clients (silent)

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

### pgbouncer-1.26.1-tls (pg 17.11) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
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

### pgcat-1.2.0 (pg 18.4) / temp-table-isolated-between-clients (silent)

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

### pgbouncer-1.26.1-tls (pg 18.4) / temp-table-isolated-between-clients (silent)

A temp table created by one client must not be visible to another client.

- step 2: `[b] simple: SELECT count(*) FROM pp_tmp`
- postgres: `error 42P01: relation "pp_tmp" does not exist`
- pooler: `ok rows=[(0)] tags=[1]`
- from https://github.com/pgdogdev/pgdog/issues/1733

### odyssey-1.5.2 (pg 17.11) / temp-table-survives-own-transactions (error)

A temp table must stay usable by the client that created it, across its later transactions.

- step 3: `[a] simple: SELECT count(*) FROM pp_own_tmp`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `error 42P01: relation "pp_own_tmp" does not exist`

### supavisor-2.10.0-rc.1-strict (pg 17.11) / temp-table-survives-own-transactions (error)

A temp table must stay usable by the client that created it, across its later transactions.

- step 1: `[a] simple: CREATE TEMP TABLE pp_own_tmp (x int)`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) temporary table creation is not allowed in transaction mode`

### odyssey-1.5.2 (pg 18.4) / temp-table-survives-own-transactions (error)

A temp table must stay usable by the client that created it, across its later transactions.

- step 3: `[a] simple: SELECT count(*) FROM pp_own_tmp`
- postgres: `ok rows=[(0)] tags=[1]`
- pooler: `error 42P01: relation "pp_own_tmp" does not exist`

### supavisor-2.10.0-rc.1-strict (pg 18.4) / temp-table-survives-own-transactions (error)

A temp table must stay usable by the client that created it, across its later transactions.

- step 1: `[a] simple: CREATE TEMP TABLE pp_own_tmp (x int)`
- postgres: `ok rows=[] tags=[0]`
- pooler: `error XX000: (ESESSIONLEAK) temporary table creation is not allowed in transaction mode`

## Expected differences (documented limitations)

### set-isolated-between-clients (silent)

A plain SET from one client must not change the setting another client sees.

- on pgbouncer-1.24.1 (pg 17.11), pgbouncer-1.25.2 (pg 17.11), pgbouncer-1.26.1 (pg 17.11), pgbouncer-1.26.1-tls (pg 17.11), pgbouncer-1.24.1 (pg 18.4), pgbouncer-1.25.2 (pg 18.4), pgbouncer-1.26.1 (pg 18.4), pgbouncer-1.26.1-tls (pg 18.4)
- documented: "SET/RESET | Yes | Never", https://github.com/pgbouncer/pgbouncer.github.io/blob/ccb120f859a47fe3a6f3b2120d16194fd11f562f/features.md#sql-feature-map-for-pooling-modes
- note: Row of the table "SQL feature map for pooling modes" (columns: Feature, Session pooling, Transaction pooling), with the table's padding spaces collapsed.
- step 3: `[a] simple: SHOW work_mem`
- postgres: `ok rows=[(7MB)] tags=[0]`
- pooler: `ok rows=[(9MB)] tags=[0]`
- from https://github.com/pgdogdev/pgdog/issues/1850
