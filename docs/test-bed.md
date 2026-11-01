# The test bed

```sh
docker compose -f docker/compose.yml up -d --wait   # Postgres + 13 pooler configurations
cargo run -- run
```

Each pooler in the test bed has a pool of one server connection, so leaks between clients
happen every time instead of by luck. `POSTGRES_IMAGE=postgres:18.4` picks the server
version. One PgBouncer accepts clients over TLS only (its connections to Postgres stay
plain); its certificate comes from a CA the test bed makes when it starts
(`docker/tls/out/<compose project>/ca.pem`; the target names it relative to the
repository root, so run poolproof from there). `scripts/update-results.sh` rebuilds
`results/` and `RESULTS.md` for Postgres 17 and 18 from a fresh test bed.

The test bed listens on 127.0.0.1 only. To run more than one at a time, give each its own
port offset:

```sh
scripts/testbed.sh up 100                    # project poolproof-100, every port + 100
scripts/testbed.sh targets 100 > t100.toml   # targets.toml with the ports shifted
cargo run -- run --targets t100.toml
scripts/testbed.sh down 100
```

To test PgDog built from source, say a fix before it's released, or to find the commit
that changed a verdict, `scripts/pgdog-ref.sh` builds any tag, branch or commit and runs the
suite against it, next to Postgres from the test bed:

```sh
scripts/pgdog-ref.sh v0.1.62                     # a release
scripts/pgdog-ref.sh main 200 -- --filter set-   # a branch, on test bed 200, a few scenarios
PGDOG_REPO=../pgdog scripts/pgdog-ref.sh my-fix  # a committed branch of your own clone
```

Building a ref runs its code (build scripts, proc macros, then PgDog itself), so build
only refs you trust, and read a fork's branch before you build it. The source, builds and
results stay in `~/.cache/poolproof/pgdog`. Results are labeled `git-<commit>`, so they
never mix with the release results in `results/`. The first build takes several minutes,
and rustup may install PgDog's pinned Rust toolchain. `PGDOG_PROFILE=debug` builds faster,
but compare a debug build only with other debug builds. The test bed offset must be free:
the script refuses one that's running, and takes its own bed down, volumes and all, when
it's done.
