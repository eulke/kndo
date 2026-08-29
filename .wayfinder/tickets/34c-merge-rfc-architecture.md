# Fusionar: RFC 0001 + 0002 + 0016 → ARCHITECTURE.md

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4)
- Blocked by: 21-rfc-arch-adapters-plugins.md

## Question

Con RFC 0001 y 0002 ya corregidos (ticket 21) y RFC 0016 ya verificado vigente: ¿puede crearse
`internal/ARCHITECTURE.md` con las tres como secciones (`## RFC 0001: Architecture`,
`## RFC 0002: Language adapters`, `## RFC 0016: Uniform component model`), cada una conservando
su numeración para que citas existentes tipo "RFC 0001 §5" sigan siendo válidas? El §7 de RFC
0002 sobre specs por-lenguaje ahora apunta a `internal/ADAPTERS.md` (ticket 34b) en vez de a
`internal/adapters/<lang>.md` — actualizar esa referencia si 34b ya cerró, o dejar una nota si
no. Borrar los 3 archivos originales de `internal/rfcs/`. Correr `cargo test -p kndo doc_links`
al final.

## Resolution

`internal/ARCHITECTURE.md` creado con las 3 RFCs como secciones numeradas. Corregida además
una frase que quedó desactualizada tras el merge (RFC 0002 §7 decía que el spec de HTML "no
existe aún" — ya existe, en `ADAPTERS.md`, creado por el ticket 34b en paralelo).

