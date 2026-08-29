# Corregir vigencia: adapters/go.md + adapters/kotlin.md (autocontradicciones)

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

- **go.md**: §2 dice correctamente que hoy el código sí diferencia `RefKind::Extend` vs
  `TypeUse` para embeddings — pero §5 y §7 (pregunta abierta #2) todavía dicen que ningún
  adapter diferencia `RefKind` y que sigue "open", sin el tachado "~~fixed~~" que sí tienen los
  otros puntos de la misma lista. Además §1 documenta 3 rungs de visibilidad y §7 punto 3 los
  recapitula como solo 2 — se agregó un rung intermedio sin actualizar el recap. Tampoco
  menciona que el adapter ya shippea `private-type-leak` (tiene fixture propia). Corregir las
  cuatro cosas.
- **kotlin.md**: §6 dice "Four fixtures" y lista cuatro, pero hay una quinta
  (`ctor-arg-and-default-value`) que el propio §2 ya cita por nombre. Corregir el conteo y la
  lista.

## Resolution

go.md: §5 y §7 ahora dicen que `RefKind::Extend` está implementado (tachado "~~fixed~~" como
los demás puntos resueltos de la lista); §7 punto 3 corregido a 3 rungs; agregada mención de
`private-type-leak`. kotlin.md §6 corregido a "Five fixtures" con la quinta listada. `cargo
test -p kndo doc_links` en verde.

