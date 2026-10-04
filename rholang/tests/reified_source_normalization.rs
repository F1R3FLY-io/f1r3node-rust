use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::{fs, thread};

use crypto::rust::hash::blake2b256::Blake2b256;
use prost::Message;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use serde_json::{Map, Value};

const CORPUS_PATH: &str = "test-vectors/ordinary-source-normalization-v1.json";
const CAPTURE_ENV: &str = "RRS_CAPTURE_SOURCES_OUT";
const CAPTURE_REVISION_ENV: &str = "RRS_CAPTURE_REVISION";
const COMPILE_STACK_BYTES: usize = 256 * 1024 * 1024;
const TEMPLATE_VALUES: [(&str, &str); 2] =
    [("initialBonds", "{}"), ("posMultiSigPublicKeys", "[]")];
const DEFAULT_TEMPLATE_VALUE: &str = "0";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rholang crate has a workspace parent")
        .to_path_buf()
}

fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("read a source directory")
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("source paths are UTF-8");
        match (path.is_dir(), path.extension().and_then(|ext| ext.to_str())) {
            (true, _) if name == "target" || name.starts_with('.') => {}
            (true, _) => collect_sources(&path, out),
            (false, Some("rho" | "rhox")) => out.push(path),
            (false, _) => {}
        }
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("source path lies under the workspace root")
        .to_str()
        .expect("source paths are UTF-8")
        .to_string()
}

fn expand_template(source: &str) -> String {
    let mut expanded = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("$$") {
        let after = &rest[start + 2..];
        match after.find("$$") {
            Some(end)
                if end > 0
                    && after[..end]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_') =>
            {
                let name = &after[..end];
                let value = TEMPLATE_VALUES
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map_or(DEFAULT_TEMPLATE_VALUE, |(_, value)| *value);
                expanded.push_str(&rest[..start]);
                expanded.push_str(value);
                rest = &after[end + 2..];
            }
            _ => {
                expanded.push_str(&rest[..start + 2]);
                rest = after;
            }
        }
    }
    expanded.push_str(rest);
    expanded
}

fn blake2b256_hex(bytes: Vec<u8>) -> String { hex::encode(Blake2b256::hash(bytes)) }

fn normalize_on_large_stack(text: String) -> Value {
    thread::Builder::new()
        .stack_size(COMPILE_STACK_BYTES)
        .spawn(move || {
            match panic::catch_unwind(AssertUnwindSafe(|| Compiler::source_to_adt(&text))) {
                Ok(Ok(par)) => {
                    let mut outcome = Map::with_capacity(3);
                    outcome.insert("outcome".to_string(), Value::from("ok"));
                    outcome.insert(
                        "prost_blake2b256".to_string(),
                        Value::from(blake2b256_hex(par.encode_to_vec())),
                    );
                    outcome.insert(
                        "stable_hash".to_string(),
                        Value::from(hex::encode(stable_hash_provider::hash(&par).bytes())),
                    );
                    outcome
                }
                Ok(Err(_)) => {
                    let mut outcome = Map::with_capacity(1);
                    outcome.insert("outcome".to_string(), Value::from("error"));
                    outcome
                }
                Err(_) => {
                    let mut outcome = Map::with_capacity(1);
                    outcome.insert("outcome".to_string(), Value::from("panic"));
                    outcome
                }
            }
        })
        .expect("spawn the normalization thread")
        .join()
        .map(Value::Object)
        .expect("join the normalization thread")
}

fn observe(relative: &str, source: &str) -> Value {
    let text = match relative.ends_with(".rhox") {
        true => expand_template(source),
        false => source.to_string(),
    };
    let mut entry = match normalize_on_large_stack(text) {
        Value::Object(map) => map,
        other => panic!("normalization produced a non-object observation: {other}"),
    };
    entry.insert(
        "source_blake2b256".to_string(),
        Value::from(blake2b256_hex(source.as_bytes().to_vec())),
    );
    Value::Object(entry)
}

fn capture(root: &Path, out_dir: &Path) {
    let mut sources = Vec::with_capacity(256);
    collect_sources(root, &mut sources);
    let mut entries = BTreeMap::new();
    for path in &sources {
        let relative = relative_path(root, path);
        let source = fs::read_to_string(path).expect("read a Rholang source file");
        entries.insert(relative.clone(), observe(&relative, &source));
    }
    let mut corpus = Map::with_capacity(5);
    corpus.insert(
        "name".to_string(),
        Value::from("ordinary_source_normalization_v1"),
    );
    corpus.insert(
        "captured_at_revision".to_string(),
        Value::from(
            std::env::var(CAPTURE_REVISION_ENV).expect("set RRS_CAPTURE_REVISION when capturing"),
        ),
    );
    corpus.insert(
        "rule".to_string(),
        Value::from(
            "every entry with outcome ok must normalize to identical prost and stable hashes under the current parser",
        ),
    );
    corpus.insert(
        "template_values".to_string(),
        Value::from(
            TEMPLATE_VALUES
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .chain(std::iter::once(format!("*={DEFAULT_TEMPLATE_VALUE}")))
                .collect::<Vec<_>>(),
        ),
    );
    corpus.insert(
        "entries".to_string(),
        Value::Object(entries.into_iter().collect()),
    );
    fs::create_dir_all(out_dir).expect("create the capture directory");
    let rendered = serde_json::to_string_pretty(&Value::Object(corpus)).expect("render the corpus");
    fs::write(
        out_dir.join("ordinary-source-normalization-v1.json"),
        format!("{rendered}\n"),
    )
    .expect("write the captured corpus");
}

fn verify(root: &Path) {
    let corpus_text =
        fs::read_to_string(root.join(CORPUS_PATH)).expect("read the source-normalization corpus");
    let corpus: Value = serde_json::from_str(&corpus_text).expect("parse the corpus JSON");
    let entries = corpus
        .get("entries")
        .and_then(Value::as_object)
        .expect("the corpus has an entries object");
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for (relative, expected) in entries {
        let path = root.join(relative);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                failures.push(format!(
                    "{relative}: cannot read the HEAD source file: {error}"
                ));
                continue;
            }
        };
        let actual = observe(relative, &source);
        match (
            expected.get("source_blake2b256"),
            actual.get("source_blake2b256"),
        ) {
            (Some(want), Some(got)) if want == got => {}
            _ => {
                failures.push(format!(
                    "{relative}: the source text differs from the captured HEAD text"
                ));
                continue;
            }
        }
        match expected.get("outcome").and_then(Value::as_str) {
            Some("ok") => {
                checked += 1;
                if &actual != expected {
                    failures.push(format!("{relative}: expected {expected}, actual {actual}"));
                }
            }
            Some(_) => {}
            None => failures.push(format!("{relative}: the corpus entry has no outcome")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} ok-at-HEAD sources changed:\n{}",
        failures.len(),
        checked,
        failures.join("\n")
    );
    assert!(
        checked > 0,
        "the corpus holds no source that normalized at HEAD"
    );
}

#[test]
fn ordinary_source_normalization_unchanged() {
    let root = workspace_root();
    match std::env::var_os(CAPTURE_ENV) {
        Some(out_dir) => capture(&root, Path::new(&out_dir)),
        None => verify(&root),
    }
}
