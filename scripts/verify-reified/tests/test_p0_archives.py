"""Unit tests for scripts/bench/check-p0-archives.py (P10 step S8).

Each test builds a small repository with one archive, its summary JSON and the provenance file,
in the scratch root (RRS_SCRATCH, or target/verification/reified-rspaces), never in the source
tree or /tmp. A negative control changes one part and expects the named failure.
"""

import gzip
import hashlib
import importlib.util
import io
import json
import os
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[2]
SCRIPT = REPO_ROOT / "scripts" / "bench" / "check-p0-archives.py"


def load():
    spec = importlib.util.spec_from_file_location("check_p0_archives", SCRIPT)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_p0_archives"] = module
    spec.loader.exec_module(module)
    return module


ARCHIVES = load()


def scratch_root() -> Path:
    base = os.environ.get("RRS_SCRATCH") or str(REPO_ROOT / "target" / "verification" / "reified-rspaces")
    root = Path(base) / "p0-archive-tests"
    root.mkdir(parents=True, exist_ok=True)
    return root


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def pack(members: dict[str, bytes]) -> bytes:
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as tar:
        for name, data in members.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    out = io.BytesIO()
    with gzip.GzipFile(fileobj=out, mode="wb", mtime=0) as gz:
        gz.write(raw.getvalue())
    return out.getvalue()


class Fixture:
    def __init__(self, root: Path, log: bytes = b"[WORKSPACE_ROOT]/repo build ok\n",
                 summary_digest: str | None = None, recorded_member: bytes | None = None) -> None:
        self.root = root
        archive = pack({"run.log": log, "samples.json": b"{}"})
        (root / "test-vectors" / "benchmarks").mkdir(parents=True, exist_ok=True)
        (root / "test-vectors" / "bench.tar.gz").write_bytes(archive)
        original = "a" * 64
        (root / "test-vectors" / "bench.json").write_text(
            json.dumps({"archive": {"sha256": summary_digest or original}}), encoding="utf-8")
        provenance = {
            "schema": ARCHIVES.SCHEMA,
            "note": "A test.",
            "rule": {},
            "archives": [{
                "archive": "test-vectors/bench.tar.gz",
                "summary": "test-vectors/bench.json",
                "original_sha256": original,
                "redacted_sha256": sha(archive),
                "changed_members": [{
                    "member": "run.log",
                    "original_sha256": "b" * 64,
                    "redacted_sha256": sha(recorded_member if recorded_member is not None else log),
                }],
            }],
        }
        (root / ARCHIVES.PROVENANCE).write_text(json.dumps(provenance), encoding="utf-8")

    def check(self) -> list[str]:
        return ARCHIVES.check(self.root)


class P0ArchivesTest(unittest.TestCase):
    def setUp(self) -> None:
        self._dir = tempfile.TemporaryDirectory(dir=scratch_root())
        self.root = Path(self._dir.name)

    def tearDown(self) -> None:
        self._dir.cleanup()

    def test_consistent_archive_passes(self):
        self.assertEqual(Fixture(self.root).check(), [])

    def test_changed_archive_fails(self):
        fixture = Fixture(self.root)
        path = self.root / "test-vectors" / "bench.tar.gz"
        path.write_bytes(path.read_bytes() + b"\0")
        problems = fixture.check()
        self.assertTrue(any("is not" in p for p in problems), problems)

    def test_summary_without_the_original_digest_fails(self):
        problems = Fixture(self.root, summary_digest="c" * 64).check()
        self.assertTrue(any("does not record the original digest" in p for p in problems), problems)

    def test_home_path_in_a_member_fails(self):
        problems = Fixture(self.root, log=b"/home/user/repo build ok\n").check()
        self.assertTrue(any("holds a home directory path" in p for p in problems), problems)

    def test_changed_member_with_another_digest_fails(self):
        problems = Fixture(self.root, recorded_member=b"another log\n").check()
        self.assertTrue(any("has not its redacted digest" in p for p in problems), problems)

    def test_missing_changed_member_fails(self):
        fixture = Fixture(self.root)
        path = self.root / ARCHIVES.PROVENANCE
        data = json.loads(path.read_text(encoding="utf-8"))
        data["archives"][0]["changed_members"].append(
            {"member": "absent.log", "original_sha256": "d" * 64, "redacted_sha256": "e" * 64})
        path.write_text(json.dumps(data), encoding="utf-8")
        problems = fixture.check()
        self.assertTrue(any("absent.log is missing" in p for p in problems), problems)


if __name__ == "__main__":
    unittest.main()
