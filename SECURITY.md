# Security

## Reporting a vulnerability

Use GitHub private vulnerability reporting: the repository's **Security** tab, then
**Report a vulnerability**. Please don't open a public issue for it. You should get a
reply within a week. Fixes go into the next release and the advisory is published with it.

Only the latest release is supported.

A pooler bug that poolproof finds is not a poolproof vulnerability. Report those to the
pooler's project.

## What poolproof does to a database

poolproof is a test tool. Point it at a scratch database, never at one with data you care
about or sessions you can't lose.

`poolproof run` connects to every URL it is given, through the pooler and directly, and:

- creates and drops its own objects: tables, temp tables and roles named `pp_*`.
  The `SET ROLE` scenarios run `CREATE ROLE pp_role` and `GRANT pp_role TO CURRENT_USER`,
  so the connecting user is a member of that role until teardown drops it. Scenarios
  that need a privilege say so in `requires`, and targets without it skip them;
- takes session advisory locks, runs `LISTEN`/`NOTIFY`, `SET`, `SET ROLE`, `DISCARD ALL`,
  `COPY FROM STDIN`, prepared statements and cursors;
- runs `pg_sleep` for up to 15 seconds and sends CancelRequests for its own queries;
- drops connections mid-query on purpose, to test abandoned clients;
- runs each scenario's `setup` and `teardown` SQL, and a target's `reset`. `reset = "all"`
  **terminates every other session on the database** that the `direct` user may end
  (against a Postgres on another machine only with `run --yes`); `reset = "auto"`
  terminates every session from the address and user the pooler connects as, which can
  include other applications behind the same NAT or pooler. `run` prints what a reset will
  end before it runs. Scenario and targets files are code: only run ones you trust.

`poolproof scan` and `poolproof report` only read the files they are given. They don't
connect to anything.

## Passwords

A URL without a password gets one the way psql does: from `PGPASSWORD`, else from the
password file (`PGPASSFILE`, default `~/.pgpass`; on Unix it is ignored unless only you
can read it). In a targets file, `${NAME}` takes the value from the environment variable
`NAME`. Prefer these: a password in a URL on the command line is visible to `ps` and
shell history, and one in a targets file to anyone who can read the file.

Output never shows passwords: URLs in errors and reports are printed without them, and
the JSON report has no URLs at all. Postgres itself may log a failing `reset` statement
with any `${NAME}` value in it, so keep secrets in URLs, not in `reset`.

## What poolproof never does

- No telemetry, no update checks. The only network connections are to the URLs you pass.
- The JSON report doesn't contain connection URLs or passwords.

## Known limitations

- TLS: the default, `sslmode=prefer`, encrypts when the server offers TLS but never
  checks its certificate, and falls back to plaintext without a word. `require` checks
  the chain only when a root certificate is given or found (`~/.postgresql/root.crt`, or
  `%APPDATA%\postgresql\root.crt` on Windows).
  For anything not on localhost, use `verify-full`. `sslrootcert=system` requires
  `verify-full`.
- Not supported: client certificates (the connection fails and says so) and
  `sslnegotiation=direct` (or `PGSSLNEGOTIATION=direct`; refused when the targets load).
  `send` clients don't use SCRAM channel binding.
- With SCRAM the password itself never goes over the wire. If the server asks for a
  clear-text password, poolproof sends it, also to anyone impersonating the server when
  the certificate isn't checked (`prefer`, or `require` without a root certificate).
- An HTML report shows links from the scenario files it was built from. Open a report
  only if you trust where its results came from.
