#!/usr/bin/env bash
# Frees disk space: removes dev/test build output (target/debug), finished agent
# worktrees under .claude/worktrees and stale render output. Release builds in
# target/dist and dist/ are kept. The next dev build recompiles (~5–10 min).
set -euo pipefail
cd "$(dirname "$0")/.."
before=$(df -k . | awk 'NR==2{print $4}')
cargo clean --profile dev 2>/dev/null || cargo clean
rm -rf target/ux-renders target/*-renders
if [ -d .claude/worktrees ]; then
    for w in .claude/worktrees/*; do
        [ -e "$w" ] || continue
        git worktree remove --force --force "$w" 2>/dev/null || rm -rf "$w"
    done
    git worktree prune
fi
after=$(df -k . | awk 'NR==2{print $4}')
echo "freed about $(( (after - before) / 1024 / 1024 )) GB (APFS may release space with a delay while Time Machine local snapshots exist)"
