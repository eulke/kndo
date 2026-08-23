package util

// Helper is exported but its only caller is a same-package sibling file: the visibility
// ladder narrows it to the Unit rung — "unexported would suffice". Being under
// internal/ its export is not root-promoted, so the evidence is purely the in-graph use.
func Helper() {}
