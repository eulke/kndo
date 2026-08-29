# Cerrar el mapa: verificar el destino y borrar el tracker

- Status: open
- Type: wayfinder:task (AFK)
- Assignee: unassigned
- Blocked by: 01-claude-md-rewrite.md, 02-internal-plan-sunset.md, 03-core-graph.md, 04-core-analysis.md, 05-core-engine-adapter.md, 06-core-plugin-cache-query.md, 07-core-remaining-top-level.md, 08-adapter-rust-src.md, 09-adapter-js.md, 10-adapter-toolkit.md, 11-adapter-java-kotlin.md, 12-adapter-swift-go.md, 13-adapter-css-html-json.md, 14-plugins-first-party.md, 15-plugin-api.md, 16-cli.md, 17-kndo-facade.md, 18-xtask.md, 19-examples-spikes.md, 21-rfc-arch-adapters-plugins.md, 22-rfc-cache-analyses-cli.md, 23-rfc-nav-perf-human.md, 24-rfc-ci-workspaces-release.md, 25-adrs-linking-cache.md, 26-adapter-specs-status-headers.md, 27-adapter-specs-self-contradictions.md, 28-adapter-spec-java-fixtures.md, 29-adapter-spec-js-ts-tsconfig.md, 31-roadmap-html-plugins-and-redundancy.md, 32-vision-html-and-plugin-findings.md, 34a-merge-adrs-and-contracts.md, 34b-merge-adapter-specs.md, 34c-merge-rfc-architecture.md, 34d-merge-rfc-plugins.md, 34e-merge-rfc-graph-cache-analyses.md, 34f-merge-rfc-cli-output-interface.md, 34g-merge-rfc-performance-workspaces-release.md, 34h-rebuild-readme.md, 35-doc-freshness-ci-gate.md

## Question

Con los tickets anteriores cerrados, ¿el destino del mapa está realmente alcanzado? Verificar:
`cargo clippy --workspace --all-targets --all-features` y `cargo test --workspace` en verde
(la limpieza de comentarios no debería haber tocado comportamiento, pero es la prueba de que
es cierto); `cargo test -p kndo doc_links` en verde (gate nombrado en CLAUDE.md, crítico
después de fusionar/borrar tantos `.md`); un grep rápido de patrones "used to be|no longer|
previously|historically|used to have" sobre `crates/`, `xtask/`, `examples/`, `spikes/perf/` y
`CLAUDE.md` no debería encontrar narración histórica sobreviviente; `internal/` debe tener
exactamente 10 documentos de diseño más `detection-gaps.md` (ver ticket 33) — confirmar que no
quedó ningún archivo viejo (`internal/rfcs/`, `internal/adrs/`, `internal/contracts/`,
`internal/adapters/` deberían no existir más); y el gate nuevo del ticket 35 debe existir y
pasar. Si todo eso da bien: borrar `.wayfinder/` (este mapa y todos sus tickets) del repo, en
el mismo commit que cierra este ticket.

## Resolution

(completar al cerrar)
