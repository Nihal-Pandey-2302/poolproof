# Real drivers

Scenarios talk to the pooler the way poolproof's own clients do. Apps talk through a
driver, and drivers prepare statements, cache them and name them in their own ways.
`poolproof drivers` runs five cases (repeated queries past the auto-prepare threshold,
two connections whose statement names collide, the same query on two connections, a
transaction, a session `SET`) with each driver, against plain Postgres and then through
the pooler. The driver images are built from a checkout of
github.com/Nihal-Pandey-2302/poolproof (their Dockerfiles aren't part of the crate):

```sh
docker buildx bake -f drivers/docker-bake.hcl --load   # build the driver images once
poolproof drivers --target pgbouncer-1.26.1
poolproof drivers --driver psycopg --pooler postgres://app@127.0.0.1:6432/app \
  --direct postgres://app@127.0.0.1:5432/app --reset all
```

(`--reset auto` refuses here: Postgres sees a pooler on the same machine connect from
poolproof's own address. `all` needs no `--yes` for a local Postgres.)

Each driver runs twice: with its defaults, and with the setting its docs give for
transaction pooling (psycopg `prepare_threshold = None`, pgx `QueryExecModeExec`, pgjdbc
`prepareThreshold = 0`), so a result says whether the defaults break and whether the fix
works. node-postgres runs with and without named queries. `drivers` takes the same flags
as `run` and gives the same verdicts and report formats. [DRIVERS.md](../DRIVERS.md) has the
results for the newest version of each pooler on Postgres 17, from
`scripts/update-driver-results.sh`. Each driver runs in Docker on the host network, so
for now it needs Linux. [drivers/README.md](../drivers/README.md) has the details and the
rules a driver program follows so it can't hide a failure.
