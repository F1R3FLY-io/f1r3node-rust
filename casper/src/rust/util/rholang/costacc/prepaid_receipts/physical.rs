use std::collections::BTreeMap;
use std::mem::size_of;

use models::rhoapi::{CostSignature, CostStack, ListParWithRandom, Par};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, funding_sig_channel, stack_branches, AuthorityStackBirth,
};
use rholang::rust::interpreter::accounting::native_phlo_rules::NativePhloAcquisitionDemand;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::native_reader::NativeLeafKind;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::serializers::serializers::decode_datum_for_epoch;

use super::snapshot::native_read_error;
use super::*;
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use crate::rust::util::rholang::costacc::supply::{decode_purse_inventory, PurseStack};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepaidStackCaptureLimits {
    pub stacks: usize,
    pub physical_cells: usize,
    pub physical_bytes: usize,
    pub bucket: PrepaidReceiptBucketLimits,
    pub cells: PrepaidCellLimits,
    pub native_cell: NativePrepaidCellLimits,
    pub receipts: PrepaidReceiptLimits,
}

#[derive(Debug)]
pub struct CapturedPrepaidStacks<'a> {
    policy: &'a AdoptedResourcePolicy,
    stacks: Vec<&'a PurseStack>,
    receipts: PrepaidReceiptSnapshot,
    bucket_limits: PrepaidReceiptBucketLimits,
}

impl<'a> CapturedPrepaidStacks<'a> {
    pub fn policy(&self) -> &'a AdoptedResourcePolicy { self.policy }

    pub fn root(&self) -> [u8; 32] { self.receipts.root() }

    pub fn stacks(&self) -> &[&'a PurseStack] { &self.stacks }

    pub(super) fn receipts(&self) -> &PrepaidReceiptSnapshot { &self.receipts }

    pub fn records_for_stack(
        &self,
        expected_root: &[u8; 32],
        stack_id: &[u8; 32],
    ) -> Result<Option<PrepaidReceiptBucket<'_>>, CasperError> {
        if *expected_root != self.root() {
            return Err(invalid(
                "physical receipt capture belongs to another state root",
            ));
        }
        let Ok(index) = self
            .stacks
            .binary_search_by_key(stack_id, |stack| stack.instance_id)
        else {
            return Ok(None);
        };
        let key = PrepaidReceiptBucket::key_for_source(&self.stacks[index].source_hash);
        let bytes = self
            .receipts
            .receipt(expected_root, &key)?
            .flatten()
            .ok_or_else(|| invalid("captured physical source has no receipt"))?;
        Ok(Some(PrepaidReceiptBucket::decode(
            bytes,
            self.bucket_limits,
        )?))
    }
}

fn charge_bytes(bytes: usize, remaining: &mut usize) -> Result<(), CasperError> {
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or_else(|| invalid("physical capture byte limit exceeded"))?;
    Ok(())
}

fn measured_stack_cell_count(stack: &CostStack) -> Result<usize, CasperError> {
    if stack.parallel_tail.is_empty() {
        return Ok(stack.cells.len());
    }
    let mut count = 0usize;
    for branch in stack_branches([0; 32], stack).map_err(|error| invalid(&error.to_string()))? {
        count = count
            .checked_add(branch.stack.cells.len())
            .ok_or_else(|| invalid("physical stack cell count overflow"))?;
    }
    Ok(count)
}

fn measured_birth_channel(head: &CostSignature) -> Result<(Vec<u8>, Par), CasperError> {
    let signature = cost_signature_to_sig(head).map_err(|error| invalid(&error.to_string()))?;
    if !signature.is_funding_former() {
        return Err(invalid("measured birth head is not a funding former"));
    }
    let channel = funding_sig_channel(&signature).map_err(|error| invalid(&error.to_string()))?;
    let encoded_len = channel.encoded_len();
    let mut key = Vec::new();
    key.try_reserve_exact(encoded_len)
        .map_err(|_| invalid("measured birth channel allocation failed"))?;
    channel
        .encode(&mut key)
        .map_err(|_| invalid("measured birth channel encoding failed"))?;
    Ok((key, channel))
}

impl RuntimeManager {
    pub fn read_measured_prepaid_stacks(
        &self,
        pre_state_root: [u8; 32],
        demand: &NativePhloAcquisitionDemand<'_>,
        policy: &AdoptedResourcePolicy,
        limits: PrepaidStackCaptureLimits,
    ) -> Result<Vec<PurseStack>, CasperError> {
        self.read_measured_prepaid_stacks_with_births(pre_state_root, demand, &[], policy, limits)
    }

    pub fn read_measured_prepaid_stacks_with_births(
        &self,
        pre_state_root: [u8; 32],
        demand: &NativePhloAcquisitionDemand<'_>,
        births: &[AuthorityStackBirth],
        policy: &AdoptedResourcePolicy,
        limits: PrepaidStackCaptureLimits,
    ) -> Result<Vec<PurseStack>, CasperError> {
        let root = Blake2b256Hash::from_bytes(pre_state_root.to_vec());
        if !self
            .history_repo
            .contains_root(&root)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?
        {
            return Err(invalid(
                "measured purse capture requires a registered state root",
            ));
        }
        let mut bytes_left = limits.physical_bytes;
        let mut channels = BTreeMap::<&[u8], (&Par, &CostSignature)>::new();
        for (located, _) in demand.occurrences() {
            let purse = located.purse();
            if !purse.authority().is_funding_former() {
                return Err(invalid("measured purse authority is not a funding former"));
            }
            let key = purse.encoded_channel();
            // Changed by H2 (bug 11419, DR-127): the capture keeps the first
            // head of each channel. A head of another form names the same
            // channel and so the same lane, and decoding reads only the lane
            // (`decode_purse_inventory`). The cell match in the selection still
            // compares forms.
            // if let Some((_, previous)) = channels.get(key) {
            //     if *previous != purse.original_authority() {
            //         return Err(invalid("one measured purse channel has conflicting heads"));
            //     }
            //     continue;
            // }
            if channels.contains_key(key) {
                continue;
            }
            if channels.len() >= limits.stacks {
                return Err(invalid("measured purse channel limit exceeded"));
            }
            charge_bytes(
                key.len()
                    .checked_add(size_of::<(&Par, &CostSignature)>())
                    .and_then(|bytes| bytes.checked_add(128))
                    .ok_or_else(|| invalid("measured purse channel size overflow"))?,
                &mut bytes_left,
            )?;
            channels.insert(key, (purse.channel(), purse.original_authority()));
        }
        let mut born_channels = BTreeMap::<Vec<u8>, (Par, &CostSignature)>::new();
        for birth in births {
            let head = birth
                .cells
                .first()
                .ok_or_else(|| invalid("measured birth has no stack head"))?;
            let (key, channel) = measured_birth_channel(head)?;
            // Changed by H2 (bug 11419, DR-127): a birth head of another form
            // keeps the first head of its channel, as a measured head does.
            // if let Some((_, previous)) = channels.get(key.as_slice()) {
            //     if *previous != head {
            //         return Err(invalid("one measured purse channel has conflicting heads"));
            //     }
            //     continue;
            // }
            // if let Some((_, previous)) = born_channels.get(key.as_slice()) {
            //     if *previous != head {
            //         return Err(invalid("one measured purse channel has conflicting heads"));
            //     }
            //     continue;
            // }
            if channels.contains_key(key.as_slice()) || born_channels.contains_key(key.as_slice()) {
                continue;
            }
            if channels
                .len()
                .checked_add(born_channels.len())
                .ok_or_else(|| invalid("measured purse channel count overflow"))?
                >= limits.stacks
            {
                return Err(invalid("measured purse channel limit exceeded"));
            }
            charge_bytes(
                key.len()
                    .checked_add(size_of::<(Par, &CostSignature)>())
                    .and_then(|bytes| bytes.checked_add(128))
                    .ok_or_else(|| invalid("measured birth channel size overflow"))?,
                &mut bytes_left,
            )?;
            born_channels.insert(key, (channel, head));
        }
        let reader = self.history_repo.native_history_reader(pre_state_root);
        let mut stacks_left = limits.stacks;
        let mut cells_left = limits.physical_cells;
        let mut stacks = Vec::new();
        for (channel, head) in channels.into_values().chain(
            born_channels
                .values()
                .map(|(channel, head)| (channel, *head)),
        ) {
            let channel_hash = stable_hash_provider::hash(channel);
            let channel_hash: [u8; 32] = channel_hash
                .0
                .as_slice()
                .try_into()
                .map_err(|_| invalid("measured purse channel hash is invalid"))?;
            let data = reader
                .with_records(NativeLeafKind::Data, &channel_hash, |records| {
                    if records.len() > stacks_left {
                        return Err(invalid("measured purse stack limit exceeded"));
                    }
                    let mut data: Vec<Datum<ListParWithRandom>> = Vec::new();
                    data.try_reserve_exact(records.len())
                        .map_err(|_| invalid("measured purse allocation failed"))?;
                    let mut raw_left = bytes_left;
                    for raw in records.iter() {
                        raw_left = raw_left
                            .checked_sub(raw.len())
                            .ok_or_else(|| invalid("measured purse byte limit exceeded"))?;
                        data.push(
                            decode_datum_for_epoch(policy.event_codec_epoch(), raw)
                                .map_err(|error| native_read_error(error.into()))?,
                        );
                    }
                    Ok(data)
                })
                .map_err(native_read_error)?
                .unwrap_or_default();
            stacks_left = stacks_left
                .checked_sub(data.len())
                .ok_or_else(|| invalid("measured purse stack limit exceeded"))?;
            for datum in &data {
                if let Some(stack) = &datum.a.cost_stack {
                    let cells = measured_stack_cell_count(stack)?;
                    cells_left = cells_left
                        .checked_sub(cells)
                        .ok_or_else(|| invalid("measured purse cell limit exceeded"))?;
                }
                charge_bytes(
                    datum
                        .a
                        .encoded_len()
                        .checked_add(channel.encoded_len())
                        .and_then(|bytes| bytes.checked_add(size_of::<PurseStack>()))
                        .ok_or_else(|| invalid("measured purse byte size overflow"))?,
                    &mut bytes_left,
                )?;
            }
            let inventory = decode_purse_inventory(&data, head)?;
            stacks
                .try_reserve(inventory.stacks.len())
                .map_err(|_| invalid("measured purse allocation failed"))?;
            stacks.extend(inventory.stacks);
        }
        stacks.sort_unstable_by_key(|stack| stack.instance_id);
        if stacks
            .windows(2)
            .any(|pair| pair[0].instance_id == pair[1].instance_id)
        {
            return Err(invalid(
                "measured purse capture has duplicate stack identity",
            ));
        }
        Ok(stacks)
    }

    pub fn capture_prepaid_stacks<'a>(
        &self,
        pre_state_root: [u8; 32],
        selected: &'a [PurseStack],
        policy: &'a AdoptedResourcePolicy,
        limits: PrepaidStackCaptureLimits,
    ) -> Result<CapturedPrepaidStacks<'a>, CasperError> {
        if selected.len() > limits.stacks {
            return Err(invalid("physical capture stack limit exceeded"));
        }
        let mut bytes_left = limits.physical_bytes;
        let mut channels = BTreeMap::<Vec<u8>, Vec<&PurseStack>>::new();
        let mut ordered = Vec::new();
        charge_bytes(
            selected
                .len()
                .checked_mul(size_of::<&PurseStack>())
                .ok_or_else(|| invalid("physical capture size overflow"))?,
            &mut bytes_left,
        )?;
        ordered
            .try_reserve_exact(selected.len())
            .map_err(|_| invalid("physical capture allocation failed"))?;
        for stack in selected {
            if stack.stack.cells.is_empty() || stack.stack.cells.len() > limits.physical_cells {
                return Err(invalid("physical capture cell count is out of range"));
            }
            let bytes = stack
                .channel
                .encoded_len()
                .checked_add(stack.stack.encoded_len())
                .and_then(|n| n.checked_add(stack.random_state.len()))
                .and_then(|n| n.checked_add(size_of::<PurseStack>()))
                .ok_or_else(|| invalid("physical capture size overflow"))?;
            charge_bytes(bytes, &mut bytes_left)?;
            channels
                .entry(stack.channel.encode_to_vec())
                .or_default()
                .push(stack);
            ordered.push(stack);
        }
        ordered.sort_unstable_by_key(|stack| stack.instance_id);
        if ordered
            .windows(2)
            .any(|pair| pair[0].instance_id == pair[1].instance_id)
        {
            return Err(invalid("duplicate physical capture identity"));
        }
        let root = Blake2b256Hash::from_bytes(pre_state_root.to_vec());
        if !self
            .history_repo
            .contains_root(&root)
            .map_err(|e| CasperError::RuntimeError(e.to_string()))?
        {
            return Err(CasperError::RuntimeError(
                "physical capture requires a registered state root".into(),
            ));
        }
        let reader = self.history_repo.native_history_reader(pre_state_root);
        let mut sources = BTreeMap::<[u8; 32], (usize, usize)>::new();
        let mut stacks_left = limits.stacks;
        let mut cells_left = limits.physical_cells;
        for (channel_bytes, captures) in &channels {
            let first = captures[0];
            let channel_hash = stable_hash_provider::hash(&first.channel);
            let channel_hash: [u8; 32] = channel_hash
                .0
                .as_slice()
                .try_into()
                .map_err(|_| invalid("physical channel hash is invalid"))?;
            let data = reader
                .with_records(NativeLeafKind::Data, &channel_hash, |records| {
                    if records.len() > stacks_left {
                        return Err(invalid("physical inventory stack limit exceeded"));
                    }
                    let mut data: Vec<Datum<ListParWithRandom>> = Vec::new();
                    data.try_reserve_exact(records.len())
                        .map_err(|_| invalid("physical inventory allocation failed"))?;
                    let mut raw_left = bytes_left;
                    for raw in records.iter() {
                        raw_left = raw_left
                            .checked_sub(raw.len())
                            .ok_or_else(|| invalid("physical inventory byte limit exceeded"))?;
                        data.push(
                            decode_datum_for_epoch(policy.event_codec_epoch(), raw)
                                .map_err(|error| native_read_error(error.into()))?,
                        );
                    }
                    Ok(data)
                })
                .map_err(native_read_error)?
                .unwrap_or_default();
            stacks_left = stacks_left
                .checked_sub(data.len())
                .ok_or_else(|| invalid("physical inventory stack limit exceeded"))?;
            for datum in &data {
                if let Some(stack) = &datum.a.cost_stack {
                    let cells = measured_stack_cell_count(stack)?;
                    cells_left = cells_left
                        .checked_sub(cells)
                        .ok_or_else(|| invalid("physical inventory cell limit exceeded"))?;
                }
                charge_bytes(
                    datum
                        .a
                        .encoded_len()
                        .checked_add(channel_bytes.len())
                        .and_then(|n| n.checked_add(size_of::<PurseStack>()))
                        .ok_or_else(|| invalid("physical inventory size overflow"))?,
                    &mut bytes_left,
                )?;
            }
            let inventory = decode_purse_inventory(&data, &first.stack.cells[0])?;
            let by_index = inventory
                .stacks
                .iter()
                .map(|stack| (stack.datum_index, stack))
                .collect::<BTreeMap<_, _>>();
            let mut counts = BTreeMap::<[u8; 32], (usize, usize)>::new();
            for stack in &inventory.stacks {
                let cell_count = measured_stack_cell_count(&stack.stack)?;
                let entry = counts.entry(stack.source_hash).or_insert((0, cell_count));
                entry.0 += 1;
                if entry.1 != cell_count {
                    return Err(invalid("one physical source has conflicting cell counts"));
                }
            }
            for captured in captures {
                if by_index.get(&captured.datum_index).copied() != Some(*captured) {
                    return Err(invalid(
                        "physical capture differs from the complete rooted inventory",
                    ));
                }
                let count = counts[&captured.source_hash];
                if sources
                    .insert(captured.source_hash, count)
                    .is_some_and(|previous| previous != count)
                {
                    return Err(invalid("conflicting physical source captures"));
                }
            }
        }
        if sources.len() > limits.receipts.entries {
            return Err(invalid("physical capture receipt limit exceeded"));
        }
        let mut keys = Vec::new();
        keys.try_reserve_exact(sources.len())
            .map_err(|_| invalid("physical receipt key allocation failed"))?;
        keys.extend(sources.keys().map(PrepaidReceiptBucket::key_for_source));
        let receipts = self.capture_prepaid_receipts(
            pre_state_root,
            policy.event_codec_epoch(),
            &keys,
            limits.receipts,
        )?;
        let mut record_cells_left = limits.physical_cells;
        for (source, (occurrences, cells)) in sources {
            let key = PrepaidReceiptBucket::key_for_source(&source);
            let bytes = receipts
                .receipt(&pre_state_root, &key)?
                .flatten()
                .ok_or_else(|| invalid("physical source has no prepaid receipt"))?;
            let bucket = PrepaidReceiptBucket::decode(bytes, limits.bucket)?;
            bucket.check_occurrences(&source, occurrences)?;
            for record in bucket.receipts() {
                let ordered_cells = OrderedPrepaidCells::decode(record, limits.cells)?;
                if ordered_cells.cells().len() != cells {
                    return Err(invalid(
                        "prepaid record differs from the complete physical cell count",
                    ));
                }
                record_cells_left = record_cells_left
                    .checked_sub(cells)
                    .ok_or_else(|| invalid("physical receipt cell limit exceeded"))?;
                for cell in ordered_cells.cells() {
                    NativePrepaidCell::decode(policy, cell, limits.native_cell)?;
                }
            }
        }
        Ok(CapturedPrepaidStacks {
            policy,
            stacks: ordered,
            receipts,
            bucket_limits: limits.bucket,
        })
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value as CostSignatureValue;

    use super::*;

    /// The measured birth channel of a head is the funding channel of its
    /// signature, and its key is the encoding of that channel. DR-131 removed
    /// the host-work charges that D-E3 (DR-110) tested here.
    #[test]
    fn measured_birth_channel_is_the_funding_channel_of_the_head() {
        let head = CostSignature {
            value: Some(CostSignatureValue::Ground(vec![9; 32])),
        };
        let (key, channel) = measured_birth_channel(&head).expect("a birth channel");
        let signature = cost_signature_to_sig(&head).expect("a signature");
        let expected = funding_sig_channel(&signature).expect("a channel");
        assert_eq!(channel, expected);
        assert_eq!(key, expected.encode_to_vec());
    }

    // Added by H2 (bug 11419, DR-127): the capture keeps the first head of
    // each measured channel, so its result does not depend on the form that
    // comes first.

    use std::sync::Arc;

    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{CostAuthority, CostStack, Expr};
    use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
    use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
    use models::rust::rholang::sorter::sortable::Sortable;
    use proptest::prelude::*;
    use rholang::rust::interpreter::accounting::authority::{
        cost_region, cost_signature_to_sig, AuthorityByteEventKind,
    };
    use rholang::rust::interpreter::accounting::byte_accounting::ByteCharge;
    use rholang::rust::interpreter::accounting::byte_receipts::{
        ByteObservation, ByteObservationSnapshot,
    };
    use rholang::rust::interpreter::accounting::native_phlo_rules::{
        native_resource_compatibility_rule, NativePhloDimension, NativePhloRules,
    };
    use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
    use rholang::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use crate::rust::util::rholang::costacc::production_limits::offered_funded_v6_production_limits;
    use crate::rust::util::rholang::costacc::supply::supply_channel;

    /// The two authority forms of one lane: the ground atom that `[lane]`
    /// gives (`sig.rs`) and the quoted process that a received `@"lane"`
    /// gives (`substitute.rs`).
    fn forms(lane: &str) -> [CostSignature; 2] {
        let ground = CostSignature {
            value: Some(CostSignatureValue::Ground(canon_ground(lane))),
        };
        let quoted = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString(lane.to_string())),
        }]);
        let name = CostSignature {
            value: Some(CostSignatureValue::Name(
                ParSortMatcher::sort_match(&quoted).term,
            )),
        };
        [ground, name]
    }

    fn schedule() -> PhloScheduleV1<'static> {
        let identities: [&[u8]; 4] = [b"compute", b"introduction", b"transfer", b"trace"];
        PhloScheduleV1 {
            protocol_version: 6,
            network: b"test",
            shard: b"root",
            settlement_asset: b"REV",
            settlement_unit: b"atomic-REV",
            decimal_scale: 8,
            classes: NativePhloDimension::ALL
                .into_iter()
                .zip(identities)
                .map(|(dimension, identity)| dimension.resource_class(identity, 1))
                .collect(),
            actual_price: 2,
            compatibility_rule: native_resource_compatibility_rule(),
        }
    }

    /// One measured introduction for each signature, in the given order. Each
    /// region has its own entropy, so its identity is distinct.
    fn observations(signatures: &[&CostSignature]) -> ByteObservationSnapshot {
        let rows = signatures
            .iter()
            .enumerate()
            .map(|(index, signature)| {
                let tag = u8::try_from(index + 1).expect("few observations");
                Arc::new(ByteObservation {
                    event_id: [tag; 32],
                    kind: AuthorityByteEventKind::ProduceIntroduction,
                    authority: CostAuthority {
                        regions: vec![
                            cost_region(signature, &[tag], 0).expect("a region of the lane")
                        ],
                    },
                    measurement: Some(ByteCharge {
                        introduction_bytes: 3,
                        transfer_bytes: 0,
                        trace_bytes: 0,
                    }),
                    legacy_amount: None,
                })
            })
            .collect();
        ByteObservationSnapshot {
            rows,
            metered_context: true,
            history_lost: false,
        }
    }

    /// The capture of a run whose measured occurrences carry `measured` in
    /// order and whose births carry `born` heads in order. The state root
    /// holds `stored` stacks on the lane's channel.
    async fn capture(
        lane: &str,
        stored: usize,
        measured: &[&CostSignature],
        born: &[&CostSignature],
    ) -> Result<Vec<PurseStack>, CasperError> {
        let limits = offered_funded_v6_production_limits();
        let measured_limits = limits.family.measured;
        let store = InMemoryStoreManager::new()
            .r_space_stores()
            .await
            .expect("in-memory stores open");
        let (manager, _) = RuntimeManager::create_with_history(
            store,
            KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            Arc::new(Default::default()),
            ExternalServices::noop(),
        );
        let mut runtime = manager.spawn_runtime().await.expect("a runtime spawns");
        let [ground, _] = forms(lane);
        let channel = supply_channel(&cost_signature_to_sig(&ground).expect("a lane signature"));
        for random in 0..stored {
            runtime
                .reducer
                .space
                .produce(
                    channel.clone(),
                    ListParWithRandom {
                        pars: Vec::new(),
                        random_state: vec![u8::try_from(random).expect("few stacks")],
                        cost_authority: None,
                        cost_stack: Some(CostStack {
                            cells: vec![ground.clone()],
                            parallel_tail: Vec::new(),
                        }),
                    },
                    false,
                )
                .await
                .expect("a stack is stored");
        }
        let root: [u8; 32] = runtime
            .create_checkpoint()
            .await
            .root
            .bytes()
            .try_into()
            .expect("a 32-byte root");
        let schedule = schedule();
        let rules = NativePhloRules::resolve(&schedule).expect("the schedule resolves");
        let snapshot = observations(measured);
        let measurements = rules
            .measure(&snapshot, measured_limits.observations)
            .expect("the observations measure");
        let regions = measurements
            .region_demands(measured_limits.regions)
            .expect("the regions check");
        let located = regions
            .locate_purses(measured_limits.purses)
            .expect("the purses locate");
        let binding = PhloScheduleBinding::new(&schedule, PhloGenesisPolicy::LIMITS)
            .expect("the schedule binds");
        let terms = schedule
            .encode(PhloGenesisPolicy::LIMITS)
            .expect("the schedule encodes");
        let demand = located
            .prepare_acquisition_demand(&binding, &terms, measured_limits.acquisition)
            .expect("the demand prepares");
        let births = born
            .iter()
            .enumerate()
            .map(|(index, head)| AuthorityStackBirth {
                produce_hash: [u8::try_from(index + 1).expect("few births"); 32],
                cells: vec![(*head).clone()],
                stack: None,
            })
            .collect::<Vec<_>>();
        manager.read_measured_prepaid_stacks_with_births(
            root,
            &demand,
            &births,
            &crate::rust::util::rholang::costacc::genesis_resource_policy::tests::retained_record_context(),
            limits.stack_capture,
        )
    }

    /// Added by H2 (bug 11419, DR-127): both forms of one lane name one
    /// channel and one lane, so the capture reads one channel for them.
    #[test]
    fn both_authority_forms_of_a_lane_name_one_channel() {
        let [ground, name] = forms("foo");
        assert_ne!(ground, name, "the forms differ");
        let (ground_key, _) = measured_birth_channel(&ground).expect("a ground channel");
        let (name_key, _) = measured_birth_channel(&name).expect("a name channel");
        assert_eq!(ground_key, name_key);
        assert_eq!(
            cost_signature_to_sig(&ground)
                .expect("a ground lane")
                .lane_hash(),
            cost_signature_to_sig(&name)
                .expect("a name lane")
                .lane_hash()
        );
    }

    /// Added by H2 (bug 11419, DR-127): a run that measures both forms of one
    /// lane captures the stacks of that lane. Before H2, the capture failed
    /// with "one measured purse channel has conflicting heads".
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_run_with_both_forms_of_a_lane_captures_its_stacks() {
        let [ground, name] = forms("foo");
        let both = capture("foo", 2, &[&ground, &name], &[])
            .await
            .expect("the capture accepts both forms");
        let one = capture("foo", 2, &[&ground], &[])
            .await
            .expect("the capture accepts one form");
        assert_eq!(both.len(), 2);
        assert_eq!(both, one);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// Added by H2 (bug 11419, DR-127): the capture result does not depend
        /// on the form that comes first, among the measured heads and the
        /// birth heads. It equals the capture of the ground form alone.
        #[test]
        fn the_capture_does_not_depend_on_the_first_form(
            lane in "[a-z]{1,6}",
            stored in 0_usize..3,
            measured in proptest::collection::vec(any::<bool>(), 1..4),
            born in proptest::collection::vec(any::<bool>(), 0..3),
        ) {
            let runtime = tokio::runtime::Runtime::new().expect("a test runtime starts");
            let both = forms(&lane);
            let pick = |names: &[bool]| {
                names
                    .iter()
                    .map(|name| &both[usize::from(*name)])
                    .collect::<Vec<_>>()
            };
            let flip = |names: &[bool]| names.iter().map(|name| !name).collect::<Vec<_>>();
            let given = runtime.block_on(capture(&lane, stored, &pick(&measured), &pick(&born)));
            let flipped = runtime.block_on(capture(
                &lane,
                stored,
                &pick(&flip(&measured)),
                &pick(&flip(&born)),
            ));
            let ground = runtime.block_on(capture(&lane, stored, &[&both[0]], &[]));
            prop_assert!(given.is_ok(), "{given:?}");
            prop_assert!(flipped.is_ok(), "{flipped:?}");
            prop_assert!(ground.is_ok(), "{ground:?}");
            let given = given.expect("checked above");
            prop_assert_eq!(&given, &flipped.expect("checked above"));
            prop_assert_eq!(&given, &ground.expect("checked above"));
            prop_assert_eq!(given.len(), stored);
        }
    }
}
