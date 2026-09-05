// A type-only import back to the entry: erased, never a hazard.
import type { area } from "./index.js";

export interface Shape {
  w: number;
  h: number;
  compute?: typeof area;
}
