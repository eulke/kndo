# Reescribir CLAUDE.md: sin narración histórica, con la política de comentarios

- Status: closed
- Type: wayfinder:task (HITL — mostrar el borrador de CLAUDE.md al usuario antes de commitear)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4)
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
contenido). Verificar en particular la sección "Gates that must never regress": los nombres de
gate deben seguir siendo exactamente los que `.github/workflows/ci.yml` verifica por nombre —
no renombrar nada, solo su justificación.

Además (decisión del ticket 33): agregar una regla nueva, generalizando "Contract first" a los
10 documentos fijos de `internal/` que definió esa decisión — todo PR que cambia el
comportamiento que uno de esos documentos describe, actualiza ese documento en el mismo PR.
Mencionar el gate de CI que lo respalda (ticket 35) igual que se menciona `doc_links` en "Gates
that must never regress", aunque ese gate todavía no exista al resolver este ticket (créalo con
redacción presente — "el gate X verifica Y" — no lo condiciones a que 35 haya cerrado).

Si el ticket 34a (fusión de contratos) ya cerró cuando se resuelva este, la referencia exacta a
`internal/contracts/core-traits.md` debe apuntar a `internal/CONTRACTS.md` (sección "Core
traits") en vez de al archivo viejo — si 34a todavía no cerró, dejar la referencia vieja y que
34a la actualice él mismo al fusionar (evitar que los dos tickets se pisen: el que cierra
segundo aplica el ajuste sobre el resultado del primero).

## Resolution

Reescrito `CLAUDE.md` completo. Se quitó la referencia a la branch
`claude/core-api-ergonomics-architecture-983pom` del header y cada incidente narrado puntual
(9 casos: `Engine::explain`, las copias CLI-side de group ordering, el merge duplicado de
config, la race de `temp_dir()`, el detalle de `-gnu`/staged-directory del release, la lista
"had never executed/installed/built/run", el `useless_conversion` de 1.98 vs 1.94, el
descriptor que casi esconde `dependencies`, y los dos links rotos de `doc_links`) — en todos
los casos se conservó la regla y su severidad, solo se reescribió en presente.

Se agregaron dos secciones:
- **"Keep internal/ current"**, generalizando "Contract first" a los 10 documentos que decidió
  el ticket 33 — pero redactada como *"every document `internal/README.md` indexes"* en vez de
  nombrar los 10 archivos directamente, porque esos archivos todavía no existen (34a-34h no
  corrieron): nombrarlos ahora habría sido exactamente el error que este mapa corrige. La regla
  es cierta hoy y sigue siendo cierta después de la fusión sin necesitar otro edit.
- **"Comments"** — la política de comentarios del mapa (presente, nunca pasado/futuro, aplica
  igual a `///` y `//`) más una segunda parte sobre dónde: solo comentar cuando el código solo
  no alcanza para un WHY no obvio.

Desviación deliberada de la instrucción original del ticket: no se agregó un nombre de gate de
CI para el ticket 35 en "Gates that must never regress" — ese gate no existe todavía, y
agregarlo habría sido la misma contaminación que "Contract first" prohíbe. El ticket 35 agrega
su propio nombre a esa lista cuando el gate exista de verdad.

La referencia a `internal/contracts/core-traits.md` en "Contract first" se dejó igual (el
ticket 34a, que fusiona los contratos, todavía no cerró) — 34a la actualiza él mismo al
fusionar, como preveía este ticket.
