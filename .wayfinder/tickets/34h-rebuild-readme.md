# Reconstruir internal/README.md con la estructura final de 10 documentos

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4)
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

`internal/README.md` reescrito: intro de producto fusionada desde `product/vision.md` (borrado),
tabla de documentos apuntando a los 10 archivos reales, `detection-gaps.md` señalado aparte,
Conventions/Open questions actualizadas. Corregí un link que el agente dejó apuntando a
`.wayfinder/tickets/33-consolidation-decision.md` — ese tracker se borra al cerrar el mapa, así
que un doc permanente no puede depender de él. `cargo test -p kndo --test doc_links` en verde.

