import argparse
import hashlib
import json
import os
import platform
import random
import re
import resource
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRATCH = ROOT / "target" / "scratch"
FIXTURES = {
    "act20k": (
        "canonical_path_admission.rs",
        "pure_reducer_is_linear_and_heap_total_at_twenty_thousand_levels",
    ),
    "protobuf_width4096": (
        "protobuf_admission.rs",
        "width_4096_validation_uses_counted_occurrences_on_a_small_native_stack",
    ),
    "nested_epathmap": (
        "protobuf_admission.rs",
        "epathmap_snapshot_and_every_nested_map_value_are_validated",
    ),
}
GOLDEN = "protobuf_goldens_epathmap_fixtures"
EXPECTED_GOLDEN_SHA256 = (
    "013fb65f709367cf65ac2e8bd0672f2324bf851a295f4777437c8be940908287"
)
EXPECTED_GOLDEN_PREFIX_SHA256 = (
    "ff0585ab7e98631b116bfd2782d70fc6495a063d21d6327d83609934c6b28d14"
)
EXPECTED_FIXTURE_MODULE_SHA256 = (
    "ad999250f2636f409607286381a0833918ead632ad7250a9fc1689b7764b8101"
)
EXPECTED_PROTOBUF_GOLDENS = {
    "e6a_index.protobuf.bin": "06a25adc40ecb9975a14fb3f064fa1a86948b91441ab85d3167823ea2125c7ad",
    "nested.protobuf.bin": "c9a72a0091a2d1ae94617301615085bfa15138697e7cf78a9d8fd29ec780f667",
    "locally_free.protobuf.bin": "3e699788a5fe904b493106715a236a62bf1099dcd7fa25f0015bcb9b8cc912a0",
    "remainder_connective.protobuf.bin": "cb151866c7fb378b7e3fdd12ceac7343019899af2263bfc9489aab6bac2ead1c",
}
EXPECTED_FIXTURE_SHA256 = {
    "act20k": "270cc26fd6e67a4bf7ca132021d9237ee96d6fe29de958cf15ebe821c66fb628",
    "protobuf_width4096": "3eda0b58390e64a08cefe1d8ced1632bdc5cfac7e35090221e5a9b3b8fc4595a",
    "nested_epathmap": "5fc9d059c701e9f8f694bdd78024bb9062ce95b0d7ac932adc72b8f9b160bd0d",
}
EXPECTED_CANDIDATE_SHA256 = {
    "models/src/rust/rholang/canonical_bytes_arena.rs": "ce493951d23332e60eda133cd72a0b26754799335d9811e56bc82cc334591438",
    "models/tests/support/canonical_source_cache.rs": "46bccdd89da525c0d6d63dc9ef4cc8d8d35b8ff21d31effa50e39ee57a468a66",
}
EXPECTED_CONTROL_ARENA_SHA256 = (
    "8baac501fa2468b049d0a100ca00000bc79f8e07f820211c303df8a82a95feb8"
)


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def fixture_text(root, fixture):
    filename, name = FIXTURES[fixture]
    source = (root / "models" / "src" / "rust" / "rholang" / filename).read_text()
    return function_text(source, name, root)


def function_text(source, name, location):
    marker = "fn " + name + "("
    if source.count(marker) != 1:
        raise ValueError(f"expected one {marker} in {location}")
    start = source.index(marker)
    opening = source.index("{", start)
    depth = 0
    for index in range(opening, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[start : index + 1]
    raise ValueError(f"unterminated {marker} in {location}")


def selected_test(binary, suffix):
    result = subprocess.run(
        [str(binary), "--list"], capture_output=True, text=True, timeout=45, check=True
    )
    names = [
        line.removesuffix(": test")
        for line in result.stdout.splitlines()
        if line.endswith(": test")
    ]
    matches = [name for name in names if name == suffix or name.endswith("::" + suffix)]
    if len(matches) != 1:
        raise ValueError(
            f"{binary}: expected one test ending in {suffix}, found {matches}"
        )
    return matches[0]


def host_snapshot(cpus):
    cpuinfo = Path("/proc/cpuinfo").read_text()
    model = next(
        (
            line.split(":", 1)[1].strip()
            for line in cpuinfo.splitlines()
            if line.startswith("model name")
        ),
        "unknown",
    )
    meminfo = {}
    for line in Path("/proc/meminfo").read_text().splitlines():
        key, _, value = line.partition(":")
        if key in ("SwapTotal", "SwapFree"):
            meminfo[key] = value.strip()
    governors = {}
    for cpu in sorted(expand_cpus(cpus)):
        path = Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor")
        governors[str(cpu)] = (
            path.read_text().strip() if path.exists() else "unavailable"
        )
    return {
        "uname": platform.uname()._asdict(),
        "model": model,
        "requested_cpus": cpus,
        "parent_affinity": sorted(os.sched_getaffinity(0)),
        "governors": governors,
        "swap": meminfo,
        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }


def pair_host_snapshot(cpus):
    meminfo = {}
    for line in Path("/proc/meminfo").read_text().splitlines():
        key, _, value = line.partition(":")
        if key in ("SwapTotal", "SwapFree"):
            meminfo[key] = int(value.strip().split()[0])
    cpu_list = ",".join(str(cpu) for cpu in sorted(expand_cpus(cpus)))
    env = os.environ.copy()
    env["LC_ALL"] = "C"
    result = subprocess.run(
        ["mpstat", "-P", cpu_list, "1", "1"],
        capture_output=True,
        text=True,
        timeout=10,
        check=True,
        env=env,
    )
    idles = []
    for line in result.stdout.splitlines():
        fields = line.split()
        if len(fields) >= 3 and fields[0] == "Average:" and fields[1].isdigit():
            idles.append(float(fields[-1]))
    if len(idles) != len(expand_cpus(cpus)):
        raise ValueError(f"mpstat did not report every pinned CPU: {result.stdout}")
    return {
        "load_average": os.getloadavg(),
        "swap_kib": meminfo,
        "host_swap_in_use": meminfo["SwapFree"] < meminfo["SwapTotal"],
        "selected_cpu_idle_percent": idles,
        "selected_cpu_busy_over_10pct": any(idle < 90.0 for idle in idles),
        "mpstat": result.stdout,
    }


def expand_cpus(text):
    cpus = set()
    for part in text.split(","):
        bounds = part.split("-", 1)
        if len(bounds) == 1:
            cpus.add(int(bounds[0]))
        else:
            cpus.update(range(int(bounds[0]), int(bounds[1]) + 1))
    if not cpus:
        raise ValueError("empty CPU affinity")
    return cpus


def require_program(name):
    if shutil.which(name) is None:
        raise ValueError(f"missing required program: {name}")


def capped_reexec(args):
    if os.environ.get("PAGE_ARENA_9334_CAPPED") == "1":
        return
    require_program("systemd-run")
    os.execvp(
        "systemd-run",
        [
            "systemd-run",
            "--user",
            "--wait",
            "--collect",
            "--pipe",
            f"--working-directory={ROOT}",
            "-p",
            f"MemoryMax={args.memory_max}",
            "-p",
            "MemorySwapMax=0",
            "-p",
            "TasksMax=256",
            "-p",
            f"RuntimeMaxSec={args.max_elapsed}",
            "--setenv=PAGE_ARENA_9334_CAPPED=1",
            "--setenv=DISPLAY=",
            "--setenv=WAYLAND_DISPLAY=",
            sys.executable,
            str(Path(__file__).resolve()),
            *sys.argv[1:],
        ],
    )


def command(args, binary, test_name):
    return [
        "taskset",
        "-c",
        args.cpus,
        str(binary),
        "--exact",
        test_name,
        "--test-threads=1",
    ]


def timed_run(args, binary, test_name, record):
    metric_file = args.out / "time-current.txt"
    metric_file.unlink(missing_ok=True)
    argv = [
        "/usr/bin/time",
        "-f",
        "%U\t%S\t%M",
        "-o",
        str(metric_file),
        *command(args, binary, test_name),
    ]
    started = time.perf_counter_ns()
    env = os.environ.copy()
    env.update(LC_ALL="C", DISPLAY="", WAYLAND_DISPLAY="")
    record["command"] = argv
    try:
        result = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=args.timeout,
            env=env,
            check=False,
        )
    except subprocess.TimeoutExpired as failure:
        record.update(
            wall_ns=time.perf_counter_ns() - started,
            timed_out=True,
            stdout=output_text(failure.stdout),
            stderr=output_text(failure.stderr),
        )
        metric_file.unlink(missing_ok=True)
        raise
    elapsed = time.perf_counter_ns() - started
    raw_metrics = metric_file.read_text().strip() if metric_file.exists() else ""
    metric_file.unlink(missing_ok=True)
    fields = raw_metrics.split("\t")
    record.update(
        wall_ns=elapsed,
        user_seconds=float(fields[0]) if len(fields) == 3 else None,
        system_seconds=float(fields[1]) if len(fields) == 3 else None,
        max_rss_kib=int(fields[2]) if len(fields) == 3 else None,
        returncode=result.returncode,
        stdout=result.stdout,
        stderr=result.stderr,
    )
    if result.returncode != 0 or not re.search(
        r"test result: ok\. 1 passed; 0 failed", result.stdout
    ):
        raise RuntimeError(f"test invocation failed: {json.dumps(record)}")
    return record


def output_text(value):
    if isinstance(value, bytes):
        return value.decode(errors="replace")
    return value or ""


def append_record(path, record):
    with path.open("a") as stream:
        stream.write(json.dumps(record, sort_keys=True) + "\n")
        stream.flush()


def test_metadata(args, arms, fixtures):
    selected = {}
    for arm, binary in arms.items():
        selected[arm] = {
            fixture: selected_test(binary, FIXTURES[fixture][1]) for fixture in fixtures
        }
    return selected


def fixed_fixture_sources(args, fixtures, check_control):
    result = {}
    for fixture in fixtures:
        candidate = fixture_text(ROOT, fixture)
        actual = hashlib.sha256(candidate.encode()).hexdigest()
        if check_control and fixture != "nested_epathmap":
            control = fixture_text(args.control_source_root, fixture)
            if control != candidate:
                raise ValueError(
                    f"matched fixture {fixture} differs between source trees"
                )
        if actual != EXPECTED_FIXTURE_SHA256[fixture]:
            raise ValueError(
                f"fixed fixture {fixture} changed from preregistered SHA256: {actual}"
            )
        result[fixture] = actual
    return result


def candidate_sources():
    actual = {}
    for relative, expected in EXPECTED_CANDIDATE_SHA256.items():
        value = digest(ROOT / relative)
        if value != expected:
            raise ValueError(f"candidate source changed: {relative} SHA256 {value}")
        actual[relative] = value
    return actual


def control_source(root):
    relative = "models/src/rust/rholang/canonical_bytes_arena.rs"
    actual = digest(root / relative)
    if actual != EXPECTED_CONTROL_ARENA_SHA256:
        raise ValueError(f"frozen control source changed: {relative} SHA256 {actual}")
    return actual


def prepare(args, fixtures, arms):
    args.out.mkdir(parents=True, exist_ok=False)
    selected = test_metadata(args, arms, fixtures)
    fixture_hashes = fixed_fixture_sources(args, fixtures, "control" in arms)
    metadata = {
        "mode": args.mode,
        "experiment": 395,
        "item": "rpb3-page-arena-cold-export-linearization",
        "host": host_snapshot(args.cpus),
        "binaries": {
            arm: {"path": str(path), "sha256": digest(path)}
            for arm, path in arms.items()
        },
        "candidate_source_sha256": candidate_sources(),
        "control_arena_source_sha256": control_source(args.control_source_root)
        if "control" in arms
        else None,
        "selected_tests": selected,
        "fixed_fixture_source_sha256": fixture_hashes,
        "control_source_root": str(args.control_source_root),
        "memory_max": args.memory_max,
        "memory_swap_max": "0",
        "timeout_seconds": args.timeout,
        "pairs": args.pairs if args.mode == "samples" else None,
        "warmups_per_arm": args.warmups if args.mode == "samples" else None,
        "seed": args.seed if args.mode == "samples" else None,
        "scope": "secondary consumer metrics; not experiment 395 primary verdict",
    }
    (args.out / "metadata.json").write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n"
    )
    if shutil.which("mpstat") is not None:
        with (args.out / "mpstat-preflight.txt").open("w") as stream:
            env = os.environ.copy()
            env["LC_ALL"] = "C"
            subprocess.run(
                ["mpstat", "-P", "ALL", "1", "1"],
                stdout=stream,
                stderr=subprocess.STDOUT,
                timeout=10,
                check=True,
                env=env,
            )
    return selected


def one_test(args, binary, test_name, record):
    argv = command(args, binary, test_name)
    env = os.environ.copy()
    env.update(LC_ALL="C", DISPLAY="", WAYLAND_DISPLAY="")
    record["command"] = argv
    try:
        result = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=args.timeout,
            env=env,
            check=False,
        )
    except subprocess.TimeoutExpired as failure:
        record.update(
            timed_out=True,
            stdout=output_text(failure.stdout),
            stderr=output_text(failure.stderr),
        )
        raise
    record.update(
        returncode=result.returncode,
        stdout=result.stdout,
        stderr=result.stderr,
    )
    if result.returncode != 0 or not re.search(
        r"test result: ok\. 1 passed; 0 failed", result.stdout
    ):
        raise RuntimeError(f"semantic test failed: {json.dumps(record)}")
    return record


def golden_sources(control_root):
    golden_relative = Path("models/tests/epathmap_canonical_fixtures.rs")
    control_golden_source = (control_root / golden_relative).read_text()
    candidate_golden_source = (ROOT / golden_relative).read_text()
    control_golden = function_text(control_golden_source, GOLDEN, control_root)
    candidate_golden = function_text(candidate_golden_source, GOLDEN, ROOT)
    if control_golden != candidate_golden:
        raise ValueError(
            "the frozen control and candidate protobuf golden fixtures differ"
        )
    if hashlib.sha256(candidate_golden.encode()).hexdigest() != EXPECTED_GOLDEN_SHA256:
        raise ValueError("protobuf golden fixture changed from the preregistered body")
    marker = "fn " + GOLDEN + "("
    control_prefix = control_golden_source[
        : control_golden_source.index(marker) + len(control_golden)
    ]
    candidate_prefix = candidate_golden_source[
        : candidate_golden_source.index(marker) + len(candidate_golden)
    ]
    if (
        control_prefix != candidate_prefix
        or hashlib.sha256(candidate_prefix.encode()).hexdigest()
        != EXPECTED_GOLDEN_PREFIX_SHA256
    ):
        raise ValueError("protobuf golden fixture helpers or pinned constants changed")
    fixtures_relative = Path("models/tests/fixtures/mod.rs")
    fixture_module = digest(ROOT / fixtures_relative)
    if (
        fixture_module != EXPECTED_FIXTURE_MODULE_SHA256
        or digest(control_root / fixtures_relative) != fixture_module
    ):
        raise ValueError("protobuf fixture-construction module changed")
    goldens = {}
    for name, expected in EXPECTED_PROTOBUF_GOLDENS.items():
        relative = Path("models/tests/fixtures/goldens") / name
        actual = digest(ROOT / relative)
        if actual != expected or digest(control_root / relative) != actual:
            raise ValueError(f"protobuf golden bytes changed: {name}")
        goldens[name] = actual
    return {
        "golden_fixture_source_sha256": hashlib.sha256(
            candidate_golden.encode()
        ).hexdigest(),
        "golden_source_prefix_sha256": EXPECTED_GOLDEN_PREFIX_SHA256,
        "fixture_module_sha256": fixture_module,
        "protobuf_golden_sha256": goldens,
    }


def semantics(args):
    args.out.mkdir(parents=True, exist_ok=False)
    fixture_hashes = fixed_fixture_sources(
        args, ("act20k", "protobuf_width4096", "nested_epathmap"), True
    )
    golden_hashes = golden_sources(args.control_source_root)
    metadata = {
        "mode": "semantics",
        "host": host_snapshot(args.cpus),
        "fixed_fixture_source_sha256": fixture_hashes,
        **golden_hashes,
        "candidate_source_sha256": candidate_sources(),
        "control_arena_source_sha256": control_source(args.control_source_root),
        "binaries": {
            "control_lib": {
                "path": str(args.control_bin),
                "sha256": digest(args.control_bin),
            },
            "candidate_lib": {
                "path": str(args.candidate_bin),
                "sha256": digest(args.candidate_bin),
            },
            "control_golden": {
                "path": str(args.control_golden_bin),
                "sha256": digest(args.control_golden_bin),
            },
            "candidate_golden": {
                "path": str(args.candidate_golden_bin),
                "sha256": digest(args.candidate_golden_bin),
            },
        },
        "scope": "golden parity and candidate-only nested EPathMap semantics; not matched nested timing",
    }
    (args.out / "metadata.json").write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n"
    )
    rows = []
    raw_path = args.out / "results.jsonl"
    for arm, binary in (
        ("control", args.control_golden_bin),
        ("candidate", args.candidate_golden_bin),
    ):
        selected = selected_test(binary, GOLDEN)
        record = {"arm": arm, "fixture": GOLDEN}
        try:
            one_test(args, binary, selected, record)
        finally:
            append_record(raw_path, record)
        rows.append(record)
    selected = selected_test(args.candidate_bin, FIXTURES["nested_epathmap"][1])
    record = {"arm": "candidate", "fixture": "nested_epathmap"}
    try:
        one_test(args, args.candidate_bin, selected, record)
    finally:
        append_record(raw_path, record)
    rows.append(record)
    (args.out / "results.json").write_text(
        json.dumps(rows, indent=2, sort_keys=True) + "\n"
    )


def sample_fixture(args, fixture, arms, tests):
    path = args.out / f"{fixture}-raw.jsonl"
    for warmup in range(args.warmups):
        order = (
            ("control", "candidate") if warmup % 2 == 0 else ("candidate", "control")
        )
        for arm in order:
            record = {
                "fixture": fixture,
                "phase": "warmup",
                "index": warmup,
                "arm": arm,
            }
            try:
                timed_run(args, arms[arm], tests[arm][fixture], record)
            finally:
                append_record(path, record)
    orders = [0] * ((args.pairs + 1) // 2) + [1] * (args.pairs // 2)
    random.Random(args.seed + list(FIXTURES).index(fixture)).shuffle(orders)
    paired = []
    host_noise = False
    for index, reverse in enumerate(orders):
        order = ("candidate", "control") if reverse else ("control", "candidate")
        host = pair_host_snapshot(args.cpus)
        host_noise |= host["host_swap_in_use"] or host["selected_cpu_busy_over_10pct"]
        pair = {}
        for arm in order:
            record = {
                "fixture": fixture,
                "phase": "sample",
                "index": index,
                "arm": arm,
                "order": list(order),
                "prepair_host": host,
            }
            try:
                timed_run(args, arms[arm], tests[arm][fixture], record)
            finally:
                append_record(path, record)
            pair[arm] = record["wall_ns"]
        paired.append(pair["candidate"] / pair["control"])
    rng = random.Random(args.seed + 1000 + list(FIXTURES).index(fixture))
    boot = sorted(
        statistics.median(rng.choices(paired, k=len(paired))) for _ in range(10_000)
    )
    summary = {
        "fixture": fixture,
        "pairs": len(paired),
        "candidate_over_control_median_pair_wall_ratio": statistics.median(paired),
        "paired_bootstrap_95pct_interval": [boot[249], boot[9749]],
        "bootstrap_resamples": len(boot),
        "host_swap_or_selected_cpu_busy_seen": host_noise,
        "host_noise_review_required": True,
        "interpretation": "secondary timing only; lower ratio is faster; no critical-regression verdict is inferred",
    }
    (args.out / f"{fixture}-summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n"
    )


def launch_cost(args, arms):
    path = args.out / "launch-only-raw.jsonl"
    for index in range(10):
        for arm, binary in arms.items():
            argv = [
                "taskset",
                "-c",
                args.cpus,
                str(binary),
                "--list",
                "--exact",
                "__page_arena_9334_no_such_test__",
            ]
            started = time.perf_counter_ns()
            result = subprocess.run(
                argv, capture_output=True, text=True, timeout=args.timeout, check=False
            )
            record = {
                "phase": "launch_only",
                "index": index,
                "arm": arm,
                "command": argv,
                "wall_ns": time.perf_counter_ns() - started,
                "returncode": result.returncode,
                "stdout": result.stdout,
                "stderr": result.stderr,
            }
            append_record(path, record)
            if result.returncode != 0:
                raise RuntimeError(
                    f"launch-only invocation failed: {json.dumps(record)}"
                )


def run_profile(args, binary, test_name):
    env = os.environ.copy()
    env.update(DISPLAY="", WAYLAND_DISPLAY="")
    subject = f"{args.arm}-{args.fixture}"
    argv = [str(binary), "--exact", test_name, "--test-threads=1"]
    if args.mode == "heaptrack":
        require_program("heaptrack")
        require_program("heaptrack_print")
        prefix = args.out / f"{subject}.heaptrack"
        run = [
            "taskset",
            "-c",
            args.cpus,
            "heaptrack",
            "--record-only",
            "-o",
            str(prefix),
            *argv,
        ]
    else:
        require_program("AMDuProfCLI")
        prefix = args.out / f"{subject}-uprof"
        run = [
            "taskset",
            "-c",
            args.cpus,
            "AMDuProfCLI",
            "collect",
            "--config",
            "tbp",
            "--limit-data",
            "512M",
            "-o",
            str(prefix),
            *argv,
        ]
    result = subprocess.run(
        run,
        capture_output=True,
        text=True,
        timeout=args.timeout,
        env=env,
        preexec_fn=limit_profile_file,
        check=False,
    )
    (args.out / "collector.json").write_text(
        json.dumps(
            {
                "command": run,
                "returncode": result.returncode,
                "stdout": result.stdout,
                "stderr": result.stderr,
            },
            indent=2,
        )
        + "\n"
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"{args.mode} collection failed with status {result.returncode}"
        )
    if args.mode == "heaptrack":
        captures = sorted(
            path for path in args.out.glob(prefix.name + "*") if path.is_file()
        )
        if len(captures) != 1:
            raise ValueError(f"expected one heaptrack capture, found {captures}")
        report = ["heaptrack_print", "-f", str(captures[0])]
    else:
        if (prefix / "session.uprof").is_file():
            session = prefix
        else:
            sessions = sorted(
                path for path in prefix.glob("*") if (path / "session.uprof").is_file()
            )
            if len(sessions) != 1:
                raise ValueError(f"expected one AMD uProf session, found {sessions}")
            session = sessions[0]
        report = ["AMDuProfCLI", "report", "-i", str(session), "--detail"]
    with (args.out / "report.txt").open("w") as stream:
        subprocess.run(
            report,
            stdout=stream,
            stderr=subprocess.STDOUT,
            timeout=args.timeout,
            env=env,
            check=True,
            preexec_fn=limit_report_file,
        )
    (args.out / "report-command.json").write_text(json.dumps(report) + "\n")


def limit_profile_file():
    resource.setrlimit(resource.RLIMIT_FSIZE, (1024 * 1024 * 1024, 1024 * 1024 * 1024))


def limit_report_file():
    resource.setrlimit(resource.RLIMIT_FSIZE, (128 * 1024 * 1024, 128 * 1024 * 1024))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("semantics", "samples", "heaptrack", "uprof"))
    parser.add_argument("--control-bin", type=Path)
    parser.add_argument("--candidate-bin", type=Path, required=True)
    parser.add_argument("--control-golden-bin", type=Path)
    parser.add_argument("--candidate-golden-bin", type=Path)
    parser.add_argument(
        "--control-source-root",
        type=Path,
        default=ROOT / "target" / "scratch" / "page-arena-control-11c92e7",
    )
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--fixture", choices=tuple(FIXTURES), default="act20k")
    parser.add_argument("--arm", choices=("control", "candidate"), default="candidate")
    parser.add_argument("--cpus", default="0-7")
    parser.add_argument("--memory-max", default="5G")
    parser.add_argument("--max-elapsed", type=int, default=1800)
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--pairs", type=int, default=51)
    parser.add_argument("--warmups", type=int, default=5)
    parser.add_argument("--seed", type=int, default=9334)
    args = parser.parse_args()
    os.environ["DISPLAY"] = ""
    os.environ["WAYLAND_DISPLAY"] = ""
    if os.environ.get("EPATHMAP_CANONICAL_GOLDENS_BLESS") is not None:
        parser.error(
            "EPATHMAP_CANONICAL_GOLDENS_BLESS must be unset; goldens are read-only"
        )
    args.out = args.out.resolve()
    if not args.out.is_relative_to(SCRATCH.resolve()):
        parser.error(f"--out must be below {SCRATCH}")
    if args.out.exists():
        parser.error(
            "--out already exists; preserve each run instead of overwriting it"
        )
    if (
        args.timeout <= 0
        or not 0 < args.max_elapsed <= 3600
        or args.pairs < 51
        or args.warmups < 5
    ):
        parser.error(
            "timeout must be positive, overall cap at most one hour, samples at least 51 pairs and five warmups per arm"
        )
    if args.mode in ("semantics", "samples") and args.control_bin is None:
        parser.error("semantics and samples require --control-bin")
    if args.mode == "semantics" and (
        args.control_golden_bin is None or args.candidate_golden_bin is None
    ):
        parser.error("semantics require both golden integration test binaries")
    if args.arm == "control" and args.control_bin is None:
        parser.error("control profile requires --control-bin")
    if (
        args.fixture == "nested_epathmap"
        and args.mode in ("heaptrack", "uprof")
        and args.arm != "candidate"
    ):
        parser.error("nested EPathMap is candidate-only; the control fixture differs")
    for name in ("taskset", "systemd-run"):
        require_program(name)
    args.candidate_bin = args.candidate_bin.resolve(strict=True)
    if args.control_bin is not None:
        args.control_bin = args.control_bin.resolve(strict=True)
    if args.control_golden_bin is not None:
        args.control_golden_bin = args.control_golden_bin.resolve(strict=True)
    if args.candidate_golden_bin is not None:
        args.candidate_golden_bin = args.candidate_golden_bin.resolve(strict=True)
    for binary in (
        args.control_bin,
        args.candidate_bin,
        args.control_golden_bin,
        args.candidate_golden_bin,
    ):
        if binary is not None and "release" not in binary.parts:
            parser.error(
                f"consumer binaries must come from the release profile: {binary}"
            )
    args.control_source_root = (
        args.control_source_root.resolve(strict=True)
        if args.mode in ("semantics", "samples")
        else args.control_source_root
    )
    capped_reexec(args)
    try:
        if args.mode == "semantics":
            semantics(args)
            return
        arms = {"candidate": args.candidate_bin}
        if args.control_bin is not None:
            arms["control"] = args.control_bin
        fixtures = (
            ("act20k", "protobuf_width4096")
            if args.mode == "samples"
            else (args.fixture,)
        )
        selected = prepare(args, fixtures, arms)
        if args.mode == "samples":
            require_program("/usr/bin/time")
            require_program("mpstat")
            for fixture in fixtures:
                sample_fixture(args, fixture, arms, selected)
            launch_cost(args, arms)
        else:
            run_profile(args, arms[args.arm], selected[args.arm][args.fixture])
    except Exception as failure:
        if args.out.is_dir():
            (args.out / "error.json").write_text(
                json.dumps(
                    {
                        "type": type(failure).__name__,
                        "message": str(failure),
                        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    },
                    indent=2,
                    sort_keys=True,
                )
                + "\n"
            )
        raise


if __name__ == "__main__":
    main()
