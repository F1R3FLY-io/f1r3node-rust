use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use supply_chain::policy;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn toml_file(name: &str) -> toml::Value {
    toml::from_str(&fs::read_to_string(root().join(name)).unwrap()).unwrap()
}

fn workflows(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("yml" | "yaml")
            )
        })
        .collect();
    files.sort();
    files
}

fn unpinned(contents: &str) -> Vec<String> {
    let uses = Regex::new(r#"(?m)^\s*(?:-\s*)?uses:\s*["']?([^\s"']+)"#).unwrap();
    let pin = Regex::new(r"@[0-9a-f]{40}$").unwrap();
    uses.captures_iter(contents)
        .map(|m| m[1].to_owned())
        .filter(|action| !action.starts_with("./") && !pin.is_match(action))
        .collect()
}

#[test]
fn all_workflows_pin_external_actions() {
    let files = workflows(&root().join(".github/workflows"));
    assert!(!files.is_empty());
    let mut failures = vec![];
    for file in files {
        for action in unpinned(&fs::read_to_string(&file).unwrap()) {
            failures.push(format!(
                "{}: {action}",
                file.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "Unpinned actions:\n{}",
        failures.join("\n")
    );
}

#[test]
fn new_workflows_and_both_yaml_extensions_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["new.yml", "renamed.yaml"] {
        fs::write(
            dir.path().join(name),
            "steps:\n  - uses: 'actions/checkout@v5'\n",
        )
        .unwrap();
    }
    fs::write(dir.path().join("notes.md"), "").unwrap();
    let files = workflows(dir.path());
    assert_eq!(files.len(), 2);
    for file in files {
        assert_eq!(unpinned(&fs::read_to_string(file).unwrap()), [
            "actions/checkout@v5"
        ]);
    }
    assert!(unpinned(&format!(
        "uses: ./local\nuses: \"actions/checkout@{}\"\n",
        "1".repeat(40)
    ))
    .is_empty());
}

#[test]
fn rust_cache_writes_require_a_trusted_push() {
    let allowed = "${{ github.event_name == 'push' && (github.ref == 'refs/heads/dev' || github.ref == 'refs/heads/master') }}";
    let mut count = 0;
    for file in workflows(&root().join(".github/workflows")) {
        let text = fs::read_to_string(&file).unwrap();
        let lines: Vec<_> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line
                .trim_start()
                .starts_with("- uses: Swatinem/rust-cache@")
                && !line.trim_start().starts_with("uses: Swatinem/rust-cache@")
            {
                continue;
            }
            count += 1;
            let indent = line.len() - line.trim_start().len()
                + if line.trim_start().starts_with('-') {
                    2
                } else {
                    0
                };
            let block: Vec<_> = lines[index + 1..]
                .iter()
                .take_while(|s| s.trim().is_empty() || s.len() - s.trim_start().len() >= indent)
                .collect();
            let guard = block
                .iter()
                .find_map(|s| s.trim().strip_prefix("save-if: "));
            assert!(
                guard == Some("false") || guard == Some(allowed),
                "{}:{}: missing cache-write restriction",
                file.file_name().unwrap().to_string_lossy(),
                index + 1
            );
        }
    }
    assert!(count > 0);
}

#[test]
fn bitmaps_resolution_and_policy_exclude_unsound_releases() {
    for file in ["Cargo.lock", "fuzz/Cargo.lock"] {
        let lock = toml_file(file);
        let versions: Vec<_> = lock["package"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["name"].as_str() == Some("bitmaps"))
            .map(|p| p["version"].as_str().unwrap())
            .collect();
        assert_eq!(versions, ["3.1.0"]);
    }
    let deny = toml_file("deny.toml");
    assert!(deny["bans"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["crate"].as_str() == Some("bitmaps:>=3.2.0")));
    assert!(!deny["advisories"]["ignore"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"].as_str() == Some("RUSTSEC-2025-0167")));
    let policy = policy::load(&root().join("supply-chain/policy.toml")).unwrap();
    assert!(!policy.exceptions.contains_key("RUSTSEC-2025-0167"));
    assert_eq!(policy.exceptions["RUSTSEC-2026-0247"].versions, ["3.1.0"]);
}

#[test]
fn deny_workflows_use_the_rust_launcher_and_keep_evidence() {
    for name in ["ci.yml", "deny-schedule.yml"] {
        let text = fs::read_to_string(root().join(".github/workflows").join(name)).unwrap();
        assert!(text.contains("check-supply-chain.sh install"));
        assert!(text.contains("check-supply-chain.sh check"));
        assert!(text.contains("path: target/supply-chain/"));
        assert!(!text.contains("cargo-deny-action@"));
        assert!(!text.contains("check_supply_chain.py"));
        assert!(!text.contains("test_supply_chain.py"));
    }
    let launcher = fs::read_to_string(root().join("scripts/ci/check-supply-chain.sh")).unwrap();
    assert!(launcher.contains("--locked"));
    assert!(launcher.contains("-p supply-chain"));
}

fn scheduled_job(name: &str) -> String {
    let workflow = fs::read_to_string(root().join(".github/workflows/deny-schedule.yml")).unwrap();
    let marker = format!("\n  {name}:\n");
    workflow
        .split_once(&marker)
        .unwrap_or_else(|| panic!("Missing scheduled audit job: {name}"))
        .1
        .lines()
        .take_while(|line| line.trim().is_empty() || line.starts_with("    "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn scheduled_audit_executes_only_the_event_commit() {
    let job = scheduled_job("deny");
    assert_eq!(job.matches("uses: actions/checkout@").count(), 1);
    assert!(job.contains("ref: ${{ github.sha }}"));
    assert!(job.contains("persist-credentials: false"));
    assert!(!job.contains("matrix."));
    assert!(!job.contains("actions: write"));
    assert!(!job.contains("GH_TOKEN:"));
    assert!(job.contains("if: github.ref == 'refs/heads/dev' || github.ref == 'refs/heads/master'"));
    assert!(job.contains("check-supply-chain.sh install"));
    assert!(job.contains("check-supply-chain.sh check"));
    assert!(job.contains("if: always()"));
    assert!(job.contains("if-no-files-found: error"));
    assert!(!job.contains("tooling.outputs.present"));
}

#[test]
fn scheduled_dispatch_has_no_checkout_or_branch_code_execution() {
    let job = scheduled_job("dispatch-dev");
    assert!(
        job.contains("if: github.event_name == 'schedule' && github.ref == 'refs/heads/master'")
    );
    assert!(job.contains("actions: write"));
    assert!(job.contains("GH_TOKEN: ${{ github.token }}"));
    assert!(!job.contains("uses:"));
    assert!(!job.contains("contents: write"));
    let commands: Vec<_> = job
        .lines()
        .filter_map(|line| line.trim().strip_prefix("run: "))
        .collect();
    assert_eq!(commands, [
        "gh workflow run deny-schedule.yml --repo \"$GITHUB_REPOSITORY\" --ref dev"
    ]);
    assert_eq!(job.matches("run:").count(), 1);
    assert!(!job.contains("continue-on-error"));
}

#[test]
fn manual_audits_do_not_accept_an_independent_checkout_reference() {
    let workflow = fs::read_to_string(root().join(".github/workflows/deny-schedule.yml")).unwrap();
    assert!(workflow.contains("  workflow_dispatch:\n"));
    assert!(workflow.contains("  schedule:\n"));
    assert!(!workflow.contains("inputs:"));
    assert!(!workflow.contains("workflow_call:"));
    assert!(workflow.contains("permissions:\n  contents: read\n"));
    assert_eq!(workflow.matches("actions: write").count(), 1);
}

#[test]
fn rust_supply_chain_sources_require_ci_owner_review() {
    let owners = fs::read_to_string(root().join(".github/CODEOWNERS")).unwrap();
    assert!(owners.lines().any(|line| {
        line.split_whitespace().collect::<Vec<_>>()
            == ["/scripts/supply-chain/", "@F1R3FLY-io/ci-maintainers"]
    }));
}

#[test]
fn docker_build_remains_frozen_offline_and_contains_the_workspace_tool() {
    let text = fs::read_to_string(root().join("node/Dockerfile")).unwrap();
    for expected in [
        "cargo fetch --locked",
        "RUN --network=none",
        "xx-cargo build --frozen",
        "sha256sum --check --strict",
        "COPY scripts/supply-chain/",
    ] {
        assert!(text.contains(expected), "{expected}");
    }
    assert!(!text.contains("type=cache"));
    assert!(!text.contains("|| true"));
    for line in text
        .lines()
        .filter(|s| s.starts_with("FROM ") && s.contains(" AS "))
    {
        assert!(line.contains("@sha256:"));
    }
}

#[test]
fn swagger_assets_are_vendored() {
    assert!(
        toml_file("node/Cargo.toml")["dependencies"]["utoipa-swagger-ui"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("vendored"))
    );
}

#[test]
fn unused_certificate_dependencies_are_removed() {
    let manifest = toml_file("comm/Cargo.toml");
    let deps = manifest["dependencies"].as_table().unwrap();
    for name in ["rustls-webpki", "webpki", "paste"] {
        assert!(!deps.contains_key(name));
    }
    assert!(manifest["dev-dependencies"]
        .as_table()
        .unwrap()
        .contains_key("paste"));
}
