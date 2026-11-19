//! Ref-counted GC root registry.
//!
//! A Rust-side caller that holds a reference to an unforgeable name — typically
//! by predicting its id via `Tools::unforgeable_name_rng` for an exploratory
//! deploy — can pin that id to prevent the scope-trapped sweep from retiring
//! the name's rspace entries while the reference is live. The pin is a counted
//! reservation: multiple callers may pin the same id concurrently, and only
//! when the last pin releases is the id eligible for sweeping again.
//!
//! The registry is keyed on raw 32-byte GPrivate id bytes today; the public
//! API wrapper in `rholang` accepts a `&Par` and extracts the key via
//! `gprivate_bytes_if_bare`, keeping the extension path open for richer
//! channel shapes without changing caller signatures.
//!
//! Scope. The registry lives on a single `ISpace` (play or replay) and is not
//! shared across them. Callers that need play-and-replay symmetric sweep
//! behavior must mirror pins onto both spaces, or arrange for sweeps on
//! pinned ids to happen on neither.
//!
//! Replay determinism. Pin state is Rust-side and does not flow through the
//! Rholang payload. The sweep's deletion set is `trapped_ids ∩ ¬pinned`; a
//! play/replay root agreement requires identical pin state at each sweep
//! boundary. See `docs/rholang/garbage-collection.md`.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, Weak};

/// A counted pin registry over opaque byte keys (GPrivate ids today).
///
/// Thread-safe. Constructed via `GcRootRegistry::new()`, which returns an
/// `Arc` — pins are issued as weak references back to the owning registry so
/// a dropped handle can decrement the count without the caller juggling the
/// `Arc`.
#[derive(Debug, Default)]
pub struct GcRootRegistry {
    counts: Mutex<HashMap<Vec<u8>, NonZeroU32>>,
}

impl GcRootRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(GcRootRegistry {
            counts: Mutex::new(HashMap::new()),
        })
    }

    /// Pin `key`, returning an RAII handle. Dropping the handle decrements
    /// the pin count. Pinning the same key twice holds a refcount of 2 and
    /// requires two drops to fully release.
    ///
    /// The caller normally reaches this through `rholang::pin_unforgeable`,
    /// which accepts a `&Par` and extracts the GPrivate id.
    pub fn pin_by_key(self: &Arc<Self>, key: Vec<u8>) -> GcRootHandle {
        let mut counts = self.counts.lock().expect("gc_roots pin");
        counts
            .entry(key.clone())
            .and_modify(|c| {
                *c = c.checked_add(1).expect("gc_roots refcount overflow");
            })
            .or_insert(NonZeroU32::new(1).expect("1 is non-zero"));
        GcRootHandle {
            registry: Arc::downgrade(self),
            key,
            released: false,
        }
    }

    /// Returns `true` iff `key` has a live pin.
    pub fn is_pinned_by_key(&self, key: &[u8]) -> bool {
        self.counts
            .lock()
            .expect("gc_roots is_pinned")
            .contains_key(key)
    }

    /// Number of distinct keys with at least one live pin. Diagnostic.
    pub fn pinned_count(&self) -> usize { self.counts.lock().expect("gc_roots pinned_count").len() }

    fn release(&self, key: &[u8]) {
        let mut counts = self.counts.lock().expect("gc_roots release");
        let Some(count) = counts.get_mut(key) else {
            // Unknown-key release: typically means an RAII handle for a key
            // that was manually cleared, or a logic bug in a caller that
            // mixes manual `release` with `Drop`. No-op in release builds,
            // trip the assert in debug.
            debug_assert!(false, "gc_roots: release of unpinned key {:x?}", key);
            return;
        };
        if let Some(dec) = NonZeroU32::new(count.get() - 1) {
            *count = dec;
        } else {
            counts.remove(key);
        }
    }
}

/// RAII pin on a `GcRootRegistry` key. Drop releases the pin.
///
/// Holds a `Weak` reference to the registry so a long-lived handle doesn't
/// keep a dropped space's registry alive. If the registry has been dropped
/// by the time the handle releases, the release is a no-op (the registry
/// — and whatever it was protecting — is already gone).
#[derive(Debug)]
pub struct GcRootHandle {
    registry: Weak<GcRootRegistry>,
    key: Vec<u8>,
    released: bool,
}

impl GcRootHandle {
    /// Release the pin now, without waiting for Drop. Subsequent drops are
    /// no-ops. Equivalent to `drop(handle)` but makes the intent explicit at
    /// a call site that needs the pin released before some later statement.
    pub fn release(mut self) { self.release_once(); }

    /// The key this handle pins. Useful for debugging; callers normally do
    /// not need this.
    pub fn key(&self) -> &[u8] { &self.key }

    fn release_once(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        if let Some(registry) = self.registry.upgrade() {
            registry.release(&self.key);
        }
    }
}

impl Drop for GcRootHandle {
    fn drop(&mut self) { self.release_once(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &[u8] = b"\x01\x02\x03";
    const KEY_B: &[u8] = b"\x04\x05\x06";

    #[test]
    fn empty_registry_pins_nothing() {
        let r = GcRootRegistry::new();
        assert!(!r.is_pinned_by_key(KEY_A));
        assert_eq!(r.pinned_count(), 0);
    }

    #[test]
    fn single_pin_blocks_until_drop() {
        let r = GcRootRegistry::new();
        let h = r.pin_by_key(KEY_A.to_vec());
        assert!(r.is_pinned_by_key(KEY_A));
        assert_eq!(r.pinned_count(), 1);
        drop(h);
        assert!(!r.is_pinned_by_key(KEY_A));
        assert_eq!(r.pinned_count(), 0);
    }

    #[test]
    fn refcount_requires_matching_drops() {
        let r = GcRootRegistry::new();
        let h1 = r.pin_by_key(KEY_A.to_vec());
        let h2 = r.pin_by_key(KEY_A.to_vec());
        assert!(r.is_pinned_by_key(KEY_A));
        drop(h1);
        assert!(r.is_pinned_by_key(KEY_A), "one pin remains");
        drop(h2);
        assert!(!r.is_pinned_by_key(KEY_A), "all pins released");
    }

    #[test]
    fn independent_keys_track_independently() {
        let r = GcRootRegistry::new();
        let _ha = r.pin_by_key(KEY_A.to_vec());
        assert!(r.is_pinned_by_key(KEY_A));
        assert!(!r.is_pinned_by_key(KEY_B));
        let hb = r.pin_by_key(KEY_B.to_vec());
        assert!(r.is_pinned_by_key(KEY_B));
        drop(hb);
        assert!(r.is_pinned_by_key(KEY_A));
        assert!(!r.is_pinned_by_key(KEY_B));
    }

    #[test]
    fn explicit_release_short_circuits_drop() {
        let r = GcRootRegistry::new();
        let h = r.pin_by_key(KEY_A.to_vec());
        h.release();
        // Dropping a second time would panic under debug_assert without the
        // `released` flag; `release()` consumes the handle so that path is
        // unreachable from safe code — asserted by the test compiling.
        assert!(!r.is_pinned_by_key(KEY_A));
    }

    #[test]
    fn handle_survives_registry_drop() {
        let r = GcRootRegistry::new();
        let h = r.pin_by_key(KEY_A.to_vec());
        drop(r);
        // Holding the Arc inside the handle via Weak lets the registry be
        // reclaimed; dropping the handle is a no-op, not a panic.
        drop(h);
    }

    #[test]
    fn concurrent_pins_and_releases() {
        use std::thread;

        let r = GcRootRegistry::new();
        let threads: Vec<_> = (0..32)
            .map(|i| {
                let r = r.clone();
                thread::spawn(move || {
                    let key = vec![i as u8];
                    for _ in 0..1000 {
                        let h = r.pin_by_key(key.clone());
                        assert!(r.is_pinned_by_key(&key));
                        drop(h);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().expect("thread join");
        }
        assert_eq!(r.pinned_count(), 0, "all pins must have released");
    }
}
