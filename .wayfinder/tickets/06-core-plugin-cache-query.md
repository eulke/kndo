# Limpiar comentarios: kndo-core plugin.rs, cache.rs, query.rs, query_envelope.rs, config.rs

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-core/src/{plugin,cache,query,query_envelope,config}.rs` — ¿todos sus comentarios
describen únicamente el comportamiento/invariante actual? Aplicar la política de
`.wayfinder/map.md` (Notes). `config.rs` es donde vive `EffectiveConfig`
(`CLAUDE.md` → "Config") — no tocar la lógica de merge, solo sus comentarios. Solo edición de
comentarios.

## Resolution

Comentarios reescritos en `plugin.rs`, `cache.rs`, `query.rs`, `query_envelope.rs`, `config.rs`.
La lógica de merge de `EffectiveConfig` no se tocó (verificado). `cargo check --workspace` en
verde.

