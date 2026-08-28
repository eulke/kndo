# Limpiar comentarios: kndo-core, resto de archivos top-level de src/

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

`crates/kndo-core/src/{agent_format,baseline,conformance,coverage,delta,discovery,gitutil,lib,
paths,plugin_gate,rkyv_support,sarif,suppression,testkit,vocab}.rs` (15 archivos que no entran
en los tickets 05/06) — ¿todos sus comentarios describen únicamente el comportamiento/invariante
actual? Aplicar la política de `.wayfinder/map.md` (Notes). `testkit.rs` es el `testkit` que
`CLAUDE.md` exige usar en tests (`MockAdapter` y compañía) — no tocar su comportamiento. Solo
edición de comentarios.

## Resolution

(completar al cerrar)
