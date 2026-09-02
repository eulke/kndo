# Install

kndo is one static binary. Releases publish an archive per target, named
`kndo-<tag>-<target>.tar.gz` and holding one directory with the `kndo` binary:

| Target | Archive |
|---|---|
| Linux x86_64 | `kndo-<tag>-x86_64-unknown-linux-musl.tar.gz` |
| Linux aarch64 | `kndo-<tag>-aarch64-unknown-linux-musl.tar.gz` |
| macOS x86_64 | `kndo-<tag>-x86_64-apple-darwin.tar.gz` |
| macOS arm64 | `kndo-<tag>-aarch64-apple-darwin.tar.gz` |

The Linux binaries are statically linked (musl) and run on any distribution.

## The installer

```sh
curl -fsSL https://raw.githubusercontent.com/eulke/kondo/main/install.sh | sh
```

It detects the platform, downloads the archive for the latest release, verifies
its checksum against the release's `checksums.txt`, and installs `kndo` into
`$HOME/.local/bin`. It is safe to re-run. Three variables change its mind:

| Variable | Meaning |
|---|---|
| `KNDO_VERSION` | a release tag, verbatim (`v1.2.0`); default: the latest release |
| `KNDO_INSTALL_DIR` | where the binary goes; default `$HOME/.local/bin` |
| `KNDO_BASE_URL` | fetch the archive and `checksums.txt` from a mirror or a directory served over HTTP instead of GitHub releases (requires `KNDO_VERSION`) |

## Homebrew

```sh
brew install eulke/tap/kndo
```

The formula downloads the same release archive for your platform; nothing
compiles on your machine.

## From source

Building needs the Rust toolchain the repository pins (`rust-toolchain.toml`);
rustup picks it up automatically.

```sh
cargo install --git https://github.com/eulke/kondo kndo-cli
```

The [MCP server](agents.md) is a second binary, `kndo-serve`, built the same
way (`cargo install --git https://github.com/eulke/kondo kndo-serve`); the
release archives carry `kndo` alone.

Windows is tested in CI on every push but has no release archive yet; build
from source there.

## Verify

```sh
kndo --version
kndo doctor
```

`doctor` prints what kndo sees from the current directory: every extension
loaded and its version, whether a `kndo.toml`, a cache or a baseline exists.
