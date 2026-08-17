//! Paired causal benchmark for exact EPathMap map union.
//!
//! The control is the immediate predecessor: recreate right topology, scan all
//! right values, clone right-only values, and compare equal-key values. The
//! treatment walks both tries together and grafts each right-only compressed
//! branch into one copy-on-write clone. Both retain the exact conflict rule;
//! no lattice is invented for `Par`.

#[path = "paired.rs"]
mod paired;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::rhoapi_ext::EntryTrie;

type Fixture = (EPathMap, EPathMap);
type JoinArm<'a> = dyn FnMut(&Fixture) -> EntryTrie + 'a;

fn gint(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn gstring(value: String) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(value)),
    }])
}

fn key(namespace: i64, index: usize) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![gint(namespace), gint(index as i64)],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn map(namespace: i64, indices: impl IntoIterator<Item = usize>, value_bytes: usize) -> EPathMap {
    EPathMap::new_map(
        indices.into_iter().map(|index| {
            (
                key(namespace, index),
                gstring(format!("{index:08}-{}", "v".repeat(value_bytes))),
            )
        }),
        Vec::new(),
        false,
        None,
    )
}

fn measure_case(label: &str, fixture: Fixture) {
    let control = fixture
        .0
        .entry_trie()
        .try_join_reconstructed_control(fixture.1.entry_trie())
        .expect("conflict-free control");
    let treatment = fixture
        .0
        .entry_trie()
        .try_join(fixture.1.entry_trie())
        .expect("conflict-free treatment");
    assert_eq!(control, treatment, "EntryTrie arms diverged for {label}");
    assert_eq!(
        control.trie_snapshot(),
        treatment.trie_snapshot(),
        "EPM1 arms diverged for {label}"
    );

    let workload = [fixture];
    let mut reconstructed = |fixture: &Fixture| {
        fixture
            .0
            .entry_trie()
            .try_join_reconstructed_control(fixture.1.entry_trie())
            .expect("conflict-free control")
    };
    let mut zipper_graft = |fixture: &Fixture| {
        fixture
            .0
            .entry_trie()
            .try_join(fixture.1.entry_trie())
            .expect("conflict-free treatment")
    };
    let mut arms: [&mut JoinArm<'_>; 2] = [&mut reconstructed, &mut zipper_graft];
    let mut release = |products: std::vec::Drain<'_, EntryTrie>| drop(products);
    let result = paired::measure_arms(
        ["reconstructed", "zipper-graft"],
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
    const ENTRIES: usize = 2_048;
    println!("epathmap exact map union: {ENTRIES} entries per operand");

    measure_case(
        "disjoint-prefix-value-64B",
        (map(1, 0..ENTRIES, 64), map(2, 0..ENTRIES, 64)),
    );
    measure_case(
        "disjoint-prefix-value-4096B",
        (map(1, 0..ENTRIES, 4_096), map(2, 0..ENTRIES, 4_096)),
    );
    measure_case(
        "half-overlap-value-64B",
        (
            map(7, 0..ENTRIES, 64),
            map(7, ENTRIES / 2..ENTRIES + ENTRIES / 2, 64),
        ),
    );
    measure_case(
        "independent-equal-value-64B",
        (map(7, 0..ENTRIES, 64), map(7, 0..ENTRIES, 64)),
    );
    let shared = map(7, 0..ENTRIES, 64);
    measure_case("shared-root-value-64B", (shared.clone(), shared));
}
