import chalk from "chalk";
import leftPad from "left-pad";
// Declared as "workspace:*" — resolves to packages/b's internal files, a clean cross-package
// edge (RFC 0011 §4). Keeps @demo/b's `util` alive through the workspace binding.
import { util } from "@demo/b";

export function runA(): string {
  return util();
}

console.log(chalk, leftPad, runA());
