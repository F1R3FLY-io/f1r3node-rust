use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::error::BulkError;
use crate::manifest::{SourceFormat, SourceSpec};
use crate::value::RawValue;

pub type RawRecord = BTreeMap<String, RawValue>;

pub struct SourceRow {
    pub line: u64,
    pub row: Result<RawRecord, String>,
}

pub const ADAPTER_IDS: &[(&str, SourceFormat)] = &[
    ("csv/1", SourceFormat::Csv),
    ("jsonl/1", SourceFormat::Jsonl),
];

pub fn adapter_id(format: SourceFormat) -> &'static str {
    ADAPTER_IDS
        .iter()
        .find(|(_, f)| *f == format)
        .map(|(id, _)| *id)
        .expect("every format has an adapter")
}

pub fn read_rows(
    spec: &SourceSpec,
    path: &Path,
    mut sink: impl FnMut(SourceRow) -> Result<(), BulkError>,
) -> Result<(), BulkError> {
    match spec.format {
        SourceFormat::Csv => read_csv(spec.delimiter as u8, path, &mut sink),
        SourceFormat::Jsonl => read_jsonl(path, &mut sink),
    }
}

fn read_csv(
    delimiter: u8,
    path: &Path,
    sink: &mut impl FnMut(SourceRow) -> Result<(), BulkError>,
) -> Result<(), BulkError> {
    let f = File::open(path).map_err(|e| BulkError::io(path, e))?;
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .flexible(true)
        .from_reader(f);
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| BulkError::Source(format!("header: {e}")))?
        .iter()
        .map(str::to_string)
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for h in &headers {
        if !seen.insert(h.as_str()) {
            return Err(BulkError::Source(format!("duplicate CSV header {h:?}")));
        }
    }
    for rec in rdr.records() {
        match rec {
            Ok(r) => {
                let line = r.position().map(|p| p.line()).unwrap_or(0);
                let row = if r.len() != headers.len() {
                    Err(format!(
                        "expected {} fields, found {}",
                        headers.len(),
                        r.len()
                    ))
                } else {
                    Ok(headers
                        .iter()
                        .cloned()
                        .zip(r.iter().map(|v| RawValue::Text(v.to_string())))
                        .collect())
                };
                sink(SourceRow { line, row })?;
            }
            Err(e) => {
                let line = e.position().map(|p| p.line()).unwrap_or(0);
                if matches!(e.kind(), csv::ErrorKind::Io(_)) {
                    return Err(BulkError::Source(e.to_string()));
                }
                sink(SourceRow {
                    line,
                    row: Err(format!("malformed CSV row: {e}")),
                })?;
            }
        }
    }
    Ok(())
}

fn read_jsonl(
    path: &Path,
    sink: &mut impl FnMut(SourceRow) -> Result<(), BulkError>,
) -> Result<(), BulkError> {
    let f = File::open(path).map_err(|e| BulkError::io(path, e))?;
    for (i, line) in BufReader::new(f).split(b'\n').enumerate() {
        let line_no = (i + 1) as u64;
        let bytes = line.map_err(|e| BulkError::io(path, e))?;
        if bytes.is_empty() {
            continue;
        }
        let row = match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(serde_json::Value::Object(map)) => Ok(map
                .into_iter()
                .map(|(k, v)| (k, RawValue::Json(v)))
                .collect()),
            Ok(_) => Err("each line must be a JSON object".to_string()),
            Err(e) => Err(format!("invalid JSON: {e}")),
        };
        sink(SourceRow { line: line_no, row })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(format: SourceFormat) -> SourceSpec {
        SourceSpec {
            format,
            locations: vec!["file:///x".into()],
            root: "00".repeat(32),
            delimiter: ',',
        }
    }

    fn collect(format: SourceFormat, body: &str) -> Vec<(u64, Result<RawRecord, String>)> {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s");
        std::fs::write(&p, body).unwrap();
        let mut out = Vec::new();
        read_rows(&spec(format), &p, |r| {
            out.push((r.line, r.row));
            Ok(())
        })
        .unwrap();
        out
    }

    #[test]
    fn csv_rows_and_short_rows() {
        let rows = collect(SourceFormat::Csv, "id,name\n1,ann\n2\n3,\"b,c\"\n");
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0].1.as_ref().unwrap()["name"],
            RawValue::Text("ann".into())
        );
        assert!(rows[1].1.is_err());
        assert_eq!(
            rows[2].1.as_ref().unwrap()["name"],
            RawValue::Text("b,c".into())
        );
        assert_eq!(rows[1].0, 3);
    }

    #[test]
    fn jsonl_rows_with_errors() {
        let rows = collect(
            SourceFormat::Jsonl,
            "{\"id\":1}\nnot json\n[1]\n\n{\"id\":2}\n",
        );
        assert_eq!(rows.len(), 4);
        assert!(rows[0].1.is_ok());
        assert!(rows[1].1.is_err());
        assert!(rows[2].1.is_err());
        assert_eq!(rows[3].0, 5);
    }

    #[test]
    fn duplicate_csv_header_is_fatal() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s");
        std::fs::write(&p, "a,a\n1,2\n").unwrap();
        assert!(read_rows(&spec(SourceFormat::Csv), &p, |_| Ok(())).is_err());
    }
}
