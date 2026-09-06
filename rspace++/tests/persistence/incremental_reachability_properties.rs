use proptest::prelude::*;

type Graph = Vec<Vec<usize>>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Ledger {
    counts: Vec<usize>,
    active: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Cursor {
    parent: usize,
    remaining: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Activation,
    Deactivation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Machine {
    phase: Phase,
    roots: Vec<usize>,
    ledger: Ledger,
    events: Vec<usize>,
    cursors: Vec<Cursor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    PendingWork,
    UnauthorizedRootRemoval,
    ZeroCountDecrement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mutation {
    None,
    DropCursorEdge,
    RepeatCursorEdge,
    DeduplicateChildEdges,
    ExpandEveryIncrement,
    ExpandEveryDecrement,
    IncrementDecisionAfterUpdate,
    DecrementDecisionAfterUpdate,
}

fn root_counts(vertex_count: usize, roots: &[usize]) -> Vec<usize> {
    let mut counts = vec![0; vertex_count];
    for &root in roots {
        counts[root] += 1;
    }
    counts
}

fn cold_graph_oracle(graph: &Graph, roots: &[usize]) -> Ledger {
    let mut reached = vec![false; graph.len()];
    let mut frontier = roots
        .iter()
        .enumerate()
        .filter_map(|(vertex, count)| (*count > 0).then_some(vertex))
        .collect::<Vec<_>>();
    while let Some(vertex) = frontier.pop() {
        if reached[vertex] {
            continue;
        }
        reached[vertex] = true;
        frontier.extend_from_slice(&graph[vertex]);
    }
    let mut counts = roots.to_vec();
    for (parent, children) in graph.iter().enumerate() {
        if reached[parent] {
            for &child in children {
                counts[child] += 1;
            }
        }
    }
    Ledger {
        counts,
        active: reached,
    }
}

fn coherent(ledger: &Ledger) -> bool {
    ledger.counts.len() == ledger.active.len() &&
        ledger
            .counts
            .iter()
            .zip(&ledger.active)
            .all(|(count, active)| *active == (*count > 0))
}

fn incoming(graph: &Graph, ledger: &Ledger, query: usize) -> usize {
    graph
        .iter()
        .enumerate()
        .filter(|(parent, _)| ledger.active[*parent])
        .map(|(_, children)| children.iter().filter(|&&child| child == query).count())
        .sum()
}

fn pending(machine: &Machine, query: usize) -> usize {
    machine
        .events
        .iter()
        .filter(|&&vertex| vertex == query)
        .count() +
        machine
            .cursors
            .iter()
            .map(|cursor| {
                cursor
                    .remaining
                    .iter()
                    .filter(|&&child| child == query)
                    .count()
            })
            .sum::<usize>()
}

fn authenticated_suffix(graph: &Graph, cursor: &Cursor) -> bool {
    graph
        .get(cursor.parent)
        .is_some_and(|children| children.ends_with(&cursor.remaining))
}

fn valid_configuration(graph: &Graph, machine: &Machine) -> bool {
    let vertex_count = graph.len();
    if machine.roots.len() != vertex_count ||
        machine.ledger.counts.len() != vertex_count ||
        machine.ledger.active.len() != vertex_count ||
        !coherent(&machine.ledger) ||
        graph.iter().flatten().any(|&child| child >= vertex_count) ||
        machine.events.iter().any(|&vertex| vertex >= vertex_count) ||
        machine
            .cursors
            .iter()
            .any(|cursor| !authenticated_suffix(graph, cursor))
    {
        return false;
    }
    (0..vertex_count).all(|vertex| {
        let required = machine.roots[vertex] + incoming(graph, &machine.ledger, vertex);
        match machine.phase {
            Phase::Activation => {
                machine.ledger.counts[vertex] + pending(machine, vertex) == required
            }
            Phase::Deactivation => {
                machine.ledger.counts[vertex] == required + pending(machine, vertex)
            }
        }
    })
}

impl Machine {
    fn activate(ledger: &Ledger, roots: &[usize], added: &[usize]) -> Self {
        let mut augmented = roots.to_vec();
        for &vertex in added {
            augmented[vertex] += 1;
        }
        Self {
            phase: Phase::Activation,
            roots: augmented,
            ledger: ledger.clone(),
            events: added.to_vec(),
            cursors: Vec::new(),
        }
    }

    fn drained(&self) -> bool { self.events.is_empty() && self.cursors.is_empty() }

    fn begin_deactivation(&self, removed: &[usize]) -> Result<Self, Refusal> {
        if !self.drained() {
            return Err(Refusal::PendingWork);
        }
        if removed.iter().any(|&vertex| vertex >= self.roots.len()) {
            return Err(Refusal::UnauthorizedRootRemoval);
        }
        let demand = root_counts(self.roots.len(), removed);
        if demand
            .iter()
            .zip(&self.roots)
            .any(|(removed, roots)| removed > roots)
        {
            return Err(Refusal::UnauthorizedRootRemoval);
        }
        Ok(Self {
            phase: Phase::Deactivation,
            roots: self
                .roots
                .iter()
                .zip(demand)
                .map(|(roots, removed)| roots - removed)
                .collect(),
            ledger: self.ledger.clone(),
            events: removed.to_vec(),
            cursors: Vec::new(),
        })
    }

    fn step(
        &mut self,
        graph: &Graph,
        prefer_cursor: bool,
        mutation: Mutation,
    ) -> Result<bool, Refusal> {
        if !self.cursors.is_empty() && (prefer_cursor || self.events.is_empty()) {
            let cursor = self.cursors.last_mut().unwrap();
            if cursor.remaining.is_empty() {
                self.cursors.pop();
            } else {
                let child = cursor.remaining.remove(0);
                if mutation != Mutation::DropCursorEdge {
                    self.events.push(child);
                }
                if mutation == Mutation::RepeatCursorEdge {
                    self.events.push(child);
                }
            }
            return Ok(true);
        }
        let Some(&vertex) = self.events.last() else {
            return Ok(false);
        };
        let before = self.ledger.counts[vertex];
        let after = match self.phase {
            Phase::Activation => before + 1,
            Phase::Deactivation => before.checked_sub(1).ok_or(Refusal::ZeroCountDecrement)?,
        };
        let install_cursor = match self.phase {
            Phase::Activation => match mutation {
                Mutation::ExpandEveryIncrement => true,
                Mutation::IncrementDecisionAfterUpdate => after == 0,
                _ => before == 0,
            },
            Phase::Deactivation => match mutation {
                Mutation::ExpandEveryDecrement => true,
                Mutation::DecrementDecisionAfterUpdate => after == 1,
                _ => before == 1,
            },
        };
        self.events.pop();
        self.ledger.counts[vertex] = after;
        self.ledger.active[vertex] = after > 0;
        if install_cursor {
            let mut remaining = graph[vertex].clone();
            if mutation == Mutation::DeduplicateChildEdges {
                let mut distinct = Vec::new();
                for child in remaining {
                    if !distinct.contains(&child) {
                        distinct.push(child);
                    }
                }
                remaining = distinct;
            }
            self.cursors.push(Cursor {
                parent: vertex,
                remaining,
            });
        }
        Ok(true)
    }
}

fn drain(graph: &Graph, machine: &mut Machine, schedule: &[bool]) -> Vec<usize> {
    assert!(!schedule.is_empty());
    assert!(valid_configuration(graph, machine), "initial configuration: {machine:?}");
    let step_bound =
        machine.events.len() + 2 * graph.iter().map(Vec::len).sum::<usize>() + graph.len();
    let mut steps = 0;
    let mut sign_changes = vec![0; graph.len()];
    while !machine.drained() {
        assert!(steps < step_bound, "bounded-model work exceeded: {machine:?}");
        let before = machine.ledger.active.clone();
        assert!(
            machine
                .step(graph, schedule[steps % schedule.len()], Mutation::None)
                .unwrap()
        );
        assert!(valid_configuration(graph, machine), "after microstep {steps}: {machine:?}");
        for (vertex, was_active) in before.into_iter().enumerate() {
            sign_changes[vertex] += usize::from(was_active != machine.ledger.active[vertex]);
            assert!(sign_changes[vertex] <= 1);
        }
        steps += 1;
    }
    assert_eq!(machine.ledger, cold_graph_oracle(graph, &machine.roots));
    sign_changes
}

fn replace_roots(
    graph: &Graph,
    ledger: &Ledger,
    old: &[usize],
    target: &[usize],
    schedule: &[bool],
) -> Machine {
    let mut activation = Machine::activate(ledger, &root_counts(graph.len(), old), target);
    drain(graph, &mut activation, schedule);
    let mut deactivation = activation.begin_deactivation(old).unwrap();
    drain(graph, &mut deactivation, schedule);
    assert_eq!(deactivation.roots, root_counts(graph.len(), target));
    deactivation
}

fn dag_histories() -> impl Strategy<Value = (Graph, Vec<Vec<usize>>, Vec<bool>)> {
    (1usize..9).prop_flat_map(|vertex_count| {
        (
            prop::collection::vec(prop::collection::vec(any::<u8>(), 0..5), vertex_count),
            prop::collection::vec(prop::collection::vec(0usize..vertex_count, 0..7), 2..7),
            prop::collection::vec(any::<bool>(), 1..17),
        )
            .prop_map(|(raw_graph, history, schedule)| {
                let graph = raw_graph
                    .into_iter()
                    .enumerate()
                    .map(|(parent, raw_children)| {
                        if parent == 0 {
                            Vec::new()
                        } else {
                            raw_children
                                .into_iter()
                                .map(|child| usize::from(child) % parent)
                                .collect()
                        }
                    })
                    .collect();
                (graph, history, schedule)
            })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn cursor_execution_preserves_phase_invariants_and_matches_cold_dag(
        (graph, history, schedule) in dag_histories(),
    ) {
        prop_assert!(graph.iter().enumerate().all(|(parent, children)| {
            children.iter().all(|&child| child < parent)
        }), "generated edges must decrease rank");
        let alternate = schedule.iter().map(|choice| !choice).collect::<Vec<_>>();
        let mut roots = Vec::new();
        let mut ledger = cold_graph_oracle(&graph, &vec![0; graph.len()]);
        for target in history {
            let first = replace_roots(&graph, &ledger, &roots, &target, &schedule);
            let second = replace_roots(&graph, &ledger, &roots, &target, &alternate);
            prop_assert_eq!(&first.ledger, &second.ledger);
            prop_assert_eq!(&first.ledger, &cold_graph_oracle(&graph, &root_counts(graph.len(), &target)));
            ledger = first.ledger;
            roots = target;
        }
        let empty = replace_roots(&graph, &ledger, &roots, &[], &schedule);
        prop_assert!(empty.ledger.counts.iter().all(|&count| count == 0));
        prop_assert!(empty.ledger.active.iter().all(|active| !active));
    }
}

#[test]
fn add_before_remove_preserves_shared_roots_and_releases_last_references() {
    let graph = vec![vec![], vec![0, 0], vec![1], vec![1]];
    let old = [2, 2, 1];
    let target = [3, 3, 1];
    let roots = root_counts(graph.len(), &old);
    let predecessor = cold_graph_oracle(&graph, &roots);
    assert_eq!(predecessor.counts, [2, 2, 2, 0]);
    let mut activation = Machine::activate(&predecessor, &roots, &target);
    assert_eq!(drain(&graph, &mut activation, &[true, false]), [0, 0, 0, 1]);
    let mut deactivation = activation.begin_deactivation(&old).unwrap();
    assert_eq!(drain(&graph, &mut deactivation, &[false, true]), [0, 0, 1, 0]);
    assert_eq!(deactivation.ledger.counts, [2, 2, 0, 2]);
    assert_eq!(predecessor, cold_graph_oracle(&graph, &roots));
    let mut last_removal = deactivation.begin_deactivation(&target).unwrap();
    assert_eq!(drain(&graph, &mut last_removal, &[true]), [1, 1, 0, 1]);
    assert_eq!(last_removal.ledger.counts, [0, 0, 0, 0]);
}

fn assert_mutant_violates_conservation(graph: &Graph, mut machine: Machine, mutation: Mutation) {
    assert!(valid_configuration(graph, &machine));
    for step in 0..64 {
        assert!(machine.step(graph, step % 2 == 1, mutation).unwrap());
        if !valid_configuration(graph, &machine) {
            assert!(coherent(&machine.ledger));
            assert!(
                (0..graph.len()).any(|vertex| {
                    let required = machine.roots[vertex] + incoming(graph, &machine.ledger, vertex);
                    match machine.phase {
                        Phase::Activation => {
                            machine.ledger.counts[vertex] + pending(&machine, vertex) != required
                        }
                        Phase::Deactivation => {
                            machine.ledger.counts[vertex] != required + pending(&machine, vertex)
                        }
                    }
                }),
                "mutant changed only non-accounting validity: {mutation:?}"
            );
            return;
        }
        assert!(!machine.drained(), "mutant reached a valid terminal: {mutation:?}");
    }
    panic!("mutant escaped the bounded witness: {mutation:?}");
}

#[test]
fn cursor_edge_and_zero_crossing_mutants_violate_conservation() {
    let graph = vec![vec![], vec![0, 0]];
    let empty = cold_graph_oracle(&graph, &[0, 0]);
    let activation = Machine::activate(&empty, &[0, 0], &[1, 1]);
    for mutation in [
        Mutation::DropCursorEdge,
        Mutation::RepeatCursorEdge,
        Mutation::DeduplicateChildEdges,
        Mutation::ExpandEveryIncrement,
        Mutation::IncrementDecisionAfterUpdate,
    ] {
        assert_mutant_violates_conservation(&graph, activation.clone(), mutation);
    }
    for root_events in [vec![1], vec![1, 1]] {
        let roots = root_counts(graph.len(), &root_events);
        let settled = Machine::activate(&cold_graph_oracle(&graph, &roots), &roots, &[]);
        let deactivation = settled.begin_deactivation(&root_events).unwrap();
        assert_mutant_violates_conservation(
            &graph,
            deactivation.clone(),
            Mutation::DecrementDecisionAfterUpdate,
        );
        if root_events.len() == 2 {
            assert_mutant_violates_conservation(
                &graph,
                deactivation,
                Mutation::ExpandEveryDecrement,
            );
        }
    }
}

#[test]
fn repeated_root_events_cannot_be_deduplicated() {
    let graph = vec![vec![], vec![0, 0]];
    let empty = cold_graph_oracle(&graph, &[0, 0]);
    let mut activation = Machine::activate(&empty, &[0, 0], &[1, 1]);
    let mut deduplicated = activation.clone();
    deduplicated.events.dedup();
    assert!(coherent(&deduplicated.ledger));
    assert!(!valid_configuration(&graph, &deduplicated));
    drain(&graph, &mut activation, &[false]);
    assert_eq!(activation.ledger.counts, [2, 2]);
    let deactivation = activation.begin_deactivation(&[1, 1]).unwrap();
    let mut deduplicated = deactivation.clone();
    deduplicated.events.dedup();
    assert!(valid_configuration(&graph, &deactivation));
    assert!(!valid_configuration(&graph, &deduplicated));
}

#[test]
fn root_removal_requires_root_authority_not_incoming_counts() {
    let graph = vec![vec![], vec![0]];
    let roots = [0, 1];
    let machine = Machine::activate(&cold_graph_oracle(&graph, &roots), &roots, &[]);
    let before = machine.clone();
    assert_eq!(machine.ledger.counts[0], 1);
    assert_eq!(machine.roots[0], 0);
    assert_eq!(machine.begin_deactivation(&[0]), Err(Refusal::UnauthorizedRootRemoval));
    assert_eq!(machine.begin_deactivation(&[1, 1]), Err(Refusal::UnauthorizedRootRemoval));
    assert_eq!(machine, before);
    let mut saturating_root_removal = machine.clone();
    saturating_root_removal.phase = Phase::Deactivation;
    saturating_root_removal.roots[0] = saturating_root_removal.roots[0].saturating_sub(1);
    saturating_root_removal.events.push(0);
    assert!(!valid_configuration(&graph, &saturating_root_removal));
}

#[test]
fn zero_count_decrement_is_not_justified_by_coherence() {
    let graph = vec![vec![]];
    let mut machine = Machine {
        phase: Phase::Deactivation,
        roots: vec![0],
        ledger: cold_graph_oracle(&graph, &[0]),
        events: vec![0],
        cursors: Vec::new(),
    };
    let before = machine.clone();
    assert!(coherent(&machine.ledger));
    assert!(!valid_configuration(&graph, &machine));
    assert_eq!(machine.step(&graph, false, Mutation::None), Err(Refusal::ZeroCountDecrement));
    assert_eq!(machine, before);
    let mut saturated = before;
    saturated.events.pop();
    saturated.ledger.counts[0] = saturated.ledger.counts[0].saturating_sub(1);
    assert!(coherent(&saturated.ledger));
    assert!(valid_configuration(&graph, &saturated));
}

#[test]
fn phase_handoff_requires_both_events_and_cursors_drained() {
    let graph = vec![vec![], vec![0]];
    let mut machine = Machine::activate(&cold_graph_oracle(&graph, &[0, 0]), &[0, 0], &[1]);
    assert_eq!(machine.begin_deactivation(&[1]), Err(Refusal::PendingWork));
    machine.step(&graph, false, Mutation::None).unwrap();
    assert!(valid_configuration(&graph, &machine));
    assert!(machine.events.is_empty());
    assert_eq!(machine.cursors.last().unwrap().remaining, [0]);
    let before = machine.clone();
    assert_eq!(machine.begin_deactivation(&[1]), Err(Refusal::PendingWork));
    assert_eq!(machine, before);
    let mut premature = machine.clone();
    premature.phase = Phase::Deactivation;
    premature.roots[1] = 0;
    premature.events.push(1);
    assert!(!valid_configuration(&graph, &premature));
    machine.step(&graph, true, Mutation::None).unwrap();
    assert!(valid_configuration(&graph, &machine));
    machine.step(&graph, false, Mutation::None).unwrap();
    assert!(valid_configuration(&graph, &machine));
    assert!(machine.events.is_empty());
    assert!(
        machine
            .cursors
            .iter()
            .all(|cursor| cursor.remaining.is_empty())
    );
    assert!(!machine.drained());
    assert_eq!(machine.begin_deactivation(&[1]), Err(Refusal::PendingWork));
    while !machine.drained() {
        machine.step(&graph, true, Mutation::None).unwrap();
        assert!(valid_configuration(&graph, &machine));
    }
    assert!(machine.begin_deactivation(&[1]).is_ok());
}

#[test]
fn authenticated_suffix_does_not_prove_processed_history() {
    let graph = vec![vec![], vec![], vec![0, 1], vec![0, 1]];
    let mut machine = Machine::activate(&cold_graph_oracle(&graph, &[0; 4]), &[0; 4], &[2]);
    machine.step(&graph, false, Mutation::None).unwrap();
    assert!(valid_configuration(&graph, &machine));
    let mut skipped = machine.clone();
    skipped.cursors.last_mut().unwrap().remaining.remove(0);
    assert!(authenticated_suffix(&graph, skipped.cursors.last().unwrap()));
    assert!(!valid_configuration(&graph, &skipped));
    let mut invented = machine.clone();
    invented.cursors.last_mut().unwrap().remaining = vec![1, 0];
    assert!(!authenticated_suffix(&graph, invented.cursors.last().unwrap()));
    assert!(!valid_configuration(&graph, &invented));
    let mut substituted_parent = machine;
    substituted_parent.cursors.last_mut().unwrap().parent = 3;
    assert!(authenticated_suffix(&graph, substituted_parent.cursors.last().unwrap()));
    assert!(valid_configuration(&graph, &substituted_parent));
}

#[test]
fn rootless_cycle_witness_requires_acyclicity() {
    let graph = vec![vec![1], vec![0]];
    let unsupported = Machine {
        phase: Phase::Activation,
        roots: vec![0, 0],
        ledger: Ledger {
            counts: vec![1, 1],
            active: vec![true, true],
        },
        events: Vec::new(),
        cursors: Vec::new(),
    };
    assert!(valid_configuration(&graph, &unsupported));
    assert!(unsupported.drained());
    let cold = cold_graph_oracle(&graph, &unsupported.roots);
    assert_eq!(cold.counts, [0, 0]);
    assert_eq!(cold.active, [false, false]);
    assert_ne!(unsupported.ledger, cold);
}
