//! Experiment 254: native PathMap prefix restriction versus a strong zipper control.
//!
//! The control focuses the source below every value-bearing selector prefix and
//! grafts that complete compressed subtrie at the same absolute destination
//! path. The treatment invokes `PathMap::restrict` once. Both preserve source
//! values and value-free topology; neither decodes a Rholang term or flattens
//! the trie into an entry collection.

#[path = "paired.rs"]
mod paired;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use pathmap::arena_compact::ArenaCompactTree;
use pathmap::zipper::{Zipper, ZipperValues, ZipperWriting};
use pathmap::PathMap;

type Fixture = (PathMap<Par>, PathMap<Par>);
type RestrictArm<'a> = dyn FnMut(&Fixture) -> PathMap<Par> + 'a;

const GROUPS: usize = 64;
const ITEMS_PER_GROUP: usize = 64;

fn value(index: usize, bytes: usize) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(vec![(index & 0xff) as u8; bytes])),
    }])
}

fn data(common_depth: usize, value_bytes: usize) -> PathMap<Par> {
    let common = vec![0x71; common_depth];
    let mut map = PathMap::new();
    for group in 0..GROUPS {
        for item in 0..ITEMS_PER_GROUP {
            let ordinal = group * ITEMS_PER_GROUP + item;
            let mut key = Vec::with_capacity(common_depth + 3);
            key.extend_from_slice(&common);
            key.push(group as u8);
            key.push(item as u8);
            key.push(0x01);
            map.insert(key.clone(), value(ordinal, value_bytes));

            // A value-free endpoint exercises topology preservation rather
            // than comparing only the ordinary key/value iterator.
            if item == 0 {
                key.extend_from_slice(&[0xfe, 0x02]);
                map.create_path(key);
            }
        }
    }
    map
}

fn mask(prefixes: impl IntoIterator<Item = Vec<u8>>) -> PathMap<Par> {
    PathMap::from_iter(prefixes.into_iter().map(|prefix| (prefix, Par::default())))
}

fn group_prefixes(common_depth: usize) -> Vec<Vec<u8>> {
    let common = vec![0x71; common_depth];
    (0..GROUPS)
        .map(|group| {
            let mut prefix = common.clone();
            prefix.push(group as u8);
            prefix
        })
        .collect()
}

/// Semantics-equivalent control composed solely from focused PathMap zippers.
fn focused_zipper_union(data: &PathMap<Par>, selectors: &PathMap<Par>) -> PathMap<Par> {
    let mut result = PathMap::new();
    for (prefix, _) in selectors.iter() {
        if prefix.is_empty() {
            return data.clone();
        }

        let source = data.read_zipper_at_path(&prefix);
        if !source.path_exists() {
            continue;
        }
        let focus_value = source.val().cloned();
        let mut destination = result.write_zipper_at_path(&prefix);
        destination.graft(&source);

        // PathMap intentionally feature-gates whether `graft` carries a focus
        // value. The control cannot depend on that build-time choice.
        if let Some(value) = focus_value {
            if destination.val().is_none() {
                destination.set_val(value);
            }
        }
    }
    result
}

fn topology_image(map: &PathMap<Par>) -> Vec<u8> {
    ArenaCompactTree::from_zipper(map.read_zipper(), |_| 0)
        .get_data()
        .to_vec()
}

fn assert_exact(label: &str, control: &PathMap<Par>, treatment: &PathMap<Par>) {
    let control_entries: Vec<_> = control.iter().collect();
    let treatment_entries: Vec<_> = treatment.iter().collect();
    assert_eq!(
        control_entries.len(),
        treatment_entries.len(),
        "value count for {label}"
    );
    for ((control_key, control_value), (treatment_key, treatment_value)) in
        control_entries.into_iter().zip(treatment_entries)
    {
        assert_eq!(control_key, treatment_key, "key order for {label}");
        assert_eq!(
            control_value.cmp(treatment_value),
            std::cmp::Ordering::Equal,
            "associated value for {label} at {control_key:?}"
        );
    }
    assert_eq!(
        topology_image(control),
        topology_image(treatment),
        "compressed topology for {label}"
    );
}

fn measure_case(label: &str, fixture: Fixture) {
    let control = focused_zipper_union(&fixture.0, &fixture.1);
    let treatment = fixture.0.restrict(&fixture.1);
    assert_exact(label, &control, &treatment);

    let workload = [fixture];
    let mut focused = |fixture: &Fixture| focused_zipper_union(&fixture.0, &fixture.1);
    let mut native = |fixture: &Fixture| fixture.0.restrict(&fixture.1);
    let mut arms: [&mut RestrictArm<'_>; 2] = [&mut focused, &mut native];
    let mut release = |products: std::vec::Drain<'_, PathMap<Par>>| drop(products);
    let result = paired::measure_arms(
        ["focused-zipper", "native-restrict"],
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
    println!(
        "PathMap restrict frontier: {} values across {} prefixes",
        GROUPS * ITEMS_PER_GROUP,
        GROUPS
    );

    measure_case(
        "singleton-prefix-64B",
        (data(8, 64), mask([group_prefixes(8)[17].clone()])),
    );
    measure_case(
        "bulk-disjoint-prefixes-64B",
        (data(8, 64), mask(group_prefixes(8))),
    );
    measure_case(
        "bulk-disjoint-prefixes-4096B",
        (data(8, 4_096), mask(group_prefixes(8))),
    );

    let common = vec![0x71; 64];
    measure_case(
        "nested-overlapping-prefixes-64B",
        (
            data(64, 64),
            mask((1..=common.len()).map(|end| common[..end].to_vec())),
        ),
    );
    measure_case("empty-mask-64B", (data(8, 64), PathMap::new()));
    measure_case("root-wildcard-64B", (data(8, 64), mask([Vec::new()])));
    measure_case(
        "disjoint-no-hit-mask-64B",
        (
            data(8, 64),
            mask((0..GROUPS).map(|group| vec![0xe1, group as u8])),
        ),
    );
    measure_case(
        "deep-shared-prefixes-64B",
        (data(128, 64), mask(group_prefixes(128))),
    );

    let mut fallback_dominated = group_prefixes(8);
    fallback_dominated.push(Vec::new());
    measure_case(
        "root-fallback-plus-exact-prefixes-64B",
        (data(8, 64), mask(fallback_dominated)),
    );
}
