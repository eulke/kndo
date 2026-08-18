//! kndo-core — the system behind every frontend.
//!
//! This crate is **language-blind** (RFC 0001 §2, the ignorance rule): it must never contain
//! the name of a language. It defines the neutral vocabulary ([`vocab`]), the contracts
//! adapters and plugins implement ([`adapter`], [`plugin`]), and the only surface frontends
//! may touch ([`engine::Engine`], contracts §5). The core never prints; frontends never
//! compute.
//!
//! Normative source of truth: `docs/contracts/core-traits.md`. Code must match it; changing
//! either requires updating both in the same PR.

pub mod adapter;
pub mod engine;
pub mod plugin;
pub mod vocab;
