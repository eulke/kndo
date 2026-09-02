//! The machine render contracts: projections of [`crate::Report`] beyond its JSON
//! envelope. Every render here is a PURE function of the report — no clock, no
//! environment, no configuration — so the byte-identity the equivalence gates prove
//! for `to_json` extends to these for free, and every frontend emits identical
//! bytes by construction.
//!
//! Two shapes, two framing rules: text documents (`to_agent`) end with a newline;
//! JSON values (`to_json`, `to_sarif`) do not — the frontend frames them. The human
//! terminal rendering is deliberately NOT here: it is presentation, owned by each
//! frontend, free to grow color and width-awareness without a contract moving.
//!
//! A render may subset the envelope; it must never exceed it. Anything a render
//! prints is readable from `to_json` output — the JSON stays the one complete
//! record, and a format that "knows more" than the envelope would be a second
//! source of truth.

mod agent;
mod sarif;
