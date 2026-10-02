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

#[derive(Clone, Debug, PartialEq, Eq)]
struct CheckedInventory {
    counts: [u64; 2],
    background_objects: u64,
    background_bytes: u64,
    objects: u64,
    bytes: u64,
}

fn checked_inventory_event(
    before: &CheckedInventory,
    vertex: usize,
    weight: u64,
    add: bool,
) -> Option<CheckedInventory> {
    let mut next = before.clone();
    let count = next.counts[vertex];
    let updated = if add {
        count.checked_add(1)?
    } else {
        count.checked_sub(1)?
    };
    if count == 0 {
        next.objects = next.objects.checked_add(1)?;
        next.bytes = next.bytes.checked_add(weight)?;
    } else if updated == 0 {
        next.objects = next.objects.checked_sub(1)?;
        next.bytes = next.bytes.checked_sub(weight)?;
    }
    next.counts[vertex] = updated;
    Some(next)
}

fn natural_inventory_event(
    before: &CheckedInventory,
    vertex: usize,
    weights: [u64; 2],
    add: bool,
) -> Option<CheckedInventory> {
    let mut counts = before.counts.map(u128::from);
    if add {
        counts[vertex] += 1;
    } else if counts[vertex] == 0 {
        return None;
    } else {
        counts[vertex] -= 1;
    }
    let objects = u128::from(before.background_objects) +
        counts.iter().filter(|&&count| count > 0).count() as u128;
    let bytes = u128::from(before.background_bytes) +
        counts
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(index, _)| weights[index] as u128)
            .sum::<u128>();
    if counts.iter().any(|&count| count > u64::MAX as u128) ||
        objects > u64::MAX as u128 ||
        bytes > u64::MAX as u128
    {
        return None;
    }
    Some(CheckedInventory {
        counts: counts.map(|count| count as u64),
        background_objects: before.background_objects,
        background_bytes: before.background_bytes,
        objects: objects as u64,
        bytes: bytes as u64,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn checked_reference_and_unique_inventory_refine_natural_arithmetic(
        initial in prop_oneof![Just(0), Just(1), Just(u64::MAX - 1), Just(u64::MAX), any::<u64>()],
        generated_background_objects in prop_oneof![Just(0), Just(u64::MAX - 1), Just(u64::MAX), any::<u64>()],
        generated_background_bytes in prop_oneof![Just(0), Just(u64::MAX - 1), Just(u64::MAX), any::<u64>()],
        first_weight in prop_oneof![Just(1), Just(u64::MAX - 1), Just(u64::MAX), any::<u64>().prop_map(|value| value.max(1))],
        second_weight in prop_oneof![Just(1), Just(u64::MAX - 1), Just(u64::MAX), any::<u64>().prop_map(|value| value.max(1))],
    ) {
        let weights = [first_weight, second_weight];
        let background_objects = generated_background_objects.min(u64::MAX - u64::from(initial > 0));
        let background_bytes = generated_background_bytes.min(
            u64::MAX - if initial > 0 { first_weight } else { 0 }
        );
        let mut state = CheckedInventory {
            counts: [initial, 0],
            background_objects,
            background_bytes,
            objects: background_objects + u64::from(initial > 0),
            bytes: background_bytes + if initial > 0 { first_weight } else { 0 },
        };
        for (vertex, add) in [(0, true), (1, true), (1, true), (0, false),
            (1, false), (1, false), (0, false)]
        {
            let before = state.clone();
            let actual = checked_inventory_event(&before, vertex, weights[vertex], add);
            let expected = natural_inventory_event(&before, vertex, weights, add);
            prop_assert_eq!(&actual, &expected);
            state = actual.unwrap_or(before);
            prop_assert_eq!(
                u128::from(state.objects),
                u128::from(state.background_objects)
                    + state.counts.iter().filter(|&&count| count > 0).count() as u128
            );
            prop_assert_eq!(
                state.bytes as u128,
                u128::from(state.background_bytes)
                    + state.counts.iter().enumerate()
                    .filter(|(_, count)| **count > 0)
                    .map(|(index, _)| weights[index] as u128)
                    .sum::<u128>()
            );
        }
    }
}

#[test]
fn checked_inventory_boundary_controls_preserve_predecessor() {
    let empty = CheckedInventory {
        counts: [0, 0],
        background_objects: 0,
        background_bytes: 0,
        objects: 0,
        bytes: 0,
    };
    assert_eq!(checked_inventory_event(&empty, 0, 1, false), None);
    let maximal = checked_inventory_event(&empty, 0, u64::MAX, true).unwrap();
    assert_eq!(maximal, CheckedInventory {
        counts: [1, 0],
        background_objects: 0,
        background_bytes: 0,
        objects: 1,
        bytes: u64::MAX
    });
    assert_eq!(checked_inventory_event(&maximal, 1, 1, true), None);
    assert_eq!(maximal.bytes, u64::MAX);
    let duplicated = checked_inventory_event(&maximal, 0, u64::MAX, true).unwrap();
    assert_eq!(duplicated, CheckedInventory {
        counts: [2, 0],
        background_objects: 0,
        background_bytes: 0,
        objects: 1,
        bytes: u64::MAX
    });
    let remaining = checked_inventory_event(&duplicated, 0, u64::MAX, false).unwrap();
    assert_eq!(remaining, maximal);
    assert_eq!(checked_inventory_event(&remaining, 0, u64::MAX, false), Some(empty));
    let almost_full = CheckedInventory {
        counts: [u64::MAX - 1, 0],
        background_objects: 0,
        background_bytes: 0,
        objects: 1,
        bytes: 1,
    };
    let full = checked_inventory_event(&almost_full, 0, 1, true).unwrap();
    assert_eq!(full.counts[0], u64::MAX);
    assert_eq!(checked_inventory_event(&full, 0, 1, true), None);
    assert_eq!(full, CheckedInventory {
        counts: [u64::MAX, 0],
        background_objects: 0,
        background_bytes: 0,
        objects: 1,
        bytes: 1
    });
    let object_almost_full = CheckedInventory {
        counts: [0, 0],
        background_objects: u64::MAX - 1,
        background_bytes: 0,
        objects: u64::MAX - 1,
        bytes: 0,
    };
    let object_full = checked_inventory_event(&object_almost_full, 0, 0, true).unwrap();
    assert_eq!(object_full.objects, u64::MAX);
    assert_eq!(object_full.bytes, 0);
    assert_eq!(
        checked_inventory_event(&object_full, 1, 0, true),
        natural_inventory_event(&object_full, 1, [0, 0], true)
    );
    assert_eq!(checked_inventory_event(&object_full, 1, 0, true), None);
    assert_eq!(object_full.objects, u64::MAX);
    assert_eq!(object_full.bytes, 0);
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

fn pending_work(machine: &Machine) -> usize {
    machine.events.len() +
        machine
            .cursors
            .iter()
            .map(|cursor| 1 + 2 * cursor.remaining.len())
            .sum::<usize>()
}

fn unspent_credit(graph: &Graph, machine: &Machine) -> usize {
    graph
        .iter()
        .enumerate()
        .filter(|(vertex, _)| match machine.phase {
            Phase::Activation => !machine.ledger.active[*vertex],
            Phase::Deactivation => machine.ledger.active[*vertex],
        })
        .map(|(_, children)| 1 + 2 * children.len())
        .sum()
}

fn potential(graph: &Graph, machine: &Machine) -> usize {
    pending_work(machine) + unspent_credit(graph, machine)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TraceWork {
    steps: usize,
    event_consumptions: usize,
    cursor_transfers: usize,
    cursor_discards: usize,
    sign_changes: Vec<usize>,
}

impl TraceWork {
    fn new(vertex_count: usize) -> Self {
        Self {
            steps: 0,
            event_consumptions: 0,
            cursor_transfers: 0,
            cursor_discards: 0,
            sign_changes: vec![0; vertex_count],
        }
    }

    fn action_counts(&self) -> [usize; 4] {
        [self.steps, self.event_consumptions, self.cursor_transfers, self.cursor_discards]
    }
}

fn take_checked_step(
    graph: &Graph,
    machine: &mut Machine,
    prefer_cursor: bool,
    work: &mut TraceWork,
) {
    let before = machine.clone();
    assert!(valid_configuration(graph, &before));
    assert_eq!(pending_work(&before) == 0, before.drained());
    assert!(machine.step(graph, prefer_cursor, Mutation::None).unwrap());
    assert!(valid_configuration(graph, machine), "after microstep {}: {machine:?}", work.steps);
    assert_eq!(before.phase, machine.phase);
    assert_eq!(before.roots, machine.roots);
    assert_eq!(potential(graph, &before), 1 + potential(graph, machine));
    assert_eq!(pending_work(machine) == 0, machine.drained());
    if machine.events.len() + 1 == before.events.len() {
        assert_eq!(machine.events, before.events[..before.events.len() - 1]);
        work.event_consumptions += 1;
    } else if machine.events.len() == before.events.len() + 1 {
        assert_eq!(machine.events[..before.events.len()], before.events);
        assert_eq!(machine.events.last(), before.cursors.last().unwrap().remaining.first());
        assert_eq!(machine.cursors.len(), before.cursors.len());
        work.cursor_transfers += 1;
    } else {
        assert_eq!(machine.events, before.events);
        assert_eq!(machine.cursors.len() + 1, before.cursors.len());
        assert!(before.cursors.last().unwrap().remaining.is_empty());
        work.cursor_discards += 1;
    }
    for vertex in 0..graph.len() {
        let was_active = before.ledger.active[vertex];
        let is_active = machine.ledger.active[vertex];
        match machine.phase {
            Phase::Activation => {
                assert!(before.ledger.counts[vertex] <= machine.ledger.counts[vertex]);
                assert!(!was_active || is_active);
            }
            Phase::Deactivation => {
                assert!(machine.ledger.counts[vertex] <= before.ledger.counts[vertex]);
                assert!(!is_active || was_active);
            }
        }
        work.sign_changes[vertex] += usize::from(was_active != is_active);
        assert!(work.sign_changes[vertex] <= 1);
    }
    work.steps += 1;
}

fn drain_cursor_trace(graph: &Graph, machine: &mut Machine, schedule: &[bool]) -> TraceWork {
    assert!(!schedule.is_empty());
    assert!(valid_configuration(graph, machine), "initial configuration: {machine:?}");
    let initial = machine.clone();
    let initial_potential = potential(graph, &initial);
    let initial_pending = pending_work(&initial);
    let initial_suffix_edges = initial
        .cursors
        .iter()
        .map(|cursor| cursor.remaining.len())
        .sum::<usize>();
    let mut work = TraceWork::new(graph.len());
    while !machine.drained() {
        assert!(work.steps < initial_potential, "bounded-model potential exhausted: {machine:?}");
        let prefer_cursor = schedule[work.steps % schedule.len()];
        take_checked_step(graph, machine, prefer_cursor, &mut work);
        assert_eq!(work.steps + potential(graph, machine), initial_potential);
    }
    assert_eq!(pending_work(machine), 0);
    let mut expanded_vertices = 0;
    let mut expanded_edges = 0;
    let mut expanded_work = 0;
    for (vertex, children) in graph.iter().enumerate() {
        let changed = initial.ledger.active[vertex] != machine.ledger.active[vertex];
        assert_eq!(work.sign_changes[vertex], usize::from(changed));
        if changed {
            expanded_vertices += 1;
            expanded_edges += children.len();
            expanded_work += 1 + 2 * children.len();
        }
    }
    assert_eq!(work.steps, initial_pending + expanded_work);
    assert_eq!(work.cursor_transfers, initial_suffix_edges + expanded_edges);
    assert_eq!(
        work.event_consumptions,
        initial.events.len() + initial_suffix_edges + expanded_edges
    );
    assert_eq!(work.cursor_discards, initial.cursors.len() + expanded_vertices);
    assert_eq!(work.steps, work.event_consumptions + work.cursor_transfers + work.cursor_discards);
    assert_eq!(unspent_credit(graph, &initial), unspent_credit(graph, machine) + expanded_work);
    let completed = machine.clone();
    assert!(!machine.step(graph, schedule[0], Mutation::None).unwrap());
    assert_eq!(*machine, completed);
    work
}

fn drain(graph: &Graph, machine: &mut Machine, schedule: &[bool]) -> Vec<usize> {
    let work = drain_cursor_trace(graph, machine, schedule);
    assert_eq!(machine.ledger, cold_graph_oracle(graph, &machine.roots));
    work.sign_changes
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
    let initial_potential = potential(graph, &machine);
    for step in 0..initial_potential {
        let before_potential = potential(graph, &machine);
        assert!(machine.step(graph, step % 2 == 1, mutation).unwrap());
        if !valid_configuration(graph, &machine) {
            assert_ne!(
                before_potential,
                1 + potential(graph, &machine),
                "mutant escaped exact work: {mutation:?}"
            );
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
        assert_eq!(before_potential, 1 + potential(graph, &machine));
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

#[test]
fn cursor_work_counts_edges_and_frame_disposal_exactly() {
    for (graph, roots, expected_actions) in [
        (vec![vec![]], vec![0], [2, 1, 0, 1]),
        (vec![vec![]], vec![0, 0], [3, 2, 0, 1]),
        (vec![vec![], vec![0, 0]], vec![1, 1], [8, 4, 2, 2]),
    ] {
        for schedule in [vec![false], vec![true], vec![true, false, false]] {
            let empty_roots = vec![0; graph.len()];
            let mut activation =
                Machine::activate(&cold_graph_oracle(&graph, &empty_roots), &empty_roots, &roots);
            let activated = drain_cursor_trace(&graph, &mut activation, &schedule);
            assert_eq!(activated.action_counts(), expected_actions);
            assert_eq!(activation.ledger, cold_graph_oracle(&graph, &activation.roots));
            let mut deactivation = activation.begin_deactivation(&roots).unwrap();
            let deactivated = drain_cursor_trace(&graph, &mut deactivation, &schedule);
            assert_eq!(deactivated.action_counts(), expected_actions);
            assert_eq!(deactivation.ledger, cold_graph_oracle(&graph, &empty_roots));
        }
    }
}

#[test]
fn unrelated_graph_extension_does_not_change_cursor_work() {
    let graph = vec![vec![], vec![0, 0]];
    let mut extended_graph = graph.clone();
    for parent in 2..12 {
        extended_graph.push(if parent == 2 {
            vec![]
        } else {
            vec![parent - 1, parent - 1, 2]
        });
    }
    for retain_unrelated_root in [false, true] {
        let roots = vec![0; graph.len()];
        let mut extended_roots = vec![0; extended_graph.len()];
        if retain_unrelated_root {
            extended_roots[11] = 1;
        }
        let mut activation = Machine::activate(&cold_graph_oracle(&graph, &roots), &roots, &[1, 1]);
        let mut extended_activation = Machine::activate(
            &cold_graph_oracle(&extended_graph, &extended_roots),
            &extended_roots,
            &[1, 1],
        );
        let initial_extra_support = extended_activation.ledger.active[2..].to_vec();
        let activated = drain_cursor_trace(&graph, &mut activation, &[false, true]);
        let extended_activated =
            drain_cursor_trace(&extended_graph, &mut extended_activation, &[true, false]);
        assert_eq!(activated.action_counts(), extended_activated.action_counts());
        assert_eq!(activated.sign_changes, extended_activated.sign_changes[..graph.len()]);
        assert!(
            extended_activated.sign_changes[graph.len()..]
                .iter()
                .all(|&changes| changes == 0)
        );
        assert_eq!(extended_activation.ledger.active[2..], initial_extra_support);
        assert_eq!(activation.ledger, cold_graph_oracle(&graph, &activation.roots));
        assert_eq!(
            extended_activation.ledger,
            cold_graph_oracle(&extended_graph, &extended_activation.roots)
        );
        let mut deactivation = activation.begin_deactivation(&[1, 1]).unwrap();
        let mut extended_deactivation = extended_activation.begin_deactivation(&[1, 1]).unwrap();
        let deactivated = drain_cursor_trace(&graph, &mut deactivation, &[true]);
        let extended_deactivated =
            drain_cursor_trace(&extended_graph, &mut extended_deactivation, &[false]);
        assert_eq!(deactivated.action_counts(), extended_deactivated.action_counts());
        assert_eq!(deactivated.sign_changes, extended_deactivated.sign_changes[..graph.len()]);
        assert!(
            extended_deactivated.sign_changes[graph.len()..]
                .iter()
                .all(|&changes| changes == 0)
        );
        assert_eq!(extended_deactivation.ledger.active[2..], initial_extra_support);
        assert_eq!(deactivation.ledger, cold_graph_oracle(&graph, &roots));
        assert_eq!(
            extended_deactivation.ledger,
            cold_graph_oracle(&extended_graph, &extended_roots)
        );
    }
}

#[test]
fn suspended_cursor_work_uses_remaining_suffix_not_root_count() {
    let graph = vec![vec![], vec![0, 0], vec![1, 0]];
    for phase in [Phase::Activation, Phase::Deactivation] {
        let mut machine = match phase {
            Phase::Activation => {
                Machine::activate(&cold_graph_oracle(&graph, &[0; 3]), &[0; 3], &[2, 2])
            }
            Phase::Deactivation => {
                let roots = root_counts(graph.len(), &[2, 2]);
                Machine::activate(&cold_graph_oracle(&graph, &roots), &roots, &[])
                    .begin_deactivation(&[2, 2])
                    .unwrap()
            }
        };
        let mut uninterrupted = machine.clone();
        let complete_work = drain_cursor_trace(&graph, &mut uninterrupted, &[true, false, true]);
        let mut prefix_work = TraceWork::new(graph.len());
        let prefix_schedule: &[bool] = match phase {
            Phase::Activation => &[false, true, false, true],
            Phase::Deactivation => &[false, true, false],
        };
        for &prefer_cursor in prefix_schedule {
            take_checked_step(&graph, &mut machine, prefer_cursor, &mut prefix_work);
        }
        let suspended = machine.clone();
        assert!(!suspended.cursors.is_empty());
        assert!(suspended.cursors.iter().any(|cursor| {
            !cursor.remaining.is_empty() && cursor.remaining.len() < graph[cursor.parent].len()
        }));
        let whole_parent_work = suspended.events.len() +
            suspended
                .cursors
                .iter()
                .map(|cursor| 1 + 2 * graph[cursor.parent].len())
                .sum::<usize>();
        assert!(pending_work(&suspended) > suspended.events.len());
        assert!(whole_parent_work > pending_work(&suspended));
        for schedule in [vec![false], vec![true, false]] {
            let mut resumed = suspended.clone();
            let work = drain_cursor_trace(&graph, &mut resumed, &schedule);
            let changed_work = work
                .sign_changes
                .iter()
                .enumerate()
                .filter(|(_, changes)| **changes != 0)
                .map(|(vertex, _)| 1 + 2 * graph[vertex].len())
                .sum::<usize>();
            assert_eq!(work.steps, pending_work(&suspended) + changed_work);
            assert_ne!(work.steps, suspended.events.len() + changed_work);
            assert_ne!(work.steps, whole_parent_work + changed_work);
            assert_eq!(prefix_work.steps + work.steps, complete_work.steps);
            assert_eq!(
                prefix_work.event_consumptions + work.event_consumptions,
                complete_work.event_consumptions
            );
            assert_eq!(
                prefix_work.cursor_transfers + work.cursor_transfers,
                complete_work.cursor_transfers
            );
            assert_eq!(
                prefix_work.cursor_discards + work.cursor_discards,
                complete_work.cursor_discards
            );
            assert_eq!(resumed, uninterrupted);
            assert_eq!(resumed.ledger, cold_graph_oracle(&graph, &resumed.roots));
        }
    }
}

#[test]
fn cyclic_cursor_execution_terminates_without_rooted_support() {
    let graph = vec![vec![0]];
    for schedule in [vec![false], vec![true], vec![true, false]] {
        let mut activation = Machine::activate(&cold_graph_oracle(&graph, &[0]), &[0], &[0]);
        let activated = drain_cursor_trace(&graph, &mut activation, &schedule);
        assert_eq!(activated.action_counts(), [4, 2, 1, 1]);
        assert_eq!(activation.ledger.counts, [2]);
        assert_eq!(activation.ledger, cold_graph_oracle(&graph, &[1]));
        let mut deactivation = activation.begin_deactivation(&[0]).unwrap();
        let deactivated = drain_cursor_trace(&graph, &mut deactivation, &schedule);
        assert_eq!(deactivated.action_counts(), [1, 1, 0, 0]);
        assert_eq!(deactivated.sign_changes, [0]);
        assert_eq!(deactivation.roots, [0]);
        assert_eq!(deactivation.ledger.counts, [1]);
        assert_eq!(deactivation.ledger.active, [true]);
        let cold = cold_graph_oracle(&graph, &[0]);
        assert_eq!(cold.counts, [0]);
        assert_ne!(deactivation.ledger, cold);
    }
}

#[test]
fn zero_pending_work_does_not_require_zero_unspent_credit() {
    let graph = vec![vec![], vec![0]];
    for phase in [Phase::Activation, Phase::Deactivation] {
        let roots = match phase {
            Phase::Activation => vec![0, 0],
            Phase::Deactivation => vec![0, 1],
        };
        let mut machine = Machine::activate(&cold_graph_oracle(&graph, &roots), &roots, &[]);
        machine.phase = phase;
        assert!(machine.drained());
        assert_eq!(pending_work(&machine), 0);
        assert_eq!(unspent_credit(&graph, &machine), 4);
        assert_eq!(potential(&graph, &machine), 4);
        let before = machine.clone();
        let work = drain_cursor_trace(&graph, &mut machine, &[true, false]);
        assert_eq!(work.action_counts(), [0, 0, 0, 0]);
        assert_eq!(machine, before);
    }
    let leaf_graph = vec![vec![]];
    let mut empty_frame = Machine::activate(&cold_graph_oracle(&leaf_graph, &[0]), &[0], &[0]);
    take_checked_step(&leaf_graph, &mut empty_frame, false, &mut TraceWork::new(1));
    assert_eq!(pending(&empty_frame, 0), 0);
    assert_eq!(pending_work(&empty_frame), 1);
    assert!(!empty_frame.drained());
    let work = drain_cursor_trace(&leaf_graph, &mut empty_frame, &[true]);
    assert_eq!(work.action_counts(), [1, 0, 0, 1]);
}

#[derive(Clone, Copy, Debug)]
enum WrongCost {
    OmitExpansionDisposal,
    OmitCursorDisposal,
    SingleCursorEdgeUnit,
    DeduplicateExpansionEdges,
    InvertCreditDomain,
}

fn wrong_potential(graph: &Graph, machine: &Machine, mutation: WrongCost) -> usize {
    let frame_base = usize::from(!matches!(mutation, WrongCost::OmitCursorDisposal));
    let suffix_unit = if matches!(mutation, WrongCost::SingleCursorEdgeUnit) {
        1
    } else {
        2
    };
    let pending = machine.events.len() +
        machine
            .cursors
            .iter()
            .map(|cursor| frame_base + suffix_unit * cursor.remaining.len())
            .sum::<usize>();
    let credit = graph
        .iter()
        .enumerate()
        .filter(|(vertex, _)| {
            let selected = match machine.phase {
                Phase::Activation => !machine.ledger.active[*vertex],
                Phase::Deactivation => machine.ledger.active[*vertex],
            };
            selected != matches!(mutation, WrongCost::InvertCreditDomain)
        })
        .map(|(_, children)| {
            let base = usize::from(!matches!(mutation, WrongCost::OmitExpansionDisposal));
            let degree = if matches!(mutation, WrongCost::DeduplicateExpansionEdges) {
                children
                    .iter()
                    .enumerate()
                    .filter(|(index, child)| !children[..*index].contains(child))
                    .count()
            } else {
                children.len()
            };
            base + 2 * degree
        })
        .sum::<usize>();
    pending + credit
}

#[test]
fn wrong_credit_and_cursor_costs_are_detected() {
    let graph = vec![vec![], vec![0, 0]];
    for phase in [Phase::Activation, Phase::Deactivation] {
        let initial = match phase {
            Phase::Activation => {
                Machine::activate(&cold_graph_oracle(&graph, &[0, 0]), &[0, 0], &[1])
            }
            Phase::Deactivation => {
                Machine::activate(&cold_graph_oracle(&graph, &[0, 1]), &[0, 1], &[])
                    .begin_deactivation(&[1])
                    .unwrap()
            }
        };
        for mutation in [
            WrongCost::OmitExpansionDisposal,
            WrongCost::OmitCursorDisposal,
            WrongCost::SingleCursorEdgeUnit,
            WrongCost::DeduplicateExpansionEdges,
            WrongCost::InvertCreditDomain,
        ] {
            let mut machine = initial.clone();
            let mut work = TraceWork::new(graph.len());
            let initial_potential = potential(&graph, &machine);
            let mut detected = false;
            while !machine.drained() {
                assert!(work.steps < initial_potential);
                let wrong_before = wrong_potential(&graph, &machine, mutation);
                let prefer_cursor = work.steps % 2 == 1;
                take_checked_step(&graph, &mut machine, prefer_cursor, &mut work);
                detected |= wrong_before != 1 + wrong_potential(&graph, &machine, mutation);
            }
            assert!(detected, "wrong cost escaped {phase:?}: {mutation:?}");
            assert_eq!(machine.ledger, cold_graph_oracle(&graph, &machine.roots));
        }
    }
}
