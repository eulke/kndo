# Limpiar comentarios: kndo-core engine.rs + adapter.rs

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
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

(completar al cerrar)
