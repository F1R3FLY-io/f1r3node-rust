//! Paired causal benchmark for EPathMap `setSubtrie` composition.
//!
//! Both arms preserve the historical child-composition codec law. The control
//! walks and reconstructs source topology, then clones/reinserts every value.
//! The treatment grafts the compressed source root into a destination zipper
//! by copy-on-write. Bare-source totality is a separate correctness repair,
//! because the predecessor panics while extending its reconstructed terminal;
//! this timing corpus therefore stays inside the predecessor's split-arm
//! domain.

#[path = "paired.rs"]
mod paired;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::pathmap_integration::{par_to_path, CursorKind};

type Fixture = (EPathMap, Vec<Vec<u8>>, CursorKind, EPathMap);
type SetSubtrieArm<'a> = dyn FnMut(&Fixture) -> EPathMap + 'a;

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

fn list(elements: impl IntoIterator<Item = Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: elements.into_iter().collect(),
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn fixture(entries: usize, map_value_bytes: Option<usize>) -> Fixture {
    let cursor_par = list([gint(999)]);
    let cursor = par_to_path(&cursor_par);
    let old_entries = (0..entries).map(|index| list([gint(999), gint(-(index as i64) - 1)]));
    let unrelated = (0..entries).map(|index| list([gint(888), gint(index as i64)]));
    let source_keys = (0..entries)
        .map(|index| list([gint((index / 16) as i64), gint((index % 16) as i64)]))
        .collect::<Vec<_>>();

    let (destination, source) = match map_value_bytes {
        None => (
            EPathMap::new(
                old_entries.chain(unrelated).collect::<Vec<_>>(),
                Vec::new(),
                false,
                None,
            ),
            EPathMap::new(source_keys, Vec::new(), false, None),
        ),
        Some(value_bytes) => (
            EPathMap::new_map(
                old_entries
                    .chain(unrelated)
                    .enumerate()
                    .map(|(index, key)| {
                        (
                            key,
                            gstring(format!("base-{index:08}-{}", "b".repeat(value_bytes))),
                        )
                    }),
                Vec::new(),
                false,
                None,
            ),
            EPathMap::new_map(
                source_keys.into_iter().enumerate().map(|(index, key)| {
                    (
                        key,
                        gstring(format!("source-{index:08}-{}", "s".repeat(value_bytes))),
                    )
                }),
                Vec::new(),
                false,
                None,
            ),
        ),
    };

    (destination, cursor, CursorKind::Split, source)
}

fn measure_case(label: &str, fixture: Fixture) {
    let mut control = fixture.0.clone();
    control
        .replace_subtrie_reconstructed_control(&fixture.1, fixture.2, &fixture.3)
        .expect("mode-compatible control");
    let mut treatment = fixture.0.clone();
    treatment
        .replace_subtrie(&fixture.1, fixture.2, &fixture.3)
        .expect("mode-compatible treatment");
    assert_eq!(control, treatment, "causal arms diverged for {label}");
    assert_eq!(
        control.trie_snapshot(),
        treatment.trie_snapshot(),
        "EPM1 arms diverged for {label}"
    );

    let workload = [fixture];
    let mut reconstructed = |fixture: &Fixture| {
        let mut destination = fixture.0.clone();
        destination
            .replace_subtrie_reconstructed_control(&fixture.1, fixture.2, &fixture.3)
            .expect("mode-compatible control");
        destination
    };
    let mut native_graft = |fixture: &Fixture| {
        let mut destination = fixture.0.clone();
        destination
            .replace_subtrie(&fixture.1, fixture.2, &fixture.3)
            .expect("mode-compatible treatment");
        destination
    };
    let mut arms: [&mut SetSubtrieArm<'_>; 2] = [&mut reconstructed, &mut native_graft];
    let mut release = |products: std::vec::Drain<'_, EPathMap>| drop(products);
    let result = paired::measure_arms(
        ["reconstructed", "native-graft"],
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
    println!(
        "epathmap setSubtrie composition: {ENTRIES} replaced entries, {ENTRIES} unrelated entries"
    );
    measure_case("set-split", fixture(ENTRIES, None));
    measure_case("map-split-value-64B", fixture(ENTRIES, Some(64)));
    measure_case("map-split-value-4096B", fixture(ENTRIES, Some(4096)));
}
