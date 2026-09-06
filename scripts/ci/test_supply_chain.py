import copy
import datetime
import hashlib
import io
import json
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_supply_chain as controls


ROOT = Path(__file__).resolve().parents[2]
TODAY = datetime.date(2026, 9, 6)


def report(*records):
    summary = {check: {"errors": 0} for check in controls.CHECKS}
    return "\n".join(
        json.dumps(record)
        for record in (*records, {"type": "summary", "fields": summary})
    )


def advisory(version="0.101.7"):
    return {
        "type": "diagnostic",
        "fields": {
            "severity": "note",
            "advisory": {"id": "RUSTSEC-2026-0104"},
            "graphs": [{"Krate": {"name": "rustls-webpki", "version": version}}],
        },
    }


class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.policy = controls.load_toml(ROOT / "supply-chain/policy.toml")
        self.deny = controls.load_toml(ROOT / "deny.toml")

    def test_current_policy_is_structurally_valid(self):
        controls.validate_policy(self.policy, self.deny, TODAY)
        controls.validate_manifests(ROOT, self.policy["manifests"])

    def test_exception_expires_on_review_date(self):
        self.policy["exceptions"]["RUSTSEC-2026-0104"]["review-by"] = TODAY
        with self.assertRaisesRegex(ValueError, "requires review"):
            controls.validate_policy(self.policy, self.deny, TODAY)

    def test_exception_requires_owner(self):
        del self.policy["exceptions"]["RUSTSEC-2026-0104"]["owner"]
        with self.assertRaises(ValueError):
            controls.validate_policy(self.policy, self.deny, TODAY)

    def test_advisory_cannot_be_ignored_without_review_record(self):
        self.deny["advisories"]["ignore"].append(
            {"id": "RUSTSEC-2025-0167", "reason": "test"}
        )
        with self.assertRaises(ValueError):
            controls.validate_policy(self.policy, self.deny, TODAY)

    def test_exception_cannot_use_a_version_range(self):
        self.policy["exceptions"]["RUSTSEC-2026-0104"]["versions"] = [">=0.101"]
        with self.assertRaises(ValueError):
            controls.validate_policy(self.policy, self.deny, TODAY)

    def test_transitive_unsoundness_cannot_be_disabled(self):
        self.deny["advisories"]["unsound"] = "workspace"
        with self.assertRaises(ValueError):
            controls.validate_policy(self.policy, self.deny, TODAY)

    def test_graph_cannot_exclude_development_or_target_dependencies(self):
        for key, value in (
            ("exclude-dev", True),
            ("targets", ["x86_64-unknown-linux-gnu"]),
            ("exclude", ["bitmaps"]),
        ):
            with self.subTest(key=key):
                deny = copy.deepcopy(self.deny)
                deny["graph"][key] = value
                with self.assertRaises(ValueError):
                    controls.validate_policy(self.policy, deny, TODAY)

    def test_license_clarifications_require_file_evidence(self):
        self.deny["licenses"]["clarify"][0]["license-files"] = []
        with self.assertRaises(ValueError):
            controls.validate_policy(self.policy, self.deny, TODAY)

    def test_build_script_allowances_cannot_be_disabled(self):
        del self.deny["bans"]["build"]["allow-build-scripts"]
        with self.assertRaises(ValueError):
            controls.validate_policy(self.policy, self.deny, TODAY)

    def test_excluded_standalone_package_requires_its_own_scan(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            results = [
                subprocess.CompletedProcess(
                    [], 0, b"Cargo.toml\0tools/Cargo.toml\0", b""
                ),
                subprocess.CompletedProcess([], 0, str(root / "Cargo.toml"), ""),
                subprocess.CompletedProcess([], 0, str(root / "tools/Cargo.toml"), ""),
            ]
            with (
                patch.object(controls.subprocess, "run", side_effect=results),
                self.assertRaises(ValueError),
            ):
                controls.validate_manifests(root, ["Cargo.toml"])

    def test_fuzz_workspace_cannot_be_removed_from_scans(self):
        with self.assertRaises(ValueError):
            controls.validate_manifests(
                ROOT, ["Cargo.toml", "scripts/soak-charts/Cargo.toml"]
            )

    def test_git_revision_must_be_full_and_match_resolved_commit(self):
        sha = "1" * 40
        for source in (
            f"git+https://example.com/repo?rev=1111111#{sha}",
            f"git+https://example.com/repo?rev={'2' * 40}#{sha}",
        ):
            with self.subTest(source=source), self.assertRaises(ValueError):
                controls.validate_metadata(
                    {"packages": [{"name": "fixture", "source": source}]}
                )
        controls.validate_metadata(
            {
                "packages": [
                    {
                        "name": "fixture",
                        "source": f"git+https://example.com/repo?rev={sha}#{sha}",
                    }
                ]
            }
        )

    def test_report_accepts_only_the_reviewed_advisory_version(self):
        errors, seen = controls.validate_report(
            report(advisory()), self.policy["exceptions"]
        )
        self.assertFalse(errors)
        self.assertEqual(seen, {"RUSTSEC-2026-0104"})
        errors, _ = controls.validate_report(
            report(advisory("0.102.8")), self.policy["exceptions"]
        )
        self.assertTrue(errors)

    def test_internal_bug_is_fatal_even_with_success_summary(self):
        bug = {
            "type": "diagnostic",
            "fields": {"severity": "bug", "code": "unresolved-workspace-dependency"},
        }
        errors, _ = controls.validate_report(report(bug), self.policy["exceptions"])
        self.assertTrue(errors)

    def test_incomplete_or_invalid_scanner_output_is_fatal(self):
        for text in (
            "",
            "not JSON",
            json.dumps({"type": "summary", "fields": {"advisories": {"errors": 0}}}),
        ):
            with self.subTest(text=text):
                errors, _ = controls.validate_report(text, self.policy["exceptions"])
                self.assertTrue(errors)

    def test_summary_requires_numeric_error_counts(self):
        for statistics in ({}, {"errors": -1}, {"errors": "0"}, None):
            summary = {check: statistics for check in controls.CHECKS}
            text = json.dumps({"type": "summary", "fields": summary})
            with self.subTest(statistics=statistics):
                errors, _ = controls.validate_report(text, {})
                self.assertTrue(errors)

    def test_scan_continues_after_a_failed_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "deny.toml").write_text((ROOT / "deny.toml").read_text())
            with (
                patch.object(controls, "validate_manifests"),
                patch.object(controls, "validate_policy"),
                patch.object(controls.subprocess, "run") as run,
                patch.object(controls, "scan_manifest") as scan,
            ):
                run.return_value.stdout = (
                    f"cargo-deny {self.policy['scanner']['version']}\n"
                )
                scan.side_effect = [
                    (["failure"], set(self.policy["exceptions"])),
                    ([], set()),
                    ([], set()),
                ]
                self.assertEqual(controls.check(root, self.policy, "cargo-deny"), 1)
                self.assertEqual(
                    [call.args[1] for call in scan.call_args_list],
                    self.policy["manifests"],
                )

    def test_scan_uses_locked_resolution_and_retains_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.lock").write_text("version = 4\n")
            (root / "deny.toml").write_text("")
            (root / "supply-chain").mkdir()
            (root / "supply-chain/policy.toml").write_text("")
            output = root / "reports"
            output.mkdir()
            metadata = subprocess.CompletedProcess(
                [], 0, json.dumps({"packages": []}), ""
            )
            scanner = subprocess.CompletedProcess([], 0, "", report())
            with patch.object(
                controls.subprocess, "run", side_effect=[metadata, scanner]
            ) as run:
                errors, _ = controls.scan_manifest(
                    root, "Cargo.toml", "cargo-deny", output, {}
                )
            self.assertFalse(errors)
            for call in run.call_args_list:
                self.assertIn("--locked", call.args[0])
                self.assertIn("--all-features", call.args[0])
            self.assertEqual(
                {path.name for path in output.iterdir()},
                {
                    "workspace-metadata.json",
                    "workspace-deny.jsonl",
                    "workspace-summary.json",
                },
            )


class InstallerTests(unittest.TestCase):
    def archive(self, name="release/cargo-deny", link=False):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w:gz") as stream:
            member = tarfile.TarInfo(name)
            data = b"fixture executable"
            if link:
                member.type = tarfile.SYMTYPE
                member.linkname = "/tmp/fixture"
                stream.addfile(member)
            else:
                member.size = len(data)
                stream.addfile(member, io.BytesIO(data))
        return buffer.getvalue()

    def test_checksum_mismatch_does_not_install_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "bin"
            with self.assertRaises(ValueError):
                controls.install_archive(self.archive(), "0" * 64, destination)
            self.assertFalse(destination.exists())

    def test_verified_archive_installs_only_the_executable(self):
        archive = self.archive("../../cargo-deny")
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "bin"
            controls.install_archive(
                archive, hashlib.sha256(archive).hexdigest(), destination
            )
            self.assertEqual(
                (destination / "cargo-deny").read_bytes(), b"fixture executable"
            )
            self.assertEqual(list(destination.iterdir()), [destination / "cargo-deny"])

    def test_executable_cannot_be_a_symlink(self):
        archive = self.archive(link=True)
        with tempfile.TemporaryDirectory() as directory, self.assertRaises(ValueError):
            controls.install_archive(
                archive, hashlib.sha256(archive).hexdigest(), Path(directory)
            )


class RepositoryTests(unittest.TestCase):
    def test_bitmaps_resolution_excludes_unsound_releases(self):
        for name in ("Cargo.lock", "fuzz/Cargo.lock"):
            with self.subTest(lockfile=name):
                packages = controls.load_toml(ROOT / name)["package"]
                versions = [p["version"] for p in packages if p["name"] == "bitmaps"]
                self.assertEqual(versions, ["3.1.0"])

    def test_bitmaps_policy_blocks_unsound_releases_without_an_ignore(self):
        deny = controls.load_toml(ROOT / "deny.toml")
        self.assertIn(
            "bitmaps:>=3.2.0", [entry["crate"] for entry in deny["bans"]["deny"]]
        )
        ignored = {entry["id"] for entry in deny["advisories"]["ignore"]}
        self.assertNotIn("RUSTSEC-2025-0167", ignored)
        policy = controls.load_toml(ROOT / "supply-chain/policy.toml")
        self.assertNotIn("RUSTSEC-2025-0167", policy["exceptions"])
        self.assertEqual(
            policy["exceptions"]["RUSTSEC-2026-0247"]["versions"], ["3.1.0"]
        )

    def test_selected_workflows_pin_external_actions(self):
        import re

        for name in (
            "ci.yml",
            "deny-schedule.yml",
            "_integration-pipeline.yml",
            "slashing-tests.yml",
        ):
            contents = (ROOT / ".github/workflows" / name).read_text()
            for action in re.findall(
                r"^\s*(?:-\s*)?uses:\s+(\S+)", contents, re.MULTILINE
            ):
                if action.startswith("./"):
                    continue
                with self.subTest(workflow=name, action=action):
                    self.assertRegex(action, r"@[0-9a-f]{40}$")

    def test_deny_workflows_use_verified_scanner_and_keep_failed_reports(self):
        for name in ("ci.yml", "deny-schedule.yml"):
            with self.subTest(workflow=name):
                contents = (ROOT / ".github/workflows" / name).read_text()
                self.assertIn("check_supply_chain.py install", contents)
                self.assertIn("check_supply_chain.py check", contents)
                self.assertIn("path: target/supply-chain/", contents)
                self.assertNotIn("cargo-deny-action@", contents)

    def test_release_build_is_locked_and_has_no_network_or_shared_cache_mount(self):
        contents = (ROOT / "node/Dockerfile").read_text()
        self.assertIn("cargo fetch --locked", contents)
        self.assertIn("RUN --network=none", contents)
        self.assertIn("xx-cargo build --frozen", contents)
        self.assertNotIn("type=cache", contents)
        self.assertNotIn("|| true", contents)
        self.assertIn("sha256sum --check --strict", contents)
        for line in contents.splitlines():
            if line.startswith("FROM ") and " AS " in line:
                self.assertIn("@sha256:", line)

    def test_swagger_assets_are_vendored(self):
        manifest = controls.load_toml(ROOT / "node/Cargo.toml")
        self.assertIn(
            "vendored", manifest["dependencies"]["utoipa-swagger-ui"]["features"]
        )

    def test_unused_certificate_dependencies_are_removed(self):
        manifest = controls.load_toml(ROOT / "comm/Cargo.toml")
        self.assertNotIn("rustls-webpki", manifest["dependencies"])
        self.assertNotIn("webpki", manifest["dependencies"])
        self.assertNotIn("paste", manifest["dependencies"])
        self.assertIn("paste", manifest["dev-dependencies"])


if __name__ == "__main__":
    unittest.main()
