use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn closure(direct: BTreeSet<u8>, edges: &[(u8, u8)]) -> BTreeSet<u8> {
    let mut reverse = BTreeMap::<u8, Vec<u8>>::new();
    for (neglecter, target) in edges {
        reverse.entry(*target).or_default().push(*neglecter);
    }
    let mut out = direct.clone();
    let mut queue = VecDeque::from_iter(direct);
    while let Some(target) = queue.pop_front() {
        for next in reverse.get(&target).into_iter().flatten() {
            if out.insert(*next) {
                queue.push_back(*next);
            }
        }
    }
    out
}

fn stake_sum(stakes: &[i64], validators: &BTreeSet<u8>) -> i64 {
    validators.iter().map(|v| stakes[*v as usize]).sum()
}

#[test]
fn uc_94_weighted_damage_requires_closure_bound_violation() {
    let stakes = [4, 4, 1, 1];
    let direct = BTreeSet::from([2]);
    let closed = closure(direct.clone(), &[(1, 2), (0, 1)]);
    assert_eq!(stake_sum(&stakes, &direct), 1);
    assert_eq!(stake_sum(&stakes, &closed), 9);
    assert!(closed.len() > 2);
}

#[test]
fn uc_94_bound_preserving_case_has_limited_damage() {
    let stakes = [4, 4, 1, 1];
    let direct = BTreeSet::from([2]);
    let closed = closure(direct.clone(), &[(3, 2)]);
    assert_eq!(stake_sum(&stakes, &closed) - stake_sum(&stakes, &direct), 1);
    assert!(closed.len() <= 2);
}
