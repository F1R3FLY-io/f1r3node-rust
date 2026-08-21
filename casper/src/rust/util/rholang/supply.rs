use models::rhoapi::{CostSignature, CostStack, ListParWithRandom, Par};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    canonical_cost_signature, cost_signature_to_sig,
};
use rholang::rust::interpreter::accounting::{Sig, SignatureChannel};
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
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let expected_key = cost_signature_to_sig(&expected_head)
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
        .lane_hash();
    let channel = supply_channel(
        &cost_signature_to_sig(&expected_head)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?,
    );
    let mut stored = Vec::new();
    for (datum_index, datum) in data.iter().enumerate() {
        if let Some(stack) = &datum.a.cost_stack {
            if !datum.a.pars.is_empty() || datum.a.cost_authority.is_some() {
                return Err(CasperError::InvalidCostSettlement(
                    "cost stack datum contains unrelated payload or authority".to_string(),
                ));
            }
            let head = stack.cells.first().ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "authority purse contains an empty cost stack".to_string(),
                )
            })?;
            let head = canonical_cost_signature(head)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let head_key = cost_signature_to_sig(&head)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
                .lane_hash();
            if head_key != expected_key {
                return Err(CasperError::InvalidCostSettlement(
                    "cost stack is stored on a channel different from its head signature"
                        .to_string(),
                ));
            }
            let mut canonical = Vec::with_capacity(stack.cells.len());
            for cell in &stack.cells {
                canonical.push(
                    canonical_cost_signature(cell)
                        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?,
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
            return Err(CasperError::InvalidCostSettlement(
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

/// The ONE channel-keying function: `Σ⟦s⟧ ≜ SignatureChannel::from_sig(s).par`.
///
/// This is the single canonical signature→name map used identically by the
/// Appendix-A translation, the supply producer (C), and the WD-D2 consumer
/// (handoff Decision 1). The g/#P axis collapses at the channel (DR-1: equal
/// atom bytes ⇒ equal channel) and compounds are permutation-invariant via
/// `ParSortMatcher::sort_match` (accounting/mod.rs:1544-1612).
///
/// PRECONDITION (F-A separation, red-team M3 — `docs/theory/cost-accounting-impl/
/// f-a-funding-vs-capability-separation.md` §3/§6): `sig` is a FUNDING-grammar
/// signature (`Sig::is_funding_former` — `g|#P|s∘s`: `Unit`/`Ground`/`Quote`
/// atoms folded by `And`). The value/capability type-logic connectives
/// (`Plus`/`With`/`Bang`/`WhyNot`/`Lolly`) and `Threshold` are CAPABILITY-LAYER
/// ONLY and are unreachable here: the only `sig` ever passed in is the envelope
/// `Sig` from `accounting::envelope_sig*` (total to `Quote`/`And`). The
/// `debug_assert!` makes that loud in debug/test builds without changing release
/// behavior; it cannot fire on any currently-valid funding deploy (envelope_sig
/// is total to Quote/And) and is the belt-and-suspenders companion to the
/// load-bearing INGRESS reject in
/// `models/.../casper_message.rs::from_proto_cosigned_with_sig_algebra`.
pub fn supply_channel(sig: &Sig) -> Par {
    debug_assert!(
        sig.is_funding_former(),
        "supply_channel: a value/capability connective (⊕/&/!/?/⊸/Threshold) \
         reached the funding supply-channel keying — these are capability-layer \
         only and unreachable on the funding path \
         (cost-accounted-rho §App-A: g|#P|s∘s). sig = {:?}",
        sig
    );
    SignatureChannel::from_sig(sig).par
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
        return Err(CasperError::InvalidCostSettlement(
            "authority inventory contains duplicate stack identities".to_string(),
        ));
    }

    let mut removals =
        std::collections::BTreeMap::<Vec<u8>, (Par, Vec<(i32, [u8; 32], ListParWithRandom)>)>::new(
        );
    let mut tails = Vec::<([u8; 32], Par, ListParWithRandom, bool)>::new();
    for (stack_id, pop_count) in stack_pops {
        if *pop_count == 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority stack settlement contains a zero pop count".to_string(),
            ));
        }
        let stack = by_id.get(stack_id).ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "authority stack settlement references an unknown stack".to_string(),
            )
        })?;
        let pop_count = usize::try_from(*pop_count).map_err(|_| {
            CasperError::InvalidCostSettlement(
                "authority stack pop count exceeds the platform range".to_string(),
            )
        })?;
        if pop_count > stack.stack.cells.len() {
            return Err(CasperError::InvalidCostSettlement(
                "authority stack settlement exceeds the stack length".to_string(),
            ));
        }
        let original = ListParWithRandom {
            pars: Vec::new(),
            random_state: stack.random_state.clone(),
            cost_authority: None,
            cost_stack: Some(stack.stack.clone()),
        };
        removals
            .entry(stack.channel.encode_to_vec())
            .or_insert_with(|| (stack.channel.clone(), Vec::new()))
            .1
            .push((stack.datum_index, *stack_id, original));

        let remaining = stack.stack.cells[pop_count..].to_vec();
        if let Some(head) = remaining.first() {
            let signature = cost_signature_to_sig(head)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
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
        channel_removals.sort_by(|left, right| right.0.cmp(&left.0));
    }
    for (channel, channel_removals) in removals.values() {
        let live = runtime_ops.runtime.reducer.space.get_data(channel).await;
        for (index, _stack_id, expected) in channel_removals {
            let datum = usize::try_from(*index)
                .ok()
                .and_then(|index| live.get(index))
                .ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "authority stack moved before atomic settlement".to_string(),
                    )
                })?;
            if &datum.a != expected {
                return Err(CasperError::InvalidCostSettlement(
                    "authority stack changed before atomic settlement".to_string(),
                ));
            }
        }
    }
    let checkpoint = runtime_ops.runtime.create_soft_checkpoint().await;
    let mutation = async {
        for (_channel_key, (channel, channel_removals)) in removals {
            for (index, stack_id, _) in channel_removals {
                runtime_ops
                    .runtime
                    .reducer
                    .space
                    .remove_data_at_recorded(&channel, index, &stack_id)
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
    use rholang::rust::interpreter::accounting::Sig;

    use super::*;

    /// The shared-basis integration invariant (handoff Coordination, Stage B
    /// Decision 5): `supply_channel(s)` is exactly `SignatureChannel::from_sig`
    /// of `s` — the SAME basis `Sig::lane_hash` is anchored to.
    /// We assert (a) the channel equality and (b) that `lane_hash` is the
    /// domain-separated Blake2b256 of exactly this channel's wire encoding, so
    /// two signatures share an identity key iff they share a supply channel.
    #[test]
    fn supply_channel_matches_canonical_purse_identity() {
        use prost::Message;

        let sigs = vec![
            Sig::Ground(vec![1, 2, 3, 4]),
            Sig::Ground(b"validator-pk-bytes".to_vec()),
            Sig::Quote(vec![9, 9, 9]),
            Sig::And(
                Box::new(Sig::Ground(vec![1])),
                Box::new(Sig::Ground(vec![2])),
            ),
            Sig::Unit,
        ];

        const SIGNATURE_LANE_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:signature-lane:v1";

        for s in &sigs {
            // (a) supply_channel == from_sig basis.
            let supply = supply_channel(s);
            let from_sig = SignatureChannel::from_sig(s).par;
            assert_eq!(
                supply, from_sig,
                "supply_channel must equal SignatureChannel::from_sig for {:?}",
                s
            );

            // (b) lane_hash is anchored to the SAME channel (no drift).
            let encoded = supply.encode_to_vec();
            let mut domain_separated =
                Vec::with_capacity(SIGNATURE_LANE_DOMAIN.len() + encoded.len());
            domain_separated.extend_from_slice(SIGNATURE_LANE_DOMAIN);
            domain_separated.extend_from_slice(&encoded);
            let expected = crypto::rust::hash::blake2b256::Blake2b256::hash(domain_separated);
            assert_eq!(
                &expected[..32],
                &s.lane_hash()[..],
                "lane_hash must be the domain-separated Blake2b256 of supply_channel for {:?}",
                s
            );
        }
    }

    #[test]
    fn purse_inventory_preserves_stack_multiplicity_and_order() {
        use models::rhoapi::cost_signature::Value;

        let head = CostSignature {
            value: Some(Value::Ground(b"head".to_vec())),
        };
        let tail = CostSignature {
            value: Some(Value::Ground(b"tail".to_vec())),
        };
        let channel = supply_channel(&Sig::Ground(b"head".to_vec()));
        let stack = CostStack {
            cells: vec![head.clone(), tail],
        };
        let stack_datum = ListParWithRandom {
            pars: Vec::new(),
            random_state: vec![1],
            cost_authority: None,
            cost_stack: Some(stack.clone()),
        };
        let data = vec![
            Datum::create(&channel, stack_datum.clone(), false),
            Datum::create(&channel, stack_datum, false),
        ];

        let inventory = decode_purse_inventory(&data, &head).unwrap();
        assert_eq!(inventory.balance, None);
        assert_eq!(inventory.stacks.len(), 2);
        assert_eq!(inventory.stacks[0].stack, stack);
        assert_eq!(inventory.stacks[1].stack, stack);
        assert_ne!(
            inventory.stacks[0].instance_id,
            inventory.stacks[1].instance_id
        );
    }

    #[test]
    fn purse_inventory_rejects_a_stack_on_the_wrong_head_channel() {
        use models::rhoapi::cost_signature::Value;

        let expected = CostSignature {
            value: Some(Value::Ground(b"expected".to_vec())),
        };
        let wrong = CostSignature {
            value: Some(Value::Ground(b"wrong".to_vec())),
        };
        let channel = supply_channel(&Sig::Ground(b"expected".to_vec()));
        let datum = Datum::create(
            &channel,
            ListParWithRandom {
                pars: Vec::new(),
                random_state: vec![1],
                cost_authority: None,
                cost_stack: Some(CostStack { cells: vec![wrong] }),
            },
            false,
        );

        assert!(decode_purse_inventory(&[datum], &expected).is_err());
    }

    #[test]
    fn purse_inventory_rejects_a_parallel_integer_wallet_datum() {
        use models::rhoapi::cost_signature::Value;
        use models::rhoapi::expr::ExprInstance;
        use models::rhoapi::Expr;

        let expected = CostSignature {
            value: Some(Value::Ground(b"expected".to_vec())),
        };
        let channel = supply_channel(&Sig::Ground(b"expected".to_vec()));
        let datum = Datum::create(
            &channel,
            ListParWithRandom {
                pars: vec![Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GInt(7)),
                }])],
                random_state: vec![1],
                cost_authority: None,
                cost_stack: None,
            },
            false,
        );

        assert!(decode_purse_inventory(&[datum], &expected).is_err());
    }
}
