# Agent harness

The v1 operational vices this neutralizes, each a recorded incident: two sessions ate
their own work verifying fixes via `git checkout`; a stash mishap deleted ten fixture
corpora and the baseline; the commit log ended at ~45% conventional-commit compliance
and git-cliff had to be configured to absorb it.

Activate before the first v2 push:

```sh
./install.sh        # sets core.hooksPath to harness/hooks for this checkout
```

and for agent sessions, prepend the wrapper to PATH so destructive verification is
refused with guidance instead of relied on not to happen:

```sh
export PATH="$(pwd)/harness/bin:$PATH"
```

## Pieces

- `hooks/commit-msg` — rejects non-conventional commit messages (`type(scope): subject`).
  This is the enforceable half of commit hygiene; CI lints the same rule at M0.
- `bin/git` — a passthrough wrapper that refuses `git stash` and
  `git checkout -- <path>` / `git checkout HEAD -- <path>` (the
  discard-working-tree forms), pointing at `verify-fix.sh` instead. Branch switching and
  every other git use passes through untouched. Enforcement note, recorded honestly:
  git has no pre-checkout/pre-stash hook, so a hook cannot block these — the wrapper is
  the enforceable mechanism for agent shells, and the judgment rule in `CLAUDE.md`
  covers humans.
- `verify-fix.sh` — proves "the test fails without the fix" without touching the working
  tree: copies the pre-fix version of a file out of git history into a temp overlay,
  runs the given test command against it, restores nothing because nothing was changed.
