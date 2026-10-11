use std::fs;
use std::path::Path;

use rholang::rust::interpreter::io::bulk::{hex_root, verify_tree};
use serde::Serialize;

use crate::error::BulkError;
use crate::manifest::ImportManifest;
use crate::transform::{self, read_partitions, render_rejects, REJECTS_FILE};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReconcileReport {
    pub ok: bool,
    #[serde(rename = "resultRoot")]
    pub result_root: String,
    #[serde(rename = "sourceRows")]
    pub source_rows: u64,
    #[serde(rename = "treeRecords")]
    pub tree_records: u64,
    #[serde(rename = "expectedRecords")]
    pub expected_records: u64,
    pub rejected: u64,
    #[serde(rename = "missingKeys")]
    pub missing_keys: u64,
    #[serde(rename = "unexpectedKeys")]
    pub unexpected_keys: u64,
    #[serde(rename = "changedRecords")]
    pub changed_records: u64,
    #[serde(rename = "rejectLogMatches")]
    pub reject_log_matches: bool,
}

pub fn reconcile(
    tree: &Path,
    manifest: &ImportManifest,
    source: &Path,
    base: Option<&Path>,
) -> Result<ReconcileReport, BulkError> {
    let root = verify_tree(tree)?;
    let base_parts = match (manifest.delta, base) {
        (true, Some(b)) => {
            verify_tree(b)?;
            Some(read_partitions(b)?)
        }
        (true, None) => {
            return Err(BulkError::Manifest(
                "reconciling a delta needs the base tree".into(),
            ))
        }
        (false, _) => None,
    };
    let expected = transform::run(manifest, source, base_parts)?;
    let actual = read_partitions(tree)?;
    let flat = |p: &transform::Partitions| -> std::collections::BTreeMap<Vec<u8>, Vec<u8>> {
        p.values()
            .flat_map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())))
            .collect()
    };
    let e = flat(&expected.partitions);
    let a = flat(&actual);
    let missing = e.keys().filter(|k| !a.contains_key(*k)).count() as u64;
    let unexpected = a.keys().filter(|k| !e.contains_key(*k)).count() as u64;
    let changed = e
        .iter()
        .filter(|(k, v)| a.get(*k).is_some_and(|av| av != *v))
        .count() as u64;
    let rejects_path = tree.join(REJECTS_FILE);
    let reject_log =
        fs::read_to_string(&rejects_path).map_err(|err| BulkError::io(&rejects_path, err))?;
    let reject_log_matches = reject_log == render_rejects(&expected.rejects);
    Ok(ReconcileReport {
        ok: missing == 0 && unexpected == 0 && changed == 0 && reject_log_matches,
        result_root: hex_root(&root),
        source_rows: expected.rows,
        tree_records: a.len() as u64,
        expected_records: e.len() as u64,
        rejected: expected.rejects.len() as u64,
        missing_keys: missing,
        unexpected_keys: unexpected,
        changed_records: changed,
        reject_log_matches,
    })
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::io::bulk::staging_path;

    use super::*;
    use crate::stage::{prepare, PrepareOptions};

    #[test]
    fn reconcile_matches_its_own_source_and_flags_another() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("s.csv");
        fs::write(&src, "id,name\n1,ann\n2,bob\nx,bad\n").unwrap();
        let root = hex_root(&crate::source::source_root(&src).unwrap());
        let m = ImportManifest::parse(&format!(
            r#"{{"version":1,"namespace":"t/x","source":{{"format":"csv","locations":["file:///x"],"root":"{root}"}},
            "transform":"tabular/1","key":["id"],"fields":[{{"name":"id","type":"int","required":true}},{{"name":"name","type":"string"}}]}}"#
        ))
        .unwrap();
        let import = d.path().join("import");
        let r = prepare(&m, &src, &PrepareOptions {
            import_root: &import,
            expected_base: None,
            expected_result: None,
            oracular_dir: None,
            dry_run: false,
        })
        .unwrap();
        let tree = staging_path(&import, &r.stage_id);
        let rep = reconcile(&tree, &m, &src, None).unwrap();
        assert!(rep.ok, "{rep:?}");
        assert_eq!((rep.source_rows, rep.tree_records, rep.rejected), (3, 2, 1));
        let other = d.path().join("o.csv");
        fs::write(&other, "id,name\n1,ann\n3,cat\n").unwrap();
        let rep2 = reconcile(&tree, &m, &other, None).unwrap();
        assert!(!rep2.ok);
        assert_eq!((rep2.missing_keys, rep2.unexpected_keys), (1, 1));
    }
}
