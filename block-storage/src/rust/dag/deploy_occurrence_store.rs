use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::BlockHash;
use models::rust::deploy_id::DeployIdV6;
use shared::rust::store::key_value_store::{
    strict_atomic_mutate, AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
};

use super::deploy_lifecycle_types::TerminalState;
use super::deploy_occurrence_types::{
    DeployOccurrence, OccurrenceActivation, OpenOccurrenceSummary, TerminalOccurrenceSummary,
    DEPLOY_OCCURRENCE_PROTOCOL_VERSION, DEPLOY_OCCURRENCE_SCHEMA_VERSION,
};

const ARCHIVE_TAG: u8 = 1;
const ACTIVE_TAG: u8 = 2;
const OPEN_SUMMARY_TAG: u8 = 3;
const TERMINAL_SUMMARY_TAG: u8 = 4;
const ACTIVATION_TAG: u8 = 5;
const ACTIVATION_KEY: &[u8] = b"fresh-v6";
const COMPOSITE_KEY_LENGTH: usize = 65;

#[derive(Clone)]
pub struct DeployOccurrenceStore {
    store: Arc<dyn KeyValueStore>,
}

pub struct OccurrenceMutationPlan {
    pub mutations: Vec<(Vec<u8>, AtomicStoreOperation)>,
}

impl DeployOccurrenceStore {
    pub fn activate_fresh(store: Arc<dyn KeyValueStore>) -> Result<Self, KvStoreError> {
        let this = Self { store };
        let key = activation_key();
        let expected = OccurrenceActivation {
            schema_version: DEPLOY_OCCURRENCE_SCHEMA_VERSION,
            protocol_version: DEPLOY_OCCURRENCE_PROTOCOL_VERSION,
        };
        let encoded = bincode::serialize(&expected)?;
        match this.store.get_one(&key)? {
            Some(existing) if existing == encoded => {
                this.repair_open_indexes()?;
                this.validate_consistency()?;
            }
            Some(_) => {
                return Err(KvStoreError::InvalidArgument(
                    "deploy occurrence activation marker is incompatible".to_string(),
                ));
            }
            None if this.store.non_empty()? => {
                return Err(KvStoreError::InvalidArgument(
                    "deploy occurrence storage contains legacy or partial state without a fresh protocol-v6 activation marker"
                        .to_string(),
                ));
            }
            None => {
                strict_atomic_mutate(&[AtomicStoreMutation {
                    store: this.store.as_ref(),
                    key,
                    operation: AtomicStoreOperation::CompareAndSwap {
                        expected: None,
                        replacement: Some(encoded),
                    },
                }])?;
            }
        }
        Ok(this)
    }

    pub fn raw_store(&self) -> &Arc<dyn KeyValueStore> { &self.store }

    pub fn commit(&self, plan: &OccurrenceMutationPlan) -> Result<(), KvStoreError> {
        let mutations = plan
            .mutations
            .iter()
            .map(|(key, operation)| AtomicStoreMutation {
                store: self.store.as_ref(),
                key: key.clone(),
                operation: operation.clone(),
            })
            .collect::<Vec<_>>();
        strict_atomic_mutate(&mutations)
    }

    pub fn insert(&self, occurrence: DeployOccurrence) -> Result<(), KvStoreError> {
        for _ in 0..64 {
            let plan = self.prepare_insert(occurrence.clone())?;
            match self.commit(&plan) {
                Err(KvStoreError::TransactionConflict(_)) => continue,
                result => return result,
            }
        }
        Err(KvStoreError::TransactionConflict(
            "deploy occurrence insertion exceeded the compare-and-swap retry limit".to_string(),
        ))
    }

    pub fn prepare_insert(
        &self,
        occurrence: DeployOccurrence,
    ) -> Result<OccurrenceMutationPlan, KvStoreError> {
        occurrence
            .validate()
            .map_err(KvStoreError::InvalidArgument)?;
        let archive_key = archive_key(occurrence.deploy_id, &occurrence.source_block_hash);
        let archive_value = bincode::serialize(&occurrence)?;
        let existing_archive = self.store.get_one(&archive_key)?;
        if existing_archive
            .as_ref()
            .is_some_and(|existing| existing != &archive_value)
        {
            return Err(KvStoreError::TransactionConflict(
                "deploy occurrence composite key has a different immutable value".to_string(),
            ));
        }
        let terminal_key = terminal_summary_key(occurrence.deploy_id);
        if let Some(encoded_terminal) = self.store.get_one(&terminal_key)? {
            return self.prepare_late_insert(
                occurrence,
                archive_key,
                archive_value,
                existing_archive.is_none(),
                terminal_key,
                encoded_terminal,
            );
        }
        let summary_key = open_summary_key(occurrence.deploy_id);
        let encoded_summary = self.store.get_one(&summary_key)?;
        let previous = encoded_summary
            .as_ref()
            .map(|bytes| bincode::deserialize::<OpenOccurrenceSummary>(bytes))
            .transpose()?;
        let summary = match previous.as_ref() {
            Some(previous) => {
                validate_open_summary(previous, occurrence.deploy_id)?;
                let canonical = if occurrence.rank_cmp(&previous.canonical).is_gt() {
                    occurrence.clone()
                } else {
                    previous.canonical.clone()
                };
                OpenOccurrenceSummary {
                    schema_version: DEPLOY_OCCURRENCE_SCHEMA_VERSION,
                    deploy_id: occurrence.deploy_id,
                    canonical,
                    archive_count: previous
                        .archive_count
                        .checked_add(u64::from(existing_archive.is_none()))
                        .ok_or_else(|| {
                            KvStoreError::InvalidArgument(
                                "deploy occurrence archive count overflow".to_string(),
                            )
                        })?,
                    revision: previous
                        .revision
                        .checked_add(u64::from(existing_archive.is_none()))
                        .ok_or_else(|| {
                            KvStoreError::InvalidArgument(
                                "deploy occurrence revision overflow".to_string(),
                            )
                        })?,
                }
            }
            None if existing_archive.is_some() => {
                self.rebuild_open_summary(occurrence.deploy_id)?
            }
            None => OpenOccurrenceSummary {
                schema_version: DEPLOY_OCCURRENCE_SCHEMA_VERSION,
                deploy_id: occurrence.deploy_id,
                canonical: occurrence.clone(),
                archive_count: 1,
                revision: 1,
            },
        };
        let mut mutations = vec![
            (
                archive_key,
                AtomicStoreOperation::PutIfAbsentOrEqual(archive_value),
            ),
            (summary_key, AtomicStoreOperation::CompareAndSwap {
                expected: encoded_summary,
                replacement: Some(bincode::serialize(&summary)?),
            }),
        ];
        if let Some(previous) = previous {
            if previous.canonical.source_block_hash != summary.canonical.source_block_hash {
                mutations.push((
                    active_key(occurrence.deploy_id, &previous.canonical.source_block_hash),
                    AtomicStoreOperation::Delete,
                ));
            }
        }
        mutations.push((
            active_key(occurrence.deploy_id, &summary.canonical.source_block_hash),
            AtomicStoreOperation::PutIfAbsentOrEqual(bincode::serialize(&summary.canonical)?),
        ));
        Ok(OccurrenceMutationPlan { mutations })
    }

    fn prepare_late_insert(
        &self,
        occurrence: DeployOccurrence,
        archive_key: Vec<u8>,
        archive_value: Vec<u8>,
        is_new: bool,
        terminal_key: Vec<u8>,
        encoded_terminal: Vec<u8>,
    ) -> Result<OccurrenceMutationPlan, KvStoreError> {
        let mut terminal: TerminalOccurrenceSummary = bincode::deserialize(&encoded_terminal)?;
        validate_terminal_summary(&terminal, occurrence.deploy_id)?;
        if occurrence.source_block_height <= terminal.compaction_horizon
            && !matches!(
                occurrence.admission_mode,
                super::deploy_occurrence_types::OccurrenceAdmissionMode::SettledHistory
            )
        {
            return Err(KvStoreError::InvalidArgument(
                "a late occurrence below the certified compaction horizon requires settled-history admission"
                    .to_string(),
            ));
        }
        let previous_representative = terminal.current_representative.clone();
        if occurrence
            .rank_cmp(&terminal.current_representative)
            .is_gt()
        {
            terminal.current_representative = occurrence.clone();
        }
        if is_new {
            terminal.archive_count = terminal.archive_count.checked_add(1).ok_or_else(|| {
                KvStoreError::InvalidArgument(
                    "deploy occurrence archive count overflow".to_string(),
                )
            })?;
            terminal.digest_generation =
                terminal.digest_generation.checked_add(1).ok_or_else(|| {
                    KvStoreError::InvalidArgument(
                        "deploy occurrence digest generation overflow".to_string(),
                    )
                })?;
            terminal.archive_digest = add_archive_digest(
                terminal.archive_digest,
                occurrence_leaf_digest(&archive_key, &archive_value)?,
            );
        }
        let mut mutations = vec![
            (
                archive_key,
                AtomicStoreOperation::PutIfAbsentOrEqual(archive_value),
            ),
            (terminal_key, AtomicStoreOperation::CompareAndSwap {
                expected: Some(encoded_terminal),
                replacement: Some(bincode::serialize(&terminal)?),
            }),
        ];
        if previous_representative.source_block_hash
            != terminal.current_representative.source_block_hash
        {
            mutations.push((
                active_key(
                    occurrence.deploy_id,
                    &previous_representative.source_block_hash,
                ),
                AtomicStoreOperation::Delete,
            ));
        }
        if occurrence.source_block_height > terminal.compaction_horizon {
            mutations.push((
                active_key(
                    occurrence.deploy_id,
                    &terminal.current_representative.source_block_hash,
                ),
                AtomicStoreOperation::PutIfAbsentOrEqual(bincode::serialize(
                    &terminal.current_representative,
                )?),
            ));
        }
        Ok(OccurrenceMutationPlan { mutations })
    }

    pub fn exact_occurrences(
        &self,
        deploy_id: DeployIdV6,
    ) -> Result<BTreeMap<BlockHash, DeployOccurrence>, KvStoreError> {
        let mut rows = BTreeMap::new();
        for (_, value) in self
            .store
            .scan_prefix_exact_len(&active_prefix(deploy_id), COMPOSITE_KEY_LENGTH)?
        {
            let occurrence: DeployOccurrence = bincode::deserialize(&value)?;
            rows.insert(
                BlockHash::copy_from_slice(&occurrence.source_block_hash),
                occurrence,
            );
        }
        for (_, value) in self
            .store
            .scan_prefix_exact_len(&archive_prefix(deploy_id), COMPOSITE_KEY_LENGTH)?
        {
            let occurrence: DeployOccurrence = bincode::deserialize(&value)?;
            rows.insert(
                BlockHash::copy_from_slice(&occurrence.source_block_hash),
                occurrence,
            );
        }
        Ok(rows)
    }

    pub fn canonical(&self, deploy_id: DeployIdV6) -> Result<Option<BlockHash>, KvStoreError> {
        if let Some(encoded) = self.store.get_one(&terminal_summary_key(deploy_id))? {
            let summary: TerminalOccurrenceSummary = bincode::deserialize(&encoded)?;
            validate_terminal_summary(&summary, deploy_id)?;
            return Ok(Some(BlockHash::copy_from_slice(
                &summary.current_representative.source_block_hash,
            )));
        }
        let Some(encoded) = self.store.get_one(&open_summary_key(deploy_id))? else {
            return Ok(None);
        };
        let summary: OpenOccurrenceSummary = bincode::deserialize(&encoded)?;
        validate_open_summary(&summary, deploy_id)?;
        Ok(Some(BlockHash::copy_from_slice(
            &summary.canonical.source_block_hash,
        )))
    }

    pub fn prepare_compaction(
        &self,
        deploy_id: DeployIdV6,
        terminal_state: TerminalState,
        rejection_count: u32,
        finalization_revision: u64,
        finalized_floor_hash: [u8; 32],
        finalized_floor_height: i64,
        compaction_horizon: i64,
    ) -> Result<OccurrenceMutationPlan, KvStoreError> {
        if let Some(encoded_terminal) = self.store.get_one(&terminal_summary_key(deploy_id))? {
            let terminal: TerminalOccurrenceSummary = bincode::deserialize(&encoded_terminal)?;
            validate_terminal_summary(&terminal, deploy_id)?;
            if terminal.terminal_state != terminal_state
                || terminal.rejection_count != rejection_count
            {
                return Err(KvStoreError::TransactionConflict(
                    "terminal deploy occurrence summary disagrees with the write-once lifecycle verdict"
                        .to_string(),
                ));
            }
            return Ok(OccurrenceMutationPlan {
                mutations: Vec::new(),
            });
        }
        let open_key = open_summary_key(deploy_id);
        let encoded_open = self.store.get_one(&open_key)?.ok_or_else(|| {
            KvStoreError::KeyNotFound("open deploy occurrence summary".to_string())
        })?;
        let open: OpenOccurrenceSummary = bincode::deserialize(&encoded_open)?;
        validate_open_summary(&open, deploy_id)?;
        let digest = self.archive_digest_with(deploy_id, None)?;
        let terminal = TerminalOccurrenceSummary {
            schema_version: DEPLOY_OCCURRENCE_SCHEMA_VERSION,
            deploy_id,
            terminal_state,
            frozen_source: open.canonical.clone(),
            current_representative: open.canonical.clone(),
            rejection_count,
            archive_count: open.archive_count,
            archive_digest: digest,
            finalization_revision,
            finalized_floor_hash,
            finalized_floor_height,
            compaction_horizon,
            digest_generation: 0,
        };
        Ok(OccurrenceMutationPlan {
            mutations: vec![
                (
                    terminal_summary_key(deploy_id),
                    AtomicStoreOperation::CompareAndSwap {
                        expected: None,
                        replacement: Some(bincode::serialize(&terminal)?),
                    },
                ),
                (open_key, AtomicStoreOperation::CompareAndSwap {
                    expected: Some(encoded_open),
                    replacement: None,
                }),
                (
                    active_key(deploy_id, &open.canonical.source_block_hash),
                    AtomicStoreOperation::Delete,
                ),
            ],
        })
    }

    pub fn validate_consistency(&self) -> Result<(), KvStoreError> {
        let marker = activation_key();
        let mut ids = BTreeSet::new();
        for (key, value) in self.store.to_map()? {
            if key == marker {
                continue;
            }
            match key.first().copied() {
                Some(ARCHIVE_TAG) => {
                    let occurrence: DeployOccurrence = bincode::deserialize(&value)?;
                    occurrence
                        .validate()
                        .map_err(KvStoreError::SerializationError)?;
                    if key != archive_key(occurrence.deploy_id, &occurrence.source_block_hash) {
                        return Err(KvStoreError::SerializationError(
                            "deploy occurrence archive key does not match its value".to_string(),
                        ));
                    }
                    ids.insert(occurrence.deploy_id);
                }
                Some(ACTIVE_TAG) => {
                    let occurrence: DeployOccurrence = bincode::deserialize(&value)?;
                    occurrence
                        .validate()
                        .map_err(KvStoreError::SerializationError)?;
                    if key != active_key(occurrence.deploy_id, &occurrence.source_block_hash) {
                        return Err(KvStoreError::SerializationError(
                            "deploy occurrence active key does not match its value".to_string(),
                        ));
                    }
                    ids.insert(occurrence.deploy_id);
                }
                Some(OPEN_SUMMARY_TAG) => {
                    let summary: OpenOccurrenceSummary = bincode::deserialize(&value)?;
                    validate_open_summary(&summary, summary.deploy_id)?;
                    if key != open_summary_key(summary.deploy_id) {
                        return Err(KvStoreError::SerializationError(
                            "open deploy occurrence summary key does not match its value"
                                .to_string(),
                        ));
                    }
                    ids.insert(summary.deploy_id);
                }
                Some(TERMINAL_SUMMARY_TAG) => {
                    let summary: TerminalOccurrenceSummary = bincode::deserialize(&value)?;
                    validate_terminal_summary(&summary, summary.deploy_id)?;
                    if key != terminal_summary_key(summary.deploy_id) {
                        return Err(KvStoreError::SerializationError(
                            "terminal deploy occurrence summary key does not match its value"
                                .to_string(),
                        ));
                    }
                    ids.insert(summary.deploy_id);
                }
                _ => {
                    return Err(KvStoreError::SerializationError(
                        "deploy occurrence storage contains an unknown row tag".to_string(),
                    ));
                }
            }
        }
        for deploy_id in ids {
            let occurrences = self.exact_archive_occurrences(deploy_id)?;
            let expected = reduce_occurrences(occurrences.values()).ok_or_else(|| {
                KvStoreError::SerializationError(
                    "deploy occurrence archive group is empty".to_string(),
                )
            })?;
            let open = self.store.get_one(&open_summary_key(deploy_id))?;
            let terminal = self.store.get_one(&terminal_summary_key(deploy_id))?;
            match (open, terminal) {
                (Some(encoded), None) => {
                    let summary: OpenOccurrenceSummary = bincode::deserialize(&encoded)?;
                    validate_open_summary(&summary, deploy_id)?;
                    if summary.archive_count != occurrences.len() as u64
                        || summary.canonical != expected
                    {
                        return Err(KvStoreError::SerializationError(
                            "open deploy occurrence summary disagrees with archive".to_string(),
                        ));
                    }
                    let active = self
                        .store
                        .scan_prefix_exact_len(&active_prefix(deploy_id), COMPOSITE_KEY_LENGTH)?;
                    if active.len() != 1
                        || active[0].0
                            != active_key(deploy_id, &summary.canonical.source_block_hash)
                        || bincode::deserialize::<DeployOccurrence>(&active[0].1)?
                            != summary.canonical
                    {
                        return Err(KvStoreError::SerializationError(
                            "open deploy occurrence active row disagrees with summary".to_string(),
                        ));
                    }
                }
                (None, Some(encoded)) => {
                    let summary: TerminalOccurrenceSummary = bincode::deserialize(&encoded)?;
                    validate_terminal_summary(&summary, deploy_id)?;
                    if summary.archive_count != occurrences.len() as u64
                        || summary.current_representative != expected
                        || summary.archive_digest != self.archive_digest_with(deploy_id, None)?
                    {
                        return Err(KvStoreError::SerializationError(
                            "terminal deploy occurrence summary disagrees with archive".to_string(),
                        ));
                    }
                    let active = self
                        .store
                        .scan_prefix_exact_len(&active_prefix(deploy_id), COMPOSITE_KEY_LENGTH)?;
                    let expected_active = (summary.current_representative.source_block_height
                        > summary.compaction_horizon)
                        .then(|| {
                            (
                                active_key(
                                    deploy_id,
                                    &summary.current_representative.source_block_hash,
                                ),
                                &summary.current_representative,
                            )
                        });
                    match (active.as_slice(), expected_active) {
                        ([], None) => {}
                        ([(key, value)], Some((expected_key, expected_value)))
                            if *key == expected_key
                                && bincode::deserialize::<DeployOccurrence>(value)?
                                    == *expected_value => {}
                        _ => {
                            return Err(KvStoreError::SerializationError(
                                "terminal deploy occurrence active row disagrees with summary"
                                    .to_string(),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(KvStoreError::SerializationError(
                        "deploy occurrence identity requires exactly one summary".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn repair_open_indexes(&self) -> Result<(), KvStoreError> {
        let ids = self
            .store
            .scan_prefix(&[ARCHIVE_TAG])?
            .into_iter()
            .map(|(_, value)| bincode::deserialize::<DeployOccurrence>(&value))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|occurrence| occurrence.deploy_id)
            .collect::<BTreeSet<_>>();
        for deploy_id in ids {
            let open_key = open_summary_key(deploy_id);
            let terminal_key = terminal_summary_key(deploy_id);
            let encoded_open = self.store.get_one(&open_key)?;
            if self.store.get_one(&terminal_key)?.is_some() {
                if encoded_open.is_some() {
                    return Err(KvStoreError::SerializationError(
                        "deploy occurrence identity has both open and terminal summaries"
                            .to_string(),
                    ));
                }
                continue;
            }
            let rebuilt = self.rebuild_open_summary(deploy_id)?;
            let mut mutations = self
                .store
                .scan_prefix_exact_len(&active_prefix(deploy_id), COMPOSITE_KEY_LENGTH)?
                .into_iter()
                .map(|(key, _)| (key, AtomicStoreOperation::Delete))
                .collect::<Vec<_>>();
            mutations.push((open_key, AtomicStoreOperation::CompareAndSwap {
                expected: encoded_open,
                replacement: Some(bincode::serialize(&rebuilt)?),
            }));
            mutations.push((
                active_key(deploy_id, &rebuilt.canonical.source_block_hash),
                AtomicStoreOperation::PutIfAbsentOrEqual(bincode::serialize(&rebuilt.canonical)?),
            ));
            self.commit(&OccurrenceMutationPlan { mutations })?;
        }
        Ok(())
    }

    fn rebuild_open_summary(
        &self,
        deploy_id: DeployIdV6,
    ) -> Result<OpenOccurrenceSummary, KvStoreError> {
        let occurrences = self.exact_archive_occurrences(deploy_id)?;
        let canonical = reduce_occurrences(occurrences.values()).ok_or_else(|| {
            KvStoreError::SerializationError(
                "cannot rebuild deploy occurrence summary without archive rows".to_string(),
            )
        })?;
        Ok(OpenOccurrenceSummary {
            schema_version: DEPLOY_OCCURRENCE_SCHEMA_VERSION,
            deploy_id,
            canonical,
            archive_count: occurrences.len() as u64,
            revision: occurrences.len() as u64,
        })
    }

    fn exact_archive_occurrences(
        &self,
        deploy_id: DeployIdV6,
    ) -> Result<BTreeMap<[u8; 32], DeployOccurrence>, KvStoreError> {
        self.store
            .scan_prefix_exact_len(&archive_prefix(deploy_id), COMPOSITE_KEY_LENGTH)?
            .into_iter()
            .map(|(_, value)| {
                let occurrence: DeployOccurrence = bincode::deserialize(&value)?;
                Ok((occurrence.source_block_hash, occurrence))
            })
            .collect()
    }

    fn archive_digest_with(
        &self,
        deploy_id: DeployIdV6,
        additional: Option<(&Vec<u8>, &Vec<u8>)>,
    ) -> Result<[u8; 32], KvStoreError> {
        let mut rows = self
            .store
            .scan_prefix_exact_len(&archive_prefix(deploy_id), COMPOSITE_KEY_LENGTH)?;
        if let Some((key, value)) = additional {
            if !rows.iter().any(|(existing, _)| existing == key) {
                rows.push((key.clone(), value.clone()));
            }
        }
        rows.into_iter()
            .try_fold([0u8; 32], |digest, (key, value)| {
                Ok(add_archive_digest(
                    digest,
                    occurrence_leaf_digest(&key, &value)?,
                ))
            })
    }
}

fn occurrence_leaf_digest(key: &[u8], value: &[u8]) -> Result<[u8; 32], KvStoreError> {
    let mut preimage = b"f1r3.deploy-occurrence.archive-leaf.v1".to_vec();
    preimage.extend_from_slice(&(key.len() as u64).to_be_bytes());
    preimage.extend_from_slice(key);
    preimage.extend_from_slice(&(value.len() as u64).to_be_bytes());
    preimage.extend_from_slice(value);
    Blake2b256::hash(preimage)
        .try_into()
        .map_err(|digest: Vec<u8>| {
            KvStoreError::SerializationError(format!(
                "deploy occurrence archive leaf digest has length {}",
                digest.len()
            ))
        })
}

fn add_archive_digest(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut result = [0u8; 32];
    let mut carry = 0u16;
    for index in (0..32).rev() {
        let sum = u16::from(left[index]) + u16::from(right[index]) + carry;
        result[index] = sum as u8;
        carry = sum >> 8;
    }
    result
}

fn reduce_occurrences<'a>(
    occurrences: impl IntoIterator<Item = &'a DeployOccurrence>,
) -> Option<DeployOccurrence> {
    occurrences.into_iter().cloned().reduce(|left, right| {
        if right.rank_cmp(&left).is_gt() {
            right
        } else {
            left
        }
    })
}

fn validate_open_summary(
    summary: &OpenOccurrenceSummary,
    deploy_id: DeployIdV6,
) -> Result<(), KvStoreError> {
    if summary.schema_version != DEPLOY_OCCURRENCE_SCHEMA_VERSION
        || summary.deploy_id != deploy_id
        || summary.canonical.deploy_id != deploy_id
    {
        return Err(KvStoreError::SerializationError(
            "invalid open deploy occurrence summary".to_string(),
        ));
    }
    Ok(())
}

fn validate_terminal_summary(
    summary: &TerminalOccurrenceSummary,
    deploy_id: DeployIdV6,
) -> Result<(), KvStoreError> {
    if summary.schema_version != DEPLOY_OCCURRENCE_SCHEMA_VERSION
        || summary.deploy_id != deploy_id
        || summary.frozen_source.deploy_id != deploy_id
        || summary.current_representative.deploy_id != deploy_id
    {
        return Err(KvStoreError::SerializationError(
            "invalid terminal deploy occurrence summary".to_string(),
        ));
    }
    Ok(())
}

fn activation_key() -> Vec<u8> {
    let mut key = vec![ACTIVATION_TAG];
    key.extend_from_slice(ACTIVATION_KEY);
    key
}

fn archive_prefix(deploy_id: DeployIdV6) -> Vec<u8> { tagged_id(ARCHIVE_TAG, deploy_id) }

fn active_prefix(deploy_id: DeployIdV6) -> Vec<u8> { tagged_id(ACTIVE_TAG, deploy_id) }

fn archive_key(deploy_id: DeployIdV6, block_hash: &[u8; 32]) -> Vec<u8> {
    tagged_hash(ARCHIVE_TAG, deploy_id, block_hash)
}

fn active_key(deploy_id: DeployIdV6, block_hash: &[u8; 32]) -> Vec<u8> {
    tagged_hash(ACTIVE_TAG, deploy_id, block_hash)
}

fn open_summary_key(deploy_id: DeployIdV6) -> Vec<u8> { tagged_id(OPEN_SUMMARY_TAG, deploy_id) }

fn terminal_summary_key(deploy_id: DeployIdV6) -> Vec<u8> {
    tagged_id(TERMINAL_SUMMARY_TAG, deploy_id)
}

fn tagged_id(tag: u8, deploy_id: DeployIdV6) -> Vec<u8> {
    let mut key = Vec::with_capacity(33);
    key.push(tag);
    key.extend_from_slice(deploy_id.as_ref());
    key
}

fn tagged_hash(tag: u8, deploy_id: DeployIdV6, block_hash: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(65);
    key.push(tag);
    key.extend_from_slice(deploy_id.as_ref());
    key.extend_from_slice(block_hash);
    key
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Barrier;

    use proptest::prelude::*;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
        Db, LmdbDirStoreManager, LmdbEnvConfig,
    };

    use super::super::deploy_occurrence_types::OccurrenceAdmissionMode;
    use super::*;

    fn deploy_id(byte: u8) -> DeployIdV6 { DeployIdV6::try_from(&[byte; 32][..]).unwrap() }

    fn occurrence(id: DeployIdV6, height: i64, hash: u8) -> DeployOccurrence {
        DeployOccurrence {
            schema_version: DEPLOY_OCCURRENCE_SCHEMA_VERSION,
            deploy_id: id,
            protocol_version: DEPLOY_OCCURRENCE_PROTOCOL_VERSION,
            source_block_hash: [hash; 32],
            source_block_height: height,
            source_validator: vec![hash; models::rust::validator::LENGTH],
            deploy_ordinal: 0,
            admission_mode: OccurrenceAdmissionMode::Normal,
            admission_ruleset_digest: vec![1; 32],
            admission_context_digest: vec![2; 32],
            sender_authority_digest: vec![3; 32],
            is_failed: false,
        }
    }

    fn store() -> DeployOccurrenceStore {
        DeployOccurrenceStore::activate_fresh(Arc::new(InMemoryKeyValueStore::new())).unwrap()
    }

    #[test]
    fn fresh_activation_rejects_legacy_or_partial_rows() {
        let raw: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        raw.put_one(b"legacy".to_vec(), b"row".to_vec()).unwrap();

        assert!(matches!(
            DeployOccurrenceStore::activate_fresh(raw),
            Err(KvStoreError::InvalidArgument(_))
        ));
    }

    #[test]
    fn reducer_and_exact_lookup_are_arrival_order_independent() {
        let id = deploy_id(7);
        let values = vec![
            occurrence(id, 1, 3),
            occurrence(id, 2, 9),
            occurrence(id, 2, 4),
        ];
        let forward = store();
        let reverse = store();
        for value in &values {
            forward.insert(value.clone()).unwrap();
        }
        for value in values.iter().rev() {
            reverse.insert(value.clone()).unwrap();
        }

        assert_eq!(
            forward.canonical(id).unwrap(),
            Some(BlockHash::copy_from_slice(&[4; 32]))
        );
        assert_eq!(
            forward.canonical(id).unwrap(),
            reverse.canonical(id).unwrap()
        );
        assert_eq!(
            forward.exact_occurrences(id).unwrap(),
            reverse.exact_occurrences(id).unwrap()
        );
    }

    #[test]
    fn duplicate_insert_repairs_a_missing_summary() {
        let id = deploy_id(8);
        let value = occurrence(id, 5, 5);
        let occurrence_store = store();
        occurrence_store
            .raw_store()
            .put_one(
                archive_key(id, &value.source_block_hash),
                bincode::serialize(&value).unwrap(),
            )
            .unwrap();

        occurrence_store.insert(value.clone()).unwrap();

        assert_eq!(
            occurrence_store.canonical(id).unwrap(),
            Some(BlockHash::copy_from_slice(&value.source_block_hash))
        );
        occurrence_store.validate_consistency().unwrap();
    }

    #[test]
    fn compaction_preserves_exact_archive_and_terminal_result() {
        let id = deploy_id(9);
        let occurrence_store = store();
        occurrence_store.insert(occurrence(id, 4, 4)).unwrap();
        occurrence_store.insert(occurrence(id, 6, 6)).unwrap();
        let plan = occurrence_store
            .prepare_compaction(id, TerminalState::Finalized, 0, 3, [7; 32], 6, 6)
            .unwrap();
        occurrence_store.commit(&plan).unwrap();

        assert_eq!(occurrence_store.exact_occurrences(id).unwrap().len(), 2);
        assert_eq!(
            occurrence_store.canonical(id).unwrap(),
            Some(BlockHash::copy_from_slice(&[6; 32]))
        );
        assert!(occurrence_store
            .raw_store()
            .scan_prefix(&active_prefix(id))
            .unwrap()
            .is_empty());
        occurrence_store.validate_consistency().unwrap();
    }

    #[test]
    fn only_settled_history_can_arrive_below_a_terminal_horizon() {
        let id = deploy_id(10);
        let occurrence_store = store();
        occurrence_store.insert(occurrence(id, 6, 6)).unwrap();
        let plan = occurrence_store
            .prepare_compaction(id, TerminalState::Finalized, 0, 3, [7; 32], 6, 6)
            .unwrap();
        occurrence_store.commit(&plan).unwrap();
        assert!(matches!(
            occurrence_store.insert(occurrence(id, 5, 5)),
            Err(KvStoreError::InvalidArgument(_))
        ));
        let mut settled = occurrence(id, 5, 5);
        settled.admission_mode = OccurrenceAdmissionMode::SettledHistory;

        occurrence_store.insert(settled).unwrap();

        assert_eq!(occurrence_store.exact_occurrences(id).unwrap().len(), 2);
        occurrence_store.validate_consistency().unwrap();
    }

    #[test]
    fn concurrent_insertions_converge_after_cas_retries() {
        let id = deploy_id(11);
        let occurrence_store = Arc::new(store());
        let barrier = Arc::new(Barrier::new(16));
        let handles = (0u8..16)
            .map(|value| {
                let occurrence_store = occurrence_store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    occurrence_store.insert(occurrence(id, i64::from(value), value))
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        assert_eq!(occurrence_store.exact_occurrences(id).unwrap().len(), 16);
        assert_eq!(
            occurrence_store.canonical(id).unwrap(),
            Some(BlockHash::copy_from_slice(&[15; 32]))
        );
        occurrence_store.validate_consistency().unwrap();
    }

    #[test]
    fn terminal_archive_digest_is_arrival_order_independent() {
        let id = deploy_id(12);
        let values = vec![
            occurrence(id, 1, 8),
            occurrence(id, 4, 2),
            occurrence(id, 4, 1),
        ];
        let forward = store();
        let reverse = store();
        for value in &values {
            forward.insert(value.clone()).unwrap();
        }
        for value in values.iter().rev() {
            reverse.insert(value.clone()).unwrap();
        }
        for occurrence_store in [&forward, &reverse] {
            let plan = occurrence_store
                .prepare_compaction(id, TerminalState::Finalized, 0, 2, [9; 32], 4, 4)
                .unwrap();
            occurrence_store.commit(&plan).unwrap();
        }
        let forward_terminal: TerminalOccurrenceSummary = bincode::deserialize(
            &forward
                .raw_store()
                .get_one(&terminal_summary_key(id))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let reverse_terminal: TerminalOccurrenceSummary = bincode::deserialize(
            &reverse
                .raw_store()
                .get_one(&terminal_summary_key(id))
                .unwrap()
                .unwrap(),
        )
        .unwrap();

        assert_eq!(forward_terminal, reverse_terminal);
    }

    #[test]
    fn persistent_row_growth_is_linear_in_distinct_occurrences() {
        for count in [1u8, 8, 64] {
            let id = deploy_id(15);
            let occurrence_store = store();
            for value in 0..count {
                occurrence_store
                    .insert(occurrence(id, i64::from(value), value))
                    .unwrap();
            }

            assert_eq!(
                occurrence_store.raw_store().to_map().unwrap().len(),
                usize::from(count) + 3
            );
        }
    }

    #[test]
    fn lmdb_reopen_repairs_derived_rows_from_the_immutable_archive() {
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target/block-storage-test-scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("occurrence-")
            .tempdir_in(scratch)
            .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let id = deploy_id(13);
        let value = occurrence(id, 7, 4);

        {
            let mut manager = LmdbDirStoreManager::new(
                directory.path().to_path_buf(),
                HashMap::from([(
                    Db::new("occurrences".to_string(), None),
                    LmdbEnvConfig::new("occurrence-env".to_string(), 16 << 20).with_max_dbs(4),
                )]),
            );
            let raw = runtime
                .block_on(manager.store("occurrences".to_string()))
                .unwrap();
            let occurrence_store = DeployOccurrenceStore::activate_fresh(raw.clone()).unwrap();
            raw.put_one(
                archive_key(id, &value.source_block_hash),
                bincode::serialize(&value).unwrap(),
            )
            .unwrap();
            drop(occurrence_store);
            drop(raw);
            runtime.block_on(manager.shutdown()).unwrap();
        }

        {
            let mut manager = LmdbDirStoreManager::new(
                directory.path().to_path_buf(),
                HashMap::from([(
                    Db::new("occurrences".to_string(), None),
                    LmdbEnvConfig::new("occurrence-env".to_string(), 16 << 20).with_max_dbs(4),
                )]),
            );
            let raw = runtime
                .block_on(manager.store("occurrences".to_string()))
                .unwrap();
            let occurrence_store = DeployOccurrenceStore::activate_fresh(raw).unwrap();

            assert_eq!(
                occurrence_store.canonical(id).unwrap(),
                Some(BlockHash::copy_from_slice(&value.source_block_hash))
            );
            assert_eq!(occurrence_store.exact_occurrences(id).unwrap().len(), 1);
            occurrence_store.validate_consistency().unwrap();
            drop(occurrence_store);
            runtime.block_on(manager.shutdown()).unwrap();
        }
    }

    proptest! {
        #[test]
        fn occurrence_reducer_and_terminal_digest_are_permutation_invariant(
            heights in prop::collection::vec(0i64..1000, 1..32),
        ) {
            let id = deploy_id(14);
            let values = heights
                .into_iter()
                .enumerate()
                .map(|(index, height)| occurrence(id, height, u8::try_from(index + 1).unwrap()))
                .collect::<Vec<_>>();
            let forward = store();
            let reverse = store();
            for value in &values {
                forward.insert(value.clone()).unwrap();
            }
            for value in values.iter().rev() {
                reverse.insert(value.clone()).unwrap();
            }
            prop_assert_eq!(forward.canonical(id).unwrap(), reverse.canonical(id).unwrap());
            prop_assert_eq!(forward.exact_occurrences(id).unwrap(), reverse.exact_occurrences(id).unwrap());
            let horizon = values.iter().map(|value| value.source_block_height).max().unwrap();
            for occurrence_store in [&forward, &reverse] {
                let plan = occurrence_store
                    .prepare_compaction(
                        id,
                        TerminalState::Finalized,
                        0,
                        1,
                        [1; 32],
                        horizon,
                        horizon,
                    )
                    .unwrap();
                occurrence_store.commit(&plan).unwrap();
            }
            let forward_terminal = forward.raw_store().get_one(&terminal_summary_key(id)).unwrap();
            let reverse_terminal = reverse.raw_store().get_one(&terminal_summary_key(id)).unwrap();
            prop_assert_eq!(forward_terminal, reverse_terminal);
        }
    }
}
