# Retirar internal/plan-detection-gap-fixes.md y limpiar el inglés de detection-gaps.md

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
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

(completar al cerrar)
