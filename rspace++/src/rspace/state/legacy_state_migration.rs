use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use heed::types::SerdeBincode;
use heed::{Database, Env, RoTxn, RwTxn, WithTls};
use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{ColdLeafAddress, validate_cold_leaf};
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::history::radix_tree::{Item, decode, empty_node};
use crate::rspace::shared::env_cache;
use crate::rspace::state::closure_verifier::{RecordValidator, verify_state_closure_with_getters};
use crate::rspace::state::import_session::StateImportError;
use crate::rspace::state::rspace_state_authority::LmdbAuthorityLayout;

type BytesDb = Database<SerdeBincode<ByteBuffer>, SerdeBincode<ByteBuffer>>;

const LEGACY_DB: &str = "db";
const LEGACY_CURRENT_ROOT_KEY: &[u8] = b"current-root";
const LEGACY_ROOT_MEMBERSHIP_VALUE: &[u8] = b"tag";
const VERIFIED_ROOT_KEY_PREFIX: &[u8] = b"rspace-root-catalog/v1/verified/";
const VERIFIED_ROOT_VALUE_PREFIX: &[u8] = b"rspace-verified-root\0\x01";
const CURRENT_ROOT_KEY: &[u8] = b"rspace-root-catalog/v1/current";
const CURRENT_ROOT_VALUE_PREFIX: &[u8] = b"rspace-current-root\0\x01";
const MIGRATION_KEY: &[u8] = b"rspace-state-authority/migration/legacy-state/v2/completed";
const MIGRATION_VALUE_PREFIX: &[u8] = b"rspace-legacy-state-migration\0\x02";
const PENDING_MIGRATION_KEY: &[u8] = b"rspace-state-authority/migration/legacy-state/v2/pending";
const PENDING_MIGRATION_VALUE_PREFIX: &[u8] = b"rspace-legacy-state-pending\0\x02";
const ROOT_HASH_LENGTH: usize = 32;
const HISTORY_KEY_LENGTH: usize = 33;
const WRITE_BATCH_ITEMS: usize = 256;

#[derive(Clone)]
struct ReadDb {
    env: Arc<Env>,
    db: BytesDb,
}

impl ReadDb {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, heed::Error> {
        let txn = self.env.read_txn()?;
        self.db.get(&txn, &key.to_vec())
    }
}

struct LegacyCatalog {
    roots: BTreeSet<Blake2b256Hash>,
    current: Option<Blake2b256Hash>,
}

struct LegacyLayout {
    domain: &'static [u8],
    history: ReadDb,
    roots_source: ReadDb,
    roots: BTreeSet<Blake2b256Hash>,
    current: Option<Blake2b256Hash>,
    cold: Option<ReadDb>,
}

struct MigrationMarker {
    namespace: Vec<u8>,
    fingerprint: [u8; 32],
    roots: BTreeSet<Blake2b256Hash>,
    current: Blake2b256Hash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingMigration {
    namespace: Vec<u8>,
    fingerprint: [u8; 32],
    roots: BTreeSet<Blake2b256Hash>,
    current: Blake2b256Hash,
}

pub(crate) fn migrate_legacy_state(
    destination_env: Arc<Env>,
    destination_history: BytesDb,
    destination_cold: BytesDb,
    destination_roots: BytesDb,
    layout: &LmdbAuthorityLayout,
    split_cold_dir: Option<&Path>,
    legacy_v2_dir: Option<&Path>,
    map_size: usize,
    max_dbs: u32,
) -> Result<(), heed::Error> {
    let destination_history_source = ReadDb {
        env: Arc::clone(&destination_env),
        db: destination_history,
    };
    let destination_roots_source = ReadDb {
        env: Arc::clone(&destination_env),
        db: destination_roots,
    };
    let destination_cold_source = ReadDb {
        env: Arc::clone(&destination_env),
        db: destination_cold,
    };
    let namespace = layout.namespace();
    let existing_marker = read_marker(&destination_roots_source, namespace)?;
    let existing_pending = read_pending(&destination_roots_source, namespace)?;
    if existing_marker.is_some() && existing_pending.is_some() {
        return Err(migration_error("completed and pending legacy migration records coexist"));
    }
    let mut layouts = Vec::new();

    let split_catalog = read_legacy_catalog(&destination_roots_source)?;
    if !split_catalog.roots.is_empty() {
        let cold_names = layout.cold_source_names();
        let candidate_count = if layout.legacy_compatible() { 2 } else { 1 };
        layouts.push(LegacyLayout {
            domain: b"split-v1",
            history: destination_history_source.clone(),
            roots_source: destination_roots_source.clone(),
            roots: split_catalog.roots,
            current: split_catalog.current,
            cold: open_existing_db(
                split_cold_dir,
                &cold_names[..candidate_count],
                map_size,
                max_dbs,
            )?,
        });
    }

    if let Some(v2_dir) = legacy_v2_dir {
        let v2_roots =
            open_existing_db(Some(&v2_dir.join("roots")), &[LEGACY_DB], map_size, max_dbs)?;
        if let Some(v2_roots) = v2_roots {
            let catalog = read_legacy_catalog(&v2_roots)?;
            if !catalog.roots.is_empty() {
                let history = open_existing_db(
                    Some(&v2_dir.join("history")),
                    &[LEGACY_DB],
                    map_size,
                    max_dbs,
                )?
                .ok_or_else(|| migration_error("legacy v2 roots exist without history"))?;
                let cold =
                    open_existing_db(Some(&v2_dir.join("cold")), &[LEGACY_DB], map_size, max_dbs)?;
                layouts.push(LegacyLayout {
                    domain: b"casper-v2",
                    history,
                    roots_source: v2_roots,
                    roots: catalog.roots,
                    current: catalog.current,
                    cold,
                });
            }
        }
    }

    if layouts.is_empty() {
        // No marker is written for an absent source. A completed marker is an
        // authority-owned receipt, so legacy files may be removed after
        // publication. A pending manifest is likewise sufficient to resume
        // typed validation from the already staged immutable closure.
        return Ok(());
    }

    if let Some(first) = layouts.first() {
        for other in &layouts[1..] {
            if other.roots != first.roots || other.current != first.current {
                return Err(migration_error("legacy split and v2 catalogs are not exact mirrors"));
            }
        }
    }

    let desired_current = resolve_current(&layouts, existing_marker.as_ref())?;
    let mut canonical_roots: Option<BTreeSet<Blake2b256Hash>> = None;
    let mut canonical_closure: Option<[u8; 32]> = None;
    for layout in &layouts {
        let closure_digest =
            migrate_layout(layout, &destination_history_source, &destination_cold_source)?;
        let catalog_after = read_legacy_catalog(&layout.roots_source)?;
        if catalog_after.roots != layout.roots || catalog_after.current != layout.current {
            return Err(migration_error(format!(
                "legacy layout {} changed while its closure was staged",
                String::from_utf8_lossy(layout.domain)
            )));
        }
        if let Some(roots) = &canonical_roots {
            if roots != &layout.roots || canonical_closure != Some(closure_digest) {
                return Err(migration_error("legacy split and v2 layouts are not exact mirrors"));
            }
        } else {
            canonical_roots = Some(layout.roots.clone());
            canonical_closure = Some(closure_digest);
        }
    }
    let roots = canonical_roots.unwrap_or_default();
    let closure = canonical_closure
        .ok_or_else(|| migration_error("legacy migration has no normalized source closure"))?;
    let mut source_fingerprint = Blake2b::<U32>::new();
    source_fingerprint.update(MIGRATION_VALUE_PREFIX);
    fingerprint_entry(&mut source_fingerprint, b"namespace", b"", namespace)?;
    fingerprint_entry(&mut source_fingerprint, b"current", b"", &desired_current.0)?;
    for root in &roots {
        fingerprint_entry(&mut source_fingerprint, b"root", b"", &root.0)?;
    }
    fingerprint_entry(&mut source_fingerprint, b"closure", b"", &closure)?;
    let fingerprint: [u8; 32] = source_fingerprint.finalize().into();
    if let Some(marker) = &existing_marker {
        if marker.fingerprint != fingerprint ||
            marker.current != desired_current ||
            marker.roots != roots
        {
            return Err(migration_error(
                "legacy state migration marker conflicts with the retained source closure",
            ));
        }
    }
    let pending = PendingMigration {
        namespace: namespace.to_vec(),
        fingerprint,
        roots,
        current: desired_current,
    };
    if let Some(existing) = &existing_pending {
        if existing != &pending {
            return Err(migration_error(
                "pending legacy migration conflicts with the retained source closure",
            ));
        }
    }
    if existing_marker.is_none() {
        write_pending(&destination_env, destination_roots, &pending)?;
    }
    Ok(())
}

/// Complete a structurally staged legacy migration with the concrete RSpace
/// record decoder. Returns `true` when a pending migration was published.
pub(crate) fn complete_legacy_state(
    destination_env: &Arc<Env>,
    destination_history: BytesDb,
    destination_cold: BytesDb,
    destination_roots: BytesDb,
    namespace: &[u8],
    validator: &RecordValidator,
) -> Result<bool, heed::Error> {
    let roots_source = ReadDb {
        env: Arc::clone(destination_env),
        db: destination_roots,
    };
    let Some(pending) = read_pending(&roots_source, namespace)? else {
        return Ok(false);
    };
    if read_marker(&roots_source, namespace)?.is_some() {
        return Err(migration_error("completed and pending legacy migration records coexist"));
    }

    let mut txn = destination_env.write_txn()?;
    let expected_pending = encode_pending(&pending)?;
    let observed_pending = destination_roots.get(&txn, &PENDING_MIGRATION_KEY.to_vec())?;
    if observed_pending != Some(expected_pending) {
        if observed_pending.is_none() {
            if let Some(bytes) = destination_roots.get(&txn, &MIGRATION_KEY.to_vec())? {
                let completed = decode_marker(&bytes, namespace)?;
                if completed.fingerprint == pending.fingerprint &&
                    completed.roots == pending.roots &&
                    completed.current == pending.current
                {
                    return Ok(false);
                }
            }
        }
        return Err(migration_error("pending legacy migration changed before typed validation"));
    }
    verify_state_closure_with_getters(
        &pending.roots.iter().cloned().collect::<Vec<_>>(),
        validator,
        |hash| {
            destination_history
                .get(&txn, &hash.0)
                .map_err(state_import_heed)
        },
        |address| {
            destination_cold
                .get(&txn, &address.encode())
                .map_err(state_import_heed)
        },
    )
    .map_err(|error| {
        migration_error(format!("legacy state closure failed concrete record validation: {error}"))
    })?;

    publish_catalog_in_txn(destination_roots, &mut txn, &pending)?;
    txn.commit()?;
    Ok(true)
}

fn migrate_layout(
    layout: &LegacyLayout,
    destination_history: &ReadDb,
    destination_cold: &ReadDb,
) -> Result<[u8; 32], heed::Error> {
    let empty_root = RadixHistory::empty_root_node_hash();
    let history_is_destination = Arc::ptr_eq(&layout.history.env, &destination_history.env);
    let history_txn = (!history_is_destination)
        .then(|| layout.history.env.read_txn())
        .transpose()?;
    let cold_txn = layout
        .cold
        .as_ref()
        .filter(|cold| !Arc::ptr_eq(&cold.env, &destination_cold.env))
        .map(|cold| cold.env.read_txn())
        .transpose()?;
    let mut fingerprint = Blake2b::<U32>::new();
    let mut history_batch = Vec::with_capacity(WRITE_BATCH_ITEMS);
    let mut cold_batch = Vec::with_capacity(WRITE_BATCH_ITEMS);
    let mut visited = HashSet::new();

    for root in &layout.roots {
        let mut work = vec![(root.clone(), Vec::new())];
        while let Some((hash, logical_prefix)) = work.pop() {
            if !visited.insert((hash.clone(), logical_prefix.clone())) {
                continue;
            }
            if hash == empty_root {
                fingerprint_entry(&mut fingerprint, b"empty-history", &logical_prefix, &hash.0)?;
                for (index, item) in empty_node().iter().enumerate().rev() {
                    push_history_item(&mut work, &logical_prefix, index, item)?;
                }
                continue;
            }

            let encoded = get_required_source_equal(
                &layout.history,
                history_txn.as_ref(),
                destination_history,
                &hash.0,
                "history object",
            )?;
            let actual = Blake2b256Hash::new(&encoded);
            if actual != hash {
                return Err(migration_error(format!(
                    "legacy history object {} has content digest {}",
                    hash, actual
                )));
            }
            fingerprint_entry(&mut fingerprint, b"history", &logical_prefix, &encoded)?;
            if !history_is_destination {
                history_batch.push((hash.0.clone(), encoded.clone()));
                if history_batch.len() == WRITE_BATCH_ITEMS {
                    put_immutable_batch(destination_history, std::mem::take(&mut history_batch))?;
                    history_batch.reserve(WRITE_BATCH_ITEMS);
                }
            }

            let node = decode(encoded).map_err(|error| {
                migration_error(format!("legacy history object {hash} cannot be decoded: {error}"))
            })?;
            for (index, item) in node.iter().enumerate().rev() {
                let (suffix, target, is_leaf) = match item {
                    Item::EmptyItem => continue,
                    Item::Leaf { prefix, value } => (prefix, value, true),
                    Item::NodePtr { prefix, ptr } => (prefix, ptr, false),
                };
                let child_prefix = extend_prefix(&logical_prefix, index, suffix)?;
                let target_hash = Blake2b256Hash::from_bytes(target.clone());
                if target_hash.0.len() != ROOT_HASH_LENGTH {
                    return Err(migration_error(format!(
                        "legacy history edge has {} digest bytes",
                        target_hash.0.len()
                    )));
                }
                if !is_leaf {
                    work.push((target_hash, child_prefix));
                    continue;
                }
                if child_prefix.len() != HISTORY_KEY_LENGTH {
                    return Err(migration_error(format!(
                        "legacy cold leaf terminates at path length {}, expected \
                         {HISTORY_KEY_LENGTH}",
                        child_prefix.len()
                    )));
                }
                let address = ColdLeafAddress::from_history_prefix(&child_prefix, target_hash)
                    .map_err(|error| migration_error(error.to_string()))?;
                let cold_bytes = get_cold_leaf(
                    layout.cold.as_ref().map(|cold| (cold, cold_txn.as_ref())),
                    destination_cold,
                    &address,
                )?;
                let records = validate_cold_leaf(&address, &cold_bytes)
                    .map_err(|error| migration_error(error.to_string()))?;
                for _ in records.records() {}
                fingerprint_entry(&mut fingerprint, b"cold", &child_prefix, &cold_bytes)?;
                cold_batch.push((address.encode(), cold_bytes));
                if cold_batch.len() == WRITE_BATCH_ITEMS {
                    put_immutable_batch(destination_cold, std::mem::take(&mut cold_batch))?;
                    cold_batch.reserve(WRITE_BATCH_ITEMS);
                }
            }
        }
    }
    put_immutable_batch(destination_history, history_batch)?;
    put_immutable_batch(destination_cold, cold_batch)?;
    Ok(fingerprint.finalize().into())
}

fn push_history_item(
    work: &mut Vec<(Blake2b256Hash, Vec<u8>)>,
    logical_prefix: &[u8],
    index: usize,
    item: &Item,
) -> Result<(), heed::Error> {
    match item {
        Item::EmptyItem => Ok(()),
        Item::Leaf { .. } => {
            Err(migration_error("the canonical empty history node unexpectedly contains a leaf"))
        }
        Item::NodePtr { prefix, ptr } => {
            let child_prefix = extend_prefix(logical_prefix, index, prefix)?;
            work.push((Blake2b256Hash::from_bytes(ptr.clone()), child_prefix));
            Ok(())
        }
    }
}

fn extend_prefix(prefix: &[u8], index: usize, suffix: &[u8]) -> Result<Vec<u8>, heed::Error> {
    let length = prefix
        .len()
        .checked_add(1)
        .and_then(|length| length.checked_add(suffix.len()))
        .ok_or_else(|| migration_error("legacy history path length overflow"))?;
    if length > HISTORY_KEY_LENGTH {
        return Err(migration_error(format!(
            "legacy history path length {length} exceeds {HISTORY_KEY_LENGTH}"
        )));
    }
    let index = u8::try_from(index)
        .map_err(|_| migration_error("legacy history child index exceeds one byte"))?;
    let mut result = Vec::with_capacity(length);
    result.extend_from_slice(prefix);
    result.push(index);
    result.extend_from_slice(suffix);
    Ok(result)
}

fn get_cold_leaf(
    source: Option<(&ReadDb, Option<&RoTxn<'_, WithTls>>)>,
    destination: &ReadDb,
    address: &ColdLeafAddress,
) -> Result<Vec<u8>, heed::Error> {
    let (source, source_txn) = source.ok_or_else(|| {
        migration_error(format!(
            "legacy retained root references cold leaf {} but the source cold database is absent",
            hex::encode(address.encode())
        ))
    })?;
    let mut legacy_bincode_key = Vec::with_capacity(8 + ROOT_HASH_LENGTH);
    legacy_bincode_key.extend_from_slice(&(ROOT_HASH_LENGTH as u64).to_le_bytes());
    legacy_bincode_key.extend_from_slice(&address.digest().0);
    let typed = address.encode();
    let raw = address.digest().0.clone();
    let mut observed = None;
    for key in [&typed, &raw, &legacy_bincode_key] {
        let value = match source_txn {
            Some(txn) => source.db.get(txn, &key.to_vec())?,
            None => source.get(key)?,
        };
        if let Some(value) = value {
            if observed.as_ref().is_some_and(|existing| existing != &value) {
                return Err(migration_error(format!(
                    "legacy cold leaf {} has conflicting source encodings",
                    hex::encode(&typed)
                )));
            }
            observed = Some(value);
        }
    }
    let source_value = observed.ok_or_else(|| {
        migration_error(format!(
            "legacy retained root references missing source cold leaf {}",
            hex::encode(&typed)
        ))
    })?;
    for key in [&typed, &raw, &legacy_bincode_key] {
        if let Some(value) = destination.get(key)? {
            if value != source_value {
                return Err(migration_error(format!(
                    "legacy cold leaf {} conflicts with staged destination bytes",
                    hex::encode(&typed)
                )));
            }
        }
    }
    Ok(source_value)
}

fn get_required_source_equal(
    source: &ReadDb,
    source_txn: Option<&RoTxn<'_, WithTls>>,
    destination: &ReadDb,
    key: &[u8],
    kind: &str,
) -> Result<Vec<u8>, heed::Error> {
    let value = match source_txn {
        Some(txn) => source.db.get(txn, &key.to_vec())?,
        None => source.get(key)?,
    }
    .ok_or_else(|| migration_error(format!("missing source {kind} {}", hex::encode(key))))?;
    if !Arc::ptr_eq(&source.env, &destination.env) {
        if let Some(existing) = destination.get(key)? {
            if existing != value {
                return Err(migration_error(format!(
                    "{kind} {} conflicts with staged destination bytes",
                    hex::encode(key)
                )));
            }
        }
    }
    Ok(value)
}

fn put_immutable_batch(db: &ReadDb, entries: Vec<(Vec<u8>, Vec<u8>)>) -> Result<(), heed::Error> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut txn = db.env.write_txn()?;
    for (key, value) in entries {
        match db.db.get(&txn, &key)? {
            Some(existing) if existing != value => {
                return Err(migration_error(format!(
                    "canonical state key {} already has conflicting bytes",
                    hex::encode(key)
                )));
            }
            Some(_) => {}
            None => db.db.put(&mut txn, &key, &value)?,
        }
    }
    txn.commit()
}

fn fingerprint_entry(
    digest: &mut Blake2b<U32>,
    domain: &[u8],
    key: &[u8],
    value: &[u8],
) -> Result<(), heed::Error> {
    for bytes in [domain, key, value] {
        let length = u64::try_from(bytes.len())
            .map_err(|_| migration_error("legacy state fingerprint input exceeds u64"))?;
        digest.update(length.to_be_bytes());
        digest.update(bytes);
    }
    Ok(())
}

fn open_existing_db(
    dir: Option<&Path>,
    names: &[&str],
    map_size: usize,
    max_dbs: u32,
) -> Result<Option<ReadDb>, heed::Error> {
    let Some(dir) = dir.filter(|dir| dir.join("data.mdb").is_file()) else {
        return Ok(None);
    };
    let resolved = crate::rspace::shared::lmdb_dir_store_manager::resolve_lmdb_environment(dir)?;
    let env = env_cache::get_or_open_env(&resolved, map_size, max_dbs)?;
    let txn = env.read_txn()?;
    let mut existing = Vec::new();
    let mut populated = Vec::new();
    for name in names {
        if let Some(db) = env
            .open_database::<SerdeBincode<ByteBuffer>, SerdeBincode<ByteBuffer>>(&txn, Some(name))?
        {
            existing.push((*name, db));
            if db.iter(&txn)?.next().transpose()?.is_some() {
                populated.push((*name, db));
            }
        }
    }
    if populated.len() > 1 {
        return Err(migration_error(format!(
            "legacy directory {} contains multiple populated candidate databases: {}",
            dir.display(),
            populated
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let selected = match (populated.pop(), existing.as_slice()) {
        (Some((_, db)), _) => Some(db),
        (None, [(_, db)]) => Some(*db),
        (None, _) => None,
    };
    drop(txn);
    Ok(selected.map(|db| ReadDb { env, db }))
}

fn read_legacy_catalog(source: &ReadDb) -> Result<LegacyCatalog, heed::Error> {
    let txn = source.env.read_txn()?;
    let mut roots = BTreeSet::new();
    let mut current = None;
    for entry in source.db.iter(&txn)? {
        let (key, value) = entry?;
        if key == LEGACY_CURRENT_ROOT_KEY {
            if value.len() != ROOT_HASH_LENGTH {
                return Err(migration_error(format!(
                    "legacy current root has {} bytes",
                    value.len()
                )));
            }
            current = Some(Blake2b256Hash::from_bytes(value));
            continue;
        }
        if key.len() != ROOT_HASH_LENGTH {
            continue;
        }
        if value != LEGACY_ROOT_MEMBERSHIP_VALUE {
            return Err(migration_error(format!(
                "legacy root membership {} has an invalid value",
                hex::encode(key)
            )));
        }
        roots.insert(Blake2b256Hash::from_bytes(key));
    }
    if let Some(current) = &current {
        if !roots.contains(current) {
            return Err(migration_error(format!(
                "legacy current root {current} has no membership"
            )));
        }
    }
    Ok(LegacyCatalog { roots, current })
}

fn resolve_current(
    layouts: &[LegacyLayout],
    marker: Option<&MigrationMarker>,
) -> Result<Blake2b256Hash, heed::Error> {
    let mut current = marker.map(|marker| marker.current.clone());
    for layout in layouts {
        if let Some(candidate) = &layout.current {
            if current.as_ref().is_some_and(|current| current != candidate) {
                return Err(migration_error(
                    "legacy state layouts disagree about the current root",
                ));
            }
            current = Some(candidate.clone());
        }
    }
    let current = current.ok_or_else(|| {
        migration_error("legacy root memberships exist without a current-root pointer")
    })?;
    for layout in layouts {
        if !layout.roots.contains(&current) {
            return Err(migration_error(format!(
                "legacy layout {} does not retain current root {current}",
                String::from_utf8_lossy(layout.domain)
            )));
        }
    }
    Ok(current)
}

fn read_marker(
    roots: &ReadDb,
    expected_namespace: &[u8],
) -> Result<Option<MigrationMarker>, heed::Error> {
    roots
        .get(MIGRATION_KEY)?
        .map(|bytes| decode_marker(&bytes, expected_namespace))
        .transpose()
}

fn read_pending(
    roots: &ReadDb,
    expected_namespace: &[u8],
) -> Result<Option<PendingMigration>, heed::Error> {
    roots
        .get(PENDING_MIGRATION_KEY)?
        .map(|bytes| decode_pending(&bytes, expected_namespace))
        .transpose()
}

fn write_pending(
    env: &Arc<Env>,
    roots: BytesDb,
    pending: &PendingMigration,
) -> Result<(), heed::Error> {
    let encoded = encode_pending(pending)?;
    let mut txn = env.write_txn()?;
    match roots.get(&txn, &PENDING_MIGRATION_KEY.to_vec())? {
        Some(existing) if existing != encoded => {
            return Err(migration_error(
                "pending legacy migration changed before staging completed",
            ));
        }
        Some(_) => {}
        None => roots.put(&mut txn, &PENDING_MIGRATION_KEY.to_vec(), &encoded)?,
    }
    txn.commit()
}

fn encode_marker(marker: &MigrationMarker) -> Result<Vec<u8>, heed::Error> {
    encode_manifest(
        MIGRATION_VALUE_PREFIX,
        &marker.namespace,
        marker.fingerprint,
        &marker.current,
        &marker.roots,
    )
}

fn decode_marker(bytes: &[u8], expected_namespace: &[u8]) -> Result<MigrationMarker, heed::Error> {
    let (namespace, fingerprint, current, roots) =
        decode_manifest(bytes, MIGRATION_VALUE_PREFIX, expected_namespace)?;
    Ok(MigrationMarker {
        namespace,
        fingerprint,
        roots,
        current,
    })
}

fn encode_pending(pending: &PendingMigration) -> Result<Vec<u8>, heed::Error> {
    encode_manifest(
        PENDING_MIGRATION_VALUE_PREFIX,
        &pending.namespace,
        pending.fingerprint,
        &pending.current,
        &pending.roots,
    )
}

fn decode_pending(
    bytes: &[u8],
    expected_namespace: &[u8],
) -> Result<PendingMigration, heed::Error> {
    let (namespace, fingerprint, current, roots) =
        decode_manifest(bytes, PENDING_MIGRATION_VALUE_PREFIX, expected_namespace)?;
    Ok(PendingMigration {
        namespace,
        fingerprint,
        roots,
        current,
    })
}

fn encode_manifest(
    prefix: &[u8],
    namespace: &[u8],
    fingerprint: [u8; 32],
    current: &Blake2b256Hash,
    roots: &BTreeSet<Blake2b256Hash>,
) -> Result<Vec<u8>, heed::Error> {
    let namespace_len = u32::try_from(namespace.len())
        .map_err(|_| migration_error("legacy migration namespace exceeds u32"))?;
    let count = u64::try_from(roots.len())
        .map_err(|_| migration_error("legacy migration root count exceeds u64"))?;
    let roots_bytes = roots
        .len()
        .checked_mul(ROOT_HASH_LENGTH)
        .ok_or_else(|| migration_error("legacy migration root bytes overflow"))?;
    let capacity = prefix
        .len()
        .checked_add(4)
        .and_then(|size| size.checked_add(namespace.len()))
        .and_then(|size| size.checked_add(32 + 32 + 8))
        .and_then(|size| size.checked_add(roots_bytes))
        .ok_or_else(|| migration_error("legacy migration manifest size overflow"))?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(&namespace_len.to_be_bytes());
    bytes.extend_from_slice(namespace);
    bytes.extend_from_slice(&fingerprint);
    bytes.extend_from_slice(&current.0);
    bytes.extend_from_slice(&count.to_be_bytes());
    for root in roots {
        bytes.extend_from_slice(&root.0);
    }
    Ok(bytes)
}

type DecodedManifest = (Vec<u8>, [u8; 32], Blake2b256Hash, BTreeSet<Blake2b256Hash>);

fn decode_manifest(
    bytes: &[u8],
    prefix: &[u8],
    expected_namespace: &[u8],
) -> Result<DecodedManifest, heed::Error> {
    let namespace_len_offset = prefix.len();
    let namespace_offset = namespace_len_offset + 4;
    if bytes.len() < namespace_offset || !bytes.starts_with(prefix) {
        return Err(migration_error("legacy migration manifest has an invalid version or length"));
    }
    let namespace_len = u32::from_be_bytes(
        bytes[namespace_len_offset..namespace_offset]
            .try_into()
            .expect("validated namespace-length range"),
    );
    let namespace_len = usize::try_from(namespace_len)
        .map_err(|_| migration_error("legacy migration namespace exceeds usize"))?;
    let fingerprint_offset = namespace_offset
        .checked_add(namespace_len)
        .ok_or_else(|| migration_error("legacy migration namespace length overflow"))?;
    let current_offset = fingerprint_offset
        .checked_add(32)
        .ok_or_else(|| migration_error("legacy migration fingerprint offset overflow"))?;
    let count_offset = current_offset
        .checked_add(32)
        .ok_or_else(|| migration_error("legacy migration current offset overflow"))?;
    let roots_offset = count_offset
        .checked_add(8)
        .ok_or_else(|| migration_error("legacy migration root-count offset overflow"))?;
    if bytes.len() < roots_offset {
        return Err(migration_error("legacy migration manifest is truncated"));
    }
    let namespace = bytes[namespace_offset..fingerprint_offset].to_vec();
    if namespace != expected_namespace {
        return Err(migration_error(format!(
            "legacy migration namespace {} does not match authority namespace {}",
            String::from_utf8_lossy(&namespace),
            String::from_utf8_lossy(expected_namespace)
        )));
    }
    let count = u64::from_be_bytes(
        bytes[count_offset..roots_offset]
            .try_into()
            .expect("validated root-count range"),
    );
    let count = usize::try_from(count)
        .map_err(|_| migration_error("legacy migration root count exceeds usize"))?;
    let expected = count
        .checked_mul(ROOT_HASH_LENGTH)
        .and_then(|size| roots_offset.checked_add(size))
        .ok_or_else(|| migration_error("legacy migration manifest size overflow"))?;
    if bytes.len() != expected {
        return Err(migration_error("legacy migration root count does not match its bytes"));
    }
    let mut roots = BTreeSet::new();
    let mut previous: Option<&[u8]> = None;
    for raw in bytes[roots_offset..].chunks_exact(ROOT_HASH_LENGTH) {
        if previous.is_some_and(|old| old >= raw) {
            return Err(migration_error(
                "legacy migration roots are not strictly ordered and unique",
            ));
        }
        previous = Some(raw);
        roots.insert(Blake2b256Hash::from_bytes(raw.to_vec()));
    }
    let current = Blake2b256Hash::from_bytes(bytes[current_offset..count_offset].to_vec());
    if !roots.contains(&current) {
        return Err(migration_error(
            "legacy migration current root is absent from its retained roots",
        ));
    }
    Ok((
        namespace,
        bytes[fingerprint_offset..current_offset]
            .try_into()
            .expect("validated fingerprint range"),
        current,
        roots,
    ))
}

fn publish_catalog_in_txn(
    roots_db: BytesDb,
    txn: &mut RwTxn<'_>,
    pending: &PendingMigration,
) -> Result<(), heed::Error> {
    let roots = &pending.roots;
    let desired_current = &pending.current;
    if !roots.contains(desired_current) {
        return Err(migration_error("legacy current root is absent from the retained root set"));
    }
    if roots_db.get(txn, &MIGRATION_KEY.to_vec())?.is_some() {
        return Err(migration_error(
            "legacy state migration was already completed before pending publication",
        ));
    }
    let expected_pending = encode_pending(pending)?;
    if roots_db.get(txn, &PENDING_MIGRATION_KEY.to_vec())? != Some(expected_pending) {
        return Err(migration_error("pending legacy migration changed before publication"));
    }
    if let Some(legacy_current) = roots_db.get(txn, &LEGACY_CURRENT_ROOT_KEY.to_vec())? {
        if legacy_current != desired_current.0 {
            return Err(migration_error("legacy current root changed before publication"));
        }
    }
    for root in roots {
        let key = verified_root_key(root);
        let value = verified_root_value(root);
        if roots_db
            .get(txn, &key)?
            .is_some_and(|existing| existing != value)
        {
            return Err(migration_error(format!(
                "verified root membership {} conflicts with migration",
                root
            )));
        }
    }

    let existing_current = roots_db
        .get(txn, &CURRENT_ROOT_KEY.to_vec())?
        .map(|bytes| decode_current(&bytes))
        .transpose()?;
    let empty = RadixHistory::empty_root_node_hash();
    let generation = match &existing_current {
        Some((root, generation)) if root == desired_current => *generation,
        Some((root, generation)) if root == &empty => generation
            .checked_add(1)
            .ok_or_else(|| migration_error("root catalog generation overflow"))?,
        Some((root, _)) => {
            return Err(migration_error(format!(
                "canonical current root {root} conflicts with legacy current root \
                 {desired_current}"
            )));
        }
        None => 0,
    };
    for root in roots {
        roots_db.put(txn, &verified_root_key(root), &verified_root_value(root))?;
    }
    roots_db.put(txn, &CURRENT_ROOT_KEY.to_vec(), &encode_current(desired_current, generation))?;
    roots_db.put(
        txn,
        &MIGRATION_KEY.to_vec(),
        &encode_marker(&MigrationMarker {
            namespace: pending.namespace.clone(),
            fingerprint: pending.fingerprint,
            roots: pending.roots.clone(),
            current: pending.current.clone(),
        })?,
    )?;
    roots_db.delete(txn, &PENDING_MIGRATION_KEY.to_vec())?;
    roots_db.delete(txn, &LEGACY_CURRENT_ROOT_KEY.to_vec())?;
    for root in roots {
        match roots_db.get(txn, &root.0)? {
            Some(value) if value == LEGACY_ROOT_MEMBERSHIP_VALUE => {
                roots_db.delete(txn, &root.0)?;
            }
            Some(_) => {
                return Err(migration_error(format!(
                    "legacy root membership {} changed before publication",
                    root
                )));
            }
            None => {}
        }
    }
    Ok(())
}

fn verified_root_key(root: &Blake2b256Hash) -> Vec<u8> {
    [VERIFIED_ROOT_KEY_PREFIX, root.0.as_slice()].concat()
}

fn verified_root_value(root: &Blake2b256Hash) -> Vec<u8> {
    [VERIFIED_ROOT_VALUE_PREFIX, root.0.as_slice()].concat()
}

fn encode_current(root: &Blake2b256Hash, generation: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(CURRENT_ROOT_VALUE_PREFIX.len() + 8 + ROOT_HASH_LENGTH);
    bytes.extend_from_slice(CURRENT_ROOT_VALUE_PREFIX);
    bytes.extend_from_slice(&generation.to_be_bytes());
    bytes.extend_from_slice(&root.0);
    bytes
}

fn decode_current(bytes: &[u8]) -> Result<(Blake2b256Hash, u64), heed::Error> {
    let expected = CURRENT_ROOT_VALUE_PREFIX.len() + 8 + ROOT_HASH_LENGTH;
    if bytes.len() != expected || !bytes.starts_with(CURRENT_ROOT_VALUE_PREFIX) {
        return Err(migration_error("canonical current root has an invalid version or length"));
    }
    let generation_offset = CURRENT_ROOT_VALUE_PREFIX.len();
    let root_offset = generation_offset + 8;
    let generation = u64::from_be_bytes(
        bytes[generation_offset..root_offset]
            .try_into()
            .expect("validated generation range"),
    );
    Ok((Blake2b256Hash::from_bytes(bytes[root_offset..].to_vec()), generation))
}

fn migration_error(message: impl Into<String>) -> heed::Error {
    heed::Error::Io(std::io::Error::other(message.into()))
}

fn state_import_heed(error: heed::Error) -> StateImportError {
    StateImportError::Store(KvStoreError::from(error))
}

#[cfg(test)]
mod tests {
    use shared::rust::test_scratch;

    use super::*;
    use crate::rspace::history::cold_store::{DataLeaf, PersistedData, PersistedDataKind};
    use crate::rspace::history::radix_tree::hash_node;
    use crate::rspace::shared::lmdb_dir_store_manager::resolve_lmdb_environment;

    fn root(seed: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![seed; 32]) }

    fn databases(path: &Path, prefix: &str) -> (Arc<Env>, BytesDb, BytesDb, BytesDb) {
        std::fs::create_dir_all(path).expect("create LMDB fixture directory");
        let env = env_cache::get_or_open_env(path, 16 * 1024 * 1024, 8).expect("fixture env");
        let mut txn = env.write_txn().expect("fixture write transaction");
        let history = env
            .create_database(&mut txn, Some(&format!("{prefix}-history")))
            .expect("history database");
        let cold = env
            .create_database(&mut txn, Some(&format!("{prefix}-cold")))
            .expect("cold database");
        let roots = env
            .create_database(&mut txn, Some(&format!("{prefix}-roots")))
            .expect("roots database");
        txn.commit().expect("fixture databases");
        (env, history, cold, roots)
    }

    fn legacy_database(path: &Path) -> (Arc<Env>, BytesDb) {
        std::fs::create_dir_all(path).expect("create legacy LMDB directory");
        let resolved = resolve_lmdb_environment(path).expect("resolve legacy LMDB directory");
        let env = env_cache::get_or_open_env(&resolved, 16 * 1024 * 1024, 8).expect("legacy env");
        let mut txn = env.write_txn().expect("legacy database transaction");
        let db = env
            .create_database(&mut txn, Some(LEGACY_DB))
            .expect("legacy database");
        txn.commit().expect("legacy database commit");
        (env, db)
    }

    fn seed_legacy_catalog(env: &Arc<Env>, roots_db: BytesDb, root: &Blake2b256Hash) {
        let mut txn = env.write_txn().expect("legacy catalog transaction");
        roots_db
            .put(&mut txn, &root.0, &LEGACY_ROOT_MEMBERSHIP_VALUE.to_vec())
            .expect("legacy membership");
        roots_db
            .put(&mut txn, &LEGACY_CURRENT_ROOT_KEY.to_vec(), &root.0)
            .expect("legacy current");
        txn.commit().expect("legacy catalog");
    }

    #[test]
    fn marker_round_trips_source_fingerprint_and_current_root() {
        let fingerprint = [9; 32];
        let current = root(7);
        let marker = MigrationMarker {
            namespace: b"rspace".to_vec(),
            fingerprint,
            roots: BTreeSet::from([current.clone()]),
            current: current.clone(),
        };
        let encoded = encode_marker(&marker).unwrap();
        let decoded = decode_marker(&encoded, b"rspace").unwrap();
        assert_eq!(decoded.namespace, b"rspace");
        assert_eq!(decoded.fingerprint, fingerprint);
        assert_eq!(decoded.roots, marker.roots);
        assert_eq!(decoded.current, current);
    }

    #[test]
    fn pending_manifest_is_namespace_bound_and_strictly_ordered() {
        let current = root(2);
        let pending = PendingMigration {
            namespace: b"eval".to_vec(),
            fingerprint: [5; 32],
            roots: BTreeSet::from([root(1), current.clone()]),
            current,
        };
        let encoded = encode_pending(&pending).unwrap();
        assert_eq!(decode_pending(&encoded, b"eval").unwrap(), pending);
        assert!(decode_pending(&encoded, b"rspace").is_err());
    }

    #[test]
    fn split_empty_root_is_staged_then_published_atomically() {
        let scratch = test_scratch::acquire("rspace-legacy-empty-");
        let (env, history, cold, roots_db) = databases(&scratch.path().join("authority"), "rspace");
        let empty = RadixHistory::empty_root_node_hash();
        seed_legacy_catalog(&env, roots_db, &empty);

        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("rspace").unwrap(),
            None,
            None,
            16 * 1024 * 1024,
            8,
        )
        .expect("stage split legacy state");
        let staged_manifest = {
            let txn = env.read_txn().expect("first staged read");
            roots_db
                .get(&txn, &PENDING_MIGRATION_KEY.to_vec())
                .unwrap()
                .expect("pending manifest after structural staging")
        };

        // Simulate process interruption between structural staging and typed
        // publication. Reopening the migration protocol must recover the
        // exact manifest without appointing a root or rewriting provenance.
        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("rspace").unwrap(),
            None,
            None,
            16 * 1024 * 1024,
            8,
        )
        .expect("resume structurally staged split state");
        {
            let txn = env.read_txn().expect("staged read");
            assert_eq!(
                roots_db.get(&txn, &PENDING_MIGRATION_KEY.to_vec()).unwrap(),
                Some(staged_manifest)
            );
            assert!(
                roots_db
                    .get(&txn, &CURRENT_ROOT_KEY.to_vec())
                    .unwrap()
                    .is_none()
            );
        }

        let validator: RecordValidator = Arc::new(|_, _| Ok(()));
        assert!(
            complete_legacy_state(&env, history, cold, roots_db, b"rspace", &validator,)
                .expect("complete split legacy state")
        );
        let txn = env.read_txn().expect("completed read");
        assert_eq!(
            roots_db
                .get(&txn, &CURRENT_ROOT_KEY.to_vec())
                .unwrap()
                .map(|bytes| decode_current(&bytes).unwrap()),
            Some((empty.clone(), 0))
        );
        assert!(
            roots_db
                .get(&txn, &verified_root_key(&empty))
                .unwrap()
                .is_some()
        );
        assert!(
            roots_db
                .get(&txn, &MIGRATION_KEY.to_vec())
                .unwrap()
                .is_some()
        );
        assert!(
            roots_db
                .get(&txn, &PENDING_MIGRATION_KEY.to_vec())
                .unwrap()
                .is_none()
        );
        assert!(
            roots_db
                .get(&txn, &LEGACY_CURRENT_ROOT_KEY.to_vec())
                .unwrap()
                .is_none()
        );
        assert!(roots_db.get(&txn, &empty.0).unwrap().is_none());
        drop(txn);

        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("rspace").unwrap(),
            None,
            None,
            16 * 1024 * 1024,
            8,
        )
        .expect("completed migration reopens after legacy source removal");
    }

    #[test]
    fn casper_v2_empty_root_uses_the_same_two_phase_protocol() {
        let scratch = test_scratch::acquire("rspace-legacy-v2-");
        let (env, history, cold, roots_db) = databases(&scratch.path().join("authority"), "rspace");
        let v2 = scratch.path().join("rspace/casper/v2");
        let (_history_env, _legacy_history) = legacy_database(&v2.join("history"));
        let (_cold_env, _legacy_cold) = legacy_database(&v2.join("cold"));
        let (roots_env, legacy_roots) = legacy_database(&v2.join("roots"));
        let empty = RadixHistory::empty_root_node_hash();
        seed_legacy_catalog(&roots_env, legacy_roots, &empty);

        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("rspace").unwrap(),
            None,
            Some(&v2),
            16 * 1024 * 1024,
            8,
        )
        .expect("stage v2 state");
        let validator: RecordValidator = Arc::new(|_, _| Ok(()));
        assert!(
            complete_legacy_state(&env, history, cold, roots_db, b"rspace", &validator,)
                .expect("complete v2 state")
        );
        let txn = env.read_txn().expect("v2 completed read");
        assert_eq!(
            roots_db
                .get(&txn, &CURRENT_ROOT_KEY.to_vec())
                .unwrap()
                .map(|bytes| decode_current(&bytes).unwrap()),
            Some((empty.clone(), 0))
        );
        assert!(
            roots_db
                .get(&txn, &MIGRATION_KEY.to_vec())
                .unwrap()
                .is_some()
        );
        drop(txn);

        // A retained source that changes after completion is evidence of a
        // different legacy authority, not permission to revise the receipt.
        let conflicting = root(23);
        seed_legacy_catalog(&roots_env, legacy_roots, &conflicting);
        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("rspace").unwrap(),
            None,
            Some(&v2),
            16 * 1024 * 1024,
            8,
        )
        .expect_err("completed receipt must reject a changed retained source");
        let txn = env.read_txn().expect("conflict leaves publication intact");
        assert_eq!(
            roots_db
                .get(&txn, &CURRENT_ROOT_KEY.to_vec())
                .unwrap()
                .map(|bytes| decode_current(&bytes).unwrap()),
            Some((empty, 0))
        );
    }

    #[test]
    fn eval_namespace_migrates_independently_and_rejects_marker_transplantation() {
        let scratch = test_scratch::acquire("rspace-legacy-eval-");
        let (env, history, cold, roots_db) = databases(&scratch.path().join("authority"), "eval");
        let empty = RadixHistory::empty_root_node_hash();
        seed_legacy_catalog(&env, roots_db, &empty);
        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("eval").unwrap(),
            None,
            None,
            16 * 1024 * 1024,
            8,
        )
        .expect("stage eval state");
        let validator: RecordValidator = Arc::new(|_, _| Ok(()));
        complete_legacy_state(&env, history, cold, roots_db, b"eval", &validator)
            .expect("complete eval state");
        let txn = env.read_txn().expect("eval completed read");
        let marker = roots_db
            .get(&txn, &MIGRATION_KEY.to_vec())
            .unwrap()
            .expect("completed marker");
        assert_eq!(decode_marker(&marker, b"eval").unwrap().namespace, b"eval");
        assert!(decode_marker(&marker, b"rspace").is_err());
    }

    #[test]
    fn divergent_split_and_v2_catalogs_fail_before_pending_publication() {
        let scratch = test_scratch::acquire("rspace-legacy-divergent-");
        let (env, history, cold, roots_db) = databases(&scratch.path().join("authority"), "rspace");
        let empty = RadixHistory::empty_root_node_hash();
        seed_legacy_catalog(&env, roots_db, &empty);

        let v2 = scratch.path().join("rspace/casper/v2");
        let (_history_env, _legacy_history) = legacy_database(&v2.join("history"));
        let (_cold_env, _legacy_cold) = legacy_database(&v2.join("cold"));
        let (roots_env, legacy_roots) = legacy_database(&v2.join("roots"));
        seed_legacy_catalog(&roots_env, legacy_roots, &root(9));

        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("rspace").unwrap(),
            None,
            Some(&v2),
            16 * 1024 * 1024,
            8,
        )
        .expect_err("divergent legacy authorities must not be merged");
        let txn = env.read_txn().expect("divergent read");
        assert!(
            roots_db
                .get(&txn, &PENDING_MIGRATION_KEY.to_vec())
                .unwrap()
                .is_none()
        );
        assert!(
            roots_db
                .get(&txn, &CURRENT_ROOT_KEY.to_vec())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn malformed_legacy_catalog_and_history_never_reach_pending_publication() {
        let scratch = test_scratch::acquire("rspace-legacy-corrupt-catalog-");

        for (label, seed) in [("bad-current", 1_u8), ("bad-membership", 2), ("orphan-current", 3)] {
            let path = scratch.path().join(label);
            let (env, db) = legacy_database(&path);
            let mut txn = env.write_txn().expect("corrupt catalog write");
            match label {
                "bad-current" => {
                    db.put(&mut txn, &root(seed).0, &LEGACY_ROOT_MEMBERSHIP_VALUE.to_vec())
                        .unwrap();
                    db.put(&mut txn, &LEGACY_CURRENT_ROOT_KEY.to_vec(), &vec![seed; 31])
                        .unwrap();
                }
                "bad-membership" => {
                    db.put(&mut txn, &root(seed).0, &b"not-tag".to_vec())
                        .unwrap();
                }
                "orphan-current" => {
                    db.put(&mut txn, &LEGACY_CURRENT_ROOT_KEY.to_vec(), &root(seed).0)
                        .unwrap();
                }
                _ => unreachable!(),
            }
            txn.commit().expect("corrupt catalog commit");
            assert!(read_legacy_catalog(&ReadDb { env, db }).is_err(), "{label}");
        }

        let (env, history, cold, roots_db) =
            databases(&scratch.path().join("malformed-history"), "rspace");
        let malformed_node = b"not-a-radix-node".to_vec();
        let retained_root = Blake2b256Hash::new(&malformed_node);
        let mut txn = env.write_txn().expect("malformed history write");
        history
            .put(&mut txn, &retained_root.0, &malformed_node)
            .expect("content-addressed malformed history object");
        txn.commit().expect("malformed history commit");
        seed_legacy_catalog(&env, roots_db, &retained_root);

        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("rspace").unwrap(),
            None,
            None,
            16 * 1024 * 1024,
            8,
        )
        .expect_err("malformed history object must fail structural staging");
        let txn = env.read_txn().expect("malformed history audit");
        assert!(
            roots_db
                .get(&txn, &PENDING_MIGRATION_KEY.to_vec())
                .unwrap()
                .is_none()
        );
        assert!(
            roots_db
                .get(&txn, &CURRENT_ROOT_KEY.to_vec())
                .unwrap()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_legacy_source_reached_through_a_symbolic_link_is_staged_and_completed() {
        let scratch = test_scratch::acquire("rspace-legacy-link-");
        let real = scratch.path().join("real");
        std::fs::create_dir_all(&real).expect("real source parent");
        let link = scratch.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symbolic link to the source parent");
        let (env, history, cold, roots_db) = databases(&scratch.path().join("authority"), "rspace");
        let v2 = link.join("rspace/casper/v2");
        let (_history_env, _legacy_history) = legacy_database(&v2.join("history"));
        let (_cold_env, _legacy_cold) = legacy_database(&v2.join("cold"));
        let (roots_env, legacy_roots) = legacy_database(&v2.join("roots"));
        let empty = RadixHistory::empty_root_node_hash();
        seed_legacy_catalog(&roots_env, legacy_roots, &empty);

        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("rspace").expect("authority layout"),
            None,
            Some(&v2),
            16 * 1024 * 1024,
            8,
        )
        .expect("stage v2 state through the link");
        let validator: RecordValidator = Arc::new(|_, _| Ok(()));
        assert!(
            complete_legacy_state(&env, history, cold, roots_db, b"rspace", &validator)
                .expect("complete v2 state through the link")
        );
        let txn = env.read_txn().expect("completed read");
        assert_eq!(
            roots_db
                .get(&txn, &CURRENT_ROOT_KEY.to_vec())
                .expect("current root read")
                .map(|bytes| decode_current(&bytes).expect("current root record")),
            Some((empty, 0))
        );
    }

    #[test]
    fn concrete_record_failure_cannot_publish_a_structurally_valid_source() {
        let scratch = test_scratch::acquire("rspace-legacy-malformed-");
        let (env, history, cold, roots_db) = databases(&scratch.path().join("authority"), "rspace");
        let cold_path = scratch.path().join("cold");
        std::fs::create_dir_all(&cold_path).expect("cold source directory");
        let cold_resolved =
            resolve_lmdb_environment(&cold_path).expect("resolve cold source directory");
        let cold_env = env_cache::get_or_open_env(&cold_resolved, 16 * 1024 * 1024, 8)
            .expect("cold source env");
        let mut cold_txn = cold_env.write_txn().expect("cold source write");
        let source_cold: BytesDb = cold_env
            .create_database(&mut cold_txn, Some("rspace-cold"))
            .expect("source cold database");

        let record: Vec<u8> = vec![0xff];
        let payload = bincode::serialize(&vec![record]).expect("record sequence");
        let cold_bytes = PersistedData::Data(DataLeaf { bytes: payload }).into_legacy_bincode();
        let view =
            crate::rspace::history::cold_store::PersistedDataView::parse_canonical(&cold_bytes)
                .expect("canonical cold envelope")
                .validate_records(PersistedDataKind::Data, Some(1))
                .expect("structural record framing");
        let digest = Blake2b256Hash::new(view.leaf_encoding());
        source_cold
            .put(&mut cold_txn, &digest.0, &cold_bytes)
            .expect("legacy raw cold leaf");
        cold_txn.commit().expect("cold source commit");

        let mut node = empty_node();
        node[0] = Item::Leaf {
            prefix: vec![7; 32],
            value: digest.0,
        };
        let (root_bytes, encoded) = hash_node(&node);
        let retained_root = Blake2b256Hash::from_bytes(root_bytes.clone());
        let mut state_txn = env.write_txn().expect("legacy history write");
        history
            .put(&mut state_txn, &root_bytes, &encoded)
            .expect("legacy history root");
        state_txn.commit().expect("legacy history commit");
        seed_legacy_catalog(&env, roots_db, &retained_root);

        migrate_legacy_state(
            Arc::clone(&env),
            history,
            cold,
            roots_db,
            &LmdbAuthorityLayout::for_prefix("rspace").unwrap(),
            Some(&cold_path),
            None,
            16 * 1024 * 1024,
            8,
        )
        .expect("structural staging succeeds");
        let reject_records: RecordValidator = Arc::new(|_, _| Err("malformed datum".to_string()));
        complete_legacy_state(&env, history, cold, roots_db, b"rspace", &reject_records)
            .expect_err("typed validation must reject publication");

        let txn = env.read_txn().expect("failed-completion read");
        assert!(
            roots_db
                .get(&txn, &PENDING_MIGRATION_KEY.to_vec())
                .unwrap()
                .is_some()
        );
        assert!(
            roots_db
                .get(&txn, &CURRENT_ROOT_KEY.to_vec())
                .unwrap()
                .is_none()
        );
        assert!(
            roots_db
                .get(&txn, &verified_root_key(&retained_root))
                .unwrap()
                .is_none()
        );
        assert!(
            roots_db
                .get(&txn, &MIGRATION_KEY.to_vec())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn current_encoding_round_trips_generation_and_root() {
        let current = root(3);
        assert_eq!(decode_current(&encode_current(&current, 41)).unwrap(), (current, 41));
    }

    #[test]
    fn path_extension_rejects_artificial_depth_beyond_the_history_schema() {
        assert!(extend_prefix(&vec![0; HISTORY_KEY_LENGTH], 0, &[]).is_err());
        assert_eq!(
            extend_prefix(&vec![0; HISTORY_KEY_LENGTH - 1], 0, &[])
                .unwrap()
                .len(),
            33
        );
    }
}
