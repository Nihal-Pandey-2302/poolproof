# Documented limitations

Some FAILs are limitations the pooler documents itself, like PgBouncer's "SET/RESET:
Never" for transaction pooling. Those are listed in `limitations/<product>.toml`, built
into the binary, each with a link to the pooler's own docs pinned to a version or commit
and the doc's words. Built in: PgBouncer, PgCat, PgDog (advisory locks) and Supavisor;
none for Odyssey yet, so nothing is marked documented on it. A FAIL that matches an entry
exactly is still a FAIL, printed as `(silent, documented)` with the doc, and marked
`expected` in JSON. Exactly means: the target's `product`, `mode`, `config` (`default`
unless the entry lists others) and `version` (keys of the [targets file](targets.md#targets-file)) are in the
entry's range, every step that differs is one the entry lists, with the outcome it lists,
and nothing hangs. A repeated scenario must fail the same way in every run. Anything else
stays an unexplained FAIL. An entry that applies to a target but doesn't match its result
(the scenario passes now, or fails another way) is printed as a stale expectation and
listed under `stale_expectations` in JSON.

Expected FAILs count for the exit code like any other: for an app, a documented SET leak
still breaks it. `--ignore-expected` leaves them out, for a pooler's own CI. With
`--compare-to`, an expected FAIL that no longer matches its entry is a regression.
`--limitations DIR` (on `run`, `drivers`, `report` and `scan`) uses your own entries,
checked against the scenarios the run uses. An entry only applies to a scenario with the
same steps it was checked against, so a scenario of your own with a built-in name isn't
judged by the built-in entries. `report` applies the current entries to saved runs, so a
new entry needs no new run.

```toml
[[limitation]]
scenario = "set-isolated-between-clients"
mode = "transaction"
config = ["default", "tls"]   # target configs; default: ["default"]
since = "1.24.1"              # lowest version it was checked on; fixed_in = "..." ends it
# postgres_since / postgres_fixed_in limit it to some Postgres versions.
differs = [{ step = 7, pooler = { status = "ok", rows = [["9MB"]], tags = [0] } }]
doc = "https://github.com/pgbouncer/pgbouncer.github.io/blob/ccb120f.../features.md"
quote = "SET/RESET | Yes | Never"
# doc_version = "1.26.1" is needed when the link isn't pinned to a commit.
# note = "..." says where in the doc the quote is, for whoever checks; shown nowhere else.
```

`pooler` is the outcome as in `run -v`: `{ status = "ok", rows, tags }`, `{ status =
"error", code }` (the message isn't compared), `{ status = "messages", messages }` for a
`send` step, or `{ status = "abandoned" }`. Timeouts, dropped connections and other
outcomes the harness makes up are refused.
