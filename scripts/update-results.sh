#!/bin/sh
# Rebuilds results/ and RESULTS.md: runs every scenario against every target in
# targets.toml, once per Postgres version, on a fresh test bed each time.
# Usage: scripts/update-results.sh [postgres image tags...]
set -eu
cd "$(dirname "$0")/.."
compose="docker compose -f docker/compose.yml"
[ $# -gt 0 ] || set -- 17.11 18.4
cargo build --release --locked -q
for version in "$@"; do
  echo "== Postgres $version"
  $compose down -v --remove-orphans >/dev/null 2>&1
  POSTGRES_IMAGE="postgres:$version" $compose up -d --wait --wait-timeout 300 >/dev/null
  # Exit 1 only means some scenario failed, which is expected.
  ./target/release/poolproof run --format json > "results/postgres-$version.json" || [ $? -eq 1 ]
done
./target/release/poolproof report results/postgres-*.json > RESULTS.md
echo "Wrote results/ and RESULTS.md"
