//! The one wasmtime [`Engine`] every bridge in this crate shares.
//!
//! One engine instead of one-per-load for two reasons. First, an `Engine` owns the Cranelift
//! compilation context and its configuration — every host bridge here wants the identical
//! config (fuel metering on, compilation cache on), and sharing the handle shares the JIT
//! code cache within the process. Second, the *disk* compilation cache hangs off engine
//! config: with it enabled, `Component::from_binary` on a component wasmtime has seen before
//! (any prior kndo run, same component bytes) skips Cranelift entirely and maps the cached
//! native code, sparing the cost of recompiling the guest every run. Cache setup is
//! best-effort: an unwritable cache directory degrades to compile-every-load, never to a
//! failed load — the same degrade-don't-fail posture as kndo's own facts cache.

use std::sync::OnceLock;

/// Fuel budget per guest call — a per-file fuel/time limit so a plugin cannot break the
/// 500 ms budget. One constant for both bridges — adapters and plugins are held
/// to the same per-call ceiling.
pub(crate) const FUEL_PER_CALL: u64 = 50_000_000;

/// Locks a mutex, recovering from poisoning.
///
/// A poisoned mutex means some thread panicked while holding it. Every lock in this crate
/// guards a pool of guest instances or one round's instance: a panic leaves that value
/// **stale**, never structurally invalid — the worst case is a `GuestState` whose guest
/// trapped, which the next call replaces anyway.
///
/// `expect` here would violate the contract this crate owes and states plainly: a trap or fuel
/// exhaustion degrades to "this component contributed nothing this round", *never* to a failed
/// `kndo check`. With `expect`, one guest's panic poisons the lock and every subsequent
/// acquisition aborts the process — turning one plugin's bug into a dead run for the whole
/// project, and for every other plugin in it. Recovering is what keeps the promise.
pub(crate) fn lock_recovering<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Memory ceiling for one guest instance.
///
/// Fuel bounds *work*, not *bytes*: `memory.grow` costs a handful of fuel units per call and
/// commits megabytes, so a guest can exhaust the host's memory long before it exhausts its
/// 50M-unit budget. This is the other half of the same promise — a plugin cannot break the
/// run — and it is the half that fails as an OOM kill rather than a caught trap.
///
/// 256 MiB: an order of magnitude above anything a convention plugin needs (the reference
/// guests peak in the low single-digit MiB) and an order of magnitude below what would matter
/// on a machine running a build. Exceeding it fails the `memory.grow` inside the guest, which
/// reaches the host as an ordinary trap and degrades to "this component contributed nothing
/// this round" — the same outcome as fuel exhaustion, through the same path.
const MAX_GUEST_MEMORY_BYTES: usize = 256 * 1024 * 1024;

/// The limiter every guest store installs. Deliberately memory-only: table and instance
/// counts are bounded by the component's own type section, which the host validates at load.
pub(crate) fn guest_limits() -> wasmtime::StoreLimits {
    wasmtime::StoreLimitsBuilder::new()
        .memory_size(MAX_GUEST_MEMORY_BYTES)
        .build()
}

/// Store data for a world with **no host imports** — the coverage ingester and the adapter
/// bridge. It exists only to carry the limiter: `Store<()>` has nowhere to put one, and
/// wasmtime resolves a limiter out of store data.
pub(crate) struct NoImports {
    pub(crate) limits: wasmtime::StoreLimits,
}

impl NoImports {
    pub(crate) fn new() -> Self {
        NoImports {
            limits: guest_limits(),
        }
    }
}

/// The process-wide engine: fuel metering enabled, disk compilation cache enabled when the
/// environment allows it.
///
/// **Epoch interruption is deliberately not enabled.** It is wall-clock, and kndo guarantees
/// byte-identical output across thread counts and machines (`threads_determinism`,
/// `patch_equivalence`): a guest cut off by elapsed time would contribute different facts on a
/// loaded machine than on an idle one. Fuel is instruction-counted and therefore deterministic,
/// which is why it — not a deadline — is what bounds a runaway guest here.
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
