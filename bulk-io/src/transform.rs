use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use rholang::rust::interpreter::io::bulk::{blake2b, hex_root};

use crate::adapter::{read_rows, RawRecord};
use crate::error::BulkError;
use crate::manifest::{FieldSpec, ImportManifest, Route, OP_FIELD};
use crate::value::{convert, encode_key, encode_record, frame, Record, Value, FILE_MAGIC};

pub const TRANSFORMS: &[&str] = &["tabular/1"];
pub const DATA_DIR: &str = "data";
pub const INDEX_FILE: &str = "INDEX";
pub const PROVENANCE_FILE: &str = "PROVENANCE";
pub const REJECTS_FILE: &str = "REJECTS";
pub const SCHEMA_FILE: &str = "SCHEMA";

pub fn transform_id(name: &str) -> Option<[u8; 32]> {
    if !TRANSFORMS.contains(&name) {
        return None;
    }
    let s = format!(
        "f1r3fly/bulk-io/transform/{name}/{}",
        env!("CARGO_PKG_VERSION")
    );
    Some(blake2b(s.as_bytes()))
}

pub type Partitions = BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>;

#[derive(Debug, Default)]
pub struct TransformOutput {
    pub partitions: Partitions,
    pub oracular: Partitions,
    pub rejects: Vec<(u64, String)>,
    pub rows: u64,
    pub upserts: u64,
    pub deletes: u64,
}

pub fn partition_of(manifest: &ImportManifest, key: &[u8]) -> String {
    let h = hex::encode(blake2b(key));
    match manifest.layout.fanout {
        0 => DATA_DIR.to_string(),
        1 => format!("{DATA_DIR}/{}", &h[0..2]),
        _ => format!("{DATA_DIR}/{}/{}", &h[0..2], &h[2..4]),
    }
}

enum Op {
    Upsert,
    Delete,
}

fn op_of(row: &RawRecord) -> Result<Op, String> {
    use crate::value::RawValue;
    let s = match row.get(OP_FIELD) {
        Some(RawValue::Text(t)) => t.as_str(),
        Some(RawValue::Json(serde_json::Value::String(t))) => t.as_str(),
        _ => return Err(format!("delta rows need {OP_FIELD} = upsert or delete")),
    };
    match s {
        "upsert" => Ok(Op::Upsert),
        "delete" => Ok(Op::Delete),
        other => Err(format!("unknown {OP_FIELD} value {other:?}")),
    }
}

fn typed(row: &RawRecord, f: &FieldSpec) -> Result<Value, String> {
    let v =
        convert(row.get(f.source_name()), f.ty).map_err(|e| format!("field {}: {e}", f.name))?;
    if f.required && v == Value::Null {
        return Err(format!("field {} is required", f.name));
    }
    Ok(v)
}

fn key_of(manifest: &ImportManifest, row: &RawRecord) -> Result<Vec<u8>, String> {
    let mut vals = Vec::with_capacity(manifest.key.len());
    for k in &manifest.key {
        vals.push(typed(row, manifest.field(k).expect("validated key"))?);
    }
    Ok(encode_key(&vals.iter().collect::<Vec<_>>()))
}

fn record_for(manifest: &ImportManifest, row: &RawRecord, route: Route) -> Result<Record, String> {
    let mut rec = Record::new();
    for f in &manifest.fields {
        let keep = f.route == route || (route == Route::Oracular && manifest.key.contains(&f.name));
        if keep {
            rec.insert(f.name.clone(), typed(row, f)?);
        }
    }
    Ok(rec)
}

pub fn run(
    manifest: &ImportManifest,
    source: &Path,
    base: Option<Partitions>,
) -> Result<TransformOutput, BulkError> {
    let mut out = TransformOutput {
        partitions: base.unwrap_or_default(),
        ..Default::default()
    };
    let max_record = manifest.layout.max_file_bytes as usize - FILE_MAGIC.len() - 8;
    let mut seen_keys = std::collections::BTreeSet::new();
    read_rows(&manifest.source, source, |r| {
        out.rows += 1;
        let row = match r.row {
            Ok(row) => row,
            Err(e) => {
                out.rejects.push((r.line, e));
                return Ok(());
            }
        };
        let op = if manifest.delta {
            match op_of(&row) {
                Ok(op) => op,
                Err(e) => {
                    out.rejects.push((r.line, e));
                    return Ok(());
                }
            }
        } else {
            Op::Upsert
        };
        let key = match key_of(manifest, &row) {
            Ok(k) => k,
            Err(e) => {
                out.rejects.push((r.line, e));
                return Ok(());
            }
        };
        if !seen_keys.insert(key.clone()) {
            out.rejects.push((r.line, "duplicate key in source".into()));
            return Ok(());
        }
        let part = partition_of(manifest, &key);
        match op {
            Op::Delete => {
                let removed = out.partitions.get_mut(&part).and_then(|p| p.remove(&key));
                if removed.is_none() {
                    out.rejects.push((r.line, "delete of an absent key".into()));
                    return Ok(());
                }
                if let Some(o) = out.oracular.get_mut(&part) {
                    o.remove(&key);
                }
                out.deletes += 1;
            }
            Op::Upsert => {
                let (cons, orac) = match (
                    record_for(manifest, &row, Route::Consensus),
                    record_for(manifest, &row, Route::Oracular),
                ) {
                    (Ok(c), Ok(o)) => (c, o),
                    (Err(e), _) | (_, Err(e)) => {
                        out.rejects.push((r.line, e));
                        return Ok(());
                    }
                };
                let bytes = encode_record(&cons);
                if bytes.len() + key.len() > max_record {
                    out.rejects
                        .push((r.line, "record exceeds layout.maxFileBytes".into()));
                    return Ok(());
                }
                out.partitions
                    .entry(part.clone())
                    .or_default()
                    .insert(key.clone(), bytes);
                if manifest.oracular_fields().next().is_some() {
                    out.oracular
                        .entry(part)
                        .or_default()
                        .insert(key, encode_record(&orac));
                }
                out.upserts += 1;
            }
        }
        Ok(())
    })?;
    out.partitions.retain(|_, p| !p.is_empty());
    out.rejects.sort();
    Ok(out)
}

fn write_file(p: &Path, bytes: &[u8]) -> Result<(), BulkError> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| BulkError::io(parent, e))?;
    }
    fs::write(p, bytes).map_err(|e| BulkError::io(p, e))
}

pub fn write_partitions(
    dir: &Path,
    parts: &Partitions,
    max_file_bytes: u64,
) -> Result<u64, BulkError> {
    let mut files = 0u64;
    for (part, records) in parts {
        let pdir = dir.join(part);
        let mut index = String::new();
        let mut n = 0usize;
        let mut buf = FILE_MAGIC.to_vec();
        let mut count = 0u64;
        let mut first: Option<Vec<u8>> = None;
        let mut last: Vec<u8> = Vec::new();
        let flush = |n: usize,
                     buf: &mut Vec<u8>,
                     count: u64,
                     first: &Option<Vec<u8>>,
                     last: &[u8],
                     index: &mut String|
         -> Result<(), BulkError> {
            let name = format!("part-{n:05}.rec");
            write_file(&pdir.join(&name), buf)?;
            index.push_str(&format!(
                "{name}\t{count}\t{}\t{}\n",
                hex::encode(first.as_deref().unwrap_or_default()),
                hex::encode(last)
            ));
            Ok(())
        };
        for (k, r) in records {
            let mut frame_bytes = Vec::with_capacity(k.len() + r.len() + 8);
            frame(k, r, &mut frame_bytes);
            if count > 0 && (buf.len() + frame_bytes.len()) as u64 > max_file_bytes {
                flush(n, &mut buf, count, &first, &last, &mut index)?;
                files += 1;
                n += 1;
                buf = FILE_MAGIC.to_vec();
                count = 0;
                first = None;
            }
            if first.is_none() {
                first = Some(k.clone());
            }
            last = k.clone();
            buf.extend_from_slice(&frame_bytes);
            count += 1;
        }
        if count > 0 {
            flush(n, &mut buf, count, &first, &last, &mut index)?;
            files += 1;
        }
        write_file(&pdir.join(INDEX_FILE), index.as_bytes())?;
    }
    Ok(files)
}

pub fn read_partitions(dir: &Path) -> Result<Partitions, BulkError> {
    let mut parts = Partitions::new();
    let data = dir.join(DATA_DIR);
    if !data.exists() {
        return Ok(parts);
    }
    let entries = rholang::rust::interpreter::io::bulk::scan_tree(dir)?;
    for e in entries {
        if !e.rel.starts_with(&format!("{DATA_DIR}/")) || !e.rel.ends_with(".rec") {
            continue;
        }
        let part = e
            .rel
            .rsplit_once('/')
            .map(|(p, _)| p.to_string())
            .unwrap_or_default();
        let bytes =
            fs::read(dir.join(&e.rel)).map_err(|err| BulkError::io(&dir.join(&e.rel), err))?;
        let slot = parts.entry(part).or_default();
        for (k, r) in crate::value::unframe_file(&bytes)? {
            if slot.insert(k, r).is_some() {
                return Err(BulkError::Record(format!("duplicate key in {}", e.rel)));
            }
        }
    }
    Ok(parts)
}

pub fn render_rejects(rejects: &[(u64, String)]) -> String {
    let mut s = String::new();
    for (line, reason) in rejects {
        let clean: String = reason
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        s.push_str(&format!("{line}\t{clean}\n"));
    }
    s
}

pub fn provenance(fields: BTreeMap<&str, serde_json::Value>) -> Vec<u8> {
    let mut v = serde_json::to_vec_pretty(&fields).expect("provenance serializes");
    v.push(b'\n');
    v
}

pub fn hex32(b: &[u8; 32]) -> String { hex_root(b) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ImportManifest;

    fn manifest(delta: bool) -> ImportManifest {
        ImportManifest::parse(&format!(
            r#"{{"version":1,"namespace":"t/n","source":{{"format":"csv","locations":["file:///x"],"root":"{}"}},
            "transform":"tabular/1","key":["id"],"delta":{delta},
            "fields":[{{"name":"id","type":"int","required":true}},{{"name":"v","type":"string"}},{{"name":"secret","type":"string","route":"oracular"}},{{"name":"junk","type":"string","route":"drop"}}],
            "layout":{{"fanout":1,"maxFileBytes":4096}}}}"#,
            "00".repeat(32)
        ))
        .unwrap()
    }

    fn src(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.csv");
        fs::write(&p, body).unwrap();
        (d, p)
    }

    #[test]
    fn transform_routes_rejects_and_dedups() {
        let (_d, p) =
            src("id,v,secret,junk\n1,a,s1,j\n2,b,s2,j\nx,c,s3,j\n1,dup,s4,j\n,missing,,\n");
        let m = manifest(false);
        let out = run(&m, &p, None).unwrap();
        assert_eq!(out.rows, 5);
        assert_eq!(out.upserts, 2);
        assert_eq!(out.rejects.len(), 3);
        let all: Vec<_> = out.partitions.values().flat_map(|p| p.values()).collect();
        assert_eq!(all.len(), 2);
        for r in all {
            let rec = crate::value::decode_record(r).unwrap();
            assert!(
                rec.contains_key("v") && !rec.contains_key("secret") && !rec.contains_key("junk")
            );
        }
        let orac: Vec<_> = out.oracular.values().flat_map(|p| p.values()).collect();
        let rec = crate::value::decode_record(orac[0]).unwrap();
        assert!(rec.contains_key("secret") && rec.contains_key("id") && !rec.contains_key("v"));
    }

    #[test]
    fn transform_is_order_independent_for_output_layout() {
        let m = manifest(false);
        let (_a, pa) = src("id,v,secret,junk\n1,a,,\n2,b,,\n3,c,,\n");
        let (_b, pb) = src("id,v,secret,junk\n3,c,,\n1,a,,\n2,b,,\n");
        assert_eq!(
            run(&m, &pa, None).unwrap().partitions,
            run(&m, &pb, None).unwrap().partitions
        );
    }

    #[test]
    fn delta_applies_upserts_and_deletes() {
        let m = manifest(false);
        let (_a, pa) = src("id,v,secret,junk\n1,a,,\n2,b,,\n");
        let base = run(&m, &pa, None).unwrap().partitions;
        let md = manifest(true);
        let (_b, pb) = src("_op,id,v,secret,junk\nupsert,2,B,,\ndelete,1,,,\nupsert,3,c,,\ndelete,9,,,\nnoop,4,,,\n");
        let out = run(&md, &pb, Some(base)).unwrap();
        assert_eq!((out.upserts, out.deletes, out.rejects.len()), (2, 1, 2));
        let keys: usize = out.partitions.values().map(|p| p.len()).sum();
        assert_eq!(keys, 2);
    }

    #[test]
    fn write_then_read_partitions_splits_files() {
        let d = tempfile::tempdir().unwrap();
        let m = manifest(false);
        let body: String = std::iter::once("id,v,secret,junk\n".to_string())
            .chain((0..400).map(|i| format!("{i},{},,\n", "x".repeat(60))))
            .collect();
        let (_s, p) = src(&body);
        let out = run(&m, &p, None).unwrap();
        let mut single = Partitions::new();
        single.insert(
            DATA_DIR.to_string(),
            out.partitions.values().flat_map(|p| p.clone()).collect(),
        );
        assert_eq!(single[DATA_DIR].len(), 400);
        let files = write_partitions(d.path(), &single, 4096).unwrap();
        assert!(files > 1, "expected a split, got {files} file");
        assert_eq!(read_partitions(d.path()).unwrap(), single);
        for e in rholang::rust::interpreter::io::bulk::scan_tree(d.path()).unwrap() {
            assert!(e.len <= 4096, "{} is {}", e.rel, e.len);
        }
    }
}
