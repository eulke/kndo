# Limpiar comentarios: crates/kndo/ (fachada)

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

`crates/kndo/` (20 archivos, ~1000 líneas de comentario) — ¿todos sus comentarios describen
únicamente el comportamiento/invariante actual? Aplicar la política de `.wayfinder/map.md`
(Notes). Este crate incluye `tests/dogfood.rs` y `tests/builtin_plugin_proofs.rs`, que son
gates nombrados en `CLAUDE.md` ("Gates that must never regress") — limpiar sus comentarios sin
tocar ninguna aserción, el `ACCEPTED`/`PROVEN` list, ni la lógica de los tests. Solo edición de
comentarios en todo el crate.

## Resolution

(completar al cerrar)
