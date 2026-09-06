import argparse
import datetime
import hashlib
import io
import json
import platform
import re
import subprocess
import sys
import tarfile
import tomllib
import urllib.request
from pathlib import Path


CHECKS = ("advisories", "bans", "licenses", "sources")


def load_toml(path):
    with path.open("rb") as stream:
        return tomllib.load(stream)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_policy(policy, deny, today):
    exceptions = policy["exceptions"]
    ignored = deny["advisories"].get("ignore", [])
    identifiers = []
    for entry in ignored:
        if not isinstance(entry, dict) or not entry.get("reason", "").strip():
            raise ValueError("Every advisory exception must have an ID and a reason.")
        identifiers.append(entry["id"])
    if len(set(identifiers)) != len(identifiers) or set(identifiers) != set(exceptions):
        raise ValueError(
            "Advisory exceptions must match supply-chain/policy.toml exactly."
        )
    for identifier, entry in exceptions.items():
        if not entry.get("owner", "").strip() or not entry.get("package", "").strip():
            raise ValueError(f"{identifier}: an owner and a package are required.")
        versions = entry.get("versions", [])
        if not versions or any(
            not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?", v)
            for v in versions
        ):
            raise ValueError(f"{identifier}: exact package versions are required.")
        deadline = entry.get("review-by")
        if type(deadline) is not datetime.date or deadline <= today:
            raise ValueError(f"{identifier}: the exception requires review.")
    expected = {
        "graph": {"all-features": True, "no-default-features": False},
        "advisories": {
            "unsound": "all",
            "unmaintained": "all",
            "yanked": "deny",
            "maximum-db-staleness": "P7D",
        },
        "bans": {"wildcards": "deny", "allow-wildcard-paths": True},
        "sources": {
            "unknown-git": "deny",
            "unknown-registry": "deny",
            "required-git-spec": "rev",
        },
    }
    for section, settings in expected.items():
        for key, value in settings.items():
            if deny.get(section, {}).get(key) != value:
                raise ValueError(f"deny.toml requires {section}.{key} = {value}.")
    build = deny["bans"].get("build", {})
    if not build.get("allow-build-scripts"):
        raise ValueError("Build scripts require an explicit allowance list.")
    if build.get("executables") != "deny" or not build.get("include-dependencies"):
        raise ValueError("Executable scanning must include build dependencies.")
    graph = deny["graph"]
    if any(
        graph.get(key)
        for key in ("exclude", "exclude-dev", "exclude-unpublished", "targets")
    ):
        raise ValueError("The supply-chain graph must not exclude packages or targets.")
    if deny["advisories"].get("disable-yank-checking"):
        raise ValueError("Yank checking must remain enabled.")
    for entry in deny["licenses"].get("clarify", []):
        if not entry.get("license-files") or any(
            not f.get("hash") for f in entry["license-files"]
        ):
            raise ValueError("License clarifications require file hashes.")


def validate_manifests(root, manifests):
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=root,
        check=True,
        capture_output=True,
    )
    discovered = set()
    for name in result.stdout.decode().split("\0"):
        if not name or Path(name).name != "Cargo.toml":
            continue
        located = subprocess.run(
            [
                "cargo",
                "locate-project",
                "--workspace",
                "--manifest-path",
                name,
                "--message-format",
                "plain",
            ],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
        )
        workspace = Path(located.stdout.strip()).resolve().relative_to(root.resolve())
        discovered.add(workspace.as_posix())
    if len(set(manifests)) != len(manifests) or set(manifests) != discovered:
        raise ValueError(
            "The scan manifest list must cover every Cargo workspace exactly once."
        )
    for name in manifests:
        path = Path(name)
        if (
            path.is_absolute()
            or ".." in path.parts
            or not (root / path).with_name("Cargo.lock").is_file()
        ):
            raise ValueError(
                f"{name}: a repository manifest and lockfile are required."
            )
        lock_name = str(path.with_name("Cargo.lock"))
        ignored = subprocess.run(
            ["git", "check-ignore", "-q", lock_name], cwd=root, check=False
        )
        if ignored.returncode != 1:
            raise ValueError(f"{lock_name}: the lockfile must not be ignored.")


def validate_metadata(metadata):
    for package in metadata["packages"]:
        source = package.get("source") or ""
        if not source.startswith("git+"):
            continue
        match = re.search(r"[?&]rev=([0-9a-f]{40})#([0-9a-f]{40})$", source)
        if not match or match[1] != match[2]:
            raise ValueError(
                f"{package['name']}: Git sources require a full, matching commit revision."
            )


def validate_report(text, exceptions):
    errors = []
    encountered = set()
    summary = None
    for line in text.splitlines():
        try:
            record = json.loads(line)
            fields = record["fields"]
            if not isinstance(fields, dict):
                raise ValueError("Invalid scanner fields")
        except (ValueError, KeyError, TypeError):
            errors.append("The scanner produced an invalid JSON record.")
            continue
        if record.get("type") == "summary":
            if summary is not None:
                errors.append("The scanner reported multiple summaries.")
            summary = fields
        if fields.get("severity") in ("error", "bug") or fields.get("level") == "ERROR":
            errors.append(
                f"{fields.get('code', 'scanner')}: {fields.get('message', 'scanner failure')}"
            )
        advisory = fields.get("advisory")
        if not advisory or advisory["id"] not in exceptions:
            continue
        identifier = advisory["id"]
        encountered.add(identifier)
        exception = exceptions[identifier]
        graphs = fields.get("graphs", [])
        if not graphs:
            errors.append(
                f"{identifier}: the scanner did not identify an affected package."
            )
        for graph in graphs:
            package = graph.get("Krate", {})
            if (
                package.get("name") != exception["package"]
                or package.get("version") not in exception["versions"]
            ):
                errors.append(
                    f"{identifier}: the advisory exception does not cover this package version."
                )
    if summary is None or set(summary) != set(CHECKS):
        errors.append("The scanner must report all four checks.")
    else:
        for check in CHECKS:
            result = summary[check]
            if (
                not isinstance(result, dict)
                or type(result.get("errors")) is not int
                or result["errors"] < 0
            ):
                errors.append("The scanner reported invalid check statistics.")
            elif result["errors"]:
                errors.append(f"The scanner reported a failed {check} check.")
    return errors, encountered


def scanner_target():
    architectures = {
        "arm64": "aarch64",
        "aarch64": "aarch64",
        "x86_64": "x86_64",
        "AMD64": "x86_64",
    }
    systems = {"Darwin": "apple-darwin", "Linux": "unknown-linux-musl"}
    return f"{architectures[platform.machine()]}-{systems[platform.system()]}"


def install_scanner(policy, destination):
    scanner = policy["scanner"]
    target = scanner_target()
    version = scanner["version"]
    filename = f"cargo-deny-{version}-{target}.tar.gz"
    url = f"https://github.com/EmbarkStudios/cargo-deny/releases/download/{version}/{filename}"
    with urllib.request.urlopen(url, timeout=120) as response:
        archive = response.read()
    install_archive(archive, scanner["sha256"][target], destination)


def install_archive(archive, expected_digest, destination):
    if hashlib.sha256(archive).hexdigest() != expected_digest:
        raise ValueError("The cargo-deny archive checksum does not match the policy.")
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as stream:
        members = [
            m
            for m in stream.getmembers()
            if m.isfile() and Path(m.name).name == "cargo-deny"
        ]
        if len(members) != 1:
            raise ValueError(
                "The cargo-deny archive must contain exactly one executable."
            )
        member_stream = stream.extractfile(members[0])
        if member_stream is None:
            raise ValueError("The cargo-deny executable cannot be read.")
        executable = member_stream.read()
    destination.mkdir(parents=True, exist_ok=True)
    output = destination / "cargo-deny"
    temporary = destination / "cargo-deny.download"
    with temporary.open("xb") as stream:
        stream.write(executable)
    temporary.chmod(0o755)
    temporary.replace(output)


def scan_manifest(root, manifest, tool, report_dir, exceptions):
    lock = (root / manifest).with_name("Cargo.lock")
    lock_hash = digest(lock)
    prefix = (
        "workspace"
        if manifest == "Cargo.toml"
        else manifest.removesuffix("/Cargo.toml").replace("/", "-")
    )
    metadata_result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--locked",
            "--all-features",
            "--format-version",
            "1",
            "--manifest-path",
            manifest,
        ],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if metadata_result.returncode:
        raise ValueError(
            f"{manifest}: locked Cargo metadata failed: {metadata_result.stderr}"
        )
    try:
        metadata = json.loads(metadata_result.stdout)
    except ValueError as error:
        raise ValueError(f"{manifest}: Cargo metadata is not valid JSON.") from error
    validate_metadata(metadata)
    (report_dir / f"{prefix}-metadata.json").write_text(metadata_result.stdout)
    result = subprocess.run(
        [
            tool,
            "--log-level",
            "info",
            "--format",
            "json",
            "--locked",
            "--all-features",
            "--manifest-path",
            manifest,
            "--config",
            "deny.toml",
            "check",
            "--show-stats",
            *CHECKS,
        ],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    report = result.stdout + result.stderr
    (report_dir / f"{prefix}-deny.jsonl").write_text(report)
    errors, encountered = validate_report(report, exceptions)
    if result.returncode:
        errors.append(f"cargo-deny exited with status {result.returncode}.")
    if digest(lock) != lock_hash:
        errors.append("The scan changed Cargo.lock.")
    evidence = {
        "manifest": manifest,
        "lock_sha256": lock_hash,
        "policy_sha256": digest(root / "deny.toml"),
        "controls_sha256": digest(root / "supply-chain/policy.toml"),
        "metadata_sha256": digest(report_dir / f"{prefix}-metadata.json"),
        "scanner_exit": result.returncode,
        "errors": errors,
    }
    (report_dir / f"{prefix}-summary.json").write_text(
        json.dumps(evidence, indent=2) + "\n"
    )
    for message in errors:
        print(f"{manifest}: {message}", file=sys.stderr)
    print(f"{manifest}: {'FAIL' if errors else 'PASS'}")
    return errors, encountered


def check(root, policy, tool):
    version = subprocess.run(
        [tool, "--version"], check=True, capture_output=True, text=True
    ).stdout.strip()
    if version != f"cargo-deny {policy['scanner']['version']}":
        raise ValueError(
            "Install the cargo-deny version specified in supply-chain/policy.toml."
        )
    validate_policy(
        policy,
        load_toml(root / "deny.toml"),
        datetime.datetime.now(datetime.timezone.utc).date(),
    )
    validate_manifests(root, policy["manifests"])
    report_dir = root / "target/supply-chain"
    report_dir.mkdir(parents=True, exist_ok=True)
    errors = []
    encountered = set()
    for manifest in policy["manifests"]:
        try:
            failures, seen = scan_manifest(
                root, manifest, tool, report_dir, policy["exceptions"]
            )
            errors.extend(failures)
            encountered.update(seen)
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            errors.append(str(error))
            print(f"{manifest}: {error}", file=sys.stderr)
    unused = set(policy["exceptions"]) - encountered
    if unused:
        errors.append("Unused advisory exceptions: " + ", ".join(sorted(unused)))
        print(errors[-1], file=sys.stderr)
    return 1 if errors else 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("install", "check"))
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[2]
    )
    parser.add_argument("--install-dir", type=Path)
    parser.add_argument("--cargo-deny", default="cargo-deny")
    args = parser.parse_args()
    try:
        root = args.root.resolve()
        policy = load_toml(root / "supply-chain/policy.toml")
        if args.command == "install":
            if args.install_dir is None:
                parser.error("install requires --install-dir")
            install_scanner(policy, args.install_dir)
            return 0
        return check(root, policy, args.cargo_deny)
    except (KeyError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Supply-chain check failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
