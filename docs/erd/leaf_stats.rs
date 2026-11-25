use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use heed::types::Bytes;
use heed::{Database, EnvFlags, EnvOpenOptions, RoTxn};
use serde::{Deserialize, Serialize};

mod leaf_decode;
mod leaf_report;
use leaf_decode::{borrowed_bytes, decode, Distribution, Elements, PersistedData};

type Hash = [u8; 32];
type RawDb = Database<Bytes, Bytes>;
const CAPTURE_BYTES: u64 = 256 * 1024 * 1024;
const CAPTURE_ROWS: usize = 1_000_000;

fn digest(bytes: &[u8]) -> Hash { Blake2b::<U32>::digest(bytes).into() }
fn hash(bytes: &[u8]) -> Result<Hash, Box<dyn Error>> {
    bytes.try_into().map_err(|_| "invalid hash length".into())
}
fn checked_time(deadline: Instant) -> Result<(), Box<dyn Error>> {
    if Instant::now() > deadline {
        Err("measurement time budget exceeded".into())
    } else {
        Ok(())
    }
}

fn get<'a>(db: RawDb, txn: &'a RoTxn, key: &[u8]) -> Result<Option<&'a [u8]>, Box<dyn Error>> {
    db.get(txn, &bincode::serialize(key)?)?
        .map(borrowed_bytes)
        .transpose()
}

fn node_children(bytes: &[u8]) -> Result<Vec<(bool, Hash)>, Box<dyn Error>> {
    let mut cursor = bytes;
    let mut last = None;
    let mut result = Vec::new();
    while !cursor.is_empty() {
        if cursor.len() < 2 {
            return Err("truncated radix header".into());
        }
        let index = cursor[0];
        if last.is_some_and(|v| v >= index) {
            return Err("radix slots are not strictly ordered".into());
        }
        last = Some(index);
        let len = 2 + usize::from(cursor[1] & 0x7f) + 32;
        if cursor.len() < len {
            return Err("truncated radix pointer".into());
        }
        result.push((cursor[1] & 0x80 != 0, hash(&cursor[len - 32..len])?));
        cursor = &cursor[len..];
    }
    Ok(result)
}

fn reachable(
    db: RawDb,
    txn: &RoTxn,
    roots: impl IntoIterator<Item = Hash>,
    deadline: Instant,
) -> Result<(BTreeSet<Hash>, usize), Box<dyn Error>> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut visited = BTreeSet::new();
    let mut leaves = BTreeSet::new();
    while let Some(key) = pending.pop() {
        checked_time(deadline)?;
        if !visited.insert(key) {
            continue;
        }
        if visited.len() > CAPTURE_ROWS || pending.len() > CAPTURE_ROWS {
            return Err("radix traversal exceeds node bound".into());
        }
        let bytes = match get(db, txn, &key)? {
            Some(bytes) => bytes,
            None if key == digest(&[]) => continue,
            None => return Err("recorded root traversal has a missing history node".into()),
        };
        if digest(bytes) != key {
            return Err("history node content hash mismatch".into());
        }
        for (is_node, child) in node_children(bytes)? {
            if is_node {
                pending.push(child);
            } else {
                leaves.insert(child);
            }
        }
    }
    Ok((leaves, visited.len()))
}

struct CapturedRow {
    key_hash: Hash,
    value: Vec<u8>,
    bytes: u64,
    reachability: &'static str,
}

#[derive(Serialize, Deserialize)]
struct CaptureInfo {
    prefix: String,
    selected_root: String,
    selected_root_role: String,
    current_root: String,
    history_transaction: usize,
    cold_transaction: usize,
    recorded_roots: usize,
    selected_root_leaf_hashes: usize,
    all_root_leaf_hashes: usize,
    visited_history_nodes: usize,
    cold_rows: usize,
    cold_page_bytes: u64,
    encoded_key_value_bytes: u64,
    raw_key_rows: u64,
    bincode_key_rows: u64,
    reader_seconds: f64,
}

fn capture(
    data: &Path,
    prefix: &str,
    selected: Option<Hash>,
) -> Result<(CaptureInfo, Vec<CapturedRow>), Box<dyn Error>> {
    let start = Instant::now();
    let deadline = start + Duration::from_secs(10);
    let mut options = EnvOpenOptions::new();
    options.max_dbs(64);
    let history = unsafe {
        options
            .flags(EnvFlags::READ_ONLY)
            .open(data.join(prefix).join("history"))?
    };
    let cold = unsafe { options.open(data.join(prefix).join("cold"))? };
    let htxn = history.read_txn()?;
    let ctxn = cold.read_txn()?;
    let hdb = history
        .open_database::<Bytes, Bytes>(&htxn, Some(&format!("{prefix}-history")))?
        .ok_or("history database absent")?;
    let roots_db = history
        .open_database::<Bytes, Bytes>(&htxn, Some(&format!("{prefix}-roots")))?
        .ok_or("roots database absent")?;
    let cdb = cold
        .open_database::<Bytes, Bytes>(&ctxn, Some(&format!("{prefix}-cold")))?
        .ok_or("cold database absent")?;
    let current =
        hash(get(roots_db, &htxn, b"current-root")?.ok_or("current root pointer absent")?)?;
    let selected_root = selected.unwrap_or(current);
    let mut roots = BTreeSet::new();
    for entry in roots_db.iter(&htxn)? {
        checked_time(deadline)?;
        let (key, value) = entry?;
        let key = borrowed_bytes(key)?;
        if key == b"current-root" {
            continue;
        }
        if borrowed_bytes(value)? != b"tag" {
            return Err("unexpected root marker value".into());
        }
        roots.insert(hash(key)?);
    }
    let recorded_roots = roots.len();
    roots.insert(current);
    roots.insert(selected_root);
    let (selected_leaves, _) = reachable(hdb, &htxn, [selected_root], deadline)?;
    let (all_leaves, visited_history_nodes) = reachable(hdb, &htxn, roots, deadline)?;
    let mut preferred_keys = BTreeMap::new();
    for leaf in &all_leaves {
        checked_time(deadline)?;
        let raw = get(cdb, &ctxn, leaf)?.is_some();
        if !raw && get(cdb, &ctxn, &bincode::serialize(leaf.as_slice())?)?.is_none() {
            return Err("root references a missing cold leaf".into());
        }
        preferred_keys.insert(*leaf, raw);
    }
    let stat = cdb.stat(&ctxn)?;
    if stat.entries > CAPTURE_ROWS {
        return Err("cold row count exceeds capture bound".into());
    }
    let mut rows = Vec::with_capacity(stat.entries);
    let mut encoded_key_value_bytes = 0;
    let mut raw_key_rows = 0;
    let mut bincode_key_rows = 0;
    for entry in cdb.iter(&ctxn)? {
        checked_time(deadline)?;
        let (key, value) = entry?;
        let bytes = (key.len() + value.len()) as u64;
        encoded_key_value_bytes += bytes;
        if encoded_key_value_bytes > CAPTURE_BYTES {
            return Err("cold data exceeds 256 MiB capture bound".into());
        }
        let logical_key = borrowed_bytes(key)?;
        let raw = logical_key.len() == 32;
        let key_hash = if raw {
            raw_key_rows += 1;
            hash(logical_key)?
        } else {
            bincode_key_rows += 1;
            hash(borrowed_bytes(logical_key)?)?
        };
        let preferred = preferred_keys.get(&key_hash) == Some(&raw);
        let reachability = if preferred && selected_leaves.contains(&key_hash) {
            "selected_root"
        } else if preferred && all_leaves.contains(&key_hash) {
            "other_recorded_roots_only"
        } else {
            "not_referenced_by_captured_roots"
        };
        rows.push(CapturedRow {
            key_hash,
            value: borrowed_bytes(value)?.to_vec(),
            bytes,
            reachability,
        });
    }
    if rows.len() != stat.entries {
        return Err("cold snapshot row count mismatch".into());
    }
    let info = CaptureInfo {
        prefix: prefix.into(),
        selected_root: hex::encode(selected_root),
        selected_root_role: if selected.is_some() {
            "finalized_block_post_state"
        } else {
            "evaluator_current_root"
        }
        .into(),
        current_root: hex::encode(current),
        history_transaction: htxn.id(),
        cold_transaction: ctxn.id(),
        recorded_roots,
        selected_root_leaf_hashes: selected_leaves.len(),
        all_root_leaf_hashes: all_leaves.len(),
        visited_history_nodes,
        cold_rows: stat.entries,
        cold_page_bytes: (stat.branch_pages + stat.leaf_pages + stat.overflow_pages) as u64
            * u64::from(stat.page_size),
        encoded_key_value_bytes,
        raw_key_rows,
        bincode_key_rows,
        reader_seconds: start.elapsed().as_secs_f64(),
    };
    Ok((info, rows))
}

#[derive(Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stats {
    record_bytes: Distribution,
    elements_per_leaf: Distribution,
    element_bytes: Distribution,
    pars_per_leaf: Distribution,
    pars_per_element: Distribution,
    par_inclusive_bytes: Distribution,
    values_patterns_or_channels_per_element: Distribution,
    outer_par_count: u64,
    outer_par_bytes: u64,
    persistent_elements: u64,
    max_type_depth: usize,
}

impl Stats {
    fn validate(&self) -> Result<(), Box<dyn Error>> {
        for distribution in [
            &self.record_bytes,
            &self.elements_per_leaf,
            &self.element_bytes,
            &self.pars_per_leaf,
            &self.pars_per_element,
            &self.par_inclusive_bytes,
            &self.values_patterns_or_channels_per_element,
        ] {
            distribution.validate()?;
        }
        if self.elements_per_leaf.count != self.record_bytes.count
            || self.pars_per_leaf.count != self.record_bytes.count
            || self.elements_per_leaf.sum != self.element_bytes.count
            || self.pars_per_element.count != self.element_bytes.count
            || self.values_patterns_or_channels_per_element.count != self.element_bytes.count
            || self.pars_per_leaf.sum != self.par_inclusive_bytes.count
            || self.pars_per_element.sum != self.par_inclusive_bytes.count
            || self.persistent_elements > self.element_bytes.count
            || self.outer_par_count > self.par_inclusive_bytes.count
            || self.outer_par_bytes > self.element_bytes.sum
            || self.element_bytes.sum > self.record_bytes.sum
        {
            return Err("nested cardinality or size totals do not reconcile".into());
        }
        Ok(())
    }

    fn merge(&mut self, other: &Self) {
        self.record_bytes.merge(&other.record_bytes);
        self.elements_per_leaf.merge(&other.elements_per_leaf);
        self.element_bytes.merge(&other.element_bytes);
        self.pars_per_leaf.merge(&other.pars_per_leaf);
        self.pars_per_element.merge(&other.pars_per_element);
        self.par_inclusive_bytes.merge(&other.par_inclusive_bytes);
        self.values_patterns_or_channels_per_element
            .merge(&other.values_patterns_or_channels_per_element);
        self.outer_par_count += other.outer_par_count;
        self.outer_par_bytes += other.outer_par_bytes;
        self.persistent_elements += other.persistent_elements;
        self.max_type_depth = self.max_type_depth.max(other.max_type_depth);
    }

    fn add(&mut self, bytes: u64, e: &Elements) {
        self.record_bytes.add(bytes);
        self.elements_per_leaf.add(e.sizes.count);
        self.element_bytes.merge(&e.sizes);
        self.pars_per_leaf.add(e.nested.par_sizes.count);
        self.pars_per_element.merge(&e.pars_per_element);
        self.par_inclusive_bytes.merge(&e.nested.par_sizes);
        self.values_patterns_or_channels_per_element
            .merge(&e.groups_or_values);
        self.outer_par_count += e.nested.outer_par_count;
        self.outer_par_bytes += e.nested.outer_par_bytes;
        self.persistent_elements += e.persistent;
        self.max_type_depth = self.max_type_depth.max(e.nested.max_type_depth);
    }
}

#[derive(Serialize, Deserialize)]
struct Measurement {
    capture: CaptureInfo,
    by_kind: BTreeMap<String, Stats>,
    by_reachability: BTreeMap<String, BTreeMap<String, Stats>>,
    decode_seconds: f64,
}

fn measure(
    data: &Path,
    prefix: &str,
    selected: Option<Hash>,
) -> Result<Measurement, Box<dyn Error>> {
    let (capture, rows) = capture(data, prefix, selected)?;
    eprintln!(
        "{prefix}: captured {} rows in {:.3}s; reader transactions released",
        rows.len(),
        capture.reader_seconds
    );
    let start = Instant::now();
    let deadline = start + Duration::from_secs(60);
    let mut by_kind: BTreeMap<String, Stats> = ["DataLeaf", "ContinuationsLeaf", "JoinsLeaf"]
        .into_iter()
        .map(|k| (k.into(), Stats::default()))
        .collect();
    let mut by_reachability: BTreeMap<String, BTreeMap<String, Stats>> = [
        "selected_root",
        "other_recorded_roots_only",
        "not_referenced_by_captured_roots",
    ]
    .into_iter()
    .map(|c| {
        (
            c.into(),
            ["DataLeaf", "ContinuationsLeaf", "JoinsLeaf"]
                .into_iter()
                .map(|k| (k.into(), Stats::default()))
                .collect(),
        )
    })
    .collect();
    for row in rows {
        checked_time(deadline)?;
        let stored: PersistedData = decode(&row.value)?;
        let (kind, payload) = match &stored {
            PersistedData::Data(v) => ("DataLeaf", &v.bytes),
            PersistedData::Continuations(v) => ("ContinuationsLeaf", &v.bytes),
            PersistedData::Joins(v) => ("JoinsLeaf", &v.bytes),
        };
        if digest(&bincode::serialize(payload)?) != row.key_hash {
            return Err("cold leaf content hash mismatch".into());
        }
        let result = leaf_decode::analyze(kind, payload, deadline)?;
        by_kind.get_mut(kind).unwrap().add(row.bytes, &result);
        by_reachability
            .get_mut(row.reachability)
            .unwrap()
            .get_mut(kind)
            .unwrap()
            .add(row.bytes, &result);
    }
    let measured_rows: u64 = by_kind.values().map(|s| s.record_bytes.count).sum();
    let measured_bytes: u64 = by_kind.values().map(|s| s.record_bytes.sum).sum();
    if measured_rows != capture.cold_rows as u64
        || measured_bytes != capture.encoded_key_value_bytes
    {
        return Err("measurement totals do not reconcile".into());
    }
    let selected_rows: u64 = by_reachability["selected_root"]
        .values()
        .map(|s| s.record_bytes.count)
        .sum();
    if selected_rows != capture.selected_root_leaf_hashes as u64 {
        return Err("selected-root coverage incomplete".into());
    }
    Ok(Measurement {
        capture,
        by_kind,
        by_reachability,
        decode_seconds: start.elapsed().as_secs_f64(),
    })
}

#[derive(Serialize, Deserialize)]
struct Probe {
    schema_version: u32,
    started_unix_seconds: u64,
    complete: bool,
    measurements: Vec<Measurement>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|v| v == "--report") {
        if args.len() < 4 || args.len() > 5 || (args.len() == 5 && args[4] != "--check") {
            return Err(
                "usage: erd-leaf-stats --report OBSERVED_JSON OUTPUT_MARKDOWN [--check]".into(),
            );
        }
        return leaf_report::generate(Path::new(&args[2]), Path::new(&args[3]), args.len() == 5);
    }
    if args.len() != 3 {
        return Err("usage: erd-leaf-stats DATA_DIRECTORY FINALIZED_POST_STATE_HASH".into());
    }
    let root = hash(&hex::decode(&args[2]).map_err(|_| "invalid root hex")?)?;
    let start = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let data = Path::new(&args[1]);
    let result = Probe {
        schema_version: 1,
        started_unix_seconds: start,
        complete: true,
        measurements: vec![
            measure(data, "rspace", Some(root))?,
            measure(data, "eval", None)?,
        ],
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn fixture(path: &Path) -> Hash {
        use leaf_decode::{
            rhoapi, Consume, ContinuationsLeaf, DataLeaf, Datum, JoinsLeaf, Produce,
            WaitingContinuation,
        };
        let datum = |count| Datum {
            a: rhoapi::ListParWithRandom {
                pars: vec![rhoapi::Par::default(); count],
                random_state: vec![],
            },
            persist: false,
            source: Produce {
                channel_hash: vec![0; 32],
                hash: vec![1; 32],
                persistent: false,
                is_deterministic: true,
                output_value: vec![],
                failed: false,
            },
        };
        let data = |count| {
            PersistedData::Data(DataLeaf {
                bytes: bincode::serialize(&vec![bincode::serialize(&datum(count)).unwrap()])
                    .unwrap(),
            })
        };
        let wk = WaitingContinuation {
            patterns: vec![rhoapi::BindPattern::default()],
            continuation: rhoapi::TaggedContinuation::default(),
            persist: true,
            peeks: BTreeSet::new(),
            source: Consume {
                channel_hashes: vec![vec![0; 32]],
                hash: vec![2; 32],
                persistent: true,
            },
        };
        let kont = PersistedData::Continuations(ContinuationsLeaf {
            bytes: bincode::serialize(&vec![bincode::serialize(&wk).unwrap()]).unwrap(),
        });
        let joins = PersistedData::Joins(JoinsLeaf {
            bytes: bincode::serialize(&vec![bincode::serialize(&vec![
                rhoapi::Par::default(),
                rhoapi::Par::default(),
            ])
            .unwrap()])
            .unwrap(),
        });
        let mut chosen = [0; 32];
        for prefix in ["rspace", "eval"] {
            let hp = path.join(prefix).join("history");
            let cp = path.join(prefix).join("cold");
            std::fs::create_dir_all(&hp).unwrap();
            std::fs::create_dir_all(&cp).unwrap();
            let he = unsafe {
                EnvOpenOptions::new()
                    .map_size(16 << 20)
                    .max_dbs(4)
                    .open(&hp)
                    .unwrap()
            };
            let ce = unsafe {
                EnvOpenOptions::new()
                    .map_size(16 << 20)
                    .max_dbs(4)
                    .open(&cp)
                    .unwrap()
            };
            let mut ht = he.write_txn().unwrap();
            let mut ct = ce.write_txn().unwrap();
            let hd = he
                .create_database::<Bytes, Bytes>(&mut ht, Some(&format!("{prefix}-history")))
                .unwrap();
            let rd = he
                .create_database::<Bytes, Bytes>(&mut ht, Some(&format!("{prefix}-roots")))
                .unwrap();
            let cd = ce
                .create_database::<Bytes, Bytes>(&mut ct, Some(&format!("{prefix}-cold")))
                .unwrap();
            let mut current = digest(&[]);
            if prefix == "rspace" {
                let values = [data(1), kont.clone(), joins.clone(), data(2)];
                let mut hashes = Vec::new();
                for v in &values {
                    let payload = match v {
                        PersistedData::Data(v) => &v.bytes,
                        PersistedData::Joins(v) => &v.bytes,
                        PersistedData::Continuations(v) => &v.bytes,
                    };
                    let h = digest(&bincode::serialize(payload).unwrap());
                    hashes.push(h);
                    let key = bincode::serialize(h.as_slice()).unwrap();
                    cd.put(
                        &mut ct,
                        &bincode::serialize(&key).unwrap(),
                        &bincode::serialize(&bincode::serialize(v).unwrap()).unwrap(),
                    )
                    .unwrap();
                }
                cd.put(
                    &mut ct,
                    &bincode::serialize(hashes[0].as_slice()).unwrap(),
                    &bincode::serialize(&bincode::serialize(&values[0]).unwrap()).unwrap(),
                )
                .unwrap();
                let mut node = Vec::new();
                for (i, h) in hashes[..3].iter().enumerate() {
                    node.extend([i as u8, 32]);
                    node.extend([0; 32]);
                    node.extend(h);
                    if i == 0 {
                        chosen = digest(&node);
                        hd.put(
                            &mut ht,
                            &bincode::serialize(chosen.as_slice()).unwrap(),
                            &bincode::serialize(&node).unwrap(),
                        )
                        .unwrap();
                        rd.put(
                            &mut ht,
                            &bincode::serialize(chosen.as_slice()).unwrap(),
                            &bincode::serialize(b"tag".as_slice()).unwrap(),
                        )
                        .unwrap();
                    }
                }
                current = digest(&node);
                hd.put(
                    &mut ht,
                    &bincode::serialize(current.as_slice()).unwrap(),
                    &bincode::serialize(&node).unwrap(),
                )
                .unwrap();
            }
            rd.put(
                &mut ht,
                &bincode::serialize(current.as_slice()).unwrap(),
                &bincode::serialize(b"tag".as_slice()).unwrap(),
            )
            .unwrap();
            rd.put(
                &mut ht,
                &bincode::serialize(b"current-root".as_slice()).unwrap(),
                &bincode::serialize(current.as_slice()).unwrap(),
            )
            .unwrap();
            ct.commit().unwrap();
            ht.commit().unwrap();
        }
        chosen
    }

    #[test]
    fn full_measurement_reconciles_variants_reachability_and_nested_records_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let root = fixture(dir.path());
        let path = dir.path().join("rspace/cold/data.mdb");
        let before = std::fs::read(&path).unwrap();
        let result = measure(dir.path(), "rspace", Some(root)).unwrap();
        assert_eq!(result.capture.cold_rows, 5);
        assert_eq!(result.capture.raw_key_rows, 1);
        assert_eq!(result.capture.bincode_key_rows, 4);
        assert_eq!(result.by_kind["DataLeaf"].record_bytes.count, 3);
        assert_eq!(result.by_kind["ContinuationsLeaf"].persistent_elements, 1);
        assert_eq!(
            result.by_kind["JoinsLeaf"]
                .values_patterns_or_channels_per_element
                .sum,
            2
        );
        assert_eq!(
            result.by_reachability["selected_root"]["DataLeaf"]
                .record_bytes
                .count,
            1
        );
        assert_eq!(
            result.by_reachability["other_recorded_roots_only"]["JoinsLeaf"]
                .record_bytes
                .count,
            1
        );
        assert_eq!(
            result.by_reachability["not_referenced_by_captured_roots"]["DataLeaf"]
                .record_bytes
                .count,
            2
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let eval = measure(dir.path(), "eval", None).unwrap();
        assert_eq!(eval.capture.cold_rows, 0);
        assert_eq!(eval.capture.selected_root_leaf_hashes, 0);
    }

    #[test]
    fn selected_root_must_exist_and_capture_deadlines_are_enforced() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        assert!(capture(dir.path(), "rspace", Some([99; 32])).is_err());
        assert!(checked_time(Instant::now() - Duration::from_secs(1)).is_err());
    }

    #[test]
    fn missing_referenced_leaves_and_corrupt_content_fail_measurement() {
        for corrupt in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let root = fixture(dir.path());
            let env = unsafe {
                EnvOpenOptions::new()
                    .max_dbs(4)
                    .open(dir.path().join("rspace/cold"))
                    .unwrap()
            };
            let mut txn = env.write_txn().unwrap();
            let db = env
                .open_database::<Bytes, Bytes>(&txn, Some("rspace-cold"))
                .unwrap()
                .unwrap();
            if corrupt {
                let (key, value) = db.iter(&txn).unwrap().next().unwrap().unwrap();
                let key = key.to_vec();
                let mut value = value.to_vec();
                let last = value.len() - 1;
                value[last] ^= 1;
                db.put(&mut txn, &key, &value).unwrap();
            } else {
                db.clear(&mut txn).unwrap();
            }
            txn.commit().unwrap();
            drop(env);
            let error = measure(dir.path(), "rspace", Some(root))
                .err()
                .unwrap()
                .to_string();
            assert_eq!(
                error,
                if corrupt {
                    "cold leaf content hash mismatch"
                } else {
                    "root references a missing cold leaf"
                }
            );
        }
    }

    #[test]
    fn compact_radix_parser_handles_both_pointer_kinds_and_rejects_damage() {
        let mut node = vec![0, 1, 9];
        node.extend([1; 32]);
        node.extend([2, 128]);
        node.extend([2; 32]);
        assert_eq!(node_children(&node).unwrap(), vec![
            (false, [1; 32]),
            (true, [2; 32])
        ]);
        assert!(node_children(&node[..node.len() - 1]).is_err());
        assert!(node_children(&[0]).is_err());
        let mut bad = vec![1, 0];
        bad.extend([1; 32]);
        bad.extend([1, 0]);
        bad.extend([2; 32]);
        assert!(node_children(&bad).is_err());
    }
}
