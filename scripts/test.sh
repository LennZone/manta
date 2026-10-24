#!/usr/bin/env bash
# Fast tests by default; the full suite before a release.
#
#   scripts/test.sh          unit + UI tests (no external services, no screenshots)
#   scripts/test.sh full     everything, incl. Docker/S3/SSH/Git/terminal integration tests
#   MANTA_RENDER_DIR=dir scripts/test.sh full   also writes the UI screenshots
#   MANTA_TEST_CONTAINERS=1 scripts/test.sh full   also runs docker/kubectl tests
#                            (their own throwaway containers; without it no
#                            test ever calls the docker or kubectl CLI)
#   MANTA_TEST_THREADS=2 scripts/test.sh   fewer tests in parallel (default: one
#                            per CPU); MANTA_TEST_WAIT_SECS raises how long UI
#                            tests wait for background work (default 60)
#   scripts/stress-tests.sh  the same suite N times under CPU load (flake rates)
set -euo pipefail
cd "$(dirname "$0")/.."
args=()
if [ -n "${MANTA_TEST_THREADS:-}" ]; then
    args=(-- --test-threads="$MANTA_TEST_THREADS")
fi
if [ "${1:-}" = full ]; then
    exec cargo test --workspace --no-fail-fast ${args[@]+"${args[@]}"}
fi
exec cargo test --workspace --lib --bins --no-fail-fast ${args[@]+"${args[@]}"}
