#[path = "manifest.rs"]
mod manifests;

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use casper_soak::{manifest, runtime, *};
    use serde_json::{json, Value};

    fn source() -> PathBuf {
        std::env::var_os("SOAK_SOURCE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .to_path_buf()
            })
    }
    fn binary() -> PathBuf {
        std::env::var_os("SOAK_HARNESS_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_casper-soak")))
    }
    fn save(path: &Path, value: &Value) {
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(path, encoded(value).unwrap()).unwrap();
    }
    fn tree(root: &Path) -> Value {
        if !root.exists() {
            return json!({});
        }
        let values: BTreeMap<_, _> = walk(root)
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p.strip_prefix(root).unwrap().to_string_lossy().into_owned(),
                    file_hash(p).unwrap(),
                )
            })
            .collect();
        json!(values)
    }
    struct Case {
        root: PathBuf,
        output: PathBuf,
        m: Value,
        env: BTreeMap<String, String>,
        calls: usize,
        _temporary: Option<tempfile::TempDir>,
    }
    impl Case {
        fn new(name: &str, mode: &str, iterations: u64) -> Self {
            let temporary = if std::env::var_os("SOAK_TEST_ARTIFACTS").is_none() {
                Some(tempfile::tempdir().unwrap())
            } else {
                None
            };
            let root = if let Some(t) = &temporary {
                t.path().join(name)
            } else {
                PathBuf::from(std::env::var_os("SOAK_TEST_ARTIFACTS").unwrap()).join(name)
            };
            fs::create_dir_all(&root).unwrap();
            for name in ["inputs", "suite", "bin", "tmp", "runner"] {
                fs::create_dir(root.join(name)).unwrap();
            }
            fs::write(root.join("bin/docker"), "#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(root.join("bin/docker"), fs::Permissions::from_mode(0o755))
                .unwrap();
            let mut m = super::manifests::sample();
            m["profile_id"] = "harness-lifecycle".into();
            m["run_id"] = name.into();
            let pins: BTreeMap<_, _> = CORE
                .iter()
                .map(|name| (*name, file_hash(&source().join(name)).unwrap()))
                .collect();
            m["source_digests"] = json!(pins);
            m["profile_digest"] = m["source_digests"]["scripts/casper-soak/src/runtime.rs"].clone();
            m["tool_versions"] = json!({"casper_soak":env!("CARGO_PKG_VERSION")});
            m["deadline"] = json!({"clock_id":"unix-seconds","epoch_seconds":(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()+120).to_string()});
            m["resource_limits"] = json!({"children":1,"timeout_seconds":30,"rss_ceiling_mb":0,"host_free_floor_mb":0,"disk_free_floor_mb":0,"artifact_bytes":1048576});
            let request = json!({"scenario_id":"fixture-scenario","pair_id":"pair-1","seed":"1","expected":0,"mode":mode,"fault_schedule":[],"required_observations":[{"event_kind":"sample","node":"validator-1","incarnation":"incarnation-1","member_id":"baseline"}]});
            let payloads = json!({"request":request,"configuration":{"duration_seconds":120,"rss_ceiling_mb":0,"host_free_floor_mb":0,"disk_free_floor_mb":0,"policy_variant":"baseline"},"fixture":{"seed":"1"},"expectation":{"value":0},"image_manifest":{"schemaVersion":2},"qualification":{"capability":"fixture-observation","provider":"docker","node_revision":m["node_revision"],"status":"qualified","evidence_kind":"synthetic_fixture"}});
            let mut assets = json!({});
            for (name, value) in payloads.as_object().unwrap() {
                let path = root.join(format!("inputs/{name}.json"));
                save(&path, value);
                assets[name] = json!({"path":path.file_name().unwrap().to_str().unwrap(),"bytes":fs::metadata(&path).unwrap().len(),"sha256":file_hash(&path).unwrap()});
            }
            for (name, bytes) in [
                ("node_binary", b"synthetic binary identity\n".to_vec()),
                (
                    "executor",
                    fs::read(source().join("scripts/bench/fixtures/casper-lifecycle-executor.sh"))
                        .unwrap(),
                ),
            ] {
                let path = root.join(format!("inputs/{name}.sh"));
                fs::write(&path, bytes).unwrap();
                assets[&name] = json!({"path":path.file_name().unwrap().to_str().unwrap(),"bytes":fs::metadata(&path).unwrap().len(),"sha256":file_hash(&path).unwrap()});
            }
            for (role, field) in [
                ("configuration", "configuration_digest"),
                ("fixture", "fixture_digest"),
                ("expectation", "expectation_digest"),
                ("node_binary", "node_binary_digest"),
                ("image_manifest", "image_digest"),
            ] {
                m[field] = assets[role]["sha256"].clone();
            }
            m["capabilities"] = json!({"fixture-observation":{"status":"qualified","provider":"docker","revision":m["node_revision"],"qualification":assets["qualification"]}});
            m["runtime"] = json!({"iterations":iterations,"iterations_per_segment":1,"required_capabilities":["fixture-observation"],"harness_digest":file_hash(&binary()).unwrap(),"executor_kind":"bash","assets":assets});
            let mut env = BTreeMap::new();
            for (name, value) in [
                ("SOAK_DURATION_SECONDS", "120"),
                ("SOAK_RSS_CEILING_MB", "0"),
                ("SOAK_HOST_FREE_FLOOR_MB", "0"),
                ("SOAK_DISK_FREE_FLOOR_MB", "0"),
                ("SOAK_RUN_BENCHMARKS", "false"),
                ("SOAK_GUARDIAN_POLL_SECONDS", "0.05"),
            ] {
                env.insert(name.into(), value.into());
            }
            for (name, path) in [
                ("SOAK_MANIFEST_PATH", root.join("manifest.json")),
                ("SOAK_INPUT_DIR", root.join("inputs")),
                ("SOAK_APPROVAL_PATH", root.join("approval.json")),
                ("SOAK_OUTPUT_DIR", root.join("output")),
                ("SYSTEM_INTEGRATION_DIR", root.join("suite")),
                ("SOAK_TMP_ROOT", root.join("tmp")),
                ("SOAK_RUNNER_ROOT", root.join("runner")),
                ("SOAK_HARNESS_BIN", binary()),
            ] {
                env.insert(name.into(), path.to_string_lossy().into_owned());
            }
            env.insert(
                "PATH".into(),
                format!(
                    "{}:{}",
                    root.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            );
            let output = root.join("output");
            let mut case = Self {
                root,
                output,
                m,
                env,
                calls: 0,
                _temporary: temporary,
            };
            let bindings: BTreeMap<_, _> = (1..=10).map(|n| (format!("H{n:02}"), true)).collect();
            let receipt = json!({"origin":"fixture-substitute","status":"passed","phase":case.m["phase"],"claim_id":"CLAIM-CASPER-SOAK-001","construction":"not-applicable","source_digests":case.m["source_digests"],"bindings":bindings});
            case.asset("verification", receipt);
            case
        }
        fn asset(&mut self, role: &str, value: Value) {
            let path = self.root.join(format!("inputs/{role}.json"));
            save(&path, &value);
            self.m["runtime"]["assets"][role] = json!({"path":path.file_name().unwrap().to_str().unwrap(),"bytes":fs::metadata(&path).unwrap().len(),"sha256":file_hash(&path).unwrap()});
            self.seal();
        }
        fn seal(&mut self) {
            save(&self.root.join("manifest.json"), &self.m);
            let assets = &self.m["runtime"]["assets"];
            let qualifications: Vec<_> = ["qualification", "verification", "ancestry"]
                .iter()
                .filter_map(|key| assets.get(*key).map(|v| v["sha256"].clone()))
                .collect();
            save(
                &self.root.join("approval.json"),
                &json!({"approved":true,"manifest_digest":file_hash(&self.root.join("manifest.json")).unwrap(),"evidence_kind":self.m["evidence_kind"],"qualification_digests":qualifications,"executor_digest":assets["executor"]["sha256"]}),
            );
            self.env.insert(
                "SOAK_APPROVAL_SHA256".into(),
                file_hash(&self.root.join("approval.json")).unwrap(),
            );
        }
        fn invoke(&mut self, expected: i32) {
            self.calls += 1;
            let before = tree(&self.output);
            let snapshots = |out: &Path| -> Value {
                json!([".soak-state", ".soak-checkpoint-state.json"]
                    .iter()
                    .filter_map(|name| fs::read_to_string(out.join(name))
                        .ok()
                        .map(|v| ((*name).to_string(), v)))
                    .collect::<BTreeMap<_, _>>())
            };
            let state = snapshots(&self.output);
            let response = Command::new("timeout")
                .args(["--signal=TERM", "--kill-after=5", "25", "bash"])
                .arg(source().join("scripts/run-merge-recovery-soak.sh"))
                .envs(&self.env)
                .env_remove("SOAK_DEADLINE_EPOCH")
                .output()
                .unwrap();
            let code = response.status.code().unwrap_or(128);
            let report = json!({"command":["bash","scripts/run-merge-recovery-soak.sh"],"expected_exit":expected,"actual_exit":code,"stdout":String::from_utf8_lossy(&response.stdout),"stderr":String::from_utf8_lossy(&response.stderr),"before":before,"after":tree(&self.output),"before_state":state,"after_state":snapshots(&self.output),"manifest_bytes_utf8":fs::read_to_string(self.root.join("manifest.json")).unwrap(),"approval_bytes_utf8":fs::read_to_string(self.root.join("approval.json")).unwrap()});
            save(
                &self.root.join(format!("invocation-{:02}.json", self.calls)),
                &report,
            );
            assert_eq!(code, expected, "{}: {report}", self.root.display());
        }
        fn entry(&self, iteration: u64) -> Value {
            record(
                &self
                    .output
                    .join(format!("casper-history/{iteration:08}.json")),
            )
            .unwrap()
        }
        fn fault(&mut self, restart: bool) {
            let mut request = record(&self.root.join("inputs/request.json")).unwrap();
            request["fault_schedule"] = json!([{"fault_id":"fault-1","action":if restart {"restart"} else {"pause"},"node":"validator-1","incarnation":"incarnation-1","required_state":if restart {"ready"} else {"paused"},"predecessor":"incarnation-0"}]);
            self.asset("request", request);
        }
    }
    #[test]
    fn production_driver_bindings() {
        for (mode, expected, verdict) in [
            ("complete", 0, "passed"),
            ("duplicate", 0, "passed"),
            ("missing", 1, "incomplete"),
            ("missing-zero", 1, "incomplete"),
            ("wrong-identity", 1, "incomplete"),
            ("missing-artifact", 1, "incomplete"),
            ("boolean-counter", 1, "incomplete"),
            ("mismatch", 1, "product_failure"),
        ] {
            let mut c = Case::new(mode, mode, 1);
            c.invoke(expected);
            assert_eq!(c.entry(1)["scenario_verdict"], verdict);
            if expected == 0 {
                assert_eq!(
                    record(&c.output.join("casper-result.json")).unwrap()["soak_verdict"],
                    "passed"
                );
            }
            if mode == "duplicate" {
                assert_eq!(c.entry(1)["observations"].as_array().unwrap().len(), 1);
            }
        }
        for mode in [
            "fault-applied",
            "fault-not-applied",
            "fault-restart-applied",
            "fault-restart-unready",
        ] {
            let mut c = Case::new(mode, mode, 1);
            c.fault(mode.contains("restart"));
            c.invoke(
                if mode.ends_with("-applied") && mode != "fault-not-applied" {
                    0
                } else {
                    1
                },
            );
        }
        let mut c = Case::new("history", "complete", 2);
        c.invoke(0);
        let first = fs::read(c.output.join("casper-history/00000001.json")).unwrap();
        c.invoke(0);
        c.invoke(0);
        assert_eq!(
            fs::read(c.output.join("casper-history/00000001.json")).unwrap(),
            first
        );
        assert_eq!(c.entry(2)["iteration"], 2);
        let mut c = Case::new("rollback", "complete", 2);
        c.invoke(0);
        let path = c.output.join(".soak-state");
        fs::write(
            &path,
            fs::read_to_string(&path)
                .unwrap()
                .replace("ITERATIONS=1\n", "ITERATIONS=0\n"),
        )
        .unwrap();
        let before = tree(&c.output);
        c.invoke(2);
        assert_eq!(tree(&c.output), before);
        let mut c = Case::new("failure-counter", "mismatch", 2);
        c.invoke(1);
        let path = c.output.join(".soak-state");
        fs::write(
            &path,
            fs::read_to_string(&path)
                .unwrap()
                .replace("FAILURES=1\n", "FAILURES=0\n"),
        )
        .unwrap();
        c.invoke(2);
        let mut c = Case::new("resource", "failure-then-stop", 2);
        c.invoke(1);
        c.invoke(1);
        let result = record(&c.output.join("casper-result.json")).unwrap();
        assert_eq!(result["termination"], "resource_stop");
        assert!(!result["product_failures"].as_array().unwrap().is_empty());
        let mut c = Case::new("terminal", "complete", 2);
        c.invoke(0);
        fs::write(c.output.join("finalize-requested"), "cancelled\n").unwrap();
        c.invoke(0);
        assert!(!c.output.join("casper-history/00000002.json").exists());
        let mut c = Case::new("timeout", "timeout", 2);
        c.m["resource_limits"]["timeout_seconds"] = 1.into();
        c.seal();
        c.invoke(1);
        c.invoke(1);
        assert!(!c.output.join("casper-history/00000002.json").exists());
        for case in [
            "capability",
            "executable",
            "configuration",
            "policy",
            "node-kind",
            "pending-verification",
            "missing-binding",
            "open-merge",
        ] {
            let mut c = Case::new(case, "complete", 1);
            let mut expected = 2;
            match case {
                "capability" => {
                    c.m["capabilities"]["fixture-observation"]["status"] = "unknown".into();
                    expected = 3;
                }
                "executable" => fs::write(c.root.join("inputs/node_binary.sh"), "changed").unwrap(),
                "configuration" => {
                    c.env.insert("SOAK_RSS_CEILING_MB".into(), "1".into());
                }
                "policy" => c.m["policy_variant"] = "experimental".into(),
                "node-kind" => c.m["evidence_kind"] = "node_observation".into(),
                "pending-verification" => {
                    c.m["runtime"]["assets"]
                        .as_object_mut()
                        .unwrap()
                        .remove("verification");
                    expected = 0;
                }
                "missing-binding" => {
                    let mut r = record(&c.root.join("inputs/verification.json")).unwrap();
                    r["bindings"].as_object_mut().unwrap().remove("H10");
                    c.asset("verification", r);
                }
                "open-merge" => {
                    c.asset("ancestry",json!({"pr":216,"merged":false,"accepted_handoff":true,"merge_revision":"1".repeat(40),"selected_dev_revision":c.m["node_revision"]}));
                    c.m["phase"] = "post_pr216_merge".into();
                    c.m["merge_gate"] = json!({"ancestry":c.m["runtime"]["assets"]["ancestry"],"merge_revision":"1".repeat(40)});
                }
                _ => unreachable!(),
            }
            c.seal();
            c.invoke(expected);
            if case == "pending-verification" {
                assert_eq!(
                    record(&c.output.join("casper-result.json")).unwrap()["soak_verdict"],
                    "non_passing"
                );
            }
        }
        for case in [
            "corrupt-capture",
            "empty-inventory",
            "cached-success",
            "incomplete-capture",
        ] {
            let mode = if case == "cached-success" {
                "mismatch"
            } else if case == "incomplete-capture" {
                "missing-artifact"
            } else {
                "complete"
            };
            let mut c = Case::new(case, mode, 2);
            c.invoke(if mode == "complete" { 0 } else { 1 });
            if case == "corrupt-capture" {
                save(
                    &c.output.join("casper-capture/00000001/sample.json"),
                    &json!("corrupt"),
                );
            }
            if case == "empty-inventory" || case == "cached-success" {
                let mut entry = c.entry(1);
                if case == "empty-inventory" {
                    entry["artifact_inventory"] = json!([]);
                } else {
                    entry["scenario_verdict"] = "passed".into();
                    entry["product_failures"] = json!([]);
                }
                save(&c.output.join("casper-history/00000001.json"), &entry);
            }
            c.invoke(2);
        }
    }
    #[test]
    fn terminal_between_admission_and_executor() {
        let mut c = Case::new("terminal-before-exec", "complete", 1);
        let entry = c.root.join("bin/terminal-entry");
        fs::write(&entry, "#!/usr/bin/env bash\nset -eu\nif [[ \"$1\" == run ]]; then printf 'cancelled\\n' >\"$SOAK_OUTPUT_DIR/finalize-requested\"; fi\nexec \"$SOAK_TEST_REAL_HARNESS_BIN\" \"$@\"\n").unwrap();
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
        c.env.insert(
            "SOAK_TEST_REAL_HARNESS_BIN".into(),
            binary().display().to_string(),
        );
        c.env
            .insert("SOAK_HARNESS_BIN".into(), entry.display().to_string());
        c.invoke(1);
        let iteration = c.output.join("iteration-00001-docker");
        assert!(!iteration.join("launch.json").exists());
        assert!(!iteration.join("sample.json").exists());
        assert_eq!(
            record(&c.output.join("casper-result.json")).unwrap()["soak_verdict"],
            "non_passing"
        );
    }

    #[test]
    fn invalid_manifests_through_driver() {
        for kind in [
            "missing",
            "array",
            "duplicate",
            "nested-duplicate",
            "nan",
            "overflow",
            "utf8",
            "oversize",
            "boolean-schema",
            "traversal",
            "duplicate-scenarios",
            "symbolic-link",
            "fifo",
            "directory",
        ] {
            let mut c = Case::new(&format!("invalid-{kind}"), "complete", 1);
            c.env.remove("SOAK_INPUT_DIR");
            let path = c.root.join("bad-manifest.json");
            let mut m = super::manifests::sample();
            let bytes = match kind {
                "missing" => b"{}".to_vec(),
                "array" => b"[]".to_vec(),
                "duplicate" => br#"{"x":1,"x":2}"#.to_vec(),
                "nested-duplicate" => br#"{"x":{"a":1,"a":2}}"#.to_vec(),
                "nan" => br#"{"x":NaN}"#.to_vec(),
                "overflow" => br#"{"x":1e999}"#.to_vec(),
                "utf8" => vec![255],
                "oversize" => vec![b' '; MAX_BYTES as usize + 1],
                "boolean-schema" => {
                    m["schema_version"] = true.into();
                    encoded(&m).unwrap()
                }
                "traversal" => {
                    m["source_digests"] = json!({"../source.rs":"a".repeat(64)});
                    encoded(&m).unwrap()
                }
                "duplicate-scenarios" => {
                    m["required_scenarios"] = json!(["same", "same"]);
                    encoded(&m).unwrap()
                }
                _ => encoded(&m).unwrap(),
            };
            match kind {
                "symbolic-link" => {
                    std::os::unix::fs::symlink(c.root.join("manifest.json"), &path).unwrap()
                }
                "fifo" => {
                    assert!(Command::new("mkfifo")
                        .arg(&path)
                        .status()
                        .unwrap()
                        .success());
                }
                "directory" => fs::create_dir(&path).unwrap(),
                _ => fs::write(&path, &bytes).unwrap(),
            }
            c.env
                .insert("SOAK_MANIFEST_PATH".into(), path.display().to_string());
            c.invoke(2);
            assert!(!c.output.exists());
            save(
                &c.root.join("invalid-input.json"),
                &json!({"kind":kind,"argument":"bad-manifest.json","expected_exit":2}),
            );
            if kind == "fifo" || kind == "symbolic-link" {
                fs::remove_file(path).unwrap();
            }
        }
    }

    #[test]
    fn manifest_resume_through_driver() {
        let mut c = Case::new("manifest-resume", "complete", 1);
        c.env.remove("SOAK_INPUT_DIR");
        c.env.insert("SOAK_DURATION_SECONDS".into(), "1".into());
        c.m = super::manifests::sample();
        save(&c.root.join("manifest.json"), &c.m);
        fs::create_dir(&c.output).unwrap();
        manifest::bind(&c.root.join("manifest.json"), &c.output).unwrap();
        fs::write(
            c.output.join(".soak-state"),
            "STARTED_AT=1\nITERATIONS=0\nFAILURES=0\nSEGMENT=1\nMANIFEST_BOUND=1\n",
        )
        .unwrap();
        save(
            &c.output.join(".soak-checkpoint-state.json"),
            &json!({"manifest_digest":file_hash(&c.root.join("manifest.json")).unwrap()}),
        );
        c.invoke(0);
        c.invoke(0);
        let original = c.m.clone();
        for field in manifest::FIELDS {
            c.m = original.clone();
            c.m[*field] = "changed".into();
            save(&c.root.join("manifest.json"), &c.m);
            let before = tree(&c.output);
            c.invoke(2);
            assert_eq!(tree(&c.output), before);
        }
        c.m = original;
        save(&c.root.join("manifest.json"), &c.m);
        c.env.remove("SOAK_MANIFEST_PATH");
        c.invoke(2);
        fs::remove_file(c.output.join(".casper-manifest.json")).unwrap();
        c.invoke(2);
        save(&c.output.join(".casper-manifest.json"), &c.m);
        c.env.insert(
            "SOAK_MANIFEST_PATH".into(),
            c.root.join("manifest.json").display().to_string(),
        );
        for name in [".soak-state", ".soak-checkpoint-state.json"] {
            let path = c.output.join(name);
            let bytes = fs::read(&path).unwrap();
            fs::remove_file(&path).unwrap();
            c.invoke(2);
            fs::write(path, bytes).unwrap();
        }
        assert_eq!(runtime::history(&c.output).unwrap().len(), 0);
    }
}
