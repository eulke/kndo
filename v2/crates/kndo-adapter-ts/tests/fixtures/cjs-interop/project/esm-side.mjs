// ESM file importing from a CJS file — the other direction of the interop pair.
import { util } from "./cjs-side.js";

export function helper() {
  return util();
}

// Exported but this file is not a manifest root, and nothing imports this name — dead.
export function deadHelper() {}
