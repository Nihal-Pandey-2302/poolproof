# poolproof results

Each scenario runs against plain Postgres, then through the pooler. FAIL means the
pooler gave a different result than Postgres: **silent** means a wrong result with no
error (the dangerous kind), error means the pooler refused, hang means no reply. A FAIL
marked (expected) is a limitation the pooler's own documentation describes: not a bug, but
it still breaks apps that rely on the feature, and it fails the run unless
--ignore-expected is given. Links point to the report each scenario came from. A
fraction like 2/5 means the scenario is timing-sensitive and failed in 2 of 5 runs.

| Scenario | pgbouncer-1.26.1 | pgdog-0.1.62 | pgcat-1.2.0 | odyssey-1.5.2 | supavisor-2.10.0-rc.1 | supavisor-2.10.0-rc.1-strict |
|---|---|---|---|---|---|---|
| `driver/node-postgres-default/auto-prepare` | pass | pass | pass | pass | pass | pass |
| `driver/node-postgres-default/same-names-different-sql` | pass | pass | pass | pass | pass | pass |
| `driver/node-postgres-default/same-sql-both-connections` | pass | pass | pass | pass | pass | pass |
| `driver/node-postgres-default/set-leak` | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `driver/node-postgres-default/transaction` | pass | pass | pass | pass | pass | pass |
| `driver/node-postgres-named/auto-prepare` | pass | pass | FAIL error | pass | FAIL error | FAIL error |
| `driver/node-postgres-named/same-names-different-sql` | pass | pass | FAIL error | pass | pass | pass |
| `driver/node-postgres-named/same-sql-both-connections` | pass | pass | FAIL error | pass | FAIL error | FAIL error |
| `driver/node-postgres-named/set-leak` | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `driver/node-postgres-named/transaction` | pass | pass | FAIL error | pass | pass | pass |
| `driver/pgjdbc-default/auto-prepare` | pass | pass | pass | pass | FAIL error | FAIL error |
| `driver/pgjdbc-default/same-names-different-sql` | pass | pass | pass | pass | FAIL error | FAIL error |
| `driver/pgjdbc-default/same-sql-both-connections` | pass | pass | pass | pass | FAIL error | FAIL error |
| `driver/pgjdbc-default/set-leak` | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `driver/pgjdbc-default/transaction` | pass | pass | pass | pass | FAIL error | FAIL error |
| `driver/pgjdbc-no-prepare/auto-prepare` | pass | pass | pass | pass | pass | FAIL error |
| `driver/pgjdbc-no-prepare/same-names-different-sql` | pass | pass | pass | pass | pass | FAIL error |
| `driver/pgjdbc-no-prepare/same-sql-both-connections` | pass | pass | pass | pass | pass | FAIL error |
| `driver/pgjdbc-no-prepare/set-leak` | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `driver/pgjdbc-no-prepare/transaction` | pass | pass | pass | pass | pass | FAIL error |
| `driver/pgx-default/auto-prepare` | pass | pass | FAIL error | pass | FAIL error | FAIL error |
| `driver/pgx-default/same-names-different-sql` | pass | pass | FAIL error | pass | pass | pass |
| `driver/pgx-default/same-sql-both-connections` | pass | pass | FAIL error | pass | FAIL error | FAIL error |
| `driver/pgx-default/set-leak` | **FAIL** silent | pass | FAIL error | **FAIL** silent | **FAIL** silent | FAIL error |
| `driver/pgx-default/transaction` | pass | pass | FAIL error | pass | pass | pass |
| `driver/pgx-exec/auto-prepare` | pass | pass | pass | pass | pass | pass |
| `driver/pgx-exec/same-names-different-sql` | pass | pass | pass | pass | pass | pass |
| `driver/pgx-exec/same-sql-both-connections` | pass | pass | pass | pass | pass | pass |
| `driver/pgx-exec/set-leak` | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `driver/pgx-exec/transaction` | pass | pass | pass | pass | pass | pass |
| `driver/psycopg-default/auto-prepare` | pass | pass | FAIL error | pass | FAIL error | FAIL error |
| `driver/psycopg-default/same-names-different-sql` | pass | pass | FAIL error | pass | FAIL error | FAIL error |
| `driver/psycopg-default/same-sql-both-connections` | pass | pass | FAIL error | pass | FAIL error | FAIL error |
| `driver/psycopg-default/set-leak` | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `driver/psycopg-default/transaction` | pass | pass | FAIL error | pass | FAIL error | FAIL error |
| `driver/psycopg-no-prepare/auto-prepare` | pass | pass | pass | pass | pass | pass |
| `driver/psycopg-no-prepare/same-names-different-sql` | pass | pass | pass | pass | pass | pass |
| `driver/psycopg-no-prepare/same-sql-both-connections` | pass | pass | pass | pass | pass | pass |
| `driver/psycopg-no-prepare/set-leak` | **FAIL** silent | pass | **FAIL** silent | **FAIL** silent | **FAIL** silent | FAIL error |
| `driver/psycopg-no-prepare/transaction` | pass | pass | pass | pass | pass | pass |

Documented limitations: built-in.

## Differences

### pgbouncer-1.26.1 / driver/node-postgres-default/set-leak (silent)

node-postgres 8.23.1 (Node.js), default settings (parameterized queries use the unnamed statement): A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`

### pgcat-1.2.0 / driver/node-postgres-default/set-leak (silent)

node-postgres 8.23.1 (Node.js), default settings (parameterized queries use the unnamed statement): A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`

### odyssey-1.5.2 / driver/node-postgres-default/set-leak (silent)

node-postgres 8.23.1 (Node.js), default settings (parameterized queries use the unnamed statement): A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`

### supavisor-2.10.0-rc.1 / driver/node-postgres-default/set-leak (silent)

node-postgres 8.23.1 (Node.js), default settings (parameterized queries use the unnamed statement): A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`

### supavisor-2.10.0-rc.1-strict / driver/node-postgres-default/set-leak (error)

node-postgres 8.23.1 (Node.js), default settings (parameterized queries use the unnamed statement): A session SET on one connection must not change what another connection sees.

- step 1: `[a] SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `

### pgcat-1.2.0 / driver/node-postgres-named/auto-prepare (error)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 2: `[a] SELECT ($1::int * 2)::text AS doubled with (21) (2 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 26000: `
- from https://node-postgres.com/features/queries#prepared-statements

### supavisor-2.10.0-rc.1 / driver/node-postgres-named/auto-prepare (error)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 9: `[b] SELECT ($1::int * 2)::text AS doubled with (21) (1 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 42P05: `
- from https://node-postgres.com/features/queries#prepared-statements

### supavisor-2.10.0-rc.1-strict / driver/node-postgres-named/auto-prepare (error)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 9: `[b] SELECT ($1::int * 2)::text AS doubled with (21) (1 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 42P05: `
- from https://node-postgres.com/features/queries#prepared-statements

### pgcat-1.2.0 / driver/node-postgres-named/same-names-different-sql (error)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: Two connections take turns with different queries, so drivers that number statements per connection give both the same name.

- step 3: `[a] SELECT 'from a ' || $1::int AS who with (1)`
- postgres: `ok rows=[(from a 1)] tags=[]`
- pooler: `error 26000: `
- from https://node-postgres.com/features/queries#prepared-statements

### pgcat-1.2.0 / driver/node-postgres-named/same-sql-both-connections (error)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 3: `[a] SELECT ($1::int + 100)::text AS shared with (1)`
- postgres: `ok rows=[(101)] tags=[]`
- pooler: `error 26000: `
- from https://node-postgres.com/features/queries#prepared-statements

### supavisor-2.10.0-rc.1 / driver/node-postgres-named/same-sql-both-connections (error)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 2: `[b] SELECT ($1::int + 100)::text AS shared with (2)`
- postgres: `ok rows=[(102)] tags=[]`
- pooler: `error 42P05: `
- from https://node-postgres.com/features/queries#prepared-statements

### supavisor-2.10.0-rc.1-strict / driver/node-postgres-named/same-sql-both-connections (error)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 2: `[b] SELECT ($1::int + 100)::text AS shared with (2)`
- postgres: `ok rows=[(102)] tags=[]`
- pooler: `error 42P05: `
- from https://node-postgres.com/features/queries#prepared-statements

### pgbouncer-1.26.1 / driver/node-postgres-named/set-leak (silent)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`
- from https://node-postgres.com/features/queries#prepared-statements

### pgcat-1.2.0 / driver/node-postgres-named/set-leak (silent)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`
- from https://node-postgres.com/features/queries#prepared-statements

### odyssey-1.5.2 / driver/node-postgres-named/set-leak (silent)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`
- from https://node-postgres.com/features/queries#prepared-statements

### supavisor-2.10.0-rc.1 / driver/node-postgres-named/set-leak (silent)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`
- from https://node-postgres.com/features/queries#prepared-statements

### supavisor-2.10.0-rc.1-strict / driver/node-postgres-named/set-leak (error)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: A session SET on one connection must not change what another connection sees.

- step 1: `[a] SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `
- from https://node-postgres.com/features/queries#prepared-statements

### pgcat-1.2.0 / driver/node-postgres-named/transaction (error)

node-postgres 8.23.1 (Node.js), named queries ({ name, text, values }), one name per query text, as apps use them: Queries that cross the auto-prepare threshold inside a transaction, then another connection reads what it committed.

- step 11: `[b] SELECT count(*)::text AS committed FROM pp_driver_txn WHERE x = $1::int with (1) (2 of 6)`
- postgres: `ok rows=[(6)] tags=[]`
- pooler: `error 26000: `
- from https://node-postgres.com/features/queries#prepared-statements

### supavisor-2.10.0-rc.1 / driver/pgjdbc-default/auto-prepare (error)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 13: `[b] SELECT ($1::int * 2)::text AS doubled with (21) (5 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-default/auto-prepare (error)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 1: `[a] SELECT ($1::int * 2)::text AS doubled with (21) (1 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error XX000: `

### supavisor-2.10.0-rc.1 / driver/pgjdbc-default/same-names-different-sql (error)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): Two connections take turns with different queries, so drivers that number statements per connection give both the same name.

- step 10: `[b] SELECT 'from b ' || $1::int AS who with (2)`
- postgres: `ok rows=[(from b 2)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-default/same-names-different-sql (error)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): Two connections take turns with different queries, so drivers that number statements per connection give both the same name.

- step 1: `[a] SELECT 'from a ' || $1::int AS who with (1)`
- postgres: `ok rows=[(from a 1)] tags=[]`
- pooler: `error XX000: `

### supavisor-2.10.0-rc.1 / driver/pgjdbc-default/same-sql-both-connections (error)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 10: `[b] SELECT ($1::int + 100)::text AS shared with (2)`
- postgres: `ok rows=[(102)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-default/same-sql-both-connections (error)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 1: `[a] SELECT ($1::int + 100)::text AS shared with (1)`
- postgres: `ok rows=[(101)] tags=[]`
- pooler: `error XX000: `

### pgbouncer-1.26.1 / driver/pgjdbc-default/set-leak (silent)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`

### pgcat-1.2.0 / driver/pgjdbc-default/set-leak (silent)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`

### odyssey-1.5.2 / driver/pgjdbc-default/set-leak (silent)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`

### supavisor-2.10.0-rc.1 / driver/pgjdbc-default/set-leak (silent)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-default/set-leak (error)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): A session SET on one connection must not change what another connection sees.

- step 1: `[a] SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `

### supavisor-2.10.0-rc.1 / driver/pgjdbc-default/transaction (error)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): Queries that cross the auto-prepare threshold inside a transaction, then another connection reads what it committed.

- step 14: `[b] SELECT count(*)::text AS committed FROM pp_driver_txn WHERE x = $1::int with (1) (5 of 6)`
- postgres: `ok rows=[(6)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-default/transaction (error)

pgjdbc 42.7.14 (Java), default settings (prepareThreshold = 5: server-side prepare from the fifth run): Queries that cross the auto-prepare threshold inside a transaction, then another connection reads what it committed.

- step 1: `[a] BEGIN`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-no-prepare/auto-prepare (error)

pgjdbc 42.7.14 (Java), prepareThreshold = 0, which turns server-side prepared statements off: The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 1: `[a] SELECT ($1::int * 2)::text AS doubled with (21) (1 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error XX000: `
- from https://jdbc.postgresql.org/documentation/server-prepare/

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-no-prepare/same-names-different-sql (error)

pgjdbc 42.7.14 (Java), prepareThreshold = 0, which turns server-side prepared statements off: Two connections take turns with different queries, so drivers that number statements per connection give both the same name.

- step 1: `[a] SELECT 'from a ' || $1::int AS who with (1)`
- postgres: `ok rows=[(from a 1)] tags=[]`
- pooler: `error XX000: `
- from https://jdbc.postgresql.org/documentation/server-prepare/

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-no-prepare/same-sql-both-connections (error)

pgjdbc 42.7.14 (Java), prepareThreshold = 0, which turns server-side prepared statements off: Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 1: `[a] SELECT ($1::int + 100)::text AS shared with (1)`
- postgres: `ok rows=[(101)] tags=[]`
- pooler: `error XX000: `
- from https://jdbc.postgresql.org/documentation/server-prepare/

### pgbouncer-1.26.1 / driver/pgjdbc-no-prepare/set-leak (silent)

pgjdbc 42.7.14 (Java), prepareThreshold = 0, which turns server-side prepared statements off: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`
- from https://jdbc.postgresql.org/documentation/server-prepare/

### pgcat-1.2.0 / driver/pgjdbc-no-prepare/set-leak (silent)

pgjdbc 42.7.14 (Java), prepareThreshold = 0, which turns server-side prepared statements off: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`
- from https://jdbc.postgresql.org/documentation/server-prepare/

### odyssey-1.5.2 / driver/pgjdbc-no-prepare/set-leak (silent)

pgjdbc 42.7.14 (Java), prepareThreshold = 0, which turns server-side prepared statements off: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`
- from https://jdbc.postgresql.org/documentation/server-prepare/

### supavisor-2.10.0-rc.1 / driver/pgjdbc-no-prepare/set-leak (silent)

pgjdbc 42.7.14 (Java), prepareThreshold = 0, which turns server-side prepared statements off: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`
- from https://jdbc.postgresql.org/documentation/server-prepare/

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-no-prepare/set-leak (error)

pgjdbc 42.7.14 (Java), prepareThreshold = 0, which turns server-side prepared statements off: A session SET on one connection must not change what another connection sees.

- step 1: `[a] SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `
- from https://jdbc.postgresql.org/documentation/server-prepare/

### supavisor-2.10.0-rc.1-strict / driver/pgjdbc-no-prepare/transaction (error)

pgjdbc 42.7.14 (Java), prepareThreshold = 0, which turns server-side prepared statements off: Queries that cross the auto-prepare threshold inside a transaction, then another connection reads what it committed.

- step 1: `[a] BEGIN`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `
- from https://jdbc.postgresql.org/documentation/server-prepare/

### pgcat-1.2.0 / driver/pgx-default/auto-prepare (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 1: `[a] SELECT ($1::int * 2)::text AS doubled with (21) (1 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 26000: `

### supavisor-2.10.0-rc.1 / driver/pgx-default/auto-prepare (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 9: `[b] SELECT ($1::int * 2)::text AS doubled with (21) (1 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/pgx-default/auto-prepare (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 9: `[b] SELECT ($1::int * 2)::text AS doubled with (21) (1 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 42P05: `

### pgcat-1.2.0 / driver/pgx-default/same-names-different-sql (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): Two connections take turns with different queries, so drivers that number statements per connection give both the same name.

- step 1: `[a] SELECT 'from a ' || $1::int AS who with (1)`
- postgres: `ok rows=[(from a 1)] tags=[]`
- pooler: `error 26000: `

### pgcat-1.2.0 / driver/pgx-default/same-sql-both-connections (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 1: `[a] SELECT ($1::int + 100)::text AS shared with (1)`
- postgres: `ok rows=[(101)] tags=[]`
- pooler: `error 26000: `

### supavisor-2.10.0-rc.1 / driver/pgx-default/same-sql-both-connections (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 2: `[b] SELECT ($1::int + 100)::text AS shared with (2)`
- postgres: `ok rows=[(102)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/pgx-default/same-sql-both-connections (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 2: `[b] SELECT ($1::int + 100)::text AS shared with (2)`
- postgres: `ok rows=[(102)] tags=[]`
- pooler: `error 42P05: `

### pgbouncer-1.26.1 / driver/pgx-default/set-leak (silent)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`

### pgcat-1.2.0 / driver/pgx-default/set-leak (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): A session SET on one connection must not change what another connection sees.

- step 1: `[a] SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[]`
- pooler: `error 26000: `

### odyssey-1.5.2 / driver/pgx-default/set-leak (silent)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`

### supavisor-2.10.0-rc.1 / driver/pgx-default/set-leak (silent)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`

### supavisor-2.10.0-rc.1-strict / driver/pgx-default/set-leak (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): A session SET on one connection must not change what another connection sees.

- step 1: `[a] SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `

### pgcat-1.2.0 / driver/pgx-default/transaction (error)

pgx 5.11.0 (Go), default settings (QueryExecModeCacheStatement: every query is prepared and cached): Queries that cross the auto-prepare threshold inside a transaction, then another connection reads what it committed.

- step 1: `[a] BEGIN`
- postgres: `ok rows=[] tags=[]`
- pooler: `error 26000: `

### pgbouncer-1.26.1 / driver/pgx-exec/set-leak (silent)

pgx 5.11.0 (Go), QueryExecModeExec: unnamed statements only, which pgx documents as working with PgBouncer: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`
- from https://pkg.go.dev/github.com/jackc/pgx/v5@v5.11.0#QueryExecMode

### pgcat-1.2.0 / driver/pgx-exec/set-leak (silent)

pgx 5.11.0 (Go), QueryExecModeExec: unnamed statements only, which pgx documents as working with PgBouncer: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`
- from https://pkg.go.dev/github.com/jackc/pgx/v5@v5.11.0#QueryExecMode

### odyssey-1.5.2 / driver/pgx-exec/set-leak (silent)

pgx 5.11.0 (Go), QueryExecModeExec: unnamed statements only, which pgx documents as working with PgBouncer: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`
- from https://pkg.go.dev/github.com/jackc/pgx/v5@v5.11.0#QueryExecMode

### supavisor-2.10.0-rc.1 / driver/pgx-exec/set-leak (silent)

pgx 5.11.0 (Go), QueryExecModeExec: unnamed statements only, which pgx documents as working with PgBouncer: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`
- from https://pkg.go.dev/github.com/jackc/pgx/v5@v5.11.0#QueryExecMode

### supavisor-2.10.0-rc.1-strict / driver/pgx-exec/set-leak (error)

pgx 5.11.0 (Go), QueryExecModeExec: unnamed statements only, which pgx documents as working with PgBouncer: A session SET on one connection must not change what another connection sees.

- step 1: `[a] SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `
- from https://pkg.go.dev/github.com/jackc/pgx/v5@v5.11.0#QueryExecMode

### pgcat-1.2.0 / driver/psycopg-default/auto-prepare (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 6: `[a] SELECT ($1::int * 2)::text AS doubled with (21) (6 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 26000: `

### supavisor-2.10.0-rc.1 / driver/psycopg-default/auto-prepare (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 14: `[b] SELECT ($1::int * 2)::text AS doubled with (21) (6 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/psycopg-default/auto-prepare (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: The same parameterized query run more times than the driver's auto-prepare threshold, then the same on another connection.

- step 14: `[b] SELECT ($1::int * 2)::text AS doubled with (21) (6 of 8)`
- postgres: `ok rows=[(42)] tags=[]`
- pooler: `error 42P05: `

### pgcat-1.2.0 / driver/psycopg-default/same-names-different-sql (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: Two connections take turns with different queries, so drivers that number statements per connection give both the same name.

- step 11: `[a] SELECT 'from a ' || $1::int AS who with (1)`
- postgres: `ok rows=[(from a 1)] tags=[]`
- pooler: `error 26000: `

### supavisor-2.10.0-rc.1 / driver/psycopg-default/same-names-different-sql (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: Two connections take turns with different queries, so drivers that number statements per connection give both the same name.

- step 12: `[b] SELECT 'from b ' || $1::int AS who with (2)`
- postgres: `ok rows=[(from b 2)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/psycopg-default/same-names-different-sql (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: Two connections take turns with different queries, so drivers that number statements per connection give both the same name.

- step 12: `[b] SELECT 'from b ' || $1::int AS who with (2)`
- postgres: `ok rows=[(from b 2)] tags=[]`
- pooler: `error 42P05: `

### pgcat-1.2.0 / driver/psycopg-default/same-sql-both-connections (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 11: `[a] SELECT ($1::int + 100)::text AS shared with (1)`
- postgres: `ok rows=[(101)] tags=[]`
- pooler: `error 26000: `

### supavisor-2.10.0-rc.1 / driver/psycopg-default/same-sql-both-connections (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 12: `[b] SELECT ($1::int + 100)::text AS shared with (2)`
- postgres: `ok rows=[(102)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/psycopg-default/same-sql-both-connections (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: Two connections take turns with the same query, so drivers that name statements by their text prepare the same name twice.

- step 12: `[b] SELECT ($1::int + 100)::text AS shared with (2)`
- postgres: `ok rows=[(102)] tags=[]`
- pooler: `error 42P05: `

### pgbouncer-1.26.1 / driver/psycopg-default/set-leak (silent)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`

### pgcat-1.2.0 / driver/psycopg-default/set-leak (silent)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`

### odyssey-1.5.2 / driver/psycopg-default/set-leak (silent)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`

### supavisor-2.10.0-rc.1 / driver/psycopg-default/set-leak (silent)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`

### supavisor-2.10.0-rc.1-strict / driver/psycopg-default/set-leak (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: A session SET on one connection must not change what another connection sees.

- step 1: `[a] SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `

### pgcat-1.2.0 / driver/psycopg-default/transaction (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: Queries that cross the auto-prepare threshold inside a transaction, then another connection reads what it committed.

- step 15: `[b] SELECT count(*)::text AS committed FROM pp_driver_txn WHERE x = $1::int with (1) (6 of 6)`
- postgres: `ok rows=[(6)] tags=[]`
- pooler: `error 26000: `

### supavisor-2.10.0-rc.1 / driver/psycopg-default/transaction (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: Queries that cross the auto-prepare threshold inside a transaction, then another connection reads what it committed.

- step 15: `[b] SELECT count(*)::text AS committed FROM pp_driver_txn WHERE x = $1::int with (1) (6 of 6)`
- postgres: `ok rows=[(6)] tags=[]`
- pooler: `error 42P05: `

### supavisor-2.10.0-rc.1-strict / driver/psycopg-default/transaction (error)

psycopg 3.3.6 (Python), default settings (prepare_threshold = 5), with autocommit on as most apps use it: Queries that cross the auto-prepare threshold inside a transaction, then another connection reads what it committed.

- step 15: `[b] SELECT count(*)::text AS committed FROM pp_driver_txn WHERE x = $1::int with (1) (6 of 6)`
- postgres: `ok rows=[(6)] tags=[]`
- pooler: `error 42P05: `

### pgbouncer-1.26.1 / driver/psycopg-no-prepare/set-leak (silent)

psycopg 3.3.6 (Python), prepare_threshold = None, as psycopg's docs advise for transaction pooling: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`
- from https://www.psycopg.org/psycopg3/docs/advanced/prepare.html#using-prepared-statements-with-pgbouncer

### pgcat-1.2.0 / driver/psycopg-no-prepare/set-leak (silent)

psycopg 3.3.6 (Python), prepare_threshold = None, as psycopg's docs advise for transaction pooling: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`
- from https://www.psycopg.org/psycopg3/docs/advanced/prepare.html#using-prepared-statements-with-pgbouncer

### odyssey-1.5.2 / driver/psycopg-no-prepare/set-leak (silent)

psycopg 3.3.6 (Python), prepare_threshold = None, as psycopg's docs advise for transaction pooling: A session SET on one connection must not change what another connection sees.

- step 3: `[a] SHOW statement_timeout`
- postgres: `ok rows=[(5s)] tags=[]`
- pooler: `ok rows=[(0)] tags=[]`
- from https://www.psycopg.org/psycopg3/docs/advanced/prepare.html#using-prepared-statements-with-pgbouncer

### supavisor-2.10.0-rc.1 / driver/psycopg-no-prepare/set-leak (silent)

psycopg 3.3.6 (Python), prepare_threshold = None, as psycopg's docs advise for transaction pooling: A session SET on one connection must not change what another connection sees.

- step 2: `[b] SHOW statement_timeout`
- postgres: `ok rows=[(0)] tags=[]`
- pooler: `ok rows=[(5s)] tags=[]`
- from https://www.psycopg.org/psycopg3/docs/advanced/prepare.html#using-prepared-statements-with-pgbouncer

### supavisor-2.10.0-rc.1-strict / driver/psycopg-no-prepare/set-leak (error)

psycopg 3.3.6 (Python), prepare_threshold = None, as psycopg's docs advise for transaction pooling: A session SET on one connection must not change what another connection sees.

- step 1: `[a] SET statement_timeout = '5s'`
- postgres: `ok rows=[] tags=[]`
- pooler: `error XX000: `
- from https://www.psycopg.org/psycopg3/docs/advanced/prepare.html#using-prepared-statements-with-pgbouncer
