import argparse
import importlib.util
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PRIMARY = HERE / "page-arena-primary-395.py"
BUILD_SHA256 = "2379e8eab70cfac189bf05cdbba57650d4bfdc69b637b450ab66c03f1ffeb341"
SCHEMA_SHA256 = "73a551815a287983bf04e25331eeedcb01f9e683de902bf91bc7f03e75ba5269"

spec = importlib.util.spec_from_file_location("page_arena_primary_395", PRIMARY)
primary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(primary)
base = primary.base


def scoped_reexec():
    if os.environ.get("PAGE_ARENA_PRIMARY_SEMANTIC_SCOPED") == "1":
        return
    argv = [
        "systemd-run",
        "--user",
        "--scope",
        "--collect",
        f"--working-directory={ROOT}",
        "-p",
        "MemoryMax=5G",
        "-p",
        "MemorySwapMax=0",
        "-p",
        "CPUQuota=100%",
        "-p",
        "TasksMax=256",
        "-p",
        "RuntimeMaxSec=1200",
        "--setenv=PAGE_ARENA_PRIMARY_SEMANTIC_SCOPED=1",
        "--setenv=DISPLAY=",
        "--setenv=WAYLAND_DISPLAY=",
        "--setenv=JAVA_TOOL_OPTIONS=-Djava.awt.headless=true",
        sys.executable,
        str(Path(__file__).resolve()),
        *sys.argv[1:],
    ]
    os.execvp("systemd-run", argv)


def run(args):
    scoped_reexec()
    if os.environ.get("EPATHMAP_CANONICAL_GOLDENS_BLESS"):
        raise ValueError("golden bless mode is forbidden")
    out = args.out.resolve()
    data, _ = primary.prepared(out)
    build, build_sha = primary.built(out, "b2", data)
    if build_sha != BUILD_SHA256:
        raise ValueError("primary b2 build pin changed")
    schemas = {
        build["capacities"][str(capacity)][arm]["generated_schema_sha256"]
        for capacity in primary.CAPACITIES
        for arm in ("strong", "candidate")
    }
    if schemas != {SCHEMA_SHA256}:
        raise ValueError("matched generated schema changed")
    attempt = base.new_output(out / "semantic-gates" / args.attempt)
    cpu = min(os.sched_getaffinity(0))
    seed = primary.config()["seeds"][0]
    metadata = {
        "script_sha256": base.sha(Path(__file__)),
        "prepared_sha256": base.sha(out / "prepared.json"),
        "build_sha256": build_sha,
        "source_manifest_sha256": data["manifest_sha256"],
        "generated_schema_sha256": SCHEMA_SHA256,
        "seed": seed,
        "pinned_cpu": cpu,
        "deadline_seconds": 1170,
        "timing_verdict": "not_sampled",
    }
    base.write_json(attempt / "metadata.json", metadata)
    deadline = time.monotonic() + 1170
    completed = []
    try:
        for capacity in primary.CAPACITIES:
            paired = {}
            for arm in ("strong", "candidate"):
                binary = Path(
                    build["capacities"][str(capacity)][arm]["binaries"]["models"]["path"]
                )
                selected = base.selected_test(binary, primary.TEST.split("::")[-1])
                os.environ["PAGE_ARENA_PRIMARY_SEED"] = seed
                os.environ["PAGE_ARENA_PRIMARY_CAPACITY"] = str(capacity)
                row = base.run_test(binary, selected, cpu, deadline, 180)
                parsed = primary.parse_record(row["stdout"], arm, seed, capacity)
                paired[arm] = {item["workload"]: item for item in parsed}
                base.append_row(
                    attempt / "primary-semantics.jsonl",
                    {"capacity": capacity, "arm": arm, "selected_test": selected, **row},
                )
                completed.append({"capacity": capacity, "arm": arm, "kind": "primary"})
            for workload in primary.WORKLOADS:
                control = paired["strong"][workload]
                candidate = paired["candidate"][workload]
                for key in (
                    "trace_digest",
                    "exports_dropped",
                    "views_dropped",
                    "active_views",
                    "branches",
                ):
                    if control[key] != candidate[key]:
                        raise ValueError(
                            f"primary semantic trace mismatch at {capacity}/{workload}/{key}"
                        )
                base.append_row(
                    attempt / "primary-cell-goldens.jsonl",
                    {"capacity": capacity, "workload": workload, "trace_digest": control["trace_digest"]},
                )
        os.environ.pop("PAGE_ARENA_PRIMARY_SEED", None)
        os.environ.pop("PAGE_ARENA_PRIMARY_CAPACITY", None)
        for fixture, (kind, suffix) in base.FIXTURES.items():
            fixture_capacity = 64 if fixture == "isolated" else 256
            for arm in ("strong", "candidate"):
                binary = Path(
                    build["capacities"][str(fixture_capacity)][arm]["binaries"][kind]["path"]
                )
                selected = base.selected_test(binary, suffix)
                row = base.run_test(binary, selected, cpu, deadline, 90)
                base.append_row(
                    attempt / "consumer-semantics.jsonl",
                    {"fixture": fixture, "capacity": fixture_capacity, "arm": arm, "selected_test": selected, **row},
                )
                completed.append({"fixture": fixture, "capacity": fixture_capacity, "arm": arm, "kind": "consumer"})
        base.write_json(
            attempt / "summary.json",
            {
                "completed": completed,
                "primary_test_invocations": 8,
                "primary_cell_goldens": 16,
                "consumer_fixture_invocations": 14,
                "primary_semantics_sha256": base.sha(attempt / "primary-semantics.jsonl"),
                "cell_goldens_sha256": base.sha(attempt / "primary-cell-goldens.jsonl"),
                "consumer_semantics_sha256": base.sha(attempt / "consumer-semantics.jsonl"),
                "timing_verdict": "not_sampled",
                "full_models_parity": "open",
            },
        )
    except Exception as error:
        base.write_json(attempt / "error.json", {"completed": completed, "error": repr(error)})
        raise
    print(attempt / "summary.json")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--attempt", type=base.attempt_name, required=True)
    run(parser.parse_args())


if __name__ == "__main__":
    main()
