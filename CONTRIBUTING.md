# Contributing

## Commits

[Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`.

- Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- Subject: imperative mood, ≤72 chars, no trailing period.
- Body: optional, **1–2 lines max**. State what changed and why if it's not obvious from the
  subject — not a design rationale. Extended discussion belongs in the PR description or the
  relevant `docs/` file, not the commit body.

```
feat(adapter-js): extract export surface from CJS module.exports
fix(cache): invalidate facts on grammar version bump
docs(rfc-0005): fix stale verdict count in taxonomy rule
```

## Branching — Gitflow

- `main` — always releasable; only tagged versions land here; merges only from `release/*` or
  `hotfix/*`.
- `develop` — integration branch; default base for new work.
- `feature/<short-name>` — branches off `develop`, merges back via PR.
- `release/<version>` — cut from `develop` to stabilize before a release; merges to `main` and
  back to `develop`.
- `hotfix/<short-name>` — branches off `main` for urgent fixes; merges to `main` and `develop`.

Delete a branch once merged.
