#!/bin/sh
# Kept for old habits: the release pipeline is scripts/build-release.sh (output in dist/).
exec "$(dirname "$0")/build-release.sh" "$@"
