import argparse
import difflib
import hashlib
import importlib.util
import json
import math
import os
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRATCH = ROOT / "target" / "scratch"
HERE = Path(__file__).resolve().parent
PLAN = HERE / "fixtures/page-arena-primary-395-plan.json"
FIXTURE = HERE / "fixtures/page-arena-primary-395.rs"
STRONG_INSTRUMENT = HERE / "fixtures/page-arena-primary-395-strong-instrument.rs"
CANDIDATE_INSTRUMENT = HERE / "fixtures/page-arena-primary-395-candidate-instrument.rs"
ATTRIBUTION = HERE / "page-arena-attribution-9334.py"
QUIET_HOST = HERE / "page-arena-quiet-host-395.py"
TEST = "rust::rholang::canonical_bytes_arena::source_cache_tests::page_arena_primary_395_seed_capacity"
RECORD = "PAGE_ARENA_PRIMARY_395_V1\t"
CAPACITIES = (32, 64, 128, 256)
WORKLOADS = ("linear", "boundary_forked", "export_drop_append", "active_retained_view")
PAYLOAD = 65_536
RUN_LIMIT_SECONDS = 7_200
CHILD_LIMIT_SECONDS = 240
PINNED_INPUT_SHA256 = {
    "plan_sha256": "42e82ad81c0492df282f3e5e3eabf4e8273fb5ecce62a56e9dfbc20d3460e5dd",
    "fixture_sha256": "4f229b3b1515f733b8f4dddd7ee734b2db8548df35e933907a89b90215c5727d",
    "strong_instrument_sha256": "3bf9a55946770518ca744dc105163bfb2582da5e5bf5341fc8e0e9652e886771",
    "candidate_instrument_sha256": "c5d0dd851090357a4c6f64a1d99e88f65dc1cdcbd5c969ed3769a1007c60d452",
    "attribution_sha256": "99a87cc885ae818116a60ea90e538fd0c9541fb8d3610133f7b401a79dd4f220",
    "quiet_host_sha256": "8f25738ca93bdb4ab4b929b2708cc3e4ed013f0705777d25d32ff629dc452bf0",
}

for trusted_path, trusted_name in (
    (ATTRIBUTION, "attribution_sha256"),
    (QUIET_HOST, "quiet_host_sha256"),
):
    if (
        hashlib.sha256(trusted_path.read_bytes()).hexdigest()
        != PINNED_INPUT_SHA256[trusted_name]
    ):
        raise RuntimeError(f"preregistered imported helper changed: {trusted_path}")

spec = importlib.util.spec_from_file_location(
    "page_arena_attribution_9334", ATTRIBUTION
)
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
quiet_spec = importlib.util.spec_from_file_location(
    "page_arena_quiet_host_395", QUIET_HOST
)
quiet = importlib.util.module_from_spec(quiet_spec)
quiet_spec.loader.exec_module(quiet)


def sha(path):
    return base.sha(path)


def write_json(path, value):
    base.write_json(path, value)


def config():
    plan = json.loads(PLAN.read_text())
    seeds = plan["seeds"]
    if (
        plan["experiment_id"] != 395
        or plan["hypothesis_id"] != 395
        or plan["primary_metric"]
        != "sealed_page_pairs_after_dropped_export_per_65536_payload_bytes"
        or plan["payload_bytes_per_cell"] != PAYLOAD
        or tuple(plan["capacities"]) != CAPACITIES
        or tuple(plan["workloads"]) != WORKLOADS
        or len(seeds) != 30
        or len(set(seeds)) != 30
        or any(not re.fullmatch(r"0x[0-9a-f]{16}", seed) for seed in seeds)
        or plan["observation"]
        != "arithmetic_mean_of_16_capacity_by_workload_sealed_page_pair_counts_per_seed"
        or plan["analysis"]
        != {
            "test": "welch_t",
            "tail": "less",
            "alpha": 0.05,
            "effect": "candidate_minus_control_cohens_d_at_most_negative_0.5",
            "multiple_testing": "benjamini_hochberg_one_primary_hypothesis_identity",
        }
        or plan["no_post_hoc_exclusions"] is not True
    ):
        raise ValueError("primary plan shape or locked criterion changed")
    return plan


def source_hashes():
    result = {
        "script_sha256": sha(Path(__file__)),
        "plan_sha256": sha(PLAN),
        "fixture_sha256": sha(FIXTURE),
        "strong_instrument_sha256": sha(STRONG_INSTRUMENT),
        "candidate_instrument_sha256": sha(CANDIDATE_INSTRUMENT),
        "attribution_sha256": sha(ATTRIBUTION),
        "quiet_host_sha256": sha(QUIET_HOST),
    }
    for name, expected in PINNED_INPUT_SHA256.items():
        if result[name] != expected:
            raise ValueError(f"preregistered primary input changed: {name}")
    return result


def projection():
    original = (ROOT / base.PROJECTION).read_text()
    insertion = (
        '\ninclude!(concat!(env!("CARGO_MANIFEST_DIR"), '
        '"/../scripts/bench/fixtures/page-arena-primary-395.rs"));\n'
    )
    if insertion in original:
        raise ValueError("primary fixture is already in the common projection")
    return original + insertion


def variant(source, capacity, instrument):
    old = "const DEFAULT_PAGE_BYTES: usize = 64;"
    if source.count(old) != 1:
        raise ValueError("arena page-capacity constant did not match the pinned source")
    return (
        source.replace(old, f"const DEFAULT_PAGE_BYTES: usize = {capacity};", 1)
        + "\n"
        + instrument
    )


def prepare(args):
    config()
    source_hashes()
    out = args.out.resolve()
    base.prepare(argparse.Namespace(out=out))
    base_data = json.loads((out / "prepared.json").read_text())
    base_manifest = json.loads((out / "source-manifest.json").read_text())
    (out / "prepared.json").rename(out / "attribution-base-prepared.json")
    (out / "source-manifest.json").rename(out / "attribution-base-source-manifest.json")
    snapshot = out / "source"
    projected = projection()
    (snapshot / base.PROPERTY).write_text(projected)
    expected = dict(base_manifest)
    expected[str(base.PROPERTY)] = sha(snapshot / base.PROPERTY)
    if base.manifest(snapshot) != expected:
        raise ValueError("primary projection changed another snapshot source file")
    (out / "primary-projection.diff").write_text(
        "".join(
            difflib.unified_diff(
                (ROOT / base.PROPERTY).read_text().splitlines(keepends=True),
                projected.splitlines(keepends=True),
                fromfile="live-candidate-source-cache.rs",
                tofile="common-primary-source-cache.rs",
            )
        )
    )
    candidate = (out / "candidate-arena.rs").read_text()
    strong = (out / "strong-compat-arena.rs").read_text()
    variants = {}
    for capacity in CAPACITIES:
        variants[str(capacity)] = {}
        for arm, original, instrument in (
            ("strong", strong, STRONG_INSTRUMENT.read_text()),
            ("candidate", candidate, CANDIDATE_INSTRUMENT.read_text()),
        ):
            target = out / "variants" / str(capacity) / f"{arm}.rs"
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(variant(original, capacity, instrument))
            diff = target.with_suffix(".diff")
            diff.write_text(
                "".join(
                    difflib.unified_diff(
                        original.splitlines(keepends=True),
                        target.read_text().splitlines(keepends=True),
                        fromfile=f"{arm}-base-arena.rs",
                        tofile=f"{arm}-{capacity}-instrumented-arena.rs",
                    )
                )
            )
            variants[str(capacity)][arm] = {
                "source": str(target),
                "source_sha256": sha(target),
                "diff_sha256": sha(diff),
            }
    write_json(out / "source-manifest.json", expected)
    write_json(
        out / "prepared.json",
        {
            **source_hashes(),
            "base_prepared_sha256": sha(out / "attribution-base-prepared.json"),
            "base_manifest_sha256": sha(out / "attribution-base-source-manifest.json"),
            "manifest_sha256": sha(out / "source-manifest.json"),
            "projection_diff_sha256": sha(out / "primary-projection.diff"),
            "projected_property_sha256": sha(snapshot / base.PROPERTY),
            "patched_dependency_manifest_sha256": base_data[
                "patched_dependency_manifest_sha256"
            ],
            "candidate_base_arena_sha256": base_data["candidate_arena_sha256"],
            "strong_base_arena_sha256": base_data["strong_compat_arena_sha256"],
            "variants": variants,
        },
    )
    print(out / "prepared.json")


def prepared(out):
    config()
    data = json.loads((out / "prepared.json").read_text())
    for name, digest in source_hashes().items():
        if data[name] != digest:
            raise ValueError(f"primary source pin changed: {name}")
    if sha(out / "attribution-base-prepared.json") != data["base_prepared_sha256"]:
        raise ValueError("attribution base preparation changed")
    if (
        sha(out / "attribution-base-source-manifest.json")
        != data["base_manifest_sha256"]
    ):
        raise ValueError("attribution base manifest changed")
    if sha(out / "source-manifest.json") != data["manifest_sha256"]:
        raise ValueError("primary source manifest changed")
    if sha(out / "primary-projection.diff") != data["projection_diff_sha256"]:
        raise ValueError("primary projection diff changed")
    expected = json.loads((out / "source-manifest.json").read_text())
    if sha(out / "source" / base.PROPERTY) != data["projected_property_sha256"]:
        raise ValueError("common primary projection changed")
    if sha(out / "candidate-arena.rs") != data["candidate_base_arena_sha256"]:
        raise ValueError("candidate base arena changed")
    if sha(out / "strong-compat-arena.rs") != data["strong_base_arena_sha256"]:
        raise ValueError("strong base arena changed")
    base.check_non_arena(out / "source", expected)
    base.check_patched_dependency(out, data)
    for capacity in CAPACITIES:
        for arm in ("strong", "candidate"):
            item = data["variants"][str(capacity)][arm]
            path = Path(item["source"])
            if not path.is_relative_to(out / "variants"):
                raise ValueError("variant source escaped the primary attempt")
            if sha(path) != item["source_sha256"]:
                raise ValueError(f"{arm} {capacity} arena variant changed")
            if sha(path.with_suffix(".diff")) != item["diff_sha256"]:
                raise ValueError(f"{arm} {capacity} arena diff changed")
    return data, expected


def build(args):
    out = args.out.resolve()
    data, expected = prepared(out)
    disk_free = shutil.disk_usage(ROOT).free
    if disk_free < 25 * 1024**3:
        raise ValueError(f"less than 25 GiB free before primary build: {disk_free}")
    attempt = base.new_output(out / "builds" / args.attempt)
    arena = out / "source" / base.ARENA
    try:
        private_target, release_bytes = base.private_release_target(attempt, disk_free)
        result = {
            "prepared_sha256": sha(out / "prepared.json"),
            "manifest_sha256": data["manifest_sha256"],
            "plan_sha256": data["plan_sha256"],
            "fixture_sha256": data["fixture_sha256"],
            "script_sha256": data["script_sha256"],
            "target_clone_metadata_sha256": sha(attempt / "target-clone-metadata.json"),
            "quarantine_sha256": sha(attempt / "quarantine.json"),
            "private_target_source_bytes": release_bytes,
            "disk_free_before_bytes": disk_free,
            "cargo_version": subprocess.check_output(
                ["cargo", "--version"], text=True
            ).strip(),
            "rustc_version": subprocess.check_output(
                ["rustc", "--version"], text=True
            ).strip(),
            "rustflags": os.environ.get("RUSTFLAGS", ""),
            "capacities": {},
        }
        schema = None
        for capacity in CAPACITIES:
            cell = attempt / str(capacity)
            cell.mkdir()
            result["capacities"][str(capacity)] = {}
            for arm in ("strong", "candidate"):
                item = data["variants"][str(capacity)][arm]
                artifact = base.build_one(
                    out,
                    cell,
                    arm,
                    Path(item["source"]),
                    expected,
                    data,
                    private_target,
                    schema,
                )
                schema = artifact["generated_schema_sha256"]
                artifact["variant_source_sha256"] = item["source_sha256"]
                result["capacities"][str(capacity)][arm] = artifact
                write_json(attempt / "build-progress.json", result)
        write_json(attempt / "build.json", result)
    except Exception as error:
        write_json(attempt / "build-error.json", {"error": repr(error)})
        raise
    finally:
        shutil.copyfile(out / "candidate-arena.rs", arena)
        base.check_non_arena(out / "source", expected)
    print(attempt / "build.json")


def built(out, attempt, data):
    path = out / "builds" / attempt / "build.json"
    result = json.loads(path.read_text())
    if (
        result["prepared_sha256"] != sha(out / "prepared.json")
        or result["manifest_sha256"] != data["manifest_sha256"]
        or result["plan_sha256"] != data["plan_sha256"]
        or result["fixture_sha256"] != data["fixture_sha256"]
        or result["script_sha256"] != data["script_sha256"]
        or set(result["capacities"]) != {str(capacity) for capacity in CAPACITIES}
    ):
        raise ValueError("primary build record does not match its source preparation")
    for capacity in CAPACITIES:
        for arm in ("strong", "candidate"):
            item = result["capacities"][str(capacity)][arm]
            if (
                item["variant_source_sha256"]
                != data["variants"][str(capacity)][arm]["source_sha256"]
            ):
                raise ValueError("primary arena variant hash mismatch")
            if (
                sha(Path(item["binaries"]["models"]["path"]))
                != item["binaries"]["models"]["sha256"]
            ):
                raise ValueError(f"protected {arm} {capacity} models binary changed")
            if (
                sha(Path(item["binaries"]["golden"]["path"]))
                != item["binaries"]["golden"]["sha256"]
            ):
                raise ValueError(f"protected {arm} {capacity} golden binary changed")
            if (
                sha(out / "builds" / attempt / str(capacity) / f"{arm}-build.log")
                != item["build_log_sha256"]
            ):
                raise ValueError(f"protected {arm} {capacity} build log changed")
    return result, sha(path)


def parse_record(output, arm, seed, capacity):
    rows = []
    for line in output.splitlines():
        if RECORD not in line:
            continue
        payload = line.split(RECORD, 1)[1]
        fields = dict(field.split("=", 1) for field in payload.split("\t"))
        if (
            fields["seed"] != seed
            or int(fields["capacity"]) != capacity
            or int(fields["payload_bytes"]) != PAYLOAD
        ):
            raise ValueError("primary fixture reported a different trace identity")
        counts = {
            key: int(fields[key])
            for key in (
                "sealed_page_pairs",
                "physical_page_pairs",
                "cache_only_source_nodes",
                "built_source_nodes",
                "exports_dropped",
                "views_dropped",
                "active_views",
                "branches",
            )
        }
        if any(value < 0 for value in counts.values()):
            raise ValueError("primary fixture reported a negative count")
        if counts["sealed_page_pairs"] > counts["physical_page_pairs"]:
            raise ValueError("more sealed page pairs than physical page pairs")
        if counts["exports_dropped"] == 0:
            raise ValueError("primary fixture did not drop any exported view")
        workload = fields["workload"]
        if workload == "active_retained_view":
            if counts["active_views"] != 2 or counts["views_dropped"] != 6:
                raise ValueError(
                    "active-view lifetime did not match the frozen fixture"
                )
        elif counts["active_views"] != 0 or counts["views_dropped"] != 0:
            raise ValueError("unexpected active view in a dropped-view workload")
        if (workload == "boundary_forked") != (counts["branches"] > 0):
            raise ValueError("boundary-fork shape did not match the frozen fixture")
        if not re.fullmatch(r"[0-9a-f]{16}", fields["trace_digest"]):
            raise ValueError("invalid primary trace digest")
        rows.append(
            {
                "arm": arm,
                "seed": seed,
                "capacity": capacity,
                "workload": workload,
                "payload_bytes": PAYLOAD,
                "trace_digest": fields["trace_digest"],
                **counts,
            }
        )
    if len(rows) != 4 or {row["workload"] for row in rows} != set(WORKLOADS):
        raise ValueError(
            "one seed/capacity invocation did not report all four workloads"
        )
    return rows


def quiet_preflight(path):
    record = json.loads((path / "preflight.json").read_text())
    if record["script_sha256"] != sha(QUIET_HOST) or not record["eligible"]:
        raise ValueError("quiet-host preflight is not eligible or its script changed")
    if record["swap_used_kib_before"] or record["swap_used_kib_after"]:
        raise ValueError("quiet-host preflight observed swap")
    if sha(path / "mpstat.txt") != record["mpstat_sha256"]:
        raise ValueError("quiet-host survey changed")
    if time.time() - path.joinpath("preflight.json").stat().st_mtime > 120:
        raise ValueError("quiet-host preflight is older than two minutes")
    return record


def sample_inner(args):
    if os.environ.get("PAGE_ARENA_PRIMARY_SCOPED") != "1":
        raise ValueError("primary sampling requires its bounded headless user scope")
    out = args.out.resolve()
    data, _ = prepared(out)
    build, build_sha = built(out, args.build_attempt, data)
    sample = args.sample.resolve()
    expected = out / "samples" / args.attempt
    if sample != expected or not sample.is_dir():
        raise ValueError("sample output is not the fresh prepared attempt")
    preflight_path = sample / "preflight"
    survey = subprocess.run(
        [sys.executable, str(QUIET_HOST), "--out", str(preflight_path)],
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    (sample / "preflight-command.log").write_text(survey.stdout + survey.stderr)
    if survey.returncode != 0:
        raise RuntimeError(f"quiet-host admission failed with exit {survey.returncode}")
    preflight = quiet_preflight(preflight_path)
    cpu = preflight["core"]["pinned_cpu"]
    plan = config()
    metadata = {
        **source_hashes(),
        "prepared_sha256": sha(out / "prepared.json"),
        "build_sha256": build_sha,
        "preflight_sha256": sha(preflight_path / "preflight.json"),
        "pinned_cpu": cpu,
        "sibling_cpus": preflight["core"]["siblings"],
        "test": TEST,
        "planned_invocations": 30 * 4 * 2,
        "planned_cell_rows": 30 * 4 * 4 * 2,
        "started_epoch_seconds": time.time(),
    }
    write_json(sample / "metadata.json", metadata)
    env = os.environ.copy()
    env.update(
        LC_ALL="C",
        DISPLAY="",
        WAYLAND_DISPLAY="",
        JAVA_TOOL_OPTIONS="-Djava.awt.headless=true",
    )
    deadline = time.monotonic() + RUN_LIMIT_SECONDS - 30
    seen = {}
    try:
        with (sample / "raw.jsonl").open("w") as raw:
            for seed_index, seed in enumerate(plan["seeds"]):
                sibling_arg = ",".join(
                    str(cpu) for cpu in preflight["core"]["siblings"]
                )
                host = subprocess.run(
                    ["mpstat", "-P", sibling_arg, "1", "1"],
                    env=env,
                    capture_output=True,
                    text=True,
                    timeout=8,
                    check=False,
                )
                host_log = sample / f"host-seed{seed_index:02d}.txt"
                host_log.write_text(host.stdout + "\nSTDERR\n" + host.stderr)
                idles = quiet.average_idle(host.stdout)
                if host.returncode != 0 or any(
                    cpu not in idles for cpu in preflight["core"]["siblings"]
                ):
                    raise RuntimeError(f"seed host survey failed: {host_log}")
                host_row = {
                    "seed_index": seed_index,
                    "seed": seed,
                    "sibling_idle_percent": {
                        str(cpu): idles[cpu] for cpu in preflight["core"]["siblings"]
                    },
                    "swap_used_kib": quiet.swap_used_kib(),
                    "log_sha256": sha(host_log),
                }
                host_row["noisy"] = bool(
                    host_row["swap_used_kib"]
                    or any(
                        value < 90
                        for value in host_row["sibling_idle_percent"].values()
                    )
                )
                with (sample / "host.jsonl").open("a") as host_stream:
                    host_stream.write(json.dumps(host_row, sort_keys=True) + "\n")
                for capacity_index, capacity in enumerate(CAPACITIES):
                    order = (
                        ("strong", "candidate")
                        if (seed_index + capacity_index) % 2 == 0
                        else ("candidate", "strong")
                    )
                    for arm in order:
                        remaining = deadline - time.monotonic()
                        if remaining <= 0:
                            raise TimeoutError(
                                "primary sample reached its 7,200-second cap"
                            )
                        binary = Path(
                            build["capacities"][str(capacity)][arm]["binaries"][
                                "models"
                            ]["path"]
                        )
                        argv = [
                            "taskset",
                            "-c",
                            str(cpu),
                            str(binary),
                            "--exact",
                            TEST,
                            "--nocapture",
                            "--test-threads=1",
                        ]
                        child_env = dict(
                            env,
                            PAGE_ARENA_PRIMARY_SEED=seed,
                            PAGE_ARENA_PRIMARY_CAPACITY=str(capacity),
                        )
                        result = subprocess.run(
                            argv,
                            env=child_env,
                            capture_output=True,
                            text=True,
                            timeout=min(CHILD_LIMIT_SECONDS, remaining),
                            check=False,
                        )
                        log = sample / f"seed{seed_index:02d}-{capacity}-{arm}.log"
                        log.write_text(result.stdout + "\nSTDERR\n" + result.stderr)
                        if (
                            result.returncode != 0
                            or "test result: ok. 1 passed" not in result.stdout
                        ):
                            raise RuntimeError(
                                f"primary test failed: {log}, exit={result.returncode}"
                            )
                        rows = parse_record(result.stdout, arm, seed, capacity)
                        for row in rows:
                            row["seed_index"] = seed_index
                            row["binary_sha256"] = build["capacities"][str(capacity)][
                                arm
                            ]["binaries"]["models"]["sha256"]
                            row["log_sha256"] = sha(log)
                            raw.write(json.dumps(row, sort_keys=True) + "\n")
                            key = (seed, capacity, row["workload"])
                            previous = seen.setdefault(key, {})
                            previous[arm] = row["trace_digest"]
                            if (
                                len(previous) == 2
                                and previous["strong"] != previous["candidate"]
                            ):
                                raise ValueError(f"arm trace divergence at {key}")
                        raw.flush()
                        os.fsync(raw.fileno())
        metadata["completed_epoch_seconds"] = time.time()
        metadata["raw_sha256"] = sha(sample / "raw.jsonl")
        metadata["host_sha256"] = sha(sample / "host.jsonl")
        metadata["swap_used_kib_after"] = quiet.swap_used_kib()
        metadata["host_load_after"] = os.getloadavg()
        write_json(sample / "metadata.json", metadata)
        print(sample / "raw.jsonl")
    except Exception as error:
        write_json(
            sample / "sample-error.json",
            {"error": repr(error), "raw_sha256": sha(sample / "raw.jsonl")},
        )
        raise


def sample(args):
    out = args.out.resolve()
    data, _ = prepared(out)
    built(out, args.build_attempt, data)
    attempt = base.new_output(out / "samples" / args.attempt)
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
        f"RuntimeMaxSec={RUN_LIMIT_SECONDS}",
        "--setenv=DISPLAY=",
        "--setenv=WAYLAND_DISPLAY=",
        "--setenv=JAVA_TOOL_OPTIONS=-Djava.awt.headless=true",
        "--setenv=LC_ALL=C",
        "--setenv=PAGE_ARENA_PRIMARY_SCOPED=1",
        sys.executable,
        str(Path(__file__)),
        "sample-inner",
        "--out",
        str(out),
        "--build-attempt",
        args.build_attempt,
        "--attempt",
        args.attempt,
        "--sample",
        str(attempt),
    ]
    write_json(attempt / "command.json", argv)
    with (attempt / "scope.log").open("w") as stream:
        result = subprocess.run(
            argv,
            stdout=stream,
            stderr=subprocess.STDOUT,
            timeout=RUN_LIMIT_SECONDS + 60,
            check=False,
        )
    if result.returncode != 0:
        write_json(
            attempt / "scope-error.json",
            {
                "exit_code": result.returncode,
                "scope_log_sha256": sha(attempt / "scope.log"),
            },
        )
        raise RuntimeError(
            f"primary bounded scope failed with exit {result.returncode}"
        )
    write_json(
        attempt / "scope-result.json",
        {"exit_code": 0, "scope_log_sha256": sha(attempt / "scope.log")},
    )
    print(attempt / "raw.jsonl")


def beta_fraction(a, b, x):
    tiny = 1e-300
    c = 1.0
    d = 1.0 - (a + b) * x / (a + 1.0)
    if abs(d) < tiny:
        d = tiny
    d = 1.0 / d
    product = d
    for step in range(1, 501):
        even = 2 * step
        numerator = step * (b - step) * x / ((a + even - 1) * (a + even))
        d = 1.0 + numerator * d
        c = 1.0 + numerator / c
        if abs(d) < tiny:
            d = tiny
        if abs(c) < tiny:
            c = tiny
        d = 1.0 / d
        product *= d * c
        numerator = -(a + step) * (a + b + step) * x / ((a + even) * (a + even + 1))
        d = 1.0 + numerator * d
        c = 1.0 + numerator / c
        if abs(d) < tiny:
            d = tiny
        if abs(c) < tiny:
            c = tiny
        d = 1.0 / d
        delta = d * c
        product *= delta
        if abs(delta - 1.0) < 3e-14:
            return product
    raise ArithmeticError("incomplete beta continued fraction did not converge")


def regularized_beta(a, b, x):
    if x == 0.0:
        return 0.0
    if x == 1.0:
        return 1.0
    factor = math.exp(
        math.lgamma(a + b)
        - math.lgamma(a)
        - math.lgamma(b)
        + a * math.log(x)
        + b * math.log1p(-x)
    )
    if x < (a + 1) / (a + b + 2):
        return factor * beta_fraction(a, b, x) / a
    return 1.0 - factor * beta_fraction(b, a, 1 - x) / b


def lower_t_cdf(t, degrees):
    x = degrees / (degrees + t * t)
    tail = 0.5 * regularized_beta(degrees / 2, 0.5, x)
    return tail if t < 0 else 1.0 - tail


def welch(control, candidate):
    n_control = len(control)
    n_candidate = len(candidate)
    if n_control != 30 or n_candidate != 30:
        raise ValueError("locked Welch comparison requires 30 observations per arm")
    mean_control = statistics.mean(control)
    mean_candidate = statistics.mean(candidate)
    variance_control = statistics.variance(control)
    variance_candidate = statistics.variance(candidate)
    standard_error_sq = variance_control / n_control + variance_candidate / n_candidate
    pooled_variance = (
        (n_control - 1) * variance_control + (n_candidate - 1) * variance_candidate
    ) / (n_control + n_candidate - 2)
    result = {
        "control_mean": mean_control,
        "candidate_mean": mean_candidate,
        "control_sample_variance": variance_control,
        "candidate_sample_variance": variance_candidate,
        "welch_tail": "less",
        "alpha": 0.05,
        "planned_n_per_arm": 30,
        "bh_hypotheses": 1,
    }
    if standard_error_sq == 0 or pooled_variance == 0:
        result["decision"] = "undefined_zero_variance"
        return result
    difference = mean_candidate - mean_control
    t = difference / math.sqrt(standard_error_sq)
    degrees = standard_error_sq**2 / (
        (variance_control / n_control) ** 2 / (n_control - 1)
        + (variance_candidate / n_candidate) ** 2 / (n_candidate - 1)
    )
    p = lower_t_cdf(t, degrees)
    d = difference / math.sqrt(pooled_variance)
    result.update(
        welch_t=t,
        welch_degrees_of_freedom=degrees,
        lower_tail_p=p,
        benjamini_hochberg_adjusted_p=p,
        cohens_d_candidate_minus_control=d,
        statistical_gate_pass=(p < 0.05 and d <= -0.5),
        decision="statistical_gate_pass"
        if p < 0.05 and d <= -0.5
        else "statistical_gate_fail",
    )
    return result


def analyze(args):
    out = args.out.resolve()
    data, _ = prepared(out)
    build, build_sha = built(out, args.build_attempt, data)
    sample = out / "samples" / args.sample_attempt
    if (sample / "sample-error.json").exists() or (
        sample / "scope-error.json"
    ).exists():
        raise ValueError("failed primary sample cannot be analyzed as complete")
    scope = json.loads((sample / "scope-result.json").read_text())
    if scope["exit_code"] != 0 or scope["scope_log_sha256"] != sha(
        sample / "scope.log"
    ):
        raise ValueError("primary bounded scope result or log changed")
    metadata = json.loads((sample / "metadata.json").read_text())
    if (
        metadata["build_sha256"] != build_sha
        or metadata["prepared_sha256"] != sha(out / "prepared.json")
        or metadata["raw_sha256"] != sha(sample / "raw.jsonl")
        or metadata["host_sha256"] != sha(sample / "host.jsonl")
        or metadata["preflight_sha256"] != sha(sample / "preflight" / "preflight.json")
    ):
        raise ValueError(
            "primary raw rows are not bound to protected source and binaries"
        )
    plan = config()
    host_rows = [
        json.loads(line) for line in (sample / "host.jsonl").read_text().splitlines()
    ]
    if (
        len(host_rows) != 30
        or [row["seed"] for row in host_rows] != plan["seeds"]
        or [row["seed_index"] for row in host_rows] != list(range(30))
    ):
        raise ValueError("primary sample lacks all 30 pre-seed host surveys")
    for row in host_rows:
        log = sample / f"host-seed{row['seed_index']:02d}.txt"
        if sha(log) != row["log_sha256"]:
            raise ValueError("primary host survey log changed")
        if row["noisy"] != bool(
            row["swap_used_kib"]
            or any(value < 90 for value in row["sibling_idle_percent"].values())
        ):
            raise ValueError("primary host noise flag changed")
    rows = [
        json.loads(line) for line in (sample / "raw.jsonl").read_text().splitlines()
    ]
    if len(rows) != 30 * 4 * 4 * 2:
        raise ValueError("primary sample omitted or duplicated a matrix cell")
    by_key = {}
    for row in rows:
        key = (row["arm"], row["seed"], row["capacity"], row["workload"])
        if key in by_key:
            raise ValueError(f"duplicate primary cell: {key}")
        if (
            row["seed"] not in plan["seeds"]
            or row["capacity"] not in CAPACITIES
            or row["workload"] not in WORKLOADS
        ):
            raise ValueError("primary row not in frozen seed/capacity/workload plan")
        if row["seed_index"] != plan["seeds"].index(row["seed"]):
            raise ValueError("primary row seed index changed")
        if row["payload_bytes"] != PAYLOAD:
            raise ValueError("primary row payload denominator changed")
        binary = build["capacities"][str(row["capacity"])][row["arm"]]["binaries"][
            "models"
        ]
        if row["binary_sha256"] != binary["sha256"]:
            raise ValueError("primary row binary hash changed")
        log = sample / f"seed{row['seed_index']:02d}-{row['capacity']}-{row['arm']}.log"
        if row["log_sha256"] != sha(log):
            raise ValueError("primary row invocation log changed")
        by_key[key] = row
    observations = []
    for seed_index, seed in enumerate(plan["seeds"]):
        for capacity in CAPACITIES:
            for workload in WORKLOADS:
                control = by_key[("strong", seed, capacity, workload)]
                candidate = by_key[("candidate", seed, capacity, workload)]
                if control["trace_digest"] != candidate["trace_digest"]:
                    raise ValueError("matched primary arms replayed different traces")
        for arm in ("strong", "candidate"):
            cell_rows = [
                by_key[(arm, seed, capacity, workload)]
                for capacity in CAPACITIES
                for workload in WORKLOADS
            ]
            observations.append(
                {
                    "seed_index": seed_index,
                    "seed": seed,
                    "arm": arm,
                    "sealed_page_pairs_mean_16_cells": statistics.mean(
                        row["sealed_page_pairs"] for row in cell_rows
                    ),
                    "cache_only_source_nodes_mean_16_cells_secondary": statistics.mean(
                        row["cache_only_source_nodes"] for row in cell_rows
                    ),
                    "cells": 16,
                }
            )
    control = [
        row["sealed_page_pairs_mean_16_cells"]
        for row in observations
        if row["arm"] == "strong"
    ]
    candidate = [
        row["sealed_page_pairs_mean_16_cells"]
        for row in observations
        if row["arm"] == "candidate"
    ]
    result = {
        "experiment_id": 395,
        "hypothesis_id": 395,
        "primary_metric": plan["primary_metric"],
        "plan_sha256": sha(PLAN),
        "build_sha256": build_sha,
        "raw_sha256": metadata["raw_sha256"],
        "raw_cells": len(rows),
        "noisy_host_seeds": [row["seed_index"] for row in host_rows if row["noisy"]],
        "swap_used_kib_after": metadata["swap_used_kib_after"],
        "host_sha256": metadata["host_sha256"],
        "observations": observations,
        "welch": welch(control, candidate),
        "acceptance_verdict": (
            "inconclusive_host_noise_and_pending_full_models_parity"
            if any(row["noisy"] for row in host_rows) or metadata["swap_used_kib_after"]
            else "pending_full_models_parity_and_secondary_gates"
        ),
    }
    destination = base.new_output(sample / "analysis")
    write_json(destination / "result.json", result)
    print(destination / "result.json")


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="mode", required=True)
    for mode in ("prepare", "build", "sample", "sample-inner", "analyze"):
        command = sub.add_parser(mode)
        command.add_argument("--out", type=Path, required=True)
        if mode in {"build", "sample", "sample-inner"}:
            command.add_argument("--attempt", type=base.attempt_name, required=True)
        if mode in {"sample", "sample-inner", "analyze"}:
            command.add_argument(
                "--build-attempt", type=base.attempt_name, required=True
            )
        if mode == "sample-inner":
            command.add_argument("--sample", type=Path, required=True)
        if mode == "analyze":
            command.add_argument(
                "--sample-attempt", type=base.attempt_name, required=True
            )
    args = parser.parse_args()
    {
        "prepare": prepare,
        "build": build,
        "sample": sample,
        "sample-inner": sample_inner,
        "analyze": analyze,
    }[args.mode](args)


if __name__ == "__main__":
    main()
