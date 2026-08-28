#!/bin/sh
# kndo installer (RFC 0014 §3.2). Downloads a precompiled release binary, verifies its checksum,
# and installs it — no cargo/rustc required. Safe to re-run: it replaces cleanly, never appends.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/eulke/kondo/main/install.sh | sh
#
# Env overrides:
#   KNDO_VERSION      pin a specific release tag (e.g. "v1.2.0"); default: latest
#   KNDO_INSTALL_DIR  install location; default: "$HOME/.local/bin"
#   KNDO_BASE_URL     fetch the archive and checksums.txt from here instead of the project's
#                     GitHub releases — an internal mirror, or a staging directory served over
#                     HTTP. Requires KNDO_VERSION (there is no releases API to ask). CI uses it
#                     to install from the artifact it just built: the layout this script assumes
#                     is a contract, and until something ran it end to end, nothing checked it.
set -eu

REPO="eulke/kondo"
INSTALL_DIR="${KNDO_INSTALL_DIR:-$HOME/.local/bin}"

log() { printf '%s\n' "$*" >&2; }
die() { log "kndo: error: $*"; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not found on PATH"
}

need curl
need tar

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os/$arch" in
        Linux/x86_64) echo "x86_64-unknown-linux-musl" ;;
        Linux/aarch64 | Linux/arm64) echo "aarch64-unknown-linux-musl" ;;
        Darwin/x86_64) echo "x86_64-apple-darwin" ;;
        Darwin/arm64) echo "aarch64-apple-darwin" ;;
        *)
            die "unsupported platform: $os/$arch (supported: Linux x86_64/aarch64, macOS \
x86_64/arm64 — Windows users, see https://github.com/$REPO#installation for the .zip release)"
            ;;
    esac
}

TARGET=$(detect_target)

resolve_version() {
    if [ -n "${KNDO_VERSION:-}" ]; then
        echo "$KNDO_VERSION"
        return
    fi
    curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name":' \
        | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
}

VERSION=$(resolve_version)
[ -n "$VERSION" ] || die "could not resolve a release version (network issue, or no releases published yet)"

ARCHIVE="kndo-${VERSION}-${TARGET}.tar.gz"
BASE_URL="${KNDO_BASE_URL:-https://github.com/$REPO/releases/download/$VERSION}"

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

log "kndo: downloading $ARCHIVE ($VERSION, $TARGET)…"
curl -fsSL -o "$WORK_DIR/$ARCHIVE" "$BASE_URL/$ARCHIVE" \
    || die "download failed: $BASE_URL/$ARCHIVE"
curl -fsSL -o "$WORK_DIR/checksums.txt" "$BASE_URL/checksums.txt" \
    || die "download failed: $BASE_URL/checksums.txt"

verify_checksum() {
    expected=$(grep " ${ARCHIVE}\$" "$WORK_DIR/checksums.txt" | awk '{print $1}')
    [ -n "$expected" ] || die "no checksum entry found for $ARCHIVE in checksums.txt"

    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$WORK_DIR/$ARCHIVE" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$WORK_DIR/$ARCHIVE" | awk '{print $1}')
    else
        die "neither 'sha256sum' nor 'shasum' found — cannot verify the download"
    fi

    [ "$expected" = "$actual" ] || die "checksum mismatch for $ARCHIVE — aborting, nothing installed \
(expected $expected, got $actual)"
}

log "kndo: verifying checksum…"
verify_checksum

mkdir -p "$INSTALL_DIR"
# The archive nests everything under a single directory named for itself
# ("kndo-v1.2.0-x86_64-apple-darwin/kndo") — strip it, or the binary is not where we look.
tar -xzf "$WORK_DIR/$ARCHIVE" -C "$WORK_DIR" --strip-components=1
[ -f "$WORK_DIR/kndo" ] || die "$ARCHIVE did not contain a kndo binary where one was expected"
cp "$WORK_DIR/kndo" "$INSTALL_DIR/kndo"
chmod +x "$INSTALL_DIR/kndo"

log "kndo: installed to $INSTALL_DIR/kndo"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        log ""
        log "  $INSTALL_DIR is not on your PATH. Add this to your shell profile:"
        log ""
        log "    export PATH=\"$INSTALL_DIR:\$PATH\""
        log ""
        ;;
esac

"$INSTALL_DIR/kndo" --version 2>/dev/null || true
log ""
log "Run 'kndo doctor' in a project to see what it detects."
