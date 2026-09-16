#!/usr/bin/env python3
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import subprocess
import sys
import time
import uuid


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def wait_for(predicate, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(0.05)
    return False


def main():
    require(os.geteuid() == 0, "The fixture requires the disposable runner's root observer.")
    try:
        metadata = json.loads(Path("/opt/d2-provisioning.json").read_text())
    except (OSError, ValueError) as error:
        raise RuntimeError("The runner provisioning metadata is unavailable or invalid.") from error
    session = os.environ.get("SOAK_DIAGNOSTIC_SESSION", "")
    require(re.fullmatch(r"ci-eph-f1r3node-rust-amd64-d2-[0-9]{8}-[0-9]{6}-[a-f0-9]{6}", session), "The diagnostic session is invalid.")
    registration = metadata["github_registration"]
    require(metadata["runner_name"] == session and isinstance(registration, bool) and not registration, "The runner identity does not match.")
    subprocess.run(["/usr/bin/systemctl", "is-active", "--quiet", "d2-expire.timer"], check=True, timeout=3)
    require(len(sys.argv) == 3, "Usage: test-soak-native-control-path.py SOURCE NEW_CASE_DIRECTORY")
    source = Path(sys.argv[1]).resolve(strict=True)
    case = Path(sys.argv[2])
    require(case.is_absolute() and case.parent == Path("/var/tmp") and re.fullmatch(r"d2-native-control-[a-f0-9]{32}", case.name), "The case directory is invalid.")
    case.mkdir(mode=0o755)
    uid = 65534
    evidence = case / "evidence"
    for name in ("evidence", "harness", "tmp", "runner"):
        path = case / name
        path.mkdir(mode=0o755)
        os.chown(path, uid, uid)
    (case / "bin").mkdir()
    for name in (
        "scripts/run-merge-recovery-soak.sh", "scripts/bench/run-soak-contained.sh",
        "scripts/bench/soak-containment.py", "scripts/bench/write-soak-summary.sh",
        "scripts/bench/collect-soak-metrics.sh", "scripts/bench/soak-metrics.json",
    ):
        target = case / "repo" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / name, target)
        target.chmod(0o755 if target.suffix == ".sh" else 0o644)
    for name in ("poetry", "docker", "oci"):
        path = case / "bin" / name
        path.write_text("#!/bin/bash\nexit 0\n")
        path.chmod(0o755)
    hook = case / "workload-init.sh"
    hook.write_text('unset BASH_ENV\nprintf "%s\\n" "$EUID" >"$SOAK_FIXTURE_START"\n/bin/cat /proc/self/cgroup >"$SOAK_FIXTURE_CGROUP"\n')
    hook.chmod(0o644)
    control_root = Path("/run") / ("soak-native-control-" + uuid.uuid4().hex)
    control_root.mkdir(mode=0o755)
    ancestor = control_root / "untrusted"
    ancestor.mkdir(mode=0o755)
    parent = ancestor / "root-parent"
    parent.mkdir(mode=0o755)
    os.chown(ancestor, uid, uid)
    control = parent / "control"
    require(parent.stat().st_uid == 0 and not parent.stat().st_mode & 0o022, "The immediate control parent is not trusted.")
    require(ancestor.stat().st_uid == uid, "The untrusted ancestor has the wrong owner.")
    writer = case / "writer.sh"
    writer.write_text(f'#!/bin/bash\nset -euo pipefail\ntrap "" TERM\nfor n in $(seq 1 600); do printf "%s\\n" "$n" >>{evidence}/unrelated.writes; sleep 0.1; done\n')
    unrelated = subprocess.Popen(["/usr/bin/setpriv", "--reuid", str(uid), "--regid", str(uid), "--clear-groups", "--", "/usr/bin/setsid", "/bin/bash", str(writer)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    descriptor = os.pidfd_open(unrelated.pid)
    poller = select.poll()
    poller.register(descriptor, select.POLLIN)
    environment = {
        "PATH": f"{case}/bin:/usr/local/bin:/usr/bin:/bin", "SYSTEM_INTEGRATION_DIR": str(case / "harness"),
        "SOAK_OUTPUT_DIR": str(evidence / "output"), "SOAK_TMP_ROOT": str(case / "tmp"),
        "SOAK_RUNNER_ROOT": str(case / "runner"), "SOAK_DURATION_SECONDS": "3",
        "SOAK_RUN_BENCHMARKS": "false", "SOAK_RSS_CEILING_MB": "0", "SOAK_HOST_FREE_FLOOR_MB": "0",
        "SOAK_DISK_FREE_FLOOR_MB": "0", "SOAK_DISK_STOP_SECONDS": "2", "SOAK_GUARDIAN_POLL_SECONDS": "0.1",
        "BASH_ENV": str(hook), "SOAK_FIXTURE_START": str(evidence / "workload-start.txt"),
        "SOAK_FIXTURE_CGROUP": str(evidence / "workload-cgroup.txt"),
    }
    process = None
    verdict = None
    try:
        require(wait_for(lambda: (evidence / "unrelated.writes").exists(), 3), "The unrelated writer did not start.")
        with (evidence / "launcher.log").open("wb") as stream:
            process = subprocess.Popen(["/usr/bin/python3", "-I", str(case / "repo/scripts/bench/soak-containment.py"), "native", str(case / "repo"), str(control), str(uid)], env=environment, stdin=subprocess.DEVNULL, stdout=stream, stderr=subprocess.STDOUT)
        status = process.wait(timeout=20)
        started = evidence / "workload-start.txt"
        workload_started = started.exists()
        if workload_started:
            require(started.read_text() == str(uid) + "\n" and started.stat().st_uid == uid, "The workload hook did not execute with the required non-root identity.")
            require((control / "launcher.json").is_file(), "The native launch observation is missing.")
        else:
            require(status == 2, "The invalid control path did not return its refusal status.")
            require("The control directory has an untrusted ancestor." in (evidence / "launcher.log").read_text(), "The launcher refused for a different reason.")
            require(not control.exists(), "The refused control path was modified.")
        before = (evidence / "unrelated.writes").stat().st_size
        time.sleep(0.5)
        after = (evidence / "unrelated.writes").stat().st_size
        observation = {
            "launcher_exit": status, "workload_started": workload_started,
            "workload_uid": uid if workload_started else None,
            "control_parent_uid": parent.stat().st_uid, "control_ancestor_uid": ancestor.stat().st_uid,
            "unrelated_dead": bool(poller.poll(0)), "unrelated_before_bytes": before,
            "unrelated_after_bytes": after, "cleanup_started": False,
        }
        (evidence / "observation.json").write_text(json.dumps(observation, indent=2) + "\n")
        require(not observation["unrelated_dead"] and after > before, "The unrelated writer stopped or stalled.")
        verdict = 1 if workload_started else 0
        (evidence / "verdict.txt").write_text(str(verdict) + "\n")
    finally:
        request = control / "request.json"
        if request.exists():
            require(control.stat().st_uid == 0 and request.stat().st_uid == 0, "The launch request is not trusted.")
            record = json.loads(request.read_text())
            unit = record["unit"]
            require(re.fullmatch(r"soak-native-[a-f0-9]{32}\.service", unit), "The cleanup unit is invalid.")
            subprocess.run(["/usr/bin/systemctl", "stop", unit], check=True, timeout=8)
            state = subprocess.run(["/usr/bin/systemctl", "show", unit, "--property=ActiveState,SubState,MainPID,InvocationID,ControlGroup,User,KillMode,Restart"], check=True, capture_output=True, text=True, timeout=3)
            (evidence / "service-after-cleanup.txt").write_text(state.stdout)
            require("MainPID=0\n" in state.stdout, "The native service did not stop during fixture cleanup.")
            shutil.copytree(control, evidence / "control", ignore=shutil.ignore_patterns("environment"))
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=3)
        if not poller.poll(0):
            signal.pidfd_send_signal(descriptor, signal.SIGKILL)
        unrelated.wait(timeout=3)
        os.close(descriptor)
    if verdict:
        print("FAIL: The native launcher executed workload code below an untrusted control ancestor.", file=sys.stderr)
    else:
        print("PASS: An untrusted control ancestor prevents workload execution and preserves the unrelated writer.")
    print("The workload initialization hook and external workload commands were substitutes. Fixture cleanup started after the recorded verdict.")
    return verdict


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(f"SETUP ERROR: {type(error).__name__}: {error}", file=sys.stderr)
        sys.exit(2)
