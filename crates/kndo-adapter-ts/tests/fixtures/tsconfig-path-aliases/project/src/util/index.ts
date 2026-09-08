// Reached only through the exact alias `~utils`, which `tsconfig.json` maps to
// this file: no relative import in the project names it.
export function format(value: number): string {
  return `#${value}`
}
