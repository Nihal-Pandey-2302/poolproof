#!/bin/bash
# Runs the suite against PgDog built from a git ref (a tag, branch or commit), to test a
# fix before it's released or to find the commit that changed a verdict.
# Usage: scripts/pgdog-ref.sh REF [OFFSET] [-- poolproof run arguments...]
#   REF     what to build: v0.1.62, main, a commit, or a branch of PGDOG_REPO
#   OFFSET  the test bed to use (see scripts/testbed.sh); default 0. It must not be
#           running: the script starts it and takes it down, volumes and all.
# Environment:
#   PGDOG_REPO      the repository to build from; default https://github.com/pgdogdev/pgdog.git.
#                   A local path works too (e.g. your fork's checkout); it's only read,
#                   so only committed refs are seen.
#   PGDOG_PROFILE   release (default) or debug. A debug build is faster to make, but
#                   compare it only with other debug builds: timing differs.
#   POSTGRES_IMAGE  the Postgres image behind PgDog; default postgres:17.11
# The source, the builds and the results stay under ${XDG_CACHE_HOME:-~/.cache}/poolproof/pgdog.
# Building a ref runs its code (build scripts, proc macros, then PgDog itself), so build
# only refs you trust, and read a fork's branch first. The first build takes several
# minutes, and rustup may install PgDog's pinned toolchain; later builds reuse both.
# Results are labeled git-<commit> and never go into results/. To compare two refs, run
# both and then
#   poolproof report A.json B.json --format html > compare.html
# Only Postgres runs in Docker; PgDog runs on this machine. The pool has one server
# connection, in transaction mode, like the test bed's PgDog targets. Linux and macOS.
set -eu
[ $# -ge 1 ] || { sed -n '4,7s/^# \{0,1\}//p' "$0" >&2; exit 2; }
ref=$1
shift
# Never let a ref be read as a git option.
case $ref in -*) echo "REF can't start with -" >&2; exit 2 ;; esac
offset=0
case ${1:-} in --) ;; "") ;; *) offset=$1; shift ;; esac
[ "${1:-}" != -- ] || shift
case $offset in *[!0-9]*) echo "OFFSET must be a number" >&2; exit 2 ;; esac
repo=${PGDOG_REPO:-https://github.com/pgdogdev/pgdog.git}
# A local path is relative to where the script was started.
[ ! -d "$repo" ] || repo=$(cd "$repo" && pwd)
cd "$(dirname "$0")/.."
profile=${PGDOG_PROFILE:-release}
case $profile in release) flag=--release ;; debug) flag= ;; *) echo "PGDOG_PROFILE must be release or debug" >&2; exit 2 ;; esac
image=${POSTGRES_IMAGE:-postgres:17.11}
cache=${XDG_CACHE_HOME:-$HOME/.cache}/poolproof/pgdog
for tool in git cargo docker jq; do
  command -v $tool >/dev/null || { echo "pgdog-ref.sh needs $tool" >&2; exit 2; }
done
# The script takes its test bed down when it's done, so never borrow one that's running.
eval "$(scripts/testbed.sh env "$offset")"
# By label, so containers of any profile count, and a docker error isn't read as "free".
ids=$(docker ps -a -q --filter "label=com.docker.compose.project=$COMPOSE_PROJECT_NAME") ||
  { echo "docker ps failed" >&2; exit 2; }
if [ -n "$ids" ]; then
  echo "test bed $offset is in use; pick another offset" >&2
  exit 2
fi

# Source: one clone, reused. Fetch every time, so branches move.
mkdir -p "$cache"
if [ ! -d "$cache/src/.git" ]; then
  git clone --quiet --no-hardlinks "$repo" "$cache/src"
else
  git -C "$cache/src" remote set-url origin "$repo"
  git -C "$cache/src" fetch --quiet --tags --force origin '+refs/heads/*:refs/remotes/origin/*'
fi
sha=$(git -C "$cache/src" rev-parse --verify --quiet "origin/$ref^{commit}" ||
  git -C "$cache/src" rev-parse --verify --quiet "$ref^{commit}") || { echo "unknown ref: $ref" >&2; exit 2; }
short=$(git -C "$cache/src" rev-parse --short=12 "$sha")
label=git-$short

# Build, unless this commit is already built. Cargo uses PgDog's own rust-toolchain.toml.
bin=$cache/bin/pgdog-$short-$profile
if [ ! -x "$bin" ]; then
  echo "== Building PgDog $ref ($label, $profile)"
  git -C "$cache/src" -c advice.detachedHead=false checkout --quiet --detach "$sha"
  (cd "$cache/src" && cargo build --locked $flag -p pgdog)
  mkdir -p "$cache/bin"
  cp "$cache/src/target/${profile}/pgdog" "$bin"
fi
cargo build --locked --release -q

# Postgres from the test bed, PgDog next to it.
pg_port=$((55432 + offset))
port=$((56459 + offset))
run=$cache/run-$offset
mkdir -p "$run" "$cache/results"
cat > "$run/pgdog.toml" <<EOF
[general]
host = "127.0.0.1"
port = $port
default_pool_size = 1
min_pool_size = 0
pooler_mode = "transaction"

[[databases]]
name = "app"
host = "127.0.0.1"
port = $pg_port
EOF
cp docker/pgdog/users.toml "$run/users.toml"
cat > "$run/targets.toml" <<EOF
[[target]]
name = "pgdog-$label"
product = "pgdog"
version = "$label"
config = "default"
pooler = "postgres://postgres:postgres@127.0.0.1:$port/app?sslmode=disable"
direct = "postgres://postgres:postgres@127.0.0.1:$pg_port/app?sslmode=disable"
mode = "transaction"
reset = "all"
EOF
pid=
# shellcheck disable=SC2329 # called by the trap
cleanup() {
  [ -z "$pid" ] || { kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null || true; }
  scripts/testbed.sh down "$offset" >/dev/null 2>&1 || true
}
trap cleanup EXIT
if ! POSTGRES_IMAGE=$image docker compose -f docker/compose.yml up -d --wait --wait-timeout 300 postgres \
  > "$run/compose.log" 2>&1; then
  echo "Postgres didn't start:" >&2
  cat "$run/compose.log" >&2
  exit 1
fi
(cd "$run" && exec "$bin" --config pgdog.toml --users users.toml) > "$run/pgdog.log" 2>&1 &
pid=$!
tries=0
until (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; do
  tries=$((tries + 1))
  if ! kill -0 "$pid" 2>/dev/null || [ $tries -gt 150 ]; then
    echo "PgDog didn't start; its log:" >&2
    tail -n 20 "$run/pgdog.log" >&2
    exit 1
  fi
  sleep 0.2
done

# Exit 1 from run only means some scenario failed.
out=$cache/results/$label-$profile-${image##*:}.json
echo "== Running the suite against PgDog $ref ($label) on ${image}"
status=0
./target/release/poolproof run --targets "$run/targets.toml" --format json "$@" > "$out" || status=$?
[ $status -le 1 ] || exit $status
./target/release/poolproof report "$out"
echo "Results: $out"
exit $status
