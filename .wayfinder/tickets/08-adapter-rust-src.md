# Limpiar comentarios: crates/kndo-adapter-rust/src/

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

`crates/kndo-adapter-rust/src/` (5 archivos, ~1190 líneas de comentario; el grep inicial mostró
varios "used to be classified as test infrastructure" en `extraction.rs`) — ¿todos sus
comentarios describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes). No tocar `crates/kndo-adapter-rust/tests/` (fixtures de
conformance, `expected.json` es contrato — ver CONTRIBUTING.md — y su volumen de comentario es
prácticamente nulo). Solo edición de comentarios en `src/`.

## Resolution

(completar al cerrar)
