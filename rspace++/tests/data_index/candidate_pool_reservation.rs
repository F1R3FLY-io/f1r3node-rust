use std::sync::{Arc, OnceLock};

use super::{Bag, CandidatePool, CandidatePoolEntry, CandidatePositionMap, Datum, Multiplicity};

fn never_decode(_: &[u8]) -> Datum<()> {
    unreachable!("reservation tests do not decode candidate identities")
}

fn pool(entries: &[(&[u8], u64)]) -> CandidatePool<()> {
    let mut authority = Bag::new();
    let ordered = entries
        .iter()
        .map(|(path, count)| {
            authority.set_count(
                path,
                Some(Multiplicity::new(*count).expect("test multiplicities are positive")),
            );
            CandidatePoolEntry {
                storage_path: Arc::<[u8]>::from(*path),
                relative_offset: 0,
                identity_offset: 0,
                datum: OnceLock::new(),
            }
        })
        .collect::<Vec<_>>();
    CandidatePool {
        authority,
        reserved: Bag::new(),
        ordered: ordered.into(),
        positions: CandidatePositionMap::new(),
        fresh: None,
        fresh_available: false,
        decode: never_decode,
    }
}

#[test]
fn reservations_never_mutate_authoritative_multiplicity() {
    let path = &[1, 2, 3][..];
    let mut pool = pool(&[(path, 3)]);
    let authority = pool.authority.clone();

    for expected_reserved in 1..=3 {
        assert!(pool.reserve(0));
        assert_eq!(pool.authority, authority);
        assert_eq!(pool.authority.count(path), 3);
        assert_eq!(pool.reserved.count(path), expected_reserved);
    }
    assert!(pool.is_empty());
    assert!(!pool.reserve(0));
    assert_eq!(pool.authority, authority);

    for expected_reserved in (0..3).rev() {
        assert!(pool.release(0));
        assert_eq!(pool.authority, authority);
        assert_eq!(pool.reserved.count(path), expected_reserved);
    }
    assert!(!pool.release(0), "a double release must not create a candidate");
    assert!(!pool.release(1), "an invalid position must not mutate either Bag");
    assert_eq!(pool.authority, authority);
    assert!(pool.reserved.is_empty());
}

#[test]
fn reservations_are_isolated_by_exact_identity_and_fully_reversible() {
    let left = &[1, 0xAA][..];
    let right = &[1, 0xBB][..];
    let mut pool = pool(&[(left, 2), (right, 1)]);
    let entry_state = pool.clone();

    assert!(pool.reserve(0));
    assert!(pool.is_available(0), "one left occurrence remains");
    assert!(pool.is_available(1), "right identity is independent");
    assert!(pool.reserve(1));
    assert!(!pool.is_available(1));
    assert!(pool.is_available(0));
    assert!(pool.reserve(0));
    assert!(pool.is_empty());

    assert!(pool.release(0));
    assert!(pool.release(1));
    assert!(pool.release(0));
    assert_eq!(pool, entry_state, "complete rollback restores both trie states exactly");
}

#[test]
fn replay_filter_refuses_to_rewrite_authority_during_selection() {
    let mut pool = pool(&[(&[1, 2, 3], 1)]);
    assert!(pool.reserve(0));
    assert!(!pool.retain_before_selection(|_| true));
}

#[test]
fn fresh_prepend_refuses_to_shift_positions_during_selection() {
    let mut pool = pool(&[(&[1, 2, 3], 1)]);
    assert!(pool.reserve(0));
    assert!(!pool.prepend_fresh(Datum::default()));
}
