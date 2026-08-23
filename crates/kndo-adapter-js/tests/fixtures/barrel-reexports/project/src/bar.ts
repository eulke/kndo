// Reached only through index.ts's `export * from './bar'` — the blanket-star re-export form.
// Via the library-surface fixpoint (assembly phase 2.7), a published
// package's star re-export extends the surface into this
// file: Bar IS public API and is deliberately NOT expected as unused below.
export function Bar(): void {}
