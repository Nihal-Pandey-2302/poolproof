#!/bin/sh
# Rebuilds results/ and RESULTS.md: runs every scenario against every target in
# targets.toml, once per Postgres version, on a fresh test bed each time.
# Usage: scripts/update-results.sh [postgres image tags...]
# PP_OFFSET=N uses the test bed at that offset (see scripts/testbed.sh); default 0.
set -eu
cd "$(dirname "$0")/.."
offset=${PP_OFFSET:-0}
[ $# -gt 0 ] || set -- 17.11 18.4
command -v jq >/dev/null || { echo "update-results.sh needs jq" >&2; exit 2; }
cargo build --release --locked -q
# New results go to *.new first and replace results/ only when every version is done.
targets=$(mktemp)
trap 'rm -f results/*.json.new "$targets"' EXIT
scripts/testbed.sh targets "$offset" > "$targets"
for version in "$@"; do
  echo "== Postgres $version"
  # testbed.sh down passes --profile '*', so variant and selftest services stop too.
  scripts/testbed.sh down "$offset" >/dev/null 2>&1
  POSTGRES_IMAGE="postgres:$version" scripts/testbed.sh up "$offset" >/dev/null
  # Exit 1 only means some scenario failed, which is expected. --check-scenarios runs
  # each scenario twice on Postgres first: one whose results differ between those runs
  # tests timing, not the pooler, and must not end up in results/.
  new="results/postgres-$version.json.new"
  ./target/release/poolproof run --targets "$targets" --check-scenarios --format json > "$new" || [ $? -eq 1 ]
  bad=$(jq -r '.results[] | select(.result == "error" and (.reason | startswith("nondeterministic scenario")))
               | "\(.scenario): \(.reason)"' "$new" | sort -u)
  if [ -n "$bad" ]; then
    printf 'Not deterministic on Postgres %s, results/ left as it was:\n%s\n' "$version" "$bad" >&2
    exit 1
  fi
done
for version in "$@"; do
  mv "results/postgres-$version.json.new" "results/postgres-$version.json"
done
./target/release/poolproof report results/postgres-*.json > RESULTS.md
echo "Wrote results/ and RESULTS.md"
