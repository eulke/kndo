# Corregir vigencia: adapters/js-ts.md (tsconfig documentado como shippeado, no lo está)

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

§3 afirma que el adapter sigue `tsconfig` `baseUrl` + `paths` (con `extends` chains), §5 lo da
por hecho ("path aliases within one graph are [followed]"), y §6 lista una fixture de
conformance de ese escenario. Nada de esto existe: `kndo-adapter-js` no declara `tsconfig.json`
en sus `manifest_globs`, no hay ninguna implementación de `baseUrl`/`paths`/`compilerOptions`
en `src/`, y ninguna de las 22 fixtures reales tiene un `tsconfig.json`. Es exactamente el caso
que `CLAUDE.md` nombra en "Contract first": el documento describe código que no existe.

Alcance de este ticket: corregir el documento para que diga lo que el adapter hace hoy (no
sigue `tsconfig` paths) — **no** implementar la feature; construirla es un esfuerzo aparte,
fuera de este mapa (ver "Out of scope" en `.wayfinder/map.md`). El resto de §3/§4 (workspace
`WorkspaceMember`, el campo `browser`, `script_invoked_names`, dynamic-import wildcards) ya
verificó vigente — no tocar.

## Resolution

(completar al cerrar)
