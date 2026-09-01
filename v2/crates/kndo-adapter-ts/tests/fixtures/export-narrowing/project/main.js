import { used } from './lib.js';
import * as everything from './ns.js';
export function fromEntry() { return used() + everything.viaNs(); }
