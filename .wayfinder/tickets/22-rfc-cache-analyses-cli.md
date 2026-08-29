# Corregir vigencia: RFC 0004, 0005, 0006 (cache, análisis, CLI/output)

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
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

RFC 0004 §2/§3 reescritas para reflejar `graphs/<key>.bin` + `graphs/latest` reales, sin
`findings.bin`, con la ausencia del config hash explicada. RFC 0005 §13 corrige la
autocontradicción (dos filas se mantienen explícitamente abiertas); §3 refleja el estado real
de `test_only`'s severidad. RFC 0006 §7 marca `[health.weights]` como no implementado y corrige
la afirmación (ya errónea) sobre el config hash en la cache key. `cargo test -p kndo doc_links`
en verde.

