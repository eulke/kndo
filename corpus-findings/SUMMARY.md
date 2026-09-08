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
| Alamofire | 561 | 444 | 3284 | 90378 | 2324 | 0 | 467 | 5 | 11 |
| Exposed | 5485 | 5150 | 20249 | 345849 | 80955 | 0 | 966 | 5 | 64 |
| flask | 226 | 105 | 1123 | 18898 | 226 | 162 | 20 | 5 | 3 |
| gin | 127 | 99 | 1271 | 38999 | 230 | 0 | 109 | 1 | 1 |
| guava | 3359 | 3278 | 66904 | 890153 | 13063 | 0 | 8247 | 5 | 15 |
| lodash | 155 | 65 | 143 | 135328 | 49 | 28 | 20 | 2 | 0 |
| ripgrep | 231 | 110 | 2447 | 61733 | 312 | 54 | 141 | 1 | 0 |
| vapor | 281 | 254 | 2933 | 49254 | 9638 | 0 | 180 | 5 | 26 |
| vite | 2763 | 1945 | 3598 | 89380 | 2476 | 161 | 690 | 11 | 21 |
