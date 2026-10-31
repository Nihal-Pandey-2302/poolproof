# Driver programs

`poolproof drivers` runs real drivers through each pooler, with their default settings
and with the setting their own docs give for poolers. Each case runs against plain
Postgres first, then through the pooler, and any step whose result differs is a FAIL,
the same as `run`.

| Driver | Language | Variants |
|---|---|---|
| [psycopg](psycopg/) 3 | Python | `default` (prepare_threshold = 5, autocommit on), `no-prepare` (prepare_threshold = None) |
| [pgx](pgx/) v5 | Go | `default` (QueryExecModeCacheStatement), `exec` (QueryExecModeExec) |
| [node-postgres](node-postgres/) | Node.js | `default` (unnamed statements), `named` (named queries) |
| [pgjdbc](pgjdbc/) | Java | `default` (prepareThreshold = 5), `no-prepare` (prepareThreshold = 0) |

`drivers.toml` lists them with the version each image pins, and `cases.toml` holds the
cases every driver runs. Scenario names are `driver/<driver>-<variant>/<case>`.

## Running

Linux only for now: the containers use `--network host` to reach `127.0.0.1` URLs, and
on Docker Desktop (macOS, Windows) the host network is the Linux VM's, not your
machine's, so in practice that only works on Linux. The images are built from a checkout
of github.com/Nihal-Pandey-2302/poolproof, which has their Dockerfiles; an installed
poolproof then runs them.

```sh
docker buildx bake -f drivers/docker-bake.hcl --load   # build the four images
poolproof drivers                                     # every target in targets.toml
poolproof drivers --driver pgx --target pgbouncer-1.26.1 -v
```

`drivers` takes the same flags as `run`, except `--scenarios` and `--reverse`.
`docker run` is told never to pull, so it only runs images you built. Supported URLs:
TCP hosts with sslmode `disable`, `prefer` or `require`. node-postgres has no `prefer`,
so it connects without TLS there.

## The program contract

A program gets its connection settings in the environment, never as arguments:
`PP_HOST`, `PP_PORT`, `PP_USER`, `PP_PASSWORD`, `PP_DBNAME`, `PP_SSLMODE`, plus
`PP_VARIANT` and `PP_STEP_TIMEOUT_MS` (from `--step-timeout`).

The steps come on stdin, one per line: client, SQL, comma-separated integer parameters,
separated by tabs. Parameters are written `$1`, `$2`; a program changes them to its
driver's placeholder. The program:

- opens one connection per client before the first step, and never opens another. No
  connection pools, no reconnects, no retries.
- runs each step with the driver's normal query API, so its own prepare and caching
  rules apply, and gives each step at most the step timeout. A step that takes longer
  reports `TIMEOUT`, and the connection is dropped: its later steps report `CLOSED`.
- reports every step, even after an error, so a driver that recovers on the next call
  still shows the step that failed.

It prints JSON lines on stdout:

```text
{"driver":"pgx","version":"5.11.0"}       the driver and version it loaded
{"step":1,"rows":[["42"],[null]]}           a step's rows, every value as text
{"step":2,"error":"26000"}                  a step's error
{"step":3,"error":"TIMEOUT"}
{"step":1,"error":"28P01","connect":true}   the client's connection failed
{"connections":2}                           done, and how many connections it opened
```

An error is a SQLSTATE, `TIMEOUT`, `CLOSED`, or `DRIVER:<class>` for an error the driver
raised without one (pgjdbc adds its own SQLSTATE: `DRIVER:PSQLException:08006`). Never
the message: its wording isn't compared. A `DRIVER:` error on Postgres makes the case
ERROR; through the pooler it is a FAIL.

poolproof never lets a broken program pass. Output it can't trust is the harness's
problem, and the case is ERROR: lines it can't read, a version that isn't the one in
`drivers.toml`, steps out of order, or a program that runs past every step's timeout
plus 30 s (then the container is removed). A program that stops early, exits with an
error or opens more connections than it has clients was broken by what it talked to.
Every step it reported keeps its outcome, and every step it didn't is `DRIVER:Stopped`.
On Postgres that makes the case ERROR. Through the pooler it is a FAIL: a difference
the steps show keeps its step and kind (a silent one stays silent) and adds what the
program did; if the steps alone would pass, the FAIL is an error at the last step whose
action says what happened, e.g. `program: opened 3 connections for 2 clients`.

## Changing a driver version

The version is part of what is tested, so it changes by hand, never through Dependabot:
the lockfile (`uv.lock`, `package-lock.json`, `go.mod` and `go.sum`, or `pom.xml`),
`drivers.toml`, then `scripts/update-driver-results.sh` and commit the results.

Dependabot ignores the drivers, so it won't open security updates for them either.
Watch the GitHub advisories for psycopg, pgx, pg (node-postgres) and pgjdbc, and bump
by hand when one applies. The programs only talk to the test bed, but an old driver
with a known hole shouldn't sit here.
