//! Thread-local forced-digest support shared by candidate-order tests.
//!
//! This module is included only by `rspace` under `cfg(test)`. Production
//! builds therefore contain neither the override state nor a conditional
//! branch. The guard is deliberately `!Send`: a forced digest cannot leak
//! across executor threads, and collision lifecycle tests use a current-thread
//! Tokio runtime.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

#[derive(Clone)]
struct Override {
    digest: Blake2b256Hash,
    hits: Rc<Cell<usize>>,
}

thread_local! {
    static OVERRIDE: RefCell<Option<Override>> = const { RefCell::new(None) };
}

pub(crate) struct ForcedCandidateDigestGuard {
    previous: Option<Override>,
    hits: Rc<Cell<usize>>,
    not_send: PhantomData<Rc<()>>,
}

impl ForcedCandidateDigestGuard {
    pub(crate) fn hits(&self) -> usize { self.hits.get() }

    pub(crate) fn reset_hits(&self) { self.hits.set(0); }
}

impl Drop for ForcedCandidateDigestGuard {
    fn drop(&mut self) {
        OVERRIDE.with(|slot| {
            *slot.borrow_mut() = self.previous.take();
        });
    }
}

pub(crate) fn force(digest: Blake2b256Hash) -> ForcedCandidateDigestGuard {
    let hits = Rc::new(Cell::new(0));
    let replacement = Override {
        digest,
        hits: Rc::clone(&hits),
    };
    let previous = OVERRIDE.with(|slot| slot.borrow_mut().replace(replacement));
    ForcedCandidateDigestGuard {
        previous,
        hits,
        not_send: PhantomData,
    }
}

pub(crate) fn digest(identity: &[u8]) -> Blake2b256Hash {
    OVERRIDE.with(|slot| {
        if let Some(forced) = slot.borrow().as_ref() {
            forced.hits.set(forced.hits.get().saturating_add(1));
            forced.digest.clone()
        } else {
            Blake2b256Hash::new(identity)
        }
    })
}
