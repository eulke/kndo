// The test file itself is exempt from `test-only` (test-role files are themselves
// excluded — reporting a test as test-only would be trivially true, not a finding).
import { helper } from "./core";
import { sample } from "./fixtures";

console.log(helper(), sample);
