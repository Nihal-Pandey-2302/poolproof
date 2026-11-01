# Results and verdicts

**[Results site](https://nihal-pandey-2302.github.io/poolproof/)**: every scenario against
PgBouncer 1.24 to 1.26 (1.26 also over TLS), PgDog 0.1.50 to 0.1.62, Supavisor 2.9 and
2.10, PgCat 1.2 and Odyssey 1.5, on Postgres 17 and 18, with the full step-by-step trace
for each cell. Where a behavior changed between tested versions, it names both, e.g.
"fails from 0.1.56 (passed in 0.1.50)". Supavisor runs with its defaults:
named_prepared_statements off. Its FAQ says transaction mode needs the flag for named
prepared statements; the named-prepared variant column shows Supavisor with it on.
PgDog's `full` variant (query parser on) pins a client that ran CREATE TEMP to its server
connection (PgDog's CHANGELOG, v0.1.57): with a pool of one, the other client can't get a
server and PgDog answers 58000 "all replicas down"; with a bigger pool it gets
Postgres's own 42P01.
[RESULTS.md](../RESULTS.md) has the same data as a table. Both are built from `results/`,
which `scripts/update-results.sh` regenerates and CI checks.

A failure is one of three kinds:

- **silent**: the pooler returned a result, but not the one Postgres returned. The
  application gets wrong data and no error. These are the dangerous ones.
- **error**: the pooler refused with an error where Postgres didn't. Loud, so you notice.
- **hang**: no reply within the step timeout (10 seconds; `--step-timeout 30s` changes
  it, a scenario's `timeout_ms` overrides it), or replies that stop short of what
  Postgres sent. Keep the step timeout above the pooler's own checkout and query
  timeouts, so you see its error instead of a hang; 10 seconds is above PgDog's 5 second
  checkout timeout.

An ERROR verdict means nothing could be compared, so it says nothing about the pooler: a
target URL didn't answer before the run, a step on the Postgres side failed in the harness
(timeout, connection failure), the pooler didn't answer again after a reset, setup,
teardown or reset failed, or `--check-scenarios` saw Postgres give two different results.
A FAIL the steps already found is kept even if teardown fails after it. ERROR only stops
that scenario; the run goes on.

`--fail-on` picks which FAILs make the exit code 1, by kind: `silent` (only silent ones),
`error` (silent and error), `any` (the default, hangs too) or `none`. An ERROR verdict
(nothing compared) counts for all but `none`. A pooler with known hangs can still gate CI
on wrong results. Results it ignores are counted on stderr.

`--compare-to last.json` compares the run with a saved `run --format json` report, so a
pooler with known failures can gate CI on new ones. Results match by target and scenario.
Only regressions fail the run: PASS to FAIL or ERROR, a FAIL getting worse (hang, then
error, then silent), and FAIL to ERROR or back. `--fail-on` still decides which of those
count. Fixed results, milder failures, and scenarios or targets only one side has are
listed but never fail it. A repeated (timing-sensitive) scenario gates on PASS and FAIL
only, so one failing run in five against a baseline that passed all five fails it. A
target whose product, version, config or Postgres differs gets a warning; a different
pool mode is an error. With `--format json` the comparison is in the report under
`comparison`.

## Example

```text
$ poolproof run --filter multi-statement
FAIL  pgdog-0.1.62             multi-statement-begin (silent)
      A simple query that starts with BEGIN must run every statement in the string.
      step 1: [app] simple: BEGIN; INSERT INTO pp_multi VALUES (1); COMMIT
      postgres: ok rows=[] tags=[0, 1, 0]
      pooler:   ok rows=[] tags=[0]
      from https://github.com/pgdogdev/pgdog/issues/1849
```

## Sharing a run

To share the outcome with your team, `--format html` writes one self-contained page with
the grid and every step's trace. `--format json` saves the raw results;
`poolproof report a.json b.json --format html` (or `markdown`) combines saved runs, and
`--label 'nightly variants=c.json'` shows one file's targets under a heading of their own,
after the others. The JSON carries `schema_version` and `generated_at`; poolproof refuses
a report in a newer format than it reads. In these formats, a progress line per result
goes to stderr. Text output is colored on a terminal unless `NO_COLOR` is set;
`FORCE_COLOR` or `CLICOLOR_FORCE` colors it anywhere.

JSON Schemas for the report, scenario files and targets files are published at
`https://nihal-pandey-2302.github.io/poolproof/schemas/v1/` as `report.schema.json`,
`scenario.schema.json` and `targets.schema.json`. A report names its schema in `$schema`,
so editors validate it. For a TOML file, put `#:schema` and the schema's URL on the first
line, and Even Better TOML (taplo) checks it as you type and shows what each field means.
This repo's `.taplo.toml` does that for its own scenarios and targets files. The schemas
check the shape of a file, not the rules poolproof applies when it loads one: one action
per step, unique names, `execute` after `prepare`, one side per client.
