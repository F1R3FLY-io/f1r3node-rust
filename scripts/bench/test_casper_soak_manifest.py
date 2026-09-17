import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


def fixture_manifest():
    return {
        "schema_version": 1,
        "run_id": "fixture-run",
        "phase": "pre_pr216_merge",
        "candidate_id": "fixture-candidate",
        "node_revision": "1" * 40,
        "node_binary_digest": "2" * 64,
        "image_digest": "3" * 64,
        "harness_revision": "4" * 40,
        "external_harness_revision": "5" * 40,
        "source_digests": {"fixture.py": "6" * 64},
        "configuration_digest": "7" * 64,
        "profile_id": "authority-finality",
        "profile_digest": "8" * 64,
        "fixture_digest": "9" * 64,
        "expectation_digest": "a" * 64,
        "seed": "1",
        "provider": "docker",
        "policy_variant": "baseline",
        "evidence_kind": "synthetic_fixture",
        "capabilities": {"paired-evaluation": {"status": "unknown"}},
        "tool_versions": {"fixture": "1"},
        "bounds": {"scenarios": 2, "observations": 3},
        "assumptions": ["Synthetic process boundary."],
        "resource_limits": {"children": 1, "timeout_seconds": 1},
        "required_scenarios": ["fixture-scenario"],
        "deadline": {"clock_id": "wall", "epoch_seconds": "1"},
        "merge_gate": None,
    }


def retained_tree(root):
    return {
        str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in root.rglob("*")
        if path.is_file()
    }


class ManifestDriverTests(unittest.TestCase):
    def test_invalid_manifests_do_not_create_run_state(self):
        manifest = fixture_manifest()
        invalid = {
            "missing": b"{}",
            "array": b"[]",
            "duplicate": b'{"schema_version":1,"schema_version":1}',
            "nested-duplicate": b'{"bounds":{"children":1,"children":2}}',
            "nan": json.dumps(manifest).replace('"scenarios": 2', '"scenarios": NaN').encode(),
            "overflow": json.dumps(manifest).replace('"scenarios": 2', '"scenarios": 1e999').encode(),
            "utf8": b"\xff",
            "oversize": b" " * (1024 * 1024 + 1),
            "boolean-schema": json.dumps({**manifest, "schema_version": True}).encode(),
            "traversal": json.dumps({**manifest, "source_digests": {"../source.py": "a" * 64}}).encode(),
            "duplicate-scenarios": json.dumps({**manifest, "required_scenarios": ["same", "same"]}).encode(),
            "symbolic-link": json.dumps(manifest).encode(),
            "fifo": b"",
            "directory": b"",
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for case, data in invalid.items():
                with self.subTest(case=case):
                    source = root / (case + ".json")
                    output = root / (case + "-output")
                    if case == "symbolic-link":
                        target = root / "target.json"
                        target.write_bytes(data)
                        source.symlink_to(target)
                    elif case == "fifo":
                        os.mkfifo(source)
                    elif case == "directory":
                        source.mkdir()
                    else:
                        source.write_bytes(data)
                    environment = os.environ.copy()
                    environment.update({
                        "SOAK_DURATION_SECONDS": "1",
                        "SOAK_DEADLINE_EPOCH": "1",
                        "SYSTEM_INTEGRATION_DIR": str(root / "suite"),
                        "SOAK_OUTPUT_DIR": str(output),
                        "SOAK_MANIFEST_PATH": str(source),
                    })
                    result = subprocess.run(
                        ["bash", str(ROOT / "scripts/run-merge-recovery-soak.sh")],
                        env=environment, capture_output=True, text=True, timeout=5,
                    )
                    self.assertEqual(result.returncode, 2, result.stderr)
                    self.assertFalse(output.exists())
                    self.assertFalse((root / "suite").exists())

    def test_resume_requires_the_original_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest_path = root / "manifest.json"
            output = root / "output"
            suite = root / "suite"
            suite.mkdir()
            manifest = fixture_manifest()
            manifest_path.write_text(json.dumps(manifest))
            environment = os.environ.copy()
            environment.update({
                "SOAK_DURATION_SECONDS": "1",
                "SOAK_DEADLINE_EPOCH": "1",
                "SYSTEM_INTEGRATION_DIR": str(suite),
                "SOAK_OUTPUT_DIR": str(output),
                "SOAK_MANIFEST_PATH": str(manifest_path),
                "SOAK_RSS_CEILING_MB": "0",
                "SOAK_HOST_FREE_FLOOR_MB": "0",
                "SOAK_DISK_FREE_FLOOR_MB": "0",
                "SOAK_RUN_BENCHMARKS": "false",
            })

            def run():
                return subprocess.run(
                    ["bash", str(ROOT / "scripts/run-merge-recovery-soak.sh")],
                    env=environment, capture_output=True, text=True, timeout=15,
                )

            first = run()
            self.assertEqual(first.returncode, 0, first.stderr)
            matching = run()
            self.assertEqual(matching.returncode, 0, matching.stderr)
            self.assertIn("SEGMENT=2\n", (output / ".soak-state").read_text())
            before = retained_tree(output)
            for field, value in manifest.items():
                with self.subTest(field=field):
                    changed = copy.deepcopy(manifest)
                    if field.endswith("_revision") or field.endswith("_digest"):
                        changed[field] = "b" + value[1:]
                    elif field == "seed":
                        changed[field] = "2"
                    elif isinstance(value, str):
                        changed[field] = value + "-changed"
                    elif field == "source_digests":
                        changed[field] = {"fixture.py": "b" * 64}
                    elif isinstance(value, dict):
                        changed[field]["changed"] = True
                    elif isinstance(value, list):
                        changed[field].append("changed")
                    else:
                        changed[field] = "changed"
                    manifest_path.write_text(json.dumps(changed))
                    result = run()
                    self.assertEqual(result.returncode, 2, f"Changed {field} was accepted: {result.stdout}")
                    self.assertEqual(retained_tree(output), before)
            manifest_path.write_text(json.dumps(manifest))
            del environment["SOAK_MANIFEST_PATH"]
            result = run()
            self.assertEqual(result.returncode, 2, "Resume dropped the manifest requirement.")
            self.assertEqual(retained_tree(output), before)
            self.assertEqual(list(output.glob("iteration-*")), [])
            bound = output / ".casper-manifest.json"
            bound.unlink()
            before = retained_tree(output)
            result = run()
            self.assertEqual(result.returncode, 2, "A missing seal removed the manifest requirement.")
            self.assertEqual(retained_tree(output), before)
            bound.write_bytes(manifest_path.read_bytes())
            environment["SOAK_MANIFEST_PATH"] = str(manifest_path)
            for missing in (".soak-state", ".soak-checkpoint-state.json"):
                with self.subTest(missing=missing):
                    path = output / missing
                    data = path.read_bytes()
                    path.unlink()
                    before = retained_tree(output)
                    result = run()
                    self.assertEqual(result.returncode, 2, "An incomplete checkpoint was accepted.")
                    self.assertEqual(retained_tree(output), before)
                    path.write_bytes(data)


if __name__ == "__main__":
    unittest.main()
