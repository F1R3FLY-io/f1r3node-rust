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
/// PRECONDITION (F-A separation, red-team M3 — `docs/casper/theory/cost-accounting-impl/
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

fn check_stack_captures(
    live: &[Datum<ListParWithRandom>],
    selected: &[&PurseStack],
) -> Result<(), CasperError> {
    let Some(first) = selected.first() else {
        return Ok(());
    };
    let head =
        first.stack.cells.first().ok_or_else(|| {
            CasperError::InvalidCostSettlement("captured stack is empty".to_string())
        })?;
    let inventory = decode_purse_inventory(live, head)?;
    let by_index = inventory
        .stacks
        .iter()
        .map(|stack| (stack.datum_index, stack))
        .collect::<std::collections::BTreeMap<_, _>>();
    for captured in selected {
        if by_index.get(&captured.datum_index).copied() != Some(*captured) {
            return Err(CasperError::InvalidCostSettlement(
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
        return Err(CasperError::InvalidCostSettlement(
            "authority inventory contains duplicate stack identities".to_string(),
        ));
    }

    let mut removals = std::collections::BTreeMap::<Vec<u8>, (Par, Vec<&PurseStack>)>::new();
    let mut tails = Vec::<([u8; 32], Par, ListParWithRandom, bool)>::new();
    for (stack_id, pop_count) in stack_pops {
        if *pop_count == 0 {
            return Err(CasperError::InvalidCostSettlement(
                "authority stack settlement contains a zero pop count".to_string(),
            ));
        }
        let stack = by_id.get(stack_id).copied().ok_or_else(|| {
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
        removals
            .entry(stack.channel.encode_to_vec())
            .or_insert_with(|| (stack.channel.clone(), Vec::new()))
            .1
            .push(stack);

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
        channel_removals.sort_by(|left, right| right.datum_index.cmp(&left.datum_index));
    }
    for (channel, channel_removals) in removals.values() {
        let live = runtime_ops.runtime.reducer.space.get_data(channel).await;
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

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::accounting::Sig;

    use super::*;

    #[tokio::test]
    async fn stack_pop_rejects_altered_capture_metadata_before_mutation() {
        use std::collections::{BTreeMap, HashMap};
        use std::sync::Arc;

        use models::rhoapi::cost_signature::Value;
        use rholang::rust::interpreter::external_services::ExternalServices;
        use rholang::rust::interpreter::matcher::r#match::Matcher;
        use rholang::rust::interpreter::rho_runtime::create_runtime_from_kv_store;
        use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
        use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

        let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
        let mut runtime = RuntimeOps::new(
            create_runtime_from_kv_store(
                store,
                Arc::new(HashMap::new()),
                false,
                &mut Vec::new(),
                Arc::new(Box::new(Matcher)),
                ExternalServices::noop(),
            )
            .await,
        );
        let head = CostSignature {
            value: Some(Value::Ground(b"head".to_vec())),
        };
        let channel = supply_channel(&Sig::Ground(b"head".to_vec()));
        for random in [1, 2] {
            runtime
                .runtime
                .reducer
                .space
                .produce(
                    channel.clone(),
                    ListParWithRandom {
                        pars: Vec::new(),
                        random_state: vec![random],
                        cost_authority: None,
                        cost_stack: Some(CostStack {
                            cells: vec![head.clone(), head.clone()],
                        }),
                    },
                    false,
                )
                .await
                .unwrap();
        }
        let base = runtime.runtime.create_checkpoint().await;
        for field in 0..8 {
            runtime.runtime.reset(&base.root).await.unwrap();
            let inventory = decode_purse_inventory(
                &runtime.runtime.reducer.space.get_data(&channel).await,
                &head,
            )
            .unwrap();
            let mut selected = inventory
                .stacks
                .iter()
                .find(|stack| stack.datum_index == 0)
                .unwrap()
                .clone();
            let mut earlier = inventory
                .stacks
                .iter()
                .find(|stack| stack.datum_index == 1)
                .unwrap()
                .clone();
            match field {
                0 => selected.source_hash[0] ^= 1,
                1 => selected.persistent = !selected.persistent,
                2 => selected.instance_id[0] ^= 1,
                3 => selected.datum_index = -1,
                4 => selected.channel = Par::default(),
                5 => selected.random_state.push(0),
                6 => selected.stack.cells.pop().map(|_| ()).unwrap(),
                _ => {
                    earlier = selected.clone();
                    selected.instance_id[0] ^= 1;
                }
            }
            let pops = BTreeMap::from([(earlier.instance_id, 1), (selected.instance_id, 1)]);
            let result = apply_stack_pops(&mut runtime, &[earlier, selected], &pops).await;
            assert!(
                result.is_err(),
                "altered captured field {field} was accepted"
            );
            let after = runtime.runtime.create_checkpoint().await;
            assert_eq!(after.root, base.root);
        }
    }

    #[tokio::test]
    async fn full_stack_capture_check_preserves_tail_state_and_replay() {
        use std::collections::BTreeMap;

        use models::rhoapi::cost_signature::Value;
        use rholang::rust::interpreter::test_utils::resources::create_runtimes;
        use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
        use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

        let store = InMemoryStoreManager::new().r_space_stores().await.unwrap();
        let (play, replay, _) = create_runtimes(store, false, &mut Vec::new()).await;
        let mut native = RuntimeOps::new(play);
        let mut replay = RuntimeOps::new(replay);
        let signatures = (1..=3)
            .map(|value| CostSignature {
                value: Some(Value::Ground(vec![value])),
            })
            .collect::<Vec<_>>();
        let channel = supply_channel(&Sig::Ground(vec![1]));
        for random in [1, 1, 2] {
            native
                .runtime
                .reducer
                .space
                .produce(
                    channel.clone(),
                    ListParWithRandom {
                        pars: Vec::new(),
                        random_state: vec![random],
                        cost_authority: None,
                        cost_stack: Some(CostStack {
                            cells: signatures.clone(),
                        }),
                    },
                    false,
                )
                .await
                .unwrap();
        }
        let base = native.runtime.create_checkpoint().await;
        let inventory = decode_purse_inventory(
            &native.runtime.reducer.space.get_data(&channel).await,
            &signatures[0],
        )
        .unwrap();
        assert_eq!(inventory.stacks.len(), 3);
        let pops = inventory
            .stacks
            .iter()
            .enumerate()
            .map(|(index, stack)| (stack.instance_id, index as u64 + 1))
            .collect::<BTreeMap<_, _>>();
        apply_stack_pops(&mut native, &inventory.stacks, &pops)
            .await
            .unwrap();
        assert!(native
            .runtime
            .reducer
            .space
            .get_data(&channel)
            .await
            .is_empty());
        for count in [1, 2] {
            let tail_channel = supply_channel(&Sig::Ground(vec![count as u8 + 1]));
            let tails = decode_purse_inventory(
                &native.runtime.reducer.space.get_data(&tail_channel).await,
                &signatures[count],
            )
            .unwrap();
            assert_eq!(tails.stacks.len(), 1);
            assert_eq!(tails.stacks[0].stack.cells, signatures[count..]);
            assert_eq!(
                tails.stacks[0].random_state,
                inventory.stacks[count - 1].random_state
            );
            assert!(!tails.stacks[0].persistent);
            assert_ne!(
                tails.stacks[0].source_hash,
                inventory.stacks[count - 1].source_hash
            );
        }
        let trace = native.runtime.take_event_log().await;
        let retained = native.runtime.create_checkpoint().await;
        replay.runtime.reset(&base.root).await.unwrap();
        replay.runtime.rig(trace).await.unwrap();
        let replay_inventory = decode_purse_inventory(
            &replay.runtime.reducer.space.get_data(&channel).await,
            &signatures[0],
        )
        .unwrap();
        assert_eq!(replay_inventory, inventory);
        apply_stack_pops(&mut replay, &replay_inventory.stacks, &pops)
            .await
            .unwrap();
        replay.runtime.check_replay_data().await.unwrap();
        assert_eq!(replay.runtime.create_checkpoint().await.root, retained.root);
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(256))]

        #[test]
        fn stack_capture_preflight_refines_complete_record_equality(
            seeds in proptest::collection::vec(proptest::prelude::any::<u8>(), 1..20),
            cell_count in 1usize..16,
            selector in proptest::prelude::any::<usize>(),
            mutation in 0usize..8,
        ) {
            use models::rhoapi::cost_signature::Value;

            let head = CostSignature { value: Some(Value::Ground(b"property-slot".to_vec())) };
            let channel = supply_channel(&Sig::Ground(b"property-slot".to_vec()));
            let data = seeds.iter().map(|seed| Datum::create(
                &channel,
                ListParWithRandom {
                    pars: Vec::new(),
                    random_state: vec![*seed],
                    cost_authority: None,
                    cost_stack: Some(CostStack { cells: vec![head.clone(); cell_count] }),
                },
                false,
            )).collect::<Vec<_>>();
            let inventory = decode_purse_inventory(&data, &head).unwrap();
            let index = selector % inventory.stacks.len();
            let mut selected = inventory.stacks[index].clone();
            match mutation {
                0 => {},
                1 => selected.instance_id[0] ^= 1,
                2 => selected.source_hash[0] ^= 1,
                3 => selected.channel = Par::default(),
                4 => selected.datum_index = -1,
                5 => selected.persistent = true,
                6 => selected.random_state.push(0),
                _ => { selected.stack.cells.pop(); },
            }
            let expected = inventory.stacks.iter().any(|stack| stack == &selected);
            proptest::prop_assert_eq!(check_stack_captures(&data, &[&selected]).is_ok(), expected);
            let mut batch = inventory.stacks.iter().collect::<Vec<_>>();
            batch.push(&selected);
            proptest::prop_assert_eq!(check_stack_captures(&data, &batch).is_ok(), expected);
            batch.reverse();
            proptest::prop_assert_eq!(check_stack_captures(&data, &batch).is_ok(), expected);
        }
    }

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
