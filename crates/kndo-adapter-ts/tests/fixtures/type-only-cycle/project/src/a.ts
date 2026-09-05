// The control: a value import in both directions IS an initialization cycle,
// anchored at the lexicographically first participant (this file).
import { b } from "./b.js";

export const a = () => b();
