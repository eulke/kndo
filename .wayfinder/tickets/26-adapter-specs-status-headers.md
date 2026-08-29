# Corregir vigencia: adapters/css.md + adapters/json.md (headers de estado obsoletos)

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

Ambos specs dicen **"Status: Draft, pre-implementation"** pero ambos adapters están
completamente implementados y shippeados (`kndo-adapter-css`, `kndo-adapter-json`, con sus
fixtures de conformance pasando). Los demás specs igualmente implementados (java.md, go.md,
js-ts.md) usan solo "Draft" sin el calificador "pre-implementation" — esa es la convención
real de estado en este repo (Draft es permanente, no es señal de "no implementado"). Quitar
"pre-implementation" de ambos headers.

Además, `adapters/css.md` §0 se superpone con la sección "M5 progress — CSS adapter" de
`internal/ROADMAP.md` (casi el mismo argumento sobre `reachability.rs` y los edges implícitos,
casi palabra por palabra). Dejar `css.md` como fuente canónica de ese argumento — no lo toques
acá, se recorta desde el lado del ROADMAP (ticket 31).

## Resolution

(completar al cerrar)
