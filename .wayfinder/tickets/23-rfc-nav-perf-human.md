# Corregir vigencia: RFC 0007, 0008, 0009 (navegación, performance, interfaz humana)

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

- **RFC 0007** (`internal/rfcs/0007-graph-navigation.md`): §8 presenta como abiertas dos
  preguntas ya resueltas en código (el cap de `trace --all` por `max_paths` + longitud +
  presupuesto de expansión; verbos planos vs. namespaced, ya resuelto según el propio log de
  `internal/README.md`) — marcarlas resueltas. El verbo `Explain` existe en el `Verb` enum
  real y no aparece en la sección de verbos del RFC — agregarlo.
- **RFC 0008** (`0008-performance-and-parallelism.md`): §6 ("adaptive execution", threshold de
  16 archivos) no está implementado — el pool se crea siempre y la extracción corre
  `par_iter()` incondicional. §2 dice que reachability y duplicate corren en paralelo intra-
  análisis — no es así, solo hay paralelismo entre análisis. §7 dice que el benchmark es
  "CI-blocking" — `CONTRIBUTING.md` y `CLAUDE.md` dicen explícitamente lo contrario (no es gate
  de CI, a propósito). Corregir las tres afirmaciones.
- **RFC 0009** (`0009-human-interface.md`): §3 lista 4 grupos de color/glyph pero el código
  tiene un quinto (`Group::Convention`, color CYAN) sin documentar. El ejemplo de "clean run"
  de Principio 2 no coincide con el formato real de `render.rs`. El ejemplo de error de §6
  ("cache locked by pid...") describe una `EngineError` estructurada que no existe — el error
  real es un `eprintln!` plano. Corregir los tres.

## Resolution

(completar al cerrar)
