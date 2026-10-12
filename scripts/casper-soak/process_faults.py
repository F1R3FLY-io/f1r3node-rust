import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import time


MAX_BYTES = 1_048_576


def clock_id():
    return "linux-monotonic:" + _read("/proc/sys/kernel/random/boot_id").decode().strip()


def _require(condition, message):
    if not condition:
        raise ValueError(message)


def _read(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as source:
        _require(stat.S_ISREG(os.fstat(source.fileno()).st_mode), "The artifact is not a regular file.")
        data = source.read(MAX_BYTES + 1)
    _require(len(data) <= MAX_BYTES, "The artifact exceeds its size bound.")
    return data


def _parse(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            _require(key not in result, "The JSON key repeats.")
            result[key] = value
        return result
    def invalid_constant(value):
        raise ValueError("The JSON number is not finite.")
    return json.loads(data, object_pairs_hook=unique, parse_constant=invalid_constant)


def _capture(path, expected):
    path = Path(path)
    _require(not path.is_symlink(), "The capture directory is a symbolic link.")
    raw = _read(path / "report.json")
    _require(hashlib.sha256(raw).hexdigest() == expected, "The capture report digest differs.")
    report = _parse(raw)
    _require(report["status"] == "captured", "The observer capture failed.")
    artifacts = {}
    artifact_bytes = {}
    for ref in report["artifacts"]:
        name = ref["path"]
        _require(name in {"binding.json", "authority.json", "hello.json", "request.json", "response.json", "mapping.json"},
                 "The capture artifact name is unsupported.")
        _require(name not in artifacts, "The capture artifact repeats.")
        data = _read(path / name)
        _require(len(data) == ref["bytes"] and hashlib.sha256(data).hexdigest() == ref["sha256"],
                 "The capture artifact identity differs.")
        artifacts[name] = _parse(data)
        artifact_bytes[name] = data
    _require({"binding.json", "authority.json", "hello.json", "request.json", "response.json"} <= artifacts.keys(),
             "The capture artifact inventory is incomplete.")
    binding = artifacts["binding.json"]
    hello = artifacts["hello.json"]
    request = artifacts["request.json"]
    response = artifacts["response.json"]
    _require(response["kind"] == "authority_snapshot" and response["live_profile_qualified"] is False,
             "The response scope differs.")
    _require(response["request_sha256"] == hashlib.sha256(artifact_bytes["request.json"]).hexdigest()
             and response["request_id"] == request["request_id"] == binding["request_id"],
             "The response request identity differs.")
    _require(request["authority"] == artifacts["authority.json"] == response["result"]["value"]["request"]
             and request["challenge"] == hello["challenge"], "The authority inputs or challenge differ.")
    _require(response["approved_request_sha256"] == request["approved_request_sha256"]
             == binding["approved_request_sha256"] == hello["approved_request_sha256"],
             "The approved request digest differs.")
    _require(response["clock"] == hello["clock"] == "observer-monotonic"
             and response["sequence"] > hello["sequence"]
             and response["monotonic_ns"] >= hello["monotonic_ns"], "The observer ordering differs.")
    identity = response["identity"]
    _require(identity == hello["identity"] and identity["incarnation"] == request["incarnation"],
             "The observer identity changed within the capture.")
    for captured, pinned in [("pid", "node_pid"), ("process_start_ticks", "process_start_ticks"),
                             ("declared_source_revision", "source_revision"),
                             ("executable_sha256", "executable_sha256"),
                             ("configuration_sha256", "configuration_sha256")]:
        _require(identity[captured] == binding[pinned], "The capture binding differs.")
    _require(response["result"]["availability"] == "available", "The authority endpoint is unavailable.")
    return identity


def _process(pid):
    data = _read(f"/proc/{pid}/stat").decode()
    suffix = data.rsplit(")", 1)[1].split()
    return {"pid": pid, "start_ticks": int(suffix[19]), "state": suffix[0]}


def _write(path, value):
    data = (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()
    with open(path, "xb") as target:
        target.write(data)
        target.flush()
        os.fsync(target.fileno())
    return {"path": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def apply_owned_fault(node, request, before_capture, after_capture, output):
    _require(os.path.isdir("/proc/self"), "Process receipts require Linux.")
    _require(request["action"] in {"pause", "restart"}, "The process action is unsupported.")
    for field in ("fault_id", "node_id", "incarnation", "trigger_event", "clock_id"):
        _require(isinstance(request[field], str) and 0 < len(request[field]) <= 256,
                 "The fault identity is invalid.")
    deadline = request["deadline_monotonic_ns"]
    _require(request["clock_id"] == clock_id(), "The fault clock differs from this host.")
    _require(type(deadline) is int and time.monotonic_ns() < deadline <= time.monotonic_ns() + 60_000_000_000,
             "The process deadline is invalid.")
    _require(node.name == request["node_id"], "The provider node identity differs.")
    handle = node._handle
    old = getattr(handle, "_proc", None)
    _require(isinstance(old, subprocess.Popen), "The provider does not own a child process.")
    identity = _capture(*before_capture)
    _require(identity["incarnation"] == request["incarnation"] and identity["pid"] == old.pid,
             "The predecessor identity differs.")
    before = _process(old.pid)
    _require(before["start_ticks"] == identity["process_start_ticks"] and before["state"] not in {"Z", "X", "T", "t"}
             and old.poll() is None, "The predecessor is not the captured running process.")
    output = Path(output)
    output.mkdir(mode=0o700)
    artifacts = []
    artifacts.append(_write(output / "request.json", request))
    artifacts.append(_write(output / "before.json", {"process": before, "identity": identity,
        "capture_path": str(before_capture[0]), "capture_report_sha256": before_capture[1]}))
    receipt = {"fault_id": request["fault_id"], "node_id": request["node_id"],
        "action": request["action"], "incarnation": request["incarnation"],
        "trigger_event": request["trigger_event"], "clock_id": request["clock_id"],
        "status": "unknown", "ready_scope": "authority_endpoint_available", "prior_exit": False, "ready": False}
    pause_attempted = False
    try:
        _require(time.monotonic_ns() <= deadline, "The fault deadline expired before application.")
        if request["action"] == "pause":
            pause_attempted = True
            node.pause()
            while time.monotonic_ns() <= deadline:
                state = _process(old.pid)
                _require(state["start_ticks"] == before["start_ticks"] and old.poll() is None,
                         "The paused process was replaced or exited.")
                if state["state"] in {"T", "t"}:
                    receipt["observed_state"] = "stopped"
                    receipt["process"] = state
                    break
                time.sleep(0.01)
            _require(receipt.get("observed_state") == "stopped", "The stopped state was not observed.")
        else:
            node.restart()
            _require(time.monotonic_ns() <= deadline, "The restart exceeded the fault deadline.")
            code = old.poll()
            _require(code is not None, "The predecessor exit was not observed.")
            receipt["prior_exit"] = True
            receipt["prior_exit_code"] = code
            new = getattr(handle, "_proc", None)
            _require(isinstance(new, subprocess.Popen) and new is not old and new.poll() is None,
                     "The replacement child is unavailable.")
            after_path, after_digest = after_capture(deadline)
            next_identity = _capture(after_path, after_digest)
            state = _process(new.pid)
            artifacts.append(_write(output / "after.json", {"identity": next_identity, "process": state,
                "capture_path": str(after_path), "capture_report_sha256": after_digest}))
            _require(next_identity["pid"] == new.pid and next_identity["process_start_ticks"] == state["start_ticks"]
                     and next_identity["incarnation"] != identity["incarnation"], "The replacement identity differs.")
            for field in ("declared_source_revision", "executable_sha256", "configuration_sha256"):
                _require(next_identity[field] == identity[field], "The replacement candidate identity differs.")
            _require(new.poll() is None and state["state"] not in {"Z", "X", "T", "t"},
                     "The replacement process is not running.")
            receipt.update(ready=True, new_incarnation=next_identity["incarnation"],
                           process=state, capture_report_sha256=after_digest)
        receipt["monotonic_ns"] = time.monotonic_ns()
        _require(receipt["monotonic_ns"] <= deadline, "The fault receipt is late.")
        receipt["status"] = "applied"
    except Exception as error:
        receipt["error"] = str(error)
        receipt["monotonic_ns"] = time.monotonic_ns()
    finally:
        if pause_attempted:
            try:
                _require(handle._proc is old, "The pause provider replaced its child.")
                node.unpause()
                resume_deadline = time.monotonic() + 2
                while time.monotonic() < resume_deadline:
                    state = _process(old.pid)
                    _require(state["start_ticks"] == before["start_ticks"] and old.poll() is None,
                             "The resumed process was replaced or exited.")
                    if state["state"] not in {"T", "t"}:
                        receipt["cleanup"] = {"status": "resumed", "process": state}
                        break
                    time.sleep(0.01)
                _require("cleanup" in receipt, "The resumed state was not observed.")
            except Exception as error:
                receipt["cleanup"] = {"status": "failed", "error": str(error)}
    artifacts.append(_write(output / "receipt.json", receipt))
    report = {"schema_version": 1, "status": "recorded", "receipt": receipt,
        "qualification": "pending", "profile_verdict": "blocked", "soak_verdict": "non_passing",
        "provider_scope": "owned_subprocess", "artifacts": artifacts,
        "recorder_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "limitations": ["Provider calls require separate timing qualification.",
                        "The observer capture requires source-bound acceptance.",
                        "Pause receipts describe the stopped observation before cleanup."]}
    _write(output / "report.json", report)
    return report
