#!/bin/sh
# Activate the harness for this checkout: hooks path + executable bits.
set -eu
cd "$(git rev-parse --show-toplevel)"
chmod +x harness/hooks/commit-msg harness/bin/git harness/verify-fix.sh harvest/harvest-fixtures.sh oracle/run-oracle.sh
git config core.hooksPath harness/hooks
echo "harness active: core.hooksPath -> harness/hooks"
echo "for agent sessions also run: export PATH=\"$(pwd)/harness/bin:\$PATH\""
