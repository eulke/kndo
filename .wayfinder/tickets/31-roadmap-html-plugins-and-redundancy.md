# Corregir vigencia: ROADMAP.md (falta HTML + 6 plugins; recortar narrativa redundante)

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

`internal/ROADMAP.md` no menciona el noveno adapter de lenguaje (`kndo-adapter-html`, activado
por defecto) ni seis plugins de ecosistema ya shippeados (`kndo-plugin-{libsass,uikit,
thymeleaf,info-plist,rkyv,wasmtime}`, cada uno documentado como "Shipped, built-in" en
`docs/src/plugins/*.md`). Peor: la sección "Post-1.0 parking lot" todavía dice que un plugin de
ecosistema real (React/Next.js) sigue "parked", lo cual contradice los seis plugins reales que
ya aterrizaron. Agregar ambas cosas al lugar del ROADMAP que les corresponda por milestone, y
corregir o quitar la afirmación del parking lot que ya no es cierta.

De paso, recortar la narrativa de diseño redundante: la sección "M5 progress — CSS adapter" se
superpone con `adapters/css.md` §0 (ticket 26 deja css.md como fuente), y "M5 progress — Java
adapter" se superpone con `adapters/java.md` §0/§4 (ticket 28). El ROADMAP es un log de
milestones y criterios de salida, no el lugar para repetir el razonamiento de diseño completo —
recortar ambas secciones a un resumen de una o dos líneas + link al spec correspondiente.

## Resolution

Agregadas dos secciones nuevas (HTML adapter M6, los 6 plugins de ecosistema M6) y notas "Since
landed" donde el ROADMAP todavía decía "still parked" contradicho por esos 6 plugins reales.
Recortadas las secciones "M5 progress — CSS adapter" y "M5 progress — Java adapter" a un
resumen + puntero a los specs correspondientes (156 líneas netas, mayoría recorte). Verificado
que no queda ninguna mención de "still parked" para React/Next.js. `cargo test -p kndo
doc_links` en verde.

