use std::cell::Cell;

use super::epathmap_envelope_admission_tests::{
    encode, field, frame, reference, reference_carrier,
};
use super::{
    canonical_singleton_epathmap_chain_proof, canonical_singleton_epathmap_parts,
    import_canonical_singleton_epathmap_par, read_canonical_varint,
};
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::var::VarInstance;
use crate::rhoapi::{EPathMap, Expr, Par, Var};
use crate::rust::epathmap_trie_codec::{
    inspect_canonical_map_snapshot, inspect_canonical_snapshot, snapshot_route_hint,
    CanonicalSnapshotKey, EPathMapMode,
};
use crate::rust::rholang::protobuf_encoder;

fn stable_key(snapshot: &[u8], key: &CanonicalSnapshotKey) -> Option<bool> {
    let bytes = match key {
        CanonicalSnapshotKey::Relative(range) => snapshot.get(range.clone())?,
        CanonicalSnapshotKey::Owned(bytes) => bytes,
    };
    crate::rust::canonical_path::validate_canonical_path(bytes)
        .ok()
        .map(|parts| parts.stable)
}

fn nested_reference(value: &[u8]) -> Option<(&[u8], bool)> {
    let (expr, lf, connective) = reference_carrier(value)?;
    let mut input = expr.strip_prefix(&[130])?;
    let body = frame(&mut input, 2)?;
    if !input.is_empty() {
        return None;
    }
    let (snapshot, map_connective, map_lf_empty, remainder) = reference(body)?;
    Some((
        snapshot,
        lf.is_empty() && !connective && map_lf_empty && !map_connective && remainder.is_none(),
    ))
}

fn recursive_chain_reference(snapshot: &[u8]) -> Option<(usize, &[u8], bool)> {
    let inspection = inspect_canonical_map_snapshot(snapshot).ok()?;
    if inspection.mode != EPathMapMode::Map
        || inspection.keys.len() != 1
        || inspection.value_bodies.len() != 1
        || stable_key(snapshot, &inspection.keys[0]) != Some(true)
    {
        return None;
    }
    let value = snapshot.get(inspection.value_bodies[0].clone())?;
    let Some((nested, stable)) = nested_reference(value) else {
        return Some((1, value, true));
    };
    if nested.len() >= snapshot.len() {
        return None;
    }
    let (depth, terminal, rest_stable) = recursive_chain_reference(nested)?;
    Some((depth.checked_add(1)?, terminal, stable && rest_stable))
}

fn observed_chain(snapshot: &[u8]) -> Option<(usize, &[u8], bool)> {
    canonical_singleton_epathmap_chain_proof(snapshot)
        .map(|parts| (parts.depth, parts.terminal, parts.nested_values_stable))
}

fn snapshot_carrier(snapshot: &[u8]) -> Vec<u8> {
    field(42, &field(258, &encode(None, false, None, snapshot)))
}

type Metadata = (bool, bool, bool, bool, bool);

fn metadata_chain(layers: &[Metadata], terminal: i64) -> Par {
    let mut value = int(terminal);
    for &(root_lf, root_connective, map_lf, map_connective, remainder) in layers.iter().rev() {
        value = carrier(EPathMap::new_map(
            [(int(1), value)],
            if map_lf { vec![1] } else { Vec::new() },
            map_connective,
            remainder.then(Var::default),
        ))
        .with_locally_free(if root_lf { vec![1] } else { Vec::new() })
        .with_connective_used(root_connective);
    }
    value
}

fn check_metadata_chain(layers: &[Metadata], terminal: i64) {
    let expected = metadata_chain(layers, terminal);
    let bytes = protobuf_encoder::encode_to_vec(&expected);
    let parts = canonical_singleton_epathmap_parts(&bytes).unwrap();
    let observed = observed_chain(parts.snapshot).unwrap();
    assert_eq!(Some(observed), recursive_chain_reference(parts.snapshot));
    assert_eq!(observed.0, layers.len());
    assert_eq!(observed.1, protobuf_encoder::encode_to_vec(&int(terminal)));
    let clear = |&(root_lf, root_connective, map_lf, map_connective, remainder): &Metadata| {
        !root_lf && !root_connective && !map_lf && !map_connective && !remainder
    };
    assert_eq!(observed.2, layers[1..].iter().all(clear));
    let component_stable = !layers[0].2 && !layers[0].3 && !layers[0].4 && observed.2;
    let imported = import_canonical_singleton_epathmap_par(&bytes).unwrap();
    assert_eq!(
        imported.components().cached_canonical_path_stability(),
        Some(component_stable)
    );
    assert_eq!(
        expected.components().cached_canonical_path_stability(),
        Some(component_stable)
    );
    assert_eq!(
        crate::rust::pathmap_crate_type_mapper::eval_stable_par_for_test(&expected),
        component_stable && !layers[0].0 && !layers[0].1
    );
    assert_eq!(
        crate::rust::pathmap_crate_type_mapper::eval_stable_par_for_test(&imported),
        component_stable && !layers[0].0 && !layers[0].1
    );
    assert_eq!(protobuf_encoder::encode_to_vec(&imported), bytes);
    assert_eq!(imported, expected);
    assert_eq!(imported.locally_free(), expected.locally_free());
    assert_eq!(imported.connective_used(), expected.connective_used());
    assert_eq!(
        crate::rust::canonical_path::encode_trie_path(&imported),
        crate::rust::canonical_path::encode_trie_path(&expected)
    );
}

#[test]
fn singleton_chain_proof_every_metadata_axis_and_integer_extremum() {
    for terminal in [i64::MIN, -1, 0, 1, i64::MAX] {
        let clean = (false, false, false, false, false);
        check_metadata_chain(&[clean, clean, clean], terminal);
        for level in 0..3 {
            for axis in 0..5 {
                let mut layers = [clean; 3];
                layers[level] = (axis == 0, axis == 1, axis == 2, axis == 3, axis == 4);
                check_metadata_chain(&layers, terminal);
            }
        }
    }
}

#[test]
fn singleton_chain_proof_false_stability_does_not_skip_later_act() {
    let child = carrier(map(1, 4)).with_locally_free(vec![1]);
    let outer = EPathMap::new_map([(int(1), child)], Vec::new(), false, None);
    let snapshot = outer.trie_snapshot();
    let (positive, work) = observe(|| observed_chain(snapshot));
    assert_eq!(positive.unwrap().2, false);
    assert_eq!(work.inspections, 2);
    let inspection = inspect_canonical_map_snapshot(snapshot).unwrap();
    let value = &snapshot[inspection.value_bodies[0].clone()];
    let nested = canonical_singleton_epathmap_parts(value).unwrap().snapshot;
    let (arena_start, _, _) = arena_and_count(nested);
    let offset = nested.as_ptr() as usize - snapshot.as_ptr() as usize + arena_start;
    let mut malformed = snapshot.to_vec();
    malformed[offset] = 255;
    let (actual, work) = observe(|| observed_chain(&malformed));
    assert_eq!(actual, None);
    assert_eq!(work.inspections, 2);
    assert_eq!(recursive_chain_reference(&malformed), None);
    assert!(import_canonical_singleton_epathmap_par(&snapshot_carrier(&malformed)).is_none());
}

#[test]
fn singleton_chain_proof_invalid_and_unstable_keys_decline() {
    let valid = map(1, 4);
    let snapshot = valid.trie_snapshot();
    assert!(observed_chain(snapshot).is_some());
    let inspection = inspect_canonical_map_snapshot(snapshot).unwrap();
    assert_eq!(stable_key(snapshot, &inspection.keys[0]), Some(true));
    let CanonicalSnapshotKey::Relative(range) = &inspection.keys[0] else {
        panic!("single inline ACT key fixture must expose its source range");
    };
    let mut invalid = snapshot.to_vec();
    invalid[range.start] = 255;
    let inspected = inspect_canonical_map_snapshot(&invalid).unwrap();
    assert_eq!(stable_key(&invalid, &inspected.keys[0]), None);
    assert_eq!(observed_chain(&invalid), None);
    assert_eq!(recursive_chain_reference(&invalid), None);
    let unstable = EPathMap::new_map(
        [(int(1).with_connective_used(true), int(7))],
        Vec::new(),
        false,
        None,
    );
    let snapshot = unstable.trie_snapshot();
    let inspected = inspect_canonical_map_snapshot(snapshot).unwrap();
    assert_eq!(stable_key(snapshot, &inspected.keys[0]), Some(false));
    assert_eq!(observed_chain(snapshot), None);
    assert_eq!(recursive_chain_reference(snapshot), None);
}

#[test]
fn singleton_chain_proof_opaque_malformed_terminal_is_not_full_import() {
    let inner = carrier(EPathMap::new_map(
        [(int(1), int(7))],
        Vec::new(),
        false,
        Some(Var {
            var_instance: Some(VarInstance::FreeVar(0)),
        }),
    ));
    let outer = EPathMap::new_map([(int(1), inner)], Vec::new(), false, None);
    let snapshot = outer.trie_snapshot();
    assert!(import_canonical_singleton_epathmap_par(&snapshot_carrier(snapshot)).is_some());
    let inspection = inspect_canonical_map_snapshot(snapshot).unwrap();
    let value_range = inspection.value_bodies[0].clone();
    let (expr, _, _) = reference_carrier(&snapshot[value_range.clone()]).unwrap();
    let remainder = super::canonical_epathmap_parts_from_expr(expr)
        .unwrap()
        .remainder
        .unwrap();
    let offset = remainder.as_ptr() as usize - snapshot.as_ptr() as usize;
    let mut malformed = snapshot.to_vec();
    malformed[offset] = 32;
    let proof = observed_chain(&malformed).unwrap();
    assert_eq!(proof.0, 1);
    assert_eq!(proof.1, &malformed[value_range]);
    assert_eq!(Some(proof), recursive_chain_reference(&malformed));
    assert!(!super::canonical_singleton_gint_par(proof.1));
    assert!(import_canonical_singleton_epathmap_par(&snapshot_carrier(&malformed)).is_none());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource("regressions"))),
        ..proptest::test_runner::Config::with_cases(128)
    })]

    #[test]
    fn singleton_chain_proof_recursive_reference_and_metadata(
        layers in proptest::collection::vec(proptest::prelude::any::<Metadata>(), 1..9),
        terminal in proptest::prelude::any::<i64>(),
    ) {
        check_metadata_chain(&layers, terminal);
    }
}

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
