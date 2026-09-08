// Reached only through the wildcard alias `@/*`, whose target directory is
// `src/`: the specifier's subpath is resolved against it.
export function probe(): number {
  return 7
}
