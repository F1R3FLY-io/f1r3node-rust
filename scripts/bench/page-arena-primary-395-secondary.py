import argparse
import importlib.util
import json
import os
import random
import resource
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PRIMARY = HERE / "page-arena-primary-395.py"
ATTRIBUTION = HERE / "page-arena-attribution-9334.py"
FULL_MODELS = HERE / "page-arena-primary-395-full-models-run.py"
BUILD_SHA256 = "2379e8eab70cfac189bf05cdbba57650d4bfdc69b637b450ab66c03f1ffeb341"
SEMANTICS_SHA256 = "729882d56641593c75d99da39b0b879ca3cea2a10a86b0c39bef4c7b39a409c2"
LEGACY_SEMANTICS_SCRIPT_SHA256 = "4937216e9276f259c1a245aa0495a301e4b902ce57bdd9d946d21a8a68a8ce14"
R3_BUILD_SHA256 = "875aa310f8b9900f481b33b41df8db080edf087f3e9820de6bd15e21f26e8e44"
R3_SCHEMA_SHA256 = "73a551815a287983bf04e25331eeedcb01f9e683de902bf91bc7f03e75ba5269"
R3_G3_SEMANTICS_SHA256 = "ef67b9f7fa1708acd3abcf5f1fde01bb666167bcdd30d065826a85af445c6a73"
R3_G3_SCRIPT_SHA256 = "0da0629ea74657efe4f39ccd22fada2c8261fa3f192456fdc257859429d43189"
FIXTURES = ("act20k", "width4096")


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


primary = module("page_arena_primary_395", PRIMARY)
attribution = module("page_arena_attribution_9334", ATTRIBUTION)
full_models = module("page_arena_full_models_run_395", FULL_MODELS)
base = primary.base


def primary_evidence(out, expected_primary_build_sha256, expected_schema_sha256):
    if os.environ.get("EPATHMAP_CANONICAL_GOLDENS_BLESS") is not None:
        raise ValueError("protobuf goldens must remain read-only")
    data, _ = primary.prepared(out)
    build, digest = primary.built(out, "b2", data)
    if digest != expected_primary_build_sha256:
        raise ValueError("primary b2 build differs from the supplied receipt")
    schemas = {
        build["capacities"][str(capacity)][arm]["generated_schema_sha256"]
        for capacity in primary.CAPACITIES
        for arm in ("strong", "candidate")
    }
    if schemas != {expected_schema_sha256}:
        raise ValueError("primary generated schema differs from the supplied receipt")
    return data, build, digest


def pinned(out, expected_primary_build_sha256=BUILD_SHA256,
           expected_semantics_sha256=SEMANTICS_SHA256,
           expected_schema_sha256=full_models.harness.SCHEMA_SHA256,
           semantic_attempt="g2"):
    data, build, digest = primary_evidence(
        out, expected_primary_build_sha256, expected_schema_sha256,
    )
    semantic_dir = out / "semantic-gates" / semantic_attempt
    semantics = semantic_dir / "summary.json"
    if base.sha(semantics) != expected_semantics_sha256:
        raise ValueError("semantic gate differs from the supplied receipt")
    summary = json.loads(semantics.read_text())
    metadata = json.loads((semantic_dir / "metadata.json").read_text())
    legacy = (
        semantic_attempt == "g2"
        and expected_primary_build_sha256 == BUILD_SHA256
        and expected_schema_sha256 == full_models.harness.SCHEMA_SHA256
        and expected_semantics_sha256 == SEMANTICS_SHA256
        and metadata["script_sha256"] == LEGACY_SEMANTICS_SCRIPT_SHA256
    )
    archived_g3 = (
        semantic_attempt == "g3"
        and expected_primary_build_sha256 == R3_BUILD_SHA256
        and expected_schema_sha256 == R3_SCHEMA_SHA256
        and expected_semantics_sha256 == R3_G3_SEMANTICS_SHA256
        and metadata["script_sha256"] == R3_G3_SCRIPT_SHA256
    )
    completed = [
        {"capacity": capacity, "arm": arm, "kind": "primary"}
        for capacity in primary.CAPACITIES
        for arm in ("strong", "candidate")
    ] + [
        {"fixture": fixture, "capacity": 64 if fixture == "isolated" else 256,
         "arm": arm, "kind": "consumer"}
        for fixture in attribution.FIXTURES
        for arm in ("strong", "candidate")
    ]
    if (
        not (legacy or archived_g3 or metadata["script_sha256"] == base.sha(Path(__file__)))
        or metadata["prepared_sha256"] != base.sha(out / "prepared.json")
        or metadata["build_sha256"] != digest
        or metadata["source_manifest_sha256"] != data["manifest_sha256"]
        or metadata["generated_schema_sha256"] != expected_schema_sha256
        or metadata["seed"] != primary.config()["seeds"][0]
        or summary["completed"] != completed
        or summary["primary_test_invocations"] != 8
        or summary["consumer_fixture_invocations"] != 14
        or summary["primary_cell_goldens"] != 16
        or summary["primary_semantics_sha256"] != base.sha(semantic_dir / "primary-semantics.jsonl")
        or summary["cell_goldens_sha256"] != base.sha(semantic_dir / "primary-cell-goldens.jsonl")
        or summary["consumer_semantics_sha256"] != base.sha(semantic_dir / "consumer-semantics.jsonl")
        or summary["timing_verdict"] != "not_sampled"
    ):
        raise ValueError("matched semantic gate is incomplete")
    binaries = {}
    for arm in ("strong", "candidate"):
        item = build["capacities"]["64"][arm]["binaries"]["models"]
        binary = Path(item["path"])
        if base.sha(binary) != item["sha256"]:
            raise ValueError(f"protected {arm} models binary changed")
        binaries[arm] = binary
    source = out / "source"
    fixture_hashes = {}
    for fixture in FIXTURES:
        relative = (
            "models/src/rust/rholang/canonical_path_admission.rs"
            if fixture == "act20k" else
            "models/src/rust/rholang/protobuf_admission.rs"
        )
        fixture_hashes[fixture] = base.sha(source / relative)
    return data, binaries, fixture_hashes


def bounded_reexec(args):
    if os.environ.get("PAGE_ARENA_SECONDARY_SCOPED") == "1":
        return
    cap = {"semantics": 1200, "samples": 1800, "profile": 600}[args.mode]
    argv = [
        "systemd-run", "--user", "--scope", "--collect",
        f"--working-directory={ROOT}",
        "-p", "MemoryMax=5G", "-p", "MemorySwapMax=0",
        "-p", "CPUQuota=100%", "-p", "TasksMax=256",
        "-p", f"RuntimeMaxSec={cap}",
        "--setenv=PAGE_ARENA_SECONDARY_SCOPED=1",
        "--setenv=DISPLAY=", "--setenv=WAYLAND_DISPLAY=",
        "--setenv=JAVA_TOOL_OPTIONS=-Djava.awt.headless=true",
        "--setenv=LC_ALL=C",
        sys.executable, str(Path(__file__).resolve()), *sys.argv[1:],
    ]
    os.execvp("systemd-run", argv)


def selected(binaries):
    return {
        arm: {
            fixture: attribution.selected_test(binary, attribution.FIXTURES[fixture][1])
            for fixture in FIXTURES
        }
        for arm, binary in binaries.items()
    }


def run_test(binary, name, cpu, deadline):
    return attribution.run_test(binary, name, cpu, deadline, 90)


def run_semantics(args):
    bounded_reexec(args)
    out = args.out.resolve()
    data, build, build_sha256 = primary_evidence(
        out, args.primary_build_sha256, args.schema_sha256,
    )
    attempt = base.new_output(out / "semantic-gates" / args.attempt)
    cpu = min(os.sched_getaffinity(0))
    seed = primary.config()["seeds"][0]
    base.write_json(attempt / "metadata.json", {
        "script_sha256": base.sha(Path(__file__)),
        "prepared_sha256": base.sha(out / "prepared.json"),
        "build_sha256": build_sha256,
        "source_manifest_sha256": data["manifest_sha256"],
        "generated_schema_sha256": args.schema_sha256,
        "seed": seed,
        "pinned_cpu": cpu,
        "deadline_seconds": 1170,
        "timing_verdict": "not_sampled",
    })
    deadline = time.monotonic() + 1170
    completed = []
    try:
        for capacity in primary.CAPACITIES:
            paired = {}
            for arm in ("strong", "candidate"):
                binary = Path(build["capacities"][str(capacity)][arm]["binaries"]["models"]["path"])
                selected = attribution.selected_test(binary, primary.TEST.split("::")[-1])
                os.environ["PAGE_ARENA_PRIMARY_SEED"] = seed
                os.environ["PAGE_ARENA_PRIMARY_CAPACITY"] = str(capacity)
                row = attribution.run_test(binary, selected, cpu, deadline, 180)
                parsed = primary.parse_record(row["stdout"], arm, seed, capacity)
                paired[arm] = {item["workload"]: item for item in parsed}
                base.append_row(attempt / "primary-semantics.jsonl", {
                    "capacity": capacity, "arm": arm, "selected_test": selected, **row,
                })
                completed.append({"capacity": capacity, "arm": arm, "kind": "primary"})
            for workload in primary.WORKLOADS:
                strong = paired["strong"][workload]
                candidate = paired["candidate"][workload]
                for key in ("trace_digest", "exports_dropped", "views_dropped",
                            "active_views", "branches"):
                    if strong[key] != candidate[key]:
                        raise ValueError(f"primary semantic trace mismatch at {capacity}/{workload}/{key}")
                base.append_row(attempt / "primary-cell-goldens.jsonl", {
                    "capacity": capacity, "workload": workload,
                    "trace_digest": strong["trace_digest"],
                })
        os.environ.pop("PAGE_ARENA_PRIMARY_SEED", None)
        os.environ.pop("PAGE_ARENA_PRIMARY_CAPACITY", None)
        for fixture, (kind, suffix) in attribution.FIXTURES.items():
            capacity = 64 if fixture == "isolated" else 256
            for arm in ("strong", "candidate"):
                binary = Path(build["capacities"][str(capacity)][arm]["binaries"][kind]["path"])
                selected = attribution.selected_test(binary, suffix)
                row = attribution.run_test(binary, selected, cpu, deadline, 90)
                base.append_row(attempt / "consumer-semantics.jsonl", {
                    "fixture": fixture, "capacity": capacity, "arm": arm,
                    "selected_test": selected, **row,
                })
                completed.append({"fixture": fixture, "capacity": capacity,
                                  "arm": arm, "kind": "consumer"})
        base.write_json(attempt / "summary.json", {
            "completed": completed,
            "primary_test_invocations": 8,
            "primary_cell_goldens": 16,
            "consumer_fixture_invocations": 14,
            "primary_semantics_sha256": base.sha(attempt / "primary-semantics.jsonl"),
            "cell_goldens_sha256": base.sha(attempt / "primary-cell-goldens.jsonl"),
            "consumer_semantics_sha256": base.sha(attempt / "consumer-semantics.jsonl"),
            "timing_verdict": "not_sampled",
            "full_models_parity": "open",
        })
    except Exception as error:
        base.write_json(attempt / "error.json", {"completed": completed, "error": repr(error)})
        raise
    finally:
        os.environ.pop("PAGE_ARENA_PRIMARY_SEED", None)
        os.environ.pop("PAGE_ARENA_PRIMARY_CAPACITY", None)
    print(attempt / "summary.json")


def green_parity(out, build_attempt, build_sha256, primary_build_sha256,
                 schema_sha256, attempt):
    build, names = full_models.guarded(
        out, build_attempt, build_sha256, primary_build_sha256, schema_sha256,
    )
    comparison = out / "full-models-parity" / build_attempt / "runs" / attempt / "comparison.json"
    parity = json.loads(comparison.read_text())
    expected_script_sha256 = {base.sha(FULL_MODELS)}
    if build_attempt == "p1" and build_sha256 == full_models.BUILD_SHA256:
        expected_script_sha256.add(full_models.LEGACY_SCRIPT_SHA256)
    if (
        parity["script_sha256"] not in expected_script_sha256
        or parity["build_sha256"] != build_sha256
        or parity["target_count"] != len(names)
        or len(parity["targets"]) != len(names)
        or {row["target"] for row in parity["targets"]} != names
        or parity["full_models_parity"] != "passed"
        or any(row["strong_exit_code"] != 0 or row["candidate_exit_code"] != 0
               or row["strong_timed_out"] or row["candidate_timed_out"]
               or row["strong_failed_tests"] or row["candidate_failed_tests"]
               for row in parity["targets"])
    ):
        raise ValueError("full models parity is not green on both matched arms")
    for arm in ("strong", "candidate"):
        run = out / "full-models-parity" / build_attempt / "runs" / parity[f"{arm}_attempt"]
        summary = json.loads((run / "summary.json").read_text())
        metadata = json.loads((run / "metadata.json").read_text())
        results = run / "results.jsonl"
        rows = [json.loads(line) for line in results.read_text().splitlines()]
        scope = json.loads((run / "scope-result.json").read_text())
        if (
            summary["arm"] != arm or summary["arm_parity"] != "passed"
            or summary["completed"] != len(names)
            or summary["expected"] != len(names)
            or summary["failed_targets"]
            or summary["results_sha256"] != base.sha(results)
            or metadata["script_sha256"] not in expected_script_sha256
            or metadata["build_sha256"] != build_sha256
            or metadata["seed"] != primary.config()["seeds"][0]
            or metadata["capacity"] != full_models.CAPACITY
            or metadata["target_names"] != sorted(names)
            or metadata["timeout_seconds_per_target"] != full_models.TARGET_TIMEOUT_SECONDS
            or scope["exit_code"] != 0
            or scope["scope_log_sha256"] != base.sha(run / "scope.log")
            or len(rows) != len(names)
            or {row["target"] for row in rows} != names
        ):
            raise ValueError(f"protected {arm} full models run changed")
        for row in rows:
            binary = build["arms"][arm]["executables"][row["target"]]["path"]
            if (
                row["arm"] != arm or row["exit_code"] != 0
                or row["command"] != [binary, "--test-threads=1"]
                or row.get("timed_out", False) or row["failed_tests"]
                or row["log_sha256"] != base.sha(run / f"{arm}-{row['target']}.log")
            ):
                raise ValueError(f"protected {arm}/{row['target']} test changed")
    binaries = {}
    for arm in ("strong", "candidate"):
        entry = build["arms"][arm]["executables"]["models"]
        binary = Path(entry["path"])
        if base.sha(binary) != entry["sha256"]:
            raise ValueError(f"protected {arm} timed library changed")
        binaries[arm] = binary
    return base.sha(comparison), binaries


def run_samples(args):
    bounded_reexec(args)
    out = args.out.resolve()
    data, _, fixtures = pinned(out, args.primary_build_sha256,
                               args.semantics_sha256, args.schema_sha256,
                               args.semantics_attempt)
    parity_sha, binaries = green_parity(
        out, args.full_models_build_attempt, args.full_models_build_sha256,
        args.primary_build_sha256, args.schema_sha256, args.parity_attempt,
    )
    attempt = base.new_output(out / "secondary-consumers" / args.attempt)
    deadline = time.monotonic() + 1740
    try:
        tests = selected(binaries)
        survey = attribution.mpstat("ALL", 30)
        (attempt / "cpu-survey.txt").write_text(survey)
        cpu = attribution.choose_cpu(survey)
        idle = {int(number): value for number, value in cpu["survey_idle"].items()}
        preflight = {
            "cpu": cpu,
            "swap_in_use": attribution.swap_in_use(),
            "load_average": os.getloadavg(),
            "survey_sha256": base.sha(attempt / "cpu-survey.txt"),
        }
        base.write_json(attempt / "preflight.json", preflight)
        if preflight["swap_in_use"] or any(value < 90 for value in idle.values()):
            raise ValueError("quiet-host preflight rejected: selected sibling busy or swap in use")
        base.write_json(attempt / "metadata.json", {
            "script_sha256": base.sha(Path(__file__)),
            "primary_build_sha256": args.primary_build_sha256,
            "generated_schema_sha256": args.schema_sha256,
            "full_models_build_sha256": args.full_models_build_sha256,
            "full_models_build_attempt": args.full_models_build_attempt,
            "prepared_sha256": base.sha(out / "prepared.json"),
            "source_manifest_sha256": data["manifest_sha256"],
            "semantic_gate_sha256": args.semantics_sha256,
            "semantic_gate_attempt": args.semantics_attempt,
            "full_models_comparison_sha256": parity_sha,
            "fixture_source_sha256": fixtures,
            "binary_sha256": {
                arm: base.sha(binary) for arm, binary in binaries.items()
            },
            "selected_tests": tests,
            "pinned_cpu": cpu["pinned_cpu"],
            "siblings": cpu["siblings"],
            "seed": 9334,
            "warmups_per_arm": 5,
            "pairs_per_fixture": 51,
            "deadline_seconds": 1740,
            "pair_noise_limit": 5,
        })
        for fixture in FIXTURES:
            for arm in ("strong", "candidate"):
                row = run_test(binaries[arm], tests[arm][fixture], cpu["pinned_cpu"], deadline)
                base.append_row(attempt / "semantic-repeat.jsonl", {
                    "fixture": fixture, "arm": arm, **row,
                })
        launch_pairs = []
        for index in range(10):
            order = ("strong", "candidate") if index % 2 == 0 else ("candidate", "strong")
            observed = {}
            for arm in order:
                row = run_test(binaries[arm], "__page_arena_9334_noop__", cpu["pinned_cpu"], deadline)
                base.append_row(attempt / "launch-only.jsonl", {
                    "index": index, "arm": arm, "order": order, **row,
                })
                observed[arm] = {"wall_ns": row["wall_ns"]}
            launch_pairs.append(observed)
        launch = attribution.summarize(launch_pairs, ("wall_ns",), 19334)
        summaries = {}
        for fixture_index, fixture in enumerate(FIXTURES):
            raw = attempt / f"{fixture}.jsonl"
            for index in range(5):
                order = ("strong", "candidate") if index % 2 == 0 else ("candidate", "strong")
                for arm in order:
                    row = run_test(binaries[arm], tests[arm][fixture], cpu["pinned_cpu"], deadline)
                    base.append_row(raw, {
                        "phase": "warmup", "index": index, "arm": arm, "order": order, **row,
                    })
            orders = [0] * 26 + [1] * 25
            random.Random(9334 + fixture_index).shuffle(orders)
            pairs = []
            noisy = 0
            for index, reverse in enumerate(orders):
                order = ("candidate", "strong") if reverse else ("strong", "candidate")
                survey = attribution.mpstat(",".join(map(str, cpu["siblings"])), 1)
                idles = attribution.average_idle(survey)
                if set(idles) != set(cpu["siblings"]):
                    raise ValueError("pair survey omitted a pinned CPU or SMT sibling")
                swapped = attribution.swap_in_use()
                busy = any(value < 90 for value in idles.values())
                noisy += int(swapped or busy)
                observed = {}
                for arm in order:
                    row = run_test(binaries[arm], tests[arm][fixture], cpu["pinned_cpu"], deadline)
                    base.append_row(raw, {
                        "phase": "sample", "index": index, "arm": arm, "order": order,
                        "preflight_idle": idles, "swap_in_use": swapped, **row,
                    })
                    observed[arm] = {
                        "wall_ns": row["wall_ns"],
                        "peak_rss_kib": row["resource"]["peak_rss_kib"],
                    }
                pairs.append(observed)
            metrics = attribution.summarize(pairs, ("wall_ns", "peak_rss_kib"), 10334 + fixture_index)
            launch_fraction = max(
                launch["wall_ns"]["strong_median"] / metrics["wall_ns"]["strong_median"],
                launch["wall_ns"]["candidate_median"] / metrics["wall_ns"]["candidate_median"],
            )
            guard = (
                "inconclusive_host_noise" if noisy > 5 else
                "inconclusive_launch_dominated" if launch_fraction > 0.5 else
                "descriptive_secondary_only"
            )
            summaries[fixture] = {
                "pairs": len(pairs), "busy_or_swap_pairs": noisy,
                "launch_fraction": launch_fraction, "guard": guard,
                "metrics": metrics, "raw_sha256": base.sha(raw),
            }
        base.write_json(attempt / "summary.json", {
            "fixtures": summaries,
            "launch_only": launch,
            "launch_raw_sha256": base.sha(attempt / "launch-only.jsonl"),
            "semantic_repeat_sha256": base.sha(attempt / "semantic-repeat.jsonl"),
            "full_models_parity": "separate_gate",
            "primary_experiment_395": "unchanged",
        })
    except Exception as error:
        base.write_json(attempt / "error.json", {"error": repr(error)})
        raise
    print(attempt / "summary.json")


def limit_file(size):
    resource.setrlimit(resource.RLIMIT_FSIZE, (size, size))


def run_profile(args):
    bounded_reexec(args)
    out = args.out.resolve()
    data, binaries, fixtures = pinned(out, args.primary_build_sha256,
                                     args.semantics_sha256, args.schema_sha256,
                                     args.semantics_attempt)
    if args.full_models_build_attempt is not None:
        build, _ = full_models.guarded(
            out, args.full_models_build_attempt, args.full_models_build_sha256,
            args.primary_build_sha256, args.schema_sha256,
        )
        binaries = {
            arm: Path(build["arms"][arm]["executables"]["models"]["path"])
            for arm in ("strong", "candidate")
        }
    attempt = base.new_output(out / "secondary-consumers" / args.attempt)
    try:
        for program in ("taskset", "heaptrack", "heaptrack_print") if args.profiler == "heaptrack" else ("taskset", "AMDuProfCLI"):
            if shutil.which(program) is None:
                raise ValueError(f"missing headless profiler command: {program}")
        tests = selected(binaries)
        cpu = min(os.sched_getaffinity(0))
        binary = binaries[args.arm]
        name = tests[args.arm][args.fixture]
        subject = [str(binary), "--exact", name, "--test-threads=1"]
        if args.profiler == "heaptrack":
            prefix = attempt / "capture.heaptrack"
            command = [
                "taskset", "-c", str(cpu), "heaptrack", "--record-only", "-o", str(prefix),
                *subject,
            ]
        else:
            prefix = attempt / "uprof"
            command = [
                "taskset", "-c", str(cpu), "AMDuProfCLI", "collect", "--config", "tbp",
                "--limit-data", "512M", "-o", str(prefix), *subject,
            ]
        base.write_json(attempt / "metadata.json", {
            "script_sha256": base.sha(Path(__file__)),
            "primary_build_sha256": args.primary_build_sha256,
            "generated_schema_sha256": args.schema_sha256,
            "full_models_build_sha256": (
                args.full_models_build_sha256
                if args.full_models_build_attempt is not None else None
            ),
            "full_models_build_attempt": args.full_models_build_attempt,
            "source_manifest_sha256": data["manifest_sha256"],
            "semantic_gate_sha256": args.semantics_sha256,
            "semantic_gate_attempt": args.semantics_attempt,
            "fixture_source_sha256": fixtures,
            "binary_sha256": base.sha(binary),
            "arm": args.arm, "fixture": args.fixture,
            "profiler": args.profiler, "selected_test": name,
            "pinned_cpu": cpu, "collector_command": command,
            "allocation_and_cpu_results": "single_execution_diagnostic",
        })
        env = os.environ.copy()
        env.update(LC_ALL="C", DISPLAY="", WAYLAND_DISPLAY="", JAVA_TOOL_OPTIONS="-Djava.awt.headless=true")
        env.pop("EPATHMAP_CANONICAL_GOLDENS_BLESS", None)
        with (attempt / "collector.log").open("w") as stream:
            result = subprocess.run(
                command, stdout=stream, stderr=subprocess.STDOUT, env=env,
                timeout=300, check=False,
                preexec_fn=lambda: limit_file(1024**3),
            )
        collector = attempt / "collector.log"
        if collector.stat().st_size > 1024**2:
            raise ValueError("profiler collector log exceeds 1 MiB")
        if (
            result.returncode != 0
            or "test result: ok. 1 passed; 0 failed" not in collector.read_text(errors="replace")
        ):
            raise RuntimeError(f"profiler target did not pass: collector exit {result.returncode}")
        if args.profiler == "heaptrack":
            captures = [path for path in attempt.glob("capture.heaptrack*") if path.is_file()]
            if len(captures) != 1:
                raise ValueError(f"expected one Heaptrack capture, found {captures}")
            capture = captures[0]
            report = ["heaptrack_print", "-f", str(capture)]
        else:
            sessions = [path for path in prefix.rglob("session.uprof") if path.is_file()]
            if len(sessions) != 1:
                raise ValueError(f"expected one AMD uProf session, found {sessions}")
            capture = sessions[0]
            report = ["AMDuProfCLI", "report", "-i", str(capture.parent), "--detail"]
        with (attempt / "report.txt").open("w") as stream:
            result = subprocess.run(
                report, stdout=stream, stderr=subprocess.STDOUT, env=env,
                timeout=240, check=False,
                preexec_fn=lambda: limit_file(128 * 1024**2),
            )
        if result.returncode != 0:
            raise RuntimeError(f"profiler report exit {result.returncode}")
        capture_files = [capture] if args.profiler == "heaptrack" else [
            path for path in prefix.rglob("*") if path.is_file()
        ]
        capture_bytes = sum(path.stat().st_size for path in capture_files)
        if capture_bytes > 1024**3:
            raise ValueError("profile capture exceeds 1 GiB archive limit")
        base.write_json(attempt / "summary.json", {
            "collector_log_sha256": base.sha(attempt / "collector.log"),
            "report_sha256": base.sha(attempt / "report.txt"),
            "capture_bytes": capture_bytes,
            "capture_files": {
                str(path.relative_to(attempt)): base.sha(path) for path in sorted(capture_files)
            },
            "report_command": report,
            "interpretation": "single profiled execution; no timing verdict",
        })
    except Exception as error:
        base.write_json(attempt / "error.json", {"error": repr(error)})
        raise
    print(attempt / "summary.json")


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="mode", required=True)
    for mode in ("semantics", "samples", "profile"):
        command = commands.add_parser(mode)
        command.add_argument("--out", type=Path, required=True)
        command.add_argument("--attempt", type=base.attempt_name, required=True)
        command.add_argument("--primary-build-sha256", default=BUILD_SHA256)
        command.add_argument("--schema-sha256", default=full_models.harness.SCHEMA_SHA256)
        if mode == "semantics":
            continue
        command.add_argument("--semantics-sha256", default=SEMANTICS_SHA256)
        command.add_argument("--semantics-attempt", type=base.attempt_name, default="g2")
        command.add_argument("--full-models-build-sha256", default=full_models.BUILD_SHA256)
        if mode == "profile":
            command.add_argument("--full-models-build-attempt", type=base.attempt_name)
            command.add_argument("--arm", choices=("strong", "candidate"), required=True)
            command.add_argument("--fixture", choices=FIXTURES, required=True)
            command.add_argument("--profiler", choices=("heaptrack", "uprof"), required=True)
        else:
            command.add_argument("--full-models-build-attempt", type=base.attempt_name,
                                 default="p1")
            command.add_argument("--parity-attempt", type=base.attempt_name, required=True)
    args = parser.parse_args()
    {"semantics": run_semantics, "samples": run_samples,
     "profile": run_profile}[args.mode](args)


if __name__ == "__main__":
    main()
