# poolproof

Find out what breaks when you put a connection pooler in front of Postgres.

Poolers like PgBouncer, PgDog, Supavisor and RDS Proxy are supposed to be invisible to the
application. In transaction mode they mostly are, except when they aren't: a `SET` leaks
to another client, a temp table shows up in someone else's session, an advisory lock
outlives the client that took it, or part of a multi-statement query silently never runs.
These bugs show up in production, one at a time.

poolproof does three things:

- **`poolproof run`** replays small scenarios against plain Postgres and then through your
  pooler, and reports every step where the results differ. Postgres is the oracle, so a
  scenario never hardcodes expected output.
- **`poolproof drivers`** does the same with real drivers as the clients: psycopg, pgx,
  node-postgres and pgjdbc, with their default settings and with the setting their docs
  give for poolers.
- **`poolproof scan`** reads your real queries (a `pg_stat_statements` export, Postgres
  logs or a `.sql` file) and flags patterns that are unsafe under transaction pooling,
  using the actual Postgres parser.

`poolproof doctor` checks a pooler setup before you run anything against it.

Docs: [a code tour](docs/code-tour.md) and [the architecture](docs/architecture.md) for
contributors, [testing a managed pooler](docs/managed-poolers.md) such as RDS Proxy or
hosted Supavisor.

## Docs

- [Install](docs/install.md): release binaries, building from source, completions and man pages
- [Results and verdicts](docs/results.md): the results site, failure kinds, `--fail-on`, `--compare-to`, report formats
- [Documented limitations](docs/limitations.md): FAILs the pooler's own docs describe
- [Testing your pooler](docs/targets.md): URLs, TLS, targets files, probes, reset, `doctor`
- [The test bed](docs/test-bed.md): running the suite locally, PgDog from source
- [Scanning your queries](docs/scan.md)
- [Real drivers](docs/drivers.md)
- [GitHub Action](docs/github-action.md)
- [Writing a scenario](docs/scenarios.md)

## Status

Not covered yet:

- RDS Proxy and hosted poolers are not in the test bed; they can be tested with the
  recipes in [docs/managed-poolers.md](docs/managed-poolers.md).
- The variant targets (`targets-variants.toml`: session mode, pool_size 4 and five
  non-default pooler settings) run nightly on Postgres 18.4 only; their results are
  published on the site under their own heading.
- `scripts/pgdog-ref.sh` has not been checked with release builds, its default.

## License

MIT OR Apache-2.0
