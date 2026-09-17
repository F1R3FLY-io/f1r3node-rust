import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

from test_casper_soak_manifest import fixture_manifest, retained_tree
from casper_soak_runtime import CORE, PROFILES


ROOT = Path(__file__).resolve().parents[2]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def save(path, value):
    data = (json.dumps(value, sort_keys=True) + "\n").encode()
    path.write_bytes(data)
    return data


class RuntimeTests(unittest.TestCase):
    def setUp(self):
        evidence = os.environ.get("SOAK_TEST_ARTIFACTS")
        if evidence:
            self.root = Path(evidence) / self._testMethodName
            self.root.mkdir(parents=True, exist_ok=False)
        else:
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            self.root = Path(temporary.name)
        self.inputs = self.root / "inputs"
        self.inputs.mkdir()
        self.output = self.root / "output"
        (self.root / "suite").mkdir()
        (self.root / "bin").mkdir()
        docker = self.root / "bin/docker"
        docker.write_text("#!/bin/sh\nexit 0\n")
        docker.chmod(0o755)
        self.calls = 0

    def prepare(self, mode="complete", iterations=1, verified=True):
        manifest = fixture_manifest()
        manifest.update(profile_id="harness-lifecycle", provider="docker", run_id=self._testMethodName)
        manifest["source_digests"] = {path: sha((ROOT / path).read_bytes()) for path in (*CORE, PROFILES["harness-lifecycle"])}
        manifest["profile_digest"] = manifest["source_digests"][PROFILES["harness-lifecycle"]]
        manifest["tool_versions"] = {"python": sys.version.split()[0]}
        manifest["deadline"] = {"clock_id": "unix-seconds", "epoch_seconds": str(int(time.time()) + 120)}
        manifest["resource_limits"] = {"children": 1, "timeout_seconds": 30, "rss_ceiling_mb": 0, "host_free_floor_mb": 0, "disk_free_floor_mb": 0, "artifact_bytes": 1048576}
        self.environment = os.environ.copy()
        self.environment.update({"PATH": str(self.root / "bin") + os.pathsep + os.environ["PATH"], "SYSTEM_INTEGRATION_DIR": str(self.root / "suite"), "SOAK_OUTPUT_DIR": str(self.output), "SOAK_INPUT_DIR": str(self.inputs), "SOAK_DURATION_SECONDS": "120", "SOAK_RSS_CEILING_MB": "0", "SOAK_HOST_FREE_FLOOR_MB": "0", "SOAK_DISK_FREE_FLOOR_MB": "0", "SOAK_RUN_BENCHMARKS": "false", "SOAK_GUARDIAN_POLL_SECONDS": "0.05", "SOAK_MANIFEST_PATH": str(self.root / "manifest.json"), "SOAK_APPROVAL_PATH": str(self.root / "approval.json")})
        configuration = {"duration_seconds": 120, "rss_ceiling_mb": 0, "host_free_floor_mb": 0, "disk_free_floor_mb": 0, "policy_variant": "baseline"}
        request = {"scenario_id": "fixture-scenario", "pair_id": "pair-1", "seed": "1", "expected": 0, "mode": mode, "fault_schedule": [], "required_observations": [{"event_kind": "sample", "node": "validator-1", "incarnation": "incarnation-1", "member_id": "baseline"}]}
        assets = {}
        payloads = {"configuration": configuration, "fixture": {"seed": "1"}, "expectation": {"value": 0}, "request": request, "image_manifest": {"schemaVersion": 2}, "qualification": {"capability": "fixture-observation", "provider": "docker", "node_revision": manifest["node_revision"], "status": "qualified", "evidence_kind": "synthetic_fixture"}}
        for name, value in payloads.items():
            path = self.inputs / (name + ".json")
            raw = save(path, value)
            assets[name] = {"path": path.name, "bytes": len(raw), "sha256": sha(raw)}
        for name, raw in (("node_binary", b"synthetic binary identity\n"), ("executor", (ROOT / "scripts/bench/fixtures/casper_lifecycle_executor.py").read_bytes())):
            path = self.inputs / (name + ".py")
            path.write_bytes(raw)
            assets[name] = {"path": path.name, "bytes": len(raw), "sha256": sha(raw)}
        for role, field in (("configuration", "configuration_digest"), ("fixture", "fixture_digest"), ("expectation", "expectation_digest"), ("node_binary", "node_binary_digest"), ("image_manifest", "image_digest")):
            manifest[field] = assets[role]["sha256"]
        manifest["capabilities"] = {"fixture-observation": {"status": "qualified", "provider": "docker", "revision": manifest["node_revision"], "qualification": assets["qualification"]}}
        manifest["runtime"] = {"iterations": iterations, "iterations_per_segment": 1, "required_capabilities": ["fixture-observation"], "assets": assets}
        if verified:
            receipt = {"origin": "fixture-substitute", "status": "passed", "phase": manifest["phase"], "claim_id": "CLAIM-CASPER-SOAK-001", "construction": "not-applicable", "source_digests": manifest["source_digests"], "bindings": {f"H{number:02d}": True for number in range(1, 11)}}
            raw = save(self.inputs / "verification.json", receipt)
            assets["verification"] = {"path": "verification.json", "bytes": len(raw), "sha256": sha(raw)}
        self.manifest = manifest
        self.reseal()

    def reseal(self):
        raw = save(self.root / "manifest.json", self.manifest)
        assets = self.manifest["runtime"]["assets"]
        approval = {"approved": True, "manifest_digest": sha(raw), "evidence_kind": "synthetic_fixture", "qualification_digests": [ref["sha256"] for key, ref in assets.items() if key in ("qualification", "verification", "ancestry")], "executor_digest": assets["executor"]["sha256"]}
        self.environment["SOAK_APPROVAL_SHA256"] = sha(save(self.root / "approval.json", approval))

    def invoke(self, expected):
        self.calls += 1
        before = retained_tree(self.output) if self.output.exists() else {}
        command = ["bash", str(ROOT / "scripts/run-merge-recovery-soak.sh")]
        completed = subprocess.run(command, env=self.environment, capture_output=True, text=True, timeout=20)
        record = {"command": ["bash", "scripts/run-merge-recovery-soak.sh"], "expected_exit": expected, "actual_exit": completed.returncode, "stdout": completed.stdout.replace(str(self.root), "[CASE]"), "stderr": completed.stderr.replace(str(self.root), "[CASE]"), "before": before, "after": retained_tree(self.output) if self.output.exists() else {}, "manifest_sha256": sha((self.root / "manifest.json").read_bytes())}
        save(self.root / f"invocation-{self.calls:02d}.json", record)
        self.assertEqual(completed.returncode, expected, completed.stdout + completed.stderr)
        return record

    def entry(self, number=1):
        return json.loads((self.output / "casper-history" / f"{number:08d}.json").read_text())

    def test_complete_profile_uses_real_dispatch(self):
        self.prepare()
        self.invoke(0)
        entry = self.entry()
        self.assertEqual(entry["scenario_verdict"], "passed")
        self.assertTrue(entry["capture_complete"])
        self.assertEqual(entry["observations"][0]["payload"], 0)
        self.assertTrue((self.output / "iteration-00001-docker/launch.json").is_file())
        result = json.loads((self.output / "casper-result.json").read_text())
        self.assertEqual(result["soak_verdict"], "passed")
        self.assertEqual(result["evidence_kind"], "synthetic_fixture")

    def test_history_survives_two_segments(self):
        self.prepare(iterations=2)
        self.invoke(0)
        first = retained_tree(self.output / "casper-capture")
        history = (self.output / "casper-history/00000001.json").read_bytes()
        self.invoke(0)
        self.assertEqual((self.output / "casper-history/00000001.json").read_bytes(), history)
        current = retained_tree(self.output / "casper-capture")
        self.assertTrue(all(current[key] == value for key, value in first.items()))
        self.assertEqual(self.entry(2)["iteration"], 2)
        self.invoke(0)
        self.assertEqual(len(list(self.output.glob("iteration-*"))), 2)

    def test_checkpoint_rollback_cannot_overwrite_history(self):
        self.prepare(iterations=2)
        self.invoke(0)
        state = self.output / ".soak-state"
        state.write_text(state.read_text().replace("ITERATIONS=1\n", "ITERATIONS=0\n"))
        checkpoint = self.output / ".soak-checkpoint-state.json"
        data = json.loads(checkpoint.read_text())
        data["iterations"] = 0
        save(checkpoint, data)
        before = retained_tree(self.output)
        self.invoke(2)
        self.assertEqual(retained_tree(self.output), before)

    def test_failure_survives_resource_stop(self):
        self.prepare("failure-then-stop", iterations=2)
        self.invoke(1)
        self.assertEqual(self.entry()["scenario_verdict"], "product_failure")
        self.invoke(1)
        result = json.loads((self.output / "casper-result.json").read_text())
        self.assertEqual(result["termination"], "resource_stop")
        self.assertTrue(result["product_failures"])
        self.assertEqual(result["soak_verdict"], "non_passing")

    def test_unknown_capability_prevents_launch(self):
        self.prepare()
        self.manifest["capabilities"]["fixture-observation"]["status"] = "unknown"
        self.reseal()
        self.invoke(3)
        self.assertFalse(self.output.exists())

    def test_executable_mutation_prevents_launch(self):
        self.prepare()
        (self.inputs / "node_binary.py").write_bytes(b"changed")
        self.invoke(2)
        self.assertFalse(self.output.exists())

    def test_effective_configuration_mismatch_prevents_launch(self):
        self.prepare()
        self.environment["SOAK_RSS_CEILING_MB"] = "1"
        self.invoke(2)
        self.assertFalse(self.output.exists())

    def test_missing_observation_is_not_zero(self):
        self.prepare("missing")
        self.invoke(1)
        self.assertEqual(self.entry()["scenario_verdict"], "incomplete")
        self.assertTrue(self.entry()["missing_observations"])

    def test_missing_presence_cannot_carry_zero(self):
        self.prepare("missing-zero")
        self.invoke(1)
        self.assertTrue(self.entry()["rejected_observations"])

    def test_wrong_incarnation_is_rejected(self):
        self.prepare("wrong-identity")
        self.invoke(1)
        self.assertTrue(self.entry()["rejected_observations"])

    def test_copies_do_not_inflate_counts(self):
        self.prepare("duplicate")
        self.invoke(0)
        self.assertEqual(len(self.entry()["observations"]), 1)

    def test_missing_artifact_cannot_pass(self):
        self.prepare("missing-artifact")
        self.invoke(1)
        self.assertNotEqual(self.entry()["scenario_verdict"], "passed")
        self.assertFalse(self.entry()["capture_complete"])

    def test_pending_verification_cannot_publish_success(self):
        self.prepare(verified=False)
        self.invoke(0)
        result = json.loads((self.output / "casper-result.json").read_text())
        self.assertEqual(result["harness_verification"], "pending")
        self.assertEqual(result["soak_verdict"], "non_passing")

    def test_failure_counter_cannot_decrease(self):
        self.prepare("mismatch", iterations=2)
        self.invoke(1)
        state = self.output / ".soak-state"
        state.write_text(state.read_text().replace("FAILURES=1\n", "FAILURES=0\n"))
        before = retained_tree(self.output)
        self.invoke(2)
        self.assertEqual(retained_tree(self.output), before)

    def test_terminal_marker_prevents_next_launch(self):
        self.prepare(iterations=2)
        self.invoke(0)
        (self.output / "finalize-requested").write_text("Synthetic cancellation.\n")
        self.invoke(0)
        self.assertEqual(len(list(self.output.glob("iteration-*"))), 1)
        result = json.loads((self.output / "casper-result.json").read_text())
        self.assertEqual(result["termination"], "cancelled")
        self.assertEqual(result["soak_verdict"], "non_passing")

    def test_policy_cannot_reuse_baseline_configuration(self):
        self.prepare()
        self.manifest["policy_variant"] = "experimental"
        self.reseal()
        self.invoke(2)
        self.assertFalse(self.output.exists())

    def test_open_candidate_does_not_satisfy_merge_gate(self):
        self.prepare()
        proof = {"pr": 216, "merged": False, "accepted_handoff": True, "merge_revision": "1" * 40, "selected_dev_revision": self.manifest["node_revision"]}
        raw = save(self.inputs / "ancestry.json", proof)
        reference = {"path": "ancestry.json", "bytes": len(raw), "sha256": sha(raw)}
        self.manifest["runtime"]["assets"]["ancestry"] = reference
        self.manifest["phase"] = "post_pr216_merge"
        self.manifest["merge_gate"] = {"ancestry": reference, "merge_revision": proof["merge_revision"]}
        self.reseal()
        self.invoke(2)
        self.assertFalse(self.output.exists())

    def test_capture_must_remain_hash_valid(self):
        self.prepare(iterations=2)
        self.invoke(0)
        (self.output / "casper-capture/00000001/sample.json").write_text("corrupted\n")
        before = retained_tree(self.output)
        self.invoke(2)
        self.assertEqual(retained_tree(self.output), before)

    def test_cached_success_cannot_override_raw_failure(self):
        self.prepare("mismatch")
        self.invoke(1)
        path = self.output / "casper-history/00000001.json"
        entry = self.entry()
        entry["scenario_verdict"] = "passed"
        entry["product_failures"] = []
        save(path, entry)
        self.invoke(2)

    def test_incomplete_capture_prevents_cleanup(self):
        self.prepare("missing-artifact", iterations=2)
        self.invoke(1)
        retained = retained_tree(self.output)
        self.invoke(2)
        self.assertEqual(retained_tree(self.output), retained)


if __name__ == "__main__":
    unittest.main()
