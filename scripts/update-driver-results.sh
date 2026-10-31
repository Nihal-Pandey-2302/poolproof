#!/bin/sh
# Rebuilds results/drivers-postgres-17.11.json and DRIVERS.md: builds the driver images,
# then runs every driver case on the targets in drivers/targets.txt, on a fresh test bed.
# Usage: scripts/update-driver-results.sh
set -eu
cd "$(dirname "$0")/.."
compose="docker compose -f docker/compose.yml"
version=17.11
command -v jq >/dev/null || { echo "update-driver-results.sh needs jq" >&2; exit 2; }
cargo build --release --locked -q
docker buildx bake -f drivers/docker-bake.hcl --load
targets=$(awk 'NF && $1 !~ /^#/ { printf "--target %s ", $1 }' drivers/targets.txt)
new="results/drivers-postgres-$version.json.new"
trap 'rm -f "$new"' EXIT
$compose down -v --remove-orphans >/dev/null 2>&1
POSTGRES_IMAGE="postgres:$version" $compose up -d --wait --wait-timeout 300 >/dev/null
# Exit 1 only means some case failed, which is expected. --check-scenarios runs each case
# twice on Postgres first: one whose results differ tests timing, not the pooler.
# shellcheck disable=SC2086 # one word per flag and name
./target/release/poolproof drivers --check-scenarios $targets --format json > "$new" || [ $? -eq 1 ]
bad=$(jq -r '.results[] | select(.result == "error" and (.reason | startswith("nondeterministic scenario")))
             | "\(.scenario): \(.reason)"' "$new" | sort -u)
if [ -n "$bad" ]; then
  printf 'Not deterministic on Postgres %s, results/ left as it was:\n%s\n' "$version" "$bad" >&2
  exit 1
fi
mv "$new" "results/drivers-postgres-$version.json"
./target/release/poolproof report results/drivers-postgres-*.json > DRIVERS.md
echo "Wrote results/drivers-postgres-$version.json and DRIVERS.md"
