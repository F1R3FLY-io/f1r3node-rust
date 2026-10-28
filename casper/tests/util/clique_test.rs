// See casper/src/test/scala/coop/rchain/casper/util/CliqueTest.scala

use std::collections::HashMap;

use casper::rust::util::clique::Clique;

fn g1() -> Vec<(i32, i32)> {
    vec![
        (1, 6),
        (1, 2),
        (1, 3),
        (2, 6),
        (2, 4),
        (2, 3),
        (3, 6),
        (4, 6),
        (4, 7),
        (4, 5),
        (5, 7),
        (8, 9),
        (10, 11),
    ]
}

fn g2() -> Vec<(i32, i32)> {
    vec![
        (1, 2),
        (1, 4),
        (1, 5),
        (1, 6),
        (2, 3),
        (3, 4),
        (3, 6),
        (4, 5),
        (4, 6),
        (5, 6),
    ]
}

#[test]
fn empty_graph_returns_zero() {
    let h: Vec<(i32, i32)> = vec![];
    let weights: HashMap<i32, num_bigint::BigInt> = HashMap::new();
    assert_eq!(
        Clique::find_maximum_clique_by_weight(&h, &weights),
        0.into()
    );
}

#[test]
fn g1_max_clique_has_4_nodes() {
    // g1 has cliques: {1,2,3,6} (size 4), {2,4,6} (size 3), {4,5,7} (size 3), {8,9}, {10,11}
    // With equal weights, max clique weight = 4
    let weights: HashMap<i32, num_bigint::BigInt> = (1..=11).map(|i| (i, 1.into())).collect();
    assert_eq!(
        Clique::find_maximum_clique_by_weight(&g1(), &weights),
        4.into()
    );
}

#[test]
fn g1_with_self_loops_same_result() {
    let mut g3 = vec![(1, 1)];
    g3.extend(g1().iter().cloned());
    let weights: HashMap<i32, num_bigint::BigInt> = (1..=11).map(|i| (i, 1.into())).collect();
    assert_eq!(
        Clique::find_maximum_clique_by_weight(&g3, &weights),
        4.into()
    );
}

#[test]
fn g2_max_clique_has_4_nodes_with_equal_weights() {
    // g2 has cliques: {1,4,5,6} (size 4), {3,4,6} (size 3), {1,2}, {2,3}
    let weights = HashMap::from([
        (1, 1.into()),
        (2, 1.into()),
        (3, 1.into()),
        (4, 1.into()),
        (5, 1.into()),
        (6, 1.into()),
    ]);
    assert_eq!(
        Clique::find_maximum_clique_by_weight(&g2(), &weights),
        4.into()
    );
}

#[test]
fn g2_max_weight_beats_max_size() {
    // {1,2} has weight 20, {1,4,5,6} has weight 13 — weight wins over size
    let weights = HashMap::from([
        (1, 10.into()),
        (2, 10.into()),
        (3, 1.into()),
        (4, 1.into()),
        (5, 1.into()),
        (6, 1.into()),
    ]);
    assert_eq!(
        Clique::find_maximum_clique_by_weight(&g2(), &weights),
        20.into()
    );
}

#[test]
fn single_edge_returns_sum_of_weights() {
    let edges = vec![(1, 2)];
    let weights = HashMap::from([(1, 5.into()), (2, 3.into())]);
    assert_eq!(
        Clique::find_maximum_clique_by_weight(&edges, &weights),
        8.into()
    );
}

#[test]
fn disconnected_nodes_returns_max_single_weight() {
    // No edges — each node is its own clique of size 1
    let edges: Vec<(i32, i32)> = vec![];
    let weights = HashMap::from([(1, 5.into()), (2, 3.into())]);
    // No edges means no cliques found, but baseline is max single weight
    assert_eq!(
        Clique::find_maximum_clique_by_weight(&edges, &weights),
        5.into()
    );
}

#[test]
fn complete_graph_returns_total_weight() {
    // K4: every pair connected — one clique of all 4 nodes
    let edges = vec![(1, 2), (1, 3), (1, 4), (2, 3), (2, 4), (3, 4)];
    let weights = HashMap::from([
        (1, 10.into()),
        (2, 20.into()),
        (3, 30.into()),
        (4, 40.into()),
    ]);
    assert_eq!(
        Clique::find_maximum_clique_by_weight(&edges, &weights),
        100.into()
    );
}

#[test]
fn clique_and_finality_preserve_512_bit_weights_and_exact_boundaries() {
    use casper::rust::safety::clique_oracle::ft_decides_exact;
    use num_bigint::BigInt;
    let unit: BigInt = BigInt::from(1) << 512;
    let weights = HashMap::from([(1, &unit * 3), (2, &unit * 4), (3, &unit * 5)]);
    assert_eq!(
        Clique::find_maximum_clique_by_weight(&[(1, 2), (2, 3)], &weights),
        &unit * 9
    );
    for scale in [BigInt::from(1), unit] {
        let total: BigInt = &scale * 10;
        let clique: BigInt = &scale * 6;
        assert!(ft_decides_exact(
            total.clone(),
            clique.clone(),
            total.clone(),
            200_000,
            1_000_000,
            false
        ));
        assert!(!ft_decides_exact(
            total.clone(),
            clique.clone(),
            total.clone(),
            200_000,
            1_000_000,
            true
        ));
        assert!(!ft_decides_exact(
            total.clone(),
            &clique - 1,
            total.clone(),
            200_000,
            1_000_000,
            false
        ));
        assert!(ft_decides_exact(
            total.clone(),
            &clique + 1,
            total.clone(),
            200_000,
            1_000_000,
            true
        ));
        assert!(!ft_decides_exact(
            &scale * 5,
            total.clone(),
            total,
            0,
            1_000_000,
            false
        ));
    }
}
