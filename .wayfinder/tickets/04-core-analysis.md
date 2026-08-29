# Limpiar comentarios: crates/kndo-core/src/analysis/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-core/src/analysis/` (17 archivos, ~1490 líneas de comentario) — ¿todos sus
comentarios describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes) archivo por archivo. Prestar atención particular a
`private_type_leak.rs` y `untested.rs`, que en el relevamiento inicial mostraron varios
comentarios "used to be"/"no longer". Solo edición de comentarios.

## Resolution

Reescritos/borrados los comentarios de 14 de los 17 archivos (`deep_import.rs`,
`dependency_hygiene.rs`, `test_only.rs` no necesitaron cambios). `private_type_leak.rs` y
`untested.rs` tenían, como se esperaba, varios "used to be"/"no longer" explícitos. Efecto
colateral detectado y corregido: `vocab.rs` (ticket 07) reescribió el rustdoc de `Related`,
`Group`, `Location` y `Severity`, y esos comentarios se embeben en los JSON Schema generados
(`schemars`) — se regeneró `schemas/*.json` (commit aparte) para que `schema_validation`
siguiera en verde. Verificado: `cargo check --workspace` y `cargo test -p kndo-core` en verde.

