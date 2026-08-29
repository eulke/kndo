# Limpiar comentarios: kndo-core, resto de archivos top-level de src/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-core/src/{agent_format,baseline,conformance,coverage,delta,discovery,gitutil,lib,
paths,plugin_gate,rkyv_support,sarif,suppression,testkit,vocab}.rs` (15 archivos que no entran
en los tickets 05/06) — ¿todos sus comentarios describen únicamente el comportamiento/invariante
actual? Aplicar la política de `.wayfinder/map.md` (Notes). `testkit.rs` es el `testkit` que
`CLAUDE.md` exige usar en tests (`MockAdapter` y compañía) — no tocar su comportamiento. Solo
edición de comentarios.

## Resolution

Comentarios reescritos en 10 de los 15 archivos del lote (`coverage.rs`, `delta.rs`,
`gitutil.rs`, `lib.rs`, `sarif.rs` no tenían nada que corregir). `testkit.rs` no cambió de
comportamiento. Efecto colateral: la reescritura del rustdoc de `Related`/`Group`/`Location`/
`Severity` en `vocab.rs` dejó desactualizados los JSON Schema comprometidos
(`schemas/kndo-output.schema.json`, `schemas/kndo-query-output.schema.json`, generados vía
`schemars` a partir de esos mismos doc comments) — detectado por `schema_validation.rs` fallando
2 de 6 tests, regenerados con `cargo xtask gen-schema` y commiteado aparte. `cargo test -p kndo
--test schema_validation` (6/6) y `cargo check --workspace` en verde.

