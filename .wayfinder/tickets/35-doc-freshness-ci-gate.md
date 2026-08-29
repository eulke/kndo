# Gate de CI: ruta de crate → documento fijo que debe acompañarla

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4)
- Blocked by: 34h-rebuild-readme.md

## Question

Con los 10 documentos fijos ya existiendo: ¿puede agregarse un check (nuevo test en
`crates/kndo/tests/` o un script de `xtask`, lo que encaje mejor con los gates existentes) que
mantenga una tabla explícita `ruta de crate → documento de internal/` (p.ej.
`crates/kndo-core/src/plugin.rs` → `internal/PLUGINS.md`, `crates/kndo-adapter-*/` →
`internal/ADAPTERS.md`) y falle cuando un diff toca una ruta mapeada sin tocar su documento
correspondiente? Circunscribir el mapeo a las rutas donde tiene señal real (los `internal/`
crates/módulos con contrato o diseño documentado) — no forzar cobertura de cada archivo del
workspace. Documentar el gate en `CLAUDE.md` junto a la regla de "todo PR que cambia
comportamiento actualiza su documento" (agregada en el ticket 01), siguiendo el mismo patrón
que "Gates that must never regress".

## Resolution

`cargo xtask check-doc-freshness` nuevo: tabla `DOC_COVERAGE` (ruta → doc de `internal/`),
deliberadamente chica; corre en el job `gates` de CI solo en eventos `pull_request` (necesita
`GITHUB_BASE_REF`). Agregado a "Gates that must never regress" en `CLAUDE.md`. 8 tests nuevos,
incluida una prueba con un repo git real para la semántica `base...HEAD`. Corrida real contra
`origin/main` sobre todo el diff de esta sesión: "clean" — confirma que cada ticket anterior
mantuvo su doc sincronizado.

