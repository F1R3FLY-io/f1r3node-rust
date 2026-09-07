use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crypto::rust::hash::sha_256::Sha256Hasher;
use serde::Deserialize;

const NODE_BASE: &str = "6781d1d671cc0b98b9de946b3871bdbb8e7f1280";
const MANIFEST: &str = include_str!("fixtures/f1r3lang_frontend_baseline.json");

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    stage: String,
    public_activation: bool,
    node_base: String,
    snapshots: Vec<Snapshot>,
    source_roots: Vec<String>,
    legacy_reference_files: Vec<String>,
    abis: Vec<Abi>,
    application_routes: Vec<Route>,
    required_feature_gates: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    name: String,
    revision: String,
    kind: String,
    files: Vec<FileDigest>,
    tracked_diff_sha256: Option<String>,
    #[serde(default)]
    modified_paths: Vec<String>,
    #[serde(default)]
    untracked_files: Vec<FileDigest>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDigest {
    path: String,
    sha256: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Abi {
    repository: String,
    path: String,
    name: String,
    value: String,
    source_pattern: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    name: String,
    path: String,
    baseline_path: String,
    target: String,
}

type Roots = BTreeMap<String, PathBuf>;
type Sources = BTreeMap<(String, String), Vec<u8>>;

fn ensure(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn hex_digest(bytes: &[u8]) -> String { hex::encode(Sha256Hasher::hash(bytes.to_vec())) }

fn check_digest(bytes: &[u8], expected: &str) -> Result<(), String> {
    ensure(hex_digest(bytes) == expected, "source digest mismatch")
}

fn relative_path(path: &str) -> Result<(), String> {
    ensure(
        !path.is_empty()
            && Path::new(path)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        format!("expected repository-relative path: {path}"),
    )
}

fn revision(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn manifest() -> Manifest { serde_json::from_str(MANIFEST).expect("valid baseline JSON") }

fn check_contract(manifest: &Manifest) -> Result<(), String> {
    ensure(manifest.schema_version == 1, "unknown manifest schema")?;
    ensure(
        manifest.stage == "admission-contract" && !manifest.public_activation,
        "a baseline contract cannot activate the public frontend",
    )?;
    ensure(manifest.node_base == NODE_BASE, "unapproved node baseline")?;
    ensure(
        manifest.source_roots == ["casper/src", "node/src", "rholang/src"],
        "source coverage roots changed",
    )?;
    let names: BTreeSet<_> = manifest
        .snapshots
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    ensure(
        names == BTreeSet::from(["node", "mettail", "venus", "legacy_parser", "papers"])
            && manifest.snapshots.len() == names.len(),
        "missing, duplicate or unknown source repository",
    )?;
    for snapshot in &manifest.snapshots {
        ensure(revision(&snapshot.revision), "invalid revision")?;
        ensure(!snapshot.files.is_empty(), "empty source snapshot")?;
        let mut paths = BTreeSet::new();
        for file in snapshot.files.iter().chain(&snapshot.untracked_files) {
            relative_path(&file.path)?;
            ensure(paths.insert(&file.path), "duplicate source path")?;
            ensure(
                file.sha256.len() == 64 && file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "invalid SHA-256",
            )?;
        }
        match snapshot.name.as_str() {
            "node" => ensure(
                snapshot.revision == manifest.node_base && snapshot.kind == "revision_sources",
                "node pin or snapshot kind mismatch",
            )?,
            "mettail" => {
                ensure(
                    snapshot.kind == "development_snapshot"
                        && snapshot.tracked_diff_sha256.is_some()
                        && !snapshot.modified_paths.is_empty()
                        && !snapshot.untracked_files.is_empty(),
                    "development work must not be represented as a clean release pin",
                )?;
                for path in &snapshot.modified_paths {
                    relative_path(path)?;
                }
            }
            _ => ensure(snapshot.kind == "revision_sources", "unknown snapshot kind")?,
        }
    }
    let mut abis = BTreeSet::new();
    for abi in &manifest.abis {
        relative_path(&abi.path)?;
        ensure(abis.insert(&abi.name), "duplicate ABI name")?;
        ensure(!abi.value.is_empty(), "empty ABI value")?;
        ensure(
            abi.source_pattern.matches("{value}").count() == 1,
            "invalid ABI witness",
        )?;
        ensure(
            manifest.snapshots.iter().any(|snapshot| {
                snapshot.name == abi.repository
                    && snapshot.files.iter().any(|file| file.path == abi.path)
            }),
            "ABI witness is outside the hashed source inventory",
        )?;
    }
    ensure(manifest.abis.len() == 14, "incomplete ABI inventory")?;
    ensure(
        manifest.application_routes.len() == 7,
        "incomplete application-route inventory",
    )?;
    for route in &manifest.application_routes {
        relative_path(&route.path)?;
        ensure(
            !route.name.is_empty()
                && !route.baseline_path.is_empty()
                && !route.target.is_empty()
                && manifest.legacy_reference_files.contains(&route.path),
            "application route lacks its baseline witness",
        )?;
    }
    ensure(
        manifest.required_feature_gates.len() == 10
            && manifest
                .required_feature_gates
                .values()
                .all(|value| !value.is_empty())
            && manifest
                .required_feature_gates
                .get("neutral_rho_ir")
                .map(String::as_str)
                == Some("not_implemented"),
        "baseline must not imply completion of the neutral frontend",
    )?;
    for path in &manifest.legacy_reference_files {
        relative_path(path)?;
    }
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("git could not run: {error}"))?;
    ensure(
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )?;
    Ok(output.stdout)
}

fn node_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn external_roots() -> Result<Roots, String> {
    let mut roots = BTreeMap::from([("node".into(), node_root())]);
    for (name, variable) in [
        ("mettail", "F1R3LANG_METTAIL_ROOT"),
        ("venus", "F1R3LANG_VENUS_ROOT"),
        ("legacy_parser", "F1R3LANG_LEGACY_PARSER_ROOT"),
        ("papers", "F1R3LANG_PAPERS_ROOT"),
    ] {
        let root = std::env::var_os(variable).ok_or_else(|| {
            format!("set {variable} explicitly; external snapshot was not checked")
        })?;
        roots.insert(name.into(), PathBuf::from(root));
    }
    Ok(roots)
}

fn load_sources(manifest: &Manifest, roots: &Roots) -> Result<Sources, String> {
    let mut sources = Sources::new();
    for snapshot in &manifest.snapshots {
        let root = roots.get(&snapshot.name).ok_or("missing repository root")?;
        for file in &snapshot.files {
            relative_path(&file.path)?;
            let bytes = match snapshot.name.as_str() {
                "node" => git(root, &[
                    "show",
                    &format!("{}:{}", snapshot.revision, file.path),
                ])?,
                _ => fs::read(root.join(&file.path)).map_err(|error| error.to_string())?,
            };
            check_digest(&bytes, &file.sha256)
                .map_err(|error| format!("{}:{}: {error}", snapshot.name, file.path))?;
            if snapshot.kind == "revision_sources" {
                let committed = git(root, &[
                    "show",
                    &format!("{}:{}", snapshot.revision, file.path),
                ])?;
                check_digest(&committed, &file.sha256)?;
            }
            sources.insert((snapshot.name.clone(), file.path.clone()), bytes);
        }
        if snapshot.kind == "development_snapshot" {
            let head = git(root, &["rev-parse", "HEAD"])?;
            ensure(
                String::from_utf8_lossy(&head).trim() == snapshot.revision,
                "development snapshot HEAD changed",
            )?;
            let diff = git(root, &[
                "diff",
                "--no-ext-diff",
                "--binary",
                "--no-color",
                "HEAD",
            ])?;
            check_digest(
                &diff,
                snapshot
                    .tracked_diff_sha256
                    .as_deref()
                    .ok_or("missing diff hash")?,
            )?;
            let modified = git(root, &["diff", "--name-only", "HEAD"])?;
            ensure(
                String::from_utf8_lossy(&modified)
                    .lines()
                    .collect::<Vec<_>>()
                    == snapshot
                        .modified_paths
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                "development modified-file inventory changed",
            )?;
            let untracked = git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
            let actual: BTreeSet<_> = untracked
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| String::from_utf8_lossy(path).into_owned())
                .collect();
            let expected: BTreeSet<_> = snapshot
                .untracked_files
                .iter()
                .map(|file| file.path.clone())
                .collect();
            ensure(
                actual == expected,
                "development untracked-file inventory changed",
            )?;
            for file in &snapshot.untracked_files {
                let bytes = fs::read(root.join(&file.path)).map_err(|error| error.to_string())?;
                check_digest(&bytes, &file.sha256)?;
            }
        }
    }
    Ok(sources)
}

fn check_abis(abis: &[Abi], sources: &Sources) -> Result<(), String> {
    for abi in abis {
        let bytes = sources
            .get(&(abi.repository.clone(), abi.path.clone()))
            .ok_or_else(|| format!("missing source for ABI {}", abi.name))?;
        let source = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        ensure(
            source.contains(&abi.source_pattern.replace("{value}", &abi.value)),
            format!("ABI witness changed: {}", abi.name),
        )?;
    }
    Ok(())
}

fn legacy_references(root: &Path, source_roots: &[String]) -> Result<BTreeSet<String>, String> {
    let mut pending = Vec::with_capacity(source_roots.len());
    for directory in source_roots {
        relative_path(directory)?;
        pending.push(root.join(directory));
    }
    let mut found = BTreeSet::new();
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        ensure(
            !metadata.file_type().is_symlink(),
            "symlink in source inventory",
        )?;
        if metadata.is_dir() {
            for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
                pending.push(entry.map_err(|error| error.to_string())?.path());
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            if source.contains("Compiler::source_to_adt")
                || source.contains("rholang_parser::RholangParser::new")
            {
                found.insert(
                    path.strip_prefix(root)
                        .map_err(|error| error.to_string())?
                        .to_str()
                        .ok_or("non-UTF-8 source path")?
                        .replace('\\', "/"),
                );
            }
        }
    }
    Ok(found)
}

fn check_reference_inventory(manifest: &Manifest, found: &BTreeSet<String>) -> Result<(), String> {
    ensure(
        *found == manifest.legacy_reference_files.iter().cloned().collect(),
        "legacy reference inventory changed; review source routes before updating the manifest",
    )
}

#[test]
fn baseline_contract_is_explicit_and_non_activating() {
    check_contract(&manifest()).expect("valid baseline contract");
}

#[test]
fn changed_baseline_or_activation_claim_is_rejected() {
    let mut changed = manifest();
    changed.node_base.replace_range(0..1, "0");
    assert!(check_contract(&changed).is_err());
    let mut changed = manifest();
    changed.public_activation = true;
    assert!(check_contract(&changed).is_err());
    let mut changed = manifest();
    changed
        .snapshots
        .iter_mut()
        .find(|item| item.name == "mettail")
        .expect("MeTTaIL snapshot")
        .kind = "revision_sources".into();
    assert!(check_contract(&changed).is_err());
}

#[test]
fn malformed_or_incomplete_inventory_is_rejected() {
    let mut changed = manifest();
    changed.abis.pop();
    assert!(check_contract(&changed).is_err());
    let mut changed = manifest();
    changed.snapshots.pop();
    assert!(check_contract(&changed).is_err());
    let mut changed = manifest();
    changed.source_roots.pop();
    assert!(check_contract(&changed).is_err());
    for path in ["", "/outside", "../outside", "source/../../outside"] {
        assert!(relative_path(path).is_err(), "{path}");
    }
}

#[test]
fn changed_source_and_abi_witnesses_are_rejected() {
    let original = b"pub const VERSION: u16 = 3;";
    assert!(check_digest(original, &hex_digest(original)).is_ok());
    assert!(check_digest(b"pub const VERSION: u16 = 4;", &hex_digest(original)).is_err());
    let mut abi = Abi {
        repository: "fixture".into(),
        path: "source.rs".into(),
        name: "version".into(),
        value: "3".into(),
        source_pattern: "pub const VERSION: u16 = {value};".into(),
    };
    let sources = BTreeMap::from([(("fixture".into(), "source.rs".into()), original.to_vec())]);
    check_abis(&[abi.clone()], &sources).expect("matching ABI");
    abi.value = "4".into();
    assert!(check_abis(&[abi], &sources).is_err());
}

#[test]
fn live_node_descends_from_the_approved_baseline() {
    let declared = manifest();
    check_contract(&declared).expect("valid baseline contract");
    git(&node_root(), &[
        "merge-base",
        "--is-ancestor",
        &declared.node_base,
        "HEAD",
    ])
    .expect("isolated adapter descends from the approved engine baseline");
}

#[test]
fn live_source_reference_inventory_matches_the_frozen_inventory() {
    let declared = manifest();
    let found = legacy_references(&node_root(), &declared.source_roots).expect("source inventory");
    check_reference_inventory(&declared, &found).expect("reviewed source reference inventory");
    let mut added = found.clone();
    added.insert("node/src/unreviewed_frontend.rs".into());
    assert!(check_reference_inventory(&declared, &added).is_err());
    let mut removed = found;
    removed.remove("node/src/rust/api/repl_grpc_service.rs");
    assert!(check_reference_inventory(&declared, &removed).is_err());
}

#[test]
fn live_pinned_sources_and_abi_witnesses_match() {
    let declared = manifest();
    check_contract(&declared).expect("valid baseline contract");
    let roots = external_roots().expect("explicit roots for the complete snapshot gate");
    let sources = load_sources(&declared, &roots).expect("exact source snapshots");
    check_abis(&declared.abis, &sources).expect("pinned ABI declarations");
    let mut current = sources;
    for abi in declared.abis.iter().filter(|abi| abi.repository == "node") {
        current.insert(
            (abi.repository.clone(), abi.path.clone()),
            fs::read(node_root().join(&abi.path)).expect("current node ABI source"),
        );
    }
    check_abis(&declared.abis, &current).expect("current node ABI declarations");
    let node = declared
        .snapshots
        .iter()
        .find(|snapshot| snapshot.name == "node")
        .expect("node snapshot");
    for file in node.files.iter().filter(|file| {
        file.path.starts_with("models/")
            || file
                .path
                .starts_with("rholang/src/rust/interpreter/accounting/")
    }) {
        check_digest(
            &fs::read(node_root().join(&file.path)).expect("current engine source"),
            &file.sha256,
        )
        .unwrap_or_else(|error| panic!("protected engine source {}: {error}", file.path));
    }
}
