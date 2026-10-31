# scan corpus

Queries that real ORMs, drivers and tools send, with the findings `poolproof scan`
should give for each. One file per source. `tests/scan_corpus.rs` runs every entry
through the input path its `form` names (a pg_stat_statements CSV, a stderr log line or
a SQL file), then the rules.

```toml
[[query]]
sql = 'SELECT pg_advisory_lock($1)'
form = "pgss"                        # how it reaches scan, see below
expect = ["session-advisory-lock"]   # rule IDs, [] = no finding
note = "prisma migrate, lock id 72707369"
pending = "C11"                      # optional: scan gets it wrong today, TODO.md item
```

`expect` is compared as a set: a query with three `set_config` calls expects
`["session-set-config"]` once.

`form`:
- `pgss`: pg_stat_statements text, same on Postgres 17 and 18.
- `pgss-17`, `pgss-18`: pg_stat_statements text that differs by version. Postgres 18
  replaces SET values with `$1`, 17 keeps them.
- `log`: as it appears in a statement log (`log_statement`, `log_min_duration_statement`).
  Bound parameters stay `$n`, literals stay literal, multi-statement strings stay whole.
- `sql`: a file such as a migration or a pg_dump script.

The pgss and pgss-17/18 texts were captured from real pg_stat_statements on Postgres 17.11
and 18.4. The query shapes come from each project's source; `note` says which part.

`pending` marks an entry where today's scan disagrees with `expect`. The test checks it
still disagrees, so fixing the item fails the test until `pending` is removed.
