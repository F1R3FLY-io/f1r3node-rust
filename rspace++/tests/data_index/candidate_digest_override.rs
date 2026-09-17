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

type DigestObservations = Rc<RefCell<Vec<(Vec<u8>, Blake2b256Hash)>>>;

thread_local! {
    static OVERRIDE: RefCell<Option<Override>> = const { RefCell::new(None) };
    static OBSERVATIONS: RefCell<Option<DigestObservations>> = const { RefCell::new(None) };
}

struct DigestObservationGuard {
    previous: Option<DigestObservations>,
    observed: DigestObservations,
}

impl DigestObservationGuard {
    fn new() -> Self {
        let observed = Rc::new(RefCell::new(Vec::new()));
        let previous = OBSERVATIONS.with(|slot| slot.borrow_mut().replace(Rc::clone(&observed)));
        Self { previous, observed }
    }
}

impl Drop for DigestObservationGuard {
    fn drop(&mut self) { OBSERVATIONS.with(|slot| *slot.borrow_mut() = self.previous.take()); }
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
    let result = OVERRIDE.with(|slot| {
        if let Some(forced) = slot.borrow().as_ref() {
            forced.hits.set(forced.hits.get().saturating_add(1));
            forced.digest.clone()
        } else {
            Blake2b256Hash::new(identity)
        }
    });
    OBSERVATIONS.with(|slot| {
        if let Some(observed) = slot.borrow().as_ref() {
            observed
                .borrow_mut()
                .push((identity.to_vec(), result.clone()));
        }
    });
    result
}

#[test]
fn catalog_hash_preimages_are_exact_raw_identity_bytes() {
    use crate::rspace::data_index::RSpaceDataIndex;
    use crate::rspace::internal::Datum;
    use crate::rspace::serializers::serializers::encode_datum;
    use crate::rspace::trace::event::Produce;

    let golden: Datum<String> = Datum {
        a: "data0".to_owned().into(),
        persist: false,
        source: Produce::new(
            Blake2b256Hash::new(&[1, 2, 3]),
            Blake2b256Hash::new(&[4, 5, 6]),
            false,
        ),
    };
    let mut changed = golden.clone();
    changed.a = "data\0other".to_owned().into();
    changed.persist = true;
    let raw = [encode_datum(&golden), encode_datum(&changed)];
    assert_eq!(raw[0].len(), 105);
    let expected_golden = "da6fd6ff637b8b5c4e824f7640cd4ef2f207ab280cc72b943abcc00c3236fef6";

    for channel in ["channel\0one", "different/channel/two"] {
        let channel = channel.to_owned();
        let mut index = RSpaceDataIndex::new();
        index.insert(&channel, &golden).unwrap();
        index.insert(&channel, &golden).unwrap();
        index.insert(&channel, &changed).unwrap();
        let guard = DigestObservationGuard::new();
        let pool = index.candidate_pool::<_, String>(&channel).unwrap();
        let observed = guard.observed.borrow().clone();
        assert_eq!(observed.len(), 2, "hash once per distinct endpoint, not occurrence");
        let mut preimages = observed
            .iter()
            .map(|(identity, _)| identity.clone())
            .collect::<Vec<_>>();
        preimages.sort();
        let mut expected_preimages = raw.to_vec();
        expected_preimages.sort();
        assert_eq!(preimages, expected_preimages);
        for (identity, actual_digest) in &observed {
            assert_eq!(actual_digest.0.len(), 32);
            assert_eq!(*actual_digest, Blake2b256Hash::new(identity));
            if *identity == raw[0] {
                assert_eq!(hex::encode(&actual_digest.0), expected_golden);
            }
        }
        let mut expected = raw
            .iter()
            .map(|identity| (Blake2b256Hash::new(identity).0, identity.clone()))
            .collect::<Vec<_>>();
        expected.sort();
        let actual = (0..pool.len())
            .map(|position| encode_datum(pool.candidate(position).unwrap().0))
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            expected
                .into_iter()
                .map(|(_, identity)| identity)
                .collect::<Vec<_>>()
        );
    }
}
