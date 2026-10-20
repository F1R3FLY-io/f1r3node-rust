use std::collections::BTreeMap;

use models::rhoapi::{CostSignature, CostStack, ListParWithRandom, Par};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    canonical_cost_signature, cost_signature_to_sig, funding_sig_channel, plan_stack_frontier,
    stack_branches, verify_physical_settlement, AuthorityEvent, AuthorityPhysicalEventDraw,
    AuthorityPhysicalInventory, StackFrontierPublication,
};
use rholang::rust::interpreter::accounting::{Sig, SignatureChannel};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::Produce;

use crate::rust::errors::CasperError;
use crate::rust::rholang::runtime::RuntimeOps;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PurseStack {
    pub instance_id: [u8; 32],
    pub source_hash: [u8; 32],
    pub channel: Par,
    pub datum_index: i32,
    pub random_state: Vec<u8>,
    pub persistent: bool,
    pub stack: CostStack,
}

pub fn supply_channel(sig: &Sig) -> Par {
    debug_assert!(sig.is_funding_former());
    SignatureChannel::from_sig(sig).par
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PurseInventory {
    pub balance: Option<i64>,
    pub stacks: Vec<PurseStack>,
}

/// A non-serializable, settlement-scope capability. Its only constructor
/// checks an exact chronological physical draw trace against the authenticated
/// authority events and captured stack trees. Raw cumulative counts can never
/// authorize a child branch, because they omit the prior-event prefix gate.
pub struct VerifiedPurseStackPops<'a> {
    live_root: [u8; 32],
    stacks: &'a [PurseStack],
    pops: BTreeMap<[u8; 32], u64>,
    branch_roots: BTreeMap<[u8; 32], [u8; 32]>,
}

impl VerifiedPurseStackPops<'_> {
    pub fn live_root(&self) -> [u8; 32] { self.live_root }
    pub fn pops(&self) -> &BTreeMap<[u8; 32], u64> { &self.pops }
    pub fn root_for_branch(&self, branch: &[u8; 32]) -> Option<[u8; 32]> {
        self.branch_roots.get(branch).copied()
    }
    pub fn stacks(&self) -> &[PurseStack] { self.stacks }
}

pub struct PlannedPurseStackPublication {
    pub original_root_id: [u8; 32],
    pub original_branch_id: [u8; 32],
    pub original_transfer_indices: Vec<usize>,
    pub channel: Par,
    pub datum: ListParWithRandom,
    pub persistent: bool,
    pub source_hash: [u8; 32],
}

impl PlannedPurseStackPublication {
    pub fn head(&self) -> &CostSignature {
        self.datum
            .cost_stack
            .as_ref()
            .and_then(|stack| stack.cells.first())
            .expect("frontier publication has a nonempty head")
    }
}

pub struct PlannedPurseStackSettlement<'a> {
    live_root: [u8; 32],
    epoch: EventCodecEpoch,
    originals: Vec<&'a PurseStack>,
    publications: Vec<PlannedPurseStackPublication>,
}

impl PlannedPurseStackSettlement<'_> {
    pub fn originals(&self) -> &[&PurseStack] { &self.originals }
    pub fn publications(&self) -> &[PlannedPurseStackPublication] { &self.publications }
}

/// Convert a verified chronological trace into one physical mutation plan per
/// original root. A child pop never retires its own synthetic datum: only the
/// one stored root datum is removed, then every independently live residual
/// subtree is published exactly once with its original preorder receipt IDs.
pub fn plan_verified_purse_stack_pops<'a>(
    verified: &VerifiedPurseStackPops<'a>,
    epoch: EventCodecEpoch,
) -> Result<PlannedPurseStackSettlement<'a>, CasperError> {
    let mut grouped = BTreeMap::<[u8; 32], BTreeMap<[u8; 32], u64>>::new();
    for (branch_id, pop) in &verified.pops {
        let root_id = verified.root_for_branch(branch_id).ok_or_else(|| {
            CasperError::RuntimeError("verified stack branch has no captured root".to_string())
        })?;
        grouped.entry(root_id).or_default().insert(*branch_id, *pop);
    }
    let mut originals = Vec::new();
    let mut publications = Vec::new();
    originals
        .try_reserve_exact(grouped.len())
        .map_err(|_| CasperError::RuntimeError("stack plan allocation failed".to_string()))?;
    for (root_id, pops) in grouped {
        let mut matches = verified
            .stacks
            .iter()
            .filter(|stack| stack.instance_id == root_id);
        let original = matches.next().ok_or_else(|| {
            CasperError::RuntimeError("verified stack root is absent".to_string())
        })?;
        if matches.next().is_some() {
            return Err(CasperError::RuntimeError(
                "verified stack capture contains duplicate roots".to_string(),
            ));
        }
        originals.push(original);
        let frontier = if original.stack.parallel_tail.is_empty() {
            let pop = pops.get(&root_id).copied().ok_or_else(|| {
                CasperError::RuntimeError("linear stack has a child pop".to_string())
            })?;
            if pops.len() != 1 || pop == 0 {
                return Err(CasperError::RuntimeError(
                    "linear stack settlement is not a positive root prefix".to_string(),
                ));
            }
            let count = usize::try_from(pop).map_err(|_| {
                CasperError::RuntimeError("linear stack pop count overflow".to_string())
            })?;
            if count > original.stack.cells.len() {
                return Err(CasperError::RuntimeError(
                    "linear stack settlement exceeds its cells".to_string(),
                ));
            }
            let remaining = &original.stack.cells[count..];
            if remaining.is_empty() {
                Vec::new()
            } else {
                vec![StackFrontierPublication {
                    original_root_id: root_id,
                    original_branch_id: root_id,
                    authored_path: Vec::new(),
                    residual: CostStack {
                        cells: remaining.to_vec(),
                        parallel_tail: Vec::new(),
                    },
                    original_transfer_indices: (count..original.stack.cells.len()).collect(),
                }]
            }
        } else {
            plan_stack_frontier(root_id, &original.stack, &pops)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?
        };
        for publication in frontier {
            let head = publication.head().ok_or_else(|| {
                CasperError::RuntimeError("frontier publication has no head".to_string())
            })?;
            let signature = cost_signature_to_sig(head)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            let channel = funding_sig_channel(&signature)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            let datum = ListParWithRandom {
                pars: Vec::new(),
                random_state: original.random_state.clone(),
                cost_authority: None,
                cost_stack: Some(publication.residual),
            };
            let source = Produce::create_for_epoch(epoch, &channel, &datum, original.persistent)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            let source_hash = source
                .hash
                .bytes()
                .try_into()
                .map_err(|_| CasperError::RuntimeError("stack source width".to_string()))?;
            publications.try_reserve(1).map_err(|_| {
                CasperError::RuntimeError("stack plan allocation failed".to_string())
            })?;
            publications.push(PlannedPurseStackPublication {
                original_root_id: publication.original_root_id,
                original_branch_id: publication.original_branch_id,
                original_transfer_indices: publication.original_transfer_indices,
                channel,
                datum,
                persistent: original.persistent,
                source_hash,
            });
        }
    }
    Ok(PlannedPurseStackSettlement {
        live_root: verified.live_root,
        epoch,
        originals,
        publications,
    })
}

/// Apply only a plan derived from a verified event-order witness. The live
/// root and every captured original are checked before the first mutation;
/// removal and frontier production then share one rollback boundary.
pub async fn apply_verified_stack_plan(
    runtime_ops: &mut RuntimeOps,
    plan: PlannedPurseStackSettlement<'_>,
) -> Result<(), CasperError> {
    let current_root: [u8; 32] = runtime_ops
        .runtime
        .get_root()
        .await
        .bytes()
        .as_slice()
        .try_into()
        .map_err(|_| CasperError::RuntimeError("live stack root width".to_string()))?;
    if current_root != plan.live_root
        || runtime_ops.runtime.reducer.space.codec_epoch() != plan.epoch
    {
        return Err(CasperError::RuntimeError(
            "verified stack plan refers to another live state or codec epoch".to_string(),
        ));
    }
    let mut removals = BTreeMap::<Vec<u8>, (Par, Vec<&PurseStack>)>::new();
    for original in &plan.originals {
        removals
            .entry(original.channel.encode_to_vec())
            .or_insert_with(|| (original.channel.clone(), Vec::new()))
            .1
            .push(*original);
    }
    for (_, channel_removals) in removals.values_mut() {
        channel_removals.sort_by(|left, right| right.datum_index.cmp(&left.datum_index));
    }
    for (channel, channel_removals) in removals.values() {
        let live = super::prepaid_receipts::read_live_data(runtime_ops, channel).await?;
        check_stack_captures(&live, channel_removals)?;
    }
    let checkpoint = runtime_ops.runtime.create_soft_checkpoint().await;
    let mutation = async {
        for (_key, (channel, channel_removals)) in removals {
            for stack in channel_removals {
                runtime_ops
                    .runtime
                    .reducer
                    .space
                    .remove_data_at_recorded(&channel, stack.datum_index, &stack.instance_id)
                    .await
                    .map_err(|error| {
                        CasperError::RuntimeError(format!(
                            "verified stack root retirement failed: {error}"
                        ))
                    })?;
            }
        }
        for publication in plan.publications {
            runtime_ops
                .runtime
                .reducer
                .space
                .produce(
                    publication.channel,
                    publication.datum,
                    publication.persistent,
                )
                .await
                .map_err(|error| {
                    CasperError::RuntimeError(format!(
                        "verified stack frontier publication failed: {error}"
                    ))
                })?;
        }
        Ok::<(), CasperError>(())
    }
    .await;
    if let Err(error) = mutation {
        runtime_ops
            .runtime
            .revert_to_soft_checkpoint(checkpoint)
            .await;
        return Err(error);
    }
    Ok(())
}

pub(crate) fn verify_purse_stack_pops<'a>(
    live_root: [u8; 32],
    stacks: &'a [PurseStack],
    events: &[AuthorityEvent<[u8; 32]>],
    signatures: &BTreeMap<[u8; 32], CostSignature>,
    inventory: &AuthorityPhysicalInventory<[u8; 32]>,
    draws: &[AuthorityPhysicalEventDraw<[u8; 32]>],
) -> Result<VerifiedPurseStackPops<'a>, CasperError> {
    let mut branch_roots = BTreeMap::new();
    let mut root_ids = std::collections::BTreeSet::new();
    for stack in stacks {
        if !root_ids.insert(stack.instance_id) {
            return Err(CasperError::RuntimeError(
                "rooted authority capture repeats a stack identity".to_string(),
            ));
        }
        if inventory.stacks.get(&stack.instance_id) != Some(&stack.stack.cells) {
            return Err(CasperError::RuntimeError(
                "physical inventory differs from the rooted stack prefix".to_string(),
            ));
        }
        let recorded = inventory.stack_trees.get(&stack.instance_id);
        if stack.stack.parallel_tail.is_empty() {
            if recorded.is_some() {
                return Err(CasperError::RuntimeError(
                    "linear stack has a forged branch sidecar".to_string(),
                ));
            }
        } else if recorded != Some(&stack.stack) {
            return Err(CasperError::RuntimeError(
                "physical inventory differs from the rooted stack tree".to_string(),
            ));
        }
        let branches = stack_branches(stack.instance_id, &stack.stack)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        for branch in branches {
            if branch_roots
                .insert(branch.stack_id, stack.instance_id)
                .is_some()
            {
                return Err(CasperError::RuntimeError(
                    "rooted stack branches have duplicate identities".to_string(),
                ));
            }
        }
    }
    let settlement = verify_physical_settlement(events, signatures, inventory, draws)
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
    if settlement
        .stack_pops
        .keys()
        .any(|branch| !branch_roots.contains_key(branch))
    {
        return Err(CasperError::RuntimeError(
            "physical draw names a stack outside the rooted prepaid capture".to_string(),
        ));
    }
    Ok(VerifiedPurseStackPops {
        live_root,
        stacks,
        pops: settlement.stack_pops,
        branch_roots,
    })
}

/// Canonicalize cell values while preserving the authored stack forest.
/// Branch position determines child authority identity, so a sorter that
/// reorders siblings or a reconstruction from flat cells is not equivalent.
fn canonical_cost_stack(stack: &CostStack) -> Result<CostStack, CasperError> {
    let mut canonical = stack.clone();
    let mut pending = Vec::new();
    pending.try_reserve(1).map_err(|_| {
        CasperError::RuntimeError("cost stack traversal allocation failed".to_string())
    })?;
    pending.push(&mut canonical);
    while let Some(node) = pending.pop() {
        for cell in &mut node.cells {
            *cell = canonical_cost_signature(cell)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        }
        pending.try_reserve(node.parallel_tail.len()).map_err(|_| {
            CasperError::RuntimeError("cost stack traversal allocation failed".to_string())
        })?;
        pending.extend(node.parallel_tail.iter_mut().rev());
    }
    Ok(canonical)
}

pub fn decode_purse_inventory(
    data: &[Datum<ListParWithRandom>],
    expected_head: &CostSignature,
) -> Result<PurseInventory, CasperError> {
    let expected_head = canonical_cost_signature(expected_head)
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
    let expected_key = cost_signature_to_sig(&expected_head)
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?
        .lane_hash();
    let channel = supply_channel(
        &cost_signature_to_sig(&expected_head)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?,
    );
    let mut stored = Vec::new();
    for (datum_index, datum) in data.iter().enumerate() {
        if let Some(stack) = &datum.a.cost_stack {
            if !datum.a.pars.is_empty() || datum.a.cost_authority.is_some() {
                return Err(CasperError::RuntimeError(
                    "cost stack datum contains unrelated payload or authority".to_string(),
                ));
            }
            let head = stack.cells.first().ok_or_else(|| {
                CasperError::RuntimeError(
                    "authority purse contains an empty cost stack".to_string(),
                )
            })?;
            let head = canonical_cost_signature(head)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            let head_key = cost_signature_to_sig(&head)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?
                .lane_hash();
            if head_key != expected_key {
                return Err(CasperError::RuntimeError(
                    "cost stack is stored on a channel different from its head signature"
                        .to_string(),
                ));
            }
            let canonical = canonical_cost_stack(stack)?;
            stored.push((
                <[u8; 32]>::try_from(datum.source.hash.bytes())
                    .expect("RSpace produce identity length"),
                datum_index as i32,
                datum.a.random_state.clone(),
                datum.persist,
                canonical,
            ));
        } else if !datum.a.pars.is_empty() || datum.a.cost_authority.is_some() {
            return Err(CasperError::RuntimeError(
                "authority purse channel contains a non-stack datum".to_string(),
            ));
        }
    }
    stored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let mut occurrences = std::collections::BTreeMap::<[u8; 32], u64>::new();
    let stacks = stored
        .into_iter()
        .map(|(source, datum_index, random_state, persistent, stack)| {
            let occurrence = occurrences.entry(source.clone()).or_default();
            let mut identity = Vec::with_capacity(source.len() + 8 + 48);
            identity.extend_from_slice(b"f1r3node:cost-accounted-rho:purse-stack:v1");
            identity.extend_from_slice(&source);
            identity.extend_from_slice(&occurrence.to_le_bytes());
            *occurrence += 1;
            PurseStack {
                instance_id: crypto::rust::hash::blake2b256::Blake2b256::hash(identity)
                    .try_into()
                    .expect("Blake2b-256 digest length"),
                source_hash: source,
                channel: channel.clone(),
                datum_index,
                random_state,
                persistent,
                stack,
            }
        })
        .collect();
    Ok(PurseInventory {
        balance: None,
        stacks,
    })
}
fn check_stack_captures(
    live: &[Datum<ListParWithRandom>],
    selected: &[&PurseStack],
) -> Result<(), CasperError> {
    let Some(first) = selected.first() else {
        return Ok(());
    };
    let head = first
        .stack
        .cells
        .first()
        .ok_or_else(|| CasperError::RuntimeError("captured stack is empty".to_string()))?;
    let inventory = decode_purse_inventory(live, head)?;
    let by_index = inventory
        .stacks
        .iter()
        .map(|stack| (stack.datum_index, stack))
        .collect::<std::collections::BTreeMap<_, _>>();
    for captured in selected {
        if by_index.get(&captured.datum_index).copied() != Some(*captured) {
            return Err(CasperError::RuntimeError(
                "authority stack capture differs from the complete live inventory".to_string(),
            ));
        }
    }
    Ok(())
}

pub async fn apply_stack_pops(
    runtime_ops: &mut RuntimeOps,
    stacks: &[PurseStack],
    stack_pops: &std::collections::BTreeMap<[u8; 32], u64>,
) -> Result<(), CasperError> {
    let by_id = stacks
        .iter()
        .map(|stack| (stack.instance_id, stack))
        .collect::<std::collections::BTreeMap<_, _>>();
    if by_id.len() != stacks.len() {
        return Err(CasperError::RuntimeError(
            "authority inventory contains duplicate stack identities".to_string(),
        ));
    }

    let mut removals = std::collections::BTreeMap::<Vec<u8>, (Par, Vec<&PurseStack>)>::new();
    let mut tails = Vec::<([u8; 32], Par, ListParWithRandom, bool)>::new();
    for (stack_id, pop_count) in stack_pops {
        if *pop_count == 0 {
            return Err(CasperError::RuntimeError(
                "authority stack settlement contains a zero pop count".to_string(),
            ));
        }
        let stack = by_id.get(stack_id).copied().ok_or_else(|| {
            CasperError::RuntimeError(
                "authority stack settlement references an unknown stack".to_string(),
            )
        })?;
        if !stack.stack.parallel_tail.is_empty() {
            return Err(CasperError::RuntimeError(
                "branched stack requires a verified event-order settlement".to_string(),
            ));
        }
        let pop_count = usize::try_from(*pop_count).map_err(|_| {
            CasperError::RuntimeError(
                "authority stack pop count exceeds the platform range".to_string(),
            )
        })?;
        if pop_count > stack.stack.cells.len() {
            return Err(CasperError::RuntimeError(
                "authority stack settlement exceeds the stack length".to_string(),
            ));
        }
        removals
            .entry(stack.channel.encode_to_vec())
            .or_insert_with(|| (stack.channel.clone(), Vec::new()))
            .1
            .push(stack);

        let remaining = stack.stack.cells[pop_count..].to_vec();
        if let Some(head) = remaining.first() {
            let signature = cost_signature_to_sig(head)
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
            tails.push((
                *stack_id,
                supply_channel(&signature),
                ListParWithRandom {
                    pars: Vec::new(),
                    random_state: stack.random_state.clone(),
                    cost_authority: None,
                    cost_stack: Some(CostStack {
                        cells: remaining,
                        parallel_tail: Vec::new(),
                    }),
                },
                stack.persistent,
            ));
        }
    }

    for (_, channel_removals) in removals.values_mut() {
        channel_removals.sort_by(|left, right| right.datum_index.cmp(&left.datum_index));
    }
    for (channel, channel_removals) in removals.values() {
        let data = runtime_ops
            .runtime
            .reducer
            .space
            .get_data(channel)
            .await
            .map_err(|error| CasperError::InterpreterError(error.into()))?;
        check_stack_captures(&data, channel_removals)?;
    }
    let checkpoint = runtime_ops.runtime.create_soft_checkpoint().await;
    let mutation = async {
        for (_channel_key, (channel, channel_removals)) in removals {
            for stack in channel_removals {
                runtime_ops
                    .runtime
                    .reducer
                    .space
                    .remove_data_at_recorded(&channel, stack.datum_index, &stack.instance_id)
                    .await
                    .map_err(|error| {
                        CasperError::RuntimeError(format!(
                            "authority stack removal failed: {error}"
                        ))
                    })?;
            }
        }

        tails.sort_by_key(|tail| tail.0);
        for (_, channel, datum, persistent) in tails {
            runtime_ops
                .runtime
                .reducer
                .space
                .produce(channel, datum, persistent)
                .await
                .map_err(|error| {
                    CasperError::RuntimeError(format!(
                        "authority stack tail release failed: {error}"
                    ))
                })?;
        }
        Ok::<(), CasperError>(())
    }
    .await;
    if let Err(error) = mutation {
        runtime_ops
            .runtime
            .revert_to_soft_checkpoint(checkpoint)
            .await;
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::CostAuthority;
    use rholang::rust::interpreter::accounting::authority::{
        authority_demand, cost_region, stack_branch_id, stack_branches, AuthorityPhysicalEventDraw,
        ResourceMultiset,
    };

    use super::*;

    fn ground(value: u8) -> CostSignature {
        CostSignature {
            value: Some(Value::Ground(vec![value])),
        }
    }

    #[test]
    fn canonical_inventory_preserves_ordered_branch_identity() {
        let tree = CostStack {
            cells: vec![ground(1)],
            parallel_tail: vec![
                CostStack {
                    cells: vec![ground(2)],
                    parallel_tail: vec![CostStack {
                        cells: vec![ground(3)],
                        parallel_tail: Vec::new(),
                    }],
                },
                CostStack {
                    cells: vec![ground(4)],
                    parallel_tail: Vec::new(),
                },
            ],
        };
        // Changed by the merge of feature/cost-accounted-rho (finding R3-35):
        // user decision 1 removed the host-work budget parameter of
        // `canonical_cost_stack`. The old call was:
        // let canonical = canonical_cost_stack(&tree, None).unwrap();
        let canonical = canonical_cost_stack(&tree).expect("a valid cost tree canonicalizes");
        let identities = |stack: &CostStack| {
            stack_branches([7; 32], stack)
                .unwrap()
                .into_iter()
                .map(|branch| {
                    (
                        branch.stack_id,
                        branch.parent_gate,
                        branch.stack.cells.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(identities(&canonical), identities(&tree));
        assert_eq!(canonical, tree);

        let mut invalid = tree;
        invalid.parallel_tail[0].parallel_tail[0].cells[0] = CostSignature {
            value: Some(Value::Unit(false)),
        };
        // Changed by the merge of feature/cost-accounted-rho (finding R3-35): no
        // budget argument. The old line was:
        // assert!(canonical_cost_stack(&invalid, None).is_err());
        assert!(canonical_cost_stack(&invalid).is_err());
    }

    #[test]
    fn verified_root_group_releases_only_unconsumed_siblings() {
        let root_id = [37; 32];
        let prefix = ground(1);
        let left = ground(2);
        let right = ground(3);
        let tree = CostStack {
            cells: vec![prefix.clone()],
            parallel_tail: vec![
                CostStack {
                    cells: vec![left.clone()],
                    parallel_tail: Vec::new(),
                },
                CostStack {
                    cells: vec![right.clone()],
                    parallel_tail: Vec::new(),
                },
            ],
        };
        let stack = PurseStack {
            instance_id: root_id,
            source_hash: [38; 32],
            channel: supply_channel(&cost_signature_to_sig(&prefix).unwrap()),
            datum_index: 0,
            random_state: vec![9; 32],
            persistent: false,
            stack: tree.clone(),
        };
        let captured = [stack];
        let inventory = AuthorityPhysicalInventory {
            balances: ResourceMultiset::default(),
            balance_custody: BTreeMap::new(),
            stacks: BTreeMap::from([(root_id, vec![prefix.clone()])]),
            born_stacks: BTreeMap::new(),
            stack_trees: BTreeMap::from([(root_id, tree)]),
        };
        let make_event = |event_id: [u8; 32], signature: &CostSignature| {
            let authority = CostAuthority {
                regions: vec![cost_region(signature, b"frontier", u32::from(event_id[0])).unwrap()],
            };
            AuthorityEvent {
                event_id,
                debit: authority_demand(&authority).unwrap(),
                authority,
            }
        };
        let events = [make_event([1; 32], &prefix), make_event([2; 32], &left)];
        let left_id = stack_branch_id(&root_id, &[0]);
        let right_id = stack_branch_id(&root_id, &[1]);
        let draws = [
            AuthorityPhysicalEventDraw {
                event_id: events[0].event_id,
                balances: ResourceMultiset::default(),
                stack_ids: vec![root_id],
            },
            AuthorityPhysicalEventDraw {
                event_id: events[1].event_id,
                balances: ResourceMultiset::default(),
                stack_ids: vec![left_id],
            },
        ];
        let verified = verify_purse_stack_pops(
            [39; 32],
            &captured,
            &events,
            &BTreeMap::new(),
            &inventory,
            &draws,
        )
        .unwrap();
        assert_eq!(verified.root_for_branch(&left_id), Some(root_id));
        assert_eq!(verified.pops().len(), 2);
        let plan = plan_verified_purse_stack_pops(&verified, EventCodecEpoch::CostV2).unwrap();
        assert_eq!(plan.originals().len(), 1);
        assert_eq!(plan.publications().len(), 1);
        assert_eq!(plan.publications()[0].original_branch_id, right_id);
        assert_eq!(plan.publications()[0].original_transfer_indices, vec![2]);
        assert_eq!(plan.publications()[0].head(), &right);

        let simultaneous = [AuthorityPhysicalEventDraw {
            event_id: [3; 32],
            balances: ResourceMultiset::default(),
            stack_ids: {
                let mut ids = vec![root_id, left_id];
                ids.sort();
                ids
            },
        }];
        let same_event_signature =
            rholang::rust::interpreter::accounting::authority::compound_cost_signatures(
                &prefix, &left,
            )
            .unwrap();
        let event = [make_event([3; 32], &same_event_signature)];
        assert!(verify_purse_stack_pops(
            [39; 32],
            &captured,
            &event,
            &BTreeMap::new(),
            &inventory,
            &simultaneous,
        )
        .is_err());
    }
}
