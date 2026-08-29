# Fusionar: RFC 0008 + 0011 + 0014 + spike de perf → PERFORMANCE-WORKSPACES-AND-RELEASE.md

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4)
- Blocked by: 23-rfc-nav-perf-human.md, 24-rfc-ci-workspaces-release.md

## Question

Con RFC 0008 corregido (ticket 23) y RFC 0011/0014 corregidos (ticket 24): ¿puede crearse
`internal/PERFORMANCE-WORKSPACES-AND-RELEASE.md` con tres secciones de RFC (`## RFC 0008:
Performance and parallelism`, `## RFC 0011: Workspaces and monorepos`, `## RFC 0014:
Distribution and release`, numeración conservada) más un apéndice final que incorpora
`internal/spikes/0001-performance.md` tal cual (el spike no fue auditado, no tocar su
contenido, solo moverlo)? Borrar los 3 archivos de RFC originales y `internal/spikes/0001-
performance.md` (la carpeta `spikes/` puede quedar vacía o borrarse si no tiene más contenido).
Correr `cargo test -p kndo doc_links` al final.

## Resolution

`internal/PERFORMANCE-WORKSPACES-AND-RELEASE.md` creado con las 3 RFCs más el spike de perf
como apéndice. El link de `ROADMAP.md` al spike (fuera del alcance de este ticket) se corrigió
aparte, directamente.

