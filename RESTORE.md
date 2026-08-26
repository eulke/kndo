# Three commits, carried by hand

This branch is a courier, not work. Delete it once the commits are restored.

The session that produced them lost its git credentials mid-run: `git push` and
even `git ls-remote` answered `could not read Username for 'https://github.com'`,
while ordinary egress still worked unauthenticated (`api.github.com` -> 403).
The GitHub API channel still had credentials, so the commits travelled through it
as a gzipped patch rather than as retyped source. That choice is deliberate: a
corrupt patch fails loudly at `git am`, whereas retyped Rust could land subtly
broken on the working branch.

## Restore

```sh
git fetch origin tmp/w5-w7-transfer
git checkout claude/core-api-ergonomics-architecture-983pom   # at 3283bb2

for f in part-00 part-01 p1 p2 part-02b part-03; do
  git show origin/tmp/w5-w7-transfer:$f
done | base64 -d | gunzip > /tmp/w5-w7.patch

sha256sum /tmp/w5-w7.patch
# must be 835b67c135fdd70e637537cd92748e74586358601eff7546bc7030f0d9d2596b

git am /tmp/w5-w7.patch
git push origin claude/core-api-ergonomics-architecture-983pom
git push origin --delete tmp/w5-w7-transfer
```

**Verify the sha256 before applying.** A mismatch means the blob was mangled in
transit; discard it and ask for it again rather than repairing it by hand.

The part order above is not alphabetical and that is not a typo — `p1` and `p2`
replace the first 21 lines of what was originally one `part-02`.

## The parts, and why there are six of them

| file | git blob hash | lines of the base64 |
|---|---|---|
| `part-00` | `4d90cdaa07ad146741f969a43cdd76c2d817a6a4` | 1-41 |
| `part-01` | `0f93b61e881f7a2daec4f4a14dd421455d14b2cd` | 42-82 |
| `p1` | `7abe9e1b666e19c7fb27cf7735095c6d292890e7` | 83-93 |
| `p2` | `21d84792dddbd7b6e3acd348c5820c4b83191424` | 94-103 |
| `part-02b` | `755d5eb19c9ee19caba45e1570ac10f21e2722a2` | 104-123 |
| `part-03` | `9a4ac78ce52f5a27a122585a2c33c78129c1d2dc` | 124-164 |

The first attempt sent the whole 16548-byte blob in one piece and it arrived 56
bytes short. A 123-byte probe then round-tripped exactly, which ruled out the
transport and pointed at transcription — so the payload was split into parts
small enough to check individually against `git hash-object`. Three of four
matched immediately; `part-02` did not, and resending it whole reproduced the
same wrong hash, so it was halved, and halved again, until every piece verified.
That is why the table has six rows instead of four.

## What is in the patch

```
b102e50 refactor(plugin-api): a conventions plugin's descriptor is one call
d951128 fix(analysis): a finding may not say more than its evidence
5b9b347 docs(gaps): §15's mechanic verified and its Rust instance re-measured
```

All three were verified before travelling: `cargo fmt`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, the full test suite,
the 72 conformance fixtures byte-identical, and kndo on itself at 49 findings /
96.8.
