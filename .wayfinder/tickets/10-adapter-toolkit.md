# Limpiar comentarios: crates/kndo-adapter-toolkit/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-adapter-toolkit/` (9 archivos, ~630 líneas de comentario) — ¿todos sus comentarios
describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes). Este crate es compartido por todos los adapters
(`CLAUDE.md` → "Adapters and the toolkit") — no tocar qué helpers expone, solo sus comentarios.

## Resolution

Comentarios reescritos en los 7 archivos que tenían algo que corregir (`classify.rs`,
`decls.rs`, `jvm_manifest.rs`, `lib.rs`, `metrics.rs`, `parsing.rs`, `refs.rs`). Ningún helper
expuesto cambió. `cargo check --workspace` en verde.

