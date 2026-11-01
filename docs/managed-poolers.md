# Testing a managed pooler

How to point poolproof at a pooler you don't run yourself: Amazon RDS Proxy, hosted
Supavisor on Supabase, or any other managed pooler. The poolproof side of these recipes
(targets file, reset, doctor, verdicts) is what the code does. The provider side (hosts,
ports, users, certificates) comes from each provider's documentation and is not tested in
poolproof's CI, so check it against your provider's docs if something doesn't match.

## The same for every provider

1. **Use a scratch database or a throwaway project.** Scenarios create and drop their own
   objects (`pp_*` tables, temp tables, advisory locks, a `pp_role` when the user may
   create roles), and every reset ends sessions. Create the database as the user you test
   with, so it owns the `public` schema and can create tables in it (Postgres 15 and later
   don't let other users create objects there).
2. **Use one role for both URLs, and not a superuser.** The pooler URL and the `direct`
   URL should log in as the same role (or the `direct` role should be a member of the
   pooler's). A role can always see and end the sessions of roles whose privileges it
   has, so the reset needs no extra privileges. Otherwise the `direct` role needs
   `pg_signal_backend` and `pg_read_all_stats` to see and end the pooler's sessions;
   without them, `run` resets nothing and marks every scenario ERROR. Not a superuser,
   because a superuser's reset ends every session on the database, other superusers'
   included. And only a superuser can end a superuser's sessions, so a pooler that logs
   in as one can't be reset by a role that isn't one.
3. **Pick `reset = "all"` and pass `--yes`.** A managed pooler usually reaches Postgres
   from several addresses (one per node or zone), and `"auto"` only ends the sessions
   from the one address it sees when it asks through the pooler. `"all"` ends every other
   session on the database that the `direct` user may end. Against a Postgres that isn't
   on your machine it needs `--yes`; without it, every scenario for that target is an
   ERROR that says so.
4. **Run `doctor` first.** It only reads: no reset, no setup, no scenario. It shows
   whether both URLs reach the same Postgres, TLS on each side, which scenarios would SKIP
   for missing privileges, what the reset would end and whether it can, and what the pool
   does when a second client connects.
5. **Then run, and keep the report.** `--format html` writes one page with every step's
   trace, to share with your team or attach to an issue.

A targets file, `managed.toml`:

```toml
[[target]]
name = "my-pooler"
pooler = "postgres://app@pooler-host:6432/pp_scratch?sslmode=verify-full"
direct = "postgres://app@postgres-host:5432/pp_scratch?sslmode=verify-full"
mode = "transaction"
reset = "all"
```

The file leaves out `product`, `version` and `config` (README, "Documented
limitations"). With `config = "default"`, FAILs that match the built-in entries for that
product are marked documented, but those entries were checked on the self-hosted
defaults, and a managed pooler's version and settings may differ. Leave `product` out,
or set it with a `config` label of your own (`config = "supabase"`) so that only your
own entries (`--limitations DIR`) apply.

```sh
export PGPASSWORD=...           # used for both URLs, kept out of `ps`
poolproof doctor --targets managed.toml --yes
poolproof run --targets managed.toml --yes --format html > my-pooler.html
```

If the two URLs need different passwords, put `${POOLER_PASSWORD}` in the pooler URL and
let the `direct` one come from `PGPASSWORD` or `~/.pgpass`.

## Amazon RDS Proxy

Not tested in poolproof's CI.

- **Where to run it.** An RDS Proxy can't be reached from outside its VPC, so run
  poolproof on a machine inside it (an EC2 instance or a bastion), where it reaches both
  the proxy and the database instance.
- **URLs.** `pooler` is the proxy endpoint, `direct` is the instance (or cluster writer)
  endpoint, both on port 5432 with the same scratch database. Log in with the user and
  password in the secret the proxy uses, on both URLs. `mode = "transaction"`: the proxy
  hands a server connection to another client between transactions unless something
  pinned it.
- **TLS.** The database instance's certificate chains to the RDS certificate bundle:
  `sslmode=verify-full&sslrootcert=/path/to/global-bundle.pem`. The proxy's certificate
  comes from AWS Certificate Manager, so the operating system's store checks it:
  `sslrootcert=system`, which implies `verify-full`.
- **Reset.** RDS has no superuser you can log in as. The master user is a member of
  `rds_superuser`, which is not a superuser to Postgres, so poolproof treats it like any
  other role. On RDS the master user (`rds_superuser`) usually holds both
  `pg_signal_backend` and `pg_read_all_stats` (through `pg_monitor`); check with
  `doctor`. Otherwise use one role on both URLs: then `reset = "all"` needs neither, and
  ends that role's other sessions on the scratch database. If the proxy logs in as a
  different role and the `direct` user lacks either privilege, every scenario is an
  ERROR with a reason like `reset = "all" can't end the pooler's sessions: this user
  can't end sessions of proxy_user without pg_signal_backend and pg_read_all_stats; as a
  superuser: GRANT pg_signal_backend, pg_read_all_stats TO "app"` (naming only what is
  missing). There is no superuser on RDS; the master user may be able to run that
  `GRANT` (check its privileges with `doctor`), otherwise use one role on both URLs.
  `doctor` shows the same refusal as a failed `reset` check.
- **SKIP.** `load-isolated-between-clients` needs a superuser, so it always SKIPs. The
  `SET ROLE` scenarios need `CREATEROLE` on the `direct` user.

## Hosted Supavisor (Supabase)

Not tested in poolproof's CI. poolproof's own results cover self-hosted Supavisor
2.9.13 and 2.10.0-rc.1 (`RESULTS.md`); the hosted one may run another version or
configuration, so compare its verdicts with those rather than expect the same.

- **Use a throwaway project.** The pooler routes to the project's `postgres` database, so
  there may be no scratch database to point it at. On that database, `reset = "all"`
  also ends the sessions of the project's own services that the user may end, which then
  reconnect. Never point it at a project anything depends on.
- **URLs.** `pooler` is the transaction-mode pooler string from the project's connect
  dialog (port 6543, user `postgres.<project ref>`). `direct` is the direct connection
  (`db.<project ref>.supabase.co:5432`, user `postgres`). Postgres sees the same role
  both ways, so the reset needs no extra privileges; `doctor`'s `reset` line confirms it.
  The direct host may resolve to IPv6 only, so run from a network with IPv6 or use the
  provider's IPv4 option. `mode = "transaction"`.
- **TLS.** `sslmode=verify-full` with `sslrootcert` set to the CA certificate from the
  project's database settings, on both URLs.
- **SKIP.** The `postgres` role isn't a superuser, so `load-isolated-between-clients`
  SKIPs. The `SET ROLE` scenarios need `CREATEROLE`.

## Other managed poolers

The same steps apply: one role on both URLs, `reset = "all"` with `--yes` on a scratch
database, `doctor` first. `direct` has to reach the same Postgres the pooler does;
`doctor`'s `same server` check compares the database, start time, version and port
seen through each URL, and fails when they differ.

## What to expect

- **Clients may not share a server connection.** poolproof's test bed gives each pooler
  one server connection, so two clients always share it. A managed pooler usually has
  more. A scenario that checks one client's state doesn't reach another only tests
  something when they land on the same server connection. Its evidence steps check that
  they did; otherwise the run doesn't count, and if no run counted, the scenario is a
  SKIP that starts "passed, but". `doctor`'s `mode` line shows what happened when it
  tried: `b got a's server connection` means they share, `b got another server
  connection` means expect those SKIPs. If the provider lets you set the pool size, a
  pool of one on a throwaway project makes the results comparable to `RESULTS.md`.
- **Pool size.** With more than one server connection, `abandoned-query-does-not-block-pool`
  and `cancel-under-saturated-pool` SKIP ("needs one server connection"); with only one,
  the four `*-survives-server-change` scenarios SKIP ("needs several server
  connections"). `run` says at the end which pool size runs them.
- **Latency.** Each step may take 10 seconds before it counts as a hang. Over a slow
  link, raise it with `--step-timeout 30s`. `--check-scenarios` runs each scenario twice
  on plain Postgres first and makes it an ERROR if the two runs differ, which catches
  timing-sensitive scenarios that the network makes unreliable.
- **Verdicts.** PASS: the pooler behaved like Postgres. FAIL: some step differed, marked
  silent (wrong result, no error), error (an error Postgres didn't give) or hang (no
  reply within the step timeout, or replies cut short). SKIP: not applicable, or missing
  privileges. ERROR: nothing was compared, for example an unreachable URL or a refused
  reset. `-v` prints every step's outcome on both sides.
- **Exit code.** 1 on any FAIL or ERROR, else 0. `--fail-on silent` or `--fail-on error`
  counts only those kinds of FAIL (ERROR verdicts still count), `--fail-on none` never
  fails. With `--compare-to last.json` (a saved `--format json` report), only
  regressions against that run count, so a scheduled run can fail on new problems and
  not on ones you already know.
- **Sessions ending.** Every reset ends the `direct` user's other sessions on the scratch
  database, and the pooler's server connections with them. The pooler reconnects, and
  poolproof waits for it to answer before each run.
