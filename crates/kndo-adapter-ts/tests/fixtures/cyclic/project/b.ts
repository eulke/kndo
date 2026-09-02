import { ping } from './a';

export function pong(n: number): number {
  return ping(n - 1);
}
