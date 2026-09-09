import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from qualification import verify

ROOT = Path(__file__).resolve().parents[2]


class DriverQualificationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(dir=os.environ["TMPDIR"])
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.node = self.root / "node"
        self.harness = self.root / "harness"
        self.output = self.root / "output"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.write(self.node / "node/src/main/resources/defaults.conf", "defaults")
        self.write(
            self.harness / "integration-tests/test/tests/custom/test_load.py",
            "fixture workload",
        )
        self.write(self.harness / "poetry.lock", "fixture lock")
        self.write(self.root / "node.bin", "fixture binary")
        self.write(self.root / "clock", "10")
        self.write(self.output / "preflight/junit.xml", self.junit())
        self.executable(
            "git",
            """
import os
import sys
if "rev-parse" in sys.argv:
    location = os.path.basename(os.getcwd())
    print(("a" if location == "node" else "c" if location == "harness" else "b") * 40)
""",
        )
        self.executable(
            "docker",
            """
import sys
if sys.argv[1:3] == ["image", "inspect"]:
    print("sha256:" + "d" * 64)
""",
        )
        self.executable("pkill", "")
        self.executable(
            "python3",
            """
import os
from pathlib import Path
import runpy
import sys
import time
clock = Path(os.environ["FIXTURE_CLOCK"])
if sys.argv[1:3] == ["-c", "import time; print(time.monotonic_ns())"]:
    current = int(clock.read_text()) + 43_200_000_000_000
    clock.write_text(str(current))
    print(current)
elif len(sys.argv) > 1 and sys.argv[1].endswith("/collect_qualification.py"):
    time.monotonic_ns = lambda: int(clock.read_text())
    sys.path.insert(0, str(Path(sys.argv[1]).parent))
    sys.argv = sys.argv[1:]
    runpy.run_path(sys.argv[0], run_name="__main__")
else:
    os.execv(sys.executable, [sys.executable, *sys.argv[1:]])
""",
        )
        self.executable(
            "poetry",
            """
import os
from pathlib import Path
import sys
import time
assert os.environ["PYTHONHASHSEED"] == "216"
junit = Path(next(arg.split("=", 1)[1] for arg in sys.argv if arg.startswith("--junitxml=")))
if not os.environ.get("FIXTURE_MISSING_JUNIT"):
    junit.write_text(os.environ["FIXTURE_JUNIT"])
archive = Path(os.environ["SYSTEM_INTEGRATION_DIR"]) / "integration-tests/log-archive/session"
archive.mkdir(parents=True, exist_ok=True)
(archive / "resource-timeseries.csv").write_text(
    "elapsed_s,node,memory_mb,cpu_percent,memory_limit_mb\\n"
    "1.0,rnode.fixture.validator1,256.0,10.0,0\\n"
    "1.0,rnode.fixture.validator2,512.0,20.0,0\\n")
print("======== 1 passed in 0.1s ========", flush=True)
time.sleep(0.4)
""",
        )
        self.environment = {
            "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
            "TMPDIR": os.environ["TMPDIR"],
            "PYTHONDONTWRITEBYTECODE": "1",
            "SOAK_NODE_REPO_DIR": str(self.node),
            "SYSTEM_INTEGRATION_DIR": str(self.harness),
            "SOAK_OUTPUT_DIR": str(self.output),
            "SOAK_TARGET_REF": "a" * 40,
            "SOAK_TARGET_SHA": "a" * 40,
            "SOAK_CONTROL_SHA": "b" * 40,
            "SYSTEM_INTEGRATION_REF": "c" * 40,
            "SOAK_QUALIFICATION_IMAGE": "sha256:" + "d" * 64,
            "F1R3FLY_NODE_IMAGE": "sha256:" + "d" * 64,
            "F1R3FLY_NODE_BINARY": str(self.root / "node.bin"),
            "SOAK_QUALIFICATION_SEEDS": "[216]",
            "GITHUB_RUN_ID": "12345",
            "GITHUB_RUN_ATTEMPT": "1",
            "SOAK_PREFLIGHT_RESULT": "passed",
            "SOAK_QUALIFICATION": "true",
            "SOAK_DURATION_SECONDS": "86400",
            "SOAK_RSS_CEILING_MB": "0",
            "SOAK_HOST_FREE_FLOOR_MB": "0",
            "SOAK_DISK_FREE_FLOOR_MB": "0",
            "SOAK_MONITOR_SNAPSHOT_SECONDS": "0.05",
            "SOAK_GUARDIAN_POLL_SECONDS": "0.1",
            "FIXTURE_CLOCK": str(self.root / "clock"),
            "FIXTURE_JUNIT": self.junit(),
        }

    @staticmethod
    def junit():
        return (
            '<testsuite tests="1" failures="0" errors="0" skipped="0">'
            '<testcase classname="fixture" name="load"/></testsuite>'
        )

    @staticmethod
    def write(path, contents):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)

    def executable(self, name, contents):
        path = self.bin / name
        path.write_text(f"#!{sys.executable}\n" + contents)
        path.chmod(0o755)

    def execute(self):
        prepared = subprocess.run(
            [
                str(self.bin / "python3"),
                str(ROOT / "scripts/soak/collect_qualification.py"),
                "prepare",
            ],
            env=self.environment,
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(prepared.returncode, 0, prepared.stderr)
        return subprocess.run(
            ["bash", str(ROOT / "scripts/run-merge-recovery-soak.sh")],
            env=self.environment,
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )

    def test_real_driver_collects_both_providers_with_simulated_clock(self):
        result = self.execute()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        directory = self.output / "qualification"
        trace = json.loads((directory / "trace.json").read_text())
        identity = json.loads((directory / "identity.json").read_text())
        manifest = verify(trace, self.output, "success", identity)
        self.assertEqual(manifest["qualified_ns"], 86_400_000_000_000)
        self.assertEqual(manifest["iterations"], 2)
        self.assertEqual(manifest["providers"], ["docker", "subprocess"])
        self.assertFalse(
            json.loads((directory / "local-result.json").read_text())["qualified"]
        )
        self.assertFalse(list(self.output.glob("iteration-*/deadline.txt")))

    def test_success_exit_without_junit_cannot_qualify(self):
        self.environment["FIXTURE_MISSING_JUNIT"] = "1"
        result = self.execute()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertTrue((self.output / "qualification/rejected.json").exists())
        self.assertFalse((self.output / "qualification/local-result.json").exists())


if __name__ == "__main__":
    unittest.main()
