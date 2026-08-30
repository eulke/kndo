// Static namespace member access resolves precisely: helpers.used stays alive, helpers.dead
// is still caught. A computed access is opaque: everything in registry stays alive.
import * as helpers from "./helpers";
import * as registry from "./registry";

export function run(): string {
  return helpers.used();
}

export function pick(name: string) {
  return registry[name as keyof typeof registry]();
}
