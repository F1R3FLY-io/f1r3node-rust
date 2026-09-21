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

from benchmark_host import cpu_snapshot
from paired_benchmark_statistics import stratified_loglog_slope

FAMILIES = ["set", "map", "not", "binder", "send", "pathmap",
            "spatial_list", "spatial_binder", "spatial_pathmap"]
ARMS = ["fixture", "operation"]
DEPTHS = [128, 256, 512, 1024, 2048, 4096]
PHASES = ["construction", "input_witness", "operation", "result_witness", "teardown"]
REPEATS = 31
WARMUPS = 3
SEED = 5915
FIELDS = ["population", "repeat", "family", "arm", "depth", "phase", "executed",
          "elapsed_ns", "allocations", "requested_bytes", "process_ns"]
PATTERN = re.compile(r"FIXTURE_SAMPLE\t(\w+)\t(\w+)\t(\d+)\t(\w+)\t([01])\t(\d+)\t(\d+)\t(\d+)")


def validate(rows):
    keys = [(r["population"], int(r["repeat"]), r["family"], r["arm"],
             int(r["depth"]), r["phase"]) for r in rows]
    expected = set()
    for population, count in [("warmup", WARMUPS), ("sample", REPEATS)]:
        expected.update(itertools.product([population], range(count), FAMILIES, ARMS, DEPTHS, PHASES))
    if len(keys) != len(set(keys)) or set(keys) != expected:
        raise ValueError("missing, duplicate or unexpected phase cells")
    for row in rows:
        skipped = row["arm"] == "fixture" and row["phase"] in ["operation", "result_witness"]
        if int(row["executed"]) != int(not skipped):
            raise ValueError("incorrect executed/skipped phase label")
        values = [int(row[field]) for field in ["elapsed_ns", "allocations", "requested_bytes"]]
        if any(value < 0 for value in values):
            raise ValueError("negative observation")
        if skipped and any(values):
            raise ValueError("skipped phases must have zero measurements")
        if not skipped and values[0] <= 0:
            raise ValueError("executed phase has no elapsed time")
        if row["phase"] == "construction" and min(values) <= 0:
            raise ValueError("empty construction observation")
        if not 0 < int(row["process_ns"]) < 20_000_000_000:
            raise ValueError("whole-child watchdog exceeded")
    return [row for row in rows if row["population"] == "sample"]


def summarize(rows):
    rows = validate(rows)
    indexed = {(r["family"], r["arm"], int(r["depth"]), int(r["repeat"]), r["phase"]): r for r in rows}
    results = {}
    for family, arm in itertools.product(FAMILIES, ARMS):
        result = {}
        for metric in ["elapsed_ns", "allocations", "requested_bytes"]:
            groups = [[sum(int(indexed[family, arm, depth, repeat, phase][metric])
                           for phase in PHASES[:2]) for repeat in range(REPEATS)] for depth in DEPTHS]
            result[metric] = stratified_loglog_slope(DEPTHS, groups, seed=SEED)
        result["depth4096_max_process_ns"] = max(int(indexed[family, arm, 4096, repeat, "construction"]["process_ns"])
                                                for repeat in range(REPEATS))
        results[f"{family}/{arm}"] = result
    upper = max(result["elapsed_ns"]["upper95"] for result in results.values())
    return {"cells": results, "maximum_latency_upper95": upper,
            "fixture_latency_pass": upper <= 1.20, "samples_per_cell": REPEATS}


def analyze(directory):
    with (directory / "samples.tsv").open() as source:
        summary = summarize(list(csv.DictReader(source, delimiter="\t")))
    (directory / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2), flush=True)
    return summary["fixture_latency_pass"]


def collect(binary, directory, cpu, pilot):
    if os.environ.get("FIXTURE_CAPPED") != "1":
        raise ValueError("run inside the prescribed systemd resource envelope")
    directory.mkdir(parents=True, exist_ok=False)
    raw, idle = cpu_snapshot(cpu)
    (directory / "cpu-before.json").write_text(raw)
    if not idle:
        raise RuntimeError("insufficient idle physical cores or selected core busy")
    sources = [Path(__file__), Path(__file__).with_name("benchmark_host.py"),
               Path(__file__).with_name("paired_benchmark_statistics.py"),
               Path("rholang/tests/stack_depth_gate.rs")]
    sources.extend(Path("rholang/tests/support") / name for name in
                   ["deep_fixture_scaling.rs", "planned_deep_fixture.rs", "counting_alloc.rs"])
    metadata = {
        "experiment": 347, "pilot_only": pilot, "seed": SEED,
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "source_sha256": {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "cpu": cpu, "stack_bytes": 1_048_576, "watchdog_seconds": 20,
        "repeats": 1 if pilot else REPEATS, "warmups": 0 if pilot else WARMUPS,
        "families": FAMILIES, "arms": ARMS, "depths": DEPTHS,
        "topology": subprocess.check_output(["lscpu", "-e=CPU,CORE,SOCKET,NODE"], text=True),
        "uname": subprocess.check_output(["uname", "-a"], text=True).strip(),
        "governor": Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor").read_text().strip(),
    }
    (directory / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    rng = random.Random(SEED)
    populations = [("pilot", 1)] if pilot else [("warmup", WARMUPS), ("sample", REPEATS)]
    with (directory / "samples.tsv").open("w") as output, (directory / "children.log").open("w") as log:
        writer = csv.writer(output, delimiter="\t")
        writer.writerow(FIELDS)
        for population, repeats in populations:
            for repeat in range(repeats):
                raw, idle = cpu_snapshot(cpu)
                (directory / f"cpu-{population}-{repeat}.json").write_text(raw)
                if not idle:
                    raise RuntimeError("CPU contention; partial population retained, not qualified")
                cells = list(itertools.product(FAMILIES, DEPTHS))
                rng.shuffle(cells)
                for family, depth in cells:
                    arms = ARMS.copy()
                    rng.shuffle(arms)
                    for arm in arms:
                        env = dict(os.environ, FIXTURE_FAMILY=family, FIXTURE_ARM=arm, FIXTURE_DEPTH=str(depth))
                        command = ["taskset", "-c", str(cpu), str(binary), "--ignored", "--exact",
                                   "deep_fixture_scaling::phase_child", "--test-threads=1", "--nocapture"]
                        start = time.monotonic_ns()
                        try:
                            child = subprocess.run(command, env=env, stdout=subprocess.PIPE,
                                                   stderr=subprocess.STDOUT, text=True, timeout=20)
                        except subprocess.TimeoutExpired as error:
                            log.write(f"TIMEOUT {population}/{repeat}/{family}/{arm}/{depth}\n{error.stdout!r}\n")
                            log.flush()
                            raise
                        duration = time.monotonic_ns() - start
                        log.write(f"CELL {population}/{repeat}/{family}/{arm}/{depth}\n{child.stdout}\n")
                        log.flush()
                        matches = PATTERN.findall(child.stdout)
                        for match in matches:
                            writer.writerow([population, repeat, *match, duration])
                        output.flush()
                        if child.returncode != 0:
                            raise RuntimeError(f"child failed: {family}/{arm}/{depth}: {child.returncode}")
                        if ([m[3] for m in matches] != PHASES
                                or any(m[:3] != (family, arm, str(depth)) for m in matches)):
                            raise ValueError("child emitted wrong or incomplete phase observations")
                print(f"completed {population} round {repeat + 1}/{repeats}", flush=True)
    raw, _ = cpu_snapshot(cpu)
    (directory / "cpu-after.json").write_text(raw)


def self_test():
    rows = []
    for population, count in [("warmup", WARMUPS), ("sample", REPEATS)]:
        for repeat, family, arm, depth, phase in itertools.product(range(count), FAMILIES, ARMS, DEPTHS, PHASES):
            executed = int(not (arm == "fixture" and phase in ["operation", "result_witness"]))
            rows.append(dict(zip(FIELDS, [population, repeat, family, arm, depth, phase,
                                          executed, executed, executed, executed, 1])))
    assert len(validate(rows)) == REPEATS * len(FAMILIES) * len(ARMS) * len(DEPTHS) * len(PHASES)
    for invalid in [rows[:-1], rows + [rows[0]],
                    [dict(r, process_ns=20_000_000_000) for r in rows],
                    [dict(r, executed=1) for r in rows],
                    [dict(r, population="pilot") for r in rows],
                    [dict(r, requested_bytes=-1) for r in rows]]:
        try:
            validate(invalid)
        except ValueError:
            pass
        else:
            raise AssertionError("invalid population accepted")
    for exponent in [1.0, 2.0]:
        estimate = stratified_loglog_slope(DEPTHS, [[float(d) ** exponent] * REPEATS for d in DEPTHS], seed=SEED)
        assert abs(estimate["upper95"] - exponent) < 1e-12
    linear = [dict(r, **{metric: r["executed"] * r["depth"]
                        for metric in ["elapsed_ns", "allocations", "requested_bytes"]}) for r in rows]
    assert summarize(linear)["fixture_latency_pass"]
    quadratic = [dict(r, elapsed_ns=r["elapsed_ns"] * r["depth"])
                 if r["family"] == "map" and r["arm"] == "fixture" else r for r in linear]
    rejection = summarize(quadratic)
    assert not rejection["fixture_latency_pass"]
    assert abs(rejection["maximum_latency_upper95"] - 2.0) < 1e-12
    print("deep fixture scaling analyzer self-tests passed")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--cpu", type=int, default=2)
    parser.add_argument("--pilot", action="store_true")
    parser.add_argument("--analyze-only", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if args.output is None or (not args.analyze_only and args.binary is None):
        parser.error("--output and (for collection) --binary required")
    if args.pilot and args.analyze_only:
        parser.error("a pilot is not an acceptance population")
    if not args.analyze_only:
        collect(args.binary.resolve(), args.output, args.cpu, args.pilot)
    if not args.pilot:
        raise SystemExit(0 if analyze(args.output) else 1)


if __name__ == "__main__":
    main()
