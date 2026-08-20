export function scoreLabel(points: number): string {
  let label = 'none';
  for (let i = 0; i < points; i += 1) {
    if (i % 3 === 0) {
      label = 'fizz';
    } else if (i % 5 === 0) {
      label = 'buzz';
    }
  }
  while (label.length < 6) {
    label = label + '!';
  }
  return points > 0 && label !== 'none' ? label : 'zero';
}
