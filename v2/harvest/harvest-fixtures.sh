#!/bin/sh
# Export v1's conformance fixtures, stdlib data, ABI reference components and example
# guests into a directory layout the v2 vendors as its regression corpus.
#
# Usage: ./harvest-fixtures.sh <v1-checkout> <output-dir>
set -eu

SRC=${1:?usage: harvest-fixtures.sh <v1-checkout> <output-dir>}
OUT=${2:?usage: harvest-fixtures.sh <v1-checkout> <output-dir>}

mkdir -p "$OUT"

for a in "$SRC"/crates/kndo-adapter-*/; do
    name=$(basename "$a" | sed 's/^kndo-adapter-//')
    if [ -d "$a/tests" ]; then
        mkdir -p "$OUT/fixtures/$name"
        cp -R "$a/tests/." "$OUT/fixtures/$name/"
    fi
    if [ -f "$a/src/stdlib.txt" ]; then
        mkdir -p "$OUT/stdlib"
        cp "$a/src/stdlib.txt" "$OUT/stdlib/$name.txt"
    fi
done

if [ -d "$SRC/crates/kndo-plugin-api/tests/compat" ]; then
    mkdir -p "$OUT/abi-compat"
    cp -R "$SRC/crates/kndo-plugin-api/tests/compat/." "$OUT/abi-compat/"
fi

if [ -d "$SRC/examples" ]; then
    mkdir -p "$OUT/example-guests"
    cp -R "$SRC/examples/." "$OUT/example-guests/"
fi

echo "harvested into $OUT:"
find "$OUT" -name expected.json | wc -l | xargs echo "  expected.json files:"
du -sh "$OUT" | cut -f1 | xargs echo "  total size:"
