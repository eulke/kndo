# Limpiar comentarios: kndo-core plugin.rs, cache.rs, query.rs, query_envelope.rs, config.rs

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

`crates/kndo-core/src/{plugin,cache,query,query_envelope,config}.rs` — ¿todos sus comentarios
describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes). `config.rs` es donde vive `EffectiveConfig`
(`CLAUDE.md` → "Config") — no tocar la lógica de merge, solo sus comentarios. Solo edición de
comentarios.

## Resolution

(completar al cerrar)
