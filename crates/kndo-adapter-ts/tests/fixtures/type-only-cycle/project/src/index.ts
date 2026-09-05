// The entry. `import type` is erased by every emitter: shape.ts importing this
// file's type back closes no initialization loop, so neither file is cyclic.
import type { Shape } from "./shape.js";
import "./a.js";

export function area(s: Shape): number {
  return s.w * s.h;
}
