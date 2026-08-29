# Fusionar: RFC 0004 + 0013 + 0005 + 0012 → GRAPH-CACHE-AND-ANALYSES.md

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4)
- Blocked by: 22-rfc-cache-analyses-cli.md

## Question

Con RFC 0004 y 0005 ya corregidos (ticket 22) y RFC 0013/0012 ya verificados vigentes: ¿puede
crearse `internal/GRAPH-CACHE-AND-ANALYSES.md` con las cuatro como secciones (`## RFC 0004:
Graph and cache`, `## RFC 0013: Incremental graph patch`, `## RFC 0005: Analyses`, `## RFC 0012:
Reference semantics and visibility`), numeración conservada? RFC 0013 §7 ya se refiere a RFC
0004 como su base — al estar en el mismo archivo, mantener esa referencia (sigue siendo
correcta, solo que ahora apunta a otra sección del mismo doc). Borrar los 4 archivos originales.
Correr `cargo test -p kndo doc_links` al final.

## Resolution

`internal/GRAPH-CACHE-AND-ANALYSES.md` creado con las 4 RFCs como secciones numeradas.

