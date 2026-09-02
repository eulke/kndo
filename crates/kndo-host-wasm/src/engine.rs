//! The one wasmtime engine every bridge shares, and the budgets that make "a
//! component cannot break the run" true. Both carry from v1 unchanged, with their
//! measurements.

use std::sync::OnceLock;

/// Fuel budget per guest call. Fuel is instruction-counted and therefore
/// deterministic — which is why it, and never a wall-clock deadline, bounds a
/// runaway guest: epoch interruption would let a loaded machine cut a guest off
/// where an idle one would not, and the byte-identity gates compare exactly that.
pub(crate) const FUEL_PER_CALL: u64 = 50_000_000;

/// Memory ceiling for one guest instance. Fuel bounds WORK, not BYTES — a
/// `memory.grow` costs a handful of fuel units and commits megabytes. Exceeding
/// this fails the grow inside the guest, which surfaces as an ordinary trap and
/// degrades to "this call contributed nothing": the same outcome as fuel
/// exhaustion, through the same path. 256 MiB is an order of magnitude above what
/// the reference guests peak at, and an order below what would matter on a machine
/// running a build.
const MAX_GUEST_MEMORY_BYTES: usize = 256 * 1024 * 1024;

pub(crate) fn guest_limits() -> wasmtime::StoreLimits {
    wasmtime::StoreLimitsBuilder::new()
        .memory_size(MAX_GUEST_MEMORY_BYTES)
        .build()
}

/// Process-wide engine: fuel metering on, wasmtime's own content-addressed disk
/// compilation cache on when the environment allows (an unwritable cache dir
/// degrades to compile-every-load, never a failed load).
pub(crate) fn shared_engine() -> &'static wasmtime::Engine {
    static ENGINE: OnceLock<wasmtime::Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);
        let _ = config.cache_config_load_default();
        wasmtime::Engine::new(&config).unwrap_or_else(|e| {
            // Only reachable if the fuel configuration itself were invalid — a
            // compile-time property of this crate, not an environment condition.
            panic!("kndo-host-wasm: engine configuration rejected: {e}")
        })
    })
}

/// A fresh, budgeted store per guest CALL. One instance per call is the
/// determinism-first lifecycle: no guest state survives between calls, so an
/// enumeration answered during one call can never poison another, and parallel
/// extraction needs no shared-instance lock. The instantiation cost rides the
/// engine's JIT cache.
pub(crate) fn budgeted_store<T>(data: T) -> wasmtime::Store<T> {
    let mut store = wasmtime::Store::new(shared_engine(), data);
    store
        .set_fuel(FUEL_PER_CALL)
        .expect("fuel metering is enabled on the shared engine");
    store
}
