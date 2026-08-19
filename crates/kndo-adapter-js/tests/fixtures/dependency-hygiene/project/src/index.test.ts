// Note: this file also shows up as `unused:file` — no adapter emits `RootKind::Test` edges
// yet (test-root detection is roadmap M3), so a test file nothing else imports is, today,
// indistinguishable from a genuinely dead one. Expected, not a bug in this fixture.
import { expect } from "chai";

expect(1).to.equal(1);
