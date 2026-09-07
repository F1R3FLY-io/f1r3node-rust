use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use chrono::NaiveDate;
use eyre::Result;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::{json, Value};
use supply_chain::policy::{self, Policy};
use supply_chain::{digest, install, report, scan, Runner, SystemRunner, CHECKS};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}
fn policy() -> Policy { policy::load(&root().join("supply-chain/policy.toml")).unwrap() }
fn deny() -> toml::Value {
    toml::from_str(&fs::read_to_string(root().join("deny.toml")).unwrap()).unwrap()
}
fn today() -> NaiveDate { NaiveDate::from_ymd_opt(2026, 9, 6).unwrap() }
fn advisory() -> Value {
    json!({"type":"diagnostic","fields":{"severity":"note","advisory":{"id":"RUSTSEC-2026-0258"},"graphs":[{"Krate":{"name":"h2","version":"0.3.27"}}]}})
}
fn report(records: &[Value]) -> String {
    let summary: serde_json::Map<_, _> = CHECKS
        .into_iter()
        .map(|c| (c.to_owned(), json!({"errors":0})))
        .collect();
    records
        .iter()
        .cloned()
        .chain([json!({"type":"summary","fields":summary})])
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn output(code: i32, stdout: impl Into<Vec<u8>>, stderr: impl Into<Vec<u8>>) -> Output {
    Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: stdout.into(),
        stderr: stderr.into(),
    }
}
#[derive(Default)]
struct FakeRunner {
    results: RefCell<VecDeque<Output>>,
    calls: RefCell<Vec<(String, Vec<String>)>>,
}
impl FakeRunner {
    fn new(outputs: Vec<Output>) -> Self {
        Self {
            results: RefCell::new(outputs.into()),
            ..Self::default()
        }
    }
}
impl Runner for FakeRunner {
    fn output(&self, _: &Path, program: &str, args: &[&str]) -> Result<Output> {
        self.calls
            .borrow_mut()
            .push((program.into(), args.iter().map(|s| (*s).into()).collect()));
        Ok(self
            .results
            .borrow_mut()
            .pop_front()
            .expect("unexpected command"))
    }
}

#[test]
fn current_policy_and_manifest_coverage_are_valid() {
    let p = policy();
    policy::validate(&p, &deny(), today()).unwrap();
    scan::validate_manifests(&root(), &p.manifests, &SystemRunner).unwrap();
}

#[test]
fn exceptions_expire_on_the_review_date() {
    let mut p = policy();
    p.exceptions.get_mut("RUSTSEC-2026-0258").unwrap().review_by = "2026-09-06".parse().unwrap();
    assert!(policy::validate(&p, &deny(), today()).is_err());
}

#[test]
fn exceptions_require_owners_packages_and_exact_versions() {
    for field in ["owner", "package", "versions", "range"] {
        let mut p = policy();
        let entry = p.exceptions.get_mut("RUSTSEC-2026-0258").unwrap();
        match field {
            "owner" => entry.owner = " ".into(),
            "package" => entry.package.clear(),
            "versions" => entry.versions.clear(),
            _ => entry.versions = vec![">=0.3".into()],
        }
        assert!(policy::validate(&p, &deny(), today()).is_err(), "{field}");
    }
}

#[test]
fn ignored_advisories_require_matching_review_records_and_reasons() {
    let p = policy();
    let mut d = deny();
    d["advisories"]["ignore"].as_array_mut().unwrap().pop();
    assert!(policy::validate(&p, &d, today()).is_err());
    let mut d = deny();
    let duplicate = d["advisories"]["ignore"][0].clone();
    d["advisories"]["ignore"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(policy::validate(&p, &d, today()).is_err());
    let mut d = deny();
    d["advisories"]["ignore"][0]["reason"] = toml::Value::String("".into());
    assert!(policy::validate(&p, &d, today()).is_err());
}

#[test]
fn weakened_controls_are_rejected() {
    for (section, key, value) in [
        ("graph", "all-features", toml::Value::Boolean(false)),
        ("graph", "no-default-features", toml::Value::Boolean(true)),
        (
            "advisories",
            "unsound",
            toml::Value::String("workspace".into()),
        ),
        (
            "advisories",
            "unmaintained",
            toml::Value::String("workspace".into()),
        ),
        ("advisories", "yanked", toml::Value::String("warn".into())),
        (
            "advisories",
            "maximum-db-staleness",
            toml::Value::String("P30D".into()),
        ),
        ("bans", "wildcards", toml::Value::String("allow".into())),
        (
            "sources",
            "required-git-spec",
            toml::Value::String("tag".into()),
        ),
    ] {
        let mut d = deny();
        d[section][key] = value;
        assert!(
            policy::validate(&policy(), &d, today()).is_err(),
            "{section}.{key}"
        );
    }
}

#[test]
fn graph_exclusions_and_disabled_yank_checks_are_rejected() {
    for (section, key, value) in [
        ("graph", "exclude-dev", toml::Value::Boolean(true)),
        ("graph", "exclude-unpublished", toml::Value::Boolean(true)),
        (
            "graph",
            "exclude",
            toml::Value::Array(vec!["bitmaps".into()]),
        ),
        (
            "graph",
            "targets",
            toml::Value::Array(vec!["x86_64-unknown-linux-gnu".into()]),
        ),
        (
            "advisories",
            "disable-yank-checking",
            toml::Value::Boolean(true),
        ),
    ] {
        let mut d = deny();
        d[section].as_table_mut().unwrap().insert(key.into(), value);
        assert!(policy::validate(&policy(), &d, today()).is_err(), "{key}");
    }
}

#[test]
fn license_clarifications_require_hashed_files() {
    for value in [
        toml::Value::Array(vec![]),
        toml::Value::Array(vec![toml::Value::Table(Default::default())]),
    ] {
        let mut d = deny();
        d["licenses"]["clarify"][0]["license-files"] = value;
        assert!(policy::validate(&policy(), &d, today()).is_err());
    }
}

#[test]
fn executable_scanning_and_build_allowances_are_required() {
    for key in ["allow-build-scripts", "executables", "include-dependencies"] {
        let mut d = deny();
        d["bans"]["build"].as_table_mut().unwrap().remove(key);
        assert!(policy::validate(&policy(), &d, today()).is_err());
    }
}

#[test]
fn manifest_coverage_cannot_omit_fuzz_or_duplicate_a_workspace() {
    let mut p = policy();
    p.manifests.retain(|s| s != "fuzz/Cargo.toml");
    assert!(scan::validate_manifests(&root(), &p.manifests, &SystemRunner).is_err());
    let mut p = policy();
    p.manifests.push("Cargo.toml".into());
    assert!(scan::validate_manifests(&root(), &p.manifests, &SystemRunner).is_err());
}

#[test]
fn a_new_standalone_package_requires_a_scan() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("tools")).unwrap();
    for name in ["Cargo.toml", "tools/Cargo.toml"] {
        fs::write(dir.path().join(name), "").unwrap();
    }
    let fake = FakeRunner::new(vec![
        output(0, "Cargo.toml\0tools/Cargo.toml\0", ""),
        output(0, dir.path().join("Cargo.toml").to_str().unwrap(), ""),
        output(0, dir.path().join("tools/Cargo.toml").to_str().unwrap(), ""),
    ]);
    assert!(scan::validate_manifests(dir.path(), &["Cargo.toml".into()], &fake).is_err());
}

#[test]
fn missing_or_ignored_lockfiles_are_rejected() {
    for ignored in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        if ignored {
            fs::write(dir.path().join("Cargo.lock"), "").unwrap();
        }
        let fake = FakeRunner::new(vec![
            output(0, "Cargo.toml\0", ""),
            output(0, dir.path().join("Cargo.toml").to_str().unwrap(), ""),
            output(0, "", ""),
        ]);
        assert!(scan::validate_manifests(dir.path(), &["Cargo.toml".into()], &fake).is_err());
    }
}

#[test]
fn git_revisions_must_be_full_and_match_the_resolved_commit() {
    let sha = "1".repeat(40);
    for rev in ["1111111".into(), "2".repeat(40)] {
        assert!(scan::validate_metadata(&json!({"packages":[{"name":"fixture","source":format!("git+https://example.com/repo?rev={rev}#{sha}")}]})).is_err());
    }
    scan::validate_metadata(
        &json!({"packages":[{"source":format!("git+https://example.com/repo?rev={sha}#{sha}")}]}),
    )
    .unwrap();
}

#[test]
fn reports_accept_only_reviewed_package_versions() {
    let p = policy();
    assert!(report::validate(&report(&[advisory()]), &p.exceptions)
        .errors
        .is_empty());
    let mut wrong = advisory();
    wrong["fields"]["graphs"][0]["Krate"]["version"] = json!("0.4.15");
    assert!(!report::validate(&report(&[wrong]), &p.exceptions)
        .errors
        .is_empty());
    let mut wrong = advisory();
    wrong["fields"]["graphs"][0]["Krate"]["name"] = json!("other");
    assert!(!report::validate(&report(&[wrong]), &p.exceptions)
        .errors
        .is_empty());
}

#[test]
fn internal_bugs_errors_and_invalid_lines_fail_closed() {
    let p = policy();
    for record in [
        json!({"fields":{"severity":"bug"}}),
        json!({"fields":{"severity":"error"}}),
        json!({"fields":{"level":"ERROR"}}),
        json!(null),
        json!([]),
        json!({"fields":[]}),
    ] {
        assert!(!report::validate(&report(&[record]), &p.exceptions)
            .errors
            .is_empty());
    }
    for text in [
        "".into(),
        "not JSON".into(),
        format!("noise\n{}", report(&[])),
    ] {
        assert!(!report::validate(&text, &p.exceptions).errors.is_empty());
    }
}

#[test]
fn malformed_nested_records_fail_without_stopping_later_records() {
    let p = policy();
    for value in [
        json!(null),
        json!({}),
        json!(true),
        json!([]),
        json!("id"),
        json!({"id":[]}),
        json!({"id":""}),
        json!({"title":"missing id"}),
    ] {
        let mut malformed = advisory();
        malformed["fields"]["advisory"] = value.clone();
        let result = report::validate(&report(&[malformed, advisory()]), &p.exceptions);
        assert!(!result.errors.is_empty(), "{value}");
        assert!(result.encountered.contains("RUSTSEC-2026-0258"));
    }
    for value in [
        json!(null),
        json!({}),
        json!("graphs"),
        json!([]),
        json!([null]),
        json!([{}]),
        json!([{"Krate":null}]),
        json!([{"Krate":[]}]),
        json!([{"Krate":{"name":[],"version":null}}]),
    ] {
        let mut malformed = advisory();
        malformed["fields"]["graphs"] = value.clone();
        let result = report::validate(&report(&[malformed, advisory()]), &p.exceptions);
        assert!(!result.errors.is_empty(), "{value}");
        assert!(result.encountered.contains("RUSTSEC-2026-0258"));
    }
}

#[test]
fn summaries_require_all_checks_and_nonnegative_integer_counts() {
    for stats in [
        json!({}),
        json!({"errors":-1}),
        json!({"errors":"0"}),
        json!({"errors":false}),
        json!({"errors":0.0}),
        json!(null),
    ] {
        let fields: serde_json::Map<_, _> = CHECKS
            .into_iter()
            .map(|c| (c.into(), stats.clone()))
            .collect();
        assert!(!report::validate(
            &json!({"type":"summary","fields":fields}).to_string(),
            &policy().exceptions
        )
        .errors
        .is_empty());
    }
    for text in [
        json!({"type":"summary","fields":{"advisories":{"errors":0}}}).to_string(),
        format!("{}\n{}", report(&[]), report(&[])),
    ] {
        assert!(!report::validate(&text, &policy().exceptions)
            .errors
            .is_empty());
    }
}

#[test]
fn scans_continue_and_retain_evidence_after_malformed_reports() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = policy();
    p.exceptions.retain(|id, _| id == "RUSTSEC-2026-0258");
    fs::create_dir_all(dir.path().join("supply-chain")).unwrap();
    fs::write(dir.path().join("deny.toml"), "").unwrap();
    fs::write(dir.path().join("supply-chain/policy.toml"), "").unwrap();
    let mut outputs = vec![];
    for (index, manifest) in p.manifests.iter().enumerate() {
        let lock = dir.path().join(manifest).with_file_name("Cargo.lock");
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        fs::write(lock, "version = 4\n").unwrap();
        outputs.push(output(0, json!({"packages":[]}).to_string(), ""));
        let mut a = advisory();
        if index == 0 {
            a["fields"]["advisory"] = json!({"title":"missing id"});
        }
        outputs.push(output(0, "", report(&[a])));
    }
    let fake = FakeRunner::new(outputs);
    assert!(!scan::scan_all(dir.path(), &p, "cargo-deny", &fake).unwrap());
    assert_eq!(fake.calls.borrow().len(), 6);
    for (_, args) in fake.calls.borrow().iter() {
        assert!(args.contains(&"--locked".into()));
        assert!(args.contains(&"--all-features".into()));
    }
    for prefix in ["workspace", "fuzz", "scripts-soak-charts"] {
        for suffix in [
            "metadata.json",
            "metadata.stderr",
            "deny.jsonl",
            "summary.json",
        ] {
            assert!(dir
                .path()
                .join(format!("target/supply-chain/{prefix}-{suffix}"))
                .is_file());
        }
        let summary: Value = serde_json::from_str(
            &fs::read_to_string(
                dir.path()
                    .join(format!("target/supply-chain/{prefix}-summary.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            summary["errors"].as_array().unwrap().is_empty(),
            prefix != "workspace"
        );
        assert_eq!(summary["lock_sha256"], digest(b"version = 4\n"));
    }
}

#[test]
fn metadata_failures_do_not_skip_later_manifests() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = policy();
    p.manifests = vec!["Cargo.toml".into(), "fuzz/Cargo.toml".into()];
    fs::create_dir_all(dir.path().join("fuzz")).unwrap();
    for name in &p.manifests {
        fs::write(dir.path().join(name).with_file_name("Cargo.lock"), "").unwrap();
    }
    let fake = FakeRunner::new(vec![output(1, "", "failed"), output(0, "invalid JSON", "")]);
    assert!(!scan::scan_all(dir.path(), &p, "cargo-deny", &fake).unwrap());
    assert_eq!(fake.calls.borrow().len(), 2);
    for prefix in ["workspace", "fuzz"] {
        assert!(dir
            .path()
            .join(format!("target/supply-chain/{prefix}-summary.json"))
            .is_file());
    }
}

fn archive(names: &[(&str, bool)]) -> Vec<u8> {
    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (name, link) in names {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o755);
        if *link {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_link_name("fixture").unwrap();
        } else {
            header.set_size(7);
        }
        let data: &[u8] = if *link { b"" } else { b"fixture" };
        archive.append_data(&mut header, name, data).unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
}

#[test]
fn installer_rejects_wrong_hash_links_and_duplicate_executables() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("bin");
    let bytes = archive(&[("release/cargo-deny", false)]);
    assert!(install::archive(&bytes, &"0".repeat(64), &destination).is_err());
    assert!(!destination.exists());
    for names in [
        vec![("cargo-deny", true)],
        vec![("a/cargo-deny", false), ("b/cargo-deny", false)],
        vec![("other", false)],
    ] {
        let bytes = archive(&names);
        assert!(install::archive(&bytes, &digest(&bytes), &destination).is_err());
        assert!(!destination.exists());
    }
}

#[test]
fn installer_writes_only_the_verified_executable_and_can_run_again() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = archive(&[("release/cargo-deny", false), ("other", false)]);
    for _ in 0..2 {
        install::archive(&bytes, &digest(&bytes), dir.path()).unwrap();
    }
    assert_eq!(fs::read(dir.path().join("cargo-deny")).unwrap(), b"fixture");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn supported_scanner_targets_are_explicit() {
    assert_eq!(
        install::target("arm64", "macos").unwrap(),
        "aarch64-apple-darwin"
    );
    assert_eq!(
        install::target("x86_64", "linux").unwrap(),
        "x86_64-unknown-linux-musl"
    );
    assert!(install::target("armv7", "linux").is_err());
    assert!(install::target("x86_64", "windows").is_err());
}
