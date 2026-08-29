#!/bin/sh
# Activate the harness for this checkout: hooks path + executable bits.
set -eu
cd "$(git rev-parse --show-toplevel)"
chmod +x v2/harness/hooks/commit-msg v2/harness/bin/git v2/harness/verify-fix.sh v2/harvest/harvest-fixtures.sh v2/oracle/run-oracle.sh
git config core.hooksPath v2/harness/hooks
echo "harness active: core.hooksPath -> v2/harness/hooks"
echo "for agent sessions also run: export PATH=\"$(pwd)/v2/harness/bin:\$PATH\""
