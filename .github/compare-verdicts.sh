#!/usr/bin/env bash
# Compares the verdicts of a CI run with the committed results:
#   .github/compare-verdicts.sh [--common-only] COMMITTED.json NEW.json
# --common-only compares only the (target, scenario) pairs both files have, and lists
# the others in the job summary: for a PR that adds or changes scenarios before the
# file is regenerated.
# Only verdicts (result and FAIL kind) are compared, by target and scenario. Not compared:
# scenarios with repeat > 1 in NEW.json, whose worst case can differ between machines.
# Pairs in results/known-flaky.txt are compared loosely, as their line says:
#   verdict  PASS and FAIL (any kind) match each other; ERROR, SKIP or a missing
#            result still differ
#   kind     a FAIL matches a FAIL of any kind
#   reached  for a pair committed as PASS or FAIL: the same result matches (a FAIL of
#            any kind), and so does a SKIP whose reason starts "passed, but" (evidence
#            showed the clients never shared a server connection, so the run didn't
#            count); the other of PASS and FAIL, ERROR, other SKIPs and a missing
#            result still differ
# Exits 1 when verdicts differ, 2 on a bad or stale known-flaky line, 3 when a JSON file
# it reads isn't a readable report.
set -euo pipefail
# One collation for sort and comm.
export LC_ALL=C
common_only=false
if [ "${1:-}" = --common-only ]; then common_only=true; shift; fi
committed=$1 new=$2
flaky_file=results/known-flaky.txt
summary=${GITHUB_STEP_SUMMARY:-/dev/null}

# Every report read below, checked first: a file jq can't read would otherwise end the
# script with jq's own exit code, which can look like 1 or 2.
for f in "$committed" "$new" results/*.json; do
  if ! jq -e '.results | type == "array"' "$f" > /dev/null 2>&1; then
    echo "::error file=$f::can't read $f as a poolproof report"
    exit 3
  fi
done

# "<target> <scenario> <verdict|kind>", one per line. sub() drops a trailing \r, so a
# file saved with CRLF line ends still matches.
awk '{ sub(/\r$/, "") }
     NF && $1 !~ /^#/ && NF == 2 { bad = 1
       printf "::error file=%s,line=%d::add verdict or kind: %s\n", FILENAME, FNR, $0 }
     NF && $1 !~ /^#/ && NF != 2 && (NF != 3 || ($3 != "verdict" && $3 != "kind" && $3 != "reached")) { bad = 1
       printf "::error file=%s,line=%d::expected \"<target> <scenario> verdict|kind|reached\", got: %s\n", FILENAME, FNR, $0 }
     END { exit bad }' "$flaky_file" || exit 2
flaky=$(awk '{ sub(/\r$/, "") } NF == 3 && $1 !~ /^#/ { print $1 "\t" $2 "\t" $3 }' "$flaky_file" \
  | jq -Rsc 'split("\n") | map(select(. != "") | split("\t") | {target: .[0], scenario: .[1], mode: .[2]})')

# Stale lines. A pair that no committed results file has is a typo. A `kind` line is only
# right for a pair committed as FAIL, a `reached` line for one committed as PASS or
# FAIL, and a `verdict` line never for one committed as ERROR or SKIP. Lines for the
# other job's file are checked there.
stale=$(jq -rn --argjson flaky "$flaky" --slurpfile c "$committed" '
  [inputs | .results[] | "\(.target) \(.scenario)"] as $known
  | ($c[0].results | map({key: "\(.target) \(.scenario)", value: .result}) | from_entries) as $mine
  | $flaky[] | "\(.target) \(.scenario)" as $k
  | if ($known | index($k)) == null then "\($k): in no results/*.json"
    elif $mine[$k] == null then empty
    elif .mode == "kind" and $mine[$k] != "fail" then "\($k): kind, but committed as \($mine[$k])"
    elif .mode == "reached" and $mine[$k] != "fail" and $mine[$k] != "pass" then "\($k): reached, but committed as \($mine[$k])"
    elif .mode == "verdict" and ($mine[$k] == "error" or $mine[$k] == "skip") then "\($k): verdict, but committed as \($mine[$k])"
    else empty end' results/*.json)
if [ -n "$stale" ]; then
  while IFS= read -r line; do echo "::error file=$flaky_file::stale: $line"; done <<<"$stale"
  exit 2
fi

# The pairs this comparison has, for the job summary.
loose=$(jq -r --argjson flaky "$flaky" '
  [.results[] | "\(.target) \(.scenario)"] as $here
  | $flaky[] | select("\(.target) \(.scenario)" as $k | $here | index($k))
  | "- \(.target) \(.scenario): \(.mode)"' "$committed")
if [ -n "$loose" ]; then
  { echo "### Compared loosely (results/known-flaky.txt)"; echo "$loose"; } >> "$summary"
fi

# The timing list comes from this run, so a scenario that just got repeat > 1 is skipped
# on both sides before results/ is regenerated.
timing=$(jq -c '[.scenarios[]? | select((.repeat // 1) > 1) | .name]' "$new")
# The committed result of each pair, built inside jq from the committed file (a file
# passed as one argument could hit the system's limit): a `reached` pair's run that
# didn't count stands for whatever was committed, so both files read it the same way.
# shellcheck disable=SC2016 # jq, not the shell, expands these
mine='($c[0].results | map({key: "\(.target) \(.scenario)", value: .result}) | from_entries) as $mine'

# `reached` pairs that matched only because this run didn't count: if a pooler stops
# sharing for good, they keep matching, so the summary says so every time.
unreached=$(jq -r --argjson timing "$timing" --argjson flaky "$flaky" --slurpfile c "$committed" "$mine"' |
  .results[] | select(.scenario as $s | $timing | index($s) | not)
  | . as $r | "\(.target) \(.scenario)" as $k
  | select([$flaky[] | select(.target == $r.target and .scenario == $r.scenario and .mode == "reached")] | length > 0)
  | select(.result == "skip" and ((.reason // "") | startswith("passed, but")))
  | select($mine[$k] == "pass" or $mine[$k] == "fail")
  | "- \($k): \(.reason)"' "$new")
if [ -n "$unreached" ]; then
  { echo "### Matched only by a run that didn't count (reached)"; echo "$unreached"; } >> "$summary"
fi

verdicts() {
  jq -r --argjson timing "$timing" --argjson flaky "$flaky" --slurpfile c "$committed" "$mine"' |
    .results[] | select(.scenario as $s | $timing | index($s) | not)
    | . as $r | "\(.target) \(.scenario)" as $k
    | ([$flaky[] | select(.target == $r.target and .scenario == $r.scenario) | .mode] | first) as $mode
    | (.result == "skip" and ((.reason // "") | startswith("passed, but"))) as $uncounted
    | (if $mode == "verdict" and (.result == "pass" or .result == "fail") then "pass or fail (known flaky)"
       elif $mode == "kind" and .result == "fail" then "fail (known flaky)"
       elif $mode == "reached" and (.result == "pass" or .result == "fail")
         then "\(.result) or not reached (known flaky)"
       elif $mode == "reached" and $uncounted and ($mine[$k] == "pass" or $mine[$k] == "fail")
         then "\($mine[$k]) or not reached (known flaky)"
       else "\(.result) \(.mismatch.kind // "")" end) as $v
    | "\($k) \($v)"' "$1" | sort
}
if $common_only; then
  pairs() { jq -r '.results[] | "\(.target) \(.scenario)"' "$1" | sort -u; }
  only_new=$(comm -13 <(pairs "$committed") <(pairs "$new"))
  only_committed=$(comm -23 <(pairs "$committed") <(pairs "$new"))
  if [ -n "$only_new" ]; then
    { echo "### Not in the committed file yet"; while IFS= read -r p; do echo "- $p"; done <<<"$only_new"; } >> "$summary"
  fi
  if [ -n "$only_committed" ]; then
    { echo "### In the committed file, not in this run"; while IFS= read -r p; do echo "- $p"; done <<<"$only_committed"; } >> "$summary"
  fi
  # Both sides keep only the pairs the other has: "target scenario" is the start of a line.
  both=$(comm -12 <(pairs "$committed") <(pairs "$new"))
  # Nothing in common would compare nothing and pass.
  if [ -z "$both" ] && [ -n "$(pairs "$committed")" ]; then
    echo "::error::no target and scenario pair in common with the committed file"
    exit 1
  fi
  keep() { awk 'NR == FNR { k[$0]; next } ($1 " " $2) in k' <(printf '%s\n' "$both") -; }
  diff <(verdicts "$committed" | keep) <(verdicts "$new" | keep)
else
  diff <(verdicts "$committed") <(verdicts "$new")
fi
