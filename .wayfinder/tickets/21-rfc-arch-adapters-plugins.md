# Corregir vigencia: RFC 0001, 0002, 0003 (arquitectura, adapters, plugin system)

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

Auditoría (11 agentes, comparando contra el código actual) encontró desvíos concretos en estos
tres RFCs. ¿Pueden corregirse las secciones puntuales sin reescribir el resto (que verificó
como vigente)?

- **RFC 0001** (`internal/rfcs/0001-architecture.md`): §5 describe el estado de performance
  congelado en el cierre de M2 ("today is a full recompute", "CI wiring deferred"), pero el
  patch incremental ya aterrizó (RFC 0013, `graph/patch.rs:412`) y el gate de regresión de
  benchmark ya existe (`cargo xtask bench --gate`). §2 lista los adapters de lenguaje y omite
  `kndo-adapter-html`, un crate shippeado y activado por defecto.
- **RFC 0002** (`0002-language-adapters.md`): el adapter HTML no aparece en ningún lado — ni en
  §3 (lenguajes non-source), ni en la tabla de §7, y falta el spec `internal/adapters/html.md`
  que §7 exige antes de cada milestone de implementación (no crear el spec en este ticket si
  no hay tiempo — al menos señalar el vacío correctamente en el propio RFC).
- **RFC 0003** (`0003-plugin-system.md`): §2 lista 6 hooks del trait `Plugin` pero el trait real
  (`plugin.rs`) ya tiene `rules()` y `contribute_findings()` (RFC 0018, aceptado y aterrizado) —
  §6 todavía dice que "custom analyses" son "post-1.0", contradicho por RFC 0018. §3 lista
  plugins "landed so far" muy atrás de los 10 crates `kndo-plugin-*` reales en `crates/kndo/Cargo.toml`.

## Resolution

(completar al cerrar)
