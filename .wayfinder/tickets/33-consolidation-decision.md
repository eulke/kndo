# Decisión: consolidar internal/ a 10 documentos fijos + mecanismo anti-desactualización

- Status: closed
- Type: wayfinder:grilling
- Assignee: claude (resuelto en conversación con el usuario)
- Blocked by: none

## Question

39 documentos es demasiado volumen para que ningún agente note cuándo uno se desincroniza del
código (por eso la auditoría del ticket previo encontró 22 desactualizados). ¿Puede
consolidarse `internal/` a una estructura fija de ~10 documentos, y qué mecanismo evita que
vuelva a desincronizarse?

## Resolution

Estructura acordada (10 documentos fijos dentro de `internal/`, cada RFC pasa de archivo propio
a sección con encabezado que conserva su número — "## RFC 0012: ..." — para que las citas
"RFC 0012 §8" ya existentes en código/docs sigan siendo válidas conceptualmente):

1. `README.md` — índice + convenciones + la visión de producto como intro (fusiona `product/vision.md`)
2. `ROADMAP.md` — log de milestones, se mantiene separado (cadencia distinta al resto)
3. `ADRS.md` — las 7 decisiones, una sección cada una, cada sección sigue siendo inmutable
4. `CONTRACTS.md` — los 3 contratos (traits, output-schema, wasm-abi) como 3 secciones
5. `ADAPTERS.md` — los 9 specs de lenguaje (los 8 existentes + sección nueva de HTML) como secciones
6. `ARCHITECTURE.md` — RFC 0001 + 0002 + 0016
7. `PLUGINS.md` — RFC 0003 + 0015 + 0017 + 0018
8. `GRAPH-CACHE-AND-ANALYSES.md` — RFC 0004 + 0013 + 0005 + 0012
9. `CLI-OUTPUT-AND-INTERFACE.md` — RFC 0006 + 0007 + 0009 + 0010
10. `PERFORMANCE-WORKSPACES-AND-RELEASE.md` — RFC 0008 + 0011 + 0014 + el spike de perf como apéndice

`internal/detection-gaps.md` queda aparte, no cuenta contra el límite — es una referencia
operativa citada por `kndo.toml`/pragmas `kndo:allow`, no un documento de diseño narrativo.

Orden: primero se corrige contenido en los archivos actuales (tickets 21-32, ya escritos contra
esos paths), después se fusiona (tickets 34a-34h) — así la fusión mueve contenido ya verificado
en vez de repetir la verificación.

Mecanismo anti-desactualización (dos obligatorios, uno opcional):
- (a) Regla nueva en `CLAUDE.md` (generaliza "Contract first"): todo PR que cambia el
  comportamiento que describe uno de estos 10 documentos, actualiza ese documento en el mismo
  PR. Se agrega al ticket 01 (que ya reescribe CLAUDE.md).
- (b) Gate de CI liviano: con 10 documentos fijos es viable mantener una tabla `ruta de crate →
  documento` y fallar el PR si toca una ruta sin tocar su documento — ticket 35, bloqueado por
  la fusión (necesita que la estructura final exista).
- (c) Opcional, no ticketeado: productizar la auditoría profunda (11 agentes comparando cada
  afirmación contra el código, la que corrió este mapa) como herramienta manual bajo demanda
  (`cargo xtask audit-docs` o similar) para antes de cada release — no como gate de cada PR por
  costo/latencia. Queda en "Not yet specified" del mapa por si se vuelve concreto más adelante.

Ticket 30 (fix puntual de la tabla de `internal/README.md`) queda superseded: el ticket 34h
reconstruye `README.md` entero con la estructura final, así que parchear la tabla vieja es
trabajo perdido. Se borró.
