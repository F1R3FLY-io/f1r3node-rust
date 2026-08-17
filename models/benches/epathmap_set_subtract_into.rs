//! Experiment 256: whole-map set subtraction versus root `subtract_into`.
//!
//! The control is production `EntryTrie::try_subtract` at `7b16eaa83`.
//! The treatment clones the left `PathMap<()>` root and destructively subtracts
//! the right read-zipper focus. Map mode is not eligible: `Par` deliberately has
//! no invented distributive lattice.

#[path = "paired.rs"]
mod paired;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::rhoapi_ext::{EPathMapAlgebraError, EntryTrie};

type Fixture = (EPathMap, EPathMap);
type SubtractProduct = Result<EntryTrie, EPathMapAlgebraError>;
type SubtractArm<'a> = dyn FnMut(&Fixture) -> SubtractProduct + 'a;

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

fn set(namespace: u8, indices: impl IntoIterator<Item = usize>, shared_prefix: usize) -> EPathMap {
    EPathMap::new(
        indices
            .into_iter()
            .map(|index| key(namespace, index, shared_prefix))
            .collect::<Vec<_>>(),
        Vec::new(),
        false,
        None,
    )
}

fn topology_only(
    namespace: u8,
    indices: impl IntoIterator<Item = usize>,
    shared_prefix: usize,
) -> EPathMap {
    let anchor = bytes_par(vec![0xff; 17]);
    let anchor_key = encode_trie_path(&anchor);
    let mut result = EPathMap::new(vec![anchor], Vec::new(), false, None);
    for index in indices {
        result
            .create_path(&encode_trie_path(&key(namespace, index, shared_prefix)))
            .expect("set specialization exists");
    }
    assert!(result.remove_encoded_entry(&anchor_key));
    assert_eq!(result.len(), 0);
    result
}

fn assert_exact(label: &str, fixture: &Fixture) {
    let left_before = fixture.0.trie_snapshot().to_vec();
    let right_before = fixture.1.trie_snapshot().to_vec();
    let control = fixture.0.entry_trie().try_subtract(fixture.1.entry_trie());
    let treatment = fixture
        .0
        .entry_trie()
        .try_subtract_into_treatment(fixture.1.entry_trie());
    assert_eq!(control, treatment, "EntryTrie result for {label}");
    assert_eq!(
        control.as_ref().unwrap().trie_snapshot(),
        treatment.as_ref().unwrap().trie_snapshot(),
        "EPM1 result for {label}"
    );
    assert_eq!(
        fixture.0.trie_snapshot(),
        left_before,
        "left operand for {label}"
    );
    assert_eq!(
        fixture.1.trie_snapshot(),
        right_before,
        "right operand for {label}"
    );
}

fn measure_case(label: &str, fixture: Fixture) {
    assert_exact(label, &fixture);

    let workload = [fixture];
    let mut whole_map =
        |fixture: &Fixture| fixture.0.entry_trie().try_subtract(fixture.1.entry_trie());
    let mut subtract_into = |fixture: &Fixture| {
        fixture
            .0
            .entry_trie()
            .try_subtract_into_treatment(fixture.1.entry_trie())
    };
    let mut arms: [&mut SubtractArm<'_>; 2] = [&mut whole_map, &mut subtract_into];
    let mut release = |products: std::vec::Drain<'_, SubtractProduct>| drop(products);
    let result = paired::measure_arms(
        ["whole-map-subtract", "root-subtract-into"],
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
    println!("Set-mode EPathMap subtraction: {ENTRIES} nominal entries");

    measure_case(
        "disjoint-roots",
        (set(1, 0..ENTRIES, 16), set(2, 0..ENTRIES, 16)),
    );
    measure_case(
        "identical",
        (set(7, 0..ENTRIES, 16), set(7, 0..ENTRIES, 16)),
    );
    measure_case(
        "half-overlap",
        (
            set(7, 0..ENTRIES, 16),
            set(7, ENTRIES / 2..ENTRIES + ENTRIES / 2, 16),
        ),
    );
    measure_case(
        "singleton-right",
        (set(7, 0..ENTRIES, 16), set(7, [ENTRIES / 2], 16)),
    );
    measure_case(
        "sparse-right",
        (set(7, 0..ENTRIES, 16), set(7, (0..ENTRIES).step_by(32), 16)),
    );
    measure_case(
        "deep-shared-prefix",
        (set(7, 0..ENTRIES, 128), set(7, ENTRIES / 2..ENTRIES, 128)),
    );
    measure_case(
        "identical-value-free-topology",
        (
            topology_only(7, 0..ENTRIES, 64),
            topology_only(7, 0..ENTRIES, 64),
        ),
    );
    measure_case(
        "partially-overlapping-value-free-topology",
        (
            topology_only(7, 0..ENTRIES, 64),
            topology_only(7, ENTRIES / 2..ENTRIES + ENTRIES / 2, 64),
        ),
    );
}
