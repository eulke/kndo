# Limpiar comentarios: crates/kndo-adapter-js/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-adapter-js/` (6 archivos, ~630 líneas de comentario) — ¿todos sus comentarios
describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes). No tocar fixtures de conformance si las hubiera bajo `tests/`.
Solo edición de comentarios.

## Resolution

Comentarios reescritos en `extraction.rs`, `lib.rs`, `manifest.rs`. `cargo check --workspace`
en verde.

