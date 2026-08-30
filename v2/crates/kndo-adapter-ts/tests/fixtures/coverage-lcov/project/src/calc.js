export function add(a, b) {
  return a + b;
}

export function clamp(value, low, high) {
  if (value < low) {
    return low;
  }
  if (value > high) {
    return high;
  }
  return value;
}

export function neverRan(x) {
  if (x > 0) {
    return x * 2;
  }
  return -x;
}
