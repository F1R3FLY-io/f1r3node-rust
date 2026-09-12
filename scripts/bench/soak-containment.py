#!/usr/bin/env python3
import json
import os
from pathlib import Path
import pwd
import re
import stat
import subprocess
import sys
import time
import uuid


NATIVE_GATE = r'''
import json
import os
from pathlib import Path
import re
import sys
import time

if os.geteuid() != 0:
    sys.exit("The native launch gate requires root.")
os.umask(0o077)
source, directory, uid, gid = sys.argv[1:]
control = Path(directory)
uid, gid = int(uid), int(gid)
if uid <= 0 or gid < 0:
    sys.exit("The native workload identity is invalid.")
invocation = os.environ.get("INVOCATION_ID", "")
if not re.fullmatch(r"[a-f0-9]{32}", invocation):
    sys.exit("The native gate has no service invocation identity.")
with (control / "environment").open(encoding="utf-8") as stream:
    environment = json.load(stream)
cgroups = Path("/proc/self/cgroup").read_text().splitlines()
if len(cgroups) != 1 or not cgroups[0].startswith("0::/system.slice/soak-native-"):
    sys.exit("The native gate has no supported run domain.")
ready = {"pid": os.getpid(), "invocation_id": invocation, "cgroup": cgroups[0][3:], "uid": uid, "gid": gid}
with (control / "gate-ready.tmp").open("x", encoding="utf-8") as stream:
    json.dump(ready, stream)
(control / "gate-ready.tmp").replace(control / "gate-ready.json")
deadline = time.monotonic() + 10
while not (control / "release.json").exists():
    if time.monotonic() >= deadline:
        sys.exit("The native launch gate received no verified release.")
    time.sleep(0.05)
with (control / "release.json").open(encoding="utf-8") as stream:
    release = json.load(stream)
if release != ready:
    sys.exit("The native launch release does not match this gate.")
os.setgroups([])
os.setgid(gid)
os.setuid(uid)
os.execve("/bin/bash", ["/bin/bash", str(Path(source) / "scripts/run-merge-recovery-soak.sh")], environment)
'''


def unit_properties(unit):
    names = ["MainPID", "InvocationID", "ControlGroup", "ActiveState", "SubState", "Result", "ExecMainStatus", "KillMode", "Restart", "User", "Description", "NoNewPrivileges", "CapabilityBoundingSet"]
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
    for ancestor in (*reversed(parent.parents), parent):
        metadata = ancestor.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
            raise ValueError("The control directory has an untrusted ancestor.")
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
    try:
        duration_seconds = int(duration)
    except ValueError as error:
        raise ValueError("The native diagnostic duration must be between 1 and 3600 seconds.") from error
    if not re.fullmatch(r"[1-9][0-9]{0,3}", duration) or duration_seconds > 3600:
        raise ValueError("The native diagnostic duration must be between 1 and 3600 seconds.")
    control.mkdir(mode=0o700)
    control.chmod(0o755)
    unit = "soak-native-" + uuid.uuid4().hex + ".service"
    description = "Soak native containment " + uuid.uuid4().hex
    properties = {
        "Description": description,
        "User": "0", "Group": "0",
        "KillMode": "control-group", "KillSignal": "SIGKILL", "SendSIGKILL": "yes",
        "TimeoutStopSec": "2", "RuntimeMaxSec": str(duration_seconds + 15), "Restart": "no",
        "MemoryMax": "256M", "TasksMax": "128", "CPUQuota": "100%",
        "NoNewPrivileges": "yes", "CapabilityBoundingSet": "CAP_SETUID CAP_SETGID", "AmbientCapabilities": "", "RestrictSUIDSGID": "yes",
        "ProtectControlGroups": "yes", "RestrictNamespaces": "yes",
        "PrivateNetwork": "yes", "ProtectSystem": "strict", "ReadWritePaths": str(base) + " " + str(control),
        "InaccessiblePaths": "-/run/docker.sock -/var/run/docker.sock -/run/containerd -/run/dbus/system_bus_socket -/run/systemd/private -/run/user",
    }
    command = ["/usr/bin/systemd-run", "--quiet", "--wait", "--pipe", "--service-type=exec", "--unit=" + unit, "--working-directory=" + str(source)]
    command.extend("--property=" + key + "=" + value for key, value in properties.items())
    environment_path = control / "environment"
    reserved = {"INVOCATION_ID", "SYSTEMD_EXEC_PID", "JOURNAL_STREAM", "NOTIFY_SOCKET", "LISTEN_PID", "LISTEN_FDS", "LISTEN_FDNAMES", "WATCHDOG_PID", "WATCHDOG_USEC"}
    descriptor = os.open(environment_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as output:
        environment = {key: value for key, value in os.environ.items() if key not in reserved and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key)}
        environment.update({"SOAK_CONTAINMENT": "required", "SOAK_RUN_DOMAIN_RECORD": str(control / "run-domain.json")})
        json.dump(environment, output)
    command.extend(["--", "/usr/bin/python3", "-I", "-c", NATIVE_GATE, str(source), str(control), str(uid), str(account.pw_gid)])
    record = {"backend": "systemd-native", "unit": unit, "uid": uid, "driver_pid": None, "termination": "unconfirmed"}
    (control / "request.json").write_text(json.dumps(record) + "\n")
    try:
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, env={"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LANG": "C.UTF-8"})
    except BaseException:
        environment_path.unlink(missing_ok=True)
        raise
    invocation = None
    cgroup = None
    deadline = time.monotonic() + duration_seconds + 25
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
                if observed.get("KillMode") != "control-group" or observed.get("Restart") != "no" or observed.get("User") != "0":
                    raise ValueError("The native service policy differs.")
                if observed.get("NoNewPrivileges") != "yes" or set(observed.get("CapabilityBoundingSet", "").split()) != {"cap_setgid", "cap_setuid"}:
                    raise ValueError("The native gate privilege policy differs.")
                try:
                    actual_group = Path(f"/proc/{pid}/cgroup").read_text().splitlines()
                    actual_uid = Path(f"/proc/{pid}").stat().st_uid
                except FileNotFoundError:
                    continue
                if actual_group != ["0::" + group] or actual_uid != 0:
                    raise ValueError("The trusted gate did not enter its native run domain.")
                try:
                    ready = json.loads((control / "gate-ready.json").read_text())
                except FileNotFoundError:
                    time.sleep(0.05)
                    continue
                grant = {"pid": pid, "invocation_id": current, "cgroup": group, "uid": uid, "gid": account.pw_gid}
                if ready != grant:
                    raise ValueError("The native gate identity differs from the manager observation.")
                invocation = current
                cgroup = Path("/sys/fs/cgroup") / group.lstrip("/")
                placement = control / "run-domain.json"
                with placement.open("x", encoding="utf-8") as output:
                    json.dump({"unit": unit, "cgroup": group, "uid": uid, "invocation_id": invocation}, output)
                placement.chmod(0o644)
                descriptor = os.open(control / "release.tmp", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
                with os.fdopen(descriptor, "w", encoding="utf-8") as output:
                    json.dump(grant, output)
                (control / "release.tmp").replace(control / "release.json")
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
    try:
        uid = int(sys.argv[4])
    except ValueError as error:
        raise ValueError("The native workload UID must be an integer.") from error
    return launch_native(sys.argv[2], sys.argv[3], uid)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"Native containment refused work: {error}", file=sys.stderr)
        sys.exit(2)
