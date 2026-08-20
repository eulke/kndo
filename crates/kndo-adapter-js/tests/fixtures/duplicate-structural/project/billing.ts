export function totalWithTax(items: number[]): number {
  let total = 0;
  for (const item of items) {
    if (item > 0) {
      total = total + item;
    }
  }
  const tax = total * 0.21;
  const rounded = Math.round(total + tax);
  if (rounded > 1000) {
    return rounded - 1;
  }
  return rounded;
}
