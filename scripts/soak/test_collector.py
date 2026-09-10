import contextlib
import io
import os
import sys
import unittest
from unittest.mock import patch

import collect_qualification as collector
import test_qualification as fixtures
from qualification import InvalidEvidence, verify


class CollectorTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.QualificationTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        self.node = self.root / "node"
        self.harness = self.root / "harness"
        for path, content in (
            (self.node / "node/src/main/resources/defaults.conf", "defaults"),
            (self.harness / collector.LOAD_TEST, "load test"),
            (self.harness / "poetry.lock", "lock"),
            (self.root / "node.bin", "node binary"),
            (self.root / "preflight/junit.xml", self.fixture.junit()),
        ):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        environment = {
            "PATH": os.environ["PATH"],
            "SOAK_NODE_REPO_DIR": str(self.node),
            "SYSTEM_INTEGRATION_DIR": str(self.harness),
            "SOAK_OUTPUT_DIR": str(self.root),
            "SOAK_TARGET_REF": "feature/candidate",
            "SOAK_TARGET_SHA": "a" * 40,
            "SOAK_CONTROL_SHA": "b" * 40,
            "SYSTEM_INTEGRATION_REF": "c" * 40,
            "SOAK_QUALIFICATION_IMAGE": "sha256:" + "d" * 64,
            "F1R3FLY_NODE_BINARY": str(self.root / "node.bin"),
            "SOAK_QUALIFICATION_SEEDS": "[216]",
            "GITHUB_RUN_ID": "12345",
            "GITHUB_RUN_ATTEMPT": "1",
            "SOAK_QUALIFICATION_OWNER_PID": str(os.getpid()),
            "SOAK_PREFLIGHT_RESULT": "passed",
        }
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, environment, clear=True).start()
        self.subject = collector.Collector(self.root)
        revisions = {
            self.node: "a" * 40,
            self.subject.control: "b" * 40,
            self.harness: "c" * 40,
        }
        patch.object(
            collector, "git_revision", side_effect=lambda path: revisions[path]
        ).start()
        self.remote = patch.object(
            collector, "resolve_reference", return_value="a" * 40
        ).start()
        self.docker = patch.object(
            collector, "command", return_value="sha256:" + "d" * 64
        ).start()
        self.now = 10
        patch.object(
            collector.time, "monotonic_ns", side_effect=lambda: self.now
        ).start()

    def complete(self):
        self.subject.prepare()
        self.subject.start()
        for index, provider in enumerate(("docker", "subprocess"), 1):
            self.assertEqual(self.subject.begin(index, provider), 216)
            self.now += 43_200_000_000_000
            credit = self.subject.end(
                index, self.root / f"iteration-{index}", 0, self.now
            )
            self.assertEqual(credit, index * 43_200_000_000_000)
        self.subject.finish()

    def test_collector_to_verifier_with_real_files_and_injected_clock(self):
        self.complete()
        trace = self.subject.read("trace.json")
        result = verify(trace, self.root, "success", self.subject.read("identity.json"))
        self.assertTrue(result["qualified"])
        self.assertEqual(result["qualified_ns"], 86_400_000_000_000)
        self.assertFalse(self.subject.read("local-result.json")["qualified"])
        self.assertTrue(self.subject.read("local-result.json")["trace_complete"])
        self.assertEqual(
            self.subject.read("local-result.json")["verdicts"]["workflow"],
            "unconfirmed",
        )

    def test_prepare_rejects_source_reference_movement(self):
        self.remote.return_value = "9" * 40
        with self.assertRaisesRegex(InvalidEvidence, "reference moved"):
            self.subject.prepare()

    def test_effective_configuration_or_binary_change_is_rejected(self):
        self.subject.prepare()
        self.subject.start()
        (self.root / "node.bin").write_text("changed binary")
        with self.assertRaisesRegex(InvalidEvidence, "inputs changed"):
            self.subject.begin(1, "docker")
        (self.root / "node.bin").write_text("node binary")
        os.environ["SOAK_RSS_CEILING_MB"] = "1"
        with self.assertRaisesRegex(InvalidEvidence, "inputs changed"):
            self.subject.begin(1, "docker")

    def test_seed_or_image_change_is_rejected(self):
        self.subject.prepare()
        self.subject.start()
        os.environ["SOAK_QUALIFICATION_SEEDS"] = "[217]"
        with self.assertRaisesRegex(InvalidEvidence, "inputs changed"):
            self.subject.begin(1, "docker")
        os.environ["SOAK_QUALIFICATION_SEEDS"] = "[216]"
        self.docker.return_value = "sha256:" + "9" * 64
        with self.assertRaisesRegex(InvalidEvidence, "inputs changed"):
            self.subject.begin(1, "docker")

    def test_noop_preflight_resume_duplicate_begin_and_early_finish_are_rejected(self):
        self.subject.prepare()
        os.environ["SOAK_PREFLIGHT_RESULT"] = "skipped"
        with self.assertRaises(InvalidEvidence):
            self.subject.start()
        os.environ["SOAK_PREFLIGHT_RESULT"] = "passed"
        self.subject.start()
        for method in (self.subject.prepare, self.subject.start, self.subject.finish):
            with self.assertRaises(InvalidEvidence):
                method()
        self.subject.begin(1, "docker")
        with self.assertRaises(InvalidEvidence):
            self.subject.begin(1, "docker")

    def test_driver_replacement_cannot_reuse_a_session(self):
        self.subject.prepare()
        self.subject.start()
        owner = self.subject.read("owner.json")
        owner["start_ticks"] = "0"
        collector.atomic_write(self.subject.directory / "owner.json", owner)
        with self.assertRaisesRegex(InvalidEvidence, "original driver"):
            self.subject.begin(1, "docker")

    def test_cli_failure_is_permanent_after_inputs_are_restored(self):
        self.subject.prepare()
        self.subject.start()
        self.remote.return_value = "9" * 40
        arguments = [
            "collect_qualification.py",
            "begin",
            "--iteration",
            "1",
            "--provider",
            "docker",
        ]
        with (
            patch.object(sys, "argv", arguments),
            contextlib.redirect_stderr(io.StringIO()),
        ):
            self.assertEqual(collector.main(), 1)
            self.remote.return_value = "a" * 40
            self.assertEqual(collector.main(), 1)
        self.assertFalse(self.subject.read("rejected.json")["qualified"])
        self.assertIn("reference moved", self.subject.read("rejected.json")["reason"])

    def test_markers_prevent_normal_completion(self):
        self.complete()
        for name in (
            "early-exit.txt",
            "host-guardian-breach.txt",
            "protection-breach.txt",
            "finalize-requested",
        ):
            marker = self.root / name
            marker.write_text("interrupted")
            with self.assertRaisesRegex(InvalidEvidence, "interrupted run"):
                self.subject.finish()
            marker.unlink()

    def test_telemetry_only_and_selection_override_cannot_qualify(self):
        for name in ("LOAD_TEST_TELEMETRY_ONLY", "PYTEST_ADDOPTS"):
            os.environ[name] = "1"
            with self.assertRaises(InvalidEvidence):
                self.subject.prepare()
            del os.environ[name]


if __name__ == "__main__":
    unittest.main()
