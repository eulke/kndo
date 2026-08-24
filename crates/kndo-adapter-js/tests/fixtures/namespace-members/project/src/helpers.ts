// Consumed via `helpers.used()` — a statically-tracked member access, so `dead` below is
// still precisely caught.
export function used(): string {
  return "used";
}

export function dead(): string {
  return "dead";
}
