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
en presente, más una sección nueva que fija esta misma política de comentarios. `internal/` y
`docs/` quedan intactos tal como están — son el registro de decisiones del proyecto, no
contaminación — salvo `internal/plan-detection-gap-fixes.md`, que se retira. Al cerrar el último
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

**Alcance de "documentación interna" (decidido):** `internal/` (RFCs, ADRs, contratos,
ROADMAP, `adapters/*.md`, `product/vision.md`, `internal/spikes/0001-performance.md`) y `docs/`
(manual de usuario, mdBook) quedan **intactos** — son el mecanismo de gobernanza del proyecto
(RFCs vivos, ADRs inmutables, contratos normativos que `CLAUDE.md` ya protege), no contaminación.
Única excepción: `internal/plan-detection-gap-fixes.md` es un plan de sesión atado a una branch
puntual (`claude/core-api-ergonomics-architecture-983pom`) — se retira (ticket 02).

**Reescritura de `CLAUDE.md` (decidido):** cada regla existente y sus gates nombrados se
mantienen en sustancia, reescritos como instrucción presente ("nunca hagas X", "Y vive en Z"),
sin el incidente/audit/branch que la originó. Se agrega una sección nueva con la política de
comentarios de arriba.

**Cierre:** el último ticket de este mapa (`20-close-map.md`) está bloqueado por todos los
demás. Al resolverlo: confirmar que el destino está alcanzado y borrar `.wayfinder/` entero
en el mismo commit.

## Decisions so far

(vacío — todavía no se cerró ningún ticket)

## Not yet specified

- Texto final exacto de cada regla reescrita de `CLAUDE.md` y de la sección de política de
  comentarios — se decide al resolver el ticket 01, no antes.
- Si conviene además un gate de CI que impida que vuelva a acumularse narración histórica en
  comentarios nuevos (más allá de la convención escrita en `CLAUDE.md`) — no evaluado todavía;
  puede graduar en un ticket propio si alguna resolución lo deja lo bastante específico.

## Out of scope

- `docs/` (mdBook de usuario) y todo `internal/` salvo `plan-detection-gap-fixes.md`: RFCs,
  ADRs, contratos, ROADMAP, `adapters/*.md`, `product/vision.md`, `spikes/0001-performance.md`
  — son el registro de decisiones del proyecto, confirmado explícitamente al fijar el destino.
