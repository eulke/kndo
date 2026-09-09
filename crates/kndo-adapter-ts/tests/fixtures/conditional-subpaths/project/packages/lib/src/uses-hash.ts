import type { Payload } from '#shapes/payload';

export function label(p: Payload) {
  return p.kind;
}
