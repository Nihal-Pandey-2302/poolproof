# Testing your pooler

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

For RDS Proxy, hosted Supavisor and other managed poolers, see
[managed-poolers.md](managed-poolers.md): which URLs, which reset, and what to
expect.

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
as in psql. Not supported yet: client certificates.

A URL names one host and port: several hosts or ports, `hostaddr`, `target_session_attrs`
and `load_balance_hosts` are refused, since poolproof would otherwise pick a host on its
own. So is `sslnegotiation=direct` (or `PGSSLNEGOTIATION=direct`), since `send` clients
always start with SSLRequest. Socket options work as in psql: `connect_timeout`,
`keepalives` (on unless `keepalives=0`) with `keepalives_idle`, `keepalives_interval` and
`keepalives_retries`, and `tcp_user_timeout` in milliseconds. One the system can't set is
an error rather than ignored.


## Targets file

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
# What the pooler is: the product as poolproof names it (pgbouncer, pgdog, supavisor,
# pgcat, odyssey), its version, and a label for its settings.
product = "pgbouncer"
version = "1.26.1"
config = "default"
```

`product`, `version` and `config` label the target in reports, and `--compare-to` warns
when they differ from the saved run's. Documented limitations only match a target that
sets all three, with `config = "default"` for a pooler with its default settings (or
another config an entry lists) and a version poolproof can compare, such as `1.26.1` or
`2.10.0-rc.1`; `latest` or `v1.26.1` matches no entry. Give a pooler with changed
settings a label of your own, so entries about the defaults don't apply to it. A
target that sets `product` but not `config` gets a warning, since no entry can match
it. `run --pooler` takes them as `--product`, `--version` and `--config` (`default`
unless given), and names the target after them, e.g. `pgbouncer-1.26.1`, or `quick`
without `--product`. `--target` and `--compare-to` go by that name, so a saved quick run
with `--product` and one without never match.

## Probes

Before its first scenario, each target is probed twice: a second client connects while
the first is connected and idle, and again while the first is inside a transaction. The
results are in the JSON report (`idle_probe`, `busy_probe`) and in `doctor`. A probe
that contradicts the declared `mode` makes the target ERROR: in transaction or statement
mode the second client must not wait for the first (or be refused while it's
connected), and in session mode it must not get the first one's server connection.
Neither may ever get it inside an open transaction. A pool with room for both fits every
mode. In session mode with one server connection, a scenario whose pooled clients hold
two connections at once is skipped: it would only show the pool waiting. So is it when
the probe can't tell. A second client counts as waiting only if, once the first client
closes, it gets the first one's backend, and does again in a second round right after
(a pool still opening its second server can hand the first one's over once); anything
else is unknown. A second client the pooler refuses with an error while the first holds
its connection counts the same way: only if, reconnecting once the first is gone, it gets
the first one's backend, in both rounds. Some poolers accept any connect and refuse at
the query, so getting in again proves nothing by itself.
`ignore_server_requirements = true` on a target (`--ignore-server-requirements` with
`run --pooler`) runs scenarios whose `one_server_connection` or
`several_server_connections` isn't met anyway, for Postgres standing in for its own
pooler.

## Reset

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
- `"exact"` ends only the pooler's own server connections, so neighbours with the same
  user and address (a NAT, a sidecar) are left alone. It needs `pool_size`, the pool
  size the pooler has for this database and user (1 to 16), and a mode that can hold a
  transaction (not statement). Before each run it holds `pool_size` transactions through
  the pooler, each reading its own backend's pid and start time, ends exactly those
  backends, then holds them again to learn the fresh ones, so a server still busy at the
  next reset (an abandoned query) is ended too. In session mode the held clients
  disconnect to give their servers back, and the pooler runs its own reset query before
  the next client gets one. Before the first scenario it checks that the pool really is
  that size: `pool_size` transactions can be held, and one client more waits (or is
  refused) until they end. If not, or if a reset can't account for every server
  connection, the target is an ERROR, never a partial reset; the message lists the
  sessions of the pooler's user and address it didn't see, which it never ends.
  `doctor` runs the same check and shows the pids a reset would end, ending nothing.
- A list of statements runs as written.
- Without `reset`, nothing is reset: leaks carry over between scenarios, and `run`
  warns about it.

With `--pooler` and `--direct` instead of a targets file, `--reset auto`, `--reset all` or
`--reset exact --pool-size N` picks the reset.

For `"exact"`, `pool_size` must be every server connection the pooler may open for the
database and user, extra pools included:

| Pooler | `pool_size` is | Also |
|---|---|---|
| PgBouncer | `default_pool_size` (or the database's `pool_size`) | `reserve_pool_size = 0`, the default: a reserve pool adds connections |
| PgDog | `default_pool_size` (or the user's `pool_size`) | |
| PgCat | the pool's `pool_size` | |
| Odyssey | the route's `pool_size` | |
| Supavisor | the tenant user's `pool_size` + 1 | Supavisor's default warm pool allows one extra server connection, so set `pool_size` to the tenant's `pool_size` + 1 (`pool_size` for users in `no_warm_pool_users`) |

A user that isn't a superuser needs `pg_signal_backend` and `pg_read_all_stats` to end
other roles' sessions, and can never end a superuser's. Without `pg_read_all_stats`,
Postgres hides the details of other roles' sessions from it, so a reset would match
nothing. Unless the `direct` user is a superuser, `"auto"` and `"all"` therefore check the
role the pooler connects as, and refuse to run when the reset couldn't end its sessions,
naming what is missing and the `GRANT` that adds it. `"all"` then leaves alone the
sessions it may not end. For your own statements, `run` warns when they terminate sessions
without those privileges.

Before the first scenario, `run` prints what the reset will touch, e.g.
`reset: 'prod-like-pgbouncer': ends every session of user app from 10.0.0.7 (not only the
pooler's) on database db at postgres-host:5432 (auto)`. The JSON report records which
kind each target used.

## doctor

`poolproof doctor` (same `--targets`, `--target`, `--pooler`/`--direct` and `--reset`
flags as `run`) checks a target before you run anything:

```text
$ poolproof doctor --target prod-like-pgbouncer
ok    prod-like-pgbouncer direct             connected to postgres://user@postgres-host:5432/db
ok    prod-like-pgbouncer direct tls         TLS (TLSv1.3, sslmode=verify-full)
ok    prod-like-pgbouncer pooler             connected to postgres://user@pooler-host:6432/db
warn  prod-like-pgbouncer pooler tls         plaintext: sslmode=prefer and the server declined TLS; set sslmode=require to insist
ok    prod-like-pgbouncer same server        database db, port 5432, started 2026-10-31 07:59:09.418230, 17.11 (addr 10.0.0.5 via pooler, 10.0.0.5 direct)
ok    prod-like-pgbouncer privileges         every scenario's requirements are met
ok    prod-like-pgbouncer reset              ends every session of user app from 10.0.0.7 (not only the pooler's) on database db at postgres-host:5432 (auto); matching sessions now: 1
ok    prod-like-pgbouncer mode               transaction or statement pooling (b got a's server connection)
ok    prod-like-pgbouncer server connections one (b waited while a was in a transaction)
```

It checks that both URLs answer and reach the same Postgres (database, start time, version
and port; the addresses are only shown, since the pooler often reaches Postgres on another
network), TLS on each side, which scenarios would SKIP for missing privileges, what reset
would end and whether the direct user can end it, and the pooling mode: a second client
gets the first one's server connection (transaction pooling), or waits for it (session
mode with a full pool), and how many server connections the pool has: whether a second
client waits (or is refused) while the first is inside a transaction, as `run`'s probes
see it ([above](#probes)). A user that isn't a superuser needs `pg_signal_backend` and
`pg_read_all_stats` to end other roles' sessions, and can never end a superuser's. If that
rules out the pooler's own sessions, the reset check fails; other sessions that
`reset = "all"` would leave are a warning, since whatever they hold (locks, `pp_*`
objects) can still change results. doctor only reads: it runs no reset, setup or scenario,
ends no session and sets nothing through the pooler. Exit code 1 if a check fails (with
`--strict`, also on a warning). `--format json` is for scripts and issues: it leaves out
URLs, hosts, users and addresses, which the text output shows. Text output also shows
server and TLS error messages in full; JSON shows only their class (e.g. `authentication
(28P01)`, `TLS certificate`, `timeout`).

`--target NAME` runs only that target from the file (repeat it for several).

Scenarios create and drop their own objects (prefixed `pp_`), so use a scratch database.
That matters even more with `reset = "all"`, which kills every session on it.
`--reverse` runs the scenarios in reverse order; verdicts should not change.
