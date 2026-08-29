# Limpiar comentarios: crates/kndo-core/src/graph/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`crates/kndo-core/src/graph/` (7 archivos, ~1980 líneas de comentario entre `//` y rustdoc) —
¿todos sus comentarios describen únicamente el comportamiento/invariante actual, sin contrastar
con un estado anterior ni narrar cuándo o por qué cambió? Aplicar la política de
`.wayfinder/map.md` (Notes) archivo por archivo. Solo edición de comentarios — el código y los
tests no cambian de comportamiento.

## Resolution

Reescritos/borrados los comentarios que contrastaban con un estado anterior en
`assemble.rs`, `provenance.rs`, `surface.rs` y `tests.rs` (narración de "se dividió del
`graph.rs` original", "antes era un SymbolId, ahora es un tipo", "dos consumidores que antes
tenían solo uno de ellos", etc.) — reescritos como invariante presente o borrados si la
narración de reorganización de archivos no dejaba contenido presente que salvar. `mod.rs`,
`patch.rs` y `plugin_round.rs` no necesitaron cambios (sus pasados narran un paso previo dentro
de la misma función, no historia del código). Verificado: `cargo check --workspace` y
`cargo test -p kndo-core` (585 tests) en verde.

