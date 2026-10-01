use super::{add_edge, ResidualEdge, ABSENT};

fn divergence(edges: &[ResidualEdge], vertices: usize) -> Vec<i128> {
    let mut result = vec![0_i128; vertices];
    for pair in edges.chunks_exact(2) {
        let flow = i128::from(pair[1].capacity);
        result[pair[1].to] += flow;
        result[pair[0].to] -= flow;
    }
    result
}

fn assert_adjacency_coverage(edges: &[ResidualEdge], heads: &[usize]) {
    assert_eq!(edges.len() % 2, 0);
    let mut expected = vec![std::collections::BTreeSet::new(); heads.len()];
    for index in 0..edges.len() {
        expected[edges[index ^ 1].to].insert(index);
    }
    for (owner, &head) in heads.iter().enumerate() {
        let mut observed = std::collections::BTreeSet::new();
        let mut cursor = head;
        while let Some(index) = super::advance_adjacency(edges, &mut cursor) {
            assert!(index < edges.len());
            assert!(
                observed.insert(index),
                "adjacency scan cannot revisit an edge"
            );
            assert_eq!(
                edges[index ^ 1].to,
                owner,
                "adjacency scan must stay with its owner"
            );
        }
        assert_eq!(
            observed, expected[owner],
            "adjacency scan must cover exactly its owned edges"
        );
    }
}

#[test]
#[should_panic(expected = "adjacency scan must cover exactly its owned edges")]
fn adjacency_coverage_detects_a_skipped_owned_edge() {
    let mut edges = Vec::new();
    let mut heads = vec![ABSENT; 3];
    add_edge(&mut edges, &mut heads, 0, 1, 1);
    add_edge(&mut edges, &mut heads, 0, 2, 1);
    heads[0] = edges[heads[0]].next;
    assert_adjacency_coverage(&edges, &heads);
}

#[test]
#[should_panic(expected = "adjacency scan must stay with its owner")]
fn adjacency_coverage_detects_another_vertexs_head() {
    let mut edges = Vec::new();
    let mut heads = vec![ABSENT; 2];
    add_edge(&mut edges, &mut heads, 0, 1, 1);
    heads[1] = heads[0];
    assert_adjacency_coverage(&edges, &heads);
}

pub(super) fn assert_unique_funding_pairs(edges: &[ResidualEdge]) {
    assert_eq!(edges.len() % 2, 0);
    let mut pairs = std::collections::BTreeSet::new();
    for pair in edges.chunks_exact(2) {
        assert!(
            pairs.insert((pair[1].to, pair[0].to)),
            "funding layout must not duplicate an original pair"
        );
    }
}

#[test]
#[should_panic(expected = "funding layout must not duplicate an original pair")]
fn funding_layout_detects_duplicate_assignment_pairs() {
    let mut edges = Vec::new();
    let mut heads = vec![ABSENT; 4];
    add_edge(&mut edges, &mut heads, 1, 2, 1);
    add_edge(&mut edges, &mut heads, 1, 2, 1);
    assert_unique_funding_pairs(&edges);
}

pub(super) fn assert_projection_identities(
    edges: &[ResidualEdge],
    sources: usize,
    obligations: usize,
) {
    let sink = sources + obligations + 1;
    let mut incoming = vec![0_i128; sources];
    let mut outgoing = vec![0_i128; obligations];
    let mut rows = vec![0_i128; sources];
    let mut columns = vec![0_i128; obligations];
    for pair in edges.chunks_exact(2) {
        let from = pair[1].to;
        let to = pair[0].to;
        let flow = i128::from(pair[1].capacity);
        if from == 0 && (1..=sources).contains(&to) {
            incoming[to - 1] += flow;
        } else if (1..=sources).contains(&from) && (sources + 1..sink).contains(&to) {
            rows[from - 1] += flow;
            columns[to - sources - 1] += flow;
        } else if (sources + 1..sink).contains(&from) && to == sink {
            outgoing[from - sources - 1] += flow;
        } else {
            panic!("funding projection requires a permitted edge kind");
        }
    }
    let actual = divergence(edges, sink + 1);
    assert_eq!(actual[0], incoming.iter().sum::<i128>());
    assert_eq!(actual[sink], -outgoing.iter().sum::<i128>());
    for source in 0..sources {
        assert_eq!(actual[source + 1], rows[source] - incoming[source]);
    }
    for obligation in 0..obligations {
        assert_eq!(
            actual[sources + 1 + obligation],
            outgoing[obligation] - columns[obligation]
        );
    }
}

#[test]
#[should_panic(expected = "funding projection requires a permitted edge kind")]
fn projection_rejects_a_direct_source_to_sink_edge() {
    let mut edges = Vec::new();
    let mut heads = vec![ABSENT; 4];
    add_edge(&mut edges, &mut heads, 0, 3, 1);
    assert_projection_identities(&edges, 1, 1);
}

pub(super) struct FundingProgress {
    initial: u64,
    previous: u64,
    total: u64,
    steps: u64,
}

impl FundingProgress {
    pub(super) fn new(initial: u64, total: u64) -> Self {
        assert!(initial <= total);
        Self {
            initial,
            previous: initial,
            total,
            steps: 0,
        }
    }

    pub(super) fn record(&mut self, funded: u64, amount: u64) {
        assert!(
            amount > 0,
            "augmentation must make positive funding progress"
        );
        assert_eq!(
            u128::from(funded),
            u128::from(self.previous) + u128::from(amount),
            "funding progress must equal the applied amount"
        );
        assert!(
            funded <= self.total,
            "funding must not exceed the obligation"
        );
        assert!(
            self.total - funded < self.total - self.previous,
            "remaining funding must strictly decrease"
        );
        self.steps = self
            .steps
            .checked_add(1)
            .expect("funding step count must fit");
        assert!(self.steps <= funded - self.initial);
        assert!(self.steps <= self.total - self.initial);
        self.previous = funded;
    }
}

#[test]
#[should_panic(expected = "augmentation must make positive funding progress")]
fn funding_progress_rejects_a_zero_update() { FundingProgress::new(0, 10).record(0, 0); }

#[test]
#[should_panic(expected = "funding progress must equal the applied amount")]
fn funding_progress_rejects_an_incorrect_counter() { FundingProgress::new(2, 10).record(5, 2); }

#[test]
#[should_panic(expected = "funding must not exceed the obligation")]
fn funding_progress_rejects_an_overfunded_obligation() { FundingProgress::new(2, 4).record(5, 3); }

pub(super) struct AugmentationSnapshot {
    capacities: Vec<u64>,
    divergence: Vec<i128>,
}

impl AugmentationSnapshot {
    pub(super) fn new(edges: &[ResidualEdge], parents: &[usize], sink: usize, amount: u64) -> Self {
        let mut capacities: Vec<_> = edges.iter().map(|edge| edge.capacity).collect();
        let mut pairs = std::collections::BTreeSet::new();
        let mut node = sink;
        while node != 0 {
            let index = parents[node];
            assert!(
                pairs.insert(index / 2),
                "a simple path cannot reuse a residual pair"
            );
            let forward = i128::from(capacities[index]) - i128::from(amount);
            let reverse = i128::from(capacities[index ^ 1]) + i128::from(amount);
            capacities[index] = u64::try_from(forward).expect("path debit must fit");
            capacities[index ^ 1] = u64::try_from(reverse).expect("path credit must fit");
            node = edges[index ^ 1].to;
        }
        let mut divergence = divergence(edges, parents.len());
        divergence[0] += i128::from(amount);
        divergence[sink] -= i128::from(amount);
        Self {
            capacities,
            divergence,
        }
    }

    pub(super) fn assert_completed(self, edges: &[ResidualEdge]) {
        assert_eq!(
            edges.iter().map(|edge| edge.capacity).collect::<Vec<_>>(),
            self.capacities,
            "completed augmentation must match every modeled residual capacity"
        );
        assert_eq!(
            divergence(edges, self.divergence.len()),
            self.divergence,
            "completed augmentation must preserve internal flow"
        );
    }
}

#[test]
#[should_panic(expected = "completed augmentation must match every modeled residual capacity")]
fn augmentation_detects_a_skipped_path_edge() {
    let (mut edges, _) = chain();
    let snapshot = AugmentationSnapshot::new(&edges, &[0, 0, 2], 2, 1);
    edges[2].capacity = 0;
    edges[3].capacity = 1;
    snapshot.assert_completed(&edges);
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(512))]

    #[test]
    fn positive_funding_histories_have_a_strictly_decreasing_remainder(
        total in proptest::num::u64::ANY,
        initial in proptest::num::u64::ANY,
        requests in proptest::collection::vec(proptest::num::u64::ANY, 0..257),
    ) {
        let initial = initial.min(total);
        let mut progress = FundingProgress::new(initial, total);
        let mut funded = initial;
        for request in requests {
            if funded == total {
                break;
            }
            let amount = request.max(1).min(total - funded);
            funded += amount;
            progress.record(funded, amount);
            proptest::prop_assert_eq!(progress.previous, funded);
            proptest::prop_assert!(progress.steps <= total - initial);
        }
    }

    #[test]
    fn reserved_discovery_queue_never_grows_its_allocation(
        priorities in proptest::collection::vec(proptest::num::u64::ANY, 1..257),
    ) {
        let vertices = priorities.len();
        let mut order: Vec<_> = priorities.iter().copied().enumerate().collect();
        order.sort_unstable_by_key(|&(node, priority)| (priority, node));
        let mut parents = vec![ABSENT; vertices];
        let mut queue = Vec::with_capacity(vertices);
        let storage = queue.as_ptr();
        let capacity = queue.capacity();
        for round in 0..3 {
            for &(node, priority) in &order {
                let edge = ResidualEdge {
                    to: node,
                    next: ABSENT,
                    capacity: if round == 0 && priority % 2 == 0 { 0 } else { 1 },
                };
                super::discover_parent(&edge, round * vertices + node, &mut parents, &mut queue);
                proptest::prop_assert!(queue.len() <= vertices);
                proptest::prop_assert!(queue.iter().all(|&vertex| vertex < vertices));
                proptest::prop_assert_eq!(queue.capacity(), capacity);
                proptest::prop_assert_eq!(queue.as_ptr(), storage);
            }
            if round > 0 {
                proptest::prop_assert_eq!(queue.len(), vertices);
                proptest::prop_assert!(parents.iter().all(|&parent| parent != ABSENT));
            }
        }
    }

    #[test]
    fn native_adjacency_walk_matches_ranked_chain_and_preserves_graph(
        seeds in proptest::collection::vec((proptest::num::usize::ANY, proptest::num::u64::ANY, proptest::bool::ANY), 1..129),
        start_seed in proptest::num::usize::ANY,
        empty in proptest::bool::ANY,
    ) {
        let edges: Vec<_> = seeds.iter().enumerate().map(|(index, &(next_seed, capacity, stop))| ResidualEdge {
            to: next_seed % seeds.len(),
            next: if index == 0 || stop { ABSENT } else { next_seed % index },
            capacity,
        }).collect();
        let initial = if empty { ABSENT } else { start_seed % edges.len() };
        let before: Vec<_> = edges.iter().map(|edge| (edge.to, edge.next, edge.capacity)).collect();
        let mut expected = Vec::new();
        let mut reference = initial;
        while reference != ABSENT {
            expected.push(reference);
            reference = before[reference].1;
        }
        let mut actual = Vec::new();
        let mut cursor = initial;
        while let Some(index) = super::advance_adjacency(&edges, &mut cursor) {
            proptest::prop_assert!(index < edges.len());
            proptest::prop_assert!(cursor == ABSENT || cursor < index);
            proptest::prop_assert!(!actual.contains(&index));
            actual.push(index);
        }
        proptest::prop_assert_eq!(&actual, &expected);
        proptest::prop_assert_eq!(cursor, ABSENT);
        if !empty {
            proptest::prop_assert!(actual.len() <= initial + 1);
        }
        for _ in 0..3 {
            proptest::prop_assert_eq!(super::advance_adjacency(&edges, &mut cursor), None);
            proptest::prop_assert_eq!(cursor, ABSENT);
        }
        proptest::prop_assert_eq!(edges.iter().map(|edge| (edge.to, edge.next, edge.capacity)).collect::<Vec<_>>(), before);
    }

    #[test]
    fn full_width_pair_indices_preserve_bounds_and_orientation(
        pairs in 1_usize..=usize::MAX / 2,
        raw_index in proptest::num::usize::ANY,
    ) {
        let length = pairs * 2;
        let index = raw_index % length;
        let partner = index ^ 1;
        proptest::prop_assert!(partner < length);
        proptest::prop_assert_eq!(partner ^ 1, index);
        proptest::prop_assert_ne!(partner, index);
        proptest::prop_assert_eq!(partner / 2, index / 2);
        proptest::prop_assert_eq!(partner, if index % 2 == 0 { index + 1 } else { index - 1 });
    }

    #[test]
    fn neighbor_scan_order_preserves_exact_discovery_membership(
        initial in proptest::collection::vec(proptest::bool::ANY, 1..65),
        source_seed in proptest::num::usize::ANY,
        neighbors in proptest::collection::vec((proptest::num::usize::ANY, proptest::num::u64::ANY, proptest::num::u64::ANY, proptest::bool::ANY), 0..129),
    ) {
        let vertices = initial.len();
        let source = source_seed % vertices;
        let mut known = initial;
        known[source] = true;
        let mut expected = known.clone();
        let mut edges = Vec::new();
        let mut heads = vec![ABSENT; vertices];
        let mut order = Vec::new();
        for (target_seed, amount, priority, disabled) in neighbors {
            let target = target_seed % vertices;
            let capacity = if disabled { 0 } else { amount };
            let index = edges.len();
            add_edge(&mut edges, &mut heads, source, target, capacity);
            order.push((priority, index));
            expected[target] |= capacity != 0;
        }
        let parents: Vec<_> = known.iter().enumerate().map(|(node, &seen)| if seen { node } else { ABSENT }).collect();
        let queue: Vec<_> = known.iter().enumerate().filter_map(|(node, &seen)| seen.then_some(node)).collect();
        let mut outcomes = Vec::new();
        for pass in 0..3 {
            if pass == 1 {
                order.reverse();
            } else if pass == 2 {
                order.sort_unstable();
            }
            let mut next_parents = parents.clone();
            let mut next_queue = queue.clone();
            for &(_, index) in &order {
                super::discover_parent(&edges[index], index, &mut next_parents, &mut next_queue);
            }
            let observed: Vec<_> = next_parents.iter().map(|&parent| parent != ABSENT).collect();
            proptest::prop_assert_eq!(&observed, &expected);
            proptest::prop_assert_eq!(&next_queue[..queue.len()], queue.as_slice());
            let unique: std::collections::BTreeSet<_> = next_queue.iter().copied().collect();
            proptest::prop_assert_eq!(unique.len(), next_queue.len());
            for node in 0..vertices {
                proptest::prop_assert_eq!(unique.contains(&node), expected[node]);
                if known[node] {
                    proptest::prop_assert_eq!(next_parents[node], parents[node]);
                } else if expected[node] {
                    let edge = next_parents[node];
                    proptest::prop_assert_eq!(edges[edge].to, node);
                    proptest::prop_assert_eq!(edges[edge ^ 1].to, source);
                    proptest::prop_assert!(edges[edge].capacity > 0);
                }
            }
            outcomes.push(observed);
        }
        proptest::prop_assert_eq!(&outcomes[0], &outcomes[1]);
        proptest::prop_assert_eq!(&outcomes[0], &outcomes[2]);
    }

    #[test]
    fn arbitrary_discovery_order_keeps_parent_paths_acyclic(
        vertices in 1_usize..65,
        root_seed in proptest::num::usize::ANY,
        operations in proptest::collection::vec((proptest::num::usize::ANY, proptest::num::usize::ANY, proptest::num::u64::ANY, proptest::bool::ANY, proptest::bool::ANY), 0..129),
    ) {
        let root = root_seed % vertices;
        let mut edges = Vec::new();
        let mut heads = vec![ABSENT; vertices];
        let mut parents = vec![ABSENT; vertices];
        parents[root] = 0;
        let mut queue = vec![root];
        for (source_seed, target_seed, capacity, disabled, reversed) in operations {
            let source = queue[source_seed % queue.len()];
            let target = target_seed % vertices;
            let capacity = if disabled { 0 } else { capacity };
            let index = edges.len() + usize::from(reversed);
            if reversed {
                add_edge(&mut edges, &mut heads, target, source, capacity);
                edges[index - 1].capacity = 0;
                edges[index].capacity = capacity;
            } else {
                add_edge(&mut edges, &mut heads, source, target, capacity);
            }
            let before = parents.clone();
            super::discover_parent(&edges[index], index, &mut parents, &mut queue);
            proptest::prop_assert_eq!(parents[root], 0);
            proptest::prop_assert_eq!(queue.first(), Some(&root));
            proptest::prop_assert!(queue.len() <= vertices);
            let mut rank = vec![ABSENT; vertices];
            for (position, &node) in queue.iter().enumerate() {
                proptest::prop_assert_eq!(rank[node], ABSENT);
                rank[node] = position;
            }
            for node in 0..vertices {
                proptest::prop_assert_eq!(parents[node] != ABSENT, rank[node] != ABSENT);
                if before[node] != ABSENT {
                    proptest::prop_assert_eq!(parents[node], before[node]);
                }
                if node == root || parents[node] == ABSENT {
                    continue;
                }
                let edge = parents[node];
                proptest::prop_assert_eq!(edges[edge].to, node);
                proptest::prop_assert!(edges[edge].capacity > 0);
                proptest::prop_assert!(rank[edges[edge ^ 1].to] < rank[node]);
                let mut path = std::collections::BTreeSet::new();
                let mut pairs = std::collections::BTreeSet::new();
                let mut current = node;
                while current != root {
                    proptest::prop_assert!(path.insert(current));
                    let operation = parents[current];
                    proptest::prop_assert!(operation < edges.len());
                    proptest::prop_assert!(pairs.insert(operation / 2));
                    proptest::prop_assert!(edges[operation].capacity > 0);
                    proptest::prop_assert_eq!(edges[operation].to, current);
                    current = edges[operation ^ 1].to;
                }
                proptest::prop_assert!(path.len() <= rank[node]);
                proptest::prop_assert!(pairs.len() < queue.len());
            }
        }
    }

    #[test]
    fn discovery_histories_preserve_parents_and_queue_membership(
        known in proptest::collection::vec(proptest::bool::ANY, 1..65),
        operations in proptest::collection::vec((proptest::num::usize::ANY, proptest::num::u64::ANY, proptest::num::usize::ANY), 0..129),
        reverse_initial_queue in proptest::bool::ANY,
    ) {
        let mut parents: Vec<_> = known.iter().enumerate().map(|(node, known)| if *known { node } else { ABSENT }).collect();
        let mut queue: Vec<_> = known.iter().enumerate().filter_map(|(node, known)| known.then_some(node)).collect();
        if reverse_initial_queue {
            queue.reverse();
        }
        for (raw_target, capacity, raw_index) in operations {
            let target = raw_target % parents.len();
            let index = raw_index % usize::MAX;
            let edge = ResidualEdge { to: target, next: ABSENT, capacity };
            let before_parents = parents.clone();
            let before_queue = queue.clone();
            super::discover_parent(&edge, index, &mut parents, &mut queue);
            if capacity != 0 && before_parents[target] == ABSENT {
                proptest::prop_assert_eq!(parents[target], index);
                proptest::prop_assert_eq!(&queue[..before_queue.len()], before_queue.as_slice());
                proptest::prop_assert_eq!(queue.last(), Some(&target));
                proptest::prop_assert_eq!(queue.len(), before_queue.len() + 1);
                for (node, previous) in before_parents.iter().enumerate() {
                    if node != target {
                        proptest::prop_assert_eq!(parents[node], *previous);
                    }
                }
            } else {
                proptest::prop_assert_eq!(&parents, &before_parents);
                proptest::prop_assert_eq!(&queue, &before_queue);
            }
            let members: std::collections::BTreeSet<_> = queue.iter().copied().collect();
            proptest::prop_assert_eq!(members.len(), queue.len());
            proptest::prop_assert!(queue.len() <= parents.len());
            for (node, parent) in parents.iter().enumerate() {
                proptest::prop_assert_eq!(*parent != ABSENT, members.contains(&node));
            }
        }
    }

    #[test]
    fn paired_construction_preserves_every_existing_edge_and_adjacency(
        vertices in 1_usize..65,
        operations in proptest::collection::vec((proptest::num::usize::ANY, proptest::num::usize::ANY, proptest::num::u64::ANY), 0..129),
    ) {
        let mut edges = Vec::new();
        let mut heads = vec![ABSENT; vertices];
        let mut owners = Vec::new();
        for (raw_from, raw_to, capacity) in operations {
            let from = raw_from % vertices;
            let to = raw_to % vertices;
            let before: Vec<_> = edges.iter().map(|edge: &ResidualEdge| (edge.to, edge.next, edge.capacity)).collect();
            let before_heads = heads.clone();
            let forward = edges.len();
            add_edge(&mut edges, &mut heads, from, to, capacity);
            owners.extend([from, to]);
            proptest::prop_assert_eq!(edges.len(), forward + 2);
            proptest::prop_assert_eq!(edges[..forward].iter().map(|edge| (edge.to, edge.next, edge.capacity)).collect::<Vec<_>>(), before);
            proptest::prop_assert_eq!((edges[forward].to, edges[forward].next, edges[forward].capacity), (to, before_heads[from], capacity));
            proptest::prop_assert_eq!((edges[forward + 1].to, edges[forward + 1].next, edges[forward + 1].capacity), (from, if from == to { forward } else { before_heads[to] }, 0));
            proptest::prop_assert_eq!(heads[to], forward + 1);
            if from != to {
                proptest::prop_assert_eq!(heads[from], forward);
            }
            for (node, head) in heads.iter().enumerate() {
                proptest::prop_assert!(*head == ABSENT || *head < edges.len());
                if node != from && node != to {
                    proptest::prop_assert_eq!(*head, before_heads[node]);
                }
            }
            for (index, edge) in edges.iter().enumerate() {
                proptest::prop_assert!(edge.to < vertices);
                proptest::prop_assert!(edge.next == ABSENT || edge.next < index);
                proptest::prop_assert!((index ^ 1) < edges.len());
                proptest::prop_assert_eq!(edges[index ^ 1].to, owners[index]);
            }
            assert_adjacency_coverage(&edges, &heads);
        }
    }

    #[test]
    fn arbitrary_sparse_flows_satisfy_projection_identities(
        sources in 1_usize..9,
        obligations in 0_usize..9,
        seeds in proptest::collection::vec((proptest::num::u64::ANY, proptest::num::u64::ANY), 1..65),
    ) {
        let sink = sources + obligations + 1;
        let mut edges = Vec::new();
        let mut heads = vec![ABSENT; sink + 1];
        for source in 0..sources {
            add_edge(&mut edges, &mut heads, 0, source + 1, 0);
        }
        for source in 0..sources {
            for obligation in 0..obligations {
                if seeds[(source * obligations + obligation) % seeds.len()].0 & 1 == 0 {
                    add_edge(&mut edges, &mut heads, source + 1, sources + 1 + obligation, 0);
                }
            }
        }
        for obligation in 0..obligations {
            add_edge(&mut edges, &mut heads, sources + 1 + obligation, sink, 0);
        }
        for (index, pair) in edges.chunks_exact_mut(2).enumerate() {
            let (capacity, flow_seed) = seeds[index % seeds.len()];
            let flow = flow_seed.min(capacity);
            pair[0].capacity = capacity - flow;
            pair[1].capacity = flow;
        }
        assert_projection_identities(&edges, sources, obligations);
        let reordered: Vec<_> = edges.rchunks_exact(2).flat_map(|pair| {
            pair.iter().map(|edge| ResidualEdge { to: edge.to, next: ABSENT, capacity: edge.capacity })
        }).collect();
        assert_projection_identities(&reordered, sources, obligations);
        proptest::prop_assert_eq!(divergence(&edges, sink + 1), divergence(&reordered, sink + 1));
    }

    #[test]
    fn arbitrary_network_history_preserves_capacity_and_divergence(
        initial in proptest::collection::vec((proptest::num::u64::ANY, proptest::num::u64::ANY), 1..33),
        operations in proptest::collection::vec((proptest::num::usize::ANY, proptest::bool::ANY, proptest::num::u64::ANY), 0..129),
    ) {
        let vertices = initial.len() + 1;
        let mut edges = Vec::new();
        let mut heads = vec![ABSENT; vertices];
        for (index, &(capacity, reverse_seed)) in initial.iter().enumerate() {
            add_edge(&mut edges, &mut heads, index, index + 1, capacity);
            let reverse = reverse_seed.min(capacity);
            edges[2 * index].capacity -= reverse;
            edges[2 * index + 1].capacity = reverse;
        }
        let mut expected_divergence = divergence(&edges, vertices);
        for (selected, reversed, amount) in operations {
            let pair = selected % initial.len();
            let index = 2 * pair + usize::from(reversed);
            let before: Vec<_> = edges.iter().map(|edge| edge.capacity).collect();
            let result = super::augment_edge(&mut edges, index, amount);
            if let Some(predecessor) = result {
                proptest::prop_assert!(amount <= before[index]);
                proptest::prop_assert_eq!(predecessor, edges[index ^ 1].to);
                expected_divergence[edges[index ^ 1].to] += i128::from(amount);
                expected_divergence[edges[index].to] -= i128::from(amount);
                proptest::prop_assert_eq!(i128::from(edges[index].capacity), i128::from(before[index]) - i128::from(amount));
                proptest::prop_assert_eq!(i128::from(edges[index ^ 1].capacity), i128::from(before[index ^ 1]) + i128::from(amount));
            } else {
                proptest::prop_assert!(amount > before[index]);
            }
            for (other, edge) in edges.iter().enumerate() {
                if other / 2 != pair || result.is_none() {
                    proptest::prop_assert_eq!(edge.capacity, before[other]);
                }
            }
            for (other, original) in initial.iter().enumerate() {
                proptest::prop_assert_eq!(u128::from(edges[2 * other].capacity) + u128::from(edges[2 * other + 1].capacity), u128::from(original.0));
            }
            proptest::prop_assert_eq!(&divergence(&edges, vertices), &expected_divergence);
        }
    }

    #[test]
    fn native_augmentation_matches_exact_contract_for_arbitrary_machine_state(
        values in proptest::collection::vec((proptest::num::u64::ANY, proptest::num::usize::ANY, proptest::num::usize::ANY), 1..65),
        selected in proptest::num::usize::ANY,
        amount in proptest::num::u64::ANY,
    ) {
        let mut edges: Vec<_> = values.iter().chain(values.iter()).map(|&(capacity, to, next)| ResidualEdge { capacity, to, next }).collect();
        let index = selected % edges.len();
        let before: Vec<_> = edges.iter().map(|edge| (edge.to, edge.next, edge.capacity)).collect();
        let forward = i128::from(before[index].2) - i128::from(amount);
        let reverse = i128::from(before[index ^ 1].2) + i128::from(amount);
        let valid = forward >= 0 && reverse <= i128::from(u64::MAX);
        let result = super::augment_edge(&mut edges, index, amount);
        proptest::prop_assert_eq!(result, if valid { Some(before[index ^ 1].0) } else { None });
        for (other, edge) in edges.iter().enumerate() {
            proptest::prop_assert_eq!((edge.to, edge.next), (before[other].0, before[other].1));
            if valid && other == index {
                proptest::prop_assert_eq!(i128::from(edge.capacity), forward);
            } else if valid && other == (index ^ 1) {
                proptest::prop_assert_eq!(i128::from(edge.capacity), reverse);
            } else {
                proptest::prop_assert_eq!(edge.capacity, before[other].2);
            }
        }
    }
}

pub(super) fn assert_graph_relation(
    capacities: &[u64],
    obligations: &[u64],
    eligible: &[Vec<bool>],
    assignment: &[Vec<u64>],
    edges: &[ResidualEdge],
) {
    let sources = capacities.len();
    let total: u128 = obligations.iter().copied().map(u128::from).sum();
    let sink = sources + obligations.len() + 1;
    let mut expected = std::collections::BTreeSet::new();
    let mut columns = vec![0_u128; obligations.len()];
    for (source, row) in assignment.iter().enumerate() {
        let draw: u128 = row.iter().copied().map(u128::from).sum();
        if draw < u128::from(capacities[source]).min(total) {
            expected.insert((0, source + 1));
        }
        if draw > 0 {
            expected.insert((source + 1, 0));
        }
        for (obligation, amount) in row.iter().enumerate() {
            let vertex = sources + 1 + obligation;
            if eligible[source][obligation] && u128::from(*amount) < total {
                expected.insert((source + 1, vertex));
            }
            if *amount > 0 {
                expected.insert((vertex, source + 1));
            }
            columns[obligation] += u128::from(*amount);
        }
    }
    for (obligation, draw) in columns.iter().enumerate() {
        let vertex = sources + 1 + obligation;
        if *draw < u128::from(obligations[obligation]) {
            expected.insert((vertex, sink));
        }
        if *draw > 0 {
            expected.insert((sink, vertex));
        }
    }
    let actual: std::collections::BTreeSet<_> = edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.capacity > 0)
        .map(|(index, edge)| (edges[index ^ 1].to, edge.to))
        .collect();
    assert_eq!(
        actual, expected,
        "native residual edges must match the funding graph model"
    );
}

pub(super) struct SearchStepSnapshot {
    queue: Vec<usize>,
    cursor: usize,
}

impl SearchStepSnapshot {
    pub(super) fn new(queue: &[usize], cursor: usize) -> Self {
        assert!(cursor < queue.len());
        assert!(!queue[..cursor].contains(&queue[cursor]));
        Self {
            queue: queue.to_vec(),
            cursor,
        }
    }

    pub(super) fn assert_completed(self, queue: &[usize], cursor: usize) {
        assert_eq!(
            cursor,
            self.cursor + 1,
            "one completed scan must advance exactly one queue position"
        );
        assert!(
            queue.starts_with(&self.queue),
            "discovery must preserve existing queue positions"
        );
        assert_eq!(&queue[..cursor], &self.queue[..self.cursor + 1]);
    }
}

#[test]
#[should_panic(expected = "one completed scan must advance exactly one queue position")]
fn queue_step_detects_a_skipped_cursor() {
    SearchStepSnapshot::new(&[0, 1], 0).assert_completed(&[0, 1], 2);
}

#[test]
#[should_panic(expected = "discovery must preserve existing queue positions")]
fn queue_step_detects_a_rewritten_prefix() {
    SearchStepSnapshot::new(&[0, 1], 0).assert_completed(&[1, 0], 1);
}

pub(super) fn assert_search_state(
    edges: &[ResidualEdge],
    heads: &[usize],
    parents: &[usize],
    queue: &[usize],
    cursor: usize,
) {
    let vertices = parents.len();
    assert_eq!(heads.len(), vertices);
    assert_eq!(queue.first(), Some(&0));
    assert!(cursor <= queue.len() && queue.len() <= vertices);
    let mut positions = vec![ABSENT; vertices];
    let mut distances = vec![ABSENT; vertices];
    distances[0] = 0;
    for (position, node) in queue.iter().copied().enumerate() {
        assert_eq!(positions[node], ABSENT, "queue vertices must be unique");
        positions[node] = position;
        assert_ne!(parents[node], ABSENT);
        if node != 0 {
            let index = parents[node];
            assert_eq!(edges[index].to, node);
            assert!(edges[index].capacity > 0);
            let predecessor = edges[index ^ 1].to;
            assert!(
                positions[predecessor] < position,
                "each parent must precede its child"
            );
            distances[node] = distances[predecessor] + 1;
        }
        if position > 0 {
            assert!(
                distances[queue[position - 1]] <= distances[node],
                "queue distances must be nondecreasing"
            );
        }
    }
    for (node, parent) in parents.iter().enumerate() {
        assert_eq!(*parent != ABSENT, positions[node] != ABSENT);
    }
    for &node in &queue[..cursor] {
        let mut index = heads[node];
        while index != ABSENT {
            let edge = &edges[index];
            if edge.capacity > 0 {
                assert_ne!(
                    positions[edge.to], ABSENT,
                    "processed vertices must discover every residual neighbor"
                );
                assert!(distances[edge.to] <= distances[node] + 1);
            }
            index = edge.next;
        }
    }
    if cursor == queue.len() || parents[vertices - 1] != ABSENT {
        let mut expected = vec![ABSENT; vertices];
        expected[0] = 0;
        for _ in 0..vertices {
            let before = expected.clone();
            for (index, edge) in edges.iter().enumerate() {
                let from = edges[index ^ 1].to;
                if edge.capacity != 0 && before[from] != ABSENT {
                    expected[edge.to] = expected[edge.to].min(before[from] + 1);
                }
            }
            if before == expected {
                break;
            }
        }
        if cursor == queue.len() {
            assert_eq!(
                distances, expected,
                "exhausted search must match independent distance relaxation"
            );
        } else {
            for &node in queue {
                assert_eq!(
                    distances[node], expected[node],
                    "known vertices must have shortest discovered distances"
                );
            }
        }
    }
}

fn chain() -> (Vec<ResidualEdge>, Vec<usize>) {
    let mut edges = Vec::new();
    let mut heads = vec![ABSENT; 3];
    add_edge(&mut edges, &mut heads, 0, 1, 1);
    add_edge(&mut edges, &mut heads, 1, 2, 1);
    (edges, heads)
}

#[test]
#[should_panic(expected = "known vertices must have shortest discovered distances")]
fn early_success_detects_a_non_shortest_parent_path() {
    let mut edges = Vec::new();
    let mut heads = vec![ABSENT; 5];
    add_edge(&mut edges, &mut heads, 0, 1, 1);
    add_edge(&mut edges, &mut heads, 0, 2, 1);
    add_edge(&mut edges, &mut heads, 1, 3, 1);
    add_edge(&mut edges, &mut heads, 3, 4, 1);
    add_edge(&mut edges, &mut heads, 2, 4, 1);
    assert_search_state(&edges, &heads, &[0, 0, 2, 4, 6], &[0, 1, 2, 3, 4], 2);
}

#[test]
#[should_panic(expected = "queue vertices must be unique")]
fn queue_invariant_detects_duplicate_discovery() {
    let (edges, heads) = chain();
    assert_search_state(&edges, &heads, &[0, ABSENT, ABSENT], &[0, 0], 0);
}

#[test]
#[should_panic(expected = "processed vertices must discover every residual neighbor")]
fn queue_invariant_detects_incomplete_neighbor_scan() {
    let (edges, heads) = chain();
    assert_search_state(&edges, &heads, &[0, ABSENT, ABSENT], &[0], 1);
}

#[test]
#[should_panic(expected = "native residual edges must match the funding graph model")]
fn graph_relation_detects_an_edge_that_bypasses_a_payer() {
    let mut edges = Vec::new();
    let mut heads = vec![ABSENT; 4];
    add_edge(&mut edges, &mut heads, 0, 2, 1);
    add_edge(&mut edges, &mut heads, 2, 3, 1);
    assert_graph_relation(&[1], &[1], &[vec![true]], &[vec![0]], &edges);
}
