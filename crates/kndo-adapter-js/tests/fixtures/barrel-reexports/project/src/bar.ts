// Reached only through index.ts's `export * from './bar'` — the blanket-star re-export form.
// Named re-exports (see foo.ts) resolve symbol-by-symbol and count as production roots
// (foo.ts's Foo does not appear as unused below); a bare `export * from` has no explicit names
// to resolve through, so it only fixes *this file's* reachability, not Bar's own — a known,
// explicitly scoped limitation (see graph.rs's `handle_reexport_statement` doc).
export function Bar(): void {}
