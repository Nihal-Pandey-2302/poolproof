#!/bin/sh
# Runs a separate copy of the test bed with every host port shifted by OFFSET, so
# several can run side by side. Offset 0 is the default test bed (project "poolproof",
# the ports in targets.toml).
# Usage: scripts/testbed.sh up|down|targets|env OFFSET
#   up       start it (extra arguments go to `docker compose up`)
#   down     stop it and remove its volumes
#   targets  print targets.toml with the ports shifted: scripts/testbed.sh targets 100 > t.toml
#   env      print the variables, to use docker compose directly: eval "$(scripts/testbed.sh env 100)"
#
# When several people share a machine, give each a fixed offset (100, 200, ...) and
# use only that one: `up` or `down` on an offset someone else uses replaces or stops
# their test bed. Offset 0 is the default bed.
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

case $cmd in
  up)
    eval "$(env_lines)"
    docker compose -f docker/compose.yml up -d --wait --wait-timeout 300 "$@"
    ;;
  down)
    eval "$(env_lines)"
    # Every profile, or services started with --profile (e.g. selftest) stay up.
    docker compose -f docker/compose.yml --profile '*' down -v --remove-orphans "$@"
    ;;
  targets)
    script=
    for p in $ports; do script="$script s/@\\([^@:/]*\\):$p\\//@\\1:$((p + offset))\\//g;"; done
    sed "$script" targets.toml
    ;;
  env) env_lines ;;
  *) echo "unknown command: $cmd (up, down, targets, env)" >&2; exit 2 ;;
esac
