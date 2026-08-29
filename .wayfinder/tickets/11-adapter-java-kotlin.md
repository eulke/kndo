# Limpiar comentarios: crates/kndo-adapter-java/ + crates/kndo-adapter-kotlin/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-adapter-java/` y `crates/kndo-adapter-kotlin/` (juntos ~525 líneas de comentario)
— ¿todos sus comentarios describen únicamente el comportamiento/invariante actual? Aplicar la
política de `.wayfinder/map.md` (Notes) a cada crate por separado (son adapters distintos, no
comparten código — agrupados acá solo por tamaño del ticket). Solo edición de comentarios.

## Resolution

Solo `kndo-adapter-kotlin/src/extraction.rs` tenía comentarios que corregir; `kndo-adapter-java`
ya estaba limpio. `cargo check --workspace` en verde.

