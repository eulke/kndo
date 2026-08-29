//! Development-time tasks, as a library so their own tests can call them.
//!
//! `main.rs` is the CLI over this; anything a test needs to assert about a task lives here.

pub mod doc_freshness;
pub mod package;
