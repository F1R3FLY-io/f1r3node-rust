use models::rhoapi::{CostSignature, CostStack, ListParWithRandom, Par};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    canonical_cost_signature, cost_signature_to_sig,
};
use rholang::rust::interpreter::accounting::{Sig, SignatureChannel};
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::internal::Datum;

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
            let mut canonical = Vec::with_capacity(stack.cells.len());
            for cell in &stack.cells {
                canonical.push(
                    canonical_cost_signature(cell)
                        .map_err(|error| CasperError::RuntimeError(error.to_string()))?,
                );
            }
            stored.push((
                <[u8; 32]>::try_from(datum.source.hash.bytes())
                    .expect("RSpace produce identity length"),
                datum_index as i32,
                datum.a.random_state.clone(),
                datum.persist,
                CostStack { cells: canonical },
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
    apply_stack_pops_with_budget(runtime_ops, stacks, stack_pops, None).await
}

pub async fn apply_stack_pops_metered(
    runtime_ops: &mut RuntimeOps,
    stacks: &[PurseStack],
    stack_pops: &std::collections::BTreeMap<[u8; 32], u64>,
    budget: &HostWorkBudget,
) -> Result<(), CasperError> {
    apply_stack_pops_with_budget(runtime_ops, stacks, stack_pops, Some(budget)).await
}

async fn apply_stack_pops_with_budget(
    runtime_ops: &mut RuntimeOps,
    stacks: &[PurseStack],
    stack_pops: &std::collections::BTreeMap<[u8; 32], u64>,
    budget: Option<&HostWorkBudget>,
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
                    cost_stack: Some(CostStack { cells: remaining }),
                },
                stack.persistent,
            ));
        }
    }

    for (_, channel_removals) in removals.values_mut() {
        channel_removals.sort_by(|left, right| right.datum_index.cmp(&left.datum_index));
    }
    for (channel, channel_removals) in removals.values() {
        let live = if let Some(budget) = budget {
            super::prepaid_receipts::read_live_data_metered(runtime_ops, channel, budget).await?
        } else {
            runtime_ops.runtime.reducer.space.get_data(channel).await
        };
        check_stack_captures(&live, channel_removals)?;
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
