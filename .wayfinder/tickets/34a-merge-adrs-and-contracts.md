# Fusionar: ADRs → ADRS.md, contratos → CONTRACTS.md

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
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

(completar al cerrar)
