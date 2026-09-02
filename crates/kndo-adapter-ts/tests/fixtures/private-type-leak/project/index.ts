type Secret = { id: number };

export function make(s: Secret): Secret {
  const copy: Secret = { id: s.id };
  return copy;
}
