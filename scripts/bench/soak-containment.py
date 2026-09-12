#!/usr/bin/env python3
import json
import os
from pathlib import Path
import pwd
import re
import subprocess
import sys
import time
import uuid


def unit_properties(unit):
    names = ["MainPID", "InvocationID", "ControlGroup", "ActiveState", "SubState", "Result", "ExecMainStatus", "KillMode", "Restart", "User", "Description"]
    result = subprocess.run(["/usr/bin/systemctl", "show", unit, "--property=" + ",".join(names)], check=True, capture_output=True, text=True, timeout=3)
    return dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)


def launch_native(source, control, uid):
    if os.geteuid() != 0 or not 0 < uid < 2**31:
        raise ValueError("Native containment requires a trusted root launcher and a non-root workload.")
    source = Path(source).resolve(strict=True)
    control = Path(control)
    parent = control.parent.resolve(strict=True)
    if not control.is_absolute() or parent != control.parent or not re.fullmatch(r"/[A-Za-z0-9_./-]+", str(control)):
        raise ValueError("The control directory must have a canonical absolute parent.")
    metadata = parent.stat()
    if metadata.st_uid != 0 or metadata.st_mode & 0o022:
        raise ValueError("The control parent must be root-owned and not writable by other users.")
    driver = source / "scripts/run-merge-recovery-soak.sh"
    if not driver.is_file():
        raise ValueError("The source directory has no soak driver.")
    account = pwd.getpwuid(uid)
    base = source.parent
    if base.stat().st_uid != 0 or base.stat().st_mode & 0o022:
        raise ValueError("The native run directory must be root-owned and not writable by other users.")
    if not re.fullmatch(r"/[A-Za-z0-9_./-]+", str(source)) or base == Path("/"):
        raise ValueError("The native source path is unsupported.")
    for key in ("SOAK_OUTPUT_DIR", "SYSTEM_INTEGRATION_DIR", "SOAK_TMP_ROOT", "SOAK_RUNNER_ROOT"):
        path = Path(os.environ[key])
        if not path.is_absolute() or path.resolve() != path or not path.is_relative_to(base) or path.is_relative_to(source):
            raise ValueError("Native writable paths must stay in the private run directory, outside the source.")
    if not Path("/sys/fs/cgroup/cgroup.controllers").is_file():
        raise ValueError("Native containment requires cgroup v2.")
    duration = os.environ["SOAK_DURATION_SECONDS"]
    if not re.fullmatch(r"[1-9][0-9]{0,3}", duration) or int(duration) > 3600:
        raise ValueError("The native diagnostic duration must be between 1 and 3600 seconds.")
    control.mkdir(mode=0o700)
    unit = "soak-native-" + uuid.uuid4().hex + ".service"
    description = "Soak native containment " + uuid.uuid4().hex
    properties = {
        "Description": description,
        "User": str(uid), "Group": str(account.pw_gid),
        "KillMode": "control-group", "KillSignal": "SIGKILL", "SendSIGKILL": "yes",
        "TimeoutStopSec": "2", "RuntimeMaxSec": str(int(duration) + 15), "Restart": "no",
        "MemoryMax": "256M", "TasksMax": "128", "CPUQuota": "100%",
        "NoNewPrivileges": "yes", "CapabilityBoundingSet": "", "RestrictSUIDSGID": "yes",
        "ProtectControlGroups": "yes", "RestrictNamespaces": "yes",
        "PrivateNetwork": "yes", "ProtectSystem": "strict", "ReadWritePaths": str(base),
        "InaccessiblePaths": "-/run/docker.sock -/var/run/docker.sock -/run/containerd -/run/dbus/system_bus_socket -/run/systemd/private -/run/user",
    }
    command = ["/usr/bin/systemd-run", "--quiet", "--wait", "--pipe", "--service-type=exec", "--unit=" + unit, "--working-directory=" + str(source)]
    command.extend("--property=" + key + "=" + value for key, value in properties.items())
    environment_path = control / "environment"
    reserved = {"INVOCATION_ID", "SYSTEMD_EXEC_PID", "JOURNAL_STREAM", "NOTIFY_SOCKET", "LISTEN_PID", "LISTEN_FDS", "LISTEN_FDNAMES", "WATCHDOG_PID", "WATCHDOG_USEC"}
    descriptor = os.open(environment_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as output:
        for key, value in sorted(os.environ.items()):
            if key in reserved or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key):
                continue
            value = value.replace("\\", "\\\\").replace('"', '\\"').replace("$", "\\$").replace("`", "\\`")
            output.write(f'{key}="{value}"\n')
    command.extend(["--property=EnvironmentFile=" + str(environment_path), "--", "/bin/bash", str(driver)])
    record = {"backend": "systemd-native", "unit": unit, "uid": uid, "driver_pid": None, "termination": "unconfirmed"}
    (control / "request.json").write_text(json.dumps(record) + "\n")
    try:
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL)
    except BaseException:
        environment_path.unlink(missing_ok=True)
        raise
    invocation = None
    cgroup = None
    deadline = time.monotonic() + int(duration) + 25
    try:
        while process.poll() is None:
            if time.monotonic() >= deadline:
                raise TimeoutError("The native launch client exceeded its observation budget.")
            observed = unit_properties(unit)
            if observed.get("Description") != description:
                time.sleep(0.05)
                continue
            current = observed.get("InvocationID", "")
            if invocation is not None and current != invocation:
                raise ValueError("The service invocation changed. Termination is unconfirmed.")
            pid = int(observed.get("MainPID", "0"))
            if pid > 0 and invocation is None and observed.get("ActiveState") == "active":
                group = observed.get("ControlGroup", "")
                if not re.fullmatch(r"[a-f0-9]{32}", current) or group != "/system.slice/" + unit:
                    raise ValueError("The native service identity is invalid.")
                if observed.get("KillMode") != "control-group" or observed.get("Restart") != "no" or observed.get("User") != str(uid):
                    raise ValueError("The native service policy differs.")
                try:
                    actual_group = Path(f"/proc/{pid}/cgroup").read_text().splitlines()
                    actual_uid = Path(f"/proc/{pid}").stat().st_uid
                except FileNotFoundError:
                    continue
                if actual_group != ["0::" + group] or actual_uid != uid:
                    raise ValueError("The driver did not enter its native run domain.")
                invocation = current
                cgroup = Path("/sys/fs/cgroup") / group.lstrip("/")
                record.update({"invocation_id": invocation, "cgroup": group, "driver_pid": pid})
                (control / "launcher.json").write_text(json.dumps(record) + "\n")
                environment_path.unlink(missing_ok=True)
            time.sleep(0.05)
        result = process.wait()
        observed = unit_properties(unit)
        if invocation is None or cgroup is None or observed.get("InvocationID") != invocation:
            raise ValueError("The native service completion identity is unconfirmed.")
        if observed.get("ActiveState") not in ("inactive", "failed"):
            raise ValueError("The native service did not stop.")
        try:
            events = dict(line.split() for line in (cgroup / "cgroup.events").read_text().splitlines())
            if events.get("populated") != "0":
                raise ValueError("The native run domain still contains processes.")
        except FileNotFoundError:
            pass
        record.update({"termination": "confirmed-native", "service": observed, "client_exit": result})
        (control / "result.json").write_text(json.dumps(record) + "\n")
        return result if result >= 0 else 128 - result
    finally:
        environment_path.unlink(missing_ok=True)
        if process.poll() is None:
            observed = unit_properties(unit)
            if invocation is not None and observed.get("InvocationID") == invocation and observed.get("Description") == description:
                subprocess.run(["/usr/bin/systemctl", "stop", unit], check=True, timeout=5)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def main():
    if len(sys.argv) != 5 or sys.argv[1] != "native":
        raise ValueError("Usage: run-soak-contained.sh native SOURCE NEW_CONTROL_DIRECTORY UID")
    return launch_native(sys.argv[2], sys.argv[3], int(sys.argv[4]))


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"Native containment refused work: {error}", file=sys.stderr)
        sys.exit(2)
