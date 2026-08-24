# RFC 0019 — Installable Agent Skill

**Status:** Accepted & landed · **Depends on:** RFC 0006 (CLI & output — `--format agent`),
RFC 0007 (graph navigation)

## 1. Motivation

kndo already treats LLM coding agents as a first-class audience — `--format agent`, the six
navigation verbs, `kndo query` batching, stable finding ids — but that surface is discoverable
only by reading `docs/src/agents.md`. An agent dropped into a project that uses kndo has no
signal that the tool exists, no guidance to prefer graph navigation over `grep` for
symbol-level questions, and no prescribed procedure for resolving findings — so two agents
asked to "clean up the unused code kndo found" would improvise two different orders, two
different suppress/fix judgment calls, and produce inconsistent diffs.

The fix is to ship an **installable skill**: a small, versioned documentation package the CLI
writes into the target project, in the emerging cross-harness convention (`SKILL.md` +
on-demand reference files under `.agents/skills/<name>/`, with `.claude/skills/<name>`
sharing the same content). Once installed, any agent session that supports skills picks it up
automatically — no prompt engineering required per project.

## 2. What ships

`crates/kndo-cli/skill/`: `SKILL.md` (frontmatter + the always-loaded core — what kndo is,
the token-efficient navigation workflow, the check loop, exit codes, a load-on-demand table)
plus four reference files loaded only when needed:

- `references/navigation.md` — full verb/flag reference, the JSON envelope, `kndo query`
  grammar, orientation recipes.
- `references/findings-playbook.md` — the resolution playbook: work order
  (defect → waste → risk → hygiene → convention), confidence-gated verification
  (certain/probable/possible), a fix-vs-suppress decision tree, and one recipe per finding
  category (means / pre-check / recipe / verify), sourced from each category's "Fix:" line in
  `docs/src/rules.md`.
- `references/output-format.md` — the agent-format grammar as `agent_format.rs` actually
  emits it (no `fix:`/`cause:` lines — see §4), plus JSON/SARIF/env-var reference.
- `references/suppressions-and-baseline.md` — pragma syntax, binding rules, the baseline
  lifecycle, and an explicit note that an agent should not run `kndo baseline`/`--update` on
  its own initiative.

This is the most novel piece: without a prescribed playbook, "fix the findings" is
under-specified — an agent has to invent an order, a confidence policy, and a suppress
threshold every time. The playbook makes that reasoning reusable and auditable instead of
ad hoc.

## 3. Installation contract

`kndo agents install` (new subcommand, alongside `check`/`health`/`init`/… in `main.rs`):

1. Embeds the package via `include_str!` at compile time (`crates/kndo-cli/src/skill.rs`) —
   the installed skill always matches the binary version that wrote it (the SKILL.md
   frontmatter's `{{version}}` placeholder is substituted with `CARGO_PKG_VERSION` at install
   time).
2. Writes the package to `.agents/skills/kndo/` — the harness-neutral home.
3. Creates a **relative** symlink `.claude/skills/kndo → ../../.agents/skills/kndo`, so
   Claude Code and any other `.agents`-aware harness read one set of files. Where symlinks
   are unavailable (typically unprivileged Windows), falls back to a plain copy and says so
   in the output — the two copies then drift until the next `install` refreshes both.
4. Is **idempotent and update-in-place**: the installed files are kndo-owned, unlike the
   `init --hook` pre-commit hook, which is user-owned and never overwritten. Re-running
   `install` after a binary upgrade is the documented update path — it overwrites drifted or
   stale content and reports `installed` / `updated (N files)` / `up to date` accordingly.
5. **Refuses rather than clobbers** a foreign `.claude/skills/kndo` (anything that is not
   already the expected symlink) — same doctrine as the pre-commit hook: exit 2 with
   instructions, never silently overwrite something that might not be kndo's.
6. `kndo init` gained an advisory line pointing at `kndo agents install` when the skill is
   not yet present, mirroring the existing pre-commit-hook advisory.

The installed files are meant to be **committed** by the target project (unlike `.kndo/`,
which stays gitignored) — every install prints a line saying so.

## 4. A documentation bug the skill fixes in passing

`docs/src/agents.md`'s worked example showed `cause:`/`fix:` lines in the agent-format
output. `agent_format.rs`'s own module doc is explicit that `remediation` is not part of the
JSON, so the renderer never emits `fix:` lines — the skill's `output-format.md` documents the
grammar as implemented, and `docs/src/agents.md` is corrected to match (§5).

## 5. Non-goals

- No MCP server (`kndo serve`) — that remains a separate, larger piece of work tracked
  informally (see the roadmap); the skill only teaches the existing CLI surface.
- No skill *generation* from `docs/src/*.md` — the mdBook prose is written for humans reading
  linearly; the skill is hand-written to be prescriptive and progressively disclosed. A
  coverage test (`crates/kndo-cli/tests/agents_install.rs`) guards against drift by asserting
  every category heading in `docs/src/rules.md` has a matching playbook recipe, and every
  navigation verb named in the CLI's own usage text appears in `references/navigation.md` —
  presence-based, not semantic, but enough to catch an added/renamed category or verb.
- No interactive update flow (diff-and-confirm) — kndo's CLI is deliberately non-interactive
  throughout; overwrite-on-reinstall is the same posture as `kndo.toml`'s "already exists,
  left untouched" for user files versus this package's "kndo-owned, always current".

## 6. Acceptance

`crates/kndo-cli/tests/agents_install.rs`: package + symlink creation, version substitution,
idempotent re-run, drift restoration, refusal on a foreign `.claude/skills/kndo`, usage-error
handling for `kndo agents` without `install`, the `init` advisory in both states, and the two
drift guards described above. All green; `cargo clippy -p kndo-cli --all-targets -D warnings`
and `cargo fmt -p kndo-cli --check` clean.
