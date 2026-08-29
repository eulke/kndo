# Corregir vigencia: RFC 0004, 0005, 0006 (cache, análisis, CLI/output)

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

- **RFC 0004** (`internal/rfcs/0004-graph-and-cache.md`): §2/§3 describen un `graph.bin` y un
  `findings.bin` únicos y una cache key que incluye un hash de config de kndo — ninguno de los
  tres existe. El código real (`cache.rs`) usa `graphs/<key>.bin` (varios coexisten) +
  `graphs/latest`, sin `findings.bin`, y su propio doc comment dice explícitamente que el hash
  de config está ausente a propósito. Corregir §2/§3 para que coincidan; el resto del documento
  (versiones de schema, el guard de patch, el lock, el cap de 256MB) ya está vigente.
- **RFC 0005** (`0005-analyses.md`): §13 dice "no candidates remain open" y a continuación
  siguen dos filas más (`hollow-test` marcada abierta, `speculative-abstraction` sin decisión) —
  corregir la frase para que no se contradiga con su propia tabla. §3 dice "revisit at M6" sin
  registrar si esa revisión pasó (M6 ya aterrizó según ROADMAP) — anotar el estado real.
- **RFC 0006** (`0006-cli-and-output.md`): el ejemplo de config en §7 incluye `[health.weights]`,
  una tabla que el engine no lee (`config::LIVE_TABLES` no la tiene, `kndo init` no la escribe)
  — exactamente el caso "config the engine does not read is not shipped" que `CLAUDE.md` ya
  nombra para el caso `[project]`. Quitar el ejemplo o marcarlo explícitamente como no
  implementado.

## Resolution

(completar al cerrar)
