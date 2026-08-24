export function classifyInput(value: number, mode: string): string {
  if (value < 0) {
    return 'negative';
  }
  if (value === 0 && mode === 'strict') {
    return 'zero';
  }
  if (value > 100 || mode === 'loose') {
    return 'large';
  }
  if (mode === 'strict') {
    return 'strict-small';
  }
  if (value % 2 === 0) {
    return 'even';
  }
  return 'odd';
}
