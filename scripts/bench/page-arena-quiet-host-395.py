import argparse
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRATCH = ROOT / "target" / "scratch"
SURVEY_SECONDS = 30
MIN_IDLE_PERCENT = 90.0


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def cpus_from_list(value):
    result = set()
    for item in value.split(","):
        endpoints = item.split("-", 1)
        if len(endpoints) == 1:
            result.add(int(endpoints[0]))
        else:
            result.update(range(int(endpoints[0]), int(endpoints[1]) + 1))
    return result


def average_idle(output):
    result = {}
    for line in output.splitlines():
        match = re.fullmatch(r"Average:\s+(\d+)\s+.+\s+(\d+(?:\.\d+)?)", line)
        if match:
            result[int(match.group(1))] = float(match.group(2))
    return result


def swap_used_kib():
    values = {}
    for line in Path("/proc/meminfo").read_text().splitlines():
        key, _, value = line.partition(":")
        if key in {"SwapTotal", "SwapFree"}:
            values[key] = int(value.split()[0])
    return values["SwapTotal"] - values["SwapFree"]


def choose_core(idles, allowed):
    groups = set()
    for cpu in allowed:
        siblings = cpus_from_list(
            Path(f"/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list")
            .read_text()
            .strip()
        )
        if siblings <= allowed and siblings <= idles.keys():
            groups.add(tuple(sorted(siblings)))
    if not groups:
        raise ValueError("no complete online sibling group in allowed affinity")
    chosen = min(
        groups,
        key=lambda group: (max(100 - idles[cpu] for cpu in group), group[0]),
    )
    return {
        "pinned_cpu": chosen[0],
        "siblings": list(chosen),
        "survey_idle_percent": {str(cpu): idles[cpu] for cpu in chosen},
        "all_siblings_at_least_90_percent_idle": all(
            idles[cpu] >= MIN_IDLE_PERCENT for cpu in chosen
        ),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    if not out.is_relative_to(SCRATCH.resolve()) or out.exists():
        raise ValueError("output must be a fresh path below target/scratch")
    out.mkdir(parents=True)
    env = os.environ.copy()
    env.update(
        LC_ALL="C",
        DISPLAY="",
        WAYLAND_DISPLAY="",
        JAVA_TOOL_OPTIONS="-Djava.awt.headless=true",
    )
    started = datetime.datetime.now(datetime.timezone.utc)
    before_swap = swap_used_kib()
    before_load = os.getloadavg()
    command = ["mpstat", "-P", "ALL", "1", str(SURVEY_SECONDS)]
    try:
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=SURVEY_SECONDS + 15,
            env=env,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        write_json(
            out / "preflight-error.json",
            {
                "command": command,
                "script_sha256": sha(Path(__file__)),
                "started_utc": started.isoformat(),
                "error": repr(error),
            },
        )
        raise
    if len(result.stdout) + len(result.stderr) > 1024 * 1024:
        raise ValueError("mpstat output exceeded 1 MiB")
    (out / "mpstat.txt").write_text(result.stdout)
    (out / "mpstat.stderr.txt").write_text(result.stderr)
    after_swap = swap_used_kib()
    finished = datetime.datetime.now(datetime.timezone.utc)
    report = {
        "command": command,
        "script_sha256": sha(Path(__file__)),
        "started_utc": started.isoformat(),
        "finished_utc": finished.isoformat(),
        "survey_seconds": SURVEY_SECONDS,
        "minimum_sibling_idle_percent": MIN_IDLE_PERCENT,
        "mpstat_exit_code": result.returncode,
        "mpstat_sha256": sha(out / "mpstat.txt"),
        "host_load_before": before_load,
        "host_load_after": os.getloadavg(),
        "swap_used_kib_before": before_swap,
        "swap_used_kib_after": after_swap,
        "allowed_affinity": sorted(os.sched_getaffinity(0)),
        "uname": dict(
            zip(
                ("system", "node", "release", "version", "machine"),
                os.uname(),
            )
        ),
    }
    if result.returncode == 0:
        try:
            core = choose_core(average_idle(result.stdout), os.sched_getaffinity(0))
            report["core"] = core
            report["governors"] = {
                str(cpu): (path.read_text().strip() if path.exists() else "unavailable")
                for cpu in core["siblings"]
                for path in [
                    Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor")
                ]
            }
        except (ValueError, OSError) as error:
            report["core_error"] = repr(error)
    report["eligible"] = bool(
        result.returncode == 0
        and report.get("core", {}).get("all_siblings_at_least_90_percent_idle")
        and before_swap == 0
        and after_swap == 0
    )
    write_json(out / "preflight.json", report)
    print(out / "preflight.json")
    if not report["eligible"]:
        sys.exit(2)


if __name__ == "__main__":
    main()
