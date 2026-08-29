# Corregir vigencia: ADR 0003 (adapter linking) + ADR 0004 (cache format)

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

- **ADR 0003** (`internal/adrs/0003-adapter-linking-strategy.md`): su sección "Implementation
  status (M5, 2026-08-21)" afirma que el bridge WASM de plugins "is still native-only" — es
  falso desde antes de que ese texto se comiteara (`plugin_host.rs` y `wit/plugin.wit`
  implementan el ABI completo, con tests de compliance). La información correcta ya vive en
  `internal/contracts/wasm-abi.md` §5. Recortar el párrafo de estado en el ADR a un puntero a
  ese contrato en vez de repetir (y errar) el detalle — un ADR registra la decisión, no el
  estado de implementación en curso.
- **ADR 0004** (`0004-cache-format.md`): repite la misma afirmación falsa que RFC 0004 sobre un
  `findings.bin` persistido — no existe, el diff se recalcula en vivo en cada invocación.
  Corregir la viñeta de "Serialization". Coordinar con el ticket 22 (RFC 0004) para no dejar la
  afirmación corregida en un lado y no en el otro.

## Resolution

ADR 0003 recorta su sección de "Implementation status" a un puntero a `contracts/wasm-abi.md`
§5 en vez de repetir (y errar) el detalle de qué está implementado. ADR 0004 corrige la misma
afirmación falsa sobre `findings.bin` que RFC 0004 (coordinado con el ticket 22 para no dejar
la corrección en un lado y no en el otro). `cargo test -p kndo doc_links` en verde.

