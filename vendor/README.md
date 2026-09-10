# Vendored crates

A crate kndo pins is sometimes wrong about a platform in a way no caller can work
around: the fix belongs in the crate. Vendoring is how kndo carries that fix — and the
discipline here exists so that carrying it never means carrying an unexplained tree.

Vendoring is NOT how kndo handles a grammar that lags the language. A grammar patched
per construct is one patch per language release, and what the engine needs from a
reader is not that it never fails but that it SAYS where it failed — see the entry on
unread text in DECISIONS. A grammar patch shrinks that region; it never decides a
verdict.

Each `vendor/<crate>/` is a crates.io release **plus the changes its provenance record
names, and nothing else**. The record is `vendor/<crate>.provenance.toml`, written by

    cargo xtask vendor --crate <crate> --version <version>

from the release archive: every file's upstream digest, and for each file the tree
changes, both digests plus a `why` a person writes. The upstream copy of every changed
file is kept under `vendor/upstream/<crate>/`, so the diff a reviewer reads is

    diff -u vendor/upstream/<crate>/<path> vendor/<crate>/<path>

No `.patch` is stored, on purpose: a stored diff can come to disagree with the tree it
claims to describe, and two files that both exist cannot. The workspace's
`[patch.crates-io]` points the dependency at the vendored tree.

`vendored_trees_are_upstream_plus_patches` reads the record back with no archive and no
network, so CI checks the claim on every run: a file edited without being recorded, a
recorded change with no reason, a missing upstream copy and a file added to or dropped
from the tree each fail it. Regenerating the record after an intentional change is one
command, and the `why` survives it.
