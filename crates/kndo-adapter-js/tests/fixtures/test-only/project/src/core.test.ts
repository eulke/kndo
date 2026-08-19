// The test file itself is exempt from `test-only` (RFC 0005 §3: "excluding test-role files
// themselves" — reporting a test as test-only would be trivially true, not a finding).
import { helper } from "./core";
import { sample } from "./fixtures";

console.log(helper(), sample);
