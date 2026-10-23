#!/usr/bin/env bash
# Fast tests by default; the full suite before a release.
#
#   scripts/test.sh          unit + UI tests (no external services, no screenshots)
#   scripts/test.sh full     everything, incl. Docker/S3/SSH/Git/terminal integration tests
#   MANTA_RENDER_DIR=dir scripts/test.sh full   also writes the UI screenshots
#   MANTA_TEST_CONTAINERS=1 scripts/test.sh full   also runs docker/kubectl tests
#                            (their own throwaway containers; without it no
#                            test ever calls the docker or kubectl CLI)
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "${1:-}" = full ]; then
    exec cargo test --workspace --no-fail-fast
fi
exec cargo test --workspace --lib --bins --no-fail-fast
