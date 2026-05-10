use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn reverse_reachability(direct: BTreeSet<u8>, neglect_edges: &[(u8, u8)]) -> BTreeSet<u8> {
    let mut reverse = BTreeMap::<u8, Vec<u8>>::new();
    for (neglecter, target) in neglect_edges {
        reverse.entry(*target).or_default().push(*neglecter);
    }

    let mut closure = direct.clone();
    let mut queue = VecDeque::from_iter(direct);
    while let Some(target) = queue.pop_front() {
        for next in reverse.get(&target).into_iter().flatten() {
            if closure.insert(*next) {
                queue.push_back(*next);
            }
        }
    }
    closure
}

#[test]
fn uc_93_deep_reachability_chain_has_path_certificate() {
    let edges = [(1, 0), (2, 1), (3, 2)];
    let closure = reverse_reachability(BTreeSet::from([0]), &edges);
    assert_eq!(closure, BTreeSet::from([0, 1, 2, 3]));
}

#[test]
fn uc_93_disconnected_cycle_cannot_exploit_closure() {
    let edges = [(1, 2), (2, 1)];
    let closure = reverse_reachability(BTreeSet::from([0]), &edges);
    assert_eq!(closure, BTreeSet::from([0]));
}
