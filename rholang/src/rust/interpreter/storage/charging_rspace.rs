// See rholang/src/main/scala/coop/rchain/rholang/interpreter/storage/ChargingRSpace.scala

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::checkpoint::{Checkpoint, SoftCheckpoint};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hot_store::HotStore;
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};
use rspace_plus_plus::rspace::rspace_interface::{
    ContResult, ISpace, MaybeConsumeResult, MaybeProduceResult, RSpaceResult,
};
use rspace_plus_plus::rspace::trace::event::Produce;
use rspace_plus_plus::rspace::trace::Log;
use rspace_plus_plus::rspace::util::unpack_option;

use crate::rust::interpreter::accounting::_cost;
use crate::rust::interpreter::accounting::comm_ledger::comm_cells;
use crate::rust::interpreter::accounting::membrane;
use crate::rust::interpreter::accounting::rev_stack::seal;
use crate::rust::interpreter::accounting::costs::{
    comm_event_storage_cost, event_storage_cost, storage_cost_consume, storage_cost_produce, Cost,
};
use crate::rust::interpreter::errors::InterpreterError;

pub struct ChargingRSpace;

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

//TODO: Make ScalaBodyRef-s have their own random state and merge it during its COMMs - OLD
fn consume_id_bytes(continuation: &TaggedContinuation) -> Result<Vec<u8>, InterpreterError> {
    match continuation.tagged_cont.as_ref().unwrap() {
        TaggedCont::ParBody(par_with_random) => Ok(par_with_random.random_state.clone()),
        TaggedCont::ScalaBodyRef(value) => Ok(value.to_be_bytes().to_vec()),
    }
}

impl ChargingRSpace {
    pub fn charging_rspace<T>(
        space: T,
        cost: _cost,
    ) -> impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> + Clone
    where
        T: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> + Clone,
    {
        #[derive(Clone)]
        struct ChargingRSpace<T> {
            space: T,
            cost: _cost,
        }

        #[async_trait]
        impl<T: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>
            ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> for ChargingRSpace<T>
        {
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
                let consume_storage = storage_cost_consume(&channels, &patterns, &continuation);
                let channels_count = channels.len() as i64;
                let left = event_storage_cost(channels_count).value;
                self.cost.charge_reserving(consume_storage.clone(), left)?;
                let follow = left
                    + if persist {
                        consume_storage.value + left
                    } else {
                        0
                    };

                let id = consume_id_bytes(&continuation)?;
                if self.cost.storage_left_in_space().is_some() {
                    let consume_res = membrane::following(
                        follow,
                        self.space
                            .consume(channels, patterns, continuation, persist, peeks),
                    )
                    .await?;
                    match consume_res.as_ref() {
                        Some((cont, data_list)) => {
                            left_in_space_match(cont, data_list, &Trigger::Consume, &self.cost, follow)?
                        }
                        None => match with_rule(&self.cost, |rule| rule.left_continuation(&id, seal::current())) {
                            true => membrane::settling(|| self.cost.charge(event_storage_cost(channels_count)))?,
                            false => refund(&self.cost, consume_storage.value, "consume storage refund")?,
                        },
                    }
                    return Ok(consume_res);
                }
                let consume_res = membrane::following(
                    follow,
                    self.space
                        .consume(channels, patterns, continuation, persist, peeks),
                )
                .await?;

                handle_result(
                    consume_res
                        .as_ref()
                        .map(|(cont, data_list)| (cont, data_list.as_slice())),
                    TriggeredBy::Consume {
                        id,
                        persistent: persist,
                        channels_count,
                    },
                    self.cost.clone(),
                    follow,
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
                if self.cost.storage_left_in_space().is_some() {
                    let id = data.random_state.clone();
                    let restored = with_rule(&self.cost, |rule| rule.restores(&id));
                    let produce_storage = storage_cost_produce(&channel, &data);
                    let left = event_storage_cost(1).value;
                    if !restored {
                        self.cost.charge_reserving(produce_storage.clone(), left)?;
                    }
                    let follow = left
                        + if persist {
                            produce_storage.value + left
                        } else {
                            0
                        };
                    with_rule(&self.cost, |rule| rule.announce_datum(&id, seal::current()));
                    let produce_res =
                        membrane::following(follow, self.space.produce(channel, data, persist))
                            .await?;
                    match produce_res.as_ref() {
                        Some((cont, data_list, _)) => left_in_space_match(
                            cont,
                            data_list,
                            &Trigger::Produce { id: id.clone(), restored },
                            &self.cost,
                            follow,
                        )?,
                        None if !restored => match with_rule(&self.cost, |rule| rule.left_datum(&id, seal::current())) {
                            true => membrane::settling(|| self.cost.charge(event_storage_cost(1)))?,
                            false => refund(&self.cost, produce_storage.value, "produces storage refund")?,
                        },
                        None => {}
                    }
                    with_rule(&self.cost, |rule| rule.withdraw_announcement(&id));
                    return Ok(produce_res);
                }

                let produce_storage = storage_cost_produce(&channel, &data);
                let left = event_storage_cost(1).value;
                self.cost.charge_reserving(produce_storage.clone(), left)?;
                let follow = left
                    + if persist {
                        produce_storage.value + left
                    } else {
                        0
                    };

                let id = data.random_state.clone();
                let produce_res =
                    membrane::following(follow, self.space.produce(channel, data, persist)).await?;
                handle_result(
                    produce_res
                        .as_ref()
                        .map(|(cont, data_list, _)| (cont, data_list.as_slice())),
                    TriggeredBy::Produce {
                        id,
                        persistent: persist,
                        channels_count: 1,
                    },
                    self.cost.clone(),
                    follow,
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

            async fn get_data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>> {
                self.space.get_data(channel).await
            }

            fn hot_store(
                &self,
            ) -> Option<
                Arc<Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>,
            > {
                self.space.hot_store()
            }

            async fn get_data_at(
                &self,
                checkpoint: &SoftCheckpoint<
                    Par,
                    BindPattern,
                    ListParWithRandom,
                    TaggedContinuation,
                >,
                channel: &Par,
            ) -> Vec<Datum<ListParWithRandom>> {
                self.space.get_data_at(checkpoint, channel).await
            }

            async fn get_waiting_continuations(
                &self,
                channels: Vec<Par>,
            ) -> Vec<WaitingContinuation<BindPattern, TaggedContinuation>> {
                self.space.get_waiting_continuations(channels).await
            }

            async fn get_joins(&self, channel: Par) -> Vec<Vec<Par>> {
                self.space.get_joins(channel).await
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
    result: Option<(
        &ContResult<Par, BindPattern, TaggedContinuation>,
        &[RSpaceResult<Par, ListParWithRandom>],
    )>,
    triggered_by: TriggeredBy,
    cost: _cost,
    follow: i64,
) -> Result<(), InterpreterError> {
    let trigger_produce = match &triggered_by {
        TriggeredBy::Produce { id, .. } => Some(id.clone()),
        TriggeredBy::Consume { .. } => None,
    };
    let (triggered_by_id_bytes, triggered_by_channels_count, triggered_by_persistent) =
        match triggered_by {
            TriggeredBy::Consume {
                id,
                persistent,
                channels_count,
            } => (id, channels_count, persistent),
            TriggeredBy::Produce { id, persistent, .. } => (id, 1, persistent),
        };

    if let Some((cont, data_list)) = result {
        let receive = seal::of_continuation(&cont.continuation).ok().flatten();
        let sends: Vec<Option<seal::SealRecord>> = data_list
            .iter()
            .map(|data| {
                if trigger_produce.as_ref() == Some(&data.removed_datum.random_state) {
                    seal::current()
                } else {
                    data.removed_datum
                        .seal
                        .as_ref()
                        .and_then(|p| seal::SealRecord::from_proto(p).ok())
                }
            })
            .collect();
        cost.comm_ledger()
            .on_comm_between(comm_cells(receive.as_ref(), sends.iter().map(Option::as_ref)));
    }
    match result {
        Some((cont, data_list)) => membrane::settling(|| {
            draw_replayed_comm(cont, data_list, follow);
            let consume_id_bytes = consume_id_bytes(&cont.continuation)?;

            // We refund for non-persistent continuations, and for the persistent continuation triggering the comm.
            // That persistent continuation is going to be charged for (without refund) once it has no matches in TS.
            let triggered_by_consume = consume_id_bytes == triggered_by_id_bytes;
            if !cont.persistent || triggered_by_consume {
                let refund =
                    storage_cost_consume(&cont.channels, &cont.patterns, &cont.continuation);
                let payer = if triggered_by_consume {
                    seal::current()
                } else {
                    seal::of_continuation(&cont.continuation).ok().flatten()
                };
                seal::within_now(payer, || {
                    cost.charge(Cost::create(-refund.value, "consume storage refund"))
                })?;
            } else {
                cost.charge(Cost::create(0, "consume storage refund"))?;
            }

            let mut refunded = false;
            for (data, channel) in data_list.iter().zip(cont.channels.iter()) {
                let triggered_by_datum = data.removed_datum.random_state == triggered_by_id_bytes;
                if data.persistent && !triggered_by_datum {
                    continue;
                }
                let refund = storage_cost_produce(channel, &data.removed_datum);
                let payer = if triggered_by_datum {
                    seal::current()
                } else {
                    data.removed_datum
                        .seal
                        .as_ref()
                        .and_then(|p| seal::SealRecord::from_proto(p).ok())
                };
                seal::within_now(payer, || {
                    cost.charge(Cost::create(-refund.value, "produces storage refund"))
                })?;
                refunded = true;
            }
            if !refunded {
                cost.charge(Cost::create(0, "produces storage refund"))?;
            }

            let last_iteration = !triggered_by_persistent;

            if last_iteration {
                cost.charge(event_storage_cost(triggered_by_channels_count))?;
            }

            cost.charge(comm_event_storage_cost(cont.channels.len() as i64))
        }),
        None => membrane::settling(|| cost.charge(event_storage_cost(triggered_by_channels_count))),
    }
}

fn draw_replayed_comm(
    cont: &ContResult<Par, BindPattern, TaggedContinuation>,
    data_list: &[RSpaceResult<Par, ListParWithRandom>],
    follow: i64,
) {
    let Some(meter) = seal::meter() else {
        return;
    };
    if !meter.is_replay() {
        return;
    }
    let mut records: Vec<seal::SealRecord> = seal::of_continuation(&cont.continuation)
        .ok()
        .flatten()
        .into_iter()
        .collect();
    records.extend(
        data_list
            .iter()
            .filter_map(|d| d.removed_datum.seal.as_ref())
            .filter_map(|p| seal::SealRecord::from_proto(p).ok()),
    );
    let follow = follow + comm_event_storage_cost(cont.channels.len() as i64).value;
    meter.draw_comm_in_replay(&records, seal::current().as_ref(), follow);
}

enum Trigger {
    Consume,
    Produce { id: Vec<u8>, restored: bool },
}

fn with_rule<T: Default>(
    cost: &_cost,
    f: impl FnOnce(&mut crate::rust::interpreter::accounting::storage_rule::LeftInSpace) -> T,
) -> T {
    cost.storage_left_in_space()
        .as_mut()
        .map(f)
        .unwrap_or_default()
}

fn refund(cost: &_cost, amount: i64, operation: &'static str) -> Result<(), InterpreterError> {
    if amount == 0 {
        return Ok(());
    }
    cost.charge(Cost::create(-amount, operation))
}

fn left_in_space_match(
    cont: &ContResult<Par, BindPattern, TaggedContinuation>,
    data_list: &[RSpaceResult<Par, ListParWithRandom>],
    trigger: &Trigger,
    cost: &_cost,
    follow: i64,
) -> Result<(), InterpreterError> {
    draw_replayed_comm(cont, data_list, follow);
    let receive = seal::of_continuation(&cont.continuation).ok().flatten();
    let sends: Vec<Option<seal::SealRecord>> = data_list
        .iter()
        .map(|data| {
            let id = &data.removed_datum.random_state;
            match trigger {
                Trigger::Produce { id: trigger_id, .. } if id == trigger_id => seal::current(),
                _ => match data
                    .removed_datum
                    .seal
                    .as_ref()
                    .map(seal::SealRecord::from_proto)
                {
                    Some(Ok(record)) => Some(record),
                    _ => with_rule(cost, |rule| rule.datum_payer(id)).flatten(),
                },
            }
        })
        .collect();
    let surfaces = comm_cells(receive.as_ref(), sends.iter().map(Option::as_ref));
    let consume_storage = storage_cost_consume(&cont.channels, &cont.patterns, &cont.continuation);
    match trigger {
        Trigger::Consume => refund(cost, consume_storage.value, "consume storage refund")?,
        Trigger::Produce { .. } if cont.persistent => {}
        Trigger::Produce { .. } => {
            let id = consume_id_bytes(&cont.continuation)?;
            if let Some(payer) = with_rule(cost, |rule| rule.removed_continuation(&id)) {
                let amount =
                    consume_storage.value + event_storage_cost(cont.channels.len() as i64).value;
                seal::within_now(payer, || refund(cost, amount, "consume storage refund"))?;
            }
        }
    }

    let mut produce_refund = 0;
    for (data, channel) in data_list.iter().zip(cont.channels.iter()) {
        let id = &data.removed_datum.random_state;
        match trigger {
            Trigger::Produce {
                id: trigger_id,
                restored,
            } if id == trigger_id => {
                if !restored {
                    produce_refund += storage_cost_produce(channel, &data.removed_datum).value;
                }
            }
            _ if data.persistent => {}
            _ => {
                if let Some(payer) = with_rule(cost, |rule| rule.removed_datum(id)) {
                    let amount = storage_cost_produce(channel, &data.removed_datum).value
                        + event_storage_cost(1).value;
                    seal::within_now(payer, || refund(cost, amount, "produces storage refund"))?;
                }
            }
        }
    }
    refund(cost, produce_refund, "produces storage refund")?;

    cost.comm_ledger().on_comm_between(surfaces);
    Ok(())
}
