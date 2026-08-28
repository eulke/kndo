# Limpiar comentarios: crates/kndo-plugin-{coverage,express,info-plist,libsass,nextjs,rkyv,serde,thymeleaf,uikit,wasmtime}/

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: none

## Question

Los diez plugins de primera parte más chicos (`kndo-plugin-coverage`, `-express`,
`-info-plist`, `-libsass`, `-nextjs`, `-rkyv`, `-serde`, `-thymeleaf`, `-uikit`, `-wasmtime`;
juntos ~710 líneas de comentario) — ¿todos sus comentarios describen únicamente el
comportamiento/invariante actual? Aplicar la política de `.wayfinder/map.md` (Notes) a cada
crate por separado. No tocar `Plugin::mutates_graph()` ni ningún otro comportamiento —
solo comentarios. Solo edición de comentarios.

## Resolution

(completar al cerrar)
