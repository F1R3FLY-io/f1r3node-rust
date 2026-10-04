use crate::{
    Chain, CommittedExecution, Error, ExecutionReceipt, MAX_RECEIPT_BYTES, MAX_RECEIPT_DEPLOYS,
    PROFILE_ID, decode, encode,
};
use cordial_miners_core::blocklace::Blocklace;
use cordial_miners_core::consensus::{
    InvalidBlock, validate_received_block, validated_received_insert, weighted_tau,
};
use cordial_miners_core::crypto::CONTENT_HASH_VERSION;
use cordial_miners_core::{Block, BlockIdentity, NodeId};
use heed::{Database, Env, EnvOpenOptions, types::Bytes};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct StoreConfig {
    pub map_size_bytes: usize,
    pub max_pending_objects: usize,
    pub max_pending_bytes: usize,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            map_size_bytes: 64 * 1024 * 1024,
            max_pending_objects: 256,
            max_pending_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Admission {
    Accepted(BlockIdentity),
    Duplicate(BlockIdentity),
    Deferred {
        object: BlockIdentity,
        missing: Vec<BlockIdentity>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EquivocationRecord {
    pub creator: Vec<u8>,
    pub round: u64,
    pub objects: Vec<BlockIdentity>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HistoryPage {
    pub start: u64,
    pub next: u64,
    pub total: u64,
    pub packets: Vec<Vec<u8>>,
}

pub struct DurableBlocklace {
    chain: Chain,
    view: Blocklace,
    count: u64,
    poisoned: bool,
    blocks: Database<Bytes, Bytes>,
    meta: Database<Bytes, Bytes>,
    pending_db: Database<Bytes, Bytes>,
    pending: BTreeMap<BlockIdentity, Vec<u8>>,
    pending_bytes: usize,
    waiting: BTreeMap<BlockIdentity, BTreeSet<BlockIdentity>>,
    missing: BTreeMap<BlockIdentity, Vec<BlockIdentity>>,
    output_db: Database<Bytes, Bytes>,
    output: Vec<BlockIdentity>,
    executed_count: u64,
    executed_state: [u8; 32],
    submissions: BTreeMap<[u8; 32], Vec<u8>>,
    submission_bytes: usize,
    config: StoreConfig,
    env: Env,
    _lease: File,
}

impl DurableBlocklace {
    pub fn open(path: &Path, chain: Chain, config: StoreConfig) -> Result<Self, Error> {
        if config.map_size_bytes < 1024 * 1024
            || config.max_pending_objects == 0
            || config.max_pending_bytes == 0
        {
            return Err(Error::Configuration(
                "consensus store must have at least 1 MiB".into(),
            ));
        }
        if path.exists() {
            if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
                return Err(Error::Configuration(
                    "consensus store cannot be a symlink".into(),
                ));
            }
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                if !entry.file_type()?.is_file()
                    || !matches!(
                        entry.file_name().to_str(),
                        Some("data.mdb" | "lock.mdb" | ".cordial-lock")
                    )
                {
                    return Err(Error::Configuration(
                        "refusing an unidentified store directory".into(),
                    ));
                }
            }
        } else {
            std::fs::create_dir_all(path)?;
        }
        let lease = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.join(".cordial-lock"))?;
        fs2::FileExt::try_lock_exclusive(&lease).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                Error::InUse
            } else {
                Error::Io(error)
            }
        })?;
        let expected = encode(&(2u32, CONTENT_HASH_VERSION, PROFILE_ID, chain.spec()))?;
        let existing = path.join("data.mdb").exists();
        if existing {
            let mut options = EnvOpenOptions::new();
            options.max_dbs(4);
            let readonly = unsafe { options.flags(heed::EnvFlags::READ_ONLY).open(path)? };
            let identity = {
                let txn = readonly.read_txn()?;
                let db: Database<Bytes, Bytes> = readonly
                    .open_database(&txn, Some("metadata"))?
                    .ok_or_else(|| Error::Corrupt("missing chain metadata".into()))?;
                db.get(&txn, b"manifest")?
                    .map(Vec::from)
                    .ok_or_else(|| Error::Corrupt("missing chain manifest".into()))?
            };
            readonly.prepare_for_closing().wait();
            if identity != expected {
                return Err(Error::ChainMismatch);
            }
        }
        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(4)
                .map_size(config.map_size_bytes)
                .open(path)?
        };
        let (blocks, meta, pending_db, output_db): (
            Database<Bytes, Bytes>,
            Database<Bytes, Bytes>,
            Database<Bytes, Bytes>,
            Database<Bytes, Bytes>,
        ) = if existing {
            let txn = env.read_txn()?;
            let blocks = env
                .open_database(&txn, Some("admitted"))?
                .ok_or_else(|| Error::Corrupt("missing admitted objects".into()))?;
            let meta = env
                .open_database(&txn, Some("metadata"))?
                .ok_or_else(|| Error::Corrupt("missing metadata".into()))?;
            let pending = env
                .open_database(&txn, Some("pending"))?
                .ok_or_else(|| Error::Corrupt("missing pending-object database".into()))?;
            let output = env
                .open_database(&txn, Some("ordered-output"))?
                .ok_or_else(|| Error::Corrupt("missing ordered-output database".into()))?;
            txn.commit()?;
            (blocks, meta, pending, output)
        } else {
            let mut txn = env.write_txn()?;
            let blocks: Database<Bytes, Bytes> = env.create_database(&mut txn, Some("admitted"))?;
            let meta: Database<Bytes, Bytes> = env.create_database(&mut txn, Some("metadata"))?;
            let pending = env.create_database(&mut txn, Some("pending"))?;
            let output = env.create_database(&mut txn, Some("ordered-output"))?;
            meta.put(&mut txn, b"manifest", &expected)?;
            meta.put(&mut txn, b"count", &0u64.to_be_bytes())?;
            meta.put(&mut txn, b"output-count", &0u64.to_be_bytes())?;
            meta.put(&mut txn, b"execution-count", &0u64.to_be_bytes())?;
            txn.commit()?;
            File::open(path)?.sync_all()?;
            (blocks, meta, pending, output)
        };
        let executed_state = chain.spec().execution_genesis;
        let mut state = Self {
            chain,
            view: Blocklace::new(),
            count: 0,
            poisoned: false,
            blocks,
            meta,
            pending_db,
            pending: BTreeMap::new(),
            pending_bytes: 0,
            waiting: BTreeMap::new(),
            missing: BTreeMap::new(),
            output_db,
            output: Vec::new(),
            executed_count: 0,
            executed_state,
            submissions: BTreeMap::new(),
            submission_bytes: 0,
            config,
            env,
            _lease: lease,
        };
        let txn = state.env.read_txn()?;
        let expected_count = meta
            .get(&txn, b"count")?
            .ok_or_else(|| Error::Corrupt("missing admission count".into()))?;
        let expected_count = u64::from_be_bytes(
            expected_count
                .try_into()
                .map_err(|_| Error::Corrupt("invalid admission count".into()))?,
        );
        for entry in blocks.iter(&txn)? {
            let (key, packet) = entry?;
            if key != state.count.to_be_bytes() {
                return Err(Error::Corrupt("admission sequence has a gap".into()));
            }
            let block = state.chain.decode_block(packet)?;
            if state.view.content(&block.identity).is_some() {
                return Err(Error::Corrupt("duplicate persisted object".into()));
            }
            let result = validated_received_insert(block, &mut state.view, state.chain.weights());
            if !result.is_valid() {
                return Err(Error::Corrupt(format!(
                    "invalid recovered history: {:?}",
                    result.errors()
                )));
            }
            state.count = state
                .count
                .checked_add(1)
                .ok_or_else(|| Error::Corrupt("admission count overflow".into()))?;
        }
        if state.count != expected_count {
            return Err(Error::Corrupt(
                "admission count does not match stored history".into(),
            ));
        }
        let output_count = meta
            .get(&txn, b"output-count")?
            .ok_or_else(|| Error::Corrupt("missing output count".into()))?;
        let output_count = u64::from_be_bytes(
            output_count
                .try_into()
                .map_err(|_| Error::Corrupt("invalid output count".into()))?,
        );
        for entry in output_db.iter(&txn)? {
            let (key, bytes) = entry?;
            if key != (state.output.len() as u64).to_be_bytes() {
                return Err(Error::Corrupt("ordered-output sequence has a gap".into()));
            }
            state.output.push(decode(bytes)?);
        }
        if state.output.len() as u64 != output_count
            || !state.native_output()?.starts_with(&state.output)
        {
            return Err(Error::Corrupt(
                "stored output does not match native finality".into(),
            ));
        }
        for entry in pending_db.iter(&txn)? {
            let (key, packet) = entry?;
            let block = state.chain.decode_block(packet)?;
            if key != encode(&block.identity)? || state.view.content(&block.identity).is_some() {
                return Err(Error::Corrupt(
                    "pending object has an inconsistent identity".into(),
                ));
            }
            state.check_pending_capacity(packet.len())?;
            state.pending_bytes += packet.len();
            state.pending.insert(block.identity, packet.to_vec());
        }
        drop(txn);
        state.recover_execution()?;
        state.recover_submissions()?;
        let recovered = state.pending.keys().cloned().collect();
        state.retry_pending(recovered)?;
        Ok(state)
    }
    pub fn admit(&mut self, packet: &[u8]) -> Result<Admission, Error> {
        let result = self.admit_one(packet)?;
        if let Admission::Accepted(id) = &result {
            let dependents = self.dependents(id);
            if let Err(error) = self.retry_pending(dependents) {
                self.poisoned = true;
                return Err(error);
            }
        }
        Ok(result)
    }

    fn admit_one(&mut self, packet: &[u8]) -> Result<Admission, Error> {
        self.check_health()?;
        let block = self.chain.decode_block(packet)?;
        let id = block.identity.clone();
        if self.view.content(&id).is_some() {
            return Ok(Admission::Duplicate(id));
        }
        let result = validate_received_block(&block, &self.view, self.chain.weights());
        if let [InvalidBlock::MissingPredecessors { missing }] = result.errors() {
            if !self.pending.contains_key(&id) {
                self.check_pending_capacity(packet.len())?;
                let mut txn = self.env.write_txn()?;
                self.pending_db.put(&mut txn, &encode(&id)?, packet)?;
                txn.commit()?;
                self.pending.insert(id.clone(), packet.to_vec());
                self.pending_bytes += packet.len();
            }
            self.wait_for(&id, missing.clone());
            return Ok(Admission::Deferred {
                object: id,
                missing: missing.clone(),
            });
        }
        if !result.is_valid() {
            return Err(Error::Native(result.errors().to_vec()));
        }
        let next = self
            .count
            .checked_add(1)
            .ok_or_else(|| Error::Corrupt("admission count overflow".into()))?;
        let mut txn = self.env.write_txn()?;
        self.blocks
            .put(&mut txn, &self.count.to_be_bytes(), packet)?;
        self.meta.put(&mut txn, b"count", &next.to_be_bytes())?;
        self.pending_db.delete(&mut txn, &encode(&id)?)?;
        txn.commit()?;
        if !validated_received_insert(block, &mut self.view, self.chain.weights()).is_valid() {
            self.poisoned = true;
            return Err(Error::Corrupt(
                "durable admission could not enter live state".into(),
            ));
        }
        self.count = next;
        self.forget_pending(&id);
        Ok(Admission::Accepted(id))
    }

    pub fn get(&self, identity: &BlockIdentity) -> Result<Option<Block>, Error> {
        self.check_health()?;
        Ok(self.view.get(identity))
    }

    pub fn submit(&mut self, submission: crate::Submission) -> Result<bool, Error> {
        self.check_health()?;
        if self.has_executed_deploy(&submission.id)? {
            return Ok(false);
        }
        if let Some(payload) = self.submissions.get(&submission.id) {
            return if payload == &submission.payload {
                Ok(false)
            } else {
                Err(Error::Packet("submission identity conflict".into()))
            };
        }
        if submission.payload.is_empty() || submission.payload.len() > crate::MAX_PAYLOAD_BYTES {
            return Err(Error::Packet("invalid submission size".into()));
        }
        if self.submissions.len() >= 256
            || self.submission_bytes + submission.payload.len() > 4 * 1024 * 1024
        {
            return Err(Error::Capacity);
        }
        let mut txn = self.env.write_txn()?;
        self.meta.put(
            &mut txn,
            &submission_key(&submission.id),
            &submission.payload,
        )?;
        txn.commit()?;
        self.submission_bytes += submission.payload.len();
        self.submissions.insert(submission.id, submission.payload);
        Ok(true)
    }

    pub fn submissions(&self, limit: usize) -> Result<Vec<crate::Submission>, Error> {
        self.check_health()?;
        if limit == 0 || limit > 64 {
            return Err(Error::Packet("invalid submission limit".into()));
        }
        Ok(self
            .submissions
            .iter()
            .take(limit)
            .map(|(id, payload)| crate::Submission {
                id: *id,
                payload: payload.clone(),
            })
            .collect())
    }

    pub fn mark_proposed(
        &mut self,
        object: &BlockIdentity,
        submissions: &[crate::Submission],
    ) -> Result<(), Error> {
        self.check_health()?;
        if self.view.content(object).is_none() {
            return Err(Error::Corrupt("proposal is not durable".into()));
        }
        let mut txn = self.env.write_txn()?;
        for submission in submissions {
            self.meta
                .delete(&mut txn, &submission_key(&submission.id))?;
        }
        txn.commit()?;
        for submission in submissions {
            if let Some(payload) = self.submissions.remove(&submission.id) {
                self.submission_bytes -= payload.len();
            }
        }
        Ok(())
    }

    fn recover_submissions(&mut self) -> Result<(), Error> {
        let txn = self.env.read_txn()?;
        for entry in self.meta.iter(&txn)? {
            let (key, payload) = entry?;
            if let Some(id) = key.strip_prefix(b"submission/") {
                let id: [u8; 32] = id
                    .try_into()
                    .map_err(|_| Error::Corrupt("invalid submission identity".into()))?;
                if payload.is_empty()
                    || payload.len() > crate::MAX_PAYLOAD_BYTES
                    || self.meta.get(&txn, &deploy_key(&id))?.is_some()
                {
                    return Err(Error::Corrupt("invalid persisted submission".into()));
                }
                self.submission_bytes += payload.len();
                self.submissions.insert(id, payload.to_vec());
                if self.submissions.len() > 256 || self.submission_bytes > 4 * 1024 * 1024 {
                    return Err(Error::Corrupt("submission pool exceeds limits".into()));
                }
            }
        }
        Ok(())
    }

    pub fn history_page(&self, start: u64, limit: usize) -> Result<HistoryPage, Error> {
        self.check_health()?;
        if start > self.count || limit == 0 || limit > 32 {
            return Err(Error::Packet("invalid history page range".into()));
        }
        let txn = self.env.read_txn()?;
        let mut page = HistoryPage {
            start,
            next: start,
            total: self.count,
            packets: vec![],
        };
        let mut size = 1024;
        for index in start..self.count.min(start.saturating_add(limit as u64)) {
            let packet = self
                .blocks
                .get(&txn, &index.to_be_bytes())?
                .ok_or_else(|| Error::Corrupt("history page contains a gap".into()))?;
            if size + packet.len() + 8 > crate::MAX_PACKET_BYTES {
                break;
            }
            size += packet.len() + 8;
            page.packets.push(packet.to_vec());
            page.next += 1;
        }
        Ok(page)
    }

    pub fn propose(
        &mut self,
        key: &k256::ecdsa::SigningKey,
        payload: Vec<u8>,
    ) -> Result<Option<Block>, Error> {
        use cordial_miners_core::consensus::{
            cordiality::is_weighted_supermajority, round::compute_all_depths, select_predecessors,
        };
        self.check_health()?;
        let creator = NodeId(key.verifying_key().to_sec1_bytes().to_vec());
        if !self.chain.weights().contains_key(&creator) {
            return Err(Error::UnknownValidator);
        }
        let own = self.view.blocks_by(&creator);
        let predecessors = if own.is_empty() {
            vec![]
        } else {
            if !self.view.satisfies_chain_axiom(&creator) {
                return Err(Error::Configuration(
                    "local validator has conflicting persisted histories".into(),
                ));
            }
            let depths = compute_all_depths(&self.view);
            let mut support = BTreeMap::<u64, std::collections::HashSet<NodeId>>::new();
            for (id, depth) in &depths {
                support
                    .entry(*depth)
                    .or_default()
                    .insert(id.creator.clone());
            }
            let Some(round) = support.into_iter().rev().find_map(|(round, creators)| {
                is_weighted_supermajority(&creators, self.chain.weights()).then_some(round)
            }) else {
                return Ok(None);
            };
            if own.iter().any(|block| {
                depths
                    .get(&block.identity)
                    .is_some_and(|depth| *depth > round)
            }) {
                return Ok(None);
            }
            let mut prefix = Blocklace::new();
            let mut identities: Vec<_> = depths
                .into_iter()
                .filter(|(_, depth)| *depth <= round)
                .collect();
            identities.sort_by(|(left, ld), (right, rd)| ld.cmp(rd).then(left.cmp(right)));
            for (id, _) in identities {
                let block = self
                    .view
                    .get(&id)
                    .ok_or_else(|| Error::Corrupt("proposal history is missing".into()))?;
                if !validated_received_insert(block, &mut prefix, self.chain.weights()).is_valid() {
                    return Err(Error::Corrupt(
                        "proposal prefix failed native validation".into(),
                    ));
                }
            }
            select_predecessors(&prefix, self.chain.weights())
                .into_iter()
                .collect()
        };
        let block = self.chain.build_block(key, predecessors, payload)?;
        match self.admit(&self.chain.encode_block(&block)?)? {
            Admission::Accepted(_) => Ok(Some(block)),
            Admission::Duplicate(_) => Ok(None),
            Admission::Deferred { .. } => {
                Err(Error::Corrupt("local proposal has missing history".into()))
            }
        }
    }

    pub fn equivocations(&self, limit: usize) -> Result<Vec<EquivocationRecord>, Error> {
        self.check_health()?;
        if limit == 0 || limit > 64 {
            return Err(Error::Packet("invalid equivocation limit".into()));
        }
        let mut records: Vec<_> = cordial_miners_core::consensus::all_equivocations(&self.view)
            .into_iter()
            .map(|equivocation| {
                let mut objects = equivocation.blocks;
                objects.sort();
                EquivocationRecord {
                    creator: equivocation.creator.0,
                    round: equivocation.round,
                    objects,
                }
            })
            .collect();
        records.sort_by(|left, right| {
            (left.round, &left.creator, &left.objects).cmp(&(right.round, &right.creator, &right.objects))
        });
        records.truncate(limit);
        Ok(records)
    }

    pub fn pending_count(&self) -> Result<usize, Error> {
        self.check_health()?;
        Ok(self.pending.len())
    }

    pub fn ordered_output(&self) -> Result<Vec<BlockIdentity>, Error> {
        self.check_health()?;
        Ok(self.output.clone())
    }

    pub fn admitted_count(&self) -> u64 {
        self.count
    }
    pub fn committed_count(&self) -> u64 {
        self.output.len() as u64
    }
    pub fn output_page(&self, start: u64, limit: usize) -> Result<Vec<BlockIdentity>, Error> {
        self.check_health()?;
        if limit == 0 || limit > 32 || start > self.output.len() as u64 {
            return Err(Error::Packet("invalid output page range".into()));
        }
        Ok(self
            .output
            .iter()
            .skip(start as usize)
            .take(limit)
            .cloned()
            .collect())
    }

    pub fn execution_checkpoint(&self) -> crate::ExecutionCheckpoint {
        crate::ExecutionCheckpoint {
            chain: self.chain.fingerprint(),
            next_index: self.executed_count,
            state: self.executed_state,
        }
    }

    pub fn next_execution(&self) -> Result<Option<CommittedExecution>, Error> {
        self.check_health()?;
        let Some(object) = self.output.get(self.executed_count as usize) else {
            return Ok(None);
        };
        let block = self
            .view
            .get(object)
            .ok_or_else(|| Error::Corrupt("committed object is missing".into()))?;
        Ok(Some(CommittedExecution {
            chain: self.chain.fingerprint(),
            index: self.executed_count,
            object: object.clone(),
            pre_state: self.executed_state,
            payload: self.chain.application_data(&block)?,
        }))
    }

    pub fn execution_receipt(&self, index: u64) -> Result<Option<ExecutionReceipt>, Error> {
        self.check_health()?;
        let txn = self.env.read_txn()?;
        self.meta
            .get(&txn, &execution_key(index))?
            .map(decode)
            .transpose()
    }

    pub fn has_executed_deploy(&self, id: &[u8; 32]) -> Result<bool, Error> {
        self.check_health()?;
        let txn = self.env.read_txn()?;
        Ok(self.meta.get(&txn, &deploy_key(id))?.is_some())
    }

    pub fn acknowledge_execution(&mut self, receipt: ExecutionReceipt) -> Result<(), Error> {
        self.check_health()?;
        if receipt.index < self.executed_count {
            return if self.execution_receipt(receipt.index)?.as_ref() == Some(&receipt) {
                Ok(())
            } else {
                Err(Error::Execution(
                    "conflicting receipt for an executed object".into(),
                ))
            };
        }
        if receipt.index != self.executed_count
            || self.output.get(receipt.index as usize) != Some(&receipt.object)
            || receipt.pre_state != self.executed_state
        {
            return Err(Error::Execution(
                "receipt does not match the next committed object and pre-state".into(),
            ));
        }
        if receipt.result.len() > MAX_RECEIPT_BYTES
            || receipt.deploy_ids.len() > MAX_RECEIPT_DEPLOYS
            || receipt.deploy_ids.iter().collect::<BTreeSet<_>>().len() != receipt.deploy_ids.len()
        {
            return Err(Error::Execution(
                "receipt limits or deploy identity uniqueness violated".into(),
            ));
        }
        let next = self
            .executed_count
            .checked_add(1)
            .ok_or_else(|| Error::Corrupt("execution cursor overflow".into()))?;
        let mut txn = self.env.write_txn()?;
        for id in &receipt.deploy_ids {
            let key = deploy_key(id);
            if self.meta.get(&txn, &key)?.is_some() {
                return Err(Error::Execution("deploy already executed".into()));
            }
            self.meta
                .put(&mut txn, &key, &receipt.index.to_be_bytes())?;
            self.meta.delete(&mut txn, &submission_key(id))?;
        }
        self.meta
            .put(&mut txn, &execution_key(receipt.index), &encode(&receipt)?)?;
        self.meta
            .put(&mut txn, b"execution-count", &next.to_be_bytes())?;
        txn.commit()?;
        for id in &receipt.deploy_ids {
            if let Some(payload) = self.submissions.remove(id) {
                self.submission_bytes -= payload.len();
            }
        }
        self.executed_count = next;
        self.executed_state = receipt.post_state;
        Ok(())
    }

    fn recover_execution(&mut self) -> Result<(), Error> {
        let txn = self.env.read_txn()?;
        let count = self
            .meta
            .get(&txn, b"execution-count")?
            .ok_or_else(|| Error::Corrupt("missing execution cursor".into()))?;
        let count = u64::from_be_bytes(
            count
                .try_into()
                .map_err(|_| Error::Corrupt("invalid execution cursor".into()))?,
        );
        if count > self.output.len() as u64 {
            return Err(Error::Corrupt(
                "execution cursor exceeds committed output".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        for index in 0..count {
            let bytes = self
                .meta
                .get(&txn, &execution_key(index))?
                .ok_or_else(|| Error::Corrupt("execution receipt sequence has a gap".into()))?;
            let receipt: ExecutionReceipt = decode(bytes)?;
            if receipt.index != index
                || receipt.object != self.output[index as usize]
                || receipt.pre_state != self.executed_state
                || receipt.result.len() > MAX_RECEIPT_BYTES
                || receipt.deploy_ids.len() > MAX_RECEIPT_DEPLOYS
            {
                return Err(Error::Corrupt(
                    "execution receipt does not match committed history".into(),
                ));
            }
            for id in receipt.deploy_ids {
                if !seen.insert(id)
                    || self.meta.get(&txn, &deploy_key(&id))?
                        != Some(index.to_be_bytes().as_slice())
                {
                    return Err(Error::Corrupt(
                        "executed deploy index is inconsistent".into(),
                    ));
                }
            }
            self.executed_state = receipt.post_state;
        }
        let mut receipt_count = 0;
        let mut deploy_count = 0;
        for entry in self.meta.iter(&txn)? {
            let (key, _) = entry?;
            if key.starts_with(b"execution/") {
                receipt_count += 1;
            }
            if key.starts_with(b"deploy/") {
                deploy_count += 1;
            }
        }
        if receipt_count != count || deploy_count != seen.len() {
            return Err(Error::Corrupt(
                "execution journal contains orphan records".into(),
            ));
        }
        self.executed_count = count;
        Ok(())
    }

    pub fn advance_output(&mut self) -> Result<usize, Error> {
        self.check_health()?;
        let output = self.native_output()?;
        if !output.starts_with(&self.output) {
            self.poisoned = true;
            return Err(Error::Corrupt(
                "native output attempted to replace a committed prefix".into(),
            ));
        }
        let added = output.len() - self.output.len();
        if added == 0 {
            return Ok(0);
        }
        let mut txn = self.env.write_txn()?;
        for (index, id) in output.iter().enumerate().skip(self.output.len()) {
            self.output_db
                .put(&mut txn, &(index as u64).to_be_bytes(), &encode(id)?)?;
        }
        self.meta.put(
            &mut txn,
            b"output-count",
            &(output.len() as u64).to_be_bytes(),
        )?;
        txn.commit()?;
        self.output = output;
        Ok(added)
    }

    fn native_output(&self) -> Result<Vec<BlockIdentity>, Error> {
        let validators = &self.chain.spec().validators;
        let leader = |wave| {
            Some(NodeId(
                validators[(wave % validators.len() as u64) as usize]
                    .public_key
                    .clone(),
            ))
        };
        weighted_tau(
            &self.view,
            self.chain.spec().wavelength,
            self.chain.weights(),
            leader,
        )
        .map_err(|error| Error::Corrupt(format!("native ordering failed: {error:?}")))
    }

    fn check_pending_capacity(&self, bytes: usize) -> Result<(), Error> {
        if self.pending.len() >= self.config.max_pending_objects
            || bytes
                > self
                    .config
                    .max_pending_bytes
                    .saturating_sub(self.pending_bytes)
        {
            Err(Error::Capacity)
        } else {
            Ok(())
        }
    }

    fn retry_pending(&mut self, mut work: Vec<BlockIdentity>) -> Result<(), Error> {
        while let Some(id) = work.pop() {
            let Some(packet) = self.pending.get(&id).cloned() else {
                continue;
            };
            match self.admit_one(&packet) {
                Ok(Admission::Accepted(accepted)) => work.extend(self.dependents(&accepted)),
                Ok(Admission::Deferred { .. }) => {}
                Err(Error::Native(_)) => {
                    let mut txn = self.env.write_txn()?;
                    self.pending_db.delete(&mut txn, &encode(&id)?)?;
                    txn.commit()?;
                    self.forget_pending(&id);
                }
                Ok(Admission::Duplicate(_)) => {
                    return Err(Error::Corrupt("admitted object remains pending".into()));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn dependents(&mut self, id: &BlockIdentity) -> Vec<BlockIdentity> {
        self.waiting
            .remove(id)
            .map(|dependents| dependents.into_iter().collect())
            .unwrap_or_default()
    }

    fn wait_for(&mut self, id: &BlockIdentity, missing: Vec<BlockIdentity>) {
        self.unwait(id);
        for dependency in &missing {
            self.waiting
                .entry(dependency.clone())
                .or_default()
                .insert(id.clone());
        }
        self.missing.insert(id.clone(), missing);
    }

    fn unwait(&mut self, id: &BlockIdentity) {
        for dependency in self.missing.remove(id).unwrap_or_default() {
            if let Some(dependents) = self.waiting.get_mut(&dependency) {
                dependents.remove(id);
                if dependents.is_empty() {
                    self.waiting.remove(&dependency);
                }
            }
        }
    }

    fn forget_pending(&mut self, id: &BlockIdentity) {
        self.unwait(id);
        if let Some(packet) = self.pending.remove(id) {
            self.pending_bytes -= packet.len();
        }
    }

    fn check_health(&self) -> Result<(), Error> {
        if self.poisoned {
            Err(Error::Corrupt(
                "admission failed after durable storage".into(),
            ))
        } else {
            Ok(())
        }
    }
}

fn execution_key(index: u64) -> Vec<u8> {
    let mut key = b"execution/".to_vec();
    key.extend(index.to_be_bytes());
    key
}

fn deploy_key(id: &[u8; 32]) -> Vec<u8> {
    let mut key = b"deploy/".to_vec();
    key.extend(id);
    key
}

fn submission_key(id: &[u8; 32]) -> Vec<u8> {
    let mut key = b"submission/".to_vec();
    key.extend(id);
    key
}
