// Deep import: bypasses @org/ui's declared exports surface — the boundary the sibling
// explicitly drew. The dependency itself is declared, so this is deep-import's finding
// alone, not undeclared's.
import { secretFn } from '@org/ui/secret';

export function run(): number {
  return secretFn();
}
