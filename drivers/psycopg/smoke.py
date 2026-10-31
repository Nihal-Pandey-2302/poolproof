"""poolproof's psycopg program: runs a case's steps and prints one JSON line per step.

The protocol is in drivers/README.md. In short: steps come on stdin, one per line
(client, SQL, comma-separated integer parameters, tab-separated). Each client gets one
connection, opened at the start and never reopened. Every step is reported, so a
driver that recovers on the next call still shows the step that failed.
"""

import asyncio
import json
import math
import os
import re
import sys

import psycopg


def emit(line):
    print(json.dumps(line), flush=True)


def code(error):
    """The SQLSTATE, or DRIVER:<class> for an error psycopg raised itself."""
    if isinstance(error, psycopg.Error) and error.sqlstate:
        return error.sqlstate
    return "DRIVER:" + type(error).__name__


def read_steps():
    steps = []
    for line in sys.stdin.read().splitlines():
        client, sql, params = line.split("\t")
        params = [int(p) for p in params.split(",") if p]
        # psycopg's placeholder is %s; the case SQL has no % of its own.
        steps.append((client, re.sub(r"\$\d+", "%s", sql), params or None))
    return steps


async def main():
    variant = os.environ["PP_VARIANT"]
    timeout = int(os.environ["PP_STEP_TIMEOUT_MS"]) / 1000
    # Autocommit on, as Django and most apps use psycopg. Without it every statement
    # opens a transaction that stays open, which pins a server connection for the
    # whole session.
    options = {"autocommit": True}
    if variant == "no-prepare":
        options["prepare_threshold"] = None
    elif variant != "default":
        sys.exit(f"unknown variant {variant}")
    steps = read_steps()
    emit({"driver": "psycopg", "version": psycopg.__version__})

    # A client is a connection, or the error every one of its steps reports.
    clients = {}
    attempts = 0
    for client in dict.fromkeys(c for c, _, _ in steps):
        attempts += 1
        try:
            clients[client] = await asyncio.wait_for(
                psycopg.AsyncConnection.connect(
                    host=os.environ["PP_HOST"],
                    port=os.environ["PP_PORT"],
                    user=os.environ["PP_USER"],
                    password=os.environ["PP_PASSWORD"],
                    dbname=os.environ["PP_DBNAME"],
                    sslmode=os.environ["PP_SSLMODE"],
                    connect_timeout=math.ceil(timeout),
                    **options,
                ),
                timeout,
            )
        except TimeoutError:
            clients[client] = {"error": "TIMEOUT", "connect": True}
        except Exception as e:
            clients[client] = {"error": code(e), "connect": True}

    for i, (client, sql, params) in enumerate(steps, 1):
        conn = clients[client]
        if isinstance(conn, dict):
            emit({"step": i, **conn})
            continue
        try:
            rows = await asyncio.wait_for(run(conn, sql, params), timeout)
            emit({"step": i, "rows": rows})
        except TimeoutError:
            emit({"step": i, "error": "TIMEOUT"})
            # Its state is unknown now; later steps on it report CLOSED.
            clients[client] = {"error": "CLOSED"}
        except Exception as e:
            emit({"step": i, "error": code(e)})

    for conn in clients.values():
        if not isinstance(conn, dict):
            try:
                await asyncio.wait_for(conn.close(), timeout)
            except Exception:
                pass
    emit({"connections": attempts})


async def run(conn, sql, params):
    cur = await conn.execute(sql, params)
    if cur.description is None:
        return []
    return [[None if v is None else str(v) for v in row] for row in await cur.fetchall()]


if __name__ == "__main__":
    asyncio.run(main())
    # Don't wait on anything a timed-out query left behind.
    os._exit(0)
