# Installation

## From source (today)

kndo is a single static binary built with Rust:

```console
$ git clone https://github.com/eulke/kondo && cd kondo
$ cargo install --path crates/kndo-cli --locked
$ kndo --version
```

## Packaged installs

Release artifacts (prebuilt binaries per platform, an `install.sh`, and a Homebrew tap) ship
with the first tagged release — the pipeline exists in-repo (`.github/workflows/release.yml`)
and activates on the first `v*` tag.

## Requirements

- Diff modes (`--staged`, `--diff <ref>`) need `git` on PATH and a repository.
- Nothing else: kndo never executes your project, downloads its dependencies, or phones home.
