// A Test-role file is a test root (role detection seeds test roots) — TestOnly,
// never `unused`, even though nothing imports it. Its chai import still drives the
// `test-only` dependency verdict.
import { expect } from "chai";

expect(1).to.equal(1);
