//! Paired causal benchmark for EPathMap member-prefix restriction.
//!
//! The control reconstructs every selector key as a `Par`, splits it back into
//! codec segments, allocates a prefix key, and (in map mode) clones its value
//! into the PathMap mask. The treatment scans canonical segment extents in
//! place, borrows the prefix, and uses an empty `Par` solely to satisfy
//! PathMap's homogeneous mask type. Both arms finish with the identical native
//! `PathMap::restrict` operation.

#[path = "paired.rs"]
mod paired;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::rhoapi_ext::EntryTrie;

type RestrictArm<'a> = dyn FnMut(&(EPathMap, EPathMap)) -> EntryTrie + 'a;

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

fn fixtures(
    buckets: usize,
    entries_per_bucket: usize,
    selected_buckets: usize,
    value_bytes: usize,
) -> (EPathMap, EPathMap, EPathMap, EPathMap) {
    let keys = (0..buckets)
        .flat_map(|bucket| {
            (0..entries_per_bucket)
                .map(move |entry| list([gint(bucket as i64), gint(entry as i64)]))
        })
        .collect::<Vec<_>>();
    let selectors = (0..selected_buckets)
        .map(|bucket| list([gint(bucket as i64)]))
        .collect::<Vec<_>>();

    let set = EPathMap::new(keys.clone(), Vec::new(), false, None);
    let set_selector = EPathMap::new(selectors.clone(), Vec::new(), false, None);
    let map = EPathMap::new_map(
        keys.into_iter().enumerate().map(|(index, key)| {
            (
                key,
                gstring(format!("{index:08}-{}", "v".repeat(value_bytes))),
            )
        }),
        Vec::new(),
        false,
        None,
    );
    let map_selector = EPathMap::new_map(
        selectors.into_iter().enumerate().map(|(index, key)| {
            (
                key,
                gstring(format!("selector-{index:08}-{}", "s".repeat(value_bytes))),
            )
        }),
        Vec::new(),
        false,
        None,
    );
    (set, set_selector, map, map_selector)
}

fn measure_case(label: &str, base: &EPathMap, selector: &EPathMap) {
    let control = base
        .entry_trie()
        .try_restrict_member_prefixes_decoded_control(selector.entry_trie())
        .expect("mode-compatible control");
    let treatment = base
        .entry_trie()
        .try_restrict_member_prefixes(selector.entry_trie())
        .expect("mode-compatible treatment");
    assert_eq!(control, treatment, "causal arms diverged for {label}");

    // Clone before timing. `EPathMap::clone` shares the PathMap roots by CoW,
    // and owned workload elements avoid making the benchmark harness's trait
    // objects carry a function-local reference lifetime.
    let workload = [(base.clone(), selector.clone())];
    let mut decoded = |pair: &(EPathMap, EPathMap)| {
        pair.0
            .entry_trie()
            .try_restrict_member_prefixes_decoded_control(pair.1.entry_trie())
            .expect("mode-compatible control")
    };
    let mut byte_native = |pair: &(EPathMap, EPathMap)| {
        pair.0
            .entry_trie()
            .try_restrict_member_prefixes(pair.1.entry_trie())
            .expect("mode-compatible treatment")
    };
    let mut arms: [&mut RestrictArm<'_>; 2] = [&mut decoded, &mut byte_native];
    let mut release = |products: std::vec::Drain<'_, _>| drop(products);
    let result = paired::measure_arms(
        ["decoded-control", "byte-native"],
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
    let (set, set_selector, map_small, map_small_selector) = fixtures(256, 16, 64, 64);
    let (_, _, map_large, map_large_selector) = fixtures(256, 16, 64, 4096);

    println!("epathmap member-prefix restriction: 4096 base entries, 64 selector prefixes");
    measure_case("set", &set, &set_selector);
    measure_case("map-value-64B", &map_small, &map_small_selector);
    measure_case("map-value-4096B", &map_large, &map_large_selector);
}
