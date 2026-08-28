# Limpiar comentarios: crates/kndo-plugin-api/

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

`crates/kndo-plugin-api/` (11 archivos, ~460 líneas de comentario; incluye `plugin_host.rs`,
donde el grep inicial mostró un "It used to be a table whose miss...") — ¿todos sus comentarios
describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes). Este crate define el ABI del plugin
(`internal/contracts/wasm-abi.md`, fuera de alcance) — la limpieza no cambia el ABI, solo la
redacción de sus comentarios.

## Resolution

(completar al cerrar)
