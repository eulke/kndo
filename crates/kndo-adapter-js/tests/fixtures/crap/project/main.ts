import { classifyInput } from './risky';
import { scoreLabel } from './safe';

export function run(): string {
  return classifyInput(7, 'strict') + scoreLabel(9);
}
