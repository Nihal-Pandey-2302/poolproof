# Changelog

All notable changes to poolproof are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow the policy
below.

## Versioning

poolproof uses [Semantic Versioning](https://semver.org/). Before 1.0, a breaking change
bumps the minor version (0.1 to 0.2) and everything else bumps the patch version.

These are the stable interfaces. Breaking one is a breaking change:

- **CLI**: command names, flags and exit codes (`run`: 0 all pass, 1 a scenario failed or
  errored; `scan`: 0 clean, 1 findings; any command: 2 poolproof itself failed). The text and markdown output is
  for people and can change in any release.
- **Scenario files** (`scenarios/*.toml`) and **targets files** (`targets.toml`): a file
  that works in one version works the same way in every later version with the same
  major version (minor, before 1.0). New optional keys are not breaking.
- **JSON report** (`run --format json`, read by `report` and `scan --results`): fields
  can be added in any release; removing, renaming or changing the meaning of a field is
  breaking. A newer poolproof reads reports from older versions of the same major.
- **SARIF output** follows SARIF 2.1.0. Rule IDs only change in a breaking release.

Not breaking, but always listed here: new or changed scenarios, a harness fix that changes
a verdict, new `scan` rules, and new pooler versions in the published results.

## [Unreleased]

First release.

### Added

- `poolproof run`: replays scenarios against Postgres and through each pooler, and reports
  every step where the results differ (silent, error or hang). Text, JSON, markdown and
  HTML output; `--pooler`/`--direct` for a single target, `--filter`, `--reverse`, `-v`.
- 30 scenarios: session state leaks (SET, SET ROLE, search_path, temp tables, advisory
  locks, cursors, prepared statements, LISTEN/NOTIFY), multi-statement transactions,
  cancel requests, abandoned queries, and protocol-level messages (statement names,
  Flush vs Sync, pipelines, COPY FROM STDIN) with raw `send` steps.
- `poolproof scan`: flags transaction-pooling-unsafe queries in pg_stat_statements CSV,
  Postgres stderr and jsonlog logs, and `.sql` files. Text, JSON and SARIF output.
  `--pooler` checks each finding against what that pooler version did in poolproof's
  own runs.
- `poolproof report`: renders saved JSON runs as markdown or one self-contained HTML page.
- Results for PgBouncer 1.24.1, 1.25.2, 1.26.1, PgDog 0.1.50, 0.1.56, 0.1.62 and
  Supavisor 2.9.13, 2.10.0-rc.1 on Postgres 17.11 and 18.4, built into the binary.
- Release binaries for Linux (x86_64, arm64), macOS (x86_64, arm64) and Windows (x86_64),
  with SHA256SUMS and build provenance attestations. `cargo binstall poolproof` installs
  them.
- Release binaries are built with thin LTO and stripped: 9.2 MB to 6.5 MB on Linux x86_64.

### Security

- HTML report: `refs` from a results file become links only for `http(s)://` URLs, with
  `rel="noopener noreferrer"`, and the page has a Content-Security-Policy that allows only
  its own inline script and style (by hash).

[Unreleased]: https://github.com/Nihal-Pandey-2302/poolproof/commits/main
