import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


MODULE_PATH = Path(__file__).resolve().parents[1] / "process_faults.py"
spec = importlib.util.spec_from_file_location("process_faults", MODULE_PATH)
faults = importlib.util.module_from_spec(spec)
spec.loader.exec_module(faults)


class Node:
    name = "controlled-child"

    def __init__(self):
        self._handle = self
        self._proc = self.spawn()
        self.pause_noop = False
        self.same_child = False
        self.calls = []

    @staticmethod
    def spawn():
        return subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])

    def pause(self):
        self.calls.append("pause")
        if not self.pause_noop:
            os.kill(self._proc.pid, signal.SIGSTOP)

    def unpause(self):
        self.calls.append("unpause")
        os.kill(self._proc.pid, signal.SIGCONT)

    def restart(self):
        self.calls.append("restart")
        if not self.same_child:
            self._proc.terminate()
            self._proc.wait(timeout=2)
            self._proc = self.spawn()

    def close(self):
        if self._proc.poll() is None:
            os.kill(self._proc.pid, signal.SIGCONT)
            self._proc.terminate()
        self._proc.wait(timeout=2)


@unittest.skipUnless(sys.platform == "linux", "Process observations require Linux.")
class ProcessFaults(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.node = Node()
        self.addCleanup(self.node.close)
        self.request = {"action": "pause", "fault_id": "fault-1", "node_id": self.node.name,
            "incarnation": "incarnation-1", "trigger_event": "loaded", "clock_id": faults.clock_id(),
            "deadline_monotonic_ns": time.monotonic_ns() + 2_000_000_000}
        self.before = self.capture("before", "incarnation-1")

    def capture(self, name, incarnation, binary="b" * 64):
        path = self.root / name
        path.mkdir()
        identity = {"pid": self.node._proc.pid,
            "process_start_ticks": faults._process(self.node._proc.pid)["start_ticks"],
            "incarnation": incarnation, "declared_source_revision": "a" * 40,
            "executable_sha256": binary, "configuration_sha256": "c" * 64}
        binding = {"node_pid": identity["pid"], "process_start_ticks": identity["process_start_ticks"],
            "source_revision": identity["declared_source_revision"], "executable_sha256": binary,
            "configuration_sha256": identity["configuration_sha256"]}
        binding.update(request_id="request-1", approved_request_sha256="d" * 64)
        authority = {"targets": []}
        request = {"request_id": "request-1", "authority": authority, "challenge": "challenge-1",
                   "incarnation": incarnation, "approved_request_sha256": "d" * 64}
        request_ref = faults._write(path / "request.json", request)
        hello = {"identity": identity, "challenge": "challenge-1", "approved_request_sha256": "d" * 64,
                 "clock": "observer-monotonic", "sequence": 1, "monotonic_ns": 1}
        response = {"identity": identity, "kind": "authority_snapshot", "live_profile_qualified": False,
                    "request_sha256": request_ref["sha256"], "request_id": "request-1",
                    "approved_request_sha256": "d" * 64, "clock": "observer-monotonic",
                    "sequence": 2, "monotonic_ns": 2,
                    "result": {"availability": "available", "value": {"request": authority}}}
        artifacts = [request_ref]
        for filename, value in [("binding.json", binding), ("hello.json", hello),
                                ("authority.json", authority), ("response.json", response)]:
            artifacts.append(faults._write(path / filename, value))
        report = faults._write(path / "report.json", {"status": "captured", "artifacts": artifacts})
        return path, report["sha256"]

    def apply(self, after=None):
        return faults.apply_owned_fault(self.node, self.request, self.before,
            after or (lambda deadline: self.capture("after", "incarnation-2")), self.root / "output")

    def test_pause_observes_stop_and_always_resumes(self):
        report = self.apply()
        self.assertEqual(report["receipt"]["status"], "applied")
        self.assertEqual(report["receipt"]["observed_state"], "stopped")
        self.assertEqual(report["receipt"]["cleanup"]["status"], "resumed")
        self.assertEqual(self.node.calls, ["pause", "unpause"])
        self.assertNotIn(faults._process(self.node._proc.pid)["state"], {"T", "t"})
        self.assertEqual(report["profile_verdict"], "blocked")
        self.assertEqual(report["soak_verdict"], "non_passing")
        for ref in report["artifacts"]:
            data = (self.root / "output" / ref["path"]).read_bytes()
            self.assertEqual(hashlib.sha256(data).hexdigest(), ref["sha256"])

    def test_successful_noop_is_not_an_applied_pause(self):
        self.node.pause_noop = True
        self.request["deadline_monotonic_ns"] = time.monotonic_ns() + 50_000_000
        receipt = self.apply()["receipt"]
        self.assertEqual(receipt["status"], "unknown")
        self.assertEqual(receipt["cleanup"]["status"], "resumed")

    def test_restart_observes_exit_and_new_identity(self):
        self.request["action"] = "restart"
        receipt = self.apply()["receipt"]
        self.assertEqual(receipt["status"], "applied")
        self.assertTrue(receipt["prior_exit"])
        self.assertIsInstance(receipt["prior_exit_code"], int)
        self.assertTrue(receipt["ready"])
        self.assertEqual(receipt["new_incarnation"], "incarnation-2")

    def test_noop_restart_cannot_claim_an_exit(self):
        self.request["action"] = "restart"
        self.node.same_child = True
        receipt = self.apply()["receipt"]
        self.assertEqual(receipt["status"], "unknown")
        self.assertFalse(receipt["prior_exit"])

    def test_reused_incarnation_and_changed_binary_cannot_pass(self):
        self.request["action"] = "restart"
        receipt = self.apply(lambda deadline: self.capture("after", "incarnation-1"))["receipt"]
        self.assertEqual(receipt["status"], "unknown")
        self.assertFalse(receipt["ready"])

    def test_changed_candidate_cannot_pass(self):
        self.request["action"] = "restart"
        receipt = self.apply(lambda deadline: self.capture("after", "incarnation-2", "d" * 64))["receipt"]
        self.assertEqual(receipt["status"], "unknown")
        self.assertFalse(receipt["ready"])

    def test_late_restart_readiness_is_retained_as_nonpassing(self):
        self.request["action"] = "restart"
        self.request["deadline_monotonic_ns"] = time.monotonic_ns() + 100_000_000
        def delayed(deadline):
            time.sleep(0.15)
            return self.capture("after", "incarnation-2")
        receipt = self.apply(delayed)["receipt"]
        self.assertEqual(receipt["status"], "unknown")
        self.assertTrue(receipt["prior_exit"])
        self.assertIn("late", receipt["error"])

    def test_corrupt_capture_and_wrong_clock_do_not_invoke_provider(self):
        self.request["clock_id"] = "other-host"
        with self.assertRaises(ValueError):
            self.apply()
        self.request["clock_id"] = faults.clock_id()
        (self.before[0] / "response.json").write_text("{}")
        with self.assertRaises(ValueError):
            self.apply()
        self.assertEqual(self.node.calls, [])

    def test_existing_output_does_not_invoke_provider(self):
        (self.root / "output").mkdir()
        with self.assertRaises(FileExistsError):
            self.apply()
        self.assertEqual(self.node.calls, [])

    def test_adopted_handle_is_rejected_before_action(self):
        class Adopted:
            pass
        self.node._handle = Adopted()
        with self.assertRaises(ValueError):
            self.apply()
        self.assertEqual(self.node.calls, [])


if __name__ == "__main__":
    unittest.main()
