# Corregir vigencia: RFC 0010, 0011, 0014 (CI/Action, workspaces, release)

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

- **RFC 0010** (`internal/rfcs/0010-ci-github-action.md`): §4 describe un "budget checklist"
  con checkboxes por regla `[delta]` en el comentario de PR — `action/render.mjs` no implementa
  nada de eso (solo tabla de findings + diagnósticos + conteo de suprimidas). El budget real se
  renderiza como tabla ok/FAIL en el CLI (`render.rs::budget_block`), nunca en la Action.
  Corregir §4 para reflejar lo que realmente se manda al PR.
- **RFC 0011** (`0011-workspaces-and-monorepos.md`): §5 documenta un override de config por
  paquete (`[package."@org/legacy-ui"]` con `mode`/`skip`) que no existe — no está en
  `config::LIVE_TABLES`, ni en la plantilla de `kndo init`, ni hay tipo `PackageMode` en el
  código. Marcar como no implementado o quitar el ejemplo. El resto del RFC (selectors
  `pkg:`/`dep:`, `--by-package`, `deep-import`) sigue vigente.
- **RFC 0014** (`0014-distribution-and-release.md`): nota menor — el aside de §0 sobre
  `Cargo.toml`'s `repository` field apuntando a un nombre futuro ya no aplica, `Cargo.toml` ya
  usa el remoto real (`eulke/kondo`). Quitar o actualizar esa nota; el resto del RFC (pipeline
  nunca corrido en un tag real, targets, secrets pendientes) sigue vigente.

## Resolution

(completar al cerrar)
