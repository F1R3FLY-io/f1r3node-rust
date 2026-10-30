// Quarantine of undecodable records in the node-local deploy stores. The
// behavior and the operator procedure are in docs/block-storage/README.md.
//
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use parking_lot::Mutex;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::{ByteBuffer, ByteString};

pub(super) type DeployTypedStore = KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>;

pub const QUARANTINED_RECORDS_METRIC: &str = "deploy_store.quarantined_records";
pub const RESTORED_RECORDS_METRIC: &str = "deploy_store.restored_records";

const MAX_LOGGED_RECORDS_PER_SCAN: usize = 10;
const MAX_LOGGED_KEY_BYTES: usize = 32;
const MAX_LOGGED_ERROR_CHARS: usize = 160;
const MAX_VERIFIED_RECORDS: usize = 16_384;
const VERIFICATION_PREFIX: &[u8] = b"F1R3VER\x01";

#[derive(Default)]
struct VerificationRead {
    learned: Vec<(ByteBuffer, ByteBuffer)>,
    unavailable: bool,
    known: Option<BTreeMap<ByteBuffer, ByteBuffer>>,
}

struct RecordError {
    reason: &'static str,
    error: KvStoreError,
}

struct DamagedRecord {
    key: ByteBuffer,
    value: ByteBuffer,
    error: KvStoreError,
    reason: &'static str,
}

#[derive(Clone)]
pub(super) struct DeployRecordQuarantine {
    store_name: &'static str,
    quarantine: Arc<dyn KeyValueStore>,
    verified: Arc<Mutex<BTreeMap<ByteBuffer, Blake2b256Hash>>>,
    verified_store: Option<Arc<dyn KeyValueStore>>,
    #[cfg(test)]
    verified_capacity: usize,
    #[cfg(test)]
    signature_checks: Arc<std::sync::atomic::AtomicUsize>,
}

impl DeployRecordQuarantine {
    pub(super) fn new(store_name: &'static str, quarantine: Arc<dyn KeyValueStore>) -> Self {
        Self {
            store_name,
            quarantine,
            verified: Arc::new(Mutex::new(BTreeMap::new())),
            verified_store: None,
            #[cfg(test)]
            verified_capacity: MAX_VERIFIED_RECORDS,
            #[cfg(test)]
            signature_checks: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    pub(super) fn with_verified_store(mut self, store: Arc<dyn KeyValueStore>) -> Self {
        self.verified_store = Some(store);
        self
    }

    fn proof(fingerprint: &Blake2b256Hash) -> ByteBuffer {
        let mut proof = VERIFICATION_PREFIX.to_vec();
        proof.extend_from_slice(&fingerprint.0);
        proof
    }

    pub(super) fn persist_verified(&self, records: Vec<(ByteBuffer, Blake2b256Hash)>) {
        self.persist_proofs(
            records
                .into_iter()
                .map(|(key, fingerprint)| (key, Self::proof(&fingerprint)))
                .collect(),
        );
    }

    fn persist_proofs(&self, records: Vec<(ByteBuffer, ByteBuffer)>) {
        let Some(store) = &self.verified_store else {
            return;
        };
        if records.is_empty() {
            return;
        }
        if let Err(error) = store.put(records) {
            tracing::warn!(store = self.store_name, %error, "Could not persist deploy verification evidence");
        }
    }

    fn persisted_match(
        &self,
        key: &ByteBuffer,
        fingerprint: &Blake2b256Hash,
        read: &mut VerificationRead,
    ) -> bool {
        let Some(store) = &self.verified_store else {
            return false;
        };
        if read.unavailable {
            return false;
        }
        if let Some(known) = &read.known {
            return known.get(key) == Some(&Self::proof(fingerprint));
        }
        match store.get_one(key) {
            Ok(proof) => proof.as_ref() == Some(&Self::proof(fingerprint)),
            Err(error) => {
                read.unavailable = true;
                tracing::warn!(store = self.store_name, %error, "Could not read deploy verification evidence");
                false
            }
        }
    }

    fn load_and_prune_proofs(
        &self,
        records: &BTreeMap<ByteBuffer, ByteBuffer>,
        read: &mut VerificationRead,
    ) {
        let Some(store) = &self.verified_store else {
            return;
        };
        let result = store.to_map().and_then(|known| {
            let stale: Vec<_> = known
                .keys()
                .filter(|key| !records.contains_key(*key))
                .cloned()
                .collect();
            if !stale.is_empty() {
                store.delete(stale)?;
            }
            Ok(known)
        });
        match result {
            Ok(known) => read.known = Some(known),
            Err(error) => {
                read.unavailable = true;
                tracing::warn!(store = self.store_name, %error, "Could not reconcile deploy verification evidence");
            }
        }
    }

    pub(super) fn remember_verified(&self, key: ByteBuffer, fingerprint: Blake2b256Hash) {
        let capacity = self.verified_capacity();
        if capacity == 0 {
            return;
        }
        let mut verified = self.verified.lock();
        if verified.len() >= capacity && !verified.contains_key(&key) {
            verified.pop_first();
        }
        verified.insert(key, fingerprint);
    }

    pub(super) fn forget(&self, key: &ByteBuffer) { self.verified.lock().remove(key); }

    pub(super) fn forget_all(&self, keys: &[ByteBuffer]) {
        {
            let mut verified = self.verified.lock();
            for key in keys {
                verified.remove(key);
            }
        }
        if let Some(store) = &self.verified_store {
            if !keys.is_empty() {
                let result = store.get(&keys.to_vec()).and_then(|proofs| {
                    let present: Vec<_> = keys
                        .iter()
                        .zip(proofs)
                        .filter_map(|(key, proof)| proof.map(|_| key.clone()))
                        .collect();
                    if present.is_empty() {
                        Ok(0)
                    } else {
                        store.delete(present)
                    }
                });
                if let Err(error) = result {
                    tracing::warn!(store = self.store_name, %error, "Could not remove deploy verification evidence");
                }
            }
        }
    }

    fn verified_capacity(&self) -> usize {
        #[cfg(test)]
        {
            self.verified_capacity
        }
        #[cfg(not(test))]
        {
            MAX_VERIFIED_RECORDS
        }
    }

    fn verify_signature(&self, deploy: &Signed<DeployData>) -> Result<(), RecordError> {
        #[cfg(test)]
        self.signature_checks
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        super::deploy_codec::verify_signature(deploy).map_err(|error| RecordError {
            reason: "signature",
            error,
        })
    }

    fn decode_verified(
        &self,
        store: &DeployTypedStore,
        key: &ByteBuffer,
        value: &ByteBuffer,
    ) -> Result<Signed<DeployData>, RecordError> {
        let mut read = VerificationRead::default();
        let result = self.decode_with_evidence(store, key, value, &mut read);
        self.persist_proofs(read.learned);
        result
    }

    fn decode_with_evidence(
        &self,
        store: &DeployTypedStore,
        key: &ByteBuffer,
        value: &ByteBuffer,
        read: &mut VerificationRead,
    ) -> Result<Signed<DeployData>, RecordError> {
        let deploy = decode_record(store, key, value).map_err(|error| RecordError {
            reason: "decode",
            error,
        })?;
        let fingerprint = Blake2b256Hash::new(value);
        let known = self.verified.lock().get(key) == Some(&fingerprint);
        if !known {
            if !self.persisted_match(key, &fingerprint, read) {
                self.verify_signature(&deploy)?;
                if self.verified_store.is_some() {
                    read.learned.push((key.clone(), Self::proof(&fingerprint)));
                }
            }
            self.remember_verified(key.clone(), fingerprint);
        }
        Ok(deploy)
    }

    pub(super) fn contains(
        &self,
        store: &DeployTypedStore,
        key: &ByteBuffer,
    ) -> Result<bool, KvStoreError> {
        self.inspect(store, key).map(|(_, verified)| verified)
    }

    pub(super) fn inspect(
        &self,
        store: &DeployTypedStore,
        key: &ByteBuffer,
    ) -> Result<(Option<ByteBuffer>, bool), KvStoreError> {
        let Some(value) = store.raw_store().get_one(key)? else {
            self.forget(key);
            return Ok((None, false));
        };
        let fingerprint = Blake2b256Hash::new(&value);
        if self.verified.lock().get(key) == Some(&fingerprint) {
            return Ok((Some(value), true));
        }
        match self.decode_verified(store, key, &value) {
            Ok(_) => Ok((Some(value), true)),
            Err(RecordError { error, reason }) => {
                self.isolate(store, vec![DamagedRecord {
                    key: key.clone(),
                    value: value.clone(),
                    error,
                    reason,
                }])?;
                Ok((Some(value), false))
            }
        }
    }

    pub(super) fn sigs(&self, store: &DeployTypedStore) -> Result<Vec<ByteString>, KvStoreError> {
        let mut sigs = Vec::new();
        let mut damaged = Vec::new();
        store.raw_store().iterate_while(&mut |key, value| {
            match store.decode_key(&key) {
                Ok(sig) => sigs.push(sig),
                Err(error) => damaged.push(DamagedRecord {
                    key,
                    value,
                    error,
                    reason: "decode",
                }),
            }
            Ok(true)
        })?;
        self.isolate(store, damaged)?;
        Ok(sigs)
    }

    pub(super) fn read_all(
        &self,
        store: &DeployTypedStore,
    ) -> Result<HashSet<Signed<DeployData>>, KvStoreError> {
        let mut deploys = HashSet::new();
        let mut damaged = Vec::new();
        let records = store.raw_store().to_map()?;
        let mut verification = VerificationRead::default();
        self.load_and_prune_proofs(&records, &mut verification);
        self.verified
            .lock()
            .retain(|key, _| records.contains_key(key));
        for (key, value) in records {
            match self.decode_with_evidence(store, &key, &value, &mut verification) {
                Ok(deploy) => {
                    deploys.insert(deploy);
                }
                Err(RecordError { error, reason }) => {
                    damaged.push(DamagedRecord {
                        key,
                        value,
                        error,
                        reason,
                    });
                }
            }
        }
        self.persist_proofs(verification.learned);
        self.isolate(store, damaged)?;
        Ok(deploys)
    }

    pub(super) fn any<F>(
        &self,
        store: &DeployTypedStore,
        mut predicate: F,
    ) -> Result<bool, KvStoreError>
    where
        F: FnMut(&Signed<DeployData>) -> Result<bool, KvStoreError>,
    {
        let mut matched = false;
        let mut predicate_error = None;
        let mut damaged = Vec::new();
        let mut verification = VerificationRead::default();
        store.raw_store().iterate_while(&mut |key, value| {
            match self.decode_with_evidence(store, &key, &value, &mut verification) {
                Ok(deploy) => match predicate(&deploy) {
                    Ok(true) => {
                        matched = true;
                        return Ok(false);
                    }
                    Ok(false) => {}
                    Err(error) => {
                        predicate_error = Some(error);
                        return Ok(false);
                    }
                },
                Err(RecordError { error, reason }) => {
                    damaged.push(DamagedRecord {
                        key,
                        value,
                        error,
                        reason,
                    });
                }
            }
            Ok(true)
        })?;
        self.persist_proofs(verification.learned);
        self.isolate(store, damaged)?;
        match predicate_error {
            Some(error) => Err(error),
            None => Ok(matched),
        }
    }

    pub(super) fn restore_readable(&self, store: &DeployTypedStore) -> Result<(), KvStoreError> {
        let quarantined = self.quarantine.to_map()?;
        if quarantined.is_empty() {
            return Ok(());
        }
        let total = quarantined.len();
        let live = store.raw_store();
        let (mut restored, mut duplicates, mut conflicts, mut removed) =
            (0usize, 0usize, 0usize, 0usize);
        let mut verification = VerificationRead::default();
        for (key, value) in quarantined {
            if self
                .decode_with_evidence(store, &key, &value, &mut verification)
                .is_err()
            {
                continue;
            }
            let inserted = live.put_one_if_absent(key.clone(), value.clone())?;
            if inserted {
                restored += 1;
            } else if live.get_one(&key)?.as_ref() != Some(&value) {
                conflicts += 1;
                continue;
            }
            if self.quarantine.delete_if_value(&key, &value)? {
                removed += 1;
                if !inserted {
                    duplicates += 1;
                }
            }
        }
        if restored > 0 {
            metrics::counter!(RESTORED_RECORDS_METRIC, "store" => self.store_name)
                .increment(restored as u64);
        }
        self.persist_proofs(verification.learned);
        tracing::warn!(
            store = self.store_name,
            restored,
            duplicates_removed = duplicates,
            kept_on_conflict = conflicts,
            remaining = total - removed,
            "Deploy store opened with quarantined records"
        );
        Ok(())
    }

    #[cfg(test)]
    fn verify_on_open(&self, store: &DeployTypedStore) -> Result<(), KvStoreError> {
        let mut damaged = Vec::new();
        store.raw_store().iterate_while(&mut |key, value| {
            if let Err(RecordError { error, reason }) = self.decode_verified(store, &key, &value) {
                damaged.push(DamagedRecord {
                    key,
                    value,
                    error,
                    reason,
                });
            }
            Ok(true)
        })?;
        self.isolate(store, damaged)
    }

    fn isolate(
        &self,
        store: &DeployTypedStore,
        damaged: Vec<DamagedRecord>,
    ) -> Result<(), KvStoreError> {
        if damaged.is_empty() {
            return Ok(());
        }
        let total = damaged.len();
        self.forget_all(
            &damaged
                .iter()
                .map(|record| record.key.clone())
                .collect::<Vec<_>>(),
        );
        let live = store.raw_store();
        let (mut moved, mut conflicts, mut logged) = (0usize, 0usize, 0usize);
        for record in damaged {
            let preserved = self
                .quarantine
                .put_one_if_absent(record.key.clone(), record.value.clone())?
                || self.quarantine.get_one(&record.key)?.as_ref() == Some(&record.value);
            if !preserved {
                conflicts += 1;
                if logged < MAX_LOGGED_RECORDS_PER_SCAN {
                    logged += 1;
                    tracing::warn!(
                        store = self.store_name,
                        reason = record.reason,
                        key_prefix = %key_prefix(&record.key),
                        key_len = record.key.len(),
                        value_len = record.value.len(),
                        error = %bounded_error(&record.error),
                        "Kept an invalid deploy record in place: its quarantine key holds different bytes"
                    );
                }
                continue;
            }
            if !live.delete_if_value(&record.key, &record.value)? {
                continue;
            }
            moved += 1;
            metrics::counter!(
                QUARANTINED_RECORDS_METRIC,
                "store" => self.store_name,
                "reason" => record.reason
            )
            .increment(1);
            if logged < MAX_LOGGED_RECORDS_PER_SCAN {
                logged += 1;
                tracing::warn!(
                    store = self.store_name,
                    reason = record.reason,
                    key_prefix = %key_prefix(&record.key),
                    key_len = record.key.len(),
                    value_len = record.value.len(),
                    error = %bounded_error(&record.error),
                    "Moved an invalid deploy record to quarantine"
                );
            }
        }
        if total > logged {
            tracing::warn!(
                store = self.store_name,
                moved,
                kept_on_conflict = conflicts,
                logged,
                "Found more invalid deploy records than were logged"
            );
        }
        Ok(())
    }
}

fn key_prefix(key: &ByteBuffer) -> String {
    hex::encode(&key[..key.len().min(MAX_LOGGED_KEY_BYTES)])
}

fn decode_record(
    store: &DeployTypedStore,
    key: &ByteBuffer,
    value: &ByteBuffer,
) -> Result<Signed<DeployData>, KvStoreError> {
    let signature = store.decode_key(key)?;
    let deploy = store.decode_value(value)?;
    if signature != deploy.sig.as_ref() {
        return Err(KvStoreError::SerializationError(
            "Deploy record key does not match its signature".to_string(),
        ));
    }
    Ok(deploy)
}

fn bounded_error(error: &KvStoreError) -> String {
    error
        .to_string()
        .chars()
        .take(MAX_LOGGED_ERROR_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Barrier, Mutex};

    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::deploy::key_value_deploy_storage::{
        KeyValueDeployStorage, KeyValueRejectedDeployBuffer, DEPLOY_STORAGE_DB,
        DEPLOY_STORAGE_QUARANTINE_DB, REJECTED_DEPLOY_BUFFER_DB,
        REJECTED_DEPLOY_BUFFER_QUARANTINE_DB,
    };

    const LEGACY_DEPLOY_HEX: &str = concat!(
        "03000000000000004e696c7b000000000000000700000000000000a086010000",
        "00000011000000000000000400000000000000726f6f7401e803000000000000",
        "4100000000000000041b84c5567b126440995d3ed5aaba0565d71e1834604819",
        "ff9c17f5e9d5dd078f70beaf8f588b541507fed6a642c5ab42dfdf8120a7f639",
        "de5122d47a69a8e8d14700000000000000304502210092526253b63faa274731",
        "a1907908ded5bb5e46fea2d28464985ca4bf3ec3c4b802206f05828da0310cfd",
        "c5e7da0cd34385559b5f6d4cf3b25ca78176416ecf6fadb50900000000000000",
        "736563703235366b31",
    );
    const MALFORMED_VALUE: &[u8] = &[0xff, 0x01];
    const UNSUPPORTED_FORMAT_VALUE: &[u8] = b"F1R3DEP\x02\x0a\x03Nil\x10\x7b";

    fn deploy(time_stamp: i64) -> Signed<DeployData> {
        signed(DeployData {
            term: "Nil".to_string(),
            time_stamp,
            phlo_price: 1,
            phlo_limit: 100_000,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        })
    }

    fn legacy_deploy() -> Signed<DeployData> {
        signed(DeployData {
            term: "Nil".to_string(),
            time_stamp: 123,
            phlo_price: 7,
            phlo_limit: 100_000,
            valid_after_block_number: 17,
            shard_id: "root".to_string(),
            expiration_timestamp: Some(1000),
            parameters: Vec::new(),
        })
    }

    fn signed(data: DeployData) -> Signed<DeployData> {
        Signed::create(data, Box::new(Secp256k1), PrivateKey::from_bytes(&[1; 32])).unwrap()
    }

    fn legacy_bytes() -> ByteBuffer { hex::decode(LEGACY_DEPLOY_HEX).unwrap() }

    fn key_for(sig: &[u8]) -> ByteBuffer {
        KeyValueTypedStoreImpl::<ByteString, Signed<DeployData>>::new(Arc::new(
            InMemoryKeyValueStore::new(),
        ))
        .encode_key(&sig.to_vec())
        .unwrap()
    }

    enum Pool {
        Storage(KeyValueDeployStorage),
        Buffer(KeyValueRejectedDeployBuffer),
    }

    impl Pool {
        fn open(
            name: &str,
            live: &Arc<dyn KeyValueStore>,
            quarantine: &Arc<dyn KeyValueStore>,
        ) -> Self {
            match name {
                DEPLOY_STORAGE_DB => Pool::Storage(
                    KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap(),
                ),
                _ => Pool::Buffer(
                    KeyValueRejectedDeployBuffer::from_stores(live.clone(), quarantine.clone())
                        .unwrap(),
                ),
            }
        }

        fn read_all(&self) -> Result<HashSet<Signed<DeployData>>, KvStoreError> {
            match self {
                Pool::Storage(storage) => storage.read_all(),
                Pool::Buffer(buffer) => buffer.read_all(),
            }
        }

        fn add(&mut self, deploys: Vec<Signed<DeployData>>) {
            match self {
                Pool::Storage(storage) => storage.add(deploys).unwrap(),
                Pool::Buffer(buffer) => buffer.add(deploys).unwrap(),
            }
        }

        fn contains_sig(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
            match self {
                Pool::Storage(storage) => storage.contains_sig(sig),
                Pool::Buffer(buffer) => buffer.contains_sig(sig),
            }
        }

        fn add_if_absent(&self, deploy: Signed<DeployData>) -> Result<bool, KvStoreError> {
            match self {
                Pool::Storage(storage) => storage.add_if_absent(deploy),
                Pool::Buffer(buffer) => buffer.add_if_absent(deploy),
            }
        }
    }

    const POOLS: [&str; 2] = [DEPLOY_STORAGE_DB, REJECTED_DEPLOY_BUFFER_DB];

    fn in_memory() -> Arc<dyn KeyValueStore> { Arc::new(InMemoryKeyValueStore::new()) }

    struct InterleavingStore {
        inner: Arc<dyn KeyValueStore>,
        before_delete: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        after_scan: Option<Arc<Barrier>>,
        reads: AtomicUsize,
        deletes: AtomicUsize,
    }

    impl InterleavingStore {
        fn new(inner: Arc<dyn KeyValueStore>, after_scan: Option<Arc<Barrier>>) -> Arc<Self> {
            Arc::new(Self {
                inner,
                before_delete: Mutex::new(None),
                after_scan,
                reads: AtomicUsize::new(0),
                deletes: AtomicUsize::new(0),
            })
        }

        fn run_hook(&self) {
            let hook = self.before_delete.lock().unwrap().take();
            if let Some(hook) = hook {
                hook();
            }
        }
    }

    impl KeyValueStore for InterleavingStore {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let values = self.inner.get(keys)?;
            self.run_hook();
            Ok(values)
        }

        fn put(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
            self.inner.put(pairs)
        }

        fn put_one_if_absent(
            &self,
            key: ByteBuffer,
            value: ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            self.inner.put_one_if_absent(key, value)
        }

        fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            self.run_hook();
            self.inner.delete(keys)
        }

        fn delete_if_value(
            &self,
            key: &ByteBuffer,
            expected: &ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            self.run_hook();
            self.inner.delete_if_value(key, expected)
        }

        fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
            for (key, value) in self.to_map()? {
                f(key, value);
            }
            Ok(())
        }

        fn iterate_while(
            &self,
            f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            for (key, value) in self.to_map()? {
                if !f(key, value)? {
                    break;
                }
            }
            Ok(())
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { self.inner.clone_box() }

        fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
            let records = self.inner.to_map()?;
            if let Some(barrier) = &self.after_scan {
                barrier.wait();
            }
            Ok(records)
        }

        fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

        fn size_bytes(&self) -> usize { self.inner.size_bytes() }
    }

    #[test]
    fn scans_preserve_a_live_record_replaced_before_deletion() {
        for name in POOLS {
            for use_any in [false, true] {
                let inner = in_memory();
                let live = InterleavingStore::new(inner.clone(), None);
                let quarantine = in_memory();
                let pool = Pool::open(name, &(live.clone() as Arc<dyn KeyValueStore>), &quarantine);
                let valid = deploy(1);
                let key = key_for(&valid.sig);
                inner
                    .put_one(key.clone(), MALFORMED_VALUE.to_vec())
                    .unwrap();
                let replacement = super::super::deploy_codec::encode(&valid).unwrap();
                let replacement_key = key.clone();
                let replacement_bytes = replacement.clone();
                let writer = inner.clone();
                *live.before_delete.lock().unwrap() = Some(Box::new(move || {
                    writer.put_one(replacement_key, replacement_bytes).unwrap();
                }));
                let recorder = DebuggingRecorder::new();
                let snapshotter = recorder.snapshotter();
                metrics::with_local_recorder(&recorder, || {
                    if use_any {
                        match &pool {
                            Pool::Storage(storage) => assert!(!storage.any(|_| Ok(false)).unwrap()),
                            Pool::Buffer(buffer) => assert!(!buffer.any(|_| Ok(false)).unwrap()),
                        }
                    } else {
                        assert!(pool.read_all().unwrap().is_empty());
                    }
                });
                assert_eq!(inner.get_one(&key).unwrap(), Some(replacement));
                assert_eq!(
                    quarantine.get_one(&key).unwrap(),
                    Some(MALFORMED_VALUE.to_vec())
                );
                assert_eq!(counter(&snapshotter, QUARANTINED_RECORDS_METRIC, name), 0);
                assert_eq!(pool.read_all().unwrap(), HashSet::from([valid]));
            }
        }
    }

    #[test]
    fn concurrent_scans_count_one_actual_quarantine_move() {
        let inner = in_memory();
        let live = InterleavingStore::new(inner.clone(), Some(Arc::new(Barrier::new(2))));
        let quarantine = in_memory();
        let mut storage =
            KeyValueDeployStorage::from_stores(inner.clone(), quarantine.clone()).unwrap();
        storage.store = DeployTypedStore::with_value_codec(
            live,
            super::super::deploy_codec::encode,
            super::super::deploy_codec::decode,
        );
        let key = key_for(&deploy(1).sig);
        inner
            .put_one(key.clone(), MALFORMED_VALUE.to_vec())
            .unwrap();
        let handles: Vec<_> = [false, true]
            .into_iter()
            .map(|use_any| {
                let storage = storage.clone();
                std::thread::spawn(move || {
                    let recorder = DebuggingRecorder::new();
                    let snapshotter = recorder.snapshotter();
                    metrics::with_local_recorder(&recorder, || {
                        if use_any {
                            assert!(!storage.any(|_| Ok(false)).unwrap());
                        } else {
                            assert!(storage.read_all().unwrap().is_empty());
                        }
                    });
                    counter(&snapshotter, QUARANTINED_RECORDS_METRIC, DEPLOY_STORAGE_DB)
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .sum::<u64>(),
            1
        );
        assert_eq!(inner.get_one(&key).unwrap(), None);
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
    }

    #[test]
    fn restore_preserves_quarantined_bytes_replaced_before_cleanup() {
        let live = in_memory();
        let inner = in_memory();
        let quarantine = InterleavingStore::new(inner.clone(), None);
        let valid = deploy(1);
        let key = key_for(&valid.sig);
        let encoded = super::super::deploy_codec::encode(&valid).unwrap();
        inner.put_one(key.clone(), encoded.clone()).unwrap();
        let writer = inner.clone();
        let replacement_key = key.clone();
        *quarantine.before_delete.lock().unwrap() = Some(Box::new(move || {
            writer
                .put_one(replacement_key, MALFORMED_VALUE.to_vec())
                .unwrap();
        }));
        KeyValueDeployStorage::from_stores(live.clone(), quarantine).unwrap();
        assert_eq!(live.get_one(&key).unwrap(), Some(encoded));
        assert_eq!(inner.get_one(&key).unwrap(), Some(MALFORMED_VALUE.to_vec()));
    }

    #[test]
    fn signature_removal_uses_one_delete_without_a_presence_read() {
        let live = InterleavingStore::new(in_memory(), None);
        let storage = KeyValueDeployStorage::from_stores(live.clone(), in_memory()).unwrap();
        let valid = deploy(1);
        storage.add(vec![valid.clone()]).unwrap();
        assert!(storage.remove_by_sig(&valid.sig).unwrap());
        assert!(!storage.remove_by_sig(&valid.sig).unwrap());
        assert_eq!(live.reads.load(Ordering::SeqCst), 0);
        assert_eq!(live.deletes.load(Ordering::SeqCst), 2);
    }

    fn counter(snapshotter: &Snapshotter, name: &str, store: &str) -> u64 {
        snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(key, _, _, _)| {
                key.key().name() == name
                    && key
                        .key()
                        .labels()
                        .any(|label| label.key() == "store" && label.value() == store)
            })
            .map(|(_, _, _, value)| match value {
                DebugValue::Counter(count) => count,
                _ => 0,
            })
            .sum()
    }

    fn seed_damaged(live: &Arc<dyn KeyValueStore>) -> (ByteBuffer, ByteBuffer) {
        let malformed_key = key_for(&[7; 64]);
        let unsupported_key = key_for(&[8; 64]);
        live.put(vec![
            (malformed_key.clone(), MALFORMED_VALUE.to_vec()),
            (unsupported_key.clone(), UNSUPPORTED_FORMAT_VALUE.to_vec()),
        ])
        .unwrap();
        (malformed_key, unsupported_key)
    }

    #[test]
    fn read_all_keeps_valid_and_legacy_records_and_quarantines_damaged_ones() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let mut pool = Pool::open(name, &live, &quarantine);
            let (d1, d2, legacy) = (deploy(1), deploy(2), legacy_deploy());
            pool.add(vec![d1.clone(), d2.clone()]);
            live.put_one(key_for(&legacy.sig), legacy_bytes()).unwrap();
            let (malformed_key, unsupported_key) = seed_damaged(&live);

            let expected = HashSet::from([d1, d2, legacy]);
            assert_eq!(pool.read_all().unwrap(), expected, "{name}");
            assert_eq!(
                quarantine.to_map().unwrap(),
                BTreeMap::from([
                    (malformed_key.clone(), MALFORMED_VALUE.to_vec()),
                    (unsupported_key.clone(), UNSUPPORTED_FORMAT_VALUE.to_vec()),
                ]),
                "{name}"
            );
            assert_eq!(
                live.get(&vec![malformed_key, unsupported_key]).unwrap(),
                vec![None, None],
                "{name}"
            );
            assert_eq!(pool.read_all().unwrap(), expected, "{name}");
        }
    }

    #[test]
    fn open_quarantines_a_versioned_record_with_an_invalid_signature() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let mut pool = Pool::open(name, &live, &quarantine);
            let valid = deploy(1);
            pool.add(vec![valid.clone()]);
            let mut tampered = deploy(2);
            let tampered_key = key_for(&tampered.sig);
            tampered.data.phlo_limit += 1;
            let tampered_bytes = versioned_bytes_unchecked(&tampered);
            live.put_one(tampered_key.clone(), tampered_bytes.clone())
                .unwrap();

            drop(pool);
            let reopened = Pool::open(name, &live, &quarantine);
            assert_eq!(
                reopened.read_all().unwrap(),
                HashSet::from([valid]),
                "{name}"
            );
            assert_eq!(
                quarantine.get_one(&tampered_key).unwrap(),
                Some(tampered_bytes),
                "{name}"
            );
        }
    }
    #[test]
    fn any_skips_damaged_records_and_still_finds_matches() {
        let (live, quarantine) = (in_memory(), in_memory());
        let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        storage.add(vec![deploy(1), deploy(2)]).unwrap();
        seed_damaged(&live);

        assert!(!storage.any(|_| Ok(false)).unwrap());
        assert_eq!(quarantine.to_map().unwrap().len(), 2);
        assert!(storage.any(|d| Ok(d.data.time_stamp == 2)).unwrap());
        assert!(matches!(
            storage.any(|_| Err(KvStoreError::InvalidArgument("predicate".to_string()))),
            Err(KvStoreError::InvalidArgument(_))
        ));
    }

    #[test]
    fn reopened_store_scans_cleanly_and_counts_each_damaged_record_once() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            for name in POOLS {
                let (live, quarantine) = (in_memory(), in_memory());
                let d1 = deploy(1);
                Pool::open(name, &live, &quarantine).add(vec![d1.clone()]);
                seed_damaged(&live);

                let reopened = Pool::open(name, &live, &quarantine);
                assert_eq!(reopened.read_all().unwrap(), HashSet::from([d1.clone()]));
                drop(reopened);

                let reopened = Pool::open(name, &live, &quarantine);
                assert_eq!(reopened.read_all().unwrap(), HashSet::from([d1]));
                assert_eq!(quarantine.to_map().unwrap().len(), 2, "{name}");
                assert_eq!(counter(&snapshotter, QUARANTINED_RECORDS_METRIC, name), 2);
                assert_eq!(counter(&snapshotter, RESTORED_RECORDS_METRIC, name), 0);
            }
        });
    }

    #[test]
    fn open_restores_quarantined_records_that_now_decode() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            for name in POOLS {
                let (live, quarantine) = (in_memory(), in_memory());
                let legacy = legacy_deploy();
                let legacy_key = key_for(&legacy.sig);
                quarantine
                    .put_one(legacy_key.clone(), legacy_bytes())
                    .unwrap();
                let (malformed_key, _) = seed_damaged(&quarantine);

                let pool = Pool::open(name, &live, &quarantine);

                assert_eq!(pool.read_all().unwrap(), HashSet::from([legacy]), "{name}");
                assert_eq!(quarantine.get_one(&legacy_key).unwrap(), None, "{name}");
                assert!(
                    quarantine.get_one(&malformed_key).unwrap().is_some(),
                    "{name}"
                );
                assert_eq!(counter(&snapshotter, RESTORED_RECORDS_METRIC, name), 1);
            }
        });
    }

    #[test]
    fn open_keeps_a_readable_quarantined_copy_when_the_live_key_holds_other_bytes() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            let legacy = legacy_deploy();
            let legacy_key = key_for(&legacy.sig);
            for live_bytes in [None, Some(MALFORMED_VALUE.to_vec())] {
                let (live, quarantine) = (in_memory(), in_memory());
                let storage =
                    KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
                match &live_bytes {
                    Some(bytes) => live.put_one(legacy_key.clone(), bytes.clone()).unwrap(),
                    None => storage
                        .store
                        .put_one(legacy.sig.to_vec(), legacy.clone())
                        .unwrap(),
                }
                let live_before = live.get_one(&legacy_key).unwrap();
                quarantine
                    .put_one(legacy_key.clone(), legacy_bytes())
                    .unwrap();

                let reopened =
                    KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
                reopened.read_all().unwrap();

                assert_eq!(live.get_one(&legacy_key).unwrap(), live_before);
                assert_eq!(
                    quarantine.get_one(&legacy_key).unwrap(),
                    Some(legacy_bytes())
                );
            }
            assert_eq!(
                counter(&snapshotter, RESTORED_RECORDS_METRIC, DEPLOY_STORAGE_DB),
                0
            );
        });
    }

    #[test]
    fn open_removes_a_quarantined_copy_identical_to_the_live_record() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            let (live, quarantine) = (in_memory(), in_memory());
            let legacy = legacy_deploy();
            let legacy_key = key_for(&legacy.sig);
            live.put_one(legacy_key.clone(), legacy_bytes()).unwrap();
            quarantine
                .put_one(legacy_key.clone(), legacy_bytes())
                .unwrap();

            KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();

            assert_eq!(live.get_one(&legacy_key).unwrap(), Some(legacy_bytes()));
            assert!(!quarantine.non_empty().unwrap());
            assert_eq!(
                counter(&snapshotter, RESTORED_RECORDS_METRIC, DEPLOY_STORAGE_DB),
                0
            );
        });
    }

    #[test]
    fn scan_keeps_a_damaged_record_when_its_quarantine_key_holds_different_bytes() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let pool = Pool::open(name, &live, &quarantine);
            let (malformed_key, _) = seed_damaged(&live);
            quarantine
                .put_one(malformed_key.clone(), vec![0xee])
                .unwrap();

            assert!(pool.read_all().unwrap().is_empty(), "{name}");
            assert_eq!(
                quarantine.get_one(&malformed_key).unwrap(),
                Some(vec![0xee]),
                "{name}"
            );
            assert_eq!(
                live.get_one(&malformed_key).unwrap(),
                Some(MALFORMED_VALUE.to_vec()),
                "{name}"
            );
        }
    }

    #[test]
    fn scan_completes_a_move_interrupted_after_the_quarantine_write() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let pool = Pool::open(name, &live, &quarantine);
            let (malformed_key, _) = seed_damaged(&live);
            quarantine
                .put_one(malformed_key.clone(), MALFORMED_VALUE.to_vec())
                .unwrap();

            assert!(pool.read_all().unwrap().is_empty(), "{name}");
            assert_eq!(live.get_one(&malformed_key).unwrap(), None, "{name}");
            assert_eq!(
                quarantine.get_one(&malformed_key).unwrap(),
                Some(MALFORMED_VALUE.to_vec()),
                "{name}"
            );
        }
    }

    #[test]
    fn any_quarantines_damaged_records_seen_before_a_predicate_error() {
        let live = FailingStore::new(false, false);
        let live_dyn: Arc<dyn KeyValueStore> = live.clone();
        let quarantine = in_memory();
        let storage =
            KeyValueDeployStorage::from_stores(live_dyn.clone(), quarantine.clone()).unwrap();
        let valid = deploy(1);
        storage.add(vec![valid.clone()]).unwrap();
        let malformed_key = key_for(&[0; 64]);
        live.put_one(malformed_key.clone(), MALFORMED_VALUE.to_vec())
            .unwrap();
        assert!(malformed_key < key_for(&valid.sig));

        assert!(matches!(
            storage.any(|_| Err(KvStoreError::InvalidArgument("predicate".to_string()))),
            Err(KvStoreError::InvalidArgument(_))
        ));
        assert_eq!(
            quarantine.get_one(&malformed_key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
        assert_eq!(live.get_one(&malformed_key).unwrap(), None);
    }

    #[tokio::test]
    async fn store_manager_opens_quarantine_next_to_each_store() {
        for (name, quarantine_name) in [
            (DEPLOY_STORAGE_DB, DEPLOY_STORAGE_QUARANTINE_DB),
            (
                REJECTED_DEPLOY_BUFFER_DB,
                REJECTED_DEPLOY_BUFFER_QUARANTINE_DB,
            ),
        ] {
            let mut kvm = InMemoryStoreManager::new();
            let live = kvm.store(name.to_string()).await.unwrap();
            let d1 = deploy(1);
            let (malformed_key, _) = seed_damaged(&live);
            let read = match name {
                DEPLOY_STORAGE_DB => {
                    let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
                    storage.add(vec![d1.clone()]).unwrap();
                    storage.read_all().unwrap()
                }
                _ => {
                    let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
                    buffer.add(vec![d1.clone()]).unwrap();
                    buffer.read_all().unwrap()
                }
            };
            assert_eq!(read, HashSet::from([d1]));
            let quarantine = kvm.store(quarantine_name.to_string()).await.unwrap();
            assert_eq!(
                quarantine.get_one(&malformed_key).unwrap(),
                Some(MALFORMED_VALUE.to_vec())
            );
        }
    }

    struct FailingStore {
        inner: InMemoryKeyValueStore,
        fail_scans: AtomicBool,
        fail_writes: AtomicBool,
    }

    impl FailingStore {
        fn new(fail_scans: bool, fail_writes: bool) -> Arc<Self> {
            Arc::new(Self {
                inner: InMemoryKeyValueStore::new(),
                fail_scans: AtomicBool::new(fail_scans),
                fail_writes: AtomicBool::new(fail_writes),
            })
        }

        fn check(flag: &AtomicBool) -> Result<(), KvStoreError> {
            if flag.load(Ordering::SeqCst) {
                Err(KvStoreError::IoError("backend unavailable".to_string()))
            } else {
                Ok(())
            }
        }
    }

    impl KeyValueStore for FailingStore {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
            self.inner.get(keys)
        }

        fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
            Self::check(&self.fail_writes)?;
            self.inner.put(kv_pairs)
        }

        fn put_one_if_absent(
            &self,
            key: ByteBuffer,
            value: ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            Self::check(&self.fail_writes)?;
            self.inner.put_one_if_absent(key, value)
        }

        fn delete_if_value(&self, key: &Vec<u8>, expected: &Vec<u8>) -> Result<bool, KvStoreError> {
            Self::check(&self.fail_writes)?;
            self.inner.delete_if_value(key, expected)
        }

        fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
            Self::check(&self.fail_writes)?;
            self.inner.delete(keys)
        }

        fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
            self.inner.iterate(f)
        }

        fn iterate_while(
            &self,
            f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            Self::check(&self.fail_scans)?;
            for (key, value) in self.inner.to_map()? {
                if !f(key, value)? {
                    break;
                }
            }
            Ok(())
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { self.inner.clone_box() }

        fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
            Self::check(&self.fail_scans)?;
            self.inner.to_map()
        }

        fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

        fn size_bytes(&self) -> usize { self.inner.size_bytes() }
    }

    #[test]
    fn backend_scan_failures_fail_the_operation_without_quarantining() {
        let live = FailingStore::new(false, false);
        let quarantine = in_memory();
        let live_dyn: Arc<dyn KeyValueStore> = live.clone();
        let storage =
            KeyValueDeployStorage::from_stores(live_dyn.clone(), quarantine.clone()).unwrap();
        seed_damaged(&live_dyn);
        live.fail_scans.store(true, Ordering::SeqCst);

        assert!(matches!(storage.read_all(), Err(KvStoreError::IoError(_))));
        assert!(matches!(
            storage.any(|_| Ok(false)),
            Err(KvStoreError::IoError(_))
        ));
        assert!(!quarantine.non_empty().unwrap());
        assert_eq!(live.inner.num_records(), 2);
    }

    #[test]
    fn quarantine_write_failures_fail_the_scan_and_keep_the_record() {
        let live = in_memory();
        let quarantine = FailingStore::new(false, true);
        let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        storage
            .store
            .put_one(deploy(1).sig.to_vec(), deploy(1))
            .unwrap();
        let (malformed_key, _) = seed_damaged(&live);

        assert!(matches!(storage.read_all(), Err(KvStoreError::IoError(_))));
        assert_eq!(
            live.get_one(&malformed_key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
    }

    #[test]
    fn conditional_delete_failures_preserve_both_copies_until_retry() {
        let live = FailingStore::new(false, false);
        let quarantine = in_memory();
        let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        let key = key_for(&deploy(1).sig);
        live.inner
            .put_one(key.clone(), MALFORMED_VALUE.to_vec())
            .unwrap();
        live.fail_writes.store(true, Ordering::SeqCst);
        assert!(matches!(storage.read_all(), Err(KvStoreError::IoError(_))));
        assert_eq!(
            live.inner.get_one(&key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
        live.fail_writes.store(false, Ordering::SeqCst);
        assert!(storage.read_all().unwrap().is_empty());
        assert_eq!(live.inner.get_one(&key).unwrap(), None);
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
    }

    #[test]
    fn logged_error_text_is_bounded() {
        let error = KvStoreError::SerializationError("x".repeat(10_000));
        assert_eq!(
            bounded_error(&error).chars().count(),
            MAX_LOGGED_ERROR_CHARS
        );
    }

    fn versioned_bytes_unchecked(deploy: &Signed<DeployData>) -> ByteBuffer {
        super::super::deploy_codec::encode_unchecked(deploy)
    }

    #[tokio::test]
    async fn persisted_verification_survives_lmdb_reopen_without_signature_checks() {
        use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
            Db, LmdbDirStoreManager, LmdbEnvConfig,
        };
        for name in POOLS {
            let directory = tempfile::tempdir().unwrap();
            let expected = HashSet::from([deploy(1), legacy_deploy()]);
            for pass in 0..2 {
                let mapping = [
                    (name, "data"),
                    ("quarantine", "data"),
                    ("verified", "evidence"),
                ]
                .into_iter()
                .map(|(database, environment)| {
                    (
                        Db::new(database.to_string(), None),
                        LmdbEnvConfig::new(environment.to_string(), 16 * 1024 * 1024),
                    )
                })
                .collect();
                let mut manager = LmdbDirStoreManager::new(directory.path().to_path_buf(), mapping);
                let live = manager.store(name.to_string()).await.unwrap();
                let quarantine = manager.store("quarantine".to_string()).await.unwrap();
                let proofs = manager.store("verified".to_string()).await.unwrap();
                if pass == 0 {
                    live.put(vec![
                        (
                            key_for(&deploy(1).sig),
                            versioned_bytes_unchecked(&deploy(1)),
                        ),
                        (key_for(&legacy_deploy().sig), legacy_bytes()),
                    ])
                    .unwrap();
                }
                let store = DeployTypedStore::with_value_codec(
                    live,
                    super::super::deploy_codec::encode,
                    super::super::deploy_codec::decode,
                );
                let guard = DeployRecordQuarantine::new(name, quarantine)
                    .with_verified_store(proofs.clone());
                assert_eq!(guard.read_all(&store).unwrap(), expected);
                assert_eq!(
                    guard.signature_checks.load(Ordering::SeqCst),
                    if pass == 0 { 2 } else { 0 }
                );
                assert_eq!(proofs.to_map().unwrap().len(), 2);
            }
        }
    }

    #[tokio::test]
    async fn stores_without_a_verification_mapping_keep_signature_checks_enabled() {
        use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
            Db, LmdbDirStoreManager, LmdbEnvConfig,
        };
        for name in POOLS {
            let directory = tempfile::tempdir().unwrap();
            let quarantine = format!("{name}_quarantine");
            let mapping = [name, quarantine.as_str()]
                .into_iter()
                .map(|name| {
                    (
                        Db::new(name.to_string(), None),
                        LmdbEnvConfig::new("data".to_string(), 16 * 1024 * 1024),
                    )
                })
                .collect();
            let mut manager = LmdbDirStoreManager::new(directory.path().to_path_buf(), mapping);
            let mut pool = match name {
                DEPLOY_STORAGE_DB => {
                    Pool::Storage(KeyValueDeployStorage::new(&mut manager).await.unwrap())
                }
                _ => Pool::Buffer(
                    KeyValueRejectedDeployBuffer::new(&mut manager)
                        .await
                        .unwrap(),
                ),
            };
            let candidate = deploy(7);
            pool.add(vec![candidate.clone()]);
            assert_eq!(pool.read_all().unwrap(), HashSet::from([candidate]));
        }
    }

    #[test]
    fn checked_writes_persist_evidence_and_removals_clean_it_up() {
        for name in POOLS {
            let (live, quarantine, proofs) = (in_memory(), in_memory(), in_memory());
            let pool = match name {
                DEPLOY_STORAGE_DB => Pool::Storage(
                    KeyValueDeployStorage::from_stores_with_verification(
                        live.clone(),
                        quarantine.clone(),
                        Some(proofs.clone()),
                    )
                    .unwrap(),
                ),
                _ => Pool::Buffer(
                    KeyValueRejectedDeployBuffer::from_stores_with_verification(
                        live.clone(),
                        quarantine.clone(),
                        Some(proofs.clone()),
                    )
                    .unwrap(),
                ),
            };
            let candidate = deploy(9);
            assert!(pool.add_if_absent(candidate.clone()).unwrap());
            assert!(proofs.get_one(&key_for(&candidate.sig)).unwrap().is_some());
            let store = DeployTypedStore::with_value_codec(
                live,
                super::super::deploy_codec::encode,
                super::super::deploy_codec::decode,
            );
            let cold =
                DeployRecordQuarantine::new(name, quarantine).with_verified_store(proofs.clone());
            assert_eq!(
                cold.read_all(&store).unwrap(),
                HashSet::from([candidate.clone()])
            );
            assert_eq!(cold.signature_checks.load(Ordering::SeqCst), 0);
            match pool {
                Pool::Storage(storage) => assert!(storage.remove_by_sig(&candidate.sig).unwrap()),
                Pool::Buffer(buffer) => assert!(buffer.remove_by_sig(&candidate.sig).unwrap()),
            }
            assert!(!proofs.non_empty().unwrap());
        }
    }

    #[test]
    fn persistent_evidence_does_not_approve_changed_or_corrupt_records() {
        for name in POOLS {
            let (live, quarantine, proofs) = (in_memory(), in_memory(), in_memory());
            let store = DeployTypedStore::with_value_codec(
                live.clone(),
                super::super::deploy_codec::encode,
                super::super::deploy_codec::decode,
            );
            let candidate = deploy(1);
            live.put_one(
                key_for(&candidate.sig),
                versioned_bytes_unchecked(&candidate),
            )
            .unwrap();
            let guard = DeployRecordQuarantine::new(name, quarantine.clone())
                .with_verified_store(proofs.clone());
            assert_eq!(guard.read_all(&store).unwrap().len(), 1);
            proofs
                .put_one(key_for(&candidate.sig), b"invalid-evidence".to_vec())
                .unwrap();
            let repaired = DeployRecordQuarantine::new(name, quarantine.clone())
                .with_verified_store(proofs.clone());
            assert_eq!(repaired.read_all(&store).unwrap().len(), 1);
            assert_eq!(repaired.signature_checks.load(Ordering::SeqCst), 1);
            assert!(!quarantine.non_empty().unwrap());
            let mut changed = candidate;
            changed.data.term.push(' ');
            live.put_one(key_for(&changed.sig), versioned_bytes_unchecked(&changed))
                .unwrap();
            let cold = DeployRecordQuarantine::new(name, quarantine.clone())
                .with_verified_store(proofs.clone());
            assert!(cold.read_all(&store).unwrap().is_empty());
            assert_eq!(cold.signature_checks.load(Ordering::SeqCst), 1);
            assert!(quarantine.non_empty().unwrap());
            assert!(!proofs.non_empty().unwrap());
        }
    }

    #[test]
    fn verification_evidence_failures_preserve_accepted_data_and_fall_back_to_checks() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let proofs = FailingStore::new(true, true);
            let pool = match name {
                DEPLOY_STORAGE_DB => Pool::Storage(
                    KeyValueDeployStorage::from_stores_with_verification(
                        live.clone(),
                        quarantine.clone(),
                        Some(proofs.clone()),
                    )
                    .unwrap(),
                ),
                _ => Pool::Buffer(
                    KeyValueRejectedDeployBuffer::from_stores_with_verification(
                        live.clone(),
                        quarantine.clone(),
                        Some(proofs.clone()),
                    )
                    .unwrap(),
                ),
            };
            let candidate = deploy(5);
            assert!(pool.add_if_absent(candidate.clone()).unwrap());
            let store = DeployTypedStore::with_value_codec(
                live,
                super::super::deploy_codec::encode,
                super::super::deploy_codec::decode,
            );
            let cold =
                DeployRecordQuarantine::new(name, quarantine).with_verified_store(proofs.clone());
            assert_eq!(cold.read_all(&store).unwrap(), HashSet::from([candidate]));
            assert_eq!(cold.signature_checks.load(Ordering::SeqCst), 1);
            assert!(!proofs.inner.non_empty().unwrap());
        }
    }

    #[test]
    fn a_failed_prepared_write_propagates_the_error_and_can_be_retried() {
        let live = FailingStore::new(false, false);
        let storage = KeyValueDeployStorage::from_stores(live.clone(), in_memory()).unwrap();
        let candidate = deploy(1);
        let prepared = storage.prepare(&candidate).unwrap();
        live.fail_writes.store(true, Ordering::SeqCst);

        assert!(matches!(
            storage.add_prepared(vec![prepared]),
            Err(KvStoreError::IoError(_))
        ));
        assert!(!live.inner.non_empty().unwrap());

        live.fail_writes.store(false, Ordering::SeqCst);
        storage.add(vec![candidate.clone()]).unwrap();
        assert_eq!(storage.read_all().unwrap(), HashSet::from([candidate]));
    }

    #[test]
    fn fingerprints_reuse_signature_checks_and_verify_again_after_a_cache_miss() {
        use crypto::rust::public_key::PublicKey;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;

        static CHECKS: AtomicUsize = AtomicUsize::new(0);

        #[derive(Clone, Debug)]
        struct CountChecks;

        impl SignaturesAlg for CountChecks {
            fn verify(&self, data: &[u8], signature: &[u8], pub_key: &[u8]) -> bool {
                CHECKS.fetch_add(1, Ordering::SeqCst);
                Secp256k1.verify(data, signature, pub_key)
            }
            fn sign(&self, data: &[u8], sec: &[u8]) -> Vec<u8> { Secp256k1.sign(data, sec) }
            fn to_public(&self, sec: &PrivateKey) -> PublicKey { Secp256k1.to_public(sec) }
            fn new_key_pair(&self) -> (PrivateKey, PublicKey) { Secp256k1.new_key_pair() }
            fn name(&self) -> String { Secp256k1.name() }
            fn sig_length(&self) -> usize { Secp256k1.sig_length() }
            fn eq(&self, other: &dyn SignaturesAlg) -> bool { self.name() == other.name() }
            fn box_clone(&self) -> Box<dyn SignaturesAlg> { Box::new(self.clone()) }
        }

        fn decode(value: &ByteBuffer) -> Result<Signed<DeployData>, KvStoreError> {
            let mut deploy = super::super::deploy_codec::decode(value)?;
            deploy.sig_algorithm = Box::new(CountChecks);
            Ok(deploy)
        }

        let live = in_memory();
        let store = DeployTypedStore::with_value_codec(
            live.clone(),
            super::super::deploy_codec::encode,
            decode,
        );
        let guard = DeployRecordQuarantine::new(DEPLOY_STORAGE_DB, in_memory());
        let mut candidate = deploy(1);
        let key = key_for(&candidate.sig);
        live.put_one(
            key.clone(),
            super::super::deploy_codec::encode(&candidate).unwrap(),
        )
        .unwrap();
        guard.verify_on_open(&store).unwrap();
        assert_eq!(CHECKS.load(Ordering::SeqCst), 1);
        assert_eq!(guard.read_all(&store).unwrap().len(), 1);
        assert!(guard.any(&store, |_| Ok(true)).unwrap());
        assert_eq!(CHECKS.load(Ordering::SeqCst), 1);

        guard.forget(&key);
        assert_eq!(guard.read_all(&store).unwrap().len(), 1);
        assert_eq!(CHECKS.load(Ordering::SeqCst), 2);
        candidate.data.phlo_limit += 1;
        live.put_one(key, versioned_bytes_unchecked(&candidate))
            .unwrap();
        assert!(guard.read_all(&store).unwrap().is_empty());
        assert_eq!(CHECKS.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn verified_cache_eviction_preserves_deploys_and_rechecks_evicted_bytes() {
        let (live, quarantine) = (in_memory(), in_memory());
        let store = DeployTypedStore::with_value_codec(
            live.clone(),
            super::super::deploy_codec::encode,
            super::super::deploy_codec::decode,
        );
        let mut guard = DeployRecordQuarantine::new(DEPLOY_STORAGE_DB, quarantine.clone());
        guard.verified_capacity = 1;
        let candidates = [deploy(1), deploy(2)];
        live.put(
            candidates
                .iter()
                .map(|candidate| {
                    (
                        key_for(&candidate.sig),
                        versioned_bytes_unchecked(candidate),
                    )
                })
                .collect(),
        )
        .unwrap();

        guard.verify_on_open(&store).unwrap();
        assert_eq!(guard.verified.lock().len(), 1);
        assert_eq!(
            guard.read_all(&store).unwrap(),
            HashSet::from(candidates.clone())
        );
        assert_eq!(guard.verified.lock().len(), 1);
        let mut evicted = candidates
            .into_iter()
            .find(|candidate| !guard.verified.lock().contains_key(&key_for(&candidate.sig)))
            .unwrap();
        let checks = guard.signature_checks.load(Ordering::Relaxed);
        assert!(guard
            .any(&store, |candidate| Ok(candidate.sig == evicted.sig))
            .unwrap());
        assert!(guard.signature_checks.load(Ordering::Relaxed) > checks);

        evicted.data.phlo_limit += 1;
        let key = key_for(&evicted.sig);
        let bytes = versioned_bytes_unchecked(&evicted);
        live.put_one(key.clone(), bytes.clone()).unwrap();
        assert!(!guard
            .any(&store, |candidate| Ok(candidate.sig == evicted.sig))
            .unwrap());
        assert_eq!(live.get_one(&key).unwrap(), None);
        assert_eq!(quarantine.get_one(&key).unwrap(), Some(bytes));
        assert!(guard.verified.lock().len() <= 1);
    }

    #[test]
    fn full_scan_removes_fingerprints_deleted_through_another_handle() {
        let live = in_memory();
        let store = DeployTypedStore::with_value_codec(
            live.clone(),
            super::super::deploy_codec::encode,
            super::super::deploy_codec::decode,
        );
        let guard = DeployRecordQuarantine::new(DEPLOY_STORAGE_DB, in_memory());
        let shared = guard.clone();
        let candidate = deploy(1);
        let key = key_for(&candidate.sig);
        live.put_one(key.clone(), versioned_bytes_unchecked(&candidate))
            .unwrap();
        guard.verify_on_open(&store).unwrap();
        assert_eq!(shared.verified.lock().len(), 1);

        live.delete(vec![key.clone()]).unwrap();
        assert!(shared.read_all(&store).unwrap().is_empty());
        assert!(guard.verified.lock().is_empty());
        let checks = guard.signature_checks.load(Ordering::Relaxed);
        live.put_one(key, versioned_bytes_unchecked(&candidate))
            .unwrap();
        assert_eq!(guard.read_all(&store).unwrap(), HashSet::from([candidate]));
        assert_eq!(guard.signature_checks.load(Ordering::Relaxed), checks + 1);
    }

    #[test]
    fn scans_reject_changed_signed_bytes_after_a_verified_write() {
        for name in POOLS {
            for use_any in [false, true] {
                let (live, quarantine) = (in_memory(), in_memory());
                let mut pool = Pool::open(name, &live, &quarantine);
                let mut tampered = deploy(1);
                pool.add(vec![tampered.clone()]);
                let key = key_for(&tampered.sig);
                tampered.data.phlo_limit += 1;
                let bytes = versioned_bytes_unchecked(&tampered);
                live.put_one(key.clone(), bytes.clone()).unwrap();

                if use_any {
                    let matched = match &pool {
                        Pool::Storage(storage) => storage.any(|_| Ok(true)),
                        Pool::Buffer(buffer) => buffer.any(|_| Ok(true)),
                    }
                    .unwrap();
                    assert!(!matched);
                } else {
                    assert!(pool.read_all().unwrap().is_empty());
                }
                assert_eq!(live.get_one(&key).unwrap(), None, "{name}");
                assert_eq!(quarantine.get_one(&key).unwrap(), Some(bytes), "{name}");
            }
        }
    }

    #[test]
    fn a_replacement_during_quarantine_is_checked_on_the_next_read() {
        let inner = in_memory();
        let live = InterleavingStore::new(inner.clone(), None);
        let quarantine = in_memory();
        let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        let mut tampered = deploy(1);
        let key = key_for(&tampered.sig);
        tampered.data.phlo_limit += 1;
        let replacement = versioned_bytes_unchecked(&tampered);
        inner
            .put_one(key.clone(), MALFORMED_VALUE.to_vec())
            .unwrap();
        let writer = inner.clone();
        let replaced_key = key.clone();
        let replaced_bytes = replacement.clone();
        *live.before_delete.lock().unwrap() = Some(Box::new(move || {
            writer.put_one(replaced_key, replaced_bytes).unwrap();
        }));

        assert!(storage.read_all().unwrap().is_empty());
        assert!(storage.read_all().unwrap().is_empty());
        assert!(!storage.any(|_| Ok(true)).unwrap());
        assert_eq!(inner.get_one(&key).unwrap(), Some(replacement));
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
    }

    #[test]
    fn quarantine_metrics_separate_decode_and_signature_failures() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, || {
            let (live, quarantine) = (in_memory(), in_memory());
            let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine).unwrap();
            let mut tampered = deploy(1);
            tampered.data.phlo_limit += 1;
            live.put(vec![
                (key_for(&tampered.sig), versioned_bytes_unchecked(&tampered)),
                (key_for(&[7; 64]), MALFORMED_VALUE.to_vec()),
            ])
            .unwrap();
            assert!(storage.read_all().unwrap().is_empty());
            assert!(storage.read_all().unwrap().is_empty());
        });
        let reasons: BTreeMap<_, _> = snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(key, _, _, _)| key.key().name() == QUARANTINED_RECORDS_METRIC)
            .filter_map(|(key, _, _, value)| {
                let reason = key
                    .key()
                    .labels()
                    .find(|label| label.key() == "reason")?
                    .value()
                    .to_string();
                match value {
                    DebugValue::Counter(count) => Some((reason, count)),
                    _ => None,
                }
            })
            .collect();
        assert_eq!(
            reasons,
            BTreeMap::from([("decode".to_string(), 1), ("signature".to_string(), 1),])
        );
    }

    #[test]
    fn signature_listing_quarantines_bad_keys_without_decoding_values() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let candidate = deploy(1);
            let key = key_for(&candidate.sig);
            let bad_key = vec![0xff];
            live.put(vec![
                (key.clone(), MALFORMED_VALUE.to_vec()),
                (bad_key.clone(), MALFORMED_VALUE.to_vec()),
            ])
            .unwrap();
            let pool = Pool::open(name, &live, &quarantine);
            let signatures = match &pool {
                Pool::Storage(storage) => storage.sigs(),
                Pool::Buffer(buffer) => buffer.sigs(),
            }
            .unwrap();

            assert_eq!(signatures, vec![candidate.sig.to_vec()]);
            assert_eq!(live.get_one(&key).unwrap(), Some(MALFORMED_VALUE.to_vec()));
            assert_eq!(live.get_one(&bad_key).unwrap(), None);
            assert_eq!(
                quarantine.get_one(&bad_key).unwrap(),
                Some(MALFORMED_VALUE.to_vec())
            );
            assert!(pool.read_all().unwrap().is_empty());
            assert_eq!(
                quarantine.get_one(&key).unwrap(),
                Some(MALFORMED_VALUE.to_vec())
            );
        }
    }

    #[test]
    fn signature_lookup_checks_only_the_requested_record_after_open() {
        for name in POOLS {
            let live = FailingStore::new(true, false);
            let quarantine = in_memory();
            let candidate = deploy(1);
            let key = key_for(&candidate.sig);
            let untouched_key = key_for(&deploy(2).sig);
            live.inner
                .put(vec![
                    (key.clone(), versioned_bytes_unchecked(&candidate)),
                    (untouched_key.clone(), MALFORMED_VALUE.to_vec()),
                ])
                .unwrap();
            let live_dyn: Arc<dyn KeyValueStore> = live.clone();
            let pool = Pool::open(name, &live_dyn, &quarantine);

            assert!(pool.contains_sig(&candidate.sig).unwrap());
            assert!(pool.contains_sig(&candidate.sig).unwrap());
            assert!(!quarantine.non_empty().unwrap());
            let mut tampered = candidate.clone();
            tampered.data.phlo_limit += 1;
            let bytes = versioned_bytes_unchecked(&tampered);
            live.inner.put_one(key.clone(), bytes.clone()).unwrap();
            assert!(!pool.contains_sig(&candidate.sig).unwrap());
            assert_eq!(quarantine.get_one(&key).unwrap(), Some(bytes));
            assert_eq!(
                live.inner.get_one(&untouched_key).unwrap(),
                Some(MALFORMED_VALUE.to_vec())
            );
        }
    }

    #[test]
    fn conditional_insertion_quarantines_invalid_existing_bytes_before_acceptance() {
        for name in POOLS {
            let candidate = deploy(1);
            let mut tampered = candidate.clone();
            tampered.data.phlo_limit += 1;
            for bytes in [
                MALFORMED_VALUE.to_vec(),
                versioned_bytes_unchecked(&tampered),
                versioned_bytes_unchecked(&deploy(2)),
            ] {
                let (live, quarantine) = (in_memory(), in_memory());
                let key = key_for(&candidate.sig);
                live.put_one(key.clone(), bytes.clone()).unwrap();
                let pool = Pool::open(name, &live, &quarantine);

                assert!(pool.add_if_absent(candidate.clone()).unwrap());
                assert!(pool.contains_sig(&candidate.sig).unwrap());
                assert!(!pool.add_if_absent(candidate.clone()).unwrap());
                assert_eq!(pool.read_all().unwrap(), HashSet::from([candidate.clone()]));
                assert_eq!(quarantine.get_one(&key).unwrap(), Some(bytes));
            }
        }
    }

    #[test]
    fn conditional_insertion_reports_an_invalid_quarantine_conflict_without_overwriting() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let candidate = deploy(1);
            let key = key_for(&candidate.sig);
            live.put_one(key.clone(), MALFORMED_VALUE.to_vec()).unwrap();
            quarantine.put_one(key.clone(), vec![0xee]).unwrap();
            let pool = Pool::open(name, &live, &quarantine);

            assert!(matches!(
                pool.add_if_absent(candidate),
                Err(KvStoreError::InvalidArgument(_))
            ));
            assert_eq!(live.get_one(&key).unwrap(), Some(MALFORMED_VALUE.to_vec()));
            assert_eq!(quarantine.get_one(&key).unwrap(), Some(vec![0xee]));
        }
    }

    #[test]
    fn signature_lookup_preserves_a_concurrent_valid_replacement() {
        let inner = in_memory();
        let live = InterleavingStore::new(inner.clone(), None);
        let quarantine = in_memory();
        let candidate = deploy(1);
        let key = key_for(&candidate.sig);
        inner
            .put_one(key.clone(), MALFORMED_VALUE.to_vec())
            .unwrap();
        let storage = KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone()).unwrap();
        let writer = inner.clone();
        let replacement_key = key.clone();
        let replacement = versioned_bytes_unchecked(&candidate);
        let replacement_bytes = replacement.clone();
        *live.before_delete.lock().unwrap() = Some(Box::new(move || {
            writer.put_one(replacement_key, replacement_bytes).unwrap();
        }));

        assert!(!storage.contains_sig(&candidate.sig).unwrap());
        assert!(storage.contains_sig(&candidate.sig).unwrap());
        assert_eq!(inner.get_one(&key).unwrap(), Some(replacement));
        assert_eq!(
            quarantine.get_one(&key).unwrap(),
            Some(MALFORMED_VALUE.to_vec())
        );
    }

    fn try_open_pool(
        name: &str,
        live: &Arc<dyn KeyValueStore>,
        quarantine: &Arc<dyn KeyValueStore>,
    ) -> Result<Pool, KvStoreError> {
        match name {
            DEPLOY_STORAGE_DB => {
                KeyValueDeployStorage::from_stores(live.clone(), quarantine.clone())
                    .map(Pool::Storage)
            }
            _ => KeyValueRejectedDeployBuffer::from_stores(live.clone(), quarantine.clone())
                .map(Pool::Buffer),
        }
    }

    #[test]
    fn first_read_quarantines_historical_records_with_invalid_signatures() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let valid = deploy(1);
            let mut tampered = deploy(2);
            tampered.data.phlo_limit += 1;
            let key = key_for(&tampered.sig);
            let bytes = versioned_bytes_unchecked(&tampered);
            live.put(vec![
                (
                    key_for(&valid.sig),
                    crate::rust::deploy::deploy_codec::encode(&valid).unwrap(),
                ),
                (key.clone(), bytes.clone()),
            ])
            .unwrap();

            let pool = Pool::open(name, &live, &quarantine);
            assert_eq!(live.get_one(&key).unwrap(), Some(bytes.clone()), "{name}");
            assert!(!quarantine.non_empty().unwrap());
            assert_eq!(pool.read_all().unwrap(), HashSet::from([valid.clone()]));
            assert_eq!(live.get_one(&key).unwrap(), None, "{name}");
            assert_eq!(
                quarantine.get_one(&key).unwrap(),
                Some(bytes.clone()),
                "{name}"
            );
            drop(pool);

            let reopened = Pool::open(name, &live, &quarantine);
            assert_eq!(reopened.read_all().unwrap(), HashSet::from([valid]));
            assert_eq!(live.get_one(&key).unwrap(), None, "{name}");
            assert_eq!(quarantine.get_one(&key).unwrap(), Some(bytes), "{name}");
        }
    }

    #[test]
    fn verified_restore_keeps_invalid_signatures_and_restores_valid_records() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let valid = deploy(1);
            let mut tampered = deploy(2);
            tampered.data.phlo_limit += 1;
            let key = key_for(&tampered.sig);
            let bytes = versioned_bytes_unchecked(&tampered);
            quarantine
                .put(vec![
                    (
                        key_for(&valid.sig),
                        crate::rust::deploy::deploy_codec::encode(&valid).unwrap(),
                    ),
                    (key.clone(), bytes.clone()),
                ])
                .unwrap();

            let pool = Pool::open(name, &live, &quarantine);
            assert_eq!(pool.read_all().unwrap(), HashSet::from([valid.clone()]));
            assert_eq!(live.get_one(&key).unwrap(), None, "{name}");
            assert_eq!(quarantine.get_one(&key).unwrap(), Some(bytes), "{name}");
            assert_eq!(quarantine.get_one(&key_for(&valid.sig)).unwrap(), None);
        }
    }

    #[test]
    fn verified_open_preserves_legacy_and_parameterized_deploys() {
        use models::rust::deploy_parameters::{DeployParameter, RholangValue};

        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let legacy = legacy_deploy();
            let mut data = deploy(3).data;
            data.parameters.push(DeployParameter {
                name: "input".to_string(),
                value: RholangValue::Int(42),
            });
            let parameterized = signed(data);
            live.put(vec![
                (key_for(&legacy.sig), legacy_bytes()),
                (
                    key_for(&parameterized.sig),
                    crate::rust::deploy::deploy_codec::encode(&parameterized).unwrap(),
                ),
            ])
            .unwrap();

            let pool = Pool::open(name, &live, &quarantine);
            assert_eq!(
                pool.read_all().unwrap(),
                HashSet::from([legacy, parameterized])
            );
            assert!(!quarantine.non_empty().unwrap());
        }
    }

    #[test]
    fn verified_open_skips_invalid_signatures_when_quarantine_has_conflicting_bytes() {
        for name in POOLS {
            let (live, quarantine) = (in_memory(), in_memory());
            let mut tampered = deploy(2);
            tampered.data.phlo_limit += 1;
            let key = key_for(&tampered.sig);
            let bytes = versioned_bytes_unchecked(&tampered);
            live.put_one(key.clone(), bytes.clone()).unwrap();
            quarantine.put_one(key.clone(), vec![0xee]).unwrap();

            let pool = try_open_pool(name, &live, &quarantine).unwrap();
            assert!(pool.read_all().unwrap().is_empty());
            assert!(pool.read_all().unwrap().is_empty());
            assert_eq!(live.get_one(&key).unwrap(), Some(bytes), "{name}");
            assert_eq!(
                quarantine.get_one(&key).unwrap(),
                Some(vec![0xee]),
                "{name}"
            );
        }
    }

    #[test]
    fn opening_defers_live_scan_failures_until_the_first_read() {
        for name in POOLS {
            let live: Arc<dyn KeyValueStore> = FailingStore::new(true, false);
            let quarantine = in_memory();
            let pool = try_open_pool(name, &live, &quarantine).unwrap();
            assert!(matches!(pool.read_all(), Err(KvStoreError::IoError(_))));
            assert!(!quarantine.non_empty().unwrap());
        }
    }

    #[test]
    fn first_read_keeps_invalid_bytes_when_the_quarantine_write_fails() {
        for name in POOLS {
            let live = in_memory();
            let quarantine: Arc<dyn KeyValueStore> = FailingStore::new(false, true);
            let mut tampered = deploy(2);
            tampered.data.phlo_limit += 1;
            let key = key_for(&tampered.sig);
            let bytes = versioned_bytes_unchecked(&tampered);
            live.put_one(key.clone(), bytes.clone()).unwrap();

            let pool = try_open_pool(name, &live, &quarantine).unwrap();
            assert!(matches!(pool.read_all(), Err(KvStoreError::IoError(_))));
            assert_eq!(live.get_one(&key).unwrap(), Some(bytes), "{name}");
            assert!(!quarantine.non_empty().unwrap());
        }
    }
}
