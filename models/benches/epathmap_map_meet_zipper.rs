//! Experiment 255: reconstructed exact map meet versus synchronized zippers.
//!
//! The control is the production topology-mask/reconstruction algorithm at
//! `eae9ea1fb`. The treatment traverses only byte children shared by both
//! PathMaps and writes equal shared values or common value-free endpoints into
//! one result trie. Both return the same first canonical conflict key.

#[path = "paired.rs"]
mod paired;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::rhoapi_ext::{EPathMapAlgebraError, EntryTrie};

type Fixture = (EPathMap, EPathMap);
type MeetProduct = Result<EntryTrie, EPathMapAlgebraError>;
type MeetArm<'a> = dyn FnMut(&Fixture) -> MeetProduct + 'a;

const ENTRIES: usize = 2_048;

fn bytes_par(bytes: Vec<u8>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(bytes)),
    }])
}

fn key(namespace: u8, index: usize, shared_prefix: usize) -> Par {
    let mut bytes = Vec::with_capacity(shared_prefix + 9);
    bytes.resize(shared_prefix, 0x71);
    bytes.push(namespace);
    bytes.extend_from_slice(&(index as u64).to_be_bytes());
    bytes_par(bytes)
}

fn value(index: usize, bytes: usize, salt: u8) -> Par {
    let mut payload = Vec::with_capacity(bytes.max(9));
    payload.extend_from_slice(&(index as u64).to_be_bytes());
    payload.push(salt);
    payload.resize(bytes.max(9), (index as u8).wrapping_add(salt));
    bytes_par(payload)
}

fn map(
    namespace: u8,
    indices: impl IntoIterator<Item = usize>,
    shared_prefix: usize,
    value_bytes: usize,
    conflict: Option<usize>,
) -> EPathMap {
    EPathMap::new_map(
        indices.into_iter().map(|index| {
            let salt = u8::from(conflict == Some(index));
            (
                key(namespace, index, shared_prefix),
                value(index, value_bytes, salt),
            )
        }),
        Vec::new(),
        false,
        None,
    )
}

fn sparse_associations_over_common_topology() -> Fixture {
    let mut left = map(7, (0..ENTRIES).step_by(2), 32, 64, None);
    let mut right = map(7, (1..ENTRIES).step_by(2), 32, 64, None);
    for index in 0..ENTRIES {
        let encoded = encode_trie_path(&key(7, index, 32));
        left.create_path(&encoded)
            .expect("map specialization exists");
        right
            .create_path(&encoded)
            .expect("map specialization exists");
    }
    (left, right)
}

fn topology_only(namespace: u8, range: std::ops::Range<usize>) -> EPathMap {
    let first = range.start;
    let anchor = key(namespace, first, 64);
    let anchor_encoded = encode_trie_path(&anchor);
    let mut result = EPathMap::new_map([(anchor, value(first, 64, 0))], Vec::new(), false, None);
    for index in range.skip(1) {
        result
            .create_path(&encode_trie_path(&key(namespace, index, 64)))
            .expect("map specialization exists");
    }
    assert!(result.remove_encoded_entry(&anchor_encoded));
    assert_eq!(result.len(), 0);
    result
}

fn assert_exact(label: &str, control: &MeetProduct, treatment: &MeetProduct) {
    match (control, treatment) {
        (Ok(control), Ok(treatment)) => {
            assert_eq!(control, treatment, "EntryTrie result for {label}");
            assert_eq!(
                control.trie_snapshot(),
                treatment.trie_snapshot(),
                "EPM1 result for {label}"
            );
        }
        (Err(control), Err(treatment)) => {
            assert_eq!(control, treatment, "first conflict for {label}");
        }
        _ => panic!("control and treatment disposition diverged for {label}"),
    }
}

fn measure_case(label: &str, fixture: Fixture) {
    let control = fixture.0.entry_trie().try_meet(fixture.1.entry_trie());
    let treatment = fixture
        .0
        .entry_trie()
        .try_meet_zipper_treatment(fixture.1.entry_trie());
    assert_exact(label, &control, &treatment);

    let workload = [fixture];
    let mut reconstructed =
        |fixture: &Fixture| fixture.0.entry_trie().try_meet(fixture.1.entry_trie());
    let mut zipper = |fixture: &Fixture| {
        fixture
            .0
            .entry_trie()
            .try_meet_zipper_treatment(fixture.1.entry_trie())
    };
    let mut arms: [&mut MeetArm<'_>; 2] = [&mut reconstructed, &mut zipper];
    let mut release = |products: std::vec::Drain<'_, MeetProduct>| drop(products);
    let result = paired::measure_arms(
        ["topology-reconstruction", "zipper-intersection"],
        &workload,
        &mut arms,
        &mut release,
    );

    println!("case={label}");
    result.print();
    result.report(label, 0, 1);
    for (sample, (control_ns, treatment_ns)) in result.samples[0]
        .times_ns
        .iter()
        .zip(&result.samples[1].times_ns)
        .enumerate()
    {
        println!("sample\t{label}\t{sample}\t{control_ns:.0}\t{treatment_ns:.0}");
    }
}

fn main() {
    println!("EPathMap exact map meet: {ENTRIES} nominal entries per operand");

    measure_case(
        "disjoint-roots-value-64B",
        (
            map(1, 0..ENTRIES, 16, 64, None),
            map(2, 0..ENTRIES, 16, 64, None),
        ),
    );
    measure_case(
        "disjoint-roots-value-4096B",
        (
            map(1, 0..ENTRIES, 16, 4_096, None),
            map(2, 0..ENTRIES, 16, 4_096, None),
        ),
    );
    measure_case(
        "half-overlap-value-64B",
        (
            map(7, 0..ENTRIES, 16, 64, None),
            map(7, ENTRIES / 2..ENTRIES + ENTRIES / 2, 16, 64, None),
        ),
    );
    measure_case(
        "independently-equal-value-64B",
        (
            map(7, 0..ENTRIES, 16, 64, None),
            map(7, 0..ENTRIES, 16, 64, None),
        ),
    );
    measure_case(
        "deep-shared-prefix-value-64B",
        (
            map(7, 0..ENTRIES, 128, 64, None),
            map(7, 0..ENTRIES, 128, 64, None),
        ),
    );
    measure_case(
        "sparse-associations-common-topology-64B",
        sparse_associations_over_common_topology(),
    );
    measure_case(
        "partially-overlapping-value-free-topology",
        (
            topology_only(7, 0..ENTRIES),
            topology_only(7, ENTRIES / 2..ENTRIES + ENTRIES / 2),
        ),
    );
    measure_case(
        "early-value-conflict-64B",
        (
            map(7, 0..ENTRIES, 16, 64, None),
            map(7, 0..ENTRIES, 16, 64, Some(0)),
        ),
    );
    measure_case(
        "late-value-conflict-64B",
        (
            map(7, 0..ENTRIES, 16, 64, None),
            map(7, 0..ENTRIES, 16, 64, Some(ENTRIES - 1)),
        ),
    );
}
