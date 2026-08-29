# Limpiar comentarios: examples/ + spikes/perf/src/main.rs

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`examples/*/src/` (5 crates demo, ~165 líneas de comentario) y `spikes/perf/src/main.rs`
(~30 líneas de comentario) — ¿todos sus comentarios describen únicamente el
comportamiento/invariante actual? Aplicar la política de `.wayfinder/map.md` (Notes). El
código del spike es chico y su propósito ya quedó fijado en `internal/spikes/0001-performance.md`
(fuera de alcance) — alcanza con limpiar sus comentarios inline, no hace falta tocar su
estructura. Solo edición de comentarios.

## Resolution

Sin cambios: ningún archivo de `examples/*/src/` ni `spikes/perf/src/main.rs` tenía comentarios
que violaran la política (volumen bajo, ~195 líneas de comentario en total, ya presentes y
correctos). Nada que commitear.

