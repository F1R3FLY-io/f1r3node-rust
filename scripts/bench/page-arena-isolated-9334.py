import argparse
import hashlib
import json
import os
import random
import re
import runpy
import statistics
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRATCH = ROOT / "target" / "scratch"
CONTROL = SCRATCH / "page-arena-control-probe-11c92e7"
CANDIDATE_SOURCE = SCRATCH / "page-arena-9334-isolated-source-candidate"
PROBE = "rust::rholang::page_arena_isolated_probe::isolated_append_export_and_contiguous_hash_requests"
EXPECTED_BINARY_SHA256 = {
    "control": "5eb609a75a929457f65409879b416c712278ac62828203fa6e399c8ef4f7559f",
    "candidate": "7fc7b139607e7b205eb4dba14448a624b59ee8aa9b625279b14bf98d5268de3f",
}
EXPECTED_SOURCE_SHA256 = {
    CONTROL
    / "models/src/rust/rholang/canonical_bytes_arena.rs": "8baac501fa2468b049d0a100ca00000bc79f8e07f820211c303df8a82a95feb8",
    CANDIDATE_SOURCE
    / "models/src/rust/rholang/canonical_bytes_arena.rs": "ce493951d23332e60eda133cd72a0b26754799335d9811e56bc82cc334591438",
    CONTROL
    / "models/tests/support/page_arena_isolated_probe.rs": "5e4a10ceb999f17655f3dc03767980d76e2d29b96b5c18dd74fad0c408dec614",
    CANDIDATE_SOURCE
    / "models/tests/support/page_arena_isolated_probe.rs": "5e4a10ceb999f17655f3dc03767980d76e2d29b96b5c18dd74fad0c408dec614",
    CONTROL
    / "models/src/rust/rholang/mod.rs": "2b055a8e1577d5c90f0915747a393b24eae3d25da99be962380c28054560352f",
    CANDIDATE_SOURCE
    / "models/src/rust/rholang/mod.rs": "2b055a8e1577d5c90f0915747a393b24eae3d25da99be962380c28054560352f",
}
FIELDS = (
    "append_export_ns",
    "export_ns",
    "freeze_ns",
    "hash_ns",
    "wall_ns",
)
HELPER_PATH = Path(__file__).with_name("page-arena-consumer-9334.py")
EXPECTED_HELPER_SHA256 = (
    "60a1e43ec0b8e44a1328bf2a737970570a9852ae1aa141592265f25b51159f59"
)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_sources():
    result = {}
    for path, expected in EXPECTED_SOURCE_SHA256.items():
        actual = digest(path)
        if actual != expected:
            raise ValueError(f"source SHA256 changed: {path}: {actual}")
        result[str(path.relative_to(ROOT))] = actual
    return result


def run_one(args, helper, arm, phase, index, order=None, host=None):
    binary = args.binaries[arm]
    argv = [
        "taskset",
        "-c",
        args.cpus,
        str(binary),
        "--exact",
        PROBE,
        "--nocapture",
        "--test-threads=1",
    ]
    record = {
        "arm": arm,
        "phase": phase,
        "index": index,
        "order": order,
        "prepair_host": host,
        "command": argv,
    }
    env = os.environ.copy()
    env.update(LC_ALL="C", DISPLAY="", WAYLAND_DISPLAY="")
    started = time.perf_counter_ns()
    try:
        remaining = args.deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("30-minute isolated-run deadline reached")
        completed = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=min(args.timeout, remaining),
            env=env,
            check=False,
        )
        record.update(
            wall_ns=time.perf_counter_ns() - started,
            returncode=completed.returncode,
            stdout=completed.stdout,
            stderr=completed.stderr,
        )
        if completed.returncode != 0 or not re.search(
            r"test result: ok\. 1 passed; 0 failed", completed.stdout
        ):
            raise RuntimeError(f"probe test failed: {arm}, {phase}, {index}")
        lines = re.findall(r"page_arena_isolated_probe_v1 ([^\r\n]+)", completed.stdout)
        if len(lines) != 1:
            raise ValueError(
                f"expected exactly one probe record: {arm}, {phase}, {index}"
            )
        values = dict(token.split("=", 1) for token in lines[0].split())
        record["probe"] = {key: int(value) for key, value in values.items()}
        probe = record["probe"]
        if (
            probe["bytes"] != 65_536
            or probe["exports"] != 1_024
            or probe["hash_requests"] != 2_046
            or probe["contiguous_hits"] != 1_023
            or probe["copied_fallbacks"] != 1_023
        ):
            raise ValueError(f"unexpected fixed probe shape: {probe}")
    except Exception as failure:
        record["error"] = f"{type(failure).__name__}: {failure}"
        raise
    finally:
        helper["append_record"](args.out / "raw.jsonl", record)
    return record


def summarize(args, pairs, noisy_pairs, swap_pairs):
    rng = random.Random(args.seed + 1000)
    summaries = {}
    for field in FIELDS:
        ratios = [pair["candidate"][field] / pair["control"][field] for pair in pairs]
        bootstrap = sorted(
            statistics.median(rng.choices(ratios, k=len(ratios))) for _ in range(10_000)
        )
        summaries[field] = {
            "control_median": statistics.median(
                pair["control"][field] for pair in pairs
            ),
            "candidate_median": statistics.median(
                pair["candidate"][field] for pair in pairs
            ),
            "candidate_over_control_paired_median_ratio": statistics.median(ratios),
            "paired_bootstrap_95pct_interval": [bootstrap[249], bootstrap[9749]],
        }
    result = {
        "pairs": len(pairs),
        "warmups_per_arm": args.warmups,
        "busy_or_swap_pairs": noisy_pairs,
        "swap_pairs": swap_pairs,
        "host_noise_review_required": noisy_pairs > 0,
        "metrics": summaries,
        "request_schedule": "1023 within-64-byte-page 32-byte hashes and 1023 boundary-crossing 32-byte hashes; a synthetic eligibility check, not a production hit ratio",
        "interpretation": "isolated diagnostic; source and host differences remain; no experiment 395 primary verdict",
    }
    (args.out / "summary.json").write_text(json.dumps(result, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--control-bin", type=Path, required=True)
    parser.add_argument("--candidate-bin", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--cpus", default="0-7")
    parser.add_argument("--pairs", type=int, default=51)
    parser.add_argument("--warmups", type=int, default=5)
    parser.add_argument("--seed", type=int, default=9334)
    parser.add_argument("--timeout", type=int, default=30)
    args = parser.parse_args()
    os.environ.update(DISPLAY="", WAYLAND_DISPLAY="", LC_ALL="C")
    if os.environ.get("PAGE_ARENA_9334_CAPPED") != "1":
        parser.error("run inside a capped, no-swap user systemd scope")
    if args.pairs < 51 or args.warmups < 5 or args.timeout <= 0:
        parser.error(
            "at least 51 pairs, five warmups, and a positive child timeout required"
        )
    args.deadline = time.monotonic() + 30 * 60
    args.out = args.out.resolve()
    if not args.out.is_relative_to(SCRATCH.resolve()) or args.out.exists():
        parser.error("--out must be a fresh directory below target/scratch")
    args.binaries = {
        "control": args.control_bin.resolve(strict=True),
        "candidate": args.candidate_bin.resolve(strict=True),
    }
    if digest(HELPER_PATH) != EXPECTED_HELPER_SHA256:
        parser.error("consumer helper source changed from the pinned version")
    helper = runpy.run_path(str(HELPER_PATH))
    for arm, expected in EXPECTED_BINARY_SHA256.items():
        binary = args.binaries[arm]
        if "release" not in binary.parts or digest(binary) != expected:
            parser.error(f"wrong release diagnostic binary: {arm} {binary}")
        if helper["selected_test"](binary, PROBE) != PROBE:
            parser.error(f"probe test missing: {binary}")
    sources = verify_sources()
    args.out.mkdir(parents=True)
    metadata = {
        "control_bin": str(args.binaries["control"]),
        "candidate_bin": str(args.binaries["candidate"]),
        "binary_sha256": EXPECTED_BINARY_SHA256,
        "helper_sha256": EXPECTED_HELPER_SHA256,
        "source_sha256": sources,
        "probe": PROBE,
        "host": helper["host_snapshot"](args.cpus),
        "pairs": args.pairs,
        "warmups_per_arm": args.warmups,
        "seed": args.seed,
        "maximum_run_seconds": 30 * 60,
        "child_timeout_seconds": args.timeout,
    }
    (args.out / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    try:
        for index in range(args.warmups):
            order = (
                ("control", "candidate") if index % 2 == 0 else ("candidate", "control")
            )
            for arm in order:
                run_one(args, helper, arm, "warmup", index, list(order))
        orders = [0] * ((args.pairs + 1) // 2) + [1] * (args.pairs // 2)
        random.Random(args.seed).shuffle(orders)
        pairs = []
        noisy_pairs = 0
        swap_pairs = 0
        for index, reverse in enumerate(orders):
            order = ("candidate", "control") if reverse else ("control", "candidate")
            host = helper["pair_host_snapshot"](args.cpus)
            noisy_pairs += int(
                host["host_swap_in_use"] or host["selected_cpu_busy_over_10pct"]
            )
            swap_pairs += int(host["host_swap_in_use"])
            observed = {}
            for arm in order:
                observed[arm] = run_one(
                    args, helper, arm, "sample", index, list(order), host
                )
            pairs.append(
                {
                    arm: {**record["probe"], "wall_ns": record["wall_ns"]}
                    for arm, record in observed.items()
                }
            )
        summarize(args, pairs, noisy_pairs, swap_pairs)
    except Exception as failure:
        (args.out / "error.json").write_text(
            json.dumps({"type": type(failure).__name__, "message": str(failure)}) + "\n"
        )
        raise


if __name__ == "__main__":
    main()
