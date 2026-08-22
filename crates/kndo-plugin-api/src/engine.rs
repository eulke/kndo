//! The one wasmtime [`Engine`] every bridge in this crate shares.
//!
//! One engine instead of one-per-load for two reasons. First, an `Engine` owns the Cranelift
//! compilation context and its configuration — every host bridge here wants the identical
//! config (fuel metering on, compilation cache on), and sharing the handle shares the JIT
//! code cache within the process. Second, the *disk* compilation cache hangs off engine
//! config: with it enabled, `Component::from_binary` on a component wasmtime has seen before
//! (any prior kndo run, same component bytes) skips Cranelift entirely and maps the cached
//! native code — the "recompiles the guest every run" cost the performance-parity work
//! (RFC 0017 §4's follow-through) removes. Cache setup is best-effort: an unwritable cache
//! directory degrades to compile-every-load, never to a failed load — the same
//! degrade-don't-fail posture as kndo's own facts cache (ADR 0004).

use std::sync::OnceLock;

/// Fuel budget per guest call (RFC 0003 §3: "per-file fuel/time limits so a plugin cannot
/// break the 500 ms budget"). One constant for both bridges — adapters and plugins are held
/// to the same per-call ceiling.
pub(crate) const FUEL_PER_CALL: u64 = 50_000_000;

/// The process-wide engine: fuel metering enabled, disk compilation cache enabled when the
/// environment allows it.
pub(crate) fn shared_engine() -> &'static wasmtime::Engine {
    static ENGINE: OnceLock<wasmtime::Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);
        // Default cache location (wasmtime's own, e.g. ~/.cache/wasmtime) — content-addressed
        // by wasmtime itself, so a changed component recompiles and an unchanged one loads
        // pre-compiled. Failure here (read-only home, exotic platform) just means no cache.
        let _ = config.cache_config_load_default();
        wasmtime::Engine::new(&config).unwrap_or_else(|e| {
            // Only reachable if the *fuel* configuration itself were invalid — a compile-time
            // property of this crate, not an environment condition.
            panic!("kndo-plugin-api: engine configuration rejected: {e}")
        })
    })
}
