# Corregir vigencia: adapters/java.md (fixture faltante y capacidad no documentada)

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

§6 dice "Four fixtures" pero lista cinco, y hay una sexta en disco que el doc no menciona en
ningún lado: `tests/fixtures/multi-release-variants`, que ejercita layouts de Multi-Release JAR
(dos variantes de la misma clase marcadas `internal-only` como symbol twins) — una capacidad
real y no trivial que ningún lector puede saber que existe. Corregir el conteo de §6 y agregar
una entrada para esta fixture (en §0, §5 o §7, la que mejor encaje con el resto del documento).

El §0/§4 de este doc también se restatea en `internal/ROADMAP.md`'s "M5 progress — Java
adapter" (misma narrativa de diseño) — no es necesario tocar eso acá, es un aparte menor que el
ticket 31 (ROADMAP) puede recortar si hay tiempo.

## Resolution

§6 corregido a "Six fixtures", con `multi-release-variants` agregada y descripta. `cargo test
-p kndo doc_links` en verde.

