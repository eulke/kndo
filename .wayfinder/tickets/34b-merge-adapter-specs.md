# Fusionar: 8 specs de adapter + HTML nuevo → ADAPTERS.md

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: 26-adapter-specs-status-headers.md, 27-adapter-specs-self-contradictions.md, 28-adapter-spec-java-fixtures.md, 29-adapter-spec-js-ts-tsconfig.md

## Question

Con css.md, json.md, go.md, kotlin.md, java.md y js-ts.md ya corregidos (tickets 26-29) y
rust.md/swift.md ya verificados vigentes: ¿puede crearse `internal/ADAPTERS.md` con los 9
lenguajes como secciones (`## CSS`, `## Go`, `## HTML`, `## Java`, `## JSON`, `## Kotlin`,
`## Rust`, `## Swift`, `## JS/TS`)? La sección HTML es nueva — no existe spec hoy — escribirla
mirando `crates/kndo-adapter-html/src/` (extraction, resolution, fixtures) con el mismo formato
que las demás secciones. Borrar los 8 archivos originales y la carpeta `internal/adapters/`
vacía. Correr `cargo test -p kndo doc_links` al final.

## Resolution

(completar al cerrar)
