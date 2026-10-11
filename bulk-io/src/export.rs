use std::fs;
use std::path::Path;

use rholang::rust::interpreter::io::bulk::{
    blake2b, hex_root, parse_hex_root, scan_tree, verify_tree, MANIFEST_FILE, ROOT_FILE,
};
use serde::{Deserialize, Serialize};

use crate::error::BulkError;
use crate::manifest::ImportManifest;
use crate::transform::{read_partitions, SCHEMA_FILE};
use crate::value::{decode_record, value_to_json, value_to_text, Value};

pub const EXPORT_FILE: &str = "EXPORT.json";
pub const TREE_DIR: &str = "tree";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Jsonl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportProof {
    pub namespace: String,
    #[serde(rename = "resultRoot")]
    pub result_root: String,
    #[serde(rename = "blockHash")]
    pub block_hash: Option<String>,
    pub format: ExportFormat,
    #[serde(rename = "renderedFile")]
    pub rendered_file: String,
    #[serde(rename = "renderedHash")]
    pub rendered_hash: String,
    pub records: u64,
}

pub fn render(tree: &Path, format: ExportFormat) -> Result<(Vec<u8>, u64), BulkError> {
    let schema_path = tree.join(SCHEMA_FILE);
    let schema = ImportManifest::parse(
        &fs::read_to_string(&schema_path).map_err(|e| BulkError::io(&schema_path, e))?,
    )?;
    let columns: Vec<String> = schema.consensus_fields().map(|f| f.name.clone()).collect();
    let mut all: Vec<(Vec<u8>, Vec<u8>)> = read_partitions(tree)?
        .into_values()
        .flat_map(|p| p.into_iter())
        .collect();
    all.sort();
    let mut out = Vec::new();
    match format {
        ExportFormat::Csv => {
            let mut w = csv::WriterBuilder::new()
                .terminator(csv::Terminator::Any(b'\n'))
                .from_writer(&mut out);
            w.write_record(&columns)
                .map_err(|e| BulkError::Export(e.to_string()))?;
            for (_, r) in &all {
                let rec = decode_record(r)?;
                let row: Vec<String> = columns
                    .iter()
                    .map(|c| value_to_text(rec.get(c).unwrap_or(&Value::Null)))
                    .collect();
                w.write_record(&row)
                    .map_err(|e| BulkError::Export(e.to_string()))?;
            }
            w.flush().map_err(|e| BulkError::Export(e.to_string()))?;
        }
        ExportFormat::Jsonl => {
            for (_, r) in &all {
                let rec = decode_record(r)?;
                let obj: serde_json::Map<String, serde_json::Value> = columns
                    .iter()
                    .map(|c| (c.clone(), value_to_json(rec.get(c).unwrap_or(&Value::Null))))
                    .collect();
                out.extend_from_slice(&serde_json::to_vec(&obj).expect("json"));
                out.push(b'\n');
            }
        }
    }
    Ok((out, all.len() as u64))
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), BulkError> {
    let mut entries = scan_tree(src)?;
    entries.push(rholang::rust::interpreter::io::bulk::TreeEntry {
        rel: MANIFEST_FILE.into(),
        len: 0,
        hash: [0; 32],
    });
    entries.push(rholang::rust::interpreter::io::bulk::TreeEntry {
        rel: ROOT_FILE.into(),
        len: 0,
        hash: [0; 32],
    });
    for e in entries {
        let to = dst.join(&e.rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|err| BulkError::io(parent, err))?;
        }
        fs::copy(src.join(&e.rel), &to).map_err(|err| BulkError::io(&to, err))?;
    }
    Ok(())
}

pub fn export(
    tree: &Path,
    out_dir: &Path,
    format: ExportFormat,
    block_hash: Option<String>,
) -> Result<ExportProof, BulkError> {
    if let Some(h) = &block_hash {
        if h.is_empty() || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(BulkError::Export("block hash must be hex".into()));
        }
    }
    let root = verify_tree(tree)?;
    if out_dir.exists()
        && fs::read_dir(out_dir)
            .map_err(|e| BulkError::io(out_dir, e))?
            .next()
            .is_some()
    {
        return Err(BulkError::Export(format!(
            "{} is not empty",
            out_dir.display()
        )));
    }
    let copy = out_dir.join(TREE_DIR);
    copy_tree(tree, &copy)?;
    if verify_tree(&copy)? != root {
        return Err(BulkError::Export("copied tree does not verify".into()));
    }
    let schema_path = copy.join(SCHEMA_FILE);
    let schema = ImportManifest::parse(
        &fs::read_to_string(&schema_path).map_err(|e| BulkError::io(&schema_path, e))?,
    )?;
    let (bytes, records) = render(&copy, format)?;
    let rendered_file = match format {
        ExportFormat::Csv => "data.csv",
        ExportFormat::Jsonl => "data.jsonl",
    };
    fs::write(out_dir.join(rendered_file), &bytes).map_err(|e| BulkError::io(out_dir, e))?;
    let proof = ExportProof {
        namespace: schema.namespace,
        result_root: hex_root(&root),
        block_hash,
        format,
        rendered_file: rendered_file.into(),
        rendered_hash: hex_root(&blake2b(&bytes)),
        records,
    };
    let mut j = serde_json::to_vec_pretty(&proof).expect("json");
    j.push(b'\n');
    fs::write(out_dir.join(EXPORT_FILE), j).map_err(|e| BulkError::io(out_dir, e))?;
    Ok(proof)
}

pub fn verify_export(
    out_dir: &Path,
    expected_root: Option<[u8; 32]>,
) -> Result<ExportProof, BulkError> {
    let p = out_dir.join(EXPORT_FILE);
    let proof: ExportProof =
        serde_json::from_slice(&fs::read(&p).map_err(|e| BulkError::io(&p, e))?)
            .map_err(|e| BulkError::Verify(format!("{EXPORT_FILE}: {e}")))?;
    let stated = parse_hex_root(&proof.result_root)
        .ok_or_else(|| BulkError::Verify("bad resultRoot".into()))?;
    if let Some(expected) = expected_root {
        if expected != stated {
            return Err(BulkError::Verify(
                "result root differs from the expected committed root".into(),
            ));
        }
    }
    let tree = out_dir.join(TREE_DIR);
    if verify_tree(&tree)? != stated {
        return Err(BulkError::Verify("tree does not match resultRoot".into()));
    }
    let (bytes, records) = render(&tree, proof.format)?;
    if hex_root(&blake2b(&bytes)) != proof.rendered_hash || records != proof.records {
        return Err(BulkError::Verify(
            "rendered data does not match the tree".into(),
        ));
    }
    let on_disk = out_dir.join(&proof.rendered_file);
    if fs::read(&on_disk).map_err(|e| BulkError::io(&on_disk, e))? != bytes {
        return Err(BulkError::Verify(format!(
            "{} was modified",
            proof.rendered_file
        )));
    }
    Ok(proof)
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::io::bulk::staging_path;

    use super::*;
    use crate::stage::{prepare, PrepareOptions};

    fn staged(dir: &Path, body: &str) -> std::path::PathBuf {
        let src = dir.join("s.csv");
        fs::write(&src, body).unwrap();
        let root = hex_root(&crate::source::source_root(&src).unwrap());
        let m = ImportManifest::parse(&format!(
            r#"{{"version":1,"namespace":"t/x","source":{{"format":"csv","locations":["file:///x"],"root":"{root}"}},
            "transform":"tabular/1","key":["id"],
            "fields":[{{"name":"id","type":"int","required":true}},{{"name":"name","type":"string"}},{{"name":"ok","type":"bool"}}]}}"#
        ))
        .unwrap();
        let import = dir.join("import");
        let r = prepare(&m, &src, &PrepareOptions {
            import_root: &import,
            expected_base: None,
            expected_result: None,
            oracular_dir: None,
            dry_run: false,
        })
        .unwrap();
        staging_path(&import, &r.stage_id)
    }

    #[test]
    fn export_round_trips_and_detects_tampering() {
        let d = tempfile::tempdir().unwrap();
        let tree = staged(d.path(), "id,name,ok\n2,\"b,c\",false\n1,ann,true\n3,,\n");
        let out = d.path().join("out");
        let proof = export(&tree, &out, ExportFormat::Csv, Some("ab12".into())).unwrap();
        assert_eq!(proof.records, 3);
        let csv = fs::read_to_string(out.join("data.csv")).unwrap();
        assert_eq!(csv.lines().next().unwrap(), "id,name,ok");
        assert!(csv.contains("\"b,c\""));
        verify_export(&out, None).unwrap();
        assert!(verify_export(&out, Some([1u8; 32])).is_err());
        fs::write(out.join("data.csv"), csv.replace("ann", "ANN")).unwrap();
        assert!(verify_export(&out, None).is_err());
    }

    #[test]
    fn jsonl_export_and_non_empty_target_rejected() {
        let d = tempfile::tempdir().unwrap();
        let tree = staged(d.path(), "id,name,ok\n1,ann,true\n");
        let out = d.path().join("out");
        export(&tree, &out, ExportFormat::Jsonl, None).unwrap();
        let line = fs::read_to_string(out.join("data.jsonl")).unwrap();
        assert_eq!(line, "{\"id\":1,\"name\":\"ann\",\"ok\":true}\n");
        assert!(export(&tree, &out, ExportFormat::Jsonl, None).is_err());
    }
}
