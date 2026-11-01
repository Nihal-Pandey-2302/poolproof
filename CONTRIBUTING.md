# Contributing

Thanks for helping. The most useful contribution is a scenario that reproduces a real
pooler bug, with a link to the issue. Bug reports about wrong verdicts come next: a wrong
PASS or FAIL makes every other result less trustworthy.

## Setup

You need Rust 1.88 or newer and Docker.

```sh
docker compose -f docker/compose.yml up -d --wait   # Postgres + every pooler in targets.toml
cargo run -- run                                    # all scenarios against targets.toml
```

The test bed listens on 127.0.0.1 only, with the password `postgres`. Each pooler has a
pool of one server connection, so state leaks between clients every time instead of by
luck. `POSTGRES_IMAGE=postgres:18.4` picks the Postgres version. `scripts/testbed.sh`
runs a second test bed on shifted ports (see [docs/test-bed.md](docs/test-bed.md)).

The test bed keeps Postgres data in tmpfs, so it leaves no volumes behind. A Postgres
container you start yourself does: the image declares a volume for its data, and
`docker rm -f` (even of a `--rm` container) leaves it. Remove throwaway containers with
`docker rm -fv`, or start them with `--rm` and stop them with `docker stop`. Because the
data is in tmpfs, a restarted `postgres` or `supavisor-meta` comes back empty, and
Supavisor loses its tenants: take the test bed down and up again instead.

## Writing a scenario

The file format is in [docs/scenarios.md](docs/scenarios.md). The workflow:

1. Find the smallest sequence of steps that shows the bug. Start from the issue's own
   reproduction if there is one, and put its URL in `refs`.
2. Write `scenarios/<what-must-hold>.toml`. Name it after the property that should hold
   (`temp-table-isolated-between-clients`), not after the bug, and write `description` as
   that property in one sentence.
3. Run it with `-v`, which prints every step's outcome on Postgres and through the pooler:

   ```sh
   cargo run -- run --filter temp-table-isolated -v
   ```

4. Read the Postgres column first. A PASS only means something if Postgres did what you
   expected: if a step errored on Postgres too (wrong SQL, missing privilege), the pooler
   "matching" it proves nothing.
5. Check that Postgres itself is deterministic for it:

   ```sh
   cargo run -- run --filter temp-table-isolated --check-scenarios
   ```

   This runs the Postgres side twice and gives ERROR if the two runs differ, e.g. a step
   that returns a pid, a time or a random value. Use `compare = "status"` or `"ignore"`
   on such steps.
6. Check the verdicts make sense on every pooler, and run the whole suite with `--reverse`
   too. A verdict that changes with order means state leaks between scenarios. A FAIL
   the pooler's own docs describe gets a documented limitation (below).
7. If the outcome depends on timing inside the pooler (pipelines, races), set `repeat`.
8. If the scenario needs a privilege a managed database may not give (`CREATE ROLE`,
   ending other sessions), list it in `requires` (`createrole`, `superuser`,
   `pg_signal_backend`). Targets without it then SKIP the scenario instead of passing
   on a permission error that both sides share.
9. Objects a scenario creates are named `pp_*` and dropped in `teardown`.
10. Regenerate the committed results and add them to the same commit:

    ```sh
    scripts/update-results.sh
    ```

    CI compares its own verdicts with `results/` and fails if they differ.

## Adding a documented limitation

A FAIL that the pooler's own docs say will happen gets an entry in
`limitations/<product>.toml`. The format is in
[docs/limitations.md](docs/limitations.md). The rules:

1. `quote` is the doc's own words, copied verbatim, and `doc` links to it pinned to a
   commit (a GitHub link with the commit hash). If it can't be pinned, `doc_version`
   names the version you read. `note` says where in the doc the quote is. A FAIL the doc
   doesn't plainly describe stays a bug: no entry.
2. `differs` lists every step where the pooler differs, each with the exact outcome it
   gives, as `run -v` prints it. A result that differs at another step, or another way,
   stays an unexplained FAIL. `since` is the lowest version you checked; add `config`,
   `fixed_in` or `postgres_since` where they apply.
3. An entry is checked against the scenario's steps as they are. After changing a
   scenario's steps, check its entries' step numbers and outcomes again.
4. `cargo test built_in_entries_match_committed_results` fails when a built-in entry
   matches no committed result, or a committed result makes one stale.
   `cargo run -- report results/postgres-*.json | diff RESULTS.md -` shows what changes,
   without a new run; commit the new RESULTS.md with the entry.

## Code

- `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must pass.
  The code must also build with Rust 1.88, the minimum supported version.
- A bug fix starts with a test that fails without it.
- Code that parses what a server or a scenario file sends (`runner/raw.rs`) has fuzz
  targets in `fuzz/`; see [fuzz/README.md](fuzz/README.md).
- No `unwrap` or `expect` outside tests, except where it can't fail (say why in a comment).
- Never print a password: not in logs, errors, reports or panics.
- A change people will notice updates README.md or docs/ and gets a line under `Unreleased` in
  [CHANGELOG.md](CHANGELOG.md) in the same commit.

## Pull requests

- One concern per PR, and small commits with one concern each.
- Commit messages: a short summary line in the imperative ("Add ...", "Fix ..."), then
  why, if it isn't obvious.
- Fill in the PR template. CI must be green before review.
- The drivers workflow (real drivers through each pooler, about 15 minutes) runs on a PR
  only when it touches `drivers/`, `src/drivers/` or the driver results, or has the
  `drivers` label; add the label to a PR that changes how scenarios run or are compared.
  It always runs on main. If it goes red there, the next merge waits until it is green
  again or the change that broke it is reverted. A run the label starts reports as
  "drivers label / drivers", not "drivers", so it can't stand in for a required "drivers"
  check.

## Reporting a security problem

Not in a public issue. See [SECURITY.md](SECURITY.md).
