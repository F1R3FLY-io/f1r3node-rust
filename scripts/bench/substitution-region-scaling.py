import argparse
import csv
import hashlib
import itertools
import json
import os
from pathlib import Path
import random
import re
import subprocess
import time

from paired_benchmark_statistics import stratified_loglog_slope
from benchmark_host import cpu_snapshot

DEPTHS = [128, 256, 512, 1024, 2048, 4096]
ENTRIES = ["no_sort", "sorted", "charged"]
SEED = 5916
REPEATS = 31
WARMUPS = 3
FIELDS = [
    "phase",
    "repeat",
    "entry",
    "depth",
    "elapsed_ns",
    "allocations",
    "requested_bytes",
    "input_bytes",
    "output_bytes",
    "process_ns",
]
PATTERN = re.compile(
    r"SUBSTITUTION_SAMPLE\t(\w+)\t(\d+)\t(\d+)\t(\d+)\t(\d+)\t(\d+)\t(\d+)"
)


def validate(rows):
    keys = [
        (row["phase"], row["entry"], int(row["depth"]), int(row["repeat"]))
        for row in rows
    ]
    expected = set(itertools.product(["sample"], ENTRIES, DEPTHS, range(REPEATS)))
    expected.update(itertools.product(["warmup"], ENTRIES, DEPTHS, range(WARMUPS)))
    if len(keys) != len(set(keys)) or set(keys) != expected:
        raise ValueError("missing, duplicate or unexpected measurement cells")
    for row in rows:
        for field in FIELDS[4:]:
            if int(row[field]) <= 0:
                raise ValueError(f"nonpositive {field}")
        if int(row["process_ns"]) >= 20_000_000_000:
            raise ValueError("20-second process watchdog exceeded")
    return [row for row in rows if row["phase"] == "sample"]


def analyze(path):
    with (path / "samples.tsv").open() as source:
        rows = validate(list(csv.DictReader(source, delimiter="\t")))
    summary = {}
    for entry in ENTRIES:
        entry_rows = [row for row in rows if row["entry"] == entry]
        summary[entry] = {}
        for metric in ["elapsed_ns", "allocations", "requested_bytes"]:
            groups = [
                [int(row[metric]) for row in entry_rows if int(row["depth"]) == depth]
                for depth in DEPTHS
            ]
            summary[entry][metric] = stratified_loglog_slope(DEPTHS, groups)
        summary[entry]["depth4096_max_process_ns"] = max(
            int(row["process_ns"]) for row in entry_rows if int(row["depth"]) == 4096
        )
    summary["latency_pass"] = all(
        summary[entry]["elapsed_ns"]["upper95"] <= 1.20 for entry in ENTRIES
    )
    (path / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2), flush=True)
    return summary["latency_pass"]


def collect(binary, path, cpu):
    if os.environ.get("SUBSTITUTION_CAPPED") != "1":
        raise ValueError("run inside the prescribed systemd memory/CPU envelope")
    path.mkdir(parents=True, exist_ok=False)
    raw, idle = cpu_snapshot(cpu)
    (path / "cpu-before.json").write_text(raw)
    if not idle:
        raise RuntimeError("not enough idle physical cores or selected CPU is busy")
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    diff = subprocess.check_output(
        ["git", "diff", "--", "rholang/tests/stack_depth_gate.rs"]
    )
    metadata = {
        "source_sha256": {
            str(source): hashlib.sha256(source.read_bytes()).hexdigest()
            for source in [
                Path(__file__),
                Path(__file__).with_name("paired_benchmark_statistics.py"),
                Path(__file__).with_name("benchmark_host.py"),
                Path("rholang/tests/stack_depth_gate.rs"),
                Path("rholang/tests/support/substitution_scaling.rs"),
            ]
        },
        "git_ref": revision,
        "binary": str(binary),
        "binary_sha256": hashlib.file_digest(binary.open("rb"), "sha256").hexdigest(),
        "gate_diff_sha256": hashlib.sha256(diff).hexdigest(),
        "seed": SEED,
        "repeats": REPEATS,
        "warmups": WARMUPS,
        "cpu": cpu,
        "stack_bytes": 1048576,
        "watchdog_seconds": 20,
        "uname": subprocess.check_output(["uname", "-a"], text=True).strip(),
        "topology": subprocess.check_output(
            ["lscpu", "-e=CPU,CORE,SOCKET,NODE"], text=True
        ),
        "governor": Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor")
        .read_text()
        .strip(),
    }
    (path / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    rng = random.Random(SEED)
    with (
        (path / "samples.tsv").open("w") as output,
        (path / "children.log").open("w") as log,
    ):
        writer = csv.writer(output, delimiter="\t")
        writer.writerow(FIELDS)
        for phase, repeats in [("warmup", WARMUPS), ("sample", REPEATS)]:
            for repeat in range(repeats):
                raw, idle = cpu_snapshot(cpu)
                (path / f"cpu-{phase}-{repeat}.json").write_text(raw)
                if not idle:
                    raise RuntimeError(
                        f"CPU contention before {phase} round {repeat}; retain partial run"
                    )
                cells = list(itertools.product(ENTRIES, DEPTHS))
                rng.shuffle(cells)
                for entry, depth in cells:
                    env = dict(
                        os.environ,
                        GATE_SUBJECT=f"substitute_bound_{entry}",
                        GATE_DEPTH=str(depth),
                        GATE_STACK="1048576",
                    )
                    command = [
                        "taskset",
                        "-c",
                        str(cpu),
                        str(binary),
                        "--ignored",
                        "--exact",
                        "gate_child",
                        "--test-threads=1",
                        "--nocapture",
                    ]
                    start = time.monotonic_ns()
                    try:
                        result = subprocess.run(
                            command,
                            env=env,
                            stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT,
                            text=True,
                            timeout=20,
                        )
                    except subprocess.TimeoutExpired as error:
                        log.write(
                            f"TIMEOUT {phase} {repeat} {entry} {depth}\n{error.stdout!r}\n"
                        )
                        log.flush()
                        raise
                    duration = time.monotonic_ns() - start
                    log.write(
                        f"CELL {phase} {repeat} {entry} {depth}\n{result.stdout}\n"
                    )
                    log.flush()
                    if result.returncode != 0:
                        raise RuntimeError(
                            f"child failed: {entry}/{depth}: {result.returncode}"
                        )
                    matches = PATTERN.findall(result.stdout)
                    if len(matches) != 1 or matches[0][:2] != (entry, str(depth)):
                        raise ValueError(
                            "child did not emit exactly its requested sample"
                        )
                    writer.writerow([phase, repeat, *matches[0], duration])
                    output.flush()
                print(f"completed {phase} round {repeat + 1}/{repeats}", flush=True)
    raw, _ = cpu_snapshot(cpu)
    (path / "cpu-after.json").write_text(raw)


def self_test():
    for exponent in [0.5, 1.0, 2.0]:
        groups = [[float(depth) ** exponent] * REPEATS for depth in DEPTHS]
        estimate = stratified_loglog_slope(DEPTHS, groups)
        assert abs(estimate["estimate"] - exponent) < 1e-12
        assert abs(estimate["upper95"] - exponent) < 1e-12
    rows = [
        dict(zip(FIELDS, ["sample", repeat, entry, depth, 1, 1, 1, 1, 1, 1]))
        for entry, depth, repeat in itertools.product(ENTRIES, DEPTHS, range(REPEATS))
    ]
    warmups = [
        dict(zip(FIELDS, ["warmup", repeat, entry, depth, 1, 1, 1, 1, 1, 1]))
        for entry, depth, repeat in itertools.product(ENTRIES, DEPTHS, range(WARMUPS))
    ]
    samples_only = rows.copy()
    rows += warmups
    assert len(validate(rows)) == 558
    for invalid in [
        rows[:-1],
        rows + [rows[0]],
        samples_only,
        rows + [dict(rows[0], phase="unknown")],
        [dict(row, process_ns=20_000_000_000) for row in rows],
    ]:
        try:
            validate(invalid)
        except ValueError:
            pass
        else:
            raise AssertionError("invalid population accepted")
    print("substitution scaling analyzer self-tests passed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--cpu", type=int, default=2)
    parser.add_argument("--analyze-only", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if args.output is None or (not args.analyze_only and args.binary is None):
        parser.error("--output and (for collection) --binary are required")
    if not args.analyze_only:
        collect(args.binary.resolve(), args.output, args.cpu)
    if not analyze(args.output):
        raise SystemExit("required latency scaling gate failed")


if __name__ == "__main__":
    main()
