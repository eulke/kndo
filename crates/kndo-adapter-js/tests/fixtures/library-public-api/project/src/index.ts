// Two public exports, neither calling the other or called by anything else in this repo —
// the standalone-utility library shape: a library's public
// API must count as reachable even when nothing in the package's own code calls it.
export function first(): string {
  return "first";
}

export function second(): string {
  return "second";
}

function unusedHelper(): void {
  // Not exported — must still be reported unused despite living in the root file.
}
