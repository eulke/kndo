# Corregir vigencia: product/vision.md (falta HTML; modelo de plugins desactualizado)

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
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

(completar al cerrar)
