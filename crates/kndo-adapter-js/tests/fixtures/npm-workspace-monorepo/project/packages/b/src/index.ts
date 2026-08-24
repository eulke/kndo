import chalk from "chalk";
// Phantom internal dependency: resolves into sibling @demo/a but @demo/b's own
// manifest never declares it — breaks publishability and build graphs.
import { runA } from "@demo/a";

export function util(): string {
  return "util";
}

console.log(chalk, runA);
