import argparse
import fcntl
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

from qualification import (
    PROVIDERS,
    REQUIRED_SECONDS,
    InvalidEvidence,
    digest,
    inspect_trace,
    parse_json,
    require,
    validate_identity,
)

CONFIG_ENV = (
    "SOAK_RSS_CEILING_MB",
    "SOAK_HOST_FREE_FLOOR_MB",
    "SOAK_DISK_FREE_FLOOR_MB",
    "SOAK_DISK_HYGIENE_BAND_MB",
    "SOAK_GUARDIAN_POLL_SECONDS",
    "SOAK_MONITOR_SNAPSHOT_SECONDS",
    "SOAK_HOST_GUARDIAN_POLL_SECONDS",
    "SOAK_HOST_GUARDIAN_CONSECUTIVE",
    "SOAK_QUALIFICATION_SEEDS",
    "F1R3_MAX_PARALLEL_BLOCKS",
    "RUST_LOG",
    "RUST_BACKTRACE",
    "MALLOC_ARENA_MAX",
    "GLIBC_TUNABLES",
    "LOAD_TEST_TELEMETRY_ONLY",
    "PYTEST_ADDOPTS",
)
LOAD_TEST = "integration-tests/test/tests/custom/test_load.py"


def command(arguments, directory=None):
    result = subprocess.run(
        arguments,
        cwd=directory,
        check=False,
        capture_output=True,
        text=True,
        timeout=60,
    )
    require(result.returncode == 0, f"Required {arguments[0]} observation failed")
    return result.stdout.strip()


def file_digest(path):
    checksum = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            checksum.update(block)
    return checksum.hexdigest()


def encoded(document):
    return json.dumps(document, sort_keys=True, indent=2).encode() + b"\n"


def atomic_write(path, document):
    with tempfile.NamedTemporaryFile(
        dir=path.parent, prefix=".write-", delete=False
    ) as output:
        temporary = Path(output.name)
        try:
            output.write(encoded(document))
            output.flush()
            os.fsync(output.fileno())
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)


def artifact(root, path):
    path = Path(path)
    return {"path": path.relative_to(root).as_posix(), "sha256": file_digest(path)}


def git_revision(path):
    revision = command(["git", "-c", "core.fsmonitor=false", "rev-parse", "HEAD"], path)
    require(digest(revision, 40), "A full Git revision is required")
    command(
        ["git", "-c", "core.fsmonitor=false", "diff", "--quiet", "HEAD", "--"], path
    )
    return revision


def resolve_reference(node, reference):
    if digest(reference, 40):
        return reference
    references = (
        [reference]
        if reference.startswith("refs/")
        else [
            f"refs/heads/{reference}",
            f"refs/tags/{reference}",
        ]
    )
    output = command(
        [
            "git",
            "ls-remote",
            "origin",
            *references,
            *(f"{item}^{{}}" for item in references),
        ],
        node,
    )
    entries = dict(line.split()[::-1] for line in output.splitlines() if line.strip())
    matches = {
        entries.get(f"{item}^{{}}", entries[item])
        for item in references
        if item in entries
    }
    require(len(matches) == 1, "The target reference is missing or ambiguous")
    return matches.pop()


class Collector:
    def __init__(self, root):
        self.root = Path(root).resolve()
        self.directory = self.root / "qualification"
        self.directory.mkdir(parents=True, exist_ok=True)
        self.node = Path(os.environ["SOAK_NODE_REPO_DIR"])
        self.harness = Path(os.environ["SYSTEM_INTEGRATION_DIR"])
        self.control = Path(__file__).resolve().parents[2]

    def read(self, name):
        return parse_json((self.directory / name).read_bytes())

    def configuration(self):
        require(
            not os.environ.get("LOAD_TEST_TELEMETRY_ONLY"),
            "Telemetry-only mode cannot qualify",
        )
        require(
            not os.environ.get("PYTEST_ADDOPTS"),
            "Qualification cannot inherit pytest selection overrides",
        )
        return {
            "environment": {name: os.environ.get(name) for name in CONFIG_ENV},
            "node_defaults_sha256": file_digest(
                self.node / "node/src/main/resources/defaults.conf"
            ),
            "load_test_sha256": file_digest(self.harness / LOAD_TEST),
            "harness_lock_sha256": file_digest(self.harness / "poetry.lock"),
            "providers": list(PROVIDERS),
            "workload": LOAD_TEST,
            "pytest_timeout_seconds": 1200,
            "iteration_timeout_seconds": 1800,
            "seed_kind": "PYTHONHASHSEED",
            "required_seconds": REQUIRED_SECONDS,
        }

    def observe(self, initial):
        image = (
            os.environ.get("F1R3FLY_NODE_IMAGE")
            or os.environ["SOAK_QUALIFICATION_IMAGE"]
        )
        observed = {
            "target_ref": os.environ["SOAK_TARGET_REF"],
            "target_sha": git_revision(self.node),
            "control_sha": git_revision(self.control),
            "harness_sha": git_revision(self.harness),
            "image_id": command(
                ["docker", "image", "inspect", "--format", "{{.Id}}", image]
            ),
            "binary_sha256": file_digest(os.environ["F1R3FLY_NODE_BINARY"]),
            "configuration_sha256": hashlib.sha256(
                encoded(self.configuration())
            ).hexdigest(),
            "seeds": parse_json(os.environ["SOAK_QUALIFICATION_SEEDS"]),
            "run_id": os.environ["GITHUB_RUN_ID"],
            "run_attempt": int(os.environ["GITHUB_RUN_ATTEMPT"]),
            "session_id": initial["session_id"],
            "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip(),
        }
        validate_identity(observed)
        require(
            resolve_reference(self.node, observed["target_ref"])
            == observed["target_sha"],
            "The target reference moved",
        )
        require(
            observed["target_sha"] == os.environ["SOAK_TARGET_SHA"],
            "The candidate revision changed",
        )
        require(
            observed["control_sha"] == os.environ["SOAK_CONTROL_SHA"],
            "The control revision changed",
        )
        require(
            observed["harness_sha"] == os.environ["SYSTEM_INTEGRATION_REF"],
            "The harness revision changed",
        )
        return observed

    def owner(self):
        pid = os.environ["SOAK_QUALIFICATION_OWNER_PID"]
        require(pid.isdigit() and int(pid) > 0, "Invalid driver process identifier")
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return {"pid": pid, "start_ticks": fields[19]}

    def prepare(self):
        require(
            not (self.directory / "identity.json").exists(),
            "Qualification preparation cannot restart",
        )
        initial = {"session_id": str(uuid.uuid4())}
        identity = self.observe(initial)
        configuration = self.configuration()
        require(
            hashlib.sha256(encoded(configuration)).hexdigest()
            == identity["configuration_sha256"],
            "Configuration changed during preparation",
        )
        atomic_write(self.directory / "configuration.json", configuration)
        atomic_write(self.directory / "identity.json", identity)
        return identity["image_id"]

    def event(self, kind, fields, at_ns=None):
        identity = self.read("identity.json")
        observed = self.observe(identity)
        require(observed == identity, "The effective qualification inputs changed")
        if at_ns is None:
            at_ns = time.monotonic_ns()
        require(at_ns <= time.monotonic_ns(), "An event timestamp lies in the future")
        return {"kind": kind, "at_ns": at_ns, "identity": observed, **fields}

    def start(self):
        require(
            not (self.directory / "trace.json").exists(),
            "Qualification sessions cannot resume",
        )
        require(
            os.environ.get("SOAK_PREFLIGHT_RESULT") == "passed",
            "Full integration preflight is required",
        )
        identity = self.read("identity.json")
        trace = {
            "schema_version": 1,
            "scope": "merge-recovery-iterations",
            "required_seconds": REQUIRED_SECONDS,
            "identity": identity,
            "configuration": artifact(self.root, self.directory / "configuration.json"),
            "preflight": {
                "outcome": "passed",
                "junit": artifact(self.root, self.root / "preflight/junit.xml"),
            },
            "events": [self.event("start", {})],
        }
        inspect_trace(trace, self.root, identity)
        atomic_write(self.directory / "owner.json", self.owner())
        atomic_write(self.directory / "trace.json", trace)
        return 0

    def append(self, kind, fields, at_ns=None):
        require(
            self.owner() == self.read("owner.json"),
            "The original driver process no longer owns this session",
        )
        trace = self.read("trace.json")
        trace["events"].append(self.event(kind, fields, at_ns))
        result = inspect_trace(trace, self.root, self.read("identity.json"))
        atomic_write(self.directory / "trace.json", trace)
        return result

    def begin(self, index, provider):
        identity = self.read("identity.json")
        seed = identity["seeds"][(index - 1) % len(identity["seeds"])]
        self.append(
            "iteration_start", {"iteration": index, "provider": provider, "seed": seed}
        )
        return seed

    def end(self, index, directory, exit_code, at_ns):
        references = {
            name: artifact(self.root, Path(directory) / filename)
            for name, filename in (
                ("junit", "junit.xml"),
                ("metrics", "metrics.json"),
                ("log", "pytest.log"),
            )
        }
        result = self.append(
            "iteration_end",
            {"iteration": index, "exit_code": exit_code, "artifacts": references},
            at_ns,
        )
        return result["qualified_ns"]

    def finish(self):
        for name in (
            "early-exit.txt",
            "protection-breach.txt",
            "host-guardian-breach.txt",
            "finalize-requested",
        ):
            require(
                not (self.root / name).exists(),
                "The driver recorded an interrupted run",
            )
        signal = self.root / "signal"
        require(
            not signal.exists() or signal.read_text().strip() != "finalize",
            "The operator requested finalization",
        )
        result = self.append("finish", {"outcome": "completed"})
        atomic_write(self.directory / "local-result.json", result)
        return result["qualified_ns"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "operation", choices=("prepare", "start", "begin", "end", "finish")
    )
    parser.add_argument("--iteration", type=int)
    parser.add_argument("--provider", choices=PROVIDERS)
    parser.add_argument("--directory", type=Path)
    parser.add_argument("--exit-code", type=int)
    parser.add_argument("--at-ns", type=int)
    args = parser.parse_args()
    collector = Collector(os.environ["SOAK_OUTPUT_DIR"])
    with (collector.directory / "collector.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            require(
                not (collector.directory / "rejected.json").exists(),
                "This qualification session is permanently rejected",
            )
            if args.operation in ("prepare", "start", "finish"):
                result = getattr(collector, args.operation)()
            elif args.operation == "begin":
                require(
                    args.iteration is not None
                    and args.iteration > 0
                    and args.provider is not None,
                    "Iteration and provider are required",
                )
                result = collector.begin(args.iteration, args.provider)
            else:
                require(
                    args.iteration is not None
                    and args.iteration > 0
                    and args.directory is not None
                    and args.exit_code is not None
                    and args.at_ns is not None,
                    "Complete iteration result arguments are required",
                )
                result = collector.end(
                    args.iteration, args.directory, args.exit_code, args.at_ns
                )
            print(result)
            return 0
        except (
            InvalidEvidence,
            OSError,
            ValueError,
            KeyError,
            subprocess.SubprocessError,
        ) as error:
            if not (collector.directory / "rejected.json").exists():
                atomic_write(
                    collector.directory / "rejected.json",
                    {"qualified": False, "reason": str(error)},
                )
            print(f"Qualification rejected: {error}", file=sys.stderr)
            return 1


if __name__ == "__main__":
    sys.exit(main())
