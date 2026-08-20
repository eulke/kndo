import { totalWithTax } from './billing';
import { sumWithFee } from './orders';

export function run(): number {
  return totalWithTax([1, 2]) + sumWithFee([3, 4]);
}
