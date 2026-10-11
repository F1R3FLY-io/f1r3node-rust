use std::collections::BTreeSet;
use std::path::Path;

use rholang::rust::interpreter::io::bulk::{blake2b, parse_hex_root, validate_namespace};
use serde::{Deserialize, Serialize};

use crate::error::BulkError;

pub const MANIFEST_VERSION: u32 = 1;
pub const MAX_FANOUT: u8 = 2;
pub const MAX_FILE_BYTES_CEILING: u64 = 32 * 1024 * 1024;
pub const DEFAULT_MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const OP_FIELD: &str = "_op";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat {
    Csv,
    Jsonl,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize
)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Int,
    Decimal,
    Bool,
    Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Consensus,
    Oracular,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    pub format: SourceFormat,
    pub locations: Vec<String>,
    pub root: String,
    #[serde(default = "default_delimiter")]
    pub delimiter: char,
}

fn default_delimiter() -> char { ',' }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    pub name: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(rename = "type")]
    pub ty: FieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default = "default_route")]
    pub route: Route,
}

fn default_route() -> Route { Route::Consensus }

impl FieldSpec {
    pub fn source_name(&self) -> &str { self.from.as_deref().unwrap_or(&self.name) }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutSpec {
    #[serde(default = "default_fanout")]
    pub fanout: u8,
    #[serde(rename = "maxFileBytes", default = "default_max_file_bytes")]
    pub max_file_bytes: u64,
}

fn default_fanout() -> u8 { 2 }

fn default_max_file_bytes() -> u64 { DEFAULT_MAX_FILE_BYTES }

impl Default for LayoutSpec {
    fn default() -> Self {
        LayoutSpec {
            fanout: default_fanout(),
            max_file_bytes: default_max_file_bytes(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportManifest {
    pub version: u32,
    pub namespace: String,
    pub source: SourceSpec,
    pub transform: String,
    pub key: Vec<String>,
    pub fields: Vec<FieldSpec>,
    #[serde(default)]
    pub layout: LayoutSpec,
    #[serde(default)]
    pub delta: bool,
}

impl ImportManifest {
    pub fn load(path: &Path) -> Result<Self, BulkError> {
        let text = std::fs::read_to_string(path).map_err(|e| BulkError::io(path, e))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, BulkError> {
        let m: ImportManifest =
            serde_json::from_str(text).map_err(|e| BulkError::Manifest(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("manifest serializes")
    }

    pub fn hash(&self) -> [u8; 32] {
        let mut buf = b"f1r3fly/bulk-io/manifest/v1".to_vec();
        buf.extend_from_slice(&self.canonical_bytes());
        blake2b(&buf)
    }

    pub fn source_root(&self) -> [u8; 32] {
        parse_hex_root(&self.source.root).expect("validated source root")
    }

    pub fn consensus_fields(&self) -> impl Iterator<Item = &FieldSpec> {
        self.fields.iter().filter(|f| f.route == Route::Consensus)
    }

    pub fn oracular_fields(&self) -> impl Iterator<Item = &FieldSpec> {
        self.fields.iter().filter(|f| f.route == Route::Oracular)
    }

    pub fn field(&self, name: &str) -> Option<&FieldSpec> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn validate(&self) -> Result<(), BulkError> {
        let bad = |m: &str| Err(BulkError::Manifest(m.to_string()));
        if self.version != MANIFEST_VERSION {
            return bad("unsupported manifest version");
        }
        if validate_namespace(&self.namespace).is_err() {
            return bad("invalid namespace");
        }
        if parse_hex_root(&self.source.root).is_none() {
            return bad("source.root must be 64 lowercase hex characters");
        }
        if self.source.locations.is_empty() {
            return bad("source.locations must not be empty");
        }
        for loc in &self.source.locations {
            if !(loc.starts_with("file://") || loc.starts_with("https://")) {
                return bad("source locations must use file:// or https://");
            }
        }
        if !self.source.delimiter.is_ascii() || self.source.delimiter == '"' {
            return bad("source.delimiter must be one ASCII character other than a quote");
        }
        if crate::transform::transform_id(&self.transform).is_none() {
            return bad("unknown transform");
        }
        if self.fields.is_empty() {
            return bad("fields must not be empty");
        }
        let mut names = BTreeSet::new();
        for f in &self.fields {
            if f.name.is_empty()
                || f.name == OP_FIELD
                || f.name.len() > 128
                || !f
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return bad("field names must be 1-128 characters of [A-Za-z0-9_] and not _op");
            }
            if !names.insert(f.name.as_str()) {
                return bad("duplicate field name");
            }
        }
        if self.key.is_empty() {
            return bad("key must name at least one field");
        }
        let mut key_names = BTreeSet::new();
        for k in &self.key {
            match self.field(k) {
                Some(f) if f.route == Route::Consensus && f.required => {}
                Some(_) => return bad("key fields must be required and routed to consensus"),
                None => return bad("key names an unknown field"),
            }
            if !key_names.insert(k.as_str()) {
                return bad("duplicate key field");
            }
        }
        if self.layout.fanout > MAX_FANOUT {
            return bad("layout.fanout must be 0, 1 or 2");
        }
        if self.layout.max_file_bytes < 4096 || self.layout.max_file_bytes > MAX_FILE_BYTES_CEILING
        {
            return bad("layout.maxFileBytes must be between 4096 and 33554432");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> String {
        format!(
            r#"{{
  "version": 1,
  "namespace": "demo/people",
  "source": {{"format": "csv", "locations": ["file:///tmp/x.csv"], "root": "{}"}},
  "transform": "tabular/1",
  "key": ["id"],
  "fields": [
    {{"name": "id", "type": "int", "required": true}},
    {{"name": "name", "type": "string"}},
    {{"name": "email", "type": "string", "route": "oracular"}}
  ]
}}"#,
            "ab".repeat(32)
        )
    }

    #[test]
    fn parses_and_hashes_stably() {
        let a = ImportManifest::parse(&sample()).unwrap();
        let b = ImportManifest::parse(&sample().replace("  ", " ")).unwrap();
        assert_eq!(a.hash(), b.hash());
        assert_eq!(a.layout, LayoutSpec::default());
    }

    #[test]
    fn rejects_unknown_fields_and_bad_keys() {
        assert!(ImportManifest::parse(
            &sample().replace("\"version\": 1", "\"version\": 1, \"extra\": 1")
        )
        .is_err());
        assert!(ImportManifest::parse(&sample().replace("[\"id\"]", "[\"email\"]")).is_err());
        assert!(ImportManifest::parse(&sample().replace("[\"id\"]", "[\"nope\"]")).is_err());
        assert!(ImportManifest::parse(&sample().replace("file:///tmp/x.csv", "http://x")).is_err());
        assert!(ImportManifest::parse(&sample().replace("demo/people", "../x")).is_err());
        assert!(ImportManifest::parse(&sample().replace("tabular/1", "wasm/1")).is_err());
    }

    #[test]
    fn hash_changes_with_content() {
        let a = ImportManifest::parse(&sample()).unwrap();
        let b = ImportManifest::parse(&sample().replace("demo/people", "demo/other")).unwrap();
        assert_ne!(a.hash(), b.hash());
    }
}
