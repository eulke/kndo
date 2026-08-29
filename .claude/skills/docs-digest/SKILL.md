---
name: docs-digest
description: Capture durable knowledge from a session into kndo's design-doc tree (or promote it to the public manual when it's user-relevant), and audit the two trees for duplicated content. Use after finishing substantial work in this repository, or when asked to check docs for drift.
---

# docs-digest

Runs the documentation discipline `CLAUDE.md`'s "Internal notes" section states: a fact lives in
exactly one place, is written where its actual audience can find it, and never accumulates as a
second copy of something already said elsewhere.

Two modes. Default is capture; pass `audit` as an argument to run the other.

## Default mode — capture and promote

1. **Gather candidates.** Look at what actually changed: `git diff` against the branch's
   merge-base (or a base ref passed as an argument), plus the current conversation's own
   decisions and discoveries — a diff shows *what* changed, the conversation carries *why*,
   which a diff alone never does.

2. **Filter for durability.** For each candidate, ask: is this a decision, an invariant, or a
   gotcha a future session would otherwise have to rediscover — or is it narration of what was
   done, already covered by the commit history? Keep only the former. Bug fixes, routine
   refactors, and anything a well-named function already makes obvious are not durable knowledge;
   skip them.

3. **Triage each durable fact.** Is it relevant to someone outside the team building kndo — an
   end user, a plugin or adapter author — rather than only to a maintainer? If so, it belongs in
   the public manual (`docs/`). Otherwise, it belongs in the design-doc tree.

4. **Check for existing coverage before writing anything.** Grep both the public manual and the
   design-doc tree for the fact's keywords. If it's already stated, update that passage in place
   (or skip entirely if it's already accurate) instead of adding a second copy anywhere.

5. **Write to the design-doc tree.** Use its top-level README's document table to pick the one
   correct file among its fixed set — never create a new file. Edit the existing section that
   already covers the topic; add a new subsection only when genuinely nothing does.

6. **Write to the public manual.** Find the existing page/section the fact belongs under and
   extend it in the manual's own voice. Never cite the design-doc tree, a retired RFC/ADR number,
   or a section-sign reference while doing so — a citation gate enforces this, but a draft should
   never rely on the gate to catch its own mistake.

7. **Verify before reporting.** Run the citation gate
   (`cargo test -p kndo --all-features --test internal_boundary`), and the link-resolution gate
   (`cargo test -p kndo --all-features --test doc_links`) if the public manual changed. Never
   commit on your own — this repository's rule is commit only when explicitly asked.

8. **Report.** A short summary: what was captured and where, what was promoted (and from where),
   and what was judged not durable or already covered — with why.

## Audit mode

An occasional, on-demand check, not something to run by default: read through the public manual
and the design-doc tree side by side, looking for passages that state the same fact in both.
This is a judgment call, not a text match — report candidate pairs for review rather than
editing anything. The person reviewing decides which side should own the content; consolidating
without asking risks deleting the more accurate of the two copies.
