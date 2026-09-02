#!/usr/bin/env bash
# Clones the oracle corpus at its pinned commits. corpus.toml is the one source of
# pins; this script only reads it. A checkout already at its pin is left alone, so a
# cached corpus directory is reused as-is.
#
# Usage: clone.sh <dest-dir>
set -euo pipefail

dest="${1:?usage: clone.sh <dest-dir>}"
toml="$(cd "$(dirname "$0")" && pwd)/corpus.toml"
mkdir -p "$dest"

names=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$toml")
githubs=$(sed -n 's/^github = "\(.*\)"$/\1/p' "$toml")
commits=$(sed -n 's/^commit = "\(.*\)"$/\1/p' "$toml")

paste <(printf '%s\n' "$names") <(printf '%s\n' "$githubs") <(printf '%s\n' "$commits") |
while IFS=$'\t' read -r name github commit; do
  [ -n "$name" ] || continue
  dir="$dest/$name"
  if [ -d "$dir/.git" ] && [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" = "$commit" ]; then
    echo "$name: already at ${commit:0:12}"
    continue
  fi
  rm -rf "$dir"
  mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" remote add origin "https://github.com/$github.git"
  git -C "$dir" fetch -q --depth 1 origin "$commit"
  git -C "$dir" checkout -q --detach FETCH_HEAD
  echo "$name: fetched ${commit:0:12}"
done
