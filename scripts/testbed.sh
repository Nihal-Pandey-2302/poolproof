#!/bin/sh
# Runs a separate copy of the test bed with every host port shifted by OFFSET, so
# several can run side by side. Offset 0 is the default test bed (project "poolproof",
# the ports in targets.toml).
# Usage: scripts/testbed.sh up|down|targets|env OFFSET
#   up       start it (extra arguments go to `docker compose up`); retries once on a busy port
#   down     stop it and remove its volumes
#   targets  print targets.toml with the ports shifted: scripts/testbed.sh targets 100 > t.toml
#   env      print the variables, to use docker compose directly: eval "$(scripts/testbed.sh env 100)"
set -eu
cd "$(dirname "$0")/.."
[ $# -ge 2 ] || { sed -n '5,9s/^# \{0,1\}//p' "$0" >&2; exit 2; }
cmd=$1 offset=$2
shift 2
case $offset in *[!0-9]*) echo "OFFSET must be a number" >&2; exit 2 ;; esac
[ "$offset" -eq 0 ] && project=poolproof || project=poolproof-$offset

# The default host ports, as written in compose.yml (${PP_PORT_55432:-55432}).
ports=$(grep -o 'PP_PORT_[0-9]*' docker/compose.yml | sort -u | sed 's/PP_PORT_//')

env_lines() {
  echo "export COMPOSE_PROJECT_NAME=$project"
  for p in $ports; do echo "export PP_PORT_$p=$((p + offset))"; done
}

# Runs `docker compose up` once, showing its output and keeping a copy in $log.
up_once() {
  { docker compose -f docker/compose.yml up -d --wait --wait-timeout 300 "$@" 2>&1; echo "exit=$?"; } |
    tee "$log" | grep -v '^exit='
  return "$(sed -n 's/^exit=//p' "$log")"
}

# The host port compose couldn't bind, if that is why `up` failed.
busy_port() {
  grep 'address already in use' "$log" | grep -o '127\.0\.0\.1:[0-9]*' | head -n 1 | sed 's/.*://'
}

case $cmd in
  up)
    eval "$(env_lines)"
    log=$(mktemp)
    trap 'rm -f "$log"' EXIT
    up_once "$@" && exit 0
    port=$(busy_port)
    # Not a port clash: fail as compose did.
    [ -n "$port" ] || exit 1
    # A port freed a moment ago (e.g. by a bed that just went down) can still be held.
    # A plain re-up can then leave the container running without its port, so recreate.
    echo "testbed.sh: port $port is in use; retrying once in 2s" >&2
    sleep 2
    up_once --force-recreate "$@" && exit 0
    port=$(busy_port)
    echo "testbed.sh: test bed $project did not come up; taking it down" >&2
    [ -z "$port" ] || echo "testbed.sh: port $port is still in use; see what holds it: ss -ltnp | grep :$port" >&2
    "$0" down "$offset" >/dev/null 2>&1 || true
    exit 1
    ;;
  down)
    eval "$(env_lines)"
    docker compose -f docker/compose.yml down -v --remove-orphans "$@"
    ;;
  targets)
    script=
    for p in $ports; do script="$script s/@\\([^@:/]*\\):$p\\//@\\1:$((p + offset))\\//g;"; done
    sed "$script" targets.toml
    ;;
  env) env_lines ;;
  *) echo "unknown command: $cmd (up, down, targets, env)" >&2; exit 2 ;;
esac
