use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
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
// `resourcePolicyGetter` takes the value of the default genesis, which sets no
// resource policy (standard_deploys.rs, token_metadata_with_policy).
const TEMPLATE_VALUES: [(&str, &str); 3] = [
    ("initialBonds", "{}"),
    ("posMultiSigPublicKeys", "[]"),
    ("resourcePolicyGetter", ""),
];
const DEFAULT_TEMPLATE_VALUE: &str = "0";
const FLOAT_CORPUS_PATH: &str = "test-vectors/ordinary-float-forms-v1.json";
const FLOAT_CORPUS_NAME: &str = "ordinary_float_forms_v1";
const FLOAT_CAPTURE_ENV: &str = "RRS_CAPTURE_FLOATS_OUT";
const CAPTURE_COMMAND_ENV: &str = "RRS_CAPTURE_COMMAND";
const PARSER_PACKAGE: &str = "rholang-parser";
const FLOAT_RULE: &str = "Each form with outcome ok must normalize to the same prost and stable hashes. A rejected form can become accepted, which is a conservative extension. An accepted form must never become rejected.";
const FLOAT_PROVENANCE_KEYS: [&str; 5] = [
    "captured_at_revision",
    "capture_command",
    "rustc_version",
    "parser_source",
    "parser_revision",
];
const FLOAT_FORMS: [(&str, &str); 15] = [
    ("trailing_dot", r#"@"rrs-float"!(5.)"#),
    ("dot_exponent", r#"@"rrs-float"!(5.e3)"#),
    ("decimal", r#"@"rrs-float"!(5.25)"#),
    ("exponent", r#"@"rrs-float"!(5e3)"#),
    ("neg_exponent", r#"@"rrs-float"!(5E-3)"#),
    ("leading_dot", r#"@"rrs-float"!(.5)"#),
    ("negative", r#"@"rrs-float"!(-2.5)"#),
    ("suffixed", r#"@"rrs-float"!(1.5f32)"#),
    ("list", r#"@"rrs-float"!([1., .25, 3.e-1])"#),
    ("arithmetic", r#"@"rrs-float"!(1.5 + 2.)"#),
    ("sync_dot", r#"@"rrs-float-x"!?(1).5"#),
    ("sync_dot_new", r#"new x in { x!?(1).5 }"#),
    ("string", r#"@"rrs-float"!("5.5")"#),
    ("comment", r#"@"rrs-float"!(1) // 5.5"#),
    ("backtick", r#"new r(`rho:registry:ops:1.0.0`) in { Nil }"#),
];

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

// The variant name of the error that rejects `text`. A derived `Debug` text
// starts with the variant name.
fn rejection_variant_on_large_stack(text: String) -> String {
    thread::Builder::new()
        .stack_size(COMPILE_STACK_BYTES)
        .spawn(move || match Compiler::source_to_adt(&text) {
            Ok(_) => panic!("a rejected form normalized on the second attempt: {text}"),
            Err(error) => format!("{error:?}")
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default()
                .to_string(),
        })
        .expect("spawn the rejection thread")
        .join()
        .expect("join the rejection thread")
}

fn float_form_outcome(source: &str) -> Map<String, Value> {
    let mut outcome = match normalize_on_large_stack(source.to_string()) {
        Value::Object(map) => map,
        other => panic!("normalization produced a non-object observation: {other}"),
    };
    if let Some("error") = outcome.get("outcome").and_then(Value::as_str) {
        let variant = rejection_variant_on_large_stack(source.to_string());
        outcome.insert("error_variant".to_string(), Value::from(variant));
    }
    outcome
}

fn rustc_version(root: &Path) -> String {
    let output = Command::new("rustc")
        .arg("-V")
        .current_dir(root)
        .output()
        .expect("run rustc -V");
    assert!(
        output.status.success(),
        "rustc -V failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("rustc -V prints UTF-8")
        .trim()
        .to_string()
}

// The git source and revision of the parser that Cargo.lock pins. A path
// source, as a `[patch]` gives, has no `source` line and fails the capture.
fn locked_parser_source(root: &Path) -> (String, String) {
    let lock = fs::read_to_string(root.join("Cargo.lock")).expect("read Cargo.lock");
    let name_line = format!("name = \"{PARSER_PACKAGE}\"");
    let mut sources = Vec::with_capacity(1);
    let mut in_parser = false;
    for line in lock.lines().map(str::trim) {
        match (line, in_parser) {
            ("[[package]]", _) => in_parser = false,
            (line, _) if line == name_line => in_parser = true,
            (line, true) => {
                if let Some(source) = line
                    .strip_prefix("source = \"")
                    .and_then(|rest| rest.strip_suffix('"'))
                {
                    sources.push(source.to_string());
                    in_parser = false;
                }
            }
            _ => {}
        }
    }
    let source = match sources.as_slice() {
        [source] if source.starts_with("git+") => source.clone(),
        other => panic!("Cargo.lock must pin one {PARSER_PACKAGE} git source, found {other:?}"),
    };
    match source.split_once('#') {
        Some((_, revision)) if !revision.is_empty() => {
            let revision = revision.to_string();
            (source, revision)
        }
        _ => panic!("the locked {PARSER_PACKAGE} source has no revision: {source}"),
    }
}

fn capture_float_forms(root: &Path, out_dir: &Path) {
    let target = root.join("target");
    let canonical_target = target.canonicalize().unwrap_or_else(|_| target.clone());
    let inside_target = out_dir.is_absolute()
        && out_dir
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
        && (out_dir.starts_with(&target) || out_dir.starts_with(&canonical_target));
    assert!(
        inside_target,
        "{FLOAT_CAPTURE_ENV} must be an absolute path under {}, got {}",
        target.display(),
        out_dir.display()
    );
    let (parser_source, parser_revision) = locked_parser_source(root);
    let forms: Vec<Value> = FLOAT_FORMS
        .iter()
        .map(|(name, source)| {
            let mut entry = Map::with_capacity(6);
            entry.insert("name".to_string(), Value::from(*name));
            entry.insert("source".to_string(), Value::from(*source));
            entry.extend(float_form_outcome(source));
            Value::Object(entry)
        })
        .collect();
    let mut corpus = Map::with_capacity(8);
    corpus.insert("name".to_string(), Value::from(FLOAT_CORPUS_NAME));
    corpus.insert(
        "captured_at_revision".to_string(),
        Value::from(
            std::env::var(CAPTURE_REVISION_ENV).expect("set RRS_CAPTURE_REVISION when capturing"),
        ),
    );
    corpus.insert(
        "capture_command".to_string(),
        Value::from(
            std::env::var(CAPTURE_COMMAND_ENV).expect("set RRS_CAPTURE_COMMAND when capturing"),
        ),
    );
    corpus.insert(
        "rustc_version".to_string(),
        Value::from(rustc_version(root)),
    );
    corpus.insert("parser_source".to_string(), Value::from(parser_source));
    corpus.insert("parser_revision".to_string(), Value::from(parser_revision));
    corpus.insert("rule".to_string(), Value::from(FLOAT_RULE));
    corpus.insert("forms".to_string(), Value::Array(forms));
    fs::create_dir_all(out_dir).expect("create the capture directory");
    let file_name = Path::new(FLOAT_CORPUS_PATH)
        .file_name()
        .expect("the float-form corpus path names a file");
    let rendered = serde_json::to_string_pretty(&Value::Object(corpus)).expect("render the corpus");
    fs::write(out_dir.join(file_name), format!("{rendered}\n"))
        .expect("write the captured float-form corpus");
}

fn verify_float_forms(root: &Path) {
    let text = fs::read_to_string(root.join(FLOAT_CORPUS_PATH)).unwrap_or_else(|error| {
        panic!("read {FLOAT_CORPUS_PATH}: {error}; capture it with {FLOAT_CAPTURE_ENV}")
    });
    let corpus: Value = serde_json::from_str(&text).expect("parse the float-form corpus");
    let mut failures = Vec::new();
    match corpus.get("name").and_then(Value::as_str) {
        Some(FLOAT_CORPUS_NAME) => {}
        other => failures.push(format!(
            "the corpus name is {other:?}, but it must be {FLOAT_CORPUS_NAME}"
        )),
    }
    for key in FLOAT_PROVENANCE_KEYS {
        match corpus.get(key).and_then(Value::as_str) {
            Some(value) if !value.is_empty() => {}
            _ => failures.push(format!("the corpus has no {key}")),
        }
    }
    let forms = corpus
        .get("forms")
        .and_then(Value::as_array)
        .expect("the corpus has a forms array");
    let listed: Vec<(&str, &str)> = forms
        .iter()
        .map(|form| {
            (
                form.get("name").and_then(Value::as_str).unwrap_or_default(),
                form.get("source")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
        })
        .collect();
    if listed != FLOAT_FORMS {
        failures.push(format!(
            "the corpus must hold exactly the fixed forms in order, but it holds {listed:?}"
        ));
    }
    let mut accepted = 0usize;
    for ((name, source), expected) in FLOAT_FORMS.iter().zip(forms) {
        match expected.get("outcome").and_then(Value::as_str) {
            Some("ok") => {
                accepted += 1;
                let actual = float_form_outcome(source);
                let unchanged = ["outcome", "prost_blake2b256", "stable_hash"]
                    .iter()
                    .all(|key| {
                        expected.get(*key).is_some() && actual.get(*key) == expected.get(*key)
                    });
                if !unchanged {
                    failures.push(format!(
                        "{name}: HEAD accepts this form, so it must keep its hashes: expected {expected}, actual {}",
                        Value::Object(actual)
                    ));
                }
            }
            Some("error") => match expected.get("error_variant").and_then(Value::as_str) {
                Some(variant) if !variant.is_empty() => {}
                _ => failures.push(format!("{name}: the rejected form has no error variant")),
            },
            Some("panic") => {}
            other => failures.push(format!("{name}: the corpus outcome {other:?} is not known")),
        }
    }
    assert!(
        failures.is_empty(),
        "the float forms differ from {FLOAT_CORPUS_PATH}:\n{}",
        failures.join("\n")
    );
    assert!(
        accepted > 0,
        "the corpus holds no float form that HEAD accepts"
    );
}

#[test]
fn ordinary_float_forms_unchanged() {
    let root = workspace_root();
    match std::env::var_os(FLOAT_CAPTURE_ENV) {
        Some(out_dir) => capture_float_forms(&root, Path::new(&out_dir)),
        None => verify_float_forms(&root),
    }
}
