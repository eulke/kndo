# Limpiar comentarios: kndo-core engine.rs + adapter.rs

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-core/src/engine.rs` y `crates/kndo-core/src/adapter.rs` (los dos archivos con más
volumen de comentario del crate, ~1570 líneas juntos, incluye la superficie de `Engine` que
`CLAUDE.md` trata como contractual) — ¿todos sus comentarios describen únicamente el
comportamiento/invariante actual? Aplicar la política de `.wayfinder/map.md` (Notes). Si algún
rustdoc público documenta la superficie que `internal/contracts/core-traits.md` considera
normativa, la limpieza no puede cambiar lo que ese contrato promete — solo cómo se redacta.
Solo edición de comentarios.

## Resolution

Comentarios reescritos en `engine.rs` y `adapter.rs`, sin tocar ninguna firma ni lo que el
contrato de `internal/contracts/core-traits.md` promete — verificado línea por línea (todos los
cambios son de comentario, ninguno de código). `cargo check --workspace` en verde.

