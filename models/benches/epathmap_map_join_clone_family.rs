//! Paired causal benchmark for exact EPathMap clone-family map union.
//!
//! The control is the selected synchronized zipper walk. The treatment first
//! proves an unchanged EntryTrie clone family through its mutation-invalidated
//! shared snapshot cell and returns the shared trie directly. Independently
//! constructed equal maps are the guard: they must use the control path.

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

fn key(index: usize) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![gint(7), gint(index as i64)],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn map(entries: usize, value_bytes: usize) -> EPathMap {
    EPathMap::new_map(
        (0..entries).map(|index| {
            (
                key(index),
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
        .try_join_clone_family_control(fixture.1.entry_trie())
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
    let mut zipper = |fixture: &Fixture| {
        fixture
            .0
            .entry_trie()
            .try_join_clone_family_control(fixture.1.entry_trie())
            .expect("conflict-free control")
    };
    let mut clone_family = |fixture: &Fixture| {
        fixture
            .0
            .entry_trie()
            .try_join(fixture.1.entry_trie())
            .expect("conflict-free treatment")
    };
    let mut arms: [&mut JoinArm<'_>; 2] = [&mut zipper, &mut clone_family];
    let mut release = |products: std::vec::Drain<'_, EntryTrie>| drop(products);
    let result = paired::measure_arms(
        ["exact-zipper", "clone-family"],
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
    println!("epathmap exact map clone-family union: {ENTRIES} entries per operand");

    let shared_64 = map(ENTRIES, 64);
    measure_case("shared-root-value-64B", (shared_64.clone(), shared_64));

    let shared_4096 = map(ENTRIES, 4_096);
    measure_case(
        "shared-root-value-4096B",
        (shared_4096.clone(), shared_4096),
    );

    measure_case(
        "independent-equal-value-64B",
        (map(ENTRIES, 64), map(ENTRIES, 64)),
    );
}
