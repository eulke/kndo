# Limpiar comentarios: crates/kndo-adapter-rust/src/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-adapter-rust/src/` (5 archivos, ~1190 líneas de comentario; el grep inicial mostró
varios "used to be classified as test infrastructure" en `extraction.rs`) — ¿todos sus
comentarios describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes). No tocar `crates/kndo-adapter-rust/tests/` (fixtures de
conformance, `expected.json` es contrato — ver CONTRIBUTING.md — y su volumen de comentario es
prácticamente nulo). Solo edición de comentarios en `src/`.

## Resolution

Comentarios reescritos en `extraction.rs` y `lib.rs`; `tests/` sin tocar. `cargo check
--workspace` en verde.

