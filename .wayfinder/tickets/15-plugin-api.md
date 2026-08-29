# Limpiar comentarios: crates/kndo-plugin-api/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-plugin-api/` (11 archivos, ~460 líneas de comentario; incluye `plugin_host.rs`,
donde el grep inicial mostró un "It used to be a table whose miss...") — ¿todos sus comentarios
describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes). Este crate define el ABI del plugin
(`internal/contracts/wasm-abi.md`, fuera de alcance) — la limpieza no cambia el ABI, solo la
redacción de sus comentarios.

## Resolution

Comentarios reescritos en `host.rs` y `plugin_host.rs`. El ABI del plugin (`internal/contracts/
wasm-abi.md`) no cambió. `cargo check --workspace` en verde.

