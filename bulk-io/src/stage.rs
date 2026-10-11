use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use rholang::rust::interpreter::io::bulk::{
    blake2b, current_root, hex_root, read_sealed_root, scan_tree, seal_tree, staging_path,
    verify_tree, EMPTY_ROOT,
};
use serde::Serialize;

use crate::adapter::adapter_id;
use crate::error::BulkError;
use crate::manifest::ImportManifest;
use crate::transform::{
    self, read_partitions, render_rejects, write_partitions, PROVENANCE_FILE, REJECTS_FILE,
    SCHEMA_FILE,
};

pub fn stage_id(
    namespace: &str,
    base: &[u8; 32],
    source_root: &[u8; 32],
    manifest_hash: &[u8; 32],
) -> [u8; 32] {
    let mut buf = b"f1r3fly/bulk-io/stage/v1".to_vec();
    buf.extend_from_slice(&(namespace.len() as u32).to_be_bytes());
    buf.extend_from_slice(namespace.as_bytes());
    buf.extend_from_slice(base);
    buf.extend_from_slice(source_root);
    buf.extend_from_slice(manifest_hash);
    blake2b(&buf)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StageReport {
    #[serde(rename = "stageId")]
    pub stage_id: String,
    pub namespace: String,
    #[serde(rename = "baseRoot")]
    pub base_root: String,
    #[serde(rename = "sourceRoot")]
    pub source_root: String,
    #[serde(rename = "manifestHash")]
    pub manifest_hash: String,
    #[serde(rename = "resultRoot")]
    pub result_root: String,
    pub rows: u64,
    pub upserts: u64,
    pub deletes: u64,
    pub rejected: u64,
    pub records: u64,
    pub partitions: u64,
    pub files: u64,
    pub bytes: u64,
    #[serde(rename = "oracularRecords")]
    pub oracular_records: u64,
    pub path: Option<String>,
    #[serde(rename = "dryRun")]
    pub dry_run: bool,
}

pub struct PrepareOptions<'a> {
    pub import_root: &'a Path,
    pub expected_base: Option<[u8; 32]>,
    pub expected_result: Option<[u8; 32]>,
    pub oracular_dir: Option<&'a Path>,
    pub dry_run: bool,
}

fn schema_compatible(base: &ImportManifest, next: &ImportManifest) -> bool {
    base.key == next.key
        && base.fields == next.fields
        && base.layout == next.layout
        && base.transform == next.transform
}

fn load_base_schema(target: &Path) -> Result<ImportManifest, BulkError> {
    let p = target.join(SCHEMA_FILE);
    let text = fs::read_to_string(&p).map_err(|e| BulkError::io(&p, e))?;
    ImportManifest::parse(&text)
}

fn make_read_only(dir: &Path) -> Result<(), BulkError> {
    for e in scan_tree(dir)? {
        let p = dir.join(&e.rel);
        fs::set_permissions(&p, fs::Permissions::from_mode(0o444))
            .map_err(|err| BulkError::io(&p, err))?;
    }
    for name in [
        rholang::rust::interpreter::io::bulk::MANIFEST_FILE,
        rholang::rust::interpreter::io::bulk::ROOT_FILE,
    ] {
        let p = dir.join(name);
        fs::set_permissions(&p, fs::Permissions::from_mode(0o444))
            .map_err(|err| BulkError::io(&p, err))?;
    }
    Ok(())
}

fn remove_tree(dir: &Path) {
    if let Ok(entries) = scan_tree(dir) {
        for e in entries {
            let _ = fs::set_permissions(dir.join(&e.rel), fs::Permissions::from_mode(0o644));
        }
    }
    let _ = fs::remove_dir_all(dir);
}

fn write_tree(
    dir: &Path,
    manifest: &ImportManifest,
    out: &transform::TransformOutput,
    prov: BTreeMap<&str, serde_json::Value>,
) -> Result<u64, BulkError> {
    fs::create_dir_all(dir).map_err(|e| BulkError::io(dir, e))?;
    let files = write_partitions(dir, &out.partitions, manifest.layout.max_file_bytes)?;
    let mut schema = manifest.clone();
    schema.delta = false;
    schema.source.locations = vec!["file:///schema".into()];
    schema.source.root = hex_root(&EMPTY_ROOT);
    fs::write(dir.join(SCHEMA_FILE), schema.canonical_bytes())
        .map_err(|e| BulkError::io(dir, e))?;
    fs::write(dir.join(REJECTS_FILE), render_rejects(&out.rejects))
        .map_err(|e| BulkError::io(dir, e))?;
    fs::write(dir.join(PROVENANCE_FILE), transform::provenance(prov))
        .map_err(|e| BulkError::io(dir, e))?;
    Ok(files)
}

pub fn prepare(
    manifest: &ImportManifest,
    source: &Path,
    opts: &PrepareOptions<'_>,
) -> Result<StageReport, BulkError> {
    let target = opts.import_root.join(&manifest.namespace);
    let base = current_root(&target)?;
    if let Some(expected) = opts.expected_base {
        if expected != base {
            return Err(BulkError::Verify(format!(
                "committed root is {}, expected base {}",
                hex_root(&base),
                hex_root(&expected)
            )));
        }
    }
    let base_parts = if manifest.delta {
        if base == EMPTY_ROOT {
            return Err(BulkError::Manifest(
                "a delta needs an existing committed namespace".into(),
            ));
        }
        verify_tree(&target)?;
        if !schema_compatible(&load_base_schema(&target)?, manifest) {
            return Err(BulkError::Manifest(
                "delta schema differs from the committed schema".into(),
            ));
        }
        Some(read_partitions(&target)?)
    } else {
        None
    };
    let source_root = manifest.source_root();
    let manifest_hash = manifest.hash();
    let sid = stage_id(&manifest.namespace, &base, &source_root, &manifest_hash);
    let sid_hex = hex_root(&sid);
    let final_dir = staging_path(opts.import_root, &sid_hex);

    if !opts.dry_run && final_dir.exists() {
        let root = verify_tree(&final_dir).map_err(|e| {
            BulkError::Verify(format!(
                "{} exists but is not a valid sealed tree ({e}); remove it and run again",
                final_dir.display()
            ))
        })?;
        if let Some(expected) = opts.expected_result {
            if expected != root {
                return Err(BulkError::Verify(format!(
                    "existing staged root {} differs from expected {}",
                    hex_root(&root),
                    hex_root(&expected)
                )));
            }
        }
    }

    let out = transform::run(manifest, source, base_parts)?;
    let records: u64 = out.partitions.values().map(|p| p.len() as u64).sum();
    let oracular_records: u64 = out.oracular.values().map(|p| p.len() as u64).sum();
    let mut prov = BTreeMap::new();
    prov.insert("stageId", serde_json::json!(sid_hex));
    prov.insert("namespace", serde_json::json!(manifest.namespace));
    prov.insert("baseRoot", serde_json::json!(hex_root(&base)));
    prov.insert("sourceRoot", serde_json::json!(hex_root(&source_root)));
    prov.insert("manifestHash", serde_json::json!(hex_root(&manifest_hash)));
    prov.insert(
        "adapter",
        serde_json::json!(adapter_id(manifest.source.format)),
    );
    prov.insert("transform", serde_json::json!(manifest.transform));
    prov.insert(
        "transformId",
        serde_json::json!(hex_root(
            &transform::transform_id(&manifest.transform).expect("validated")
        )),
    );
    prov.insert("rows", serde_json::json!(out.rows));
    prov.insert("upserts", serde_json::json!(out.upserts));
    prov.insert("deletes", serde_json::json!(out.deletes));
    prov.insert("rejected", serde_json::json!(out.rejects.len()));
    prov.insert("records", serde_json::json!(records));

    let work_parent = if opts.dry_run {
        std::env::temp_dir()
    } else {
        final_dir
            .parent()
            .expect("staging has a parent")
            .to_path_buf()
    };
    fs::create_dir_all(&work_parent).map_err(|e| BulkError::io(&work_parent, e))?;
    let work = work_parent.join(format!(".work-{sid_hex}-{}", std::process::id()));
    remove_tree(&work);
    let result = (|| -> Result<(u64, [u8; 32], u64), BulkError> {
        let files = write_tree(&work, manifest, &out, prov)?;
        let root = seal_tree(&work)?;
        let bytes = scan_tree(&work)?.iter().map(|e| e.len).sum();
        Ok((files, root, bytes))
    })();
    let (files, root, bytes) = match result {
        Ok(v) => v,
        Err(e) => {
            remove_tree(&work);
            return Err(e);
        }
    };
    if let Some(expected) = opts.expected_result {
        if expected != root {
            remove_tree(&work);
            return Err(BulkError::Verify(format!(
                "computed result root {} differs from expected {}",
                hex_root(&root),
                hex_root(&expected)
            )));
        }
    }

    let path = if opts.dry_run {
        remove_tree(&work);
        None
    } else {
        if final_dir.exists() {
            remove_tree(&work);
            if read_sealed_root(&final_dir)? != root {
                return Err(BulkError::Verify(format!(
                    "{} holds a different tree; remove it and run again",
                    final_dir.display()
                )));
            }
        } else {
            make_read_only(&work)?;
            fs::rename(&work, &final_dir).map_err(|e| BulkError::io(&final_dir, e))?;
        }
        if let Some(odir) = opts.oracular_dir {
            if oracular_records > 0 {
                let odest: PathBuf = odir.join(&manifest.namespace).join(&sid_hex);
                remove_tree(&odest);
                write_partitions(&odest, &out.oracular, manifest.layout.max_file_bytes)?;
            }
        }
        Some(final_dir.display().to_string())
    };

    Ok(StageReport {
        stage_id: sid_hex,
        namespace: manifest.namespace.clone(),
        base_root: hex_root(&base),
        source_root: hex_root(&source_root),
        manifest_hash: hex_root(&manifest_hash),
        result_root: hex_root(&root),
        rows: out.rows,
        upserts: out.upserts,
        deletes: out.deletes,
        rejected: out.rejects.len() as u64,
        records,
        partitions: out.partitions.len() as u64,
        files,
        bytes,
        oracular_records,
        path,
        dry_run: opts.dry_run,
    })
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::io::bulk::{retired_path, swap_in};

    use super::*;

    fn manifest(root: &str, delta: bool) -> ImportManifest {
        ImportManifest::parse(&format!(
            r#"{{"version":1,"namespace":"t/people","source":{{"format":"csv","locations":["file:///x"],"root":"{root}"}},
            "transform":"tabular/1","key":["id"],"delta":{delta},
            "fields":[{{"name":"id","type":"int","required":true}},{{"name":"name","type":"string"}},{{"name":"email","type":"string","route":"oracular"}}]}}"#
        ))
        .unwrap()
    }

    fn source(dir: &Path, body: &str) -> (PathBuf, String) {
        let p = dir.join(format!("src-{}.csv", body.len()));
        fs::write(&p, body).unwrap();
        let root = hex_root(&crate::source::source_root(&p).unwrap());
        (p, root)
    }

    fn opts(import: &Path) -> PrepareOptions<'_> {
        PrepareOptions {
            import_root: import,
            expected_base: None,
            expected_result: None,
            oracular_dir: None,
            dry_run: false,
        }
    }

    #[test]
    fn two_validators_compute_the_same_root() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let (p, root) = source(
            a.path(),
            "id,name,email\n1,ann,a@example.com\n2,bob,b@example.com\n",
        );
        let m = manifest(&root, false);
        let ra = prepare(&m, &p, &opts(&a.path().join("import"))).unwrap();
        let rb = prepare(&m, &p, &opts(&b.path().join("import"))).unwrap();
        assert_eq!(ra.result_root, rb.result_root);
        assert_eq!(ra.stage_id, rb.stage_id);
        assert_eq!(ra.records, 2);
        let again = prepare(&m, &p, &opts(&a.path().join("import"))).unwrap();
        assert_eq!(again.result_root, ra.result_root);
    }

    #[test]
    fn dry_run_writes_nothing_and_matches() {
        let a = tempfile::tempdir().unwrap();
        let (p, root) = source(a.path(), "id,name,email\n1,ann,\n");
        let m = manifest(&root, false);
        let import = a.path().join("import");
        let mut o = opts(&import);
        o.dry_run = true;
        let dry = prepare(&m, &p, &o).unwrap();
        assert!(dry.path.is_none());
        assert!(!import.join(".bulk/staging").join(&dry.stage_id).exists());
        o.dry_run = false;
        assert_eq!(prepare(&m, &p, &o).unwrap().result_root, dry.result_root);
    }

    #[test]
    fn expected_result_mismatch_fails_cleanly() {
        let a = tempfile::tempdir().unwrap();
        let (p, root) = source(a.path(), "id,name,email\n1,ann,\n");
        let m = manifest(&root, false);
        let import = a.path().join("import");
        let mut o = opts(&import);
        o.expected_result = Some([5u8; 32]);
        assert!(prepare(&m, &p, &o).is_err());
        let staging = a.path().join("import/.bulk/staging");
        assert!(fs::read_dir(&staging)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true));
    }

    #[test]
    fn full_flow_import_commit_delta_commit() {
        let a = tempfile::tempdir().unwrap();
        let import = a.path().join("import");
        let (p1, r1) = source(a.path(), "id,name,email\n1,ann,a@example.com\n2,bob,\n");
        let m1 = manifest(&r1, false);
        let s1 = prepare(&m1, &p1, &opts(&import)).unwrap();
        let target = import.join("t/people");
        let res1 = rholang::rust::interpreter::io::bulk::parse_hex_root(&s1.result_root).unwrap();
        swap_in(
            &staging_path(&import, &s1.stage_id),
            &target,
            &retired_path(&import, &s1.stage_id),
            &res1,
            &EMPTY_ROOT,
        )
        .unwrap();
        assert_eq!(verify_tree(&target).unwrap(), res1);

        let (p2, r2) = source(a.path(), "_op,id,name,email\nupsert,3,cat,\ndelete,1,,\n");
        let m2 = manifest(&r2, true);
        let mut o = opts(&import);
        o.expected_base = Some(res1);
        let s2 = prepare(&m2, &p2, &o).unwrap();
        assert_eq!((s2.upserts, s2.deletes, s2.records), (1, 1, 2));
        assert_eq!(s2.base_root, s1.result_root);
        let res2 = rholang::rust::interpreter::io::bulk::parse_hex_root(&s2.result_root).unwrap();
        swap_in(
            &staging_path(&import, &s2.stage_id),
            &target,
            &retired_path(&import, &s2.stage_id),
            &res2,
            &res1,
        )
        .unwrap();
        assert_eq!(verify_tree(&target).unwrap(), res2);
    }

    #[test]
    fn oracular_fields_never_enter_the_consensus_tree() {
        let a = tempfile::tempdir().unwrap();
        let import = a.path().join("import");
        let orac = a.path().join("oracular");
        let (p, root) = source(a.path(), "id,name,email\n1,ann,secret@example.com\n");
        let m = manifest(&root, false);
        let mut o = opts(&import);
        o.oracular_dir = Some(&orac);
        let s = prepare(&m, &p, &o).unwrap();
        assert_eq!(s.oracular_records, 1);
        let staged = staging_path(&import, &s.stage_id);
        for e in scan_tree(&staged).unwrap() {
            let bytes = fs::read(staged.join(&e.rel)).unwrap();
            assert!(
                !bytes.windows(6).any(|w| w == b"secret"),
                "{} leaked",
                e.rel
            );
        }
        assert!(orac.join("t/people").join(&s.stage_id).exists());
    }
}
