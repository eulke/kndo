# Limpiar comentarios: crates/kndo-adapter-css/ + kndo-adapter-html/ + kndo-adapter-json/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-adapter-css/`, `crates/kndo-adapter-html/` y `crates/kndo-adapter-json/` (juntos
~240 líneas de comentario, los tres adapters más chicos) — ¿todos sus comentarios describen
únicamente el comportamiento/invariante actual? Aplicar la política de `.wayfinder/map.md`
(Notes) a cada crate por separado. Solo edición de comentarios.

## Resolution

Comentarios reescritos en los tres adapters (`lib.rs` de cada uno). `cargo check --workspace`
en verde.

