pub mod manifest;
pub mod models;
pub mod runtime;

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use eyre::{ensure, eyre, Result};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

pub const MAX_BYTES: u64 = 1024 * 1024;
pub const CORE: &[&str] = &[
    "scripts/run-merge-recovery-soak.sh",
    "scripts/bench/casper-soak.sh",
    "scripts/casper-soak/Cargo.toml",
    "scripts/casper-soak/src/lib.rs",
    "scripts/casper-soak/src/main.rs",
    "scripts/casper-soak/src/manifest.rs",
    "scripts/casper-soak/src/models.rs",
    "scripts/casper-soak/src/runtime.rs",
    "scripts/bench/write-soak-summary.sh",
    "scripts/bench/collect-soak-metrics.sh",
    "scripts/bench/soak-metrics.json",
];

pub fn text(value: &Value) -> Result<&str> {
    value.as_str().ok_or_else(|| eyre!("A string is required."))
}
pub fn number(value: &Value) -> Result<u64> {
    value
        .as_u64()
        .ok_or_else(|| eyre!("An unsigned integer is required."))
}
pub fn array(value: &Value) -> Result<&Vec<Value>> {
    value
        .as_array()
        .ok_or_else(|| eyre!("An array is required."))
}
pub fn object(value: &Value) -> Result<&Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| eyre!("An object is required."))
}
pub fn hash(data: &[u8]) -> String { format!("{:x}", Sha256::digest(data)) }
pub fn encoded(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
pub fn regular(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() <= limit,
        "The input is not a bounded regular file."
    );
    let mut data = Vec::new();
    file.take(limit + 1).read_to_end(&mut data)?;
    ensure!(data.len() as u64 <= limit, "The input exceeds its bound.");
    Ok(data)
}
pub fn file_hash(path: &Path) -> Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() <= 1024 * MAX_BYTES,
        "The executable input is not bounded."
    );
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        total += size as u64;
        ensure!(
            total <= 1024 * MAX_BYTES,
            "The executable input grew beyond its bound."
        );
        hash.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn relative(root: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.contains('\\'),
        "The relative path is invalid."
    );
    let path = Path::new(name);
    ensure!(
        path.components()
            .all(|part| matches!(part, Component::Normal(_))),
        "The path escapes its root."
    );
    ensure!(
        path.components()
            .map(|p| p.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
            == name,
        "The path is not canonical."
    );
    let mut current = root.to_path_buf();
    ensure!(!current.is_symlink(), "The root is a symbolic link.");
    for part in path.components() {
        current.push(part);
        ensure!(!current.is_symlink(), "The path contains a symbolic link.");
    }
    Ok(current)
}
pub fn exclusive(path: &Path, bytes: &[u8], readonly: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("The capture has no parent."))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    if readonly {
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o444))?;
    }
    temporary.as_file().sync_all()?;
    fs::hard_link(temporary.path(), path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn record(path: &Path) -> Result<Value> { parse(&regular(path, MAX_BYTES)?) }
pub fn artifact(root: &Path, reference: &Value) -> Result<Vec<u8>> {
    let path = relative(root, text(&reference["path"])?)?;
    let data = regular(&path, MAX_BYTES)?;
    ensure!(
        number(&reference["bytes"])? == data.len() as u64
            && text(&reference["sha256"])? == hash(&data),
        "The artifact identity differs."
    );
    Ok(data)
}
pub fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        ensure!(!path.is_symlink(), "The capture contains a symbolic link.");
        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        } else {
            result.push(path);
        }
        ensure!(
            pending.len() + result.len() <= 10000,
            "The capture exceeds its file bound."
        );
    }
    result.sort();
    Ok(result)
}

struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                Number::from_f64(v)
                    .map(|n| Strict(Value::Number(n)))
                    .ok_or_else(|| E::custom("A finite number is required."))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(Strict(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(Strict(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut values = Map::new();
                while let Some((key, Strict(value))) = map.next_entry::<String, Strict>()? {
                    if values.insert(key, value).is_some() {
                        return Err(de::Error::custom("A JSON key is duplicated."));
                    }
                }
                Ok(Strict(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}
pub fn parse(bytes: &[u8]) -> Result<Value> {
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    let Strict(value) = Strict::deserialize(&mut parser)?;
    parser.end()?;
    object(&value)?;
    Ok(value)
}
