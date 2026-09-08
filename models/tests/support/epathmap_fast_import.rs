use std::cell::Cell;

use super::{canonical_singleton_epathmap_chain_proof, read_canonical_varint};
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{EPathMap, Expr, Par};
use crate::rust::epathmap_trie_codec::{
    inspect_canonical_map_snapshot, inspect_canonical_snapshot, snapshot_route_hint,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Work {
    inspections: usize,
    copied_keys: usize,
    copied_bytes: usize,
}

thread_local! {
    static OBSERVED: Cell<Option<Work>> = const { Cell::new(None) };
}

pub(crate) fn record_inspection() {
    OBSERVED.with(|cell| {
        if let Some(mut work) = cell.get() {
            work.inspections += 1;
            cell.set(Some(work));
        }
    });
}

pub(crate) fn record_key_copy(bytes: usize) {
    OBSERVED.with(|cell| {
        if let Some(mut work) = cell.get() {
            work.copied_keys += 1;
            work.copied_bytes += bytes;
            cell.set(Some(work));
        }
    });
}

fn observe<T>(action: impl FnOnce() -> T) -> (T, Work) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { OBSERVED.with(|cell| cell.set(None)); }
    }
    OBSERVED.with(|cell| assert!(cell.replace(Some(Work::default())).is_none()));
    let reset = Reset;
    let result = action();
    let work = OBSERVED.with(|cell| cell.get().unwrap());
    drop(reset);
    (result, work)
}

fn int(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn string(value: String) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(value)),
    }])
}

fn carrier(map: EPathMap) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map)),
    }])
}

fn map(width: usize, prefix_len: usize) -> EPathMap {
    let prefix = "x".repeat(prefix_len);
    EPathMap::new_map(
        (0..width).map(|index| (string(format!("{prefix}-{index:08}")), int(index as i64))),
        Vec::new(),
        false,
        None,
    )
}

fn arena_and_count(snapshot: &[u8]) -> (usize, usize, usize) {
    let mut at = 6;
    let arena_len = usize::try_from(read_canonical_varint(snapshot, &mut at).unwrap()).unwrap();
    let arena_start = at;
    at += arena_len;
    let count_at = at;
    read_canonical_varint(snapshot, &mut at).unwrap();
    (arena_start, count_at, at)
}

#[test]
fn singleton_prefilter_rejects_wide_maps_before_inspection_or_expanded_key_copy() {
    for width in [2, 16, 64] {
        let map = map(width, 4096);
        let snapshot = map.trie_snapshot();
        let (observed_width, control) =
            observe(|| inspect_canonical_map_snapshot(snapshot).unwrap().keys.len());
        assert_eq!(observed_width, width);
        assert_eq!(control.inspections, 1);
        assert!(
            control.copied_keys > 0,
            "unfiltered control must visit the actual copy branch"
        );
        assert!(
            control.copied_bytes >= 4096 * width,
            "shared prefix must be copied across endpoints: {control:?}"
        );
        let (accepted, work) =
            observe(|| canonical_singleton_epathmap_chain_proof(snapshot).is_some());
        assert!(!accepted);
        assert_eq!(
            work,
            Work::default(),
            "CEI-07 wide input must decline before inspection"
        );
    }
}

#[test]
fn singleton_prefilter_checks_width_at_every_nested_level() {
    let mut outer = map(16, 256);
    for _ in 0..3 {
        outer = EPathMap::new_map(
            [(string("outer".to_owned()), carrier(outer))],
            Vec::new(),
            false,
            None,
        );
    }
    let snapshot = outer.trie_snapshot();
    let (accepted, work) = observe(|| canonical_singleton_epathmap_chain_proof(snapshot).is_some());
    assert!(!accepted);
    assert_eq!(
        work.inspections, 3,
        "the nested wide map must not trigger a fourth inspection"
    );
}

#[test]
fn singleton_prefilter_forged_count_still_requires_actual_act_validation() {
    let map = map(16, 256);
    let snapshot = map.trie_snapshot();
    let (_, count_at, mut first_end) = arena_and_count(snapshot);
    let first_len =
        usize::try_from(read_canonical_varint(snapshot, &mut first_end).unwrap()).unwrap();
    first_end += first_len;
    let mut forged = snapshot[..first_end].to_vec();
    forged[count_at] = 1;
    assert_eq!(snapshot_route_hint(&forged).unwrap().map_value_count(), 1);
    let (accepted, work) = observe(|| canonical_singleton_epathmap_chain_proof(&forged).is_some());
    assert!(
        !accepted,
        "a forged declaration cannot replace ACT value-count validation"
    );
    assert_eq!(work.inspections, 1);
    assert_eq!(work.copied_keys, 0);
    assert_eq!(work.copied_bytes, 0);
}

#[test]
fn singleton_prefilter_preserves_positive_admission_and_rejects_same_hint_corruption() {
    let map = map(1, 8);
    let snapshot = map.trie_snapshot();
    let hint = snapshot_route_hint(snapshot).unwrap();
    let (accepted, work) = observe(|| canonical_singleton_epathmap_chain_proof(snapshot).is_some());
    assert!(accepted, "CEI-07 non-vacuous valid singleton");
    assert_eq!(work.inspections, 1);

    let (arena_start, _, _) = arena_and_count(snapshot);
    let mut malformed = snapshot.to_vec();
    malformed[arena_start] = 255;
    assert_eq!(snapshot_route_hint(&malformed).unwrap(), hint);
    let (accepted, work) =
        observe(|| canonical_singleton_epathmap_chain_proof(&malformed).is_some());
    assert!(!accepted);
    assert_eq!(
        work.inspections, 1,
        "same-hint corruption must reach full inspection"
    );
    assert_eq!(work.copied_keys, 0);

    for invalid in [b"".as_slice(), b"EPM1\x01\x09\x00\x00".as_slice()] {
        let (accepted, work) =
            observe(|| canonical_singleton_epathmap_chain_proof(invalid).is_some());
        assert!(!accepted);
        assert_eq!(work, Work::default());
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("regressions"),
        )),
        ..proptest::test_runner::Config::with_cases(128)
    })]

    #[test]
    fn singleton_prefilter_full_inspection_refines_hint_for_generated_and_mutated_images(
        width in 0usize..17,
        prefix_len in 0usize..65,
        offset in proptest::prelude::any::<u16>(),
        replacement in proptest::prelude::any::<u8>(),
        mutation in 0u8..3,
    ) {
        let map = map(width, prefix_len);
        let original = map.trie_snapshot();
        let mut changed = original.to_vec();
        let at = usize::from(offset) % changed.len();
        match mutation {
            0 => changed[at] = replacement,
            1 => changed.truncate(at),
            _ => changed.push(replacement),
        }
        proptest::prop_assert!(inspect_canonical_snapshot(original).is_ok());
        for snapshot in [original, changed.as_slice()] {
            if let Ok(inspection) = inspect_canonical_snapshot(snapshot) {
                let hint = snapshot_route_hint(snapshot).ok()
                    .map(|hint| (hint.mode(), hint.map_value_count()));
                proptest::prop_assert_eq!(
                    hint,
                    Some((inspection.mode, inspection.value_bodies.len())),
                );
            }
        }
    }
}
