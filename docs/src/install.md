# Installation

kndo is a single static binary with no runtime dependencies.

## Prebuilt binaries

Each release publishes prebuilt archives per platform on the project's GitHub releases page:
`kndo-<tag>-<target>.tar.gz` for Linux x86_64/aarch64 (musl — static, distribution-independent)
and macOS x86_64/arm64. The `<tag>` is the release tag verbatim, leading `v` included.

There is no Windows binary. It was a release target until the build was actually attempted: one
of the tree-sitter grammars kndo compiles hands the MSVC compiler a flag it refuses, so the
binary could not be produced at all. Building from source with `cargo install` hits the same
wall. On Windows, use WSL — the Linux musl archive runs there unchanged.

Every archive holds a single directory named for itself, so extraction strips one component.
Download, extract, and put `kndo` on your `PATH`:

```console
$ curl -fsSL https://github.com/eulke/kondo/releases/latest/download/kndo-v0.1.0-aarch64-apple-darwin.tar.gz \
    | tar -xz --strip-components=1
$ ./kndo --version
kndo 0.1.0 (schema 1.3.0)
```

Or let the installer pick the right one for your platform and verify its checksum:

```console
$ curl -fsSL https://raw.githubusercontent.com/eulke/kondo/main/install.sh | sh
```

In GitHub Actions you do not need to install anything by hand — the
[first-party Action](ci.md) downloads the release matching your platform (or builds from
source with `version: source`).

## From source

kndo builds with stable Rust:

```console
$ git clone https://github.com/eulke/kondo && cd kondo
$ cargo install --path crates/kndo-cli --locked
$ kndo --version
```

Embedders who want a smaller binary can build the distribution crate with a subset of
languages (`--no-default-features --features js,go,…`); the CLI's default build includes every
language and the WebAssembly plugin runtime.

## Requirements

- Diff modes (`--staged`, `--diff <ref>`) need `git` on `PATH` and a git repository. Full
  scans work anywhere, git or not.
- Nothing else. kndo never executes your project, never downloads its dependencies, and never
  phones home — it reads source files and manifests, and writes only under `.kndo/` in the
  project (plus its per-machine plugin directory if you install plugins).

## What kndo writes

| Path | What | Commit it? |
|---|---|---|
| `kndo.toml` | configuration, written by `kndo init` | yes |
| `.kndo/baseline.json` | acknowledged findings, written by `kndo baseline` | yes |
| `.kndo/cache/` | content-addressed analysis cache | no (`kndo init` adds `.kndo/` to `.gitignore`) |
| `.kndo/health.json` | last full-run health score, for trend display | no |
| `.kndo/plugins/` | project-local plugin/adapter components you drop in | your call |

Note: `kndo init` gitignores the whole `.kndo/` directory; if you adopt a baseline, force-add
it (`git add -f .kndo/baseline.json`) or keep an explicit `!.kndo/baseline.json` rule — the
baseline is meant to be reviewed and committed.

Next: [Getting started](getting-started.md).
