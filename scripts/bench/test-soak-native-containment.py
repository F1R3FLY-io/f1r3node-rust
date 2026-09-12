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


class BehavioralFailure(Exception):
    pass


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


def dead(descriptor):
    poller = select.poll()
    poller.register(descriptor, select.POLLIN)
    return bool(poller.poll(0))


def process_fields(pid):
    return Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()


def main():
    require(os.geteuid() == 0, "The fixture requires the disposable runner's root observer.")
    metadata = json.loads(Path("/opt/d2-provisioning.json").read_text())
    session = os.environ.get("SOAK_DIAGNOSTIC_SESSION", "")
    require(re.fullmatch(r"ci-eph-f1r3node-rust-amd64-d2-[0-9]{8}-[0-9]{6}-[a-f0-9]{6}", session), "The diagnostic session is invalid.")
    require(metadata["runner_name"] == session and metadata["github_registration"] is False, "The runner identity does not match.")
    subprocess.run(["systemctl", "is-active", "--quiet", "d2-expire.timer"], check=True)
    require(len(sys.argv) == 3, "Usage: test-soak-native-containment.py SOURCE NEW_CASE_DIRECTORY")
    source = Path(sys.argv[1]).resolve(strict=True)
    case = Path(sys.argv[2])
    require(case.is_absolute() and case.parent == Path("/var/tmp") and re.fullmatch(r"d2-native-[a-f0-9]{32}", case.name), "The case directory is invalid.")
    case.mkdir(mode=0o755)
    uid = 65534
    evidence = case / "evidence"
    for name in ("evidence", "harness", "tmp", "runner"):
        path = case / name
        path.mkdir(mode=0o755)
        os.chown(path, uid, uid)
    (case / "bin").mkdir()
    files = [
        "scripts/run-merge-recovery-soak.sh", "scripts/bench/run-soak-contained.sh",
        "scripts/bench/soak-containment.py", "scripts/bench/write-soak-summary.sh",
        "scripts/bench/collect-soak-metrics.sh", "scripts/bench/soak-metrics.json",
    ]
    for name in files:
        target = case / "repo" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / name, target)
        target.chmod(0o755 if target.suffix == ".sh" else 0o644)
    writer = case / "writer.sh"
    writer.write_text('#!/bin/bash\nset -euo pipefail\ntrap "" TERM\nprintf "%s\\n" "$$" >"$1.pid"\nfor n in $(seq 1 600); do printf "%s\\n" "$n" >>"$1.writes"; sleep 0.1; done\n')
    poetry = case / "bin/poetry"
    poetry.write_text(f'#!/bin/bash\nset -euo pipefail\ntrap "exit 143" TERM INT\nprintf "Iteration admitted.\\n" >>{evidence}/admissions.txt\nsetsid bash {writer} {evidence}/owned >/dev/null 2>&1 &\nwhile :; do sleep 0.1; done\n')
    poetry.chmod(0o755)
    for name in ("docker", "oci"):
        path = case / "bin" / name
        path.write_text("#!/bin/bash\nexit 0\n")
        path.chmod(0o755)
    unrelated_environment = os.environ.copy()
    unrelated_environment.pop("SOAK_PROCESS_OWNER", None)
    subprocess.Popen(["setpriv", "--reuid", str(uid), "--regid", str(uid), "--clear-groups", "--", "setsid", "bash", str(writer), str(evidence / "unrelated")], env=unrelated_environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    environment = os.environ.copy()
    environment.update({
        "PATH": f"{case}/bin:/usr/local/bin:/usr/bin:/bin", "SYSTEM_INTEGRATION_DIR": str(case / "harness"),
        "SOAK_OUTPUT_DIR": str(evidence / "output"), "SOAK_TMP_ROOT": str(case / "tmp"),
        "SOAK_RUNNER_ROOT": str(case / "runner"), "SOAK_DURATION_SECONDS": "120",
        "SOAK_RUN_BENCHMARKS": "false", "SOAK_RSS_CEILING_MB": "0", "SOAK_HOST_FREE_FLOOR_MB": "0",
        "SOAK_DISK_FREE_FLOOR_MB": "0", "SOAK_DISK_STOP_SECONDS": "2", "SOAK_GUARDIAN_POLL_SECONDS": "0.1",
    })
    launcher = case / "repo/scripts/bench/run-soak-contained.sh"
    controls = []

    def start(name):
        control = Path("/run") / ("soak-native-test-" + uuid.uuid4().hex)
        controls.append(control)
        stream = (evidence / f"{name}.log").open("wb")
        process = subprocess.Popen(["bash", str(launcher), "native", str(case / "repo"), str(control), str(uid)], env=environment, stdout=stream, stderr=subprocess.STDOUT)
        stream.close()
        return process, control

    process, control = start("driver")
    require(wait_for(lambda: (evidence / "owned.pid").exists() and (control / "launcher.json").exists(), 10), "The fixture did not reach active work.")
    require((evidence / "admissions.txt").read_text().splitlines() == ["Iteration admitted."], "The fixture did not admit exactly one iteration.")
    driver = json.loads((control / "launcher.json").read_text())["driver_pid"]
    ready = list((evidence / "output").glob(".crash-monitor.*/ready"))
    require(len(ready) == 1, "The crash monitor is unavailable.")
    monitor = int(ready[0].read_text())
    driver_descriptor = os.pidfd_open(driver)
    monitor_descriptor = os.pidfd_open(monitor)
    for pid in (driver, monitor):
        require(Path(f"/proc/{pid}").stat().st_uid == uid and os.getsid(pid) == pid, "A controller identity differs.")
    require(Path(f"/proc/{driver}/cmdline").read_bytes().split(b"\0") == [b"/bin/bash", str(case / "repo/scripts/run-merge-recovery-soak.sh").encode(), b""], "The driver command differs.")
    require(Path(f"/proc/{monitor}/cmdline").read_bytes().split(b"\0") == [b"python3", b"-", str(driver).encode(), str(ready[0].parent).encode(), str(evidence / "output").encode(), b""], "The monitor command differs.")
    require(int(process_fields(monitor)[1]) == driver, "The monitor parent differs.")
    roles = {}
    for role in ("owned", "unrelated"):
        pid = int((evidence / f"{role}.pid").read_text())
        descriptor = os.pidfd_open(pid)
        require(Path(f"/proc/{pid}").stat().st_uid == uid and not dead(descriptor), "A writer identity differs.")
        roles[role] = descriptor
        owner = [item for item in Path(f"/proc/{pid}/environ").read_bytes().split(b"\0") if item.startswith(b"SOAK_PROCESS_OWNER=")]
        require((role == "owned" and len(owner) == 1) or (role == "unrelated" and not owner), "The writer ownership marker differs.")
        (evidence / f"{role}-cgroup.txt").write_bytes(Path(f"/proc/{pid}/cgroup").read_bytes())
    (evidence / "driver-cgroup.txt").write_bytes(Path(f"/proc/{driver}/cgroup").read_bytes())
    shutil.copyfile(evidence / "output/.soak-state", evidence / "state-before.txt")
    started = time.monotonic()
    for pid, descriptor in ((driver, driver_descriptor), (monitor, monitor_descriptor)):
        signal.pidfd_send_signal(descriptor, signal.SIGSTOP)
        require(wait_for(lambda: process_fields(pid)[0] == "T", 1), "A controller did not suspend.")
    for descriptor in (driver_descriptor, monitor_descriptor):
        signal.pidfd_send_signal(descriptor, signal.SIGKILL)
    require(wait_for(lambda: dead(driver_descriptor) and dead(monitor_descriptor), 2), "Controller death was not confirmed.")
    (evidence / "fault.txt").write_text("The fixture suspended both controllers and confirmed both deaths through their process descriptors.\n")
    wait_for(lambda: dead(roles["owned"]), 12)
    before = {role: (evidence / f"{role}.writes").read_bytes() for role in roles}
    time.sleep(0.5)
    after = {role: (evidence / f"{role}.writes").read_bytes() for role in roles}
    observed = {"elapsed_seconds": time.monotonic() - started, "driver_dead": dead(driver_descriptor), "monitor_dead": dead(monitor_descriptor)}
    for role in roles:
        observed[role] = {"dead": dead(roles[role]), "before_bytes": len(before[role]), "after_bytes": len(after[role])}
    (evidence / "observation.json").write_text(json.dumps(observed, indent=2) + "\n")
    if dead(roles["unrelated"]) or before["unrelated"] == after["unrelated"]:
        raise BehavioralFailure("Controller loss stopped or stalled the unrelated writer.")
    if not dead(roles["owned"]) or before["owned"] != after["owned"]:
        raise BehavioralFailure("Both controllers died but the owned writer continued.")
    require(process.wait(timeout=10) != 0, "The killed driver returned success.")
    for name in ("restart-1", "restart-2"):
        restart, _ = start(name)
        require(restart.wait(timeout=10) != 0, "A restart returned success.")
        summary = json.loads((evidence / "output/summary.json").read_text())
        counts = [summary[key] for key in ("iterations", "failures", "bench_segments", "bench_failures")]
        shutil.copyfile(evidence / "output/summary.json", evidence / f"{name}-summary.json")
        if counts != [1, 1, 0, 0] or len((evidence / "admissions.txt").read_text().splitlines()) != 1:
            raise BehavioralFailure("A restart lost the failure or admitted new work.")
    for index, directory in enumerate(controls):
        shutil.copytree(directory, evidence / f"control-{index}")
    print("PASS: Both controllers died, native containment stopped the owned writer, and two refused restarts retained one failure.")
    print("Docker commands were substitutes. This case does not verify Docker containment or durable publication.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BehavioralFailure as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
    except Exception as error:
        print(f"SETUP ERROR: {type(error).__name__}: {error}", file=sys.stderr)
        sys.exit(2)
