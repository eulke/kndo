// Consumed via `registry[name]` — a computed member access, so every export here is plausibly
// used (a wildcard over that namespace's exports) and none may be flagged.
export function alpha(): string {
  return "alpha";
}

export function beta(): string {
  return "beta";
}
