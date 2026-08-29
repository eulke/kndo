# Limpiar comentarios: crates/kndo-cli/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-cli/` (6 archivos, ~400 líneas de comentario) — ¿todos sus comentarios describen
únicamente el comportamiento/invariante actual? Aplicar la política de `.wayfinder/map.md`
(Notes). Recordar la regla de fachada de `CLAUDE.md`: `kndo-cli` importa solo desde
`kndo::<Name>` — la limpieza de comentarios no toca esos imports, solo su redacción. Solo
edición de comentarios.

## Resolution

Comentarios reescritos en `main.rs`, `nav.rs`, `render.rs`. Los imports de fachada
(`kndo::<Name>`) no se tocaron. `cargo check --workspace` en verde.

