# Corregir vigencia: product/vision.md (falta HTML; modelo de plugins desactualizado)

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

§4 ("Supported languages") lista 8 lenguajes y omite HTML, el noveno adapter, ya activado por
defecto (el propio código en `crates/kndo/src/lib.rs:56` todavía dice "eight feature-gated
languages" — corregir ahí también si el resolver de este ticket lo nota, aunque ese archivo le
corresponde al ticket 07 de limpieza de comentarios; si ya está cerrado, coordinar). El
Principio #3 subestima el modelo de plugins actual: RFC 0018 (aceptado y aterrizado) permite
que un plugin de terceros emita findings de primera clase bajo su propio namespace
`plugin:<coordinate>/<rule>`, no solo contribuir roots/edges/anotaciones a verdictos del core.
Corregir ambas cosas; el resto del documento (tabla de detecciones, números de perf, no-goals)
ya verificó vigente.

## Resolution

§4 agrega HTML a los lenguajes soportados. Principio #3 actualizado para mencionar que un
plugin puede emitir findings de primera clase bajo su propio namespace (RFC 0018), no solo
contribuir a verdictos del core. `cargo test -p kndo doc_links` en verde.

