#!/usr/bin/env bash
# Run the test suite N times under parallel CPU load and report flake rates.
#
#   scripts/stress-tests.sh            5 runs of what scripts/test.sh runs
#   scripts/stress-tests.sh 10         10 runs
#   STRESS_LOAD=8 scripts/stress-tests.sh      8 busy-loop workers (default: CPU count)
#   STRESS_PACKAGES="-p manta" scripts/stress-tests.sh   only the app crate
#   STRESS_THREADS=4 scripts/stress-tests.sh   pass --test-threads=4
#   STRESS_FILTER=trash_ux scripts/stress-tests.sh  only tests matching a filter
#   STRESS_FULL=1 scripts/stress-tests.sh      also the integration tests (crates/*/tests,
#                                              like `scripts/test.sh full`)
#
# The busy loops (`yes > /dev/null`) compete with the tests for the CPU like a
# loaded CI runner does, which is where timing-dependent tests flake. The test
# binaries are built once and copied to target/stress/bin, so editing the
# sources while it runs doesn't change what is measured. Logs of every run are
# kept in target/stress/; the summary lists each failed test with how often it
# failed.
set -uo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)

runs="${1:-5}"
load="${STRESS_LOAD:-$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)}"
packages="${STRESS_PACKAGES:---workspace}"
filter="${STRESS_FILTER:-}"
kinds=(--lib --bins)
[ "${STRESS_FULL:-0}" = 1 ] && kinds=(--lib --bins --tests)
args=()
[ -n "$filter" ] && args+=("$filter")
[ -n "${STRESS_THREADS:-}" ] && args+=(--test-threads="$STRESS_THREADS")

target="${CARGO_TARGET_DIR:-$root/target}"
work="$target/stress"
rm -rf "$work"
mkdir -p "$work/bin"

echo "Building tests ($packages)…"
# One line per test binary: "<executable> <package dir>".
# shellcheck disable=SC2086
cargo test $packages "${kinds[@]}" --no-run --message-format=json -q |
    python3 -c '
import json, os, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["profile"]["test"]:
        print(m["executable"], os.path.dirname(m["manifest_path"]))
' >"$work/binaries.txt" || exit 1
while read -r exe dir; do
    cp "$exe" "$work/bin/"
    echo "$work/bin/$(basename "$exe") $dir"
done <"$work/binaries.txt" >"$work/run-list.txt"
echo "$(wc -l <"$work/run-list.txt" | tr -d ' ') test binaries"

pids=()
stop_load() {
    for p in "${pids[@]:-}"; do
        [ -n "$p" ] && kill "$p" 2>/dev/null
    done
    pids=()
}
trap stop_load EXIT INT TERM
for _ in $(seq 1 "$load"); do
    yes >/dev/null &
    pids+=($!)
done
echo "CPU load: $load busy workers; $runs runs"

failed_runs=0
for i in $(seq 1 "$runs"); do
    log="$work/run-$i.log"
    : >"$log"
    start=$(date +%s)
    status=ok
    while read -r exe dir; do
        # Like cargo: run in the package's directory.
        if ! (cd "$dir" && "$exe" ${args[@]+"${args[@]}"}) </dev/null >>"$log" 2>&1; then
            status=FAILED
        fi
    done <"$work/run-list.txt"
    [ "$status" = ok ] || failed_runs=$((failed_runs + 1))
    echo "run $i/$runs: $status ($(($(date +%s) - start)) s)"
done
stop_load

echo
echo "== Summary: $failed_runs of $runs runs had failures =="
# libtest lists the failed tests under a final "failures:" heading.
for f in "$work"/run-*.log; do
    awk '/^failures:$/ {f=1; next} f && /^    [A-Za-z_]/ {print $1} /^test result:/ {f=0}' "$f" | sort -u
done | sort | uniq -c | sort -rn |
    awk -v n="$runs" '{printf "  %-90s %d/%d\n", $2, $1, n}'
passed=$(grep -ho '[0-9]* passed' "$work"/run-*.log | awk '{s+=$1} END {print s+0}')
failed=$(grep -ho '[0-9]* failed' "$work"/run-*.log | awk '{s+=$1} END {print s+0}')
echo "  test executions: $((passed + failed)), failed: $failed"
[ "$failed_runs" -eq 0 ]
