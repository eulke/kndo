import { thing } from "used-dep";
import { p } from "phantom-dep";

const plugin = "spelled-dep/plugin";

export function run() {
  return thing(plugin, p);
}
