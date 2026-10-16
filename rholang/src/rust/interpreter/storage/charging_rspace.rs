use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use rspace_plus_plus::rspace::checkpoint::{Checkpoint, SoftCheckpoint};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};
use rspace_plus_plus::rspace::rspace_interface::{
    ContResult, ISpace, MaybeConsumeResult, MaybeProduceResult, ProduceCommitGuard,
    RSpaceAccountingObserver, RSpaceResult,
};
use rspace_plus_plus::rspace::trace::event::Produce;
use rspace_plus_plus::rspace::trace::Log;
use rspace_plus_plus::rspace::util::unpack_option;

use crate::rust::interpreter::accounting::costs::{
    comm_event_storage_cost, event_storage_cost, storage_cost_consume, storage_cost_produce, Cost,
};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::host_work::HostWorkBudget;
use crate::rust::interpreter::rho_runtime::RhoISpace;

pub struct ChargingRSpace;

/// Adapter for an already-installed native replay host-work lease. It does
/// not choose an epoch or mint a budget; the adopted CostV2 policy must have
/// installed the finite `RuntimeBudget` lease before this is constructed.
pub(crate) struct NativeReplaySourceMeter(pub(crate) HostWorkBudget);

pub(crate) fn finite_native_replay_meter(
    host_work: HostWorkBudget,
) -> Result<NativeReplaySourceMeter, RSpaceError> {
    if host_work.is_rejected()
        || HostWorkDimension::ALL.iter().any(|dimension| {
            let cap = host_work.limits().get(*dimension).get();
            cap == 0 || cap == u64::MAX
        })
    {
        return Err(RSpaceError::HostWorkRejected);
    }
    Ok(NativeReplaySourceMeter(host_work))
}

impl SourceMeter for NativeReplaySourceMeter {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), RSpaceError> {
        for (dimension, amount) in [
            (HostWorkDimension::VerificationOperations, operations),
            (HostWorkDimension::VerificationBytes, scanned),
            (HostWorkDimension::SearchStateBytes, backing),
        ] {
            let units = u64::try_from(amount).map_err(|_| RSpaceError::HostWorkRejected)?;
            self.0
                .reserve(dimension, HostWorkUnits::new(units))
                .map_err(|_| RSpaceError::HostWorkRejected)?;
        }
        Ok(())
    }
}

pub(crate) fn native_replay_meter(
    cost: &RuntimeBudget,
) -> Result<NativeReplaySourceMeter, RSpaceError> {
    if cost.native_event_codec_epoch() != Some(EventCodecEpoch::CostV2)
        || !cost.native_execution_active()
        || !cost.has_comm_accounting_scope()
        || cost.is_unmetered()
    {
        return Err(RSpaceError::HostWorkRejected);
    }
    finite_native_replay_meter(
        cost.native_host_work()
            .ok_or(RSpaceError::HostWorkRejected)?,
    )
}

#[derive(Clone)]
pub enum TriggeredBy {
    Consume {
        id: Vec<u8>,
        persistent: bool,
        channels_count: i64,
    },
    Produce {
        id: Vec<u8>,
        persistent: bool,
        channels_count: i64,
    },
}

fn consume_id_bytes(continuation: &TaggedContinuation) -> Result<Vec<u8>, InterpreterError> {
    match continuation.tagged_cont.as_ref().unwrap() {
        TaggedCont::ParBody(par_with_random) => Ok(par_with_random.random_state.clone()),
        TaggedCont::ScalaBodyRef(value) => Ok(value.to_be_bytes().to_vec()),
    }
}

impl ChargingRSpace {
    pub fn charging_rspace(
        space: RhoISpace,
        cost: RuntimeBudget,
    ) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> + Clone {
        #[derive(Clone)]
        struct ChargingRSpace {
            space: RhoISpace,
            cost: RuntimeBudget,
        }

        #[async_trait]
        impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> for ChargingRSpace {
            fn codec_epoch(&self) -> EventCodecEpoch {
                self.space.codec_epoch()
            }

            fn set_accounting_observer(
                &self,
                observer: Option<
                    Arc<
                        dyn RSpaceAccountingObserver<
                            Par,
                            BindPattern,
                            ListParWithRandom,
                            TaggedContinuation,
                        >,
                    >,
                >,
            ) {
                self.space.set_accounting_observer(observer);
            }

            async fn consume(
                &self,
                channels: Vec<Par>,
                patterns: Vec<BindPattern>,
                continuation: TaggedContinuation,
                persist: bool,
                peeks: BTreeSet<i32>,
            ) -> Result<
                MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
                RSpaceError,
            > {
                if !self.cost.is_legacy() {
                    return self
                        .space
                        .consume(channels, patterns, continuation, persist, peeks)
                        .await;
                }
                self.cost.charge_legacy(storage_cost_consume(
                    &channels,
                    &patterns,
                    &continuation,
                ))?;

                let id = consume_id_bytes(&continuation)?;
                let channels_count = channels.len() as i64;
                let consume_res = self
                    .space
                    .consume(channels, patterns, continuation, persist, peeks)
                    .await?;

                handle_result(
                    consume_res.clone(),
                    TriggeredBy::Consume {
                        id,
                        persistent: persist,
                        channels_count,
                    },
                    self.cost.clone(),
                )?;
                Ok(consume_res)
            }

            async fn produce(
                &self,
                channel: Par,
                data: ListParWithRandom,
                persist: bool,
            ) -> Result<
                MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
                RSpaceError,
            > {
                if !self.cost.is_legacy() {
                    return self.space.produce(channel, data, persist).await;
                }
                self.cost
                    .charge_legacy(storage_cost_produce(&channel, &data))?;

                let id = data.random_state.clone();
                let produce_res = self.space.produce(channel, data, persist).await?;
                let common_result = produce_res
                    .clone()
                    .map(|(cont, data_list, _)| (cont, data_list));
                handle_result(
                    common_result,
                    TriggeredBy::Produce {
                        id,
                        persistent: persist,
                        channels_count: 1,
                    },
                    self.cost.clone(),
                )?;
                Ok(produce_res)
            }

            async fn produce_guarded(
                &self,
                channel: Par,
                data: ListParWithRandom,
                persist: bool,
                guard: &dyn ProduceCommitGuard,
            ) -> Result<
                MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
                RSpaceError,
            > {
                if !self.cost.is_legacy() {
                    return self
                        .space
                        .produce_guarded(channel, data, persist, guard)
                        .await;
                }
                self.cost
                    .charge_legacy(storage_cost_produce(&channel, &data))?;

                let id = data.random_state.clone();
                let produce_res = self
                    .space
                    .produce_guarded(channel, data, persist, guard)
                    .await?;
                let common_result = produce_res
                    .clone()
                    .map(|(cont, data_list, _)| (cont, data_list));
                handle_result(
                    common_result,
                    TriggeredBy::Produce {
                        id,
                        persistent: persist,
                        channels_count: 1,
                    },
                    self.cost.clone(),
                )?;
                Ok(produce_res)
            }

            async fn install(
                &self,
                channels: Vec<Par>,
                patterns: Vec<BindPattern>,
                continuation: TaggedContinuation,
            ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, RSpaceError>
            {
                self.space.install(channels, patterns, continuation).await
            }

            async fn create_checkpoint(&self) -> Result<Checkpoint, RSpaceError> {
                self.space.create_checkpoint().await
            }

            async fn get_data(
                &self,
                channel: &Par,
            ) -> Result<Vec<Datum<ListParWithRandom>>, RSpaceError> {
                self.space.get_data(channel).await
            }

            async fn get_data_metered(
                &self,
                channel: &Par,
                meter: &(dyn SourceMeter + Sync),
            ) -> Result<Vec<Datum<ListParWithRandom>>, RSpaceError> {
                self.space.get_data_metered(channel, meter).await
            }

            async fn get_data_cold_decode_metered(
                &self,
                channel: &Par,
                meter: &(dyn SourceMeter + Sync),
            ) -> Result<Vec<Datum<ListParWithRandom>>, RSpaceError> {
                self.space.get_data_cold_decode_metered(channel, meter).await
            }

            async fn get_waiting_continuations(
                &self,
                channels: Vec<Par>,
            ) -> Result<Vec<WaitingContinuation<BindPattern, TaggedContinuation>>, RSpaceError>
            {
                self.space.get_waiting_continuations(channels).await
            }

            async fn get_joins(&self, channel: Par) -> Result<Vec<Vec<Par>>, RSpaceError> {
                self.space.get_joins(channel).await
            }

            async fn remove_all_data(&self, channel: &Par) -> Result<(), RSpaceError> {
                self.space.remove_all_data(channel).await
            }

            async fn remove_data_at(&self, channel: &Par, index: i32) -> Result<(), RSpaceError> {
                self.space.remove_data_at(channel, index).await
            }

            async fn remove_data_at_recorded(
                &self,
                channel: &Par,
                index: i32,
                operation_id: &[u8],
            ) -> Result<(), RSpaceError> {
                self.space
                    .remove_data_at_recorded(channel, index, operation_id)
                    .await
            }

            async fn remove_all_continuations(
                &self,
                channels: Vec<Par>,
            ) -> Result<(), RSpaceError> {
                self.space.remove_all_continuations(channels).await
            }

            async fn clear(&self) -> Result<(), RSpaceError> { self.space.clear().await }

            async fn get_root(&self) -> Blake2b256Hash { self.space.get_root().await }

            async fn reset(&self, root: &Blake2b256Hash) -> Result<(), RSpaceError> {
                self.space.reset(root).await
            }

            async fn consume_result(
                &self,
                channel: Vec<Par>,
                pattern: Vec<BindPattern>,
            ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, RSpaceError>
            {
                let consume_res = self
                    .space
                    .consume(
                        channel,
                        pattern,
                        TaggedContinuation::default(),
                        false,
                        BTreeSet::new(),
                    )
                    .await?;
                Ok(unpack_option(&consume_res))
            }

            async fn to_map(
                &self,
            ) -> HashMap<Vec<Par>, Row<BindPattern, ListParWithRandom, TaggedContinuation>>
            {
                self.space.to_map().await
            }

            async fn create_soft_checkpoint(
                &self,
            ) -> SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation>
            {
                self.space.create_soft_checkpoint().await
            }

            async fn take_event_log(&self) -> Log { self.space.take_event_log().await }

            async fn revert_to_soft_checkpoint(
                &self,
                checkpoint: SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
            ) -> Result<(), RSpaceError> {
                self.space.revert_to_soft_checkpoint(checkpoint).await
            }

            async fn rig_and_reset(
                &self,
                start_root: Blake2b256Hash,
                log: Log,
            ) -> Result<(), RSpaceError> {
                self.space.rig_and_reset(start_root, log).await
            }

            async fn rig(&self, log: Log) -> Result<(), RSpaceError> { self.space.rig(log).await }

            async fn rig_cost_v2_metered(
                &self,
                start_root: Blake2b256Hash,
                log: Log,
                meter: &(dyn SourceMeter + Send + Sync),
            ) -> Result<(), RSpaceError> {
                if self.cost.native_event_codec_epoch() != Some(EventCodecEpoch::CostV2) {
                    return Err(RSpaceError::HostWorkRejected);
                }
                self.space.rig_cost_v2_metered(start_root, log, meter).await
            }

            async fn check_replay_data(&self) -> Result<(), RSpaceError> {
                self.space.check_replay_data().await
            }

            async fn is_replay(&self) -> bool { self.space.is_replay().await }

            async fn update_produce(&self, produce: Produce) -> () {
                self.space.update_produce(produce).await
            }

            async fn set_report_phase(
                &self,
                phase: rspace_plus_plus::rspace::reporting_rspace::ReportPhase,
            ) {
                self.space.set_report_phase(phase).await
            }
        }

        ChargingRSpace { space, cost }
    }
}

fn handle_result(
    result: MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    triggered_by: TriggeredBy,
    cost: RuntimeBudget,
) -> Result<(), InterpreterError> {
    let triggered_by_id_bytes = match triggered_by.clone() {
        TriggeredBy::Consume { id, .. } => id,
        TriggeredBy::Produce { id, .. } => id,
    };
    let triggered_by_channels_count = match triggered_by {
        TriggeredBy::Consume { channels_count, .. } => channels_count,
        TriggeredBy::Produce { .. } => 1,
    };
    let triggered_by_persistent = match triggered_by {
        TriggeredBy::Consume { persistent, .. } => persistent,
        TriggeredBy::Produce { persistent, .. } => persistent,
    };

    match result {
        Some((cont, data_list)) => {
            let consume_id_bytes = consume_id_bytes(&cont.continuation)?;

            let refund_for_consume =
                if !cont.persistent || consume_id_bytes == triggered_by_id_bytes {
                    storage_cost_consume(&cont.channels, &cont.patterns, &cont.continuation)
                } else {
                    Cost::create(0, "refund_for_consume")
                };

            let refund_for_produces =
                refund_for_removing_produces(data_list, cont.clone(), triggered_by);

            cost.charge_legacy(Cost::create(
                -refund_for_consume.value,
                "consume storage refund",
            ))?;
            cost.charge_legacy(Cost::create(
                -refund_for_produces.value,
                "produces storage refund",
            ))?;

            let last_iteration = !triggered_by_persistent;

            if last_iteration {
                cost.charge_legacy(event_storage_cost(triggered_by_channels_count))?;
            }

            cost.charge_legacy(comm_event_storage_cost(cont.channels.len() as i64))
        }
        None => cost.charge_legacy(event_storage_cost(triggered_by_channels_count)),
    }
}

fn refund_for_removing_produces(
    data_list: Vec<RSpaceResult<Par, ListParWithRandom>>,
    cont: ContResult<Par, BindPattern, TaggedContinuation>,
    triggered_by: TriggeredBy,
) -> Cost {
    let triggered_id_bytes = match triggered_by {
        TriggeredBy::Consume { id, .. } => id,
        TriggeredBy::Produce { id, .. } => id,
    };

    let removed_data: Vec<(RSpaceResult<Par, ListParWithRandom>, Par)> = data_list
        .into_iter()
        .zip(cont.channels.into_iter())
        .filter(|(data, _)| {
            !data.persistent || data.removed_datum.random_state == triggered_id_bytes
        })
        .collect();

    removed_data
        .into_iter()
        .map(|(data, channel)| storage_cost_produce(&channel, &data.removed_datum))
        .fold(
            Cost::create(0, "refund_for_removing_produces init"),
            |acc, cost| {
                Cost::create(
                    acc.value + cost.value,
                    "refund_for_removing_produces operation",
                )
            },
        )
}

#[cfg(test)]
mod native_replay_meter_tests {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use models::rust::utils::new_gstring_par;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::interpreter::deterministic_reduction::{DeterministicRSpace, ReductionCoordinator};
    use crate::rust::interpreter::matcher::r#match::Matcher;

    #[tokio::test]
    async fn metered_cold_read_crosses_both_runtime_adapters() {
        let mut manager = InMemoryStoreManager::new();
        let store = manager.r_space_stores().await.unwrap();
        let raw = RSpace::create(store, Arc::new(Box::new(Matcher))).unwrap();
        let raw: RhoISpace = Arc::new(Box::new(raw));
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let scheduled: RhoISpace = Arc::new(Box::new(DeterministicRSpace::new(
            raw,
            ReductionCoordinator::default(),
            budget.clone(),
        )));
        let charging = ChargingRSpace::charging_rspace(scheduled, budget);
        let meter = |_: usize, _: usize, _: usize| Ok::<(), RSpaceError>(());
        let channel = new_gstring_par("prepaid-receipt-test".into(), Vec::new(), false);
        assert!(charging
            .get_data_cold_decode_metered(&channel, &meter)
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn finite_replay_meter_charges_all_three_dimensions_and_refuses_exhaustion() {
        let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(2)));
        let meter = NativeReplaySourceMeter(host.clone());
        meter.reserve(1, 2, 1).unwrap();
        assert_eq!(
            host.usage(HostWorkDimension::VerificationOperations).get(),
            1
        );
        assert_eq!(host.usage(HostWorkDimension::VerificationBytes).get(), 2);
        assert_eq!(host.usage(HostWorkDimension::SearchStateBytes).get(), 1);
        assert!(matches!(
            meter.reserve(0, 1, 0),
            Err(RSpaceError::HostWorkRejected)
        ));
    }

    #[test]
    fn replay_meter_refuses_missing_or_unbounded_dimensions() {
        assert!(
            finite_native_replay_meter(HostWorkBudget::new(HostWorkLimits::uniform(
                HostWorkLimit::new(0),
            )))
            .is_err()
        );
        assert!(
            finite_native_replay_meter(HostWorkBudget::new(HostWorkLimits::uniform(
                HostWorkLimit::new(u64::MAX),
            )))
            .is_err()
        );
    }
}
