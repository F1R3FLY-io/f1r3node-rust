//! Replay COMM selection must not inherit `Counter` / `HashMap` iteration
//! order.
//!
//! A replay key can name several distinct COMMs. Both `locked_consume` and
//! `locked_produce` try that bucket as an ordered list, so the projection from
//! the multiset is a consensus-visible choice. These tests pin the two pieces
//! of the repair independently: the generic projection sorts even adversarial
//! hash collisions, and `COMM` exposes an equality-consistent total order over
//! every identity field.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::MultisetMultiMap;
use rspace_plus_plus::rspace::trace::event::{COMM, Consume, Produce};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Colliding(u8);

impl Hash for Colliding {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Deliberately force one hash bucket. Without the production sort,
        // insertion permutations then project in different orders reliably
        // instead of depending on RandomState to expose the defect.
        0u8.hash(state);
    }
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    fn visit<T: Clone>(at: usize, row: &mut Vec<T>, out: &mut Vec<Vec<T>>) {
        if at == row.len() {
            out.push(row.clone());
            return;
        }
        for index in at..row.len() {
            row.swap(at, index);
            visit(at + 1, row, out);
            row.swap(at, index);
        }
    }

    let mut row = items.to_vec();
    let mut out = Vec::new();
    visit(0, &mut row, &mut out);
    out
}

#[test]
fn distinct_projection_is_canonical_across_all_insertion_orders() {
    let values: Vec<Colliding> = (0..6).map(Colliding).collect();
    let mut checked = 0usize;

    for insertion_order in permutations(&values) {
        let multimap = MultisetMultiMap::empty();
        for value in &insertion_order {
            multimap.add_binding("shared-io-event", *value);
        }
        // The projection is DISTINCT: a counter multiplicity is not another
        // candidate attempt. Removal still observes the stored count.
        multimap.add_binding("shared-io-event", insertion_order[0]);

        assert_eq!(
            multimap.get_distinct_values_sorted(&"shared-io-event"),
            Some(values.clone()),
            "a Counter insertion permutation changed its ordered projection"
        );
        assert_eq!(
            multimap
                .map
                .get(&"shared-io-event")
                .and_then(|counter| counter.get(&insertion_order[0]).copied()),
            Some(2),
            "canonical projection must not erase the underlying multiplicity"
        );
        checked += 1;
    }

    assert_eq!(checked, 720, "the test must cover all 6! insertion orders");
}

fn digest(byte: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![byte; 32]) }

fn produce(byte: u8) -> Produce { Produce::new(digest(byte), digest(byte + 1), false) }

fn comm(byte: u8) -> COMM {
    let p = produce(byte + 2);
    COMM {
        consume: Consume {
            channel_hashes: vec![digest(byte)],
            hash: digest(byte + 1),
            persistent: false,
        },
        produces: vec![p.clone()],
        peeks: BTreeSet::new(),
        times_repeated: BTreeMap::from([(p, byte as i32)]),
    }
}

#[test]
fn comm_order_is_total_and_consistent_with_identity() {
    let mut fixtures: Vec<COMM> = (0..7).map(comm).collect();
    // Exercise fields after `consume`, rather than letting that first field
    // distinguish every row and leave the rest of the comparator untested.
    let baseline_consume = fixtures[0].consume.clone();
    for fixture in fixtures.iter_mut().skip(1) {
        fixture.consume = baseline_consume.clone();
    }
    fixtures[2].produces = vec![produce(90)];
    fixtures[3].peeks.insert(3);
    fixtures[4].times_repeated = BTreeMap::from([(produce(91), 4)]);
    fixtures[5].consume.persistent = true;
    fixtures[6] = fixtures[0].clone();

    for left in &fixtures {
        for right in &fixtures {
            assert_eq!(
                left.cmp(right).is_eq(),
                left == right,
                "COMM ordering and equality disagree"
            );
            assert_eq!(left.cmp(right), right.cmp(left).reverse());

            for third in &fixtures {
                if left <= right && right <= third {
                    assert!(left <= third, "COMM order is not transitive");
                }
            }
        }
    }
}

#[test]
fn both_replay_ingress_paths_use_the_shared_projection() {
    let source = include_str!("../src/rspace/replay_rspace.rs");
    assert_eq!(
        source.matches(".get_distinct_values_sorted(").count(),
        2,
        "consume and produce replay must both use the canonical multiset projection"
    );
    assert!(
        !source.contains(".map(|comms| {\n                comms\n                    .iter()"),
        "a raw Counter<COMM> projection has returned to replay"
    );
}
