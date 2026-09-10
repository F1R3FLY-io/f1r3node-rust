import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from collections import Counter
from pathlib import Path

import test_qualification as fixtures
import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/merge-recovery-soak.yml"


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(dir=os.environ["TMPDIR"])
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.workflow = yaml.load(WORKFLOW.read_text(), Loader=yaml.BaseLoader)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        fake_gh = self.bin / "gh"
        fake_gh.write_text("#!/usr/bin/env bash\nprintf '%s\\n' '" + "a" * 40 + "'\n")
        fake_gh.chmod(0o755)

    def schedule(self, duration="qualification-24h", **overrides):
        step = self.workflow["jobs"]["schedule_gate"]["steps"][0]
        environment = dict(os.environ)
        environment.update(dict.fromkeys(step["env"], ""))
        environment.update(
            {
                "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
                "INPUT_DURATION": duration,
                "INPUT_TARGET_REF": "feature/candidate",
                "EVENT_NAME": "workflow_dispatch",
                "GITHUB_RUN_ATTEMPT": "1",
                "GITHUB_RUN_ID": "12345",
                "GITHUB_REPOSITORY": "example/node",
                "GITHUB_OUTPUT": str(self.root / "output"),
                "GITHUB_STEP_SUMMARY": str(self.root / "summary"),
                "RUNNER_TEMP": str(self.root),
            }
        )
        environment.update(overrides)
        (self.root / "output").write_text("")
        result = subprocess.run(
            ["bash", "-euo", "pipefail", "-c", step["run"]],
            env=environment,
            cwd=self.root,
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        )
        outputs = dict(
            line.split("=", 1)
            for line in (self.root / "output").read_text().splitlines()
        )
        return result, outputs

    def test_manual_qualification_has_full_duration_and_no_checkpoints(self):
        result, outputs = self.schedule()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(outputs["duration_seconds"], "86400")
        self.assertEqual(outputs["qualification"], "true")
        self.assertEqual(outputs["kind"], "qualification")
        self.assertEqual(outputs["should_run"], "true")
        self.assertEqual(outputs["retry_attempt"], "0")
        self.assertEqual(outputs["run_benchmarks"], "false")
        for index in range(1, 6):
            self.assertEqual(outputs[f"checkpoint_{index}"], "")

    def test_qualification_rejects_all_shortening_and_restart_modes(self):
        cases = {
            "EVENT_NAME": "schedule",
            "INPUT_SCHEDULED_SLOT": "1",
            "INPUT_WINDOW_END": "1",
            "INPUT_SERIES": "daily",
            "INPUT_RETRY_ATTEMPT": "1",
            "GITHUB_RUN_ATTEMPT": "2",
            "INPUT_CANARY": "true",
            "INPUT_PREFLIGHT_ONLY": "true",
            "INPUT_SKIP_PREFLIGHT": "true",
            "INPUT_INJECT_PROTECTION_BREACH": "true",
            "INPUT_CANDIDATE_TAG": "v1.0.0-canary.1",
            "INPUT_RESTART_OF_RUN_ID": "999",
        }
        for key, value in cases.items():
            result, _ = self.schedule(**{key: value})
            self.assertNotEqual(result.returncode, 0, key)

    def test_existing_daily_and_weekend_durations_remain_unchanged(self):
        for duration, seconds in (("daily-24h", "79200"), ("weekend-60h", "216000")):
            result, outputs = self.schedule(duration)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(outputs["duration_seconds"], seconds)
            self.assertEqual(outputs["qualification"], "false")

    def test_qualification_cannot_retry_or_enter_scheduled_dashboard(self):
        for name in ("retry_within_window", "perf_report"):
            self.assertIn(
                "needs.schedule_gate.outputs.qualification != 'true'",
                self.workflow["jobs"][name]["if"],
            )
        final = self.workflow["jobs"]["qualification_gate"]
        self.assertIn("always()", final["if"])
        self.assertIn("inputs.duration == 'qualification-24h'", final["if"])
        validation = next(step for step in final["steps"] if "run" in step)
        self.assertEqual(validation["env"]["SOAK_RESULT"], "${{ needs.soak.result }}")

    def test_workflow_terminal_gate_runs_actual_verifier(self):
        fixture = fixtures.QualificationTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        evidence = self.root / "qualification-evidence"
        shutil.copytree(fixture.root, evidence)
        directory = evidence / "qualification"
        directory.mkdir()
        identity = directory / "identity.json"
        identity.write_text(json.dumps(fixture.identity))
        (directory / "trace.json").write_text(json.dumps(fixture.trace()))
        verifier = self.root / "scripts/soak/qualification.py"
        verifier.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / "scripts/soak/qualification.py", verifier)
        step = next(
            step
            for step in self.workflow["jobs"]["qualification_gate"]["steps"]
            if "run" in step
        )
        environment = {
            **os.environ,
            "PINNED_IDENTITY_SHA256": hashlib.sha256(identity.read_bytes()).hexdigest(),
            "TARGET_SHA": fixture.identity["target_sha"],
        }
        for conclusion in ("success", "failure", "cancelled", "skipped", ""):
            environment["SOAK_RESULT"] = conclusion
            result = subprocess.run(
                ["bash", "-euo", "pipefail", "-c", step["run"]],
                env=environment,
                cwd=self.root,
                capture_output=True,
                text=True,
                timeout=20,
                check=False,
            )
            self.assertEqual(
                result.returncode == 0, conclusion == "success", result.stderr
            )
            manifest = json.loads(
                (evidence / "qualification-manifest.json").read_text()
            )
            self.assertEqual(manifest["qualified"], conclusion == "success")

    def test_driver_qualification_guard_rejects_invalid_modes_before_work(self):
        for changes in (
            {"SOAK_DURATION_SECONDS": "79200"},
            {"SOAK_DEADLINE_EPOCH": "1"},
            {"SOAK_RUN_BENCHMARKS": "true"},
        ):
            environment = {
                **os.environ,
                "SOAK_QUALIFICATION": "true",
                "SOAK_DURATION_SECONDS": "86400",
                "SYSTEM_INTEGRATION_DIR": str(self.root),
                "SOAK_OUTPUT_DIR": str(self.root),
                **changes,
            }
            result = subprocess.run(
                ["bash", str(ROOT / "scripts/run-merge-recovery-soak.sh")],
                env=environment,
                capture_output=True,
                text=True,
                timeout=10,
                check=False,
            )
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertFalse((self.root / ".soak-state").exists())

    def test_driver_adds_no_shellcheck_diagnostics(self):
        baseline = subprocess.run(
            [
                "git",
                "-c",
                "core.fsmonitor=false",
                "show",
                "HEAD:scripts/run-merge-recovery-soak.sh",
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=20,
            check=True,
        ).stdout

        def diagnostics(source):
            result = subprocess.run(
                ["shellcheck", "--format=json", "--shell=bash", "-"],
                input=source,
                capture_output=True,
                text=True,
                timeout=20,
                check=False,
            )
            self.assertIn(result.returncode, (0, 1), result.stderr)
            lines = source.splitlines()
            return Counter(
                (entry["code"], entry["message"], lines[entry["line"] - 1].strip())
                for entry in json.loads(result.stdout)
            )

        current = (ROOT / "scripts/run-merge-recovery-soak.sh").read_text()
        self.assertFalse(diagnostics(current) - diagnostics(baseline))


if __name__ == "__main__":
    unittest.main()
