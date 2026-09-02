// The "dynamic-import directory scan" shape: the template's static prefix narrows the
// wildcard to src/handlers — everything there stays alive, nothing outside gets a free pass.
export async function load(name: string) {
  return import(`./handlers/${name}`);
}
