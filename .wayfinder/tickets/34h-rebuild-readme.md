# Reconstruir internal/README.md con la estructura final de 10 documentos

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: 32-vision-html-and-plugin-findings.md, 34a-merge-adrs-and-contracts.md, 34b-merge-adapter-specs.md, 34c-merge-rfc-architecture.md, 34d-merge-rfc-plugins.md, 34e-merge-rfc-graph-cache-analyses.md, 34f-merge-rfc-cli-output-interface.md, 34g-merge-rfc-performance-workspaces-release.md

## Question

Con los 9 documentos restantes ya fusionados/corregidos y `product/vision.md` ya corregido
(ticket 32): ¿puede reescribirse `internal/README.md` desde cero para reflejar la estructura
final? Debe incluir: la intro de producto (fusiona el contenido de `product/vision.md`, que se
borra), una tabla de documentos actualizada listando exactamente los 10 archivos fijos
(`README.md`, `ROADMAP.md`, `ADRS.md`, `CONTRACTS.md`, `ADAPTERS.md`, `ARCHITECTURE.md`,
`PLUGINS.md`, `GRAPH-CACHE-AND-ANALYSES.md`, `CLI-OUTPUT-AND-INTERFACE.md`,
`PERFORMANCE-WORKSPACES-AND-RELEASE.md`) más `detection-gaps.md` señalado aparte como
referencia operativa (no cuenta como documento de diseño), y las secciones de Conventions y
Open questions ya verificadas vigentes (pueden pasar casi sin cambios). Correr
`cargo test -p kndo doc_links` al final — este es el ticket con más links internos, prestar
atención particular acá.

## Resolution

(completar al cerrar)
