# Limpiar comentarios: crates/kndo-plugin-{coverage,express,info-plist,libsass,nextjs,rkyv,serde,thymeleaf,uikit,wasmtime}/

- Status: closed
- Type: wayfinder:task (AFK)
- Assignee: claude (sesión claude/wayfinder-cleanup-docs-9c4ic4, resuelto en paralelo vía Workflow)
- Blocked by: none

## Question

Los diez plugins de primera parte más chicos (`kndo-plugin-coverage`, `-express`,
`-info-plist`, `-libsass`, `-nextjs`, `-rkyv`, `-serde`, `-thymeleaf`, `-uikit`, `-wasmtime`;
juntos ~710 líneas de comentario) — ¿todos sus comentarios describen únicamente el
comportamiento/invariante actual? Aplicar la política de `.wayfinder/map.md` (Notes) a cada
crate por separado. No tocar `Plugin::mutates_graph()` ni ningún otro comportamiento —
solo comentarios. Solo edición de comentarios.

## Resolution

Solo 4 de los 10 crates tenían comentarios que corregir: `kndo-plugin-coverage`,
`kndo-plugin-info-plist`, `kndo-plugin-libsass`, `kndo-plugin-uikit`. `Plugin::mutates_graph()`
y el resto del comportamiento no se tocaron. `cargo check --workspace` en verde.

