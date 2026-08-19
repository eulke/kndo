// CJS exports consumed by an ESM named import.
exports.util = function () {
  return 42;
};

// Declared via exports.deadUtil but never imported by anyone — dead.
exports.deadUtil = function () {};
