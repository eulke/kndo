# Limpiar comentarios: xtask/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`xtask/` (5 archivos, ~290 líneas de comentario; incluye `xtask::package`, que
`CLAUDE.md` trata como el único productor del artefacto de release) — ¿todos sus comentarios
describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes). Solo edición de comentarios.

## Resolution

Comentarios reescritos en `main.rs` y `package.rs`. `cargo check --workspace` en verde.

