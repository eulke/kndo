// a.ts and b.ts import each other — a real, mutually-recursive module cycle: everything is
// alive and used, the only defect is the loop itself.
import { pong } from './b';

export function ping(n: number): number {
  return n <= 0 ? 0 : pong(n - 1);
}
