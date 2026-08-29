# Limpiar comentarios: crates/kndo-adapter-swift/ + crates/kndo-adapter-go/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-adapter-swift/` y `crates/kndo-adapter-go/` (juntos ~635 líneas de comentario) —
¿todos sus comentarios describen únicamente el comportamiento/invariante actual? Aplicar la
política de `.wayfinder/map.md` (Notes) a cada crate por separado (agrupados solo por tamaño
del ticket). Solo edición de comentarios.

## Resolution

Comentarios reescritos en ambos adapters (`extraction.rs`/`lib.rs` de cada uno). `cargo check
--workspace` en verde.

