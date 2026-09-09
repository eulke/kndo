# v2 corpus measurement

The default adapter set over the corpus pinned in `corpus/corpus.toml`.
Regenerate with `cargo xtask corpus --corpus-dir <clones>`; the oracle to
compare against is `oracle/`. A repo with zero claimed files speaks a
language no default adapter claims yet; an `unused` abstention means the
graph has no roots — nothing in the tree (manifest entries, convention
roots, dispatch anchors) said where execution starts, so the analysis
declines to judge rather than accuse everything.

| repo | discovered | claimed | decls | refs | import edges | unresolved | findings | abstentions | diagnostics |
|---|---|---|---|---|---|---|---|---|---|
| Alamofire | 561 | 444 | 3284 | 90378 | 2324 | 0 | 1484 | 5 | 11 |
| Exposed | 5485 | 5150 | 20249 | 345849 | 82818 | 0 | 965 | 5 | 64 |
| flask | 226 | 105 | 1123 | 18898 | 197 | 162 | 19 | 5 | 3 |
| gin | 127 | 99 | 1271 | 38999 | 230 | 0 | 109 | 1 | 1 |
| guava | 3359 | 3278 | 66904 | 890153 | 659229 | 0 | 8235 | 5 | 15 |
| lodash | 155 | 65 | 143 | 135328 | 49 | 28 | 20 | 2 | 0 |
| ripgrep | 231 | 110 | 2447 | 61751 | 312 | 54 | 143 | 1 | 0 |
| vapor | 281 | 254 | 2933 | 49254 | 9638 | 0 | 732 | 5 | 26 |
| vite | 2763 | 1945 | 3598 | 89380 | 2590 | 161 | 712 | 11 | 21 |
