import argparse
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
HARNESS = HERE / "page-arena-primary-395-full-models.py"
BUILD_SHA256 = "86b89d8e9dd5849d4eaa518d01a1c32ae0e7eb235cb204872973e87292544cf4"
LEGACY_SCRIPT_SHA256 = "8ca5c6a98b277680a60b8a7cbeae69710262e843b091b1c1eddbe4b7398aa7f1"
CAPACITY = 64
PREVIOUS_RUNNER_SHA256 = "88d276fdbd6b2a1aca00ea9862c7bbe84f948163972c806910ce3226e6a15a89"
PREVIOUS_RESULTS_SHA256 = "4f2df0bc7e23ed0ffdcc9ede2a892aeb30f32708c1e30fd0fd42c76e682261fa"
PREVIOUS_ERROR_SHA256 = "478fef9f8fae55025cfa4532fed34485cad64e75487c3cc109c137071fa0dfd0"
PREVIOUS_SCOPE_RESULT_SHA256 = "b61f7c5dcad18b2ac472b2490b8fdc29c81219e31563eb5ea0708b01c6677ecf"
PREVIOUS_COMMAND_SHA256 = "8e75c11eaccf0e5b864f98a98517fff31e765c2dc4263a28a16a068ba8e7410f"
PREVIOUS_TIMEOUT_LOG_SHA256 = "170c7cd99e70be2a3a49ae64765aa86aa7d36333786518c1a9046d9db29e6b54"
TARGET_TIMEOUT_SECONDS = 1800
FAILED_TEST = re.compile(r"^test (.+?) \.\.\. FAILED$", re.MULTILINE)

spec = importlib.util.spec_from_file_location("page_arena_full_models_395", HARNESS)
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)
base = harness.base


def guarded(out, build_attempt, expected_build_sha256=BUILD_SHA256,
            expected_primary_build_sha256=harness.BUILD_SHA256,
            expected_schema_sha256=harness.SCHEMA_SHA256):
    data, _, names = harness.pinned(out, expected_primary_build_sha256)
    report, digest = harness.built(
        out, build_attempt, data, names,
        expected_primary_build_sha256, expected_schema_sha256,
    )
    if digest != expected_build_sha256:
        raise ValueError("all-target build differs from the supplied receipt")
    return report, names


def resume_rows(out, args, attempt, build, ordered):
    if args.resume_from is None:
        return []
    if args.build_attempt != "p1" or args.build_sha256 != BUILD_SHA256:
        raise ValueError("historical r3 continuation belongs only to the protected p1 build")
    previous = out / "full-models-parity" / args.build_attempt / "runs" / args.resume_from
    source = previous / "runner-source.py"
    if base.sha(source) != PREVIOUS_RUNNER_SHA256:
        raise ValueError("previous runner source was not preserved exactly")
    if (
        base.sha(previous / "results.jsonl") != PREVIOUS_RESULTS_SHA256
        or base.sha(previous / "error.json") != PREVIOUS_ERROR_SHA256
        or base.sha(previous / "scope-result.json") != PREVIOUS_SCOPE_RESULT_SHA256
        or base.sha(previous / "command.json") != PREVIOUS_COMMAND_SHA256
        or base.sha(previous / f"{args.arm}-trie_escape_arm_stack.log")
        != PREVIOUS_TIMEOUT_LOG_SHA256
    ):
        raise ValueError("previous bounded attempt differs from its recorded evidence")
    error = json.loads((previous / "error.json").read_text())
    command = json.loads((previous / "command.json").read_text())
    scope = json.loads((previous / "scope-result.json").read_text())
    old_path = previous / "results.jsonl"
    rows = [json.loads(line) for line in old_path.read_text().splitlines()]
    if (
        args.arm != "strong"
        or args.resume_from != "r3-strong"
        or error["arm"] != args.arm
        or error["completed"] != 51
        or error["expected"] != len(ordered)
        or len(rows) != 51
        or [row["target"] for row in rows] != ordered[:51]
        or command["script_sha256"] != PREVIOUS_RUNNER_SHA256
        or scope["exit_code"] != 1
        or scope["scope_log_sha256"] != base.sha(previous / "scope.log")
        or "TimeoutExpired" not in error["error"]
        or ordered[51] not in error["error"]
    ):
        raise ValueError("previous bounded strong attempt is not the expected prefix")
    for row in rows:
        name = row["target"]
        original = previous / f"{args.arm}-{name}.log"
        binary = Path(build["arms"][args.arm]["executables"][name]["path"])
        if (
            row["arm"] != args.arm
            or row["command"] != [str(binary), "--test-threads=1"]
            or row["log_sha256"] != base.sha(original)
        ):
            raise ValueError(f"previous {args.arm}/{name} result changed")
        destination = attempt / original.name
        shutil.copy2(original, destination)
        if base.sha(destination) != row["log_sha256"]:
            raise ValueError(f"resumed {args.arm}/{name} log copy changed")
        base.append_row(attempt / "results.jsonl", row)
    base.write_json(attempt / "resumed-from.json", {
        "attempt": args.resume_from,
        "previous_runner_sha256": PREVIOUS_RUNNER_SHA256,
        "previous_results_sha256": PREVIOUS_RESULTS_SHA256,
        "previous_error_sha256": PREVIOUS_ERROR_SHA256,
        "previous_scope_result_sha256": PREVIOUS_SCOPE_RESULT_SHA256,
        "previous_command_sha256": PREVIOUS_COMMAND_SHA256,
        "previous_timeout_log_sha256": PREVIOUS_TIMEOUT_LOG_SHA256,
        "next_target": ordered[51],
        "next_target_timeout_seconds": TARGET_TIMEOUT_SECONDS,
    })
    return rows


def run_inner(args):
    if os.environ.get("PAGE_ARENA_FULL_MODELS_SCOPED") != "1":
        raise ValueError("full models tests require their bounded headless scope")
    seed = harness.primary.config()["seeds"][0]
    if (
        os.environ.get("PAGE_ARENA_PRIMARY_SEED") != seed
        or os.environ.get("PAGE_ARENA_PRIMARY_CAPACITY") != str(CAPACITY)
    ):
        raise ValueError("primary fixture seed/capacity environment changed")
    out = args.out.resolve()
    build, names = guarded(out, args.build_attempt, args.build_sha256,
                           args.primary_build_sha256, args.schema_sha256)
    attempt = out / "full-models-parity" / args.build_attempt / "runs" / args.attempt
    if not attempt.is_dir():
        raise ValueError("test attempt was not prepared")
    if (attempt / "metadata.json").exists() or (attempt / "results.jsonl").exists():
        raise ValueError("test attempt has already started")
    env = os.environ.copy()
    env.update(LC_ALL="C", DISPLAY="", WAYLAND_DISPLAY="", JAVA_TOOL_OPTIONS="-Djava.awt.headless=true")
    env.pop("EPATHMAP_CANONICAL_GOLDENS_BLESS", None)
    base.write_json(attempt / "metadata.json", {
        "script_sha256": base.sha(Path(__file__)),
        "build_sha256": args.build_sha256,
        "arm": args.arm,
        "seed": seed,
        "capacity": CAPACITY,
        "target_names": sorted(names),
        "timeout_seconds_per_target": TARGET_TIMEOUT_SECONDS,
        "deadline_seconds": 7170,
        "resume_from": args.resume_from,
    })
    deadline = time.monotonic() + 7170
    ordered = ["models", *sorted(names - {"models"})]
    completed = []
    try:
        completed = resume_rows(out, args, attempt, build, ordered)
        for name in ordered[len(completed):]:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("full models arm deadline reached")
            binary = Path(build["arms"][args.arm]["executables"][name]["path"])
            log = attempt / f"{args.arm}-{name}.log"
            command = [str(binary), "--test-threads=1"]
            timed_out = False
            timeout_seconds = min(TARGET_TIMEOUT_SECONDS, remaining)
            with log.open("w") as stream:
                try:
                    result = subprocess.run(
                        command, stdout=stream, stderr=subprocess.STDOUT,
                        timeout=timeout_seconds, env=env, check=False,
                    )
                    exit_code = result.returncode
                except subprocess.TimeoutExpired:
                    timed_out = True
                    exit_code = 124
            if log.stat().st_size > 32 * 1024**2:
                raise ValueError(f"unbounded test log: {log}")
            row = {
                "arm": args.arm, "target": name, "command": command,
                "exit_code": exit_code, "timed_out": timed_out,
                "timeout_seconds": timeout_seconds,
                "log_sha256": base.sha(log),
                "failed_tests": FAILED_TEST.findall(log.read_text(errors="replace")),
            }
            base.append_row(attempt / "results.jsonl", row)
            completed.append(row)
        failed = [row for row in completed if row["exit_code"] != 0]
        base.write_json(attempt / "summary.json", {
            "arm": args.arm, "completed": len(completed), "expected": len(names),
            "results_sha256": base.sha(attempt / "results.jsonl"),
            "failed_targets": [row["target"] for row in failed],
            "arm_parity": "passed" if not failed else "failed",
        })
    except Exception as error:
        base.write_json(attempt / "error.json", {
            "arm": args.arm, "completed": len(completed), "expected": len(names),
            "error": repr(error),
        })
        raise
    print(attempt / "summary.json")


def compare(args):
    out = args.out.resolve()
    build, names = guarded(out, args.build_attempt, args.build_sha256,
                           args.primary_build_sha256, args.schema_sha256)
    directory = out / "full-models-parity" / args.build_attempt / "runs"
    arms = {}
    for arm, attempt_name in (("strong", args.strong_attempt), ("candidate", args.candidate_attempt)):
        attempt = directory / attempt_name
        summary = json.loads((attempt / "summary.json").read_text())
        metadata = json.loads((attempt / "metadata.json").read_text())
        scope = json.loads((attempt / "scope-result.json").read_text())
        result_path = attempt / "results.jsonl"
        if (
            summary["arm"] != arm
            or metadata["script_sha256"] not in (
                {base.sha(Path(__file__)), LEGACY_SCRIPT_SHA256}
                if args.build_attempt == "p1" and args.build_sha256 == BUILD_SHA256
                else {base.sha(Path(__file__))}
            )
            or metadata["build_sha256"] != args.build_sha256
            or metadata["arm"] != arm
            or metadata["seed"] != harness.primary.config()["seeds"][0]
            or metadata["capacity"] != CAPACITY
            or metadata["target_names"] != sorted(names)
            or metadata["timeout_seconds_per_target"] != TARGET_TIMEOUT_SECONDS
            or summary["completed"] != len(names)
            or summary["expected"] != len(names)
            or summary["results_sha256"] != base.sha(result_path)
            or scope["exit_code"] != 0
            or scope["scope_log_sha256"] != base.sha(attempt / "scope.log")
        ):
            raise ValueError(f"incomplete or changed {arm} run: {attempt}")
        rows = [json.loads(line) for line in result_path.read_text().splitlines()]
        by_target = {row["target"]: row for row in rows}
        if len(rows) != len(names) or set(by_target) != names:
            raise ValueError(f"{arm} target census changed")
        if summary["failed_targets"] != [row["target"] for row in rows if row["exit_code"] != 0]:
            raise ValueError(f"{arm} failure summary changed")
        for name, row in by_target.items():
            binary = build["arms"][arm]["executables"][name]["path"]
            if (
                row["arm"] != arm
                or row["command"] != [binary, "--test-threads=1"]
                or row["log_sha256"] != base.sha(attempt / f"{arm}-{name}.log")
                or (row["exit_code"] == 0 and row.get("timed_out", False))
            ):
                raise ValueError(f"{arm}/{name} raw log changed")
        arms[arm] = by_target
    comparison = []
    for name in sorted(names):
        strong = arms["strong"][name]
        candidate = arms["candidate"][name]
        comparison.append({
            "target": name,
            "strong_exit_code": strong["exit_code"],
            "candidate_exit_code": candidate["exit_code"],
            "strong_failed_tests": strong["failed_tests"],
            "candidate_failed_tests": candidate["failed_tests"],
            "strong_timed_out": strong.get("timed_out", False),
            "candidate_timed_out": candidate.get("timed_out", False),
            "same_failure_signature": (
                strong["exit_code"] == candidate["exit_code"]
                and strong["failed_tests"] == candidate["failed_tests"]
                and not strong.get("timed_out", False)
                and not candidate.get("timed_out", False)
                and (strong["exit_code"] == 0 or bool(strong["failed_tests"]))
            ),
        })
    report = {
        "script_sha256": base.sha(Path(__file__)),
        "build_sha256": args.build_sha256,
        "strong_attempt": args.strong_attempt,
        "candidate_attempt": args.candidate_attempt,
        "target_count": len(names),
        "full_models_parity": "passed" if all(
            row["strong_exit_code"] == row["candidate_exit_code"] == 0
            for row in comparison
        ) else "failed",
        "failure_signatures_match": all(row["same_failure_signature"] for row in comparison),
        "targets": comparison,
    }
    destination = base.new_output(directory / args.attempt)
    base.write_json(destination / "comparison.json", report)
    print(destination / "comparison.json")


def run(args):
    out = args.out.resolve()
    guarded(out, args.build_attempt, args.build_sha256,
            args.primary_build_sha256, args.schema_sha256)
    attempt = base.new_output(
        out / "full-models-parity" / args.build_attempt / "runs" / args.attempt
    )
    seed = harness.primary.config()["seeds"][0]
    command = [
        "systemd-run", "--user", "--scope", "--collect",
        f"--working-directory={ROOT}",
        "-p", "MemoryMax=5G", "-p", "MemorySwapMax=0",
        "-p", "CPUQuota=100%", "-p", "TasksMax=256",
        "-p", "RuntimeMaxSec=7200",
        "--setenv=PAGE_ARENA_FULL_MODELS_SCOPED=1",
        f"--setenv=PAGE_ARENA_PRIMARY_SEED={seed}",
        f"--setenv=PAGE_ARENA_PRIMARY_CAPACITY={CAPACITY}",
        "--setenv=DISPLAY=", "--setenv=WAYLAND_DISPLAY=",
        "--setenv=JAVA_TOOL_OPTIONS=-Djava.awt.headless=true",
        "--setenv=LC_ALL=C",
        sys.executable, str(Path(__file__).resolve()), "run-inner",
        "--out", str(out), "--build-attempt", args.build_attempt,
        "--attempt", args.attempt, "--arm", args.arm,
        "--build-sha256", args.build_sha256,
        "--primary-build-sha256", args.primary_build_sha256,
        "--schema-sha256", args.schema_sha256,
    ]
    if args.resume_from is not None:
        command.extend(["--resume-from", args.resume_from])
    base.write_json(attempt / "command.json", {
        "argv": command, "script_sha256": base.sha(Path(__file__))
    })
    with (attempt / "scope.log").open("w") as stream:
        result = subprocess.run(
            command, stdout=stream, stderr=subprocess.STDOUT,
            timeout=7230, check=False,
        )
    base.write_json(attempt / "scope-result.json", {
        "exit_code": result.returncode,
        "scope_log_sha256": base.sha(attempt / "scope.log"),
    })
    if result.returncode != 0:
        raise RuntimeError(f"full models {args.arm} scope exit {result.returncode}")
    print(attempt / "summary.json")


def main():
    parser = argparse.ArgumentParser()
    modes = parser.add_subparsers(dest="mode", required=True)
    for mode in ("run", "run-inner", "compare"):
        command = modes.add_parser(mode)
        command.add_argument("--out", type=Path, required=True)
        command.add_argument("--build-attempt", type=base.attempt_name, required=True)
        command.add_argument("--attempt", type=base.attempt_name, required=True)
        command.add_argument("--build-sha256", default=BUILD_SHA256)
        command.add_argument("--primary-build-sha256", default=harness.BUILD_SHA256)
        command.add_argument("--schema-sha256", default=harness.SCHEMA_SHA256)
        if mode == "compare":
            command.add_argument("--strong-attempt", type=base.attempt_name, required=True)
            command.add_argument("--candidate-attempt", type=base.attempt_name, required=True)
        else:
            command.add_argument("--arm", choices=harness.ARMS, required=True)
            command.add_argument("--resume-from", type=base.attempt_name)
    args = parser.parse_args()
    {"run": run, "run-inner": run_inner, "compare": compare}[args.mode](args)


if __name__ == "__main__":
    main()
