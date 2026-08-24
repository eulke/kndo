// CJS entry point requiring an ESM file — one direction of the interop pair.
const { helper } = require("./esm-side.mjs");

exports.run = function () {
  return helper();
};
