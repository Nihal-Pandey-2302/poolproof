# Security

## Reporting a vulnerability

Use GitHub private vulnerability reporting: the repository's **Security** tab, then
**Report a vulnerability**. Please don't open a public issue for it. You should get a reply
within a week. Fixes go into the next release and the advisory is published with it.

Only the latest release is supported.

A pooler bug that poolproof finds is not a poolproof vulnerability. Report those to the
pooler's project.

## What poolproof does to a database

poolproof is a test tool. Point it at a scratch database, never at one with data you care
about or sessions you can't lose.

`poolproof run` connects to every URL it is given, through the pooler and directly, and:

- creates and drops its own objects: tables, temp tables and roles named `pp_*`
  (`CREATE ROLE` needs that privilege);
- takes session advisory locks, runs `LISTEN`/`NOTIFY`, `SET`, `SET ROLE`, `DISCARD ALL`,
  `COPY FROM STDIN`, prepared statements and cursors;
- runs `pg_sleep` for up to 15 seconds and sends CancelRequests for its own queries;
- drops connections mid-query on purpose, to test abandoned clients;
- runs each scenario's `setup` and `teardown` SQL, and a target's `reset` SQL, as written.
  The `reset` in the example targets file **terminates every other session on the
  database**. Scenario and targets files are code: only run ones you trust.

`poolproof scan` and `poolproof report` only read the files they are given. They don't
connect to anything.

## What poolproof never does

- No telemetry, no update checks. The only network connections are to the URLs you pass.
- The JSON report doesn't contain connection URLs or passwords.

## Known limitations

- No TLS yet: connections are unencrypted. Use it on localhost or a trusted network.
  With SCRAM the password itself never goes over the wire; if the server asks for a
  clear-text password, poolproof sends it.
- Passwords come from the targets file or the command line, where `ps` and shell history
  can see them. Use a scratch database with a throwaway password.
- An HTML report shows links from the scenario files it was built from. Open a report
  only if you trust where its results came from.
