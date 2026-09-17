import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from casper_soak_manifest import parse, read_regular, require, validate


ROOT = Path(__file__).resolve().parents[2]
PROFILES = {
    name.replace("_", "-"): f"scripts/bench/casper_soak_profiles/{name}.py"
    for name in ("authority_finality", "publication", "recovery", "merge_accounting", "slashing", "version_phlo", "carrier_index")
}
PROFILES["harness-lifecycle"] = "scripts/bench/fixtures/casper_lifecycle_profile.py"
CORE = ("scripts/run-merge-recovery-soak.sh", "scripts/bench/casper_soak_manifest.py", "scripts/bench/casper_soak_runtime.py")
IDENTITY = ("manifest_digest", "run_id", "scenario_id", "pair_id", "member_id", "node", "incarnation", "segment", "iteration")


class Blocked(ValueError):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False) + "\n").encode()


def relative(root, name):
    require(isinstance(name, str) and bool(name), "An artifact path is missing.")
    path = Path(name)
    require(not path.is_absolute() and str(path) == name and ".." not in path.parts and "\\" not in name, "An artifact path escapes its root.")
    current = Path(root)
    require(not current.is_symlink(), "An artifact root is a symbolic link.")
    for part in path.parts:
        current = current / part
        require(not current.is_symlink(), "An artifact path contains a symbolic link.")
    return current


def artifact(root, reference):
    require(isinstance(reference, dict), "An artifact reference is invalid.")
    data = read_regular(relative(root, reference["path"]))
    require(type(reference["bytes"]) is int and len(data) == reference["bytes"], "An artifact length differs.")
    require(digest(data) == reference["sha256"], "An artifact digest differs.")
    return data


def exclusive(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=".capture-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.link(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        os.unlink(temporary)


def load_module(name, path):
    specification = importlib.util.spec_from_file_location(name, path)
    if specification is None or specification.loader is None:
        raise ValueError("The profile module cannot load.")
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


def unsigned(value):
    require(isinstance(value, str) and value.isascii() and value.isdigit() and (value == "0" or not value.startswith("0")), "A decimal integer is invalid.")
    require(len(value) <= 20, "A decimal integer exceeds its bound.")
    try:
        return int(value)
    except ValueError as error:
        raise ValueError("The decimal integer is invalid.") from error


def true(value):
    return isinstance(value, bool) and value


def admission(iteration=1):
    data = read_regular(os.environ["SOAK_MANIFEST_PATH"])
    manifest = parse(data)
    validate(manifest)
    inputs = Path(os.environ["SOAK_INPUT_DIR"])
    approval_data = read_regular(os.environ["SOAK_APPROVAL_PATH"])
    require(digest(approval_data) == os.environ["SOAK_APPROVAL_SHA256"], "The approval digest differs.")
    approval = parse(approval_data)
    require(approval["manifest_digest"] == digest(data) and approval["evidence_kind"] == manifest["evidence_kind"], "The approval identifies different inputs.")
    if not true(approval.get("approved")):
        raise Blocked("The candidate has no approval.")
    profile = manifest["profile_id"]
    require(profile in PROFILES, "The profile is not registered.")
    if profile == "harness-lifecycle":
        require(manifest["evidence_kind"] == "synthetic_fixture", "The lifecycle fixture cannot produce node observations.")
    module_path = ROOT / PROFILES[profile]
    if not module_path.is_file():
        raise Blocked("The profile adapter is unavailable.")
    required_sources = (*CORE, PROFILES[profile])
    for name in required_sources:
        require(name in manifest["source_digests"], "A required source pin is missing.")
    for name, expected in manifest["source_digests"].items():
        require(digest(read_regular(relative(ROOT, name))) == expected, "The executable source bytes differ.")
    runtime = manifest["runtime"]
    require(type(runtime["iterations"]) is int and 1 <= runtime["iterations"] <= 100000, "The iteration bound is invalid.")
    require(type(runtime["iterations_per_segment"]) is int and 1 <= runtime["iterations_per_segment"] <= runtime["iterations"], "The segment bound is invalid.")
    require(runtime["required_capabilities"], "The capability inventory is empty.")
    for name in runtime["required_capabilities"]:
        capability = manifest["capabilities"].get(name, {})
        if capability.get("status") != "qualified":
            raise Blocked("A required capability is not qualified.")
        require(capability["provider"] == manifest["provider"] and capability["revision"] == manifest["node_revision"], "Capability qualification identifies another candidate.")
        reference = capability["qualification"]
        qualification = parse(artifact(inputs, reference))
        require(reference["sha256"] in approval["qualification_digests"], "Capability evidence has no approval.")
        require(qualification == {"capability": name, "provider": manifest["provider"], "node_revision": manifest["node_revision"], "status": "qualified", "evidence_kind": manifest["evidence_kind"]}, "Capability qualification is inconsistent.")
    assets = runtime["assets"]
    contents = {name: artifact(inputs, ref) for name, ref in assets.items()}
    for name, field in (("configuration", "configuration_digest"), ("fixture", "fixture_digest"), ("expectation", "expectation_digest"), ("node_binary", "node_binary_digest")):
        require(digest(contents[name]) == manifest[field], "A pinned input differs.")
    require(digest(read_regular(module_path)) == manifest["profile_digest"], "The profile digest differs.")
    require(assets["executor"]["sha256"] == approval["executor_digest"], "The executor is not approved.")
    if manifest["provider"] == "docker":
        require(digest(contents["image_manifest"]) == manifest["image_digest"], "The image manifest digest differs.")
    configuration = parse(contents["configuration"])
    limits = manifest["resource_limits"]
    require(limits["children"] == 1 and type(limits["children"]) is int, "The driver supports one workload child.")
    for key in ("timeout_seconds", "rss_ceiling_mb", "host_free_floor_mb", "disk_free_floor_mb", "artifact_bytes"):
        require(type(limits[key]) is int and limits[key] >= 0, "A resource limit is invalid.")
    require(limits["timeout_seconds"] > 0 and limits["artifact_bytes"] > 0, "A resource bound is empty.")
    bindings = {"SOAK_DURATION_SECONDS": "duration_seconds", "SOAK_RSS_CEILING_MB": "rss_ceiling_mb", "SOAK_HOST_FREE_FLOOR_MB": "host_free_floor_mb", "SOAK_DISK_FREE_FLOOR_MB": "disk_free_floor_mb"}
    for variable, key in bindings.items():
        require(type(configuration[key]) is int and str(configuration[key]) == os.environ.get(variable), "The effective configuration differs.")
        if key in limits:
            require(configuration[key] == limits[key], "The resource configuration differs.")
    require(configuration["policy_variant"] == manifest["policy_variant"], "The policy configuration differs.")
    require(os.environ.get("SOAK_RUN_BENCHMARKS", "false") == "false", "Profile runs cannot add unpinned benchmarks.")
    require(manifest["deadline"]["clock_id"] == "unix-seconds", "The deadline clock is unsupported.")
    unsigned(manifest["deadline"]["epoch_seconds"])
    require(manifest["tool_versions"]["python"] == sys.version.split()[0], "The Python version differs.")
    if manifest["phase"] == "post_pr216_merge":
        gate = manifest["merge_gate"]
        proof = parse(artifact(inputs, gate["ancestry"]))
        require(gate["ancestry"]["sha256"] in approval["qualification_digests"], "The merge evidence is not approved.")
        require(proof["pr"] == 216 and true(proof["merged"]) and true(proof["accepted_handoff"]), "The actual merge and accepted handoff are required.")
        require(proof["merge_revision"] == gate["merge_revision"] and proof["selected_dev_revision"] == manifest["node_revision"], "The merge identity differs.")
        result = subprocess.run(["git", "-C", os.environ["SOAK_NODE_REPO_DIR"], "merge-base", "--is-ancestor", proof["merge_revision"], proof["selected_dev_revision"]], timeout=10, check=False, capture_output=True)
        require(result.returncode == 0, "The merge is not in selected dev ancestry.")
    envelope = parse(contents["request"])
    requests = envelope.get("scenarios", [envelope])
    require(isinstance(requests, list) and 0 < len(requests) <= manifest["bounds"]["scenarios"], "The scenario inventory exceeds its bound.")
    identities = [item["scenario_id"] for item in requests]
    require(len(set(identities)) == len(identities) and set(identities) == set(manifest["required_scenarios"]), "The scenario inventory differs.")
    require(runtime["iterations"] >= len(requests), "The iteration bound cannot cover the scenarios.")
    request = requests[(iteration - 1) % len(requests)]
    require(request["seed"] == manifest["seed"], "The request seed differs.")
    require(request["required_observations"], "The required observation inventory is empty.")
    request = {**request, "fixture": parse(contents["fixture"]), "expectation": parse(contents["expectation"]), "policy_variant": manifest["policy_variant"]}
    module = load_module("casper_profile", module_path)
    return manifest, digest(data), request, module, inputs, approval


def history(output):
    entries = []
    previous = None
    for path in sorted((output / "casper-history").glob("*.json")):
        raw = read_regular(path)
        entry = parse(raw)
        require(entry["iteration"] == len(entries) + 1 and entry["previous_digest"] == previous, "The history chain is inconsistent.")
        require(path.name == f'{entry["iteration"]:08d}.json', "The history filename differs.")
        for reference in entry["artifact_inventory"]:
            artifact(output, reference)
        entries.append(entry)
        previous = digest(raw)
    return entries, previous


def check_history(output, iterations, failures=0):
    entries, _ = history(output)
    require(len(entries) == iterations, "The checkpoint iteration differs from retained history.")
    require(all(entry["capture_complete"] for entry in entries), "An incomplete capture prevents another launch or cleanup.")
    require(failures >= sum(entry["scenario_verdict"] != "passed" for entry in entries), "The checkpoint erased failure history.")
    directories = list(output.glob("iteration-*"))
    require(len(directories) == iterations, "An uncommitted iteration cannot be reused.")
    return entries


def correlate(manifest, manifest_digest, request, records, segment, iteration, directory):
    accepted, rejected, receipts = [], [], []
    ids, events, sequences = set(), {}, {}
    required = request["required_observations"]
    for record in records:
        try:
            require(record["schema_version"] == 1 and type(record["schema_version"]) is int, "Unsupported observation schema.")
            for key in ("record_id", "producer", "event_id", "event_kind", "node", "incarnation", "member_id"):
                require(isinstance(record[key], str) and bool(record[key]), "Missing observation identity.")
            require(record["record_id"] not in ids, "Duplicate transport record.")
            ids.add(record["record_id"])
            expected = {"manifest_digest": manifest_digest, "run_id": manifest["run_id"], "scenario_id": request["scenario_id"], "pair_id": request["pair_id"], "segment": segment, "iteration": iteration}
            require(all(record[key] == value for key, value in expected.items()), "Observation identity differs.")
            require(any(all(record[key] == item[key] for key in ("node", "incarnation", "member_id")) for item in required), "Observation context differs.")
            sequence = unsigned(record["producer_sequence"])
            producer = (record["producer"], record["node"], record["incarnation"])
            require(sequence > sequences.get(producer, -1), "Producer sequence is not increasing.")
            sequences[producer] = sequence
            clock = record["observation_time"]
            require(isinstance(clock["clock_id"], str) and bool(clock["clock_id"]), "Missing observation clock.")
            unsigned(clock["monotonic_ns"])
            require(isinstance(clock["utc"], str) and bool(clock["utc"]), "Missing UTC observation time.")
            presence = record["presence"]
            require(presence in ("observed", "missing", "error"), "Invalid presence state.")
            if presence != "observed":
                require(record["payload"] is None and bool(record["reason"]), "Unknown observations need a null payload and reason.")
            reference = record["raw_reference"]
            artifact(directory, reference)
            require(reference["capture_state"] == "complete" and reference["producer"] == record["producer"] and record["record_id"] in reference["observation_ids"], "The raw reference is not correlated.")
            event = tuple(record[key] for key in IDENTITY) + (record["event_id"],)
            payload = encoded({key: record.get(key) for key in ("event_kind", "payload", "presence", "reason")})
            if event in events:
                require(events[event] == payload, "Conflicting copies of one event.")
                continue
            events[event] = payload
            if record["event_kind"] == "fault_acknowledgment":
                receipt = record["payload"]
                fault = next((item for item in request["fault_schedule"] if item["fault_id"] == receipt["fault_id"]), None)
                if fault is None or receipt["status"] != "applied":
                    raise ValueError("A fault has no application receipt.")
                require(receipt["action"] == fault["action"] and record["node"] == fault["node"] and record["incarnation"] == fault["incarnation"], "The fault receipt differs.")
                require(receipt["observed_state"] == fault["required_state"], "The requested fault state was not observed.")
                if receipt["action"] == "restart":
                    require(true(receipt["prior_exit"]) and true(receipt["ready"]) and receipt["predecessor"] != record["incarnation"], "The restart lacks an incarnation transition.")
                    require(receipt["predecessor"] == fault["predecessor"], "The restart predecessor differs.")
                receipts.append(record)
            else:
                accepted.append(record)
        except (ValueError, KeyError, TypeError, OSError) as error:
            rejected.append({"record_id": record.get("record_id") if isinstance(record, dict) else None, "reason": type(error).__name__})
    missing = [item for item in required if not any(record["presence"] == "observed" and all(record.get(key) == value for key, value in item.items()) for record in accepted)]
    return accepted, rejected, receipts, missing


def run(output, directory, segment, iteration):
    manifest, manifest_digest, request, module, inputs, approval = admission(iteration)
    entries, previous = history(output)
    require(iteration == len(entries) + 1, "The next iteration would overwrite history.")
    request = {**request, "manifest_digest": manifest_digest, "run_id": manifest["run_id"], "segment": segment, "iteration": iteration}
    generated = module.generate(parse(encoded(request)))
    require(generated["faults"] == request["fault_schedule"], "The generated fault schedule differs.")
    exclusive(directory / "request.json", encoded({"request": request, "generated": generated}))
    executor = relative(inputs, manifest["runtime"]["assets"]["executor"]["path"])
    command = [sys.executable, str(executor), str(directory / "request.json"), str(directory)]
    exclusive(directory / "launch.json", encoded({"manifest_digest": manifest_digest, "iteration": iteration, "executor_digest": approval["executor_digest"], "launch_count": 1}))
    try:
        result = subprocess.run(command, check=False, timeout=manifest["resource_limits"]["timeout_seconds"])
        return result.returncode
    except subprocess.TimeoutExpired:
        return 124


def finish(output, directory, segment, iteration, status, termination):
    manifest, manifest_digest, request, module, _, _ = admission(iteration)
    entries, previous = history(output)
    require(iteration == len(entries) + 1, "The next iteration would overwrite history.")
    transport_error = None
    try:
        transport = parse(read_regular(directory / "transport.json"))
        records = transport["observations"]
        require(isinstance(records, list) and all(isinstance(record, dict) for record in records), "The observation transport is invalid.")
        require(len(records) <= manifest["bounds"]["observations"], "The observation bound was exceeded.")
    except (OSError, ValueError, KeyError, TypeError) as error:
        records = []
        transport_error = type(error).__name__
    observations, rejected, receipts, missing = correlate(manifest, manifest_digest, request, records, segment, iteration, directory)
    collected = module.collect(manifest, {"observations": observations, "rejected_sources": rejected})
    scenario = module.classify(request, collected["observations"], receipts)
    require(scenario["scenario_verdict"] in ("passed", "product_failure", "incomplete", "blocked", "invalid_input"), "The profile verdict is invalid.")
    require(isinstance(scenario["product_failures"], list), "The failure inventory is invalid.")
    faults_missing = set(item["fault_id"] for item in request["fault_schedule"]) - set(item["payload"]["fault_id"] for item in receipts)
    inventory = []
    capture_error = None
    try:
        total = 0
        count = 0
        captured = {}
        for path in directory.rglob("*"):
            count += 1
            require(count <= 1000, "The artifact count exceeds the capture bound.")
            if path.is_dir():
                require(not path.is_symlink(), "A capture directory is a symbolic link.")
                continue
            data = read_regular(path)
            total += len(data)
            require(total <= manifest["resource_limits"]["artifact_bytes"], "The artifact budget was exceeded.")
            name = str(Path("casper-capture") / f"{iteration:08d}" / path.relative_to(directory))
            target = relative(output, name)
            exclusive(target, data)
            reference = {"path": name, "bytes": len(data), "sha256": digest(data)}
            artifact(output, reference)
            inventory.append(reference)
            captured[str(path.relative_to(directory))] = reference
        for record in observations + receipts:
            source = record["raw_reference"]
            retained = captured.get(source["path"], {})
            require(retained.get("sha256") == source["sha256"] and retained.get("bytes") == source["bytes"], "The correlated artifact changed before capture.")
    except (OSError, ValueError) as error:
        capture_error = type(error).__name__
    failures = scenario["product_failures"]
    if failures:
        verdict = "product_failure"
    elif missing or rejected or faults_missing or transport_error or capture_error or status != 0 or termination != "completed":
        verdict = "incomplete"
    else:
        verdict = scenario["scenario_verdict"]
    entry = {"schema_version": 1, "manifest_digest": manifest_digest, "scenario_id": request["scenario_id"], "segment": segment, "iteration": iteration, "previous_digest": previous, "scenario_verdict": verdict, "product_failures": failures, "termination": termination, "exit_code": status, "missing_observations": missing, "rejected_observations": rejected, "missing_faults": sorted(faults_missing), "fault_receipts": receipts, "observations": observations, "transport_error": transport_error, "capture_error": capture_error, "capture_complete": capture_error is None and transport_error is None and not rejected, "artifact_inventory": inventory}
    exclusive(output / "casper-history" / f"{iteration:08d}.json", encoded(entry))
    return 0 if verdict == "passed" else 1


def verification(manifest, inputs, approval):
    assets = manifest["runtime"]["assets"]
    if "verification" not in assets:
        return "pending"
    reference = assets["verification"]
    report = parse(artifact(inputs, reference))
    require(reference["sha256"] in approval["qualification_digests"], "The verification receipt has no approval.")
    require(report["phase"] == manifest["phase"] and report["claim_id"] == "CLAIM-CASPER-SOAK-001", "The verification scope differs.")
    require(report["source_digests"] == manifest["source_digests"], "The verification source identity differs.")
    require(report["construction"] == "not-applicable", "Construction cannot discharge this claim.")
    require(report["status"] == "passed", "The verification did not pass.")
    require(set(report["bindings"]) == {f"H{number:02d}" for number in range(1, 11)} and all(true(value) for value in report["bindings"].values()), "A required driver binding is missing.")
    if report["origin"] == "fixture-substitute":
        require(manifest["profile_id"] == "harness-lifecycle" and manifest["evidence_kind"] == "synthetic_fixture", "A fixture receipt cannot qualify node observations.")
        return "passed"
    require(report["origin"] == "executed-verifier", "The verification origin is unsupported.")
    model_runner = load_module("casper_model_runner", ROOT / "scripts/ci/check_casper_soak_models.py")
    model_report = parse(artifact(inputs, report["model_report"]))
    plan = parse(read_regular(model_runner.PLAN))
    controls, _ = model_runner.registered_controls(plan)
    require(model_report["runner_sha256"] == digest(read_regular(ROOT / "scripts/ci/check_casper_soak_models.py")), "The verifier source differs.")
    require(model_report["plan_sha256"] == digest(read_regular(model_runner.PLAN)), "The verification plan differs.")
    require(model_report["status"] == "passed" and len(model_report["results"]) == len(controls), "The model search is incomplete.")
    for control, result in zip(controls, model_report["results"]):
        require(result["configuration"] == control["configuration"] and result["exit"] == control["expected_exit"], "The control identity or exit differs.")
        require(result["model_sha256"] == digest(read_regular(ROOT / plan["model"])), "The model source differs.")
        require(result["configuration_sha256"] == digest(read_regular(model_runner.PLAN.parent / control["configuration"])), "The control source differs.")
        log = artifact(inputs, report["model_logs"][control["configuration"]])
        require(digest(log) == result["log_sha256"] and model_runner.classify(result["exit"], log.decode(), control.get("property")) == "passed", "The model result is not an exact accepted verdict.")
    return "passed"


def publication(output, termination):
    manifest_data = read_regular(output / ".casper-manifest.json")
    manifest = parse(manifest_data)
    entries, _ = history(output)
    qualified, manifest_digest, _, _, inputs, approval = admission()
    require(qualified == manifest, "The publication manifest differs.")
    for entry in entries:
        _, _, request, module, _, _ = admission(entry["iteration"])
        directory = output / "casper-capture" / f'{entry["iteration"]:08d}'
        if not entry["capture_complete"]:
            require(entry["scenario_verdict"] != "passed", "An incomplete capture claims success.")
            continue
        captured_request = parse(read_regular(directory / "request.json"))
        expected_request = {**request, "manifest_digest": manifest_digest, "run_id": manifest["run_id"], "segment": entry["segment"], "iteration": entry["iteration"]}
        require(captured_request["request"] == expected_request and captured_request["generated"] == module.generate(parse(encoded(expected_request))), "The retained generation differs.")
        records = parse(read_regular(directory / "transport.json"))["observations"]
        observations, rejected, receipts, missing = correlate(manifest, manifest_digest, request, records, entry["segment"], entry["iteration"], directory)
        collected = module.collect(manifest, {"observations": observations, "rejected_sources": rejected})
        scenario = module.classify(request, collected["observations"], receipts)
        require(entry["product_failures"] == scenario["product_failures"] and entry["observations"] == observations and entry["fault_receipts"] == receipts, "The retained classification differs from raw evidence.")
        faults_missing = set(item["fault_id"] for item in request["fault_schedule"]) - set(item["payload"]["fault_id"] for item in receipts)
        if scenario["product_failures"]:
            verdict = "product_failure"
        elif missing or rejected or faults_missing or entry["exit_code"] != 0 or entry["termination"] != "completed":
            verdict = "incomplete"
        else:
            verdict = scenario["scenario_verdict"]
        require(entry["scenario_verdict"] == verdict, "A cached verdict differs from retained evidence.")
    harness = verification(manifest, inputs, approval)
    require(all(entry["manifest_digest"] == digest(manifest_data) for entry in entries), "Retained history identifies another manifest.")
    requested = manifest["runtime"]["iterations"]
    scenarios = set(manifest["required_scenarios"])
    completed = {entry["scenario_id"] for entry in entries if entry["scenario_verdict"] == "passed"}
    failures = [failure for entry in entries for failure in entry["product_failures"]]
    complete = len(entries) == requested and scenarios <= completed and all(entry["scenario_verdict"] == "passed" and entry["capture_complete"] for entry in entries)
    result = {"schema_version": 1, "manifest_digest": digest(manifest_data), "evidence_kind": manifest["evidence_kind"], "harness_verification": harness, "soak_verdict": "passed" if complete and harness == "passed" and termination == "completed" else "non_passing", "termination": termination, "requested_cases": requested, "completed_cases": len(entries), "scenario_results": entries, "product_failures": failures, "missing_scenarios": sorted(scenarios - completed), "coverage_complete": complete, "artifact_inventory": [ref for entry in entries for ref in entry["artifact_inventory"]]}
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=("admit", "history", "run", "finish", "publish"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--directory", type=Path)
    parser.add_argument("--segment", type=int, default=0)
    parser.add_argument("--iteration", type=int, default=0)
    parser.add_argument("--status", type=int, default=0)
    parser.add_argument("--failures", type=int, default=0)
    parser.add_argument("--termination", default="completed")
    args = parser.parse_args()
    try:
        if args.action == "admit":
            manifest, *_ = admission()
            print(encoded({"iterations": manifest["runtime"]["iterations"], "iterations_per_segment": manifest["runtime"]["iterations_per_segment"], "provider": manifest["provider"], "deadline": manifest["deadline"]["epoch_seconds"]}).decode(), end="")
        elif args.action == "history":
            check_history(args.output, args.iteration, args.failures)
        elif args.action == "run":
            return run(args.output, args.directory, args.segment, args.iteration)
        elif args.action == "finish":
            return finish(args.output, args.directory, args.segment, args.iteration, args.status, args.termination)
        else:
            print(encoded(publication(args.output, args.termination)).decode(), end="")
        return 0
    except Blocked as error:
        print(encoded({"scenario_verdict": "blocked", "soak_verdict": "non_passing", "reason": str(error), "launch_count": 0}).decode(), end="")
        return 3
    except (OSError, ValueError, KeyError, TypeError, RecursionError, subprocess.SubprocessError) as error:
        print(encoded({"scenario_verdict": "invalid_input", "soak_verdict": "non_passing", "reason": type(error).__name__}).decode(), end="")
        return 2


if __name__ == "__main__":
    sys.exit(main())
