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
| Alamofire | 561 | 444 | 3272 | 87176 | 2324 | 0 | 591 | 5 | 11 |
| Exposed | 5485 | 5150 | 11673 | 236954 | 80955 | 0 | 986 | 5 | 64 |
| flask | 226 | 105 | 1104 | 18899 | 226 | 162 | 28 | 5 | 0 |
| gin | 127 | 99 | 1205 | 43995 | 230 | 0 | 108 | 1 | 0 |
| guava | 3359 | 3278 | 66898 | 890153 | 13063 | 0 | 9925 | 5 | 13 |
| lodash | 155 | 65 | 133 | 134577 | 48 | 28 | 19 | 2 | 0 |
| ripgrep | 231 | 110 | 2447 | 61733 | 309 | 54 | 155 | 1 | 0 |
| vapor | 281 | 254 | 2898 | 43955 | 9638 | 0 | 216 | 5 | 26 |
| vite | 2736 | 1946 | 3494 | 87576 | 2340 | 165 | 828 | 11 | 19 |
