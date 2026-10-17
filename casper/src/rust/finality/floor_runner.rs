use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::validator::Validator;
use shared::rust::dag::observation_work::WorkMeter;

use crate::rust::errors::CasperError;
use crate::rust::finality::floor::{
    floor_of_block_metered, hold_for, incompatible_fork_error, live_tips_metered,
    parent_frontier_metered, sound_with_inherited_metered, view_outcome_metered, Floor,
    FloorOfView, IntroducedSigsMemo,
};
use crate::rust::safety::clique_oracle::FtThreshold;

pub const MAX_CONSECUTIVE_INVALIDATIONS: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub [u8; 32]);

fn push_bytes(buffer: &mut Vec<u8>, bytes: &[u8]) {
    buffer.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    buffer.extend_from_slice(bytes);
}

pub fn snapshot_id(
    dag: &KeyValueDagRepresentation,
    ftt: FtThreshold,
) -> Result<SnapshotId, CasperError> {
    let mut latest: Vec<(Validator, BlockHash)> = dag.latest_message_hashes().into_iter().collect();
    latest.sort();
    let mut invalid: Vec<BlockHash> = dag
        .invalid_blocks_set
        .iter()
        .map(|metadata| metadata.block_hash.clone())
        .collect();
    invalid.sort();
    let lfb = dag.last_finalized_block();
    let lfb_number = dag.block_number(&lfb).unwrap_or(-1);

    let mut buffer = Vec::new();
    push_bytes(&mut buffer, b"f1r3-floor-snapshot-v1");
    buffer.extend_from_slice(&(latest.len() as u64).to_be_bytes());
    for (validator, hash) in &latest {
        push_bytes(&mut buffer, validator);
        push_bytes(&mut buffer, hash);
    }
    push_bytes(&mut buffer, &lfb);
    buffer.extend_from_slice(&lfb_number.to_be_bytes());
    buffer.extend_from_slice(&ftt.num.to_be_bytes());
    buffer.extend_from_slice(&ftt.den.to_be_bytes());
    buffer.extend_from_slice(&(invalid.len() as u64).to_be_bytes());
    for hash in &invalid {
        push_bytes(&mut buffer, hash);
    }
    let digest = Blake2b256::hash(buffer);
    let bytes: [u8; 32] = digest
        .try_into()
        .map_err(|_| CasperError::RuntimeError("snapshot digest is not 32 bytes".to_string()))?;
    Ok(SnapshotId(bytes))
}

#[derive(Clone, Copy, Debug)]
pub enum Budget {
    Unlimited,
    Steps(u64),
    Until(Instant),
}

impl Budget {
    fn exhausted(&self, taken: u64) -> bool {
        match self {
            Budget::Unlimited => false,
            Budget::Steps(limit) => taken >= *limit,
            Budget::Until(deadline) => Instant::now() >= *deadline,
        }
    }
}

#[derive(Debug)]
pub enum Advance {
    Yielded,
    Ready(Result<FloorOfView, CasperError>),
}

enum Phase {
    Tips,
    Floors {
        tips: Vec<BlockHash>,
        live: BTreeMap<Validator, BlockHash>,
        inherited: Vec<Floor>,
    },
    Frontiers {
        tips: Vec<BlockHash>,
        live: BTreeMap<Validator, BlockHash>,
        inherited: Vec<Floor>,
        frontiers: Vec<Floor>,
    },
    Probe {
        tips: Vec<BlockHash>,
        live: BTreeMap<Validator, BlockHash>,
        known_floors: Vec<Floor>,
        known_frontiers: Vec<Floor>,
        index: usize,
        floor_of_tip: Option<Floor>,
        decidable: Vec<(BlockHash, Floor, Floor)>,
        first_missing: BlockHash,
    },
    Compat {
        parents: Vec<BlockHash>,
        inherited: Vec<Floor>,
        candidates: Vec<Floor>,
        ordered: Vec<Floor>,
        candidate_index: usize,
        inherited_index: usize,
        memo: IntroducedSigsMemo,
    },
    Containment {
        derived: Floor,
    },
    Done(Option<Result<FloorOfView, CasperError>>),
    Interrupted,
}

impl Phase {
    fn name(&self) -> &'static str {
        match self {
            Phase::Tips => "tips",
            Phase::Floors { .. } => "floors",
            Phase::Frontiers { .. } => "frontiers",
            Phase::Probe { .. } => "probe",
            Phase::Compat { .. } => "compatibility",
            Phase::Containment { .. } => "containment",
            Phase::Done(_) => "done",
            Phase::Interrupted => "interrupted",
        }
    }
}

fn compat_phase(
    parents: Vec<BlockHash>,
    inherited: Vec<Floor>,
    frontiers: Vec<Floor>,
) -> Phase {
    let mut candidates = inherited.clone();
    candidates.extend(frontiers);
    let mut ordered = candidates.clone();
    ordered.sort_by(|a, b| {
        b.block_number
            .cmp(&a.block_number)
            .then_with(|| b.hash.cmp(&a.hash))
    });
    Phase::Compat {
        parents,
        inherited,
        candidates,
        ordered,
        candidate_index: 0,
        inherited_index: 0,
        memo: IntroducedSigsMemo::new(),
    }
}

pub struct FloorCursor {
    dag: KeyValueDagRepresentation,
    current: Floor,
    ftt: FtThreshold,
    sigma: SnapshotId,
    phase: Phase,
    steps_taken: u64,
}

impl FloorCursor {
    pub fn new(
        dag: KeyValueDagRepresentation,
        current: Floor,
        ftt: FtThreshold,
    ) -> Result<Self, CasperError> {
        let sigma = snapshot_id(&dag, ftt)?;
        Ok(Self::with_sigma(dag, current, ftt, sigma))
    }

    fn with_sigma(
        dag: KeyValueDagRepresentation,
        current: Floor,
        ftt: FtThreshold,
        sigma: SnapshotId,
    ) -> Self {
        Self {
            dag,
            current,
            ftt,
            sigma,
            phase: Phase::Tips,
            steps_taken: 0,
        }
    }

    pub fn sigma(&self) -> SnapshotId { self.sigma }

    pub fn current(&self) -> &Floor { &self.current }

    pub fn dag(&self) -> &KeyValueDagRepresentation { &self.dag }

    pub fn steps_taken(&self) -> u64 { self.steps_taken }

    pub fn phase_name(&self) -> &'static str { self.phase.name() }

    pub async fn advance<W: WorkMeter>(
        &mut self,
        meter: &W,
        block_store: &KeyValueBlockStore,
        budget: Budget,
    ) -> Advance {
        let mut taken = 0u64;
        loop {
            if let Phase::Done(result) = &mut self.phase {
                return match result.take() {
                    Some(result) => Advance::Ready(result),
                    None => Advance::Ready(Err(CasperError::RuntimeError(
                        "floor cursor result was already taken".to_string(),
                    ))),
                };
            }
            if taken > 0 && budget.exhausted(taken) {
                return Advance::Yielded;
            }
            self.step(meter, block_store).await;
            taken += 1;
            self.steps_taken += 1;
        }
    }

    async fn step<W: WorkMeter>(&mut self, meter: &W, block_store: &KeyValueBlockStore) {
        let phase = std::mem::replace(&mut self.phase, Phase::Interrupted);
        let ftt = self.ftt;
        let dag = &self.dag;
        let current = &self.current;
        let finish = |result: Result<FloorOfView, CasperError>| Phase::Done(Some(result));
        self.phase = match phase {
            Phase::Interrupted | Phase::Tips => match live_tips_metered(meter, dag, current) {
                Err(error) => finish(Err(error)),
                Ok(None) => finish(Ok(FloorOfView::NoAdvance)),
                Ok(Some((tips, live))) => Phase::Floors {
                    tips,
                    live,
                    inherited: Vec::new(),
                },
            },
            Phase::Floors {
                tips,
                live,
                mut inherited,
            } => {
                let index = inherited.len();
                if index == tips.len() {
                    Phase::Frontiers {
                        tips,
                        live,
                        inherited,
                        frontiers: Vec::new(),
                    }
                } else {
                    match floor_of_block_metered(meter, dag, block_store, &tips[index], ftt).await {
                        Ok(floor) => {
                            inherited.push(floor);
                            Phase::Floors {
                                tips,
                                live,
                                inherited,
                            }
                        }
                        Err(CasperError::BlockNotHeld(missing, _)) => Phase::Probe {
                            tips,
                            live,
                            known_floors: inherited,
                            known_frontiers: Vec::new(),
                            index: 0,
                            floor_of_tip: None,
                            decidable: Vec::new(),
                            first_missing: missing,
                        },
                        Err(error) => finish(hold_for(error, ftt)),
                    }
                }
            }
            Phase::Frontiers {
                tips,
                live,
                inherited,
                mut frontiers,
            } => {
                let index = frontiers.len();
                if index == tips.len() {
                    compat_phase(tips, inherited, frontiers)
                } else {
                    match parent_frontier_metered(meter, dag, &tips[index], &live, ftt).await {
                        Ok(frontier) => {
                            frontiers.push(frontier);
                            Phase::Frontiers {
                                tips,
                                live,
                                inherited,
                                frontiers,
                            }
                        }
                        Err(CasperError::BlockNotHeld(missing, _)) => Phase::Probe {
                            tips,
                            live,
                            known_floors: inherited,
                            known_frontiers: frontiers,
                            index: 0,
                            floor_of_tip: None,
                            decidable: Vec::new(),
                            first_missing: missing,
                        },
                        Err(error) => finish(hold_for(error, ftt)),
                    }
                }
            }
            Phase::Probe {
                tips,
                live,
                known_floors,
                known_frontiers,
                index,
                floor_of_tip,
                mut decidable,
                first_missing,
            } => {
                if index == tips.len() {
                    if decidable.is_empty() {
                        finish(Ok(FloorOfView::NoAdvance))
                    } else if decidable.len() == tips.len() {
                        finish(Ok(FloorOfView::AbsenceHold {
                            missing: first_missing,
                        }))
                    } else {
                        let mut parents = Vec::with_capacity(decidable.len());
                        let mut inherited = Vec::with_capacity(decidable.len());
                        let mut frontiers = Vec::with_capacity(decidable.len());
                        for (tip, floor, frontier) in decidable {
                            parents.push(tip);
                            inherited.push(floor);
                            frontiers.push(frontier);
                        }
                        compat_phase(parents, inherited, frontiers)
                    }
                } else {
                    let tip = tips[index].clone();
                    match floor_of_tip {
                        None => {
                            let resolved = match known_floors.get(index) {
                                Some(floor) => Ok(floor.clone()),
                                None => {
                                    floor_of_block_metered(meter, dag, block_store, &tip, ftt).await
                                }
                            };
                            match resolved {
                                Ok(floor) => Phase::Probe {
                                    tips,
                                    live,
                                    known_floors,
                                    known_frontiers,
                                    index,
                                    floor_of_tip: Some(floor),
                                    decidable,
                                    first_missing,
                                },
                                Err(CasperError::BlockNotHeld(missing, _)) => {
                                    tracing::debug!(
                                        target: "f1r3fly.finalizer",
                                        tip = %PrettyPrinter::build_string_bytes(&tip),
                                        missing = %PrettyPrinter::build_string_bytes(&missing),
                                        "abstaining a tip whose derivation needs a block below this node's history"
                                    );
                                    Phase::Probe {
                                        tips,
                                        live,
                                        known_floors,
                                        known_frontiers,
                                        index: index + 1,
                                        floor_of_tip: None,
                                        decidable,
                                        first_missing,
                                    }
                                }
                                Err(error) => finish(Err(error)),
                            }
                        }
                        Some(floor) => {
                            let resolved = match known_frontiers.get(index) {
                                Some(frontier) => Ok(frontier.clone()),
                                None => parent_frontier_metered(meter, dag, &tip, &live, ftt).await,
                            };
                            match resolved {
                                Ok(frontier) => {
                                    decidable.push((tip, floor, frontier));
                                    Phase::Probe {
                                        tips,
                                        live,
                                        known_floors,
                                        known_frontiers,
                                        index: index + 1,
                                        floor_of_tip: None,
                                        decidable,
                                        first_missing,
                                    }
                                }
                                Err(CasperError::BlockNotHeld(missing, _)) => {
                                    tracing::debug!(
                                        target: "f1r3fly.finalizer",
                                        tip = %PrettyPrinter::build_string_bytes(&tip),
                                        missing = %PrettyPrinter::build_string_bytes(&missing),
                                        "abstaining a tip whose derivation needs a block below this node's history"
                                    );
                                    Phase::Probe {
                                        tips,
                                        live,
                                        known_floors,
                                        known_frontiers,
                                        index: index + 1,
                                        floor_of_tip: None,
                                        decidable,
                                        first_missing,
                                    }
                                }
                                Err(error) => finish(Err(error)),
                            }
                        }
                    }
                }
            }
            Phase::Compat {
                parents,
                inherited,
                candidates,
                ordered,
                candidate_index,
                inherited_index,
                mut memo,
            } => {
                if candidate_index == ordered.len() {
                    finish(hold_for(
                        incompatible_fork_error(&parents, &candidates),
                        ftt,
                    ))
                } else if inherited_index == inherited.len() {
                    Phase::Containment {
                        derived: ordered[candidate_index].clone(),
                    }
                } else {
                    let candidate = &ordered[candidate_index];
                    let other = &inherited[inherited_index];
                    if other.hash == candidate.hash {
                        Phase::Compat {
                            parents,
                            inherited,
                            candidates,
                            ordered,
                            candidate_index,
                            inherited_index: inherited_index + 1,
                            memo,
                        }
                    } else {
                        match sound_with_inherited_metered(
                            meter,
                            dag,
                            block_store,
                            &parents,
                            candidate,
                            other,
                            &mut memo,
                        ) {
                            Ok(true) => Phase::Compat {
                                parents,
                                inherited,
                                candidates,
                                ordered,
                                candidate_index,
                                inherited_index: inherited_index + 1,
                                memo,
                            },
                            Ok(false) => {
                                tracing::debug!(
                                    target: "f1r3.trace.floor",
                                    candidate = %PrettyPrinter::build_string_bytes(&candidate.hash),
                                    candidate_number = candidate.block_number,
                                    inherited = %PrettyPrinter::build_string_bytes(&other.hash),
                                    inherited_number = other.block_number,
                                    "floor candidate skipped: neither captures nor re-merges an inherited floor"
                                );
                                Phase::Compat {
                                    parents,
                                    inherited,
                                    candidates,
                                    ordered,
                                    candidate_index: candidate_index + 1,
                                    inherited_index: 0,
                                    memo,
                                }
                            }
                            Err(error) => finish(hold_for(error, ftt)),
                        }
                    }
                }
            }
            Phase::Containment { derived } => {
                finish(view_outcome_metered(meter, dag, block_store, derived, current))
            }
            done @ Phase::Done(_) => done,
        };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preparation {
    Fresh,
    Resumed,
    Reset,
    Frozen,
}

#[derive(Default)]
pub struct CursorSlot {
    cursor: Option<FloorCursor>,
    invalidations: u32,
}

pub type SharedCursor = tokio::sync::Mutex<CursorSlot>;

pub fn new_shared_cursor() -> Arc<SharedCursor> {
    Arc::new(tokio::sync::Mutex::new(CursorSlot::default()))
}

impl CursorSlot {
    pub fn prepare(
        &mut self,
        fresh: KeyValueDagRepresentation,
        current: Floor,
        ftt: FtThreshold,
    ) -> Result<Preparation, CasperError> {
        let sigma = snapshot_id(&fresh, ftt)?;
        let preparation = match &self.cursor {
            None => Preparation::Fresh,
            Some(cursor) if cursor.sigma == sigma => Preparation::Resumed,
            Some(cursor) if matches!(cursor.phase, Phase::Interrupted) => Preparation::Reset,
            Some(_) if self.invalidations >= MAX_CONSECUTIVE_INVALIDATIONS => Preparation::Frozen,
            Some(_) => Preparation::Reset,
        };
        match preparation {
            Preparation::Fresh => {
                self.cursor = Some(FloorCursor::with_sigma(fresh, current, ftt, sigma));
            }
            Preparation::Reset => {
                self.invalidations += 1;
                self.cursor = Some(FloorCursor::with_sigma(fresh, current, ftt, sigma));
            }
            Preparation::Resumed | Preparation::Frozen => {}
        }
        Ok(preparation)
    }

    pub fn cursor(&self) -> Option<&FloorCursor> { self.cursor.as_ref() }

    pub fn invalidations(&self) -> u32 { self.invalidations }

    pub async fn advance<W: WorkMeter>(
        &mut self,
        meter: &W,
        block_store: &KeyValueBlockStore,
        budget: Budget,
    ) -> Option<(Advance, SnapshotId, Floor, KeyValueDagRepresentation)> {
        let cursor = self.cursor.as_mut()?;
        let advance = cursor.advance(meter, block_store, budget).await;
        let sigma = cursor.sigma;
        let current = cursor.current.clone();
        let dag = cursor.dag.clone();
        if matches!(advance, Advance::Ready(_)) {
            self.cursor = None;
            self.invalidations = 0;
        }
        Some((advance, sigma, current, dag))
    }
}
