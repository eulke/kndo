#!/bin/sh
# Prove a test fails without its fix, without touching the working tree.
#
# Usage: verify-fix.sh <rev> <file-with-the-fix> -- <test command...>
#
# Copies <file> as it was at <rev> (pre-fix) into a temporary worktree of HEAD with
# only that file overlaid, runs the test command there, and reports. The real working
# tree is never modified.
set -eu

rev=${1:?usage: verify-fix.sh <rev> <file> -- <cmd...>}
file=${2:?usage: verify-fix.sh <rev> <file> -- <cmd...>}
shift 2
[ "${1:-}" = "--" ] && shift

tmp=$(mktemp -d)
trap 'git worktree remove --force "$tmp" 2>/dev/null || true; rm -rf "$tmp"' EXIT

git worktree add --detach --quiet "$tmp" HEAD
git show "$rev:$file" > "$tmp/$file"

echo "verify-fix: running in a temp worktree of HEAD with $file from $rev overlaid"
if (cd "$tmp" && "$@"); then
    echo "verify-fix: test PASSED without the fix — it does not cover the fix." >&2
    exit 1
else
    echo "verify-fix: test failed without the fix, as it should. Fix is covered."
fi
