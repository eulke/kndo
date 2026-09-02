# Vendored crates

`tree-sitter-scss/` is crates.io's `tree-sitter-scss 1.0.0` (MIT, by Amaan Qureshi, from
<https://github.com/tree-sitter-grammars/tree-sitter-scss>), verbatim but for one line of
`bindings/rust/build.rs`: the `-Wno-unused-parameter` warning flag is passed with
`flag_if_supported`, because MSVC's `cl.exe` refuses it and upstream hands it over
unconditionally — the one line that kept kndo off Windows. The workspace's
`[patch.crates-io]` points the dependency here; nothing else about the grammar changes.
