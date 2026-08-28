# Reescribir CLAUDE.md: sin narración histórica, con la política de comentarios

- Status: open
- Type: wayfinder:task (HITL — mostrar el borrador de CLAUDE.md al usuario antes de commitear)
- Assignee: unassigned
- Blocked by: none

## Question

`CLAUDE.md` es el ejemplo más visible de lo que este mapa limpia: cada regla se justifica
narrando un incidente puntual ("found during a full ergonomics audit", "we once had
`Engine::explain` documented and never implemented", la branch
`claude/core-api-ergonomics-architecture-983pom`, "six jobs went red on a `useless_conversion`
that 1.98 reports and 1.94 does not"). ¿Puede reescribirse cada regla y cada gate nombrado
como instrucción presente ("nunca hagas X", "Y vive en Z", "el gate `nombre` verifica Z"),
manteniendo la regla y su severidad intactas pero sin el incidente que la originó — y agregar
una sección nueva que fije la política de comentarios de `.wayfinder/map.md` (Notes) para el
codebase Rust?

Alcance: solo `CLAUDE.md`. No tocar `CONTRIBUTING.md` (referencia a CLAUDE.md, no repite su
contenido) ni ningún doc de `internal/` (fuera de alcance, ver Out of scope del mapa).
Verificar en particular la sección "Gates that must never regress": los nombres de gate deben
seguir siendo exactamente los que `.github/workflows/ci.yml` verifica por nombre — no renombrar
nada, solo su justificación.

## Resolution

(completar al cerrar)
