#!/usr/bin/env python3
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile


TOOL = Path(__file__).with_name("verification-storage.py")


class StorageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.source = self.base / "repo"
        self.source.mkdir()
        subprocess.run(["git", "init", "-q", str(self.source)], check=True)
        self.put("scripts/tool.sh", "#!/bin/sh\nprintf ok\n")
        self.put("scripts/charts/Cargo.toml", "[package]\nname='fixture'\n")
        subprocess.run(["git", "-C", str(self.source), "add", "--", "scripts"], check=True)
        self.put("scripts/charts/target/debug/generated", "x" * 65536)
        self.put("scripts/untracked-secret", "not for capture")
        self.store = self.base / "store"

    def put(self, name, data):
        path = self.source / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(data)
        return path

    def command(self, *args, success=True):
        result = subprocess.run([sys.executable, "-I", str(TOOL), *map(str, args)],
                                capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0 if success else 2, result.stderr)
        try:
            return json.loads(result.stdout) if success else {}
        except ValueError as error:
            self.fail(f"The command did not return valid JSON: {error}")

    def capture(self, *args, success=True):
        return self.command("snapshot", self.source, self.store, "scripts", "--min-free-bytes", "1", *args, success=success)

    def test_legacy_recipe_copies_build_output(self):
        destination = self.base / "legacy"
        shutil.copytree(self.source / "scripts", destination)
        self.assertTrue((destination / "charts/target/debug/generated").exists())

    def test_source_only_capture_and_index_preservation(self):
        index = (self.source / ".git/index").read_bytes()
        result = self.capture()
        snapshot = Path(result["snapshot"]) / "source"
        self.assertEqual(result["files"], 2)
        self.assertFalse((snapshot / "scripts/charts/target").exists())
        self.assertFalse((snapshot / "scripts/untracked-secret").exists())
        self.assertEqual(index, (self.source / ".git/index").read_bytes())
        self.assertEqual((snapshot / "scripts/tool.sh").read_bytes(), (self.source / "scripts/tool.sh").read_bytes())
        self.assertEqual((snapshot / "scripts/tool.sh").stat().st_mode & 0o222, 0)

    def test_reuse_and_working_changes(self):
        first = self.capture()
        again = self.capture()
        self.assertTrue(again["reused"])
        self.assertEqual(first["snapshot"], again["snapshot"])
        self.put("scripts/tool.sh", "changed but not staged\n")
        changed = self.capture()
        self.assertNotEqual(first["snapshot"], changed["snapshot"])
        self.assertFalse(changed["reused"])

    def test_explicit_untracked_input(self):
        self.put("fixture.py", "print('fixture')\n")
        result = self.capture("--extra-file", "fixture.py")
        self.assertEqual(result["files"], 3)
        self.assertTrue((Path(result["snapshot"]) / "source/fixture.py").is_file())

    def test_tracked_generated_input_is_reported_not_copied(self):
        subprocess.run(["git", "-C", str(self.source), "add", "--", "scripts/charts/target"], check=True)
        result = self.capture()
        self.assertEqual(result["excluded_tracked_paths"], ["scripts/charts/target/debug/generated"])
        self.assertEqual(result["files"], 2)

    def test_byte_limit_before_store_creation(self):
        self.capture("--max-bytes", "1", success=False)
        self.assertFalse(self.store.exists())

    def test_file_limit(self):
        self.capture("--max-files", "1", success=False)
        self.assertFalse(self.store.exists())

    def test_store_admission_limit(self):
        self.capture("--max-store-bytes", "1", success=False)
        self.assertEqual(list(self.store.glob("[0-9a-f]" * 64)), [])

    def test_free_space_admission(self):
        self.capture("--min-free-bytes", str(2**63 - 1), success=False)
        self.assertFalse(self.store.exists())

    def test_symlink_input_refused(self):
        original = self.source / "scripts/tool.sh"
        original.unlink()
        original.symlink_to(self.source / "scripts/untracked-secret")
        self.capture(success=False)
        self.assertFalse(self.store.exists())

    def test_parent_symlink_refused(self):
        moved = self.source / "outside"
        (self.source / "scripts/charts").rename(moved)
        (self.source / "scripts/charts").symlink_to(moved, target_is_directory=True)
        self.capture(success=False)

    def test_store_inside_source_refused(self):
        self.store = self.source / "target/store"
        self.capture(success=False)
        self.assertFalse(self.store.exists())

    def test_tampered_snapshot_refused(self):
        result = self.capture()
        file = Path(result["snapshot"]) / "source/scripts/tool.sh"
        file.chmod(0o644)
        file.write_text("tampered")
        self.capture(success=False)

    def test_traversal_refused(self):
        self.capture("--extra-file", "../outside", success=False)
        self.assertFalse(self.store.exists())

    def archive(self):
        path = self.base / "archive.zip"
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("one/data.txt", b"retained\r\n")
        extracted = self.base / "extracted"
        (extracted / "one").mkdir(parents=True)
        (extracted / "one/data.txt").write_bytes(b"retained\r\n")
        return path, extracted, hashlib.sha256(path.read_bytes()).hexdigest()

    def test_archive_coverage_does_not_delete(self):
        archive, extracted, digest = self.archive()
        result = self.command("verify-zip", archive, extracted, "--sha256", digest)
        self.assertEqual(result["files"], 1)
        self.assertFalse(result["deleted"])
        self.assertTrue((extracted / "one/data.txt").exists())

    def test_archive_corruption_refused(self):
        archive, extracted, digest = self.archive()
        archive.write_bytes(archive.read_bytes() + b"changed")
        self.command("verify-zip", archive, extracted, "--sha256", digest, success=False)

    def test_extra_extracted_file_refused(self):
        archive, extracted, digest = self.archive()
        (extracted / "extra").write_text("unique evidence")
        self.command("verify-zip", archive, extracted, "--sha256", digest, success=False)

    def test_changed_extracted_file_refused(self):
        archive, extracted, digest = self.archive()
        (extracted / "one/data.txt").write_bytes(b"retained\n")
        self.command("verify-zip", archive, extracted, "--sha256", digest, success=False)

    def test_zip_traversal_and_symlink_refused(self):
        for name, link in (("../escape", False), ("link", True)):
            with self.subTest(name=name):
                archive = self.base / "unsafe.zip"
                with zipfile.ZipFile(archive, "w") as stream:
                    info = zipfile.ZipInfo(name)
                    if link:
                        info.create_system = 3
                        info.external_attr = 0o120777 << 16
                    stream.writestr(info, b"outside")
                extracted = self.base / "empty"
                extracted.mkdir(exist_ok=True)
                self.command("verify-zip", archive, extracted, "--sha256", hashlib.sha256(archive.read_bytes()).hexdigest(), success=False)

    def test_archive_expansion_limit(self):
        archive, extracted, digest = self.archive()
        self.command("verify-zip", archive, extracted, "--sha256", digest, "--max-bytes", "1", success=False)

    def test_extra_snapshot_output_refused(self):
        result = self.capture()
        (Path(result["snapshot"]) / "unrecorded-output").write_text("extra")
        self.capture(success=False)

    def test_source_directories_are_read_only(self):
        result = self.capture()
        source = Path(result["snapshot"]) / "source"
        self.assertEqual(source.stat().st_mode & 0o222, 0)
        self.assertEqual((source / "scripts").stat().st_mode & 0o222, 0)

    def test_extraction_root_link_refused(self):
        archive, extracted, digest = self.archive()
        alias = self.base / "alias"
        alias.symlink_to(extracted, target_is_directory=True)
        self.command("verify-zip", archive, alias, "--sha256", digest, success=False)

    def test_unarchived_empty_directory_refused(self):
        archive, extracted, digest = self.archive()
        (extracted / "unique-empty-directory").mkdir()
        self.command("verify-zip", archive, extracted, "--sha256", digest, success=False)

    def cargo_wrapper(self, arguments, extra=None):
        executable = self.base / "bin/cargo"
        executable.parent.mkdir(exist_ok=True)
        executable.write_text('#!/bin/sh\nprintf "%s\\n" "$CARGO_TARGET_DIR" "$CARGO_INCREMENTAL" "$CARGO_PROFILE_DEV_DEBUG" "$CARGO_PROFILE_TEST_DEBUG" "$@"\n')
        executable.chmod(0o755)
        environment = {"PATH": f"{executable.parent}:/usr/bin:/bin", "HOME": str(self.base),
                       "CBC_CARGO_CACHE": str(self.base / "cache"), "CBC_MIN_FREE_GIB": "1"}
        environment.update(extra or {})
        return subprocess.run(["bash", str(TOOL.with_name("cargo-low-disk.sh")), *arguments],
                              cwd=self.source, env=environment, text=True, capture_output=True, timeout=20)

    def test_local_cargo_profile(self):
        result = self.cargo_wrapper(["test", "-p", "fixture"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [str(self.base / "cache"), "0", "line-tables-only",
                                                     "line-tables-only", "test", "-p", "fixture"])

    def test_cargo_ci_and_profile_overrides_refused(self):
        for args, environment in ((["test"], {"CI": "true"}), (["build", "--release"], {}),
                                  (["test", "--target-dir=target"], {})):
            result = self.cargo_wrapper(args, environment)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertFalse((self.base / "cache").exists())

    def test_cargo_low_space_refused(self):
        result = self.cargo_wrapper(["check"], {"CBC_MIN_FREE_GIB": "1048576"})
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertFalse((self.base / "cache").exists())

    def test_cargo_cache_inside_source_refused(self):
        result = self.cargo_wrapper(["check"], {"CBC_CARGO_CACHE": str(self.source / "target")})
        self.assertEqual(result.returncode, 2, result.stderr)

    def test_audit_does_not_authorize_deletion(self):
        result = self.command("audit", self.source / "scripts")
        self.assertFalse(result["roots"][0]["deletion_authorized"])
        self.assertGreater(result["roots"][0]["logical_bytes"], 65536)


if __name__ == "__main__":
    unittest.main()
