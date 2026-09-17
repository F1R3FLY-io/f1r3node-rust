import importlib.util
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[1] / "check_casper_soak_models.py"
SPEC = importlib.util.spec_from_file_location("casper_soak_model_runner", SOURCE)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("Cannot load the harness model runner.")
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


def violation(name):
    return f"Error: Invariant {name} is violated.\nThe behavior up to this point is:\nState 1: initial\n"


class VerdictTests(unittest.TestCase):
    def test_clean_requires_completed_search(self):
        self.assertEqual(RUNNER.classify(0, RUNNER.CLEAN_MARKER), "passed")
        self.assertNotEqual(RUNNER.classify(0, "Starting TLC"), "passed")
        self.assertNotEqual(RUNNER.classify(0, RUNNER.CLEAN_MARKER + "\nError: broken"), "passed")

    def test_negative_requires_exact_exit_property_and_trace(self):
        log = violation("IdentityPinned")
        self.assertEqual(RUNNER.classify(12, log, "IdentityPinned"), "passed")
        for code in [0, 1, 124, -9]:
            with self.subTest(code=code):
                self.assertNotEqual(RUNNER.classify(code, log, "IdentityPinned"), "passed")
        self.assertNotEqual(RUNNER.classify(12, log, "PostMergeGate"), "passed")
        self.assertNotEqual(RUNNER.classify(12, "Invariant IdentityPinned is violated.", "IdentityPinned"), "passed")

    def test_unexpected_success_and_multiple_violations_fail(self):
        self.assertNotEqual(RUNNER.classify(0, RUNNER.CLEAN_MARKER, "IdentityPinned"), "passed")
        log = violation("IdentityPinned") + violation("PostMergeGate")
        self.assertNotEqual(RUNNER.classify(12, log, "IdentityPinned"), "passed")


class ConfigurationTests(unittest.TestCase):
    def setUp(self):
        self.control = {"configuration": "clean.cfg", "properties": ["Safe"], "expected_exit": 0}
        self.text = "INVARIANT TypeOK\nINVARIANT Safe\n  AllowBug = FALSE\n"

    def test_exact_properties_and_knobs(self):
        RUNNER.validate_configuration(self.control, self.text, ["AllowBug"])
        for bad in [self.text.replace("INVARIANT Safe\n", ""), self.text.replace("FALSE", "TRUE"), self.text + "INVARIANT Safe\n"]:
            with self.subTest(text=bad), self.assertRaises(ValueError):
                RUNNER.validate_configuration(self.control, bad, ["AllowBug"])

    def test_registry_rejects_path_escape_and_unmatched_properties(self):
        plan = {
            "positive_control": self.control,
            "negative_controls": [{"configuration": "unsafe.cfg", "property": "Safe", "knob": "AllowBug", "expected_exit": 12}],
        }
        RUNNER.registered_controls(plan)
        plan["negative_controls"][0]["configuration"] = "../escape.cfg"
        with self.assertRaises(ValueError):
            RUNNER.registered_controls(plan)
        plan["negative_controls"][0]["configuration"] = "unsafe.cfg"
        plan["negative_controls"][0]["property"] = "Other"
        with self.assertRaises(ValueError):
            RUNNER.registered_controls(plan)


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.model = self.root / "Model.tla"
        self.config = self.root / "clean.cfg"
        self.jar = self.root / "tools.jar"
        self.model.write_text("model fixture")
        self.config.write_text("INVARIANT TypeOK\nINVARIANT Safe\nAllowBug = FALSE\n")
        self.jar.write_bytes(b"jar fixture")
        self.control = {"configuration": "clean.cfg", "properties": ["Safe"], "expected_exit": 0}

    def run_control(self):
        return RUNNER.run_control(self.model, self.config, self.control, ["AllowBug"], "java", self.jar, 1, self.root)

    def test_timeout_retains_partial_log_and_never_passes(self):
        error = subprocess.TimeoutExpired(["java"], 1, output=b"partial trace")
        with patch.object(RUNNER.subprocess, "run", side_effect=error):
            result = self.run_control()
        self.assertEqual(result["outcome"], "timeout")
        self.assertIsNone(result["exit"])
        self.assertEqual((self.root / result["log"]).read_text(), "partial trace")
        self.assertEqual(result["log_sha256"], RUNNER.digest(self.root / result["log"]))

    def test_missing_configuration_never_runs_prover(self):
        self.config.unlink()
        with patch.object(RUNNER.subprocess, "run") as execute:
            result = self.run_control()
        execute.assert_not_called()
        self.assertEqual(result["outcome"], "tool_error")

    def test_success_records_inputs_and_checks_timeout_and_memory(self):
        completed = subprocess.CompletedProcess([], 0, RUNNER.CLEAN_MARKER.encode())
        with patch.object(RUNNER.subprocess, "run", return_value=completed) as execute:
            result = self.run_control()
        self.assertEqual(result["outcome"], "passed")
        self.assertEqual(result["model_sha256"], RUNNER.digest(self.model))
        self.assertEqual(execute.call_args.kwargs["timeout"], 1)
        self.assertIn("-Xmx512m", execute.call_args.args[0])

    def test_source_mutation_invalidates_result(self):
        def mutate(*args, **kwargs):
            self.model.write_text("changed fixture")
            return subprocess.CompletedProcess([], 0, RUNNER.CLEAN_MARKER.encode())
        with patch.object(RUNNER.subprocess, "run", side_effect=mutate):
            result = self.run_control()
        self.assertEqual(result["outcome"], "input_changed")

    def test_missing_java_is_tool_error(self):
        with patch.object(RUNNER.subprocess, "run", side_effect=FileNotFoundError("java")):
            result = self.run_control()
        self.assertEqual(result["outcome"], "tool_error")

    def test_cancellation_cannot_pass(self):
        with patch.object(RUNNER.subprocess, "run", side_effect=KeyboardInterrupt):
            result = self.run_control()
        self.assertEqual(result["outcome"], "cancelled")
        self.assertIsNone(result["exit"])

    def test_existing_output_directory_is_not_overwritten(self):
        marker = self.root / "report.json"
        marker.write_text("previous evidence")
        with patch("sys.stderr", new=io.StringIO()), self.assertRaises(SystemExit) as error:
            RUNNER.main(["--output-dir", str(self.root)])
        self.assertEqual(error.exception.code, 2)
        self.assertEqual(marker.read_text(), "previous evidence")


if __name__ == "__main__":
    unittest.main()
