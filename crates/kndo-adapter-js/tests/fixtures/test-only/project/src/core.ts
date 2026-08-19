// `core` is production-reachable (called from index.ts, the root). `helper` is declared here
// too but only ever called from the test below — a symbol "enshrined by tests" (RFC 0005 §3).
export function core(): string {
  return "core";
}

export function helper(): string {
  return "helper";
}
