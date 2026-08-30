// A NAMED default export: the declaration keeps its own name, but a consumer's default
// import binds `default`. Without the alias linking the two, this reads unused.
export default function mergeConfig(a, b) {
  return Object.assign({}, a, b);
}
