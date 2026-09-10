import copy
import hashlib
import importlib.util
import json
import os
import random
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("qualification.py")
SPEC = importlib.util.spec_from_file_location("qualification", MODULE_PATH)
qualification = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(qualification)


class QualificationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(dir=os.environ["TMPDIR"])
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.identity = {
            "target_ref": "feature/candidate",
            "target_sha": "a" * 40,
            "control_sha": "b" * 40,
            "harness_sha": "c" * 40,
            "image_id": "sha256:" + "d" * 64,
            "binary_sha256": "e" * 64,
            "configuration_sha256": "f" * 64,
            "seeds": [23, 101],
            "run_id": "12345",
            "run_attempt": 1,
            "session_id": "12345678-1234-4234-9234-123456789abc",
            "boot_id": "22345678-1234-4234-9234-123456789abc",
        }
        self.configuration = self.artifact(
            "configuration.json", '{"workload": "merge-recovery"}'
        )
        self.identity["configuration_sha256"] = self.configuration["sha256"]
        self.artifacts = [self.iteration_artifacts(1), self.iteration_artifacts(2)]
        self.preflight = self.artifact("preflight.xml", self.junit())

    @staticmethod
    def junit(failed=False, skipped=False):
        result = "<failure/>" if failed else "<skipped/>" if skipped else ""
        return (
            f'<testsuites><testsuite tests="1" failures="{int(failed)}" errors="0" skipped="{int(skipped)}">'
            f'<testcase classname="custom.test_load" name="merge_recovery">{result}</testcase>'
            "</testsuite></testsuites>"
        )

    def artifact(self, name, contents):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
        return {"path": name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

    def iteration_artifacts(self, number):
        provider = "docker" if number % 2 else "subprocess"
        prefix = f"iteration-{number}"
        return {
            "junit": self.artifact(f"{prefix}/junit.xml", self.junit()),
            "log": self.artifact(f"{prefix}/pytest.log", "1 passed\n"),
            "metrics": self.artifact(
                f"{prefix}/metrics.json",
                json.dumps(
                    {
                        "iteration": number,
                        "provider": provider,
                        "exit_code": 0,
                        "ok": True,
                        "pytest": {"passed": 1, "failed": 0, "errors": 0, "skipped": 0},
                        "rss_peak_mb": 1500,
                        "cpu_peak_pct": 75,
                    }
                ),
            ),
        }

    def event(self, kind, at, **fields):
        return {
            "kind": kind,
            "at_ns": at,
            "identity": copy.deepcopy(self.identity),
            **fields,
        }

    def trace(self, durations=None, idle=0):
        durations = durations or [43_200_000_000_000, 43_200_000_000_000]
        events = [self.event("start", 10)]
        at = 10
        for number, duration in enumerate(durations, 1):
            at += idle
            provider = "docker" if number % 2 else "subprocess"
            events.append(
                self.event(
                    "iteration_start",
                    at,
                    iteration=number,
                    provider=provider,
                    seed=self.identity["seeds"][
                        (number - 1) % len(self.identity["seeds"])
                    ],
                )
            )
            at += duration
            events.append(
                self.event(
                    "iteration_end",
                    at,
                    iteration=number,
                    exit_code=0,
                    artifacts=copy.deepcopy(self.artifacts[number - 1]),
                )
            )
        events.append(self.event("finish", at, outcome="completed"))
        return {
            "schema_version": 1,
            "scope": "merge-recovery-iterations",
            "required_seconds": 86400,
            "identity": copy.deepcopy(self.identity),
            "configuration": copy.deepcopy(self.configuration),
            "preflight": {"outcome": "passed", "junit": self.preflight},
            "events": events,
        }

    def verify(self, trace, conclusion="success"):
        return qualification.verify(trace, self.root, conclusion, self.identity)

    def assert_rejected(self, trace, conclusion="success"):
        with self.assertRaises(qualification.InvalidEvidence):
            self.verify(trace, conclusion)

    def test_exact_duration_and_normal_terminal_evidence_qualify(self):
        result = self.verify(self.trace())
        self.assertTrue(result["qualified"])
        self.assertEqual(result["qualified_ns"], 86_400_000_000_000)
        self.assertEqual(result["iterations"], 2)
        self.assertEqual(result["providers"], ["docker", "subprocess"])
        self.assertEqual(len(result["artifacts"]), 8)

    def test_generated_integer_duration_boundaries(self):
        random_source = random.Random(2167520)
        for _ in range(300):
            threshold = 86_400_000_000_000
            left = random_source.randrange(1, threshold)
            for offset in (-1, 0, 1):
                trace = self.trace([left, threshold - left + offset])
                if offset < 0:
                    self.assert_rejected(trace)
                else:
                    self.assertEqual(
                        self.verify(trace)["qualified_ns"], threshold + offset
                    )

    def test_idle_elapsed_time_cannot_fund_qualification(self):
        self.assert_rejected(self.trace([1, 1], idle=86_400_000_000_000))

    def test_arbitrary_precision_duration_does_not_overflow(self):
        trace = self.trace([2**80, 2**80])
        self.assertEqual(self.verify(trace)["qualified_ns"], 2**81)

    def test_each_identity_field_must_remain_fixed_at_every_boundary(self):
        for field in self.identity:
            for position in range(6):
                trace = self.trace()
                observed = trace["events"][position]["identity"]
                observed[field] = [8] if field == "seeds" else "changed"
                self.assert_rejected(trace)

    def test_changed_then_restored_identity_remains_rejected(self):
        trace = self.trace()
        trace["events"][2]["identity"]["target_sha"] = "9" * 40
        self.assert_rejected(trace)

    def test_consistent_substitution_cannot_replace_the_requested_identity(self):
        trace = self.trace()
        trace["identity"]["target_sha"] = "9" * 40
        for event in trace["events"]:
            event["identity"]["target_sha"] = "9" * 40
        self.assert_rejected(trace)

    def test_numeric_equality_cannot_hide_invalid_observation_types(self):
        trace = self.trace()
        trace["events"][2]["identity"]["run_attempt"] = True
        self.assert_rejected(trace)
        trace = self.trace()
        trace["events"][2]["identity"]["seeds"] = [23.0, 101.0]
        self.assert_rejected(trace)

    def test_missing_or_changed_configuration_cannot_qualify(self):
        trace = self.trace()
        trace["configuration"] = self.artifact(
            "other-configuration.json", '{"workload": "other"}'
        )
        self.assert_rejected(trace)
        trace = self.trace()
        del trace["configuration"]
        self.assert_rejected(trace)

    def test_each_missing_event_and_duplicate_event_is_rejected(self):
        original = self.trace()
        for index in range(len(original["events"])):
            trace = copy.deepcopy(original)
            del trace["events"][index]
            self.assert_rejected(trace)
            trace = copy.deepcopy(original)
            trace["events"].insert(index, copy.deepcopy(trace["events"][index]))
            self.assert_rejected(trace)

    def test_any_failure_or_cancellation_cannot_be_hidden_by_suffix(self):
        for kind in (
            "cancel",
            "restart",
            "failure",
            "input_changed",
            "protection_breach",
        ):
            for index in range(1, 6):
                trace = self.trace()
                trace["events"].insert(
                    index, self.event(kind, trace["events"][index]["at_ns"])
                )
                self.assert_rejected(trace)

    def test_no_work_never_qualifies(self):
        trace = self.trace()
        trace["events"] = [trace["events"][0], trace["events"][-1]]
        self.assert_rejected(trace)

    def test_cancelled_failed_skipped_or_unknown_job_never_qualifies(self):
        for outcome in ("cancelled", "failure", "skipped", "timed_out", "", None, True):
            self.assert_rejected(self.trace(), outcome)

    def test_early_finish_and_post_finish_events_are_rejected(self):
        for outcome in ("finalized", "deadline", "interrupted", "skipped"):
            trace = self.trace()
            trace["events"][-1]["outcome"] = outcome
            self.assert_rejected(trace)
        trace = self.trace()
        trace["events"].append(self.event("cancel", trace["events"][-1]["at_ns"]))
        self.assert_rejected(trace)

    def test_missing_failed_skipped_or_empty_preflight_is_rejected(self):
        for preflight in ({}, {"outcome": "skipped"}, {"outcome": "failed"}):
            trace = self.trace()
            trace["preflight"] = preflight
            self.assert_rejected(trace)
        for report in (
            self.junit(failed=True),
            self.junit(skipped=True),
            "<testsuite tests='0'/>",
            "<testsuites/>",
        ):
            trace = self.trace()
            trace["preflight"]["junit"] = self.artifact("bad-preflight.xml", report)
            self.assert_rejected(trace)

    def test_shortened_policy_and_rerun_attempt_are_rejected(self):
        for seconds in (0, 79200, 86399, 86401, "86400", True):
            trace = self.trace()
            trace["required_seconds"] = seconds
            self.assert_rejected(trace)
        trace = self.trace()
        trace["identity"]["run_attempt"] = 2
        for event in trace["events"]:
            event["identity"]["run_attempt"] = 2
        self.assert_rejected(trace)

    def test_negative_boolean_fractional_or_reversed_time_is_rejected(self):
        for value in (-1, True, 1.5, "99", None):
            trace = self.trace()
            trace["events"][2]["at_ns"] = value
            self.assert_rejected(trace)
        trace = self.trace()
        trace["events"][3]["at_ns"] = trace["events"][2]["at_ns"] - 1
        self.assert_rejected(trace)

    def test_provider_seed_index_and_exit_must_match(self):
        for field, value in (("iteration", 2), ("seed", 42), ("provider", "unknown")):
            trace = self.trace()
            trace["events"][1][field] = value
            self.assert_rejected(trace)
        for code in (1, 124, 137, True, "0"):
            trace = self.trace()
            trace["events"][2]["exit_code"] = code
            self.assert_rejected(trace)

    def test_report_failure_skip_empty_and_false_summary_are_rejected(self):
        for report in (
            self.junit(failed=True),
            self.junit(skipped=True),
            "<testsuites/>",
            self.junit().replace('tests="1"', 'tests="2"'),
            self.junit()
            .replace("<testsuite ", '<testsuite unexpected="1" ')
            .replace('tests="1"', 'tests="0"'),
        ):
            trace = self.trace()
            trace["events"][2]["artifacts"]["junit"] = self.artifact("bad.xml", report)
            self.assert_rejected(trace)

    def test_changed_missing_empty_and_reused_artifacts_are_rejected(self):
        trace = self.trace()
        (self.root / "iteration-1/pytest.log").write_text("changed\n")
        self.assert_rejected(trace)
        self.artifacts[0] = self.iteration_artifacts(1)
        trace = self.trace()
        (self.root / "iteration-1/pytest.log").unlink()
        self.assert_rejected(trace)
        self.artifacts[0] = self.iteration_artifacts(1)
        trace = self.trace()
        trace["events"][4]["artifacts"]["log"] = trace["events"][2]["artifacts"]["log"]
        self.assert_rejected(trace)
        trace = self.trace()
        trace["events"][2]["artifacts"]["log"] = self.artifact("empty.log", "")
        self.assert_rejected(trace)

    def test_symbolic_links_and_escape_paths_are_rejected(self):
        trace = self.trace()
        original = trace["events"][2]["artifacts"]["log"]
        (self.root / "alias.log").symlink_to(self.root / original["path"])
        for path in (
            "alias.log",
            "../outside.log",
            "/outside.log",
            "iteration-1/../iteration-1/pytest.log",
        ):
            altered = copy.deepcopy(trace)
            altered["events"][2]["artifacts"]["log"]["path"] = path
            self.assert_rejected(altered)

    def test_metrics_must_correspond_to_successful_measured_iteration(self):
        reference = self.artifacts[0]["metrics"]
        original = json.loads((self.root / reference["path"]).read_text())
        for field, value in (
            ("iteration", 2),
            ("provider", "subprocess"),
            ("exit_code", 1),
            ("ok", False),
            ("rss_peak_mb", None),
            ("cpu_peak_pct", None),
            ("pytest", {"passed": 0, "failed": 0, "errors": 0, "skipped": 0}),
        ):
            trace = self.trace()
            metrics = {**original, field: value}
            trace["events"][2]["artifacts"]["metrics"] = self.artifact(
                "bad-metrics.json", json.dumps(metrics)
            )
            self.assert_rejected(trace)

    def test_json_duplicate_keys_and_nonfinite_numbers_are_rejected(self):
        for contents in (
            '{"ok": true, "ok": false}',
            '{"value": NaN}',
            '{"value": Infinity}',
        ):
            with self.assertRaises(qualification.InvalidEvidence):
                qualification.parse_json(contents)

    def test_cli_success_and_failure_manifests_bind_actual_artifacts(self):
        trace_path = self.root / "trace.json"
        identity_path = self.root / "identity.json"
        manifest_path = self.root / "qualification.json"
        trace_path.write_text(json.dumps(self.trace()))
        identity_path.write_text(json.dumps(self.identity))
        command = [
            sys.executable,
            str(MODULE_PATH),
            str(trace_path),
            "--expected-identity",
            str(identity_path),
            "--evidence-root",
            str(self.root),
            "--output",
            str(manifest_path),
            "--workflow-conclusion",
            "success",
        ]
        result = subprocess.run(
            command, capture_output=True, text=True, timeout=10, check=False
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(json.loads(manifest_path.read_text())["qualified"])
        (self.root / "iteration-1/pytest.log").write_text(
            "altered after qualification\n"
        )
        result = subprocess.run(
            command, capture_output=True, text=True, timeout=10, check=False
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertFalse(json.loads(manifest_path.read_text())["qualified"])
        self.assertIn(
            "digest mismatch", json.loads(manifest_path.read_text())["reason"]
        )

    def test_cli_requires_external_conclusion_and_identity(self):
        result = subprocess.run(
            [sys.executable, str(MODULE_PATH), "missing.json"],
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(result.returncode, 2)

    def test_wrapper_counts_cannot_contradict_test_cases(self):
        trace = self.trace()
        report = self.junit().replace("<testsuites>", '<testsuites tests="0">')
        trace["events"][2]["artifacts"]["junit"] = self.artifact(
            "bad-wrapper.xml", report
        )
        self.assert_rejected(trace)


if __name__ == "__main__":
    unittest.main()
