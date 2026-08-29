# Mapa: limpieza de comentarios y documentación interna

Tracker: Markdown local (no se configuró un tracker en este repo, ver `SKILL.md` de wayfinder
→ "si no hay tracker, default a local-markdown"). Convención de este tracker, documentada acá
porque no hay un doc de tracker separado:

- `.wayfinder/map.md` — este archivo, el mapa.
- `.wayfinder/tickets/NN-slug.md` — cada ticket, con esta cabecera:
  ```
  - Status: open | closed
  - Type: wayfinder:task | wayfinder:research | wayfinder:prototype | wayfinder:grilling
  - Assignee: unassigned | <marca de quién lo tomó>
  - Blocked by: none | <lista de archivos de ticket>
  ```
  seguida de `## Question` y, al cerrarlo, `## Resolution`.
- Reclamar un ticket = poner algo en `Assignee` **antes** de tocar nada.
- Un ticket está **unblocked** cuando todo lo listado en `Blocked by` tiene `Status: closed`.
- El **frontier** = tickets `open`, `unblocked`, `Assignee: unassigned`.

## Destination

El codebase de kndo (`crates/`, `xtask/`, `examples/`, `spikes/perf/`) y `CLAUDE.md` quedan sin
narración histórica: todo comentario que sobrevive describe solo lo que es cierto ahora — un WHY
no obvio, una invariante, una restricción — nunca contrasta con un estado pasado ("antes X, ahora
Y", "ya no", "se agregó para") ni promete uno futuro, y no queda ninguna referencia a un incidente,
branch, PR o sesión puntual. `CLAUDE.md` conserva cada regla y cada gate que ya tiene, reescritos
en presente, más una sección nueva que fija esta misma política de comentarios y una regla que
generaliza "Contract first" a los 10 documentos fijos en los que termina consolidado
`internal/` (ver Notes). `internal/` se corrige primero donde quedó desactualizado (22 de 39
documentos lo estaban) y luego se consolida a esos 10 documentos fijos más
`detection-gaps.md` aparte — nada se pierde, se reorganiza para que se pueda mantener al día.
`internal/plan-detection-gap-fixes.md` se retira. `docs/` no se toca. Al cerrar el último
ticket de este mapa, el propio tracker (`.wayfinder/`) se borra del repo.

## Notes

**Dominio:** workspace Rust de `kndo-core` (analizador estático multi-lenguaje). 26 crates,
236 archivos `.rs`, ~14.000 líneas de comentario (8.893 `///`/`//!` de rustdoc + 5.080 `//`
inline) al momento de armar este mapa.

**Este esfuerzo ejecuta, no solo decide** (override explícito de la regla por defecto de
wayfinder "Plan, don't do"): cada ticket, salvo el de cierre, es un lote de limpieza real —
se resuelve editando y comiteando el código, no dejando una spec para después.

**Política de comentarios (ya decidida — no volver a discutirla por ticket):**
Un comentario (tanto `//` inline como `///`/`//!` de rustdoc) dice solo lo que es cierto en este
momento. Si borrar las palabras "antes", "ahora", "previamente", "ya no", "se agregó/cambió
para" deja el comentario sin sentido, se reescribe como invariante presente o se borra. Ejemplo:
"esto ya no es `Field`, así que nada corriente abajo puede confundirlo con una constante" →
"esto nunca es `Field`; nada corriente abajo puede confundirlo con una constante". Se aplica
parejo a `//` y a rustdoc público — si hay algo que un caller necesita saber, se dice como
hecho presente, no como contraste histórico. No tocar comentarios que ya son presentes y solo
documentan un WHY no obvio (la mayoría de los rustdoc de contratos/invariantes está bien así).
No genera falta ningún cambio de comportamiento: es edición de comentarios/prose únicamente.

**Alcance de "documentación interna" (decidido, refinado tras auditoría):** `internal/` no se
achica por volumen — es el mecanismo de gobernanza del proyecto (RFCs vivos, ADRs inmutables,
contratos normativos que `CLAUDE.md` ya protege). Una auditoría de sus 39 documentos (11 agentes
en paralelo, cada afirmación comparada contra el código actual) confirmó que ninguno es
redundante ni está muerto: cada RFC/ADR/contrato/spec sigue siendo la única fuente de esa
decisión. Lo que sí encontró: **22 de 39 quedaron desactualizados** — afirman algo que el código
ya no hace, o falta algo que el código ya hace — ver tickets 21–32. La corrección es edición
dirigida de las secciones puntuales que fallaron la verificación, nunca reescritura completa ni
recorte estructural del documento. Dos redundancias reales, ambas resueltas dentro de la
corrección misma (no como borrado de un documento entero): la nota de estado de ADR 0003
duplica lo que `contracts/wasm-abi.md` §5 ya documenta correctamente — se recorta en el ADR, el
contrato es la fuente; el `## 0` de `adapters/css.md` se superpone con la sección "M5 progress —
CSS adapter" de `ROADMAP.md` — `css.md` queda como fuente canónica, el ROADMAP se recorta a un
puntero. `docs/` (manual de usuario, mdBook) no fue auditado — sigue fuera de alcance. Única
baja real: `internal/plan-detection-gap-fixes.md`, un plan de sesión atado a una branch puntual
(`claude/core-api-ergonomics-architecture-983pom`) — se retira (ticket 02).

**Reescritura de `CLAUDE.md` (decidido):** cada regla existente y sus gates nombrados se
mantienen en sustancia, reescritos como instrucción presente ("nunca hagas X", "Y vive en Z"),
sin el incidente/audit/branch que la originó. Se agrega una sección nueva con la política de
comentarios de arriba.

**Cierre:** el último ticket de este mapa (`20-close-map.md`) está bloqueado por todos los
demás. Al resolverlo: confirmar que el destino está alcanzado y borrar `.wayfinder/` entero
en el mismo commit.

## Decisions so far

- [Decisión: consolidar internal/ a 10 documentos fijos + mecanismo anti-desactualización](tickets/33-consolidation-decision.md):
  10 documentos fijos (README, ROADMAP, ADRS, CONTRACTS, ADAPTERS, ARCHITECTURE, PLUGINS,
  GRAPH-CACHE-AND-ANALYSES, CLI-OUTPUT-AND-INTERFACE, PERFORMANCE-WORKSPACES-AND-RELEASE) más
  `detection-gaps.md` aparte; corregir contenido primero (tickets 21-32), fusionar después
  (tickets 34a-34h); regla nueva en CLAUDE.md (ticket 01) + gate de CI (ticket 35) para que no
  vuelva a desincronizarse.

## Not yet specified

- Texto final exacto de cada regla reescrita de `CLAUDE.md` y de la sección de política de
  comentarios — se decide al resolver el ticket 01, no antes.
- Si conviene además un gate de CI que impida que vuelva a acumularse narración histórica en
  comentarios nuevos (más allá de la convención escrita en `CLAUDE.md`) — no evaluado todavía;
  puede graduar en un ticket propio si alguna resolución lo deja lo bastante específico.
- Productizar la auditoría profunda (11 agentes comparando cada afirmación de un doc contra el
  código, la que corrió este mapa) como herramienta manual bajo demanda (`cargo xtask
  audit-docs` o similar) para correr antes de cada release — mencionado como opcional en la
  resolución del ticket 33, todavía no lo bastante concreto para ticketear.

## Out of scope

- `docs/` (mdBook de usuario): no auditado, no se toca.
- Construir funcionalidad nueva para que el código alcance lo que un doc desactualizado
  promete — la corrección va en el sentido contrario: el doc se ajusta a lo que el código hace
  hoy. Caso concreto: `adapters/js-ts.md` documenta soporte de `tsconfig` `baseUrl`/`paths` que
  no existe en `kndo-adapter-js` (ticket 29) — se corrige el doc, no se construye la feature.
- Restructurar el layout de `internal/` (fusionar carpetas, cambiar la convención
  RFC/ADR/contrato/ROADMAP) — la auditoría no encontró necesidad de eso; toda corrección es
  puntual, documento por documento.
