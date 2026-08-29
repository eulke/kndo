# Limpiar comentarios: crates/kndo/ (fachada)

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo/` (20 archivos, ~1000 líneas de comentario) — ¿todos sus comentarios describen
únicamente el comportamiento/invariante actual? Aplicar la política de `.wayfinder/map.md`
(Notes). Este crate incluye `tests/dogfood.rs` y `tests/builtin_plugin_proofs.rs`, que son
gates nombrados en `CLAUDE.md` ("Gates that must never regress") — limpiar sus comentarios sin
tocar ninguna aserción, el `ACCEPTED`/`PROVEN` list, ni la lógica de los tests. Solo edición de
comentarios en todo el crate.

## Resolution

Comentarios reescritos en `src/` (author.rs, lib.rs) y 13 archivos de `tests/` (fixtures sin
tocar). Verificado a mano, línea por línea, que `dogfood.rs`, `builtin_plugin_proofs.rs`,
`doc_links.rs`, `schema_validation.rs`, `plugin_dependency_implication.rs` y
`adapter_dependency_implication.rs` — los seis gates nombrados en `CLAUDE.md` que este ticket
toca — no cambiaron ninguna aserción, la lista `ACCEPTED`/`PROVEN`, ni lógica de test, solo
comentarios. Corrido explícitamente: `cargo test -p kndo --test dogfood` (2/2),
`--test doc_links` (1/1) — ambos en verde.

