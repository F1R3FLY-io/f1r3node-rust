import argparse
import hashlib
import json
import math
import os
import re
import sys
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path, PurePosixPath

REQUIRED_SECONDS = 86_400
NANOSECONDS = 1_000_000_000
PROVIDERS = ("docker", "subprocess")
IDENTITY_FIELDS = {
    "target_ref",
    "target_sha",
    "control_sha",
    "harness_sha",
    "image_id",
    "binary_sha256",
    "configuration_sha256",
    "seeds",
    "run_id",
    "run_attempt",
    "session_id",
    "boot_id",
}
MAX_STRUCTURED_BYTES = 16 * 1024 * 1024


class InvalidEvidence(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise InvalidEvidence(message)


def exact_fields(value, fields, label):
    require(
        type(value) is dict and set(value) == set(fields), f"Invalid {label} fields"
    )


def natural(value):
    return type(value) is int and value >= 0


def digest(value, length=64):
    return (
        type(value) is str and re.fullmatch(rf"[0-9a-f]{{{length}}}", value) is not None
    )


def parse_json(contents):
    def object_pairs(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "Duplicate JSON key")
            result[key] = value
        return result

    def constant(_):
        raise InvalidEvidence("Nonfinite JSON number")

    try:
        return json.loads(
            contents, object_pairs_hook=object_pairs, parse_constant=constant
        )
    except (ValueError, UnicodeError) as error:
        raise InvalidEvidence(f"Invalid JSON: {error}") from error


def validate_identity(identity):
    exact_fields(identity, IDENTITY_FIELDS, "identity")
    reference = identity["target_ref"]
    require(
        type(reference) is str
        and reference
        and not any(c.isspace() for c in reference),
        "Invalid target reference",
    )
    for name in ("target_sha", "control_sha", "harness_sha"):
        require(digest(identity[name], 40), f"Invalid {name}")
    image = identity["image_id"]
    require(
        type(image) is str and image.startswith("sha256:") and digest(image[7:]),
        "An immutable Docker image identifier is required",
    )
    for name in ("binary_sha256", "configuration_sha256"):
        require(digest(identity[name]), f"Invalid {name}")
    seeds = identity["seeds"]
    require(
        type(seeds) is list
        and seeds
        and all(natural(seed) and seed < 2**32 for seed in seeds),
        "Explicit unsigned 32-bit Python hash seeds are required",
    )
    require(
        type(identity["run_id"]) is str
        and re.fullmatch(r"[1-9][0-9]*", identity["run_id"]),
        "Invalid run identifier",
    )
    require(
        type(identity["run_attempt"]) is int and identity["run_attempt"] == 1,
        "A restarted workflow cannot qualify",
    )
    for name in ("session_id", "boot_id"):
        try:
            require(
                type(identity[name]) is str
                and str(uuid.UUID(identity[name])) == identity[name],
                f"Invalid {name}",
            )
        except ValueError as error:
            raise InvalidEvidence(f"Invalid {name}") from error


class Artifacts:
    def __init__(self, root):
        self.root = Path(root).resolve(strict=True)
        self.references = []
        self.paths = set()

    def read(self, reference, structured=False):
        exact_fields(reference, {"path", "sha256"}, "artifact reference")
        name = reference["path"]
        require(
            type(name) is str and name and "\\" not in name, "Invalid artifact path"
        )
        path = PurePosixPath(name)
        require(
            not path.is_absolute()
            and path.as_posix() == name
            and all(part not in ("", ".", "..") for part in name.split("/")),
            "Artifact path must remain below its evidence root",
        )
        require(
            name not in self.paths, "An artifact cannot establish two separate results"
        )
        require(digest(reference["sha256"]), "Invalid artifact digest")
        target = self.root
        for part in path.parts:
            target = target / part
            require(
                not target.is_symlink(), "Artifact symbolic links are not permitted"
            )
        try:
            require(target.is_file(), "Missing artifact")
            before = target.stat()
            require(before.st_size > 0, "Empty artifact")
            require(
                not structured or before.st_size <= MAX_STRUCTURED_BYTES,
                "Structured artifact exceeds the size limit",
            )
            checksum = hashlib.sha256()
            contents = bytearray()
            with target.open("rb") as source:
                for block in iter(lambda: source.read(1024 * 1024), b""):
                    checksum.update(block)
                    if structured:
                        contents.extend(block)
                        require(
                            len(contents) <= MAX_STRUCTURED_BYTES,
                            "Structured artifact grew during validation",
                        )
                after = os.fstat(source.fileno())
            require(
                (before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns)
                == (after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns),
                "Artifact changed during validation",
            )
            require(
                checksum.hexdigest() == reference["sha256"], "Artifact digest mismatch"
            )
        except OSError as error:
            raise InvalidEvidence(f"Artifact cannot be read: {error}") from error
        self.paths.add(name)
        self.references.append(dict(reference))
        return bytes(contents)

    def junit(self, reference):
        contents = self.read(reference, structured=True)
        require(
            b"<!DOCTYPE" not in contents.upper()
            and b"<!ENTITY" not in contents.upper(),
            "JUnit declarations are not permitted",
        )
        try:
            root = ET.fromstring(contents)
        except ET.ParseError as error:
            raise InvalidEvidence(f"Invalid JUnit XML: {error}") from error
        require(root.tag in ("testsuite", "testsuites"), "Invalid JUnit root")
        cases = list(root.iter("testcase"))
        require(cases, "JUnit report contains no test cases")
        require(
            not any(list(root.iter(tag)) for tag in ("failure", "error", "skipped")),
            "JUnit report contains failed, erroneous, or skipped tests",
        )
        names = [(case.get("classname"), case.get("name")) for case in cases]
        require(
            all(name for _, name in names) and len(set(names)) == len(names),
            "JUnit test identities are missing or repeated",
        )
        suites = list(root.iter("testsuite"))
        require(suites, "JUnit report contains no test suites")
        for wrapper in root.iter("testsuites"):
            for name, expected in (
                ("tests", len(list(wrapper.iter("testcase")))),
                ("failures", 0),
                ("errors", 0),
                ("skipped", 0),
            ):
                require(
                    wrapper.get(name) in (None, str(expected)),
                    "JUnit wrapper count mismatch",
                )
        direct_case_count = 0
        for suite in suites:
            nested = list(suite.iter("testcase"))
            require(
                suite.get("tests") == str(len(nested)), "JUnit suite count mismatch"
            )
            require(
                all(
                    suite.get(name) == "0" for name in ("failures", "errors", "skipped")
                ),
                "JUnit suite verdict mismatch",
            )
            direct_case_count += len(suite.findall("testcase"))
        require(
            direct_case_count == len(cases), "JUnit test case has no direct suite owner"
        )
        return len(cases)

    def iteration(self, references, index, provider):
        exact_fields(references, {"junit", "metrics", "log"}, "iteration artifacts")
        count = self.junit(references["junit"])
        metrics = parse_json(self.read(references["metrics"], structured=True))
        require(type(metrics) is dict, "Invalid metrics document")
        require(
            type(metrics.get("iteration")) is int and metrics["iteration"] == index,
            "Metrics iteration mismatch",
        )
        require(metrics.get("provider") == provider, "Metrics provider mismatch")
        require(
            type(metrics.get("exit_code")) is int
            and metrics["exit_code"] == 0
            and metrics.get("ok") is True,
            "Metrics report an unsuccessful iteration",
        )
        expected = {"passed": count, "failed": 0, "errors": 0, "skipped": 0}
        observed = metrics.get("pytest")
        exact_fields(observed, expected, "metrics test counts")
        require(
            all(
                type(observed[key]) is int and observed[key] == value
                for key, value in expected.items()
            ),
            "Metrics and JUnit test counts disagree",
        )
        for name in ("rss_peak_mb", "cpu_peak_pct"):
            value = metrics.get(name)
            require(
                (type(value) is int or (type(value) is float and math.isfinite(value)))
                and value >= 0,
                f"Missing or invalid resource measurement: {name}",
            )
        require(metrics["rss_peak_mb"] > 0, "No node resident memory was measured")
        self.read(references["log"])


def inspect_trace(document, evidence_root, expected_identity):
    require(
        not (Path(evidence_root) / "qualification/rejected.json").exists(),
        "The collector permanently rejected this session",
    )
    exact_fields(
        document,
        {
            "schema_version",
            "scope",
            "required_seconds",
            "identity",
            "configuration",
            "preflight",
            "events",
        },
        "qualification document",
    )
    require(
        type(document["schema_version"]) is int and document["schema_version"] == 1,
        "Unsupported qualification schema",
    )
    require(
        document["scope"] == "merge-recovery-iterations",
        "Unsupported qualification scope",
    )
    require(
        type(document["required_seconds"]) is int
        and document["required_seconds"] == REQUIRED_SECONDS,
        "The qualification policy requires 86,400 seconds",
    )
    identity = document["identity"]
    validate_identity(identity)
    validate_identity(expected_identity)
    require(
        identity == expected_identity,
        "The run differs from its separately pinned identity",
    )
    artifacts = Artifacts(evidence_root)
    configuration = document["configuration"]
    exact_fields(configuration, {"path", "sha256"}, "configuration reference")
    require(
        configuration["sha256"] == identity["configuration_sha256"],
        "The configuration digest changed",
    )
    require(
        type(parse_json(artifacts.read(configuration, structured=True))) is dict,
        "The effective configuration must be a JSON object",
    )
    preflight = document["preflight"]
    exact_fields(preflight, {"outcome", "junit"}, "preflight")
    require(preflight["outcome"] == "passed", "The integration preflight did not pass")
    artifacts.junit(preflight["junit"])
    events = document["events"]
    require(
        type(events) is list and len(events) >= 1,
        "Incomplete qualification event sequence",
    )
    started = previous = None
    pending = None
    qualified = iterations = 0
    seen = set()
    finished = False
    for position, event in enumerate(events):
        require(type(event) is dict, "Invalid event")
        require(not finished, "Events occur after final publication")
        validate_identity(event.get("identity"))
        require(event.get("identity") == identity, "The recorded run identity changed")
        at = event.get("at_ns")
        require(
            natural(at) and (previous is None or at >= previous),
            "Invalid monotonic event timestamp",
        )
        previous = at
        kind = event.get("kind")
        common = {"kind", "at_ns", "identity"}
        if kind == "start":
            exact_fields(event, common, "start event")
            require(
                position == 0 and started is None,
                "A qualification session cannot restart",
            )
            started = at
        elif kind == "iteration_start":
            exact_fields(
                event, common | {"iteration", "provider", "seed"}, "iteration start"
            )
            require(
                started is not None and pending is None, "Invalid iteration start order"
            )
            index = event["iteration"]
            require(
                type(index) is int and index == iterations + 1,
                "Invalid iteration identifier",
            )
            provider = PROVIDERS[iterations % len(PROVIDERS)]
            require(event["provider"] == provider, "The provider schedule changed")
            seed = identity["seeds"][iterations % len(identity["seeds"])]
            require(
                type(event["seed"]) is int and event["seed"] == seed,
                "The seed schedule changed",
            )
            pending = (index, provider, at)
        elif kind == "iteration_end":
            exact_fields(
                event, common | {"iteration", "exit_code", "artifacts"}, "iteration end"
            )
            require(pending is not None, "Iteration end has no matching start")
            index, provider, begin = pending
            require(
                type(event["iteration"]) is int and event["iteration"] == index,
                "Iteration result identifier mismatch",
            )
            require(
                type(event["exit_code"]) is int and event["exit_code"] == 0,
                "An iteration did not complete successfully",
            )
            require(at > begin, "Iteration duration must be positive")
            artifacts.iteration(event["artifacts"], index, provider)
            qualified += at - begin
            iterations += 1
            seen.add(provider)
            pending = None
        elif kind == "finish":
            exact_fields(event, common | {"outcome"}, "finish event")
            require(
                started is not None and pending is None,
                "Incomplete session at publication",
            )
            require(
                event["outcome"] == "completed", "The session did not finish normally"
            )
            require(
                qualified >= REQUIRED_SECONDS * NANOSECONDS,
                "Insufficient qualified duration",
            )
            require(
                at - started >= qualified, "Qualified duration exceeds elapsed duration"
            )
            require(
                seen == set(PROVIDERS),
                "Both providers must complete successful iterations",
            )
            finished = True
        else:
            raise InvalidEvidence("The trace contains an invalidating or unknown event")
    return {
        "schema_version": 1,
        "scope": document["scope"],
        "qualified": False,
        "trace_complete": finished,
        "required_seconds": REQUIRED_SECONDS,
        "qualified_ns": qualified,
        "elapsed_ns": previous - started,
        "iterations": iterations,
        "providers": sorted(seen),
        "identity": dict(identity),
        "verdicts": {
            "workflow": "unconfirmed",
            "preflight": "passed",
            "iterations": "passed",
            "identity": "unchanged",
        },
        "artifacts": artifacts.references,
        "trace_sha256": hashlib.sha256(
            json.dumps(document, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest(),
    }


def verify(document, evidence_root, workflow_conclusion, expected_identity):
    require(
        workflow_conclusion == "success", "The workflow did not complete successfully"
    )
    result = inspect_trace(document, evidence_root, expected_identity)
    require(result["trace_complete"], "The qualification session has no final result")
    result["qualified"] = True
    result["verdicts"]["workflow"] = "success"
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("trace", type=Path)
    parser.add_argument("--evidence-root", type=Path, required=True)
    parser.add_argument("--workflow-conclusion", required=True)
    parser.add_argument("--expected-identity", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        with args.trace.open("rb") as source:
            contents = source.read(MAX_STRUCTURED_BYTES + 1)
        require(
            len(contents) <= MAX_STRUCTURED_BYTES,
            "Qualification trace exceeds the size limit",
        )
        with args.expected_identity.open("rb") as source:
            identity_contents = source.read(MAX_STRUCTURED_BYTES + 1)
        require(
            len(identity_contents) <= MAX_STRUCTURED_BYTES,
            "Expected identity exceeds the size limit",
        )
        result = verify(
            parse_json(contents),
            args.evidence_root,
            args.workflow_conclusion,
            parse_json(identity_contents),
        )
        status = 0
    except (InvalidEvidence, OSError) as error:
        result = {"schema_version": 1, "qualified": False, "reason": str(error)}
        status = 1
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"qualified": result["qualified"], "manifest": str(args.output)}))
    return status


if __name__ == "__main__":
    sys.exit(main())
