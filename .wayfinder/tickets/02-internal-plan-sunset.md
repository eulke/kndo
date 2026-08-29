# Retirar internal/plan-detection-gap-fixes.md y limpiar el inglés de detection-gaps.md

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4)
- Blocked by: none

## Question

`internal/plan-detection-gap-fixes.md` dice de sí mismo "this file exists so the work survives
a session boundary" y está atado a la branch `claude/core-api-ergonomics-architecture-983pom`;
su propio estado dice "W1–W5 and W7 landed; W6 is the only item left". No lo referencia ningún
otro doc ni código (verificado por grep). ¿Puede migrarse el punto W6 pendiente a
`internal/detection-gaps.md` (como una entrada más de gap conocido, con su root cause y
dirección de fix, igual que las demás entradas de ese archivo) y borrarse el archivo de plan?

Además, `internal/detection-gaps.md` tiene frases en español mezcladas con el inglés (p.ej.
"RESUELTO", "Ya no.", "Sigue siendo un límite...") pese a que la convención del proyecto
(`internal/README.md` → Conventions) pide que los documentos estén en inglés. Traducir esas
frases sin cambiar el contenido técnico.

No tocar el resto de `internal/` (fuera de alcance del mapa).

## Resolution

`internal/plan-detection-gap-fixes.md` borrado. Antes de borrarlo verifiqué su "## 2. What
remains — W6": de los 6 candidatos de esa tabla, 5 ya estaban resueltos y no lo decía nadie:

- `kndo-plugin-info-plist` y `kndo-plugin-thymeleaf` — shippeados, wireados en
  `crates/kndo/Cargo.toml` como features default. Ninguno tenía su resolución documentada en
  `detection-gaps.md`.
- `kndo:vite`, `kndo:rollup` y `kndo:guava-testlib` — ya completamente documentados en las §19 y
  §20/§20-bis existentes de `detection-gaps.md` (con más rigor que la tabla del plan: números
  medidos, por qué NO se construyeron como plugin, y qué se construyó en su lugar —
  `kndo-adapter-html` para el caso de vite). No hacía falta migrar nada ahí, ya estaba hecho.
- `kndo-adapter-markdown` (el sexto candidato) — decidido explícitamente que NO se construye,
  razón documentada en el propio `crates/kndo/tests/doc_links.rs` (verificado vigente por la
  auditoría de RFCs). No necesita entrada en `detection-gaps.md`, ya vive donde corresponde.

Lo único que faltaba: la §14 de `detection-gaps.md` seguía marcada "(GAP)" — abierta — pese a
que sus tres casos (info-plist, thymeleaf, vite) ya estaban resueltos. La reescribí como
"(RESOLVED)", documentando cómo se cerró cada uno (verificado contra el código real de
`kndo-plugin-info-plist` y `kndo-plugin-thymeleaf`, no solo copiado del plan) y apuntando a
§20/§20-bis para vite en vez de duplicar esa explicación ahí también.

También verifiqué que la mecánica "servido ≠ usado" que el plan describía como hallazgo de esta
sesión (§0 "W6 — el primer tramo") ya vive donde debe: como comentario presente en
`crates/kndo-core/src/analysis/unused.rs` (`served_only`, con sus tests) — no se perdió nada al
borrar el plan.

Traduje el español mezclado en `detection-gaps.md`: el bloque grande de §3 (RFC 0012
§3-ter/§3-quater), el párrafo de §4, y seis frases-título en negrita sueltas en §1/§7/§8
("Ya no." → "Not anymore.", "Medición"/"Medido..." → "Measurement"/"Measured...", "Lo que queda
fuera" → "What stays out", etc.), sin cambiar ningún número, identificador de código ni
referencia de RFC.
