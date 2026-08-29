# Fusionar: RFC 0003 + 0015 + 0017 + 0018 → PLUGINS.md

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: 21-rfc-arch-adapters-plugins.md

## Question

Con RFC 0003 ya corregido (ticket 21) y RFC 0015/0017/0018 ya verificados vigentes: ¿puede
crearse `internal/PLUGINS.md` con las cuatro como secciones (`## RFC 0003: Plugin system`,
`## RFC 0015: Plugin identity and dependencies`, `## RFC 0017: Plugin platform, second pass`,
`## RFC 0018: Plugin-contributed findings`), cada una con su numeración conservada? Estas
cuatro ya se citan entre sí bastante (0017 dice "since landed by..." sobre 0003/0015, 0018
depende de la mecánica de 0003) — al quedar en el mismo archivo, esas referencias cruzadas
pueden simplificarse a "§X de este documento" en vez de repetir "RFC 0003" si el resolutor lo
ve claramente mejor, pero no es obligatorio. Borrar los 4 archivos originales. Correr
`cargo test -p kndo doc_links` al final.

## Resolution

(completar al cerrar)
