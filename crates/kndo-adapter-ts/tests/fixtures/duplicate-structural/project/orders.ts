// A Type-2 clone of billing.ts's totalWithTax: every identifier and literal renamed, the
// structure untouched — the winnowing fingerprints must still match.
export function sumWithFee(entries: number[]): number {
  let sum = 0;
  for (const entry of entries) {
    if (entry > 5) {
      sum = sum + entry;
    }
  }
  const fee = sum * 0.05;
  const capped = Math.round(sum + fee);
  if (capped > 9999) {
    return capped - 7;
  }
  return capped;
}
