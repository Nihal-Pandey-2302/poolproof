# Writing a scenario

A scenario is a TOML file in `scenarios/`. Clients are named; each connects on its first
step. `direct = true` connects to Postgres directly, which is how scenarios check server
state behind the pooler. A client is either direct or pooled for all of its steps.
Scenario names must be unique. `refs` links the issues or docs the scenario came from.
Reports print them after a FAIL as `from`, or as `related:` for a GitHub link to another
project than the target's `product` (a PgDog issue on a PgBouncer FAIL, or any GitHub
link on a target without `product`).

```toml
# Shortened: the real file adds evidence steps (below), without which this would PASS
# whenever the two clients don't share a server connection.
name = "temp-table-isolated-between-clients"
description = "A temp table created by one client must not be visible to another client."
refs = ["https://github.com/pgdogdev/pgdog/issues/1733"]

[[step]]
client = "a"
simple = "CREATE TEMP TABLE pp_tmp (x int)"

[[step]]
client = "b"
simple = "SELECT count(*) FROM pp_tmp"
```

Step actions:

- `simple`: simple query protocol, can hold several statements.
- `prepare` + `name`, then `execute`: extended protocol. Cast columns to `text`. An
  `execute` needs an earlier `prepare` of that name by the same client since its last
  `disconnect` or abandoned query, or the scenario fails to load.
- `disconnect`: close the connection cleanly. On Postgres it waits until the backend has
  exited, so the next step no longer sees its locks or temp tables.
- `wait`: collect the result of this client's `background` query.
- `cancel`: send a CancelRequest with this client's key, through the same pooler.
- `notifications`: the NOTIFY messages this client received since the last such step.
- `send`: write protocol messages directly and record every reply, for the things drivers
  hide: statement names, Flush vs Sync, pipelines, COPY. For example:

  ```toml
  [[step]]
  client = "a"
  send = ["parse s1 SELECT 'a'::text", "bind - s1", "execute -", "sync"]
  # replies: ParseComplete, BindComplete, DataRow a, CommandComplete SELECT 1, ReadyForQuery I
  ```

  Messages: `parse NAME SQL`, `bind PORTAL STATEMENT`, `execute PORTAL [MAX_ROWS]`,
  `describe statement|portal NAME`, `close statement|portal NAME`, `sync`, `flush`, `query
  SQL`, `copydata TEXT`, `copydone`. `-` is the unnamed statement or portal. Replies are
  read up to one ReadyForQuery per `sync`/`query`/`copydone`; without those, until the
  server is quiet for half a second. Errors are recorded by SQLSTATE only. A client that
  uses `send` has its own raw connection; `evidence` and `disconnect` are its only other
  steps.
  ParameterStatus replies are skipped, since poolers add and drop them freely; with
  `compare_parameter_status = true` a `send` step records them and compares them as a
  set, wherever in the step's reply they come. A dropped or changed value is a silent
  failure; `compare = "status"` ignores them. The set is per step: one that arrives
  after the step's ReadyForQuery shows as missing there and extra in the next step.
  COPY TO STDOUT replies are `CopyOutResponse`, one `CopyData` line per message (hex if
  it isn't UTF-8) and `CopyDone`.

Options on a `simple` step: `background = true` starts the query and moves on, so other
clients can act while it runs. `abandon_after_ms` drops the connection without a goodbye
if the query is still running after that long, like a client that crashed or timed out.

Options on any step: `delay_ms` sleeps first, e.g. so a cancel lands while a query runs.
`compare` can be `full` (default), `status` or `ignore`.

Scenario options: `setup` and `teardown` run directly on Postgres around each run;
`timeout_ms` overrides the step timeout for this scenario; `modes` limits which pooling
modes a scenario applies to; `repeat = 5` runs the pooler side five times and fails if any
run fails. Use `repeat` when the outcome depends on timing inside the pooler, as with
pipelines: reports then show how many runs failed, and CI doesn't treat a change in those
verdicts as a regression, because the worst case can differ between machines.
`requires` lists privileges the `direct` user needs (`createrole`, `superuser`,
`pg_signal_backend`); on a target without them the scenario is skipped, with the reason.
If the privileges can't be read, it is ERROR instead. It can also name what the pool must
look like: `one_server_connection` for a scenario that only means something when two
clients share one server connection, `several_server_connections` for the opposite.
Those are judged by the target's probes ([targets.md](targets.md#probes)): unmet, the scenario is skipped, e.g.
"needs one server connection (probe: several)". In statement mode the probe can't tell,
so both are skipped.

A step with `precondition = true` is a `simple` query of a pooled client (not the first
step) that returns one boolean: whether this run tests what the scenario means to, e.g.
that this client got a given backend. It doesn't run on Postgres and is never compared.
Through the pooler, `t` goes on; `f` stops the run, which then doesn't count (a SKIP if
no run met it); anything else (an error, a hang, no row) is a FAIL like any other step.
With `repeat`, only the runs that met it count, and the output says how many did.

A step with `evidence = true` (and nothing else) records which backend a pooled client
is on: poolproof asks the pooler for `pg_backend_pid()` (a Query message for a `send`
client) and reads that backend's start time on direct right away. It doesn't run on
Postgres and is never compared. `same_backend = [[2, 5]]` names pairs of evidence steps
that must be on one backend for a run through the pooler to count: a scenario about one
client's state reaching another only tests that when the second client really got the
first one's server connection. A difference at another step is a FAIL as always, and so
is an evidence step the pooler answers with an error, a hang or no pid. A run that passed
with a pair on different backends, or where a backend was gone before its start time
could be read, doesn't count; if no run counts, the result is a SKIP saying the second
client never reached the first one's connection. A start time direct may not see is
compared by pid alone. `same_backend_or_closed = [[2, 5]]` is the same check for a
first client that disconnects: the run also counts when the second client is elsewhere
but the first one's backend (same pid and start) has exited by then, which poolproof
waits for on direct for a few seconds, since a backend exits a moment after its client
leaves. `ignore_server_requirements` turns the pair checks off. Put
evidence steps right after the steps whose server connection matters; in transaction
mode a client can still move between its step and the evidence after it, which a
`BEGIN`..`COMMIT` around both rules out where that doesn't change what is tested. An
evidence step of a `send` client must follow one that ends with `sync` or a `query` (not
one that starts COPY FROM STDIN, and nothing after the last sync but queries), so the
connection is waiting for a new query; an evidence step there anyway is the scenario's
mistake and an ERROR. Two more rules for where evidence goes:

- Never on a client inside an aborted transaction (after an error inside `BEGIN`, before
  `ROLLBACK`): its SELECT gets 25P02, which reads as a FAIL of the pooler.
- Not on the client that parsed the unnamed statement, between its Parse and its use:
  the evidence query replaces the unnamed statement.

`poolproof run -v --filter <name>` prints every step's result on Postgres and through the
pooler. Use it on a new scenario: a PASS only means something if Postgres did what you
expected.
`poolproof run --check-scenarios --filter <name>` runs its Postgres side twice and gives
ERROR if the results differ, e.g. because a step returns a random value, a time or a pid.
Use `compare = "status"` or `"ignore"` on such steps.

The best scenarios come from real bug reports. If your pooler bit you, a scenario with a
link to the issue is the most useful contribution. [CONTRIBUTING.md](../CONTRIBUTING.md) has
the workflow.
