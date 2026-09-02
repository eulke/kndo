import { test } from 'node:test';
import assert from 'node:assert';
import { add, clamp } from '../src/calc.js';

test('add', () => {
  assert.strictEqual(add(2, 3), 5);
});

test('clamp', () => {
  assert.strictEqual(clamp(5, 0, 10), 5);
  assert.strictEqual(clamp(-1, 0, 10), 0);
  assert.strictEqual(clamp(99, 0, 10), 10);
});
