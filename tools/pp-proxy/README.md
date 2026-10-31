# pp-proxy

Self-test targets for poolproof. Not part of the poolproof package.

One process, one listener per fault, all forwarding to one Postgres:

    PROXY_LISTEN="6501=none,6502=drop-pipelined" PROXY_UPSTREAM=postgres:5432 pp-proxy

| Fault | What it does |
|---|---|
| `none` | Copies bytes, one server connection per client. Must pass every scenario. |
| `drop-pipelined` | Drops client messages sent after a Sync until its ReadyForQuery. |
| `swallow-error` | Drops ErrorResponse after startup. |
| `no-copydone` | Drops the client's CopyDone. |
| `wrong-tag` | Reports one row more in INSERT command tags. |
| `shared-server` | One server connection for all clients, no reset, shared cancel key. Logs in as `PROXY_USER` / `PROXY_PASSWORD` / `PROXY_DATABASE`. |
| `refuse` | Accepts and closes. |
| `pool-of-two` | Like `shared-server` with two server connections, handed out last in, first out; the second opens when a client needs it. A client's cancel key is that of the connection it would get at startup: the most recently released free one, or the last one opened when both are held. |

`PROXY_BIND` sets the listen address (default `127.0.0.1`; in a container set it to `0.0.0.0`).

`selftest/expected.toml` lists the verdict each target must get. In the docker test bed
(ports 56471-56478) the proxy runs under the `selftest` profile:

    docker compose -f docker/compose.yml --profile selftest up -d --wait postgres pp-proxy
    poolproof run --targets selftest/targets.toml --format json > selftest.json
    pp-selftest-check selftest/expected.toml selftest.json scenarios

CI runs the same check with the proxy built on the runner instead of in Docker.
Take the bed down with `--profile selftest` too, or the proxy keeps running.
