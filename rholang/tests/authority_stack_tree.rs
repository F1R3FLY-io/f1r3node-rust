use std::collections::BTreeMap;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostAuthority, CostSignature, CostStack};
use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
use rholang::rust::interpreter::accounting::authority::{
    allocate_physical_settlement, apply_physical_settlement,
    apply_physical_settlement_with_host_work, authority_demand,
    canonical_authority, cost_region, plan_stack_frontier_with_host_work, stack_branch_id,
    stack_branches, validate_stack_birth,
    verify_physical_settlement, AuthorityError, AuthorityEvent, AuthorityPhysicalEventDraw,
    AuthorityPhysicalInventory, ResourceMultiset, StackBirthShape,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;

fn signature(name: &[u8]) -> CostSignature {
    CostSignature { value: Some(Value::Ground(name.to_vec())) }
}

fn event(id: u8, signatures: &[CostSignature]) -> AuthorityEvent<[u8; 32]> {
    let authority = canonical_authority(&CostAuthority {
        regions: signatures
            .iter()
            .enumerate()
            .map(|(index, signature)| cost_region(signature, b"tree integration", index as u32).unwrap())
            .collect(),
    })
    .unwrap();
    AuthorityEvent {
        event_id: [id; 32],
        debit: authority_demand(&authority).unwrap(),
        authority,
    }
}

fn draw(id: u8, mut stack_ids: Vec<[u8; 32]>) -> AuthorityPhysicalEventDraw<[u8; 32]> {
    stack_ids.sort();
    AuthorityPhysicalEventDraw {
        event_id: [id; 32],
        balances: ResourceMultiset::default(),
        stack_ids,
    }
}

#[test]
fn one_shared_prefix_then_independent_siblings() {
    let prefix = signature(b"prefix");
    let left = signature(b"left");
    let right = signature(b"right");
    let root_id = [71; 32];
    let tree = CostStack {
        cells: vec![prefix.clone()],
        parallel_tail: vec![
            CostStack { cells: vec![left.clone()], parallel_tail: Vec::new() },
            CostStack { cells: vec![right.clone()], parallel_tail: Vec::new() },
        ],
    };
    let left_id = stack_branch_id(&root_id, &[0]);
    let right_id = stack_branch_id(&root_id, &[1]);
    let branches = stack_branches(root_id, &tree).unwrap();
    assert_eq!(branches.iter().map(|b| b.transfer_offset).collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(branches[1].parent_gate, Some((root_id, 1)));
    assert_eq!(branches[2].parent_gate, Some((root_id, 1)));
    assert_eq!(
        validate_stack_birth(&[prefix.clone(), left.clone(), right.clone()], Some(&tree), None)
            .unwrap(),
        StackBirthShape { cell_count: 3, branch_count: 2 },
    );
    assert_eq!(
        validate_stack_birth(&[prefix.clone(), right.clone(), left.clone()], Some(&tree), None),
        Err(AuthorityError::InvalidStackBirth),
    );
    let inventory = AuthorityPhysicalInventory {
        balances: ResourceMultiset::default(),
        balance_custody: BTreeMap::new(),
        stacks: BTreeMap::from([(root_id, vec![prefix.clone()])]),
        born_stacks: BTreeMap::new(),
        stack_trees: BTreeMap::from([(root_id, tree)]),
    };
    let prefix_event = event(1, std::slice::from_ref(&prefix));
    let right_event = event(2, std::slice::from_ref(&right));
    let left_event = event(3, std::slice::from_ref(&left));
    let events = [prefix_event.clone(), right_event.clone(), left_event.clone()];
    let draws = [
        draw(1, vec![root_id]),
        draw(2, vec![right_id]),
        draw(3, vec![left_id]),
    ];
    let settlement = verify_physical_settlement(&events, &BTreeMap::new(), &inventory, &draws)
        .expect("either sibling may advance first after the prefix");
    assert_eq!(
        allocate_physical_settlement(&events, &BTreeMap::new(), &inventory)
            .unwrap()
            .stack_pops,
        settlement.stack_pops,
    );
    assert_eq!(
        verify_physical_settlement(
            &[right_event.clone(), prefix_event.clone()],
            &BTreeMap::new(),
            &inventory,
            &[draw(2, vec![right_id]), draw(1, vec![root_id])],
        ),
        Err(AuthorityError::PhysicalAuthorityMismatch),
    );
    let prefix_and_right = event(4, &[prefix.clone(), right.clone()]);
    assert_eq!(
        verify_physical_settlement(
            &[prefix_and_right],
            &BTreeMap::new(),
            &inventory,
            &[draw(4, vec![root_id, right_id])],
        ),
        Err(AuthorityError::PhysicalAuthorityMismatch),
    );
    let siblings = event(5, &[left, right]);
    assert!(verify_physical_settlement(
        &[prefix_event, siblings],
        &BTreeMap::new(),
        &inventory,
        &[draw(1, vec![root_id]), draw(5, vec![left_id, right_id])],
    )
    .is_ok());

    let mut residual = inventory;
    apply_physical_settlement(&mut residual, &settlement).unwrap();
    assert!(residual.stacks[&root_id].is_empty());
    assert!(residual.stack_trees[&root_id].parallel_tail.iter().all(|branch| branch.cells.is_empty()));
}

#[test]
fn metered_apply_refuses_before_mutation_and_keeps_linear_budget_parity() {
    let prefix = signature(b"prefix");
    let child = signature(b"child");
    let sibling = signature(b"sibling");
    let root_id = [72; 32];
    let tree = CostStack {
        cells: vec![prefix.clone()],
        parallel_tail: vec![
            CostStack { cells: vec![child.clone()], parallel_tail: Vec::new() },
            CostStack { cells: vec![sibling.clone()], parallel_tail: Vec::new() },
        ],
    };
    let branched = AuthorityPhysicalInventory {
        balances: ResourceMultiset::default(),
        balance_custody: BTreeMap::new(),
        stacks: BTreeMap::from([(root_id, vec![prefix.clone()])]),
        born_stacks: BTreeMap::new(),
        stack_trees: BTreeMap::from([(root_id, tree)]),
    };
    let events = [event(1, std::slice::from_ref(&prefix)), event(2, std::slice::from_ref(&child))];
    let settlement = allocate_physical_settlement(&events, &BTreeMap::new(), &branched).unwrap();
    let mut tight_limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000));
    tight_limits.set(HostWorkDimension::SearchCandidates, HostWorkLimit::new(0));
    let tight = HostWorkBudget::new(tight_limits);
    let mut unchanged = branched.clone();
    assert_eq!(
        apply_physical_settlement_with_host_work(&mut unchanged, &settlement, Some(&tight)),
        Err(AuthorityError::HostWorkRejected),
    );
    assert_eq!(unchanged, branched, "a branch-path budget refusal must be atomic");

    let generous = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000)));
    let mut applied = branched;
    apply_physical_settlement_with_host_work(&mut applied, &settlement, Some(&generous)).unwrap();
    assert!(applied.stack_trees[&root_id].cells.is_empty());
    assert!(applied.stack_trees[&root_id].parallel_tail[0].cells.is_empty());
    assert_eq!(applied.stack_trees[&root_id].parallel_tail[1].cells, vec![sibling]);
    assert!(generous.usage(HostWorkDimension::SearchCandidates).get() > 0);

    let linear = AuthorityPhysicalInventory {
        balances: ResourceMultiset::default(),
        balance_custody: BTreeMap::new(),
        stacks: BTreeMap::from([(root_id, vec![prefix.clone()])]),
        born_stacks: BTreeMap::new(),
        stack_trees: BTreeMap::new(),
    };
    let linear_events = [event(3, std::slice::from_ref(&prefix))];
    let linear_settlement =
        allocate_physical_settlement(&linear_events, &BTreeMap::new(), &linear).unwrap();
    let mut historical = linear.clone();
    apply_physical_settlement(&mut historical, &linear_settlement).unwrap();
    let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    let before = budget.usages();
    let mut metered = linear;
    apply_physical_settlement_with_host_work(&mut metered, &linear_settlement, Some(&budget))
        .unwrap();
    assert_eq!(metered, historical);
    assert_eq!(budget.usages(), before, "linear application must keep exact host-budget parity");
}

#[test]
fn frontier_partition_releases_only_live_subtrees_with_original_indices() {
    let root_id = [73; 32];
    let root_a = signature(b"root-a");
    let root_b = signature(b"root-b");
    let left = signature(b"left");
    let grandchild = signature(b"grandchild");
    let right_a = signature(b"right-a");
    let right_b = signature(b"right-b");
    let tree = CostStack {
        cells: vec![root_a, root_b],
        parallel_tail: vec![
            CostStack {
                cells: vec![left],
                parallel_tail: vec![CostStack {
                    cells: vec![grandchild.clone()],
                    parallel_tail: Vec::new(),
                }],
            },
            CostStack {
                cells: vec![right_a.clone(), right_b.clone()],
                parallel_tail: Vec::new(),
            },
        ],
    };
    let before = tree.clone();
    let left_id = stack_branch_id(&root_id, &[0]);
    let grandchild_id = stack_branch_id(&root_id, &[0, 0]);
    let right_id = stack_branch_id(&root_id, &[1]);
    // These are cumulative pops from a previously verified multi-event
    // settlement, not a claim that a child may pop in its parent's event.
    let pops = BTreeMap::from([(root_id, 2), (left_id, 1)]);
    let published = plan_stack_frontier_with_host_work(root_id, &tree, &pops, None).unwrap();
    assert_eq!(tree, before, "the pure planner must not mutate its birth tree");
    assert_eq!(published.len(), 2);
    assert_eq!(published[0].original_root_id, root_id);
    assert_eq!(published[0].original_branch_id, grandchild_id);
    assert_eq!(published[0].authored_path, vec![0, 0]);
    assert_eq!(published[0].head(), Some(&grandchild));
    assert_eq!(published[0].original_transfer_indices, vec![3]);
    assert_eq!(published[1].original_branch_id, right_id);
    assert_eq!(published[1].authored_path, vec![1]);
    assert_eq!(published[1].head(), Some(&right_a));
    assert_eq!(published[1].residual.cells, vec![right_a, right_b]);
    assert_eq!(published[1].original_transfer_indices, vec![4, 5]);
    let mut all = published
        .iter()
        .flat_map(|part| part.original_transfer_indices.iter().copied())
        .collect::<Vec<_>>();
    all.sort_unstable();
    assert_eq!(all, vec![3, 4, 5], "one original cell index per surviving occurrence");

    let generous = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000)));
    assert_eq!(
        plan_stack_frontier_with_host_work(root_id, &tree, &pops, Some(&generous)).unwrap(),
        published,
    );
    assert!(generous.usage(HostWorkDimension::SearchCandidates).get() > 0);

    let mut tight_limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000));
    tight_limits.set(HostWorkDimension::SearchCandidates, HostWorkLimit::new(0));
    let tight = HostWorkBudget::new(tight_limits);
    assert_eq!(
        plan_stack_frontier_with_host_work(root_id, &tree, &pops, Some(&tight)),
        Err(AuthorityError::HostWorkRejected),
    );
    assert_eq!(tree, before, "budget refusal cannot alter the original tree");
    assert_eq!(
        plan_stack_frontier_with_host_work(root_id, &tree, &BTreeMap::from([([99; 32], 1)]), None),
        Err(AuthorityError::UnknownStackResource),
    );
    assert_eq!(
        plan_stack_frontier_with_host_work(root_id, &tree, &BTreeMap::from([(left_id, 2)]), None),
        Err(AuthorityError::ExhaustedStackResource),
    );
}

#[test]
fn deep_frontier_partition_is_iterative_and_linear_stacks_stay_separate() {
    let root_id = [74; 32];
    let cell = signature(b"deep");
    let linear = CostStack { cells: vec![cell.clone()], parallel_tail: Vec::new() };
    assert_eq!(
        plan_stack_frontier_with_host_work(root_id, &linear, &BTreeMap::new(), None),
        Err(AuthorityError::InvalidStackBirth),
        "legacy linear settlement must retain its historical path",
    );
    let mut tree = linear;
    for _ in 0..4_096 {
        tree = CostStack { cells: vec![cell.clone()], parallel_tail: vec![tree] };
    }
    let published = plan_stack_frontier_with_host_work(
        root_id,
        &tree,
        &BTreeMap::from([(root_id, 1)]),
        None,
    )
    .expect("4096-deep frontier traversal is heap-bounded");
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].authored_path, vec![0]);
    assert_eq!(published[0].original_transfer_indices.len(), 4_096);
    assert_eq!(published[0].original_transfer_indices[0], 1);
    assert_eq!(*published[0].original_transfer_indices.last().unwrap(), 4_096);
}
