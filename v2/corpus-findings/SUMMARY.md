# v2 corpus measurement

The default adapter set over the corpus pinned in `corpus/corpus.toml`.
Regenerate with `cargo xtask corpus --corpus-dir <clones>`; the oracle to
compare against is `oracle/`. A repo with zero claimed files speaks a
language no default adapter claims yet; an `unused` abstention means the
graph has no roots (root evidence arrives with manifest capabilities), so
the analysis declines to judge rather than accuse everything.

| repo | discovered | claimed | decls | refs | import edges | unresolved | findings | abstentions | diagnostics |
|---|---|---|---|---|---|---|---|---|---|
| Alamofire | 555 | 10 | 6 | 33326 | 0 | 0 | 5 | 3 | 0 |
| Exposed | 5476 | 7 | 30 | 57519 | 0 | 0 | 0 | 3 | 0 |
| gin | 118 | 99 | 1205 | 43995 | 230 | 0 | 108 | 0 | 0 |
| guava | 3352 | 2 | 0 | 11 | 0 | 0 | 0 | 3 | 0 |
| lodash | 146 | 54 | 133 | 134577 | 34 | 8 | 21 | 0 | 0 |
| ripgrep | 231 | 110 | 2447 | 61733 | 306 | 54 | 143 | 0 | 0 |
| vapor | 273 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| vite | 2712 | 1553 | 3491 | 87573 | 1911 | 261 | 895 | 0 | 6 |
