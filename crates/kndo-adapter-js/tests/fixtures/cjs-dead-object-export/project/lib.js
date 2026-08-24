// Both exported through the module.exports object; only helper has an importer.
function helper() {
  return 1;
}

// The export site itself must not count as a use — with no importer binding this name,
// it is dead (the exact case a removed last-call-site produces at a distance).
function legacy() {
  return 2;
}

module.exports = { helper, legacy };
