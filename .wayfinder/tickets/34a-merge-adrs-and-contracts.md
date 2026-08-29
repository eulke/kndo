# Fusionar: ADRs → ADRS.md, contratos → CONTRACTS.md

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4)
- Blocked by: 25-adrs-linking-cache.md

## Question

Con ADR 0003 y 0004 ya corregidos (ticket 25) y los 3 contratos ya verificados vigentes: ¿puede
crearse `internal/ADRS.md` con las 7 ADRs como secciones (`## ADR 0001: ...` etc., cada una
conserva su número y su inmutabilidad una vez `Accepted`) y `internal/CONTRACTS.md` con los 3
contratos como secciones (`## Core traits`, `## Output schema (JSON)`, `## WASM ABI`)? Borrar
los 10 archivos originales (`internal/adrs/*.md`, `internal/contracts/*.md`) y las carpetas
vacías. Actualizar la referencia exacta de `CLAUDE.md`'s "Contract first" a
`internal/contracts/core-traits.md` → sección "Core traits" de `internal/CONTRACTS.md`
(coordinar con el ticket 01, que también edita `CLAUDE.md` — si 01 ya cerró, aplicar el cambio
sobre su resultado). Correr `cargo test -p kndo doc_links` al final — cualquier link real que
apuntara a las rutas viejas tiene que quedar corregido.

## Resolution

`internal/ADRS.md` y `internal/CONTRACTS.md` creados, 10 archivos originales borrados.
Encontré y corregí una dependencia funcional real: `crates/kndo/tests/schema_validation.rs` y
`crates/kndo-core/src/vocab.rs` leían `internal/contracts/output-schema.md` en disco en un test
de verdad, no solo en un comentario — actualizados a `internal/CONTRACTS.md`, ambos tests
verificados en verde. Se descubrió una contaminación en el primer intento (worktrees basados en
un commit anterior a toda la sesión, pisando las 22 correcciones de vigencia ya hechas) —
detectado antes de commitear, revertido, y re-corrido sin aislamiento de worktree. `CLAUDE.md`
y `docs/src/plugins/authoring.md` actualizados a la nueva ruta.

