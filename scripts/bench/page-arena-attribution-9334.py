import argparse
import difflib
import hashlib
import json
import os
import random
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRATCH = ROOT / "target" / "scratch"
ARENA = Path("models/src/rust/rholang/canonical_bytes_arena.rs")
PROBE = Path("models/tests/support/page_arena_isolated_probe.rs")
PROPERTY = Path("models/tests/support/canonical_source_cache.rs")
PROJECTION = Path("scripts/bench/fixtures/page-arena-common-source-cache-9334.rs")
PATCHED_DEPENDENCY = Path("rholang-rs-cost-accounting-transpiler")
PATCHED_DEPENDENCY_ROOT = ROOT.parent / PATCHED_DEPENDENCY
STRONG_ARENA = SCRATCH / "page-arena-control-11c92e7" / ARENA
STRONG_SHA256 = "8baac501fa2468b049d0a100ca00000bc79f8e07f820211c303df8a82a95feb8"
CANDIDATE_SHA256 = "ce493951d23332e60eda133cd72a0b26754799335d9811e56bc82cc334591438"
PROBE_SHA256 = "5e4a10ceb999f17655f3dc03767980d76e2d29b96b5c18dd74fad0c408dec614"
PROPERTY_SHA256 = "46bccdd89da525c0d6d63dc9ef4cc8d8d35b8ff21d31effa50e39ee57a468a66"
PROJECTION_SHA256 = "fc699ddd74a5bc5b2ea3450e4fbda24369d8d0f827b51d29ce2b0eef0aed7c78"
FIXTURES = {
    "common_fork": (
        "models",
        "common_historical_fork_export_preserves_bytes_and_provenance",
    ),
    "common_borrowed": (
        "models",
        "common_registered_source_slice_and_generated_tail_preserve_origin",
    ),
    "isolated": (
        "models",
        "isolated_append_export_and_contiguous_hash_requests",
    ),
    "act20k": (
        "models",
        "pure_reducer_is_linear_and_heap_total_at_twenty_thousand_levels",
    ),
    "width4096": (
        "models",
        "width_4096_validation_uses_counted_occurrences_on_a_small_native_stack",
    ),
    "nested_epathmap": (
        "models",
        "epathmap_snapshot_and_every_nested_map_value_are_validated",
    ),
    "golden": ("golden", "protobuf_goldens_epathmap_fixtures"),
}


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def new_output(path):
    path = path.resolve()
    if not path.is_relative_to(SCRATCH.resolve()) or path.exists():
        raise ValueError("output must be a fresh path below target/scratch")
    path.mkdir(parents=True)
    return path


def attempt_name(value):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,31}", value):
        raise argparse.ArgumentTypeError("attempt must be a short plain label")
    return value


def manifest(root):
    result = {}
    for directory, dirs, files in os.walk(root):
        dirs[:] = sorted(
            name for name in dirs if name not in {"target", ".git", "node_modules"}
        )
        for name in dirs:
            if (Path(directory) / name).is_symlink():
                raise ValueError(f"source directory is a symlink: {directory}/{name}")
        for name in sorted(files):
            if name in {"target", ".git"}:
                continue
            path = Path(directory) / name
            if path.is_symlink() or not path.is_file():
                raise ValueError(f"source file is not a regular file: {path}")
            result[str(path.relative_to(root))] = sha(path)
    return result


def target_metadata(root):
    result = {}
    for directory, dirs, files in os.walk(root):
        dirs.sort()
        for name in dirs:
            if (Path(directory) / name).is_symlink():
                raise ValueError(f"target directory is a symlink: {directory}/{name}")
        for name in sorted(files):
            path = Path(directory) / name
            if path.is_symlink() or not path.is_file():
                raise ValueError(f"target file is not regular: {path}")
            stat = path.stat()
            result[str(path.relative_to(root))] = [stat.st_size, stat.st_mtime_ns]
    return result


def quarantine_cmake_run_pairs(private, attempt, quarantined):
    fingerprints = private / "release" / ".fingerprint"
    build = private / "release" / "build"
    count = 0
    for cache in sorted(build.glob("*/out/build/CMakeCache.txt")):
        companion = cache.parents[2]
        entry = fingerprints / companion.name
        markers = list(entry.glob("run-build-script-*.json")) if entry.is_dir() else []
        if len(markers) != 1 or not cache.is_file():
            raise ValueError(f"missing or ambiguous CMake run pair: {cache}")
        marker = markers[0]
        if {item.name for item in entry.iterdir()} != {marker.name, marker.stem}:
            raise ValueError(f"unexpected CMake run fingerprint contents: {entry}")
        fingerprint_files = manifest(entry)
        build_files = manifest(companion)
        archived_fp = attempt / "quarantine" / ".fingerprint" / entry.name
        archived_build = attempt / "quarantine" / "build" / companion.name
        if archived_fp.exists() or archived_build.exists():
            raise ValueError(f"CMake run pair overlaps an existing quarantine: {cache}")
        archived_fp.parent.mkdir(parents=True, exist_ok=True)
        archived_build.parent.mkdir(parents=True, exist_ok=True)
        entry.rename(archived_fp)
        companion.rename(archived_build)
        if (
            manifest(archived_fp) != fingerprint_files
            or manifest(archived_build) != build_files
        ):
            raise ValueError(f"CMake run pair quarantine changed bytes: {cache}")
        quarantined.append(
            {
                "kind": "cmake_run",
                "cache_path": str(cache.relative_to(private)),
                "fingerprint_path": str(entry.relative_to(private)),
                "fingerprint_files_sha256": fingerprint_files,
                "build_path": str(companion.relative_to(private)),
                "build_files_sha256": build_files,
            }
        )
        count += 1
    if list(build.glob("*/out/build/CMakeCache.txt")):
        raise ValueError("private target retained a relocated CMake cache")
    return count


def private_release_target(attempt, disk_free):
    release = ROOT / "target" / "release"
    before = target_metadata(release)
    release_bytes = sum(size for size, _ in before.values())
    if disk_free - release_bytes < 20 * 1024**3:
        raise ValueError(
            f"insufficient space for private release target: free={disk_free}, source={release_bytes}"
        )
    private = attempt / "private-target"
    shutil.copytree(release, private / "release")
    after = target_metadata(release)
    copied = target_metadata(private / "release")
    if before != after or before != copied:
        raise ValueError("release target moved during private copy")
    write_json(attempt / "target-clone-metadata.json", copied)
    if shutil.disk_usage(ROOT).free < 15 * 1024**3:
        raise ValueError("less than 15 GiB free after private target copy")
    fingerprints = private / "release" / ".fingerprint"
    build = private / "release" / "build"
    quarantined = []
    kinds = {"build": 0, "run": 0, "cmake_run": 0}
    for entry in sorted(fingerprints.glob("models-*")):
        compiled = (entry / "build-script-build-script-build.json").is_file()
        executed = (entry / "run-build-script-build-script-build.json").is_file()
        if not compiled and not executed:
            continue
        if compiled == executed:
            raise ValueError(f"ambiguous models build-script fingerprint: {entry}")
        kind = "build" if compiled else "run"
        allowed = (
            {
                "build-script-build-script-build",
                "build-script-build-script-build.json",
                "dep-build-script-build-script-build",
                "invoked.timestamp",
            }
            if compiled
            else {
                "run-build-script-build-script-build",
                "run-build-script-build-script-build.json",
            }
        )
        if {item.name for item in entry.iterdir()} != allowed:
            raise ValueError(f"unexpected models {kind} fingerprint contents: {entry}")
        companion = build / entry.name
        if not companion.is_dir():
            raise ValueError(f"models {kind} build directory missing: {companion}")
        fingerprint_files = manifest(entry)
        build_files = manifest(companion)
        archived_fp = attempt / "quarantine" / ".fingerprint" / entry.name
        archived_build = attempt / "quarantine" / "build" / entry.name
        archived_fp.parent.mkdir(parents=True, exist_ok=True)
        archived_build.parent.mkdir(parents=True, exist_ok=True)
        entry.rename(archived_fp)
        companion.rename(archived_build)
        if (
            manifest(archived_fp) != fingerprint_files
            or manifest(archived_build) != build_files
        ):
            raise ValueError(f"models {kind} build-script quarantine changed bytes")
        quarantined.append(
            {
                "kind": kind,
                "fingerprint_path": str(entry.relative_to(private)),
                "fingerprint_files_sha256": fingerprint_files,
                "build_path": str(companion.relative_to(private)),
                "build_files_sha256": build_files,
            }
        )
        kinds[kind] += 1
    if kinds["build"] == 0 or kinds["build"] != kinds["run"]:
        raise ValueError(f"unexpected models build-script fingerprint count: {kinds}")
    if list(build.glob("models-*/build-script-build")):
        raise ValueError("private target retained a stale models build script")
    kinds["cmake_run"] = quarantine_cmake_run_pairs(private, attempt, quarantined)
    write_json(
        attempt / "quarantine.json",
        {"categories": kinds, "entries": quarantined},
    )
    return private, release_bytes


def method(source, signature):
    if source.count(signature) != 1:
        raise ValueError(f"expected one candidate method: {signature}")
    start = source.index(signature)
    opening = source.index("{", start)
    depth = 0
    for index in range(opening, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[start : index + 1]
    raise ValueError(f"unterminated candidate method: {signature}")


def insert_once(source, anchor, insertion, after=False):
    if source.count(anchor) != 1:
        raise ValueError(f"expected one strong-arena insertion point: {anchor}")
    if after:
        return source.replace(anchor, anchor + insertion + "\n\n", 1)
    return source.replace(anchor, insertion + "\n\n" + anchor, 1)


def strong_with_shims(strong, candidate):
    signatures = {
        "provenance_at_indexed": "    fn provenance_at_indexed(",
        "same_view": "    pub(crate) fn same_view(&self, other: &Self)",
        "last_provenance": "    pub(crate) fn last_provenance(",
    }
    shims = {
        name: method(candidate, signature) for name, signature in signatures.items()
    }
    for name in shims:
        if signatures[name] in strong:
            raise ValueError(f"frozen strong arena already has {name}")
    result = insert_once(
        strong, "    fn handle(span: SequenceSpan)", shims["provenance_at_indexed"]
    )
    result = insert_once(
        result, "impl FrozenCanonicalBytes {\n", shims["same_view"], True
    )
    result = insert_once(
        result,
        "    /// O(1) structural concatenation. Both operands remain valid checkpoints.",
        shims["last_provenance"],
    )
    return result, {
        name: hashlib.sha256(body.encode()).hexdigest() for name, body in shims.items()
    }


def check_non_arena(snapshot, expected):
    current = manifest(snapshot)
    key = str(ARENA)
    before = {name: digest for name, digest in expected.items() if name != key}
    after = {name: digest for name, digest in current.items() if name != key}
    if before != after:
        missing = sorted(before.keys() - after.keys())
        added = sorted(after.keys() - before.keys())
        changed = sorted(
            name for name in before.keys() & after.keys() if before[name] != after[name]
        )
        raise ValueError(
            f"non-arena source changed: missing={missing}, added={added}, changed={changed}"
        )
    return current


def check_patched_dependency(out, data):
    saved = out / "patched-dependency-manifest.json"
    if sha(saved) != data["patched_dependency_manifest_sha256"]:
        raise ValueError("patched dependency manifest changed")
    expected = json.loads(saved.read_text())
    if manifest(out / PATCHED_DEPENDENCY) != expected:
        raise ValueError("patched dependency snapshot changed")


def prepared(out):
    data = json.loads((out / "prepared.json").read_text())
    if sha(Path(__file__)) != data["script_sha256"]:
        raise ValueError("attribution harness changed after source preparation")
    expected = json.loads((out / "source-manifest.json").read_text())
    if sha(out / "source-manifest.json") != data["manifest_sha256"]:
        raise ValueError("prepared source manifest changed")
    if sha(out / "candidate-arena.rs") != data["candidate_arena_sha256"]:
        raise ValueError("protected candidate arena changed")
    if sha(out / "strong-compat-arena.rs") != data["strong_compat_arena_sha256"]:
        raise ValueError("protected strong compatibility arena changed")
    if sha(out / "source" / PROPERTY) != data["projected_property_sha256"]:
        raise ValueError("common projected test file changed")
    if sha(out / "source" / PROJECTION) != data["projection_source_sha256"]:
        raise ValueError("projection source changed inside the snapshot")
    check_non_arena(out / "source", expected)
    check_patched_dependency(out, data)
    return data, expected


def prepare(args):
    if sha(ROOT / ARENA) != CANDIDATE_SHA256 or sha(ROOT / PROBE) != PROBE_SHA256:
        raise ValueError(
            "candidate arena or fixed probe is no longer the pinned source"
        )
    if sha(STRONG_ARENA) != STRONG_SHA256:
        raise ValueError("frozen strong-root arena changed")
    if sha(ROOT / PROPERTY) != PROPERTY_SHA256:
        raise ValueError("live candidate source-cache property changed")
    if sha(ROOT / PROJECTION) != PROJECTION_SHA256:
        raise ValueError("common test-only projection changed")
    cargo_manifest = (ROOT / "Cargo.toml").read_text()
    for package in (
        "rholang-parser",
        "rholang-tree-sitter",
        "rholang-tree-sitter-proc-macro",
    ):
        entry = f'{package} = {{ path = "../{PATCHED_DEPENDENCY}/{package}"'
        if entry not in cargo_manifest:
            raise ValueError(f"unrecognized patched dependency path: {package}")
    out = new_output(args.out)
    try:
        dependency_before = manifest(PATCHED_DEPENDENCY_ROOT)
        shutil.copytree(
            PATCHED_DEPENDENCY_ROOT,
            out / PATCHED_DEPENDENCY,
            ignore=shutil.ignore_patterns(".git", "target", "node_modules"),
        )
        dependency_after = manifest(PATCHED_DEPENDENCY_ROOT)
        dependency_copy = manifest(out / PATCHED_DEPENDENCY)
        if (
            dependency_before != dependency_after
            or dependency_before != dependency_copy
        ):
            raise ValueError("patched dependency moved during snapshot")
        write_json(out / "patched-dependency-manifest.json", dependency_copy)
        before = manifest(ROOT)
        snapshot = out / "source"
        shutil.copytree(
            ROOT,
            snapshot,
            ignore=shutil.ignore_patterns(".git", "target", "node_modules"),
        )
        after = manifest(ROOT)
        copied = manifest(snapshot)
        if before != after or before != copied:
            raise ValueError("source moved during snapshot; no build may use it")
        shutil.copyfile(snapshot / PROJECTION, snapshot / PROPERTY)
        projected = manifest(snapshot)
        expected_projection = dict(before)
        expected_projection[str(PROPERTY)] = PROJECTION_SHA256
        if projected != expected_projection:
            raise ValueError(
                "test-only snapshot projection changed another source file"
            )
        (out / "common-property-projection.diff").write_text(
            "".join(
                difflib.unified_diff(
                    (ROOT / PROPERTY).read_text().splitlines(keepends=True),
                    (snapshot / PROPERTY).read_text().splitlines(keepends=True),
                    fromfile="live-candidate-source-cache.rs",
                    tofile="common-source-cache-projection.rs",
                )
            )
        )
        candidate = (snapshot / ARENA).read_text()
        strong = STRONG_ARENA.read_text()
        variant, shims = strong_with_shims(strong, candidate)
        (out / "candidate-arena.rs").write_text(candidate)
        (out / "strong-compat-arena.rs").write_text(variant)
        (out / "strong-compat.diff").write_text(
            "".join(
                difflib.unified_diff(
                    strong.splitlines(keepends=True),
                    variant.splitlines(keepends=True),
                    fromfile="frozen-strong-arena.rs",
                    tofile="strong-compat-arena.rs",
                )
            )
        )
        write_json(out / "source-manifest.json", projected)
        write_json(
            out / "prepared.json",
            {
                "candidate_arena_sha256": sha(out / "candidate-arena.rs"),
                "strong_original_sha256": STRONG_SHA256,
                "strong_compat_arena_sha256": sha(out / "strong-compat-arena.rs"),
                "probe_sha256": sha(snapshot / PROBE),
                "original_property_sha256": PROPERTY_SHA256,
                "projected_property_sha256": PROJECTION_SHA256,
                "projection_source_sha256": sha(snapshot / PROJECTION),
                "property_projection_diff_sha256": sha(
                    out / "common-property-projection.diff"
                ),
                "only_snapshot_source_change": str(PROPERTY),
                "patched_dependency_manifest_sha256": sha(
                    out / "patched-dependency-manifest.json"
                ),
                "patched_dependency_snapshot": str(out / PATCHED_DEPENDENCY),
                "manifest_sha256": sha(out / "source-manifest.json"),
                "shim_method_sha256": shims,
                "shared_target": str(ROOT / "target"),
                "snapshot": str(snapshot),
                "script_sha256": sha(Path(__file__)),
            },
        )
    except Exception as error:
        write_json(out / "prepare-error.json", {"error": repr(error)})
        raise
    print(out / "prepared.json")


def artifact_paths(log):
    found = {}
    for line in log.read_text().splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("reason") != "compiler-artifact" or not event.get("executable"):
            continue
        target = event["target"]
        if target["name"] == "models" and "lib" in target["kind"]:
            found["models"] = Path(event["executable"])
        if target["name"] == "epathmap_canonical_fixtures" and "test" in target["kind"]:
            found["golden"] = Path(event["executable"])
    if set(found) != {"models", "golden"}:
        raise ValueError(f"expected lib and golden executables, found {found}")
    return found


def build_one(
    out, attempt, arm, variant, expected, data, private_target, strong_schema=None
):
    snapshot = out / "source"
    arena = snapshot / ARENA
    shutil.copyfile(variant, arena)
    if sha(arena) != sha(variant):
        raise ValueError(f"{arm} arena copy failed")
    check_non_arena(snapshot, expected)
    check_patched_dependency(out, data)
    log = attempt / f"{arm}-build.log"
    argv = [
        "systemd-run",
        "--user",
        "--scope",
        "--collect",
        f"--working-directory={snapshot}",
        "-p",
        "MemoryMax=5G",
        "-p",
        "MemorySwapMax=0",
        "-p",
        "CPUQuota=100%",
        "-p",
        "TasksMax=256",
        "-p",
        "RuntimeMaxSec=900",
        f"--setenv=CARGO_TARGET_DIR={private_target}",
        "--setenv=CARGO_INCREMENTAL=0",
        "--setenv=DISPLAY=",
        "--setenv=WAYLAND_DISPLAY=",
        "--setenv=JAVA_TOOL_OPTIONS=-Djava.awt.headless=true",
        "--setenv=LC_ALL=C",
        "cargo",
        "test",
        "--locked",
        "-p",
        "models",
        "--release",
        "--lib",
        "--test",
        "epathmap_canonical_fixtures",
        "--no-run",
        "--message-format=json",
    ]
    write_json(attempt / f"{arm}-command.json", argv)
    started_ns = time.time_ns()
    with log.open("w") as stream:
        result = subprocess.run(
            argv, stdout=stream, stderr=subprocess.STDOUT, timeout=930, check=False
        )
    check_non_arena(snapshot, expected)
    check_patched_dependency(out, data)
    if result.returncode != 0:
        raise RuntimeError(
            f"{arm} bounded build failed with exit {result.returncode}: {log}"
        )
    build_root = private_target / "release" / "build"
    scripts = sorted(build_root.glob("models-*/build-script-build"))
    generated = sorted(build_root.glob("models-*/out/rhoapi_bincode_schema.rs"))
    if not scripts or not generated:
        raise ValueError(f"{arm} build omitted models build-script/schema artifacts")
    if strong_schema is None and not any(
        path.stat().st_mtime_ns >= started_ns for path in scripts
    ):
        raise ValueError("strong models build script was not freshly compiled")
    schema = max(generated, key=lambda path: path.stat().st_mtime_ns)
    if strong_schema is None and schema.stat().st_mtime_ns < started_ns:
        raise ValueError("strong models schema was not freshly generated")
    schema_sha256 = sha(schema)
    if b"CS_" not in schema.read_bytes():
        raise ValueError(f"{arm} generated schema lacks the pinned Form prefix")
    if strong_schema is not None and schema_sha256 != strong_schema:
        raise ValueError(
            "candidate generated schema differs from strong generated schema"
        )
    build_script = max(scripts, key=lambda path: path.stat().st_mtime_ns)
    binaries = artifact_paths(log)
    output = {}
    for kind, source in binaries.items():
        if not source.is_file():
            raise ValueError(f"reported {arm} binary missing: {source}")
        destination = attempt / f"{arm}-{kind}"
        shutil.copy2(source, destination)
        output[kind] = {"path": str(destination), "sha256": sha(destination)}
    return {
        "arena_sha256": sha(variant),
        "build_log_sha256": sha(log),
        "build_script_sha256": sha(build_script),
        "build_script_path": str(build_script),
        "generated_schema_sha256": schema_sha256,
        "generated_schema_path": str(schema),
        "binaries": output,
    }


def build(args):
    out = args.out.resolve()
    data, expected = prepared(out)
    disk_free = shutil.disk_usage(ROOT).free
    if disk_free < 25 * 1024**3:
        raise ValueError(
            f"less than 25 GiB free before private-target build: {disk_free}"
        )
    arena = out / "source" / ARENA
    if sha(arena) != data["candidate_arena_sha256"]:
        raise ValueError("snapshot arena is not restored to candidate source")
    attempt = new_output(out / "builds" / args.attempt)
    try:
        private_target, release_bytes = private_release_target(attempt, disk_free)
        result = {
            "prepared_sha256": sha(out / "prepared.json"),
            "manifest_sha256": data["manifest_sha256"],
            "script_sha256": sha(Path(__file__)),
            "cargo_version": subprocess.check_output(
                ["cargo", "--version"], text=True
            ).strip(),
            "rustc_version": subprocess.check_output(
                ["rustc", "--version"], text=True
            ).strip(),
            "rustflags": os.environ.get("RUSTFLAGS", ""),
            "shared_target": data["shared_target"],
            "private_target": str(private_target),
            "private_target_source_bytes": release_bytes,
            "target_clone_metadata_sha256": sha(attempt / "target-clone-metadata.json"),
            "quarantine_sha256": sha(attempt / "quarantine.json"),
            "patched_dependency_manifest_sha256": data[
                "patched_dependency_manifest_sha256"
            ],
            "disk_free_before_bytes": disk_free,
            "arms": {},
        }
        for arm, variant in (
            ("strong", out / "strong-compat-arena.rs"),
            ("candidate", out / "candidate-arena.rs"),
        ):
            result["arms"][arm] = build_one(
                out,
                attempt,
                arm,
                variant,
                expected,
                data,
                private_target,
                result["arms"].get("strong", {}).get("generated_schema_sha256"),
            )
        write_json(attempt / "build.json", result)
    except Exception as error:
        write_json(attempt / "build-error.json", {"error": repr(error)})
        raise
    finally:
        shutil.copyfile(out / "candidate-arena.rs", arena)
        check_non_arena(out / "source", expected)
    print(attempt / "build.json")


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
        parts = line.split()
        if len(parts) >= 3 and parts[0] == "Average:" and parts[1].isdigit():
            result[int(parts[1])] = float(parts[-1])
    return result


def mpstat(cpu_spec, seconds):
    env = os.environ.copy()
    env.update(LC_ALL="C", DISPLAY="", WAYLAND_DISPLAY="")
    result = subprocess.run(
        ["mpstat", "-P", cpu_spec, "1", str(seconds)],
        capture_output=True,
        text=True,
        timeout=seconds + 15,
        env=env,
        check=True,
    )
    return result.stdout


def choose_cpu(output):
    idles = average_idle(output)
    allowed = os.sched_getaffinity(0)
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
        raise ValueError("no complete online SMT sibling group in affinity")
    chosen = min(
        groups, key=lambda group: (max(100 - idles[cpu] for cpu in group), group[0])
    )
    return {
        "pinned_cpu": chosen[0],
        "siblings": list(chosen),
        "survey_idle": {str(c): idles[c] for c in chosen},
    }


def swap_in_use():
    values = {}
    for line in Path("/proc/meminfo").read_text().splitlines():
        key, _, value = line.partition(":")
        if key in {"SwapTotal", "SwapFree"}:
            values[key] = int(value.split()[0])
    return values["SwapFree"] < values["SwapTotal"]


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
        raise ValueError(f"{binary}: expected one {suffix}, found {matches}")
    return matches[0]


def run_test(binary, name, cpu, deadline, timeout):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError("attribution measurement deadline reached")
    argv = [
        "taskset",
        "-c",
        str(cpu),
        "/usr/bin/time",
        "-f",
        "PAGE_ARENA_RESOURCE user_s=%U system_s=%S peak_rss_kib=%M voluntary_cs=%w involuntary_cs=%c",
        str(binary),
        "--exact",
        name,
        "--nocapture",
        "--test-threads=1",
    ]
    env = os.environ.copy()
    env.update(LC_ALL="C", DISPLAY="", WAYLAND_DISPLAY="")
    env.pop("EPATHMAP_CANONICAL_GOLDENS_BLESS", None)
    started = time.perf_counter_ns()
    result = subprocess.run(
        argv,
        capture_output=True,
        text=True,
        timeout=min(timeout, remaining),
        env=env,
        check=False,
    )
    record = {
        "command": argv,
        "wall_ns": time.perf_counter_ns() - started,
        "returncode": result.returncode,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }
    if len(result.stdout) + len(result.stderr) > 1024 * 1024:
        raise ValueError(f"unbounded test output from {name}")
    resource = re.findall(
        r"^PAGE_ARENA_RESOURCE ([^\r\n]+)$", result.stderr, re.MULTILINE
    )
    if len(resource) != 1:
        raise ValueError(f"GNU time resource record missing or duplicated: {name}")
    values = dict(token.split("=", 1) for token in resource[0].split())
    record["resource"] = {
        "user_s": float(values["user_s"]),
        "system_s": float(values["system_s"]),
        "peak_rss_kib": int(values["peak_rss_kib"]),
        "voluntary_cs": int(values["voluntary_cs"]),
        "involuntary_cs": int(values["involuntary_cs"]),
    }
    expected_passes = 0 if name == "__page_arena_9334_noop__" else 1
    if result.returncode != 0 or not re.search(
        rf"test result: ok\. {expected_passes} passed; 0 failed", result.stdout
    ):
        raise RuntimeError(f"test failed: {name}: {record}")
    if name.endswith("isolated_append_export_and_contiguous_hash_requests"):
        matches = re.findall(r"page_arena_isolated_probe_v1 ([^\r\n]+)", result.stdout)
        if len(matches) != 1:
            raise ValueError("isolated probe record missing or duplicated")
        record["probe"] = {
            key: int(value)
            for key, value in (token.split("=", 1) for token in matches[0].split())
        }
        probe = record["probe"]
        if tuple(
            probe[key]
            for key in (
                "bytes",
                "exports",
                "hash_requests",
                "contiguous_hits",
                "copied_fallbacks",
            )
        ) != (65536, 1024, 2046, 1023, 1023):
            raise ValueError(f"isolated probe shape changed: {probe}")
    return record


def append_row(path, row):
    with path.open("a") as stream:
        stream.write(json.dumps(row, sort_keys=True) + "\n")


def summarize(pairs, metrics, seed):
    result = {}
    rng = random.Random(seed)
    for metric in metrics:
        ratios = [pair["candidate"][metric] / pair["strong"][metric] for pair in pairs]
        intervals = sorted(
            statistics.median(rng.choices(ratios, k=len(ratios))) for _ in range(10_000)
        )
        result[metric] = {
            "strong_median": statistics.median(
                pair["strong"][metric] for pair in pairs
            ),
            "candidate_median": statistics.median(
                pair["candidate"][metric] for pair in pairs
            ),
            "paired_median_candidate_over_strong": statistics.median(ratios),
            "bootstrap_95pct": [intervals[249], intervals[9749]],
        }
    return result


def capped_reexec(args):
    if os.environ.get("PAGE_ARENA_9334_ATTR_CAPPED") == "1":
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
        "RuntimeMaxSec=600" if args.mode == "semantics" else "RuntimeMaxSec=1800",
        "--setenv=PAGE_ARENA_9334_ATTR_CAPPED=1",
        "--setenv=DISPLAY=",
        "--setenv=WAYLAND_DISPLAY=",
        "--setenv=JAVA_TOOL_OPTIONS=-Djava.awt.headless=true",
        sys.executable,
        str(Path(__file__).resolve()),
        *sys.argv[1:],
    ]
    os.execvp("systemd-run", argv)


def semantics(args):
    capped_reexec(args)
    if os.environ.get("EPATHMAP_CANONICAL_GOLDENS_BLESS"):
        raise ValueError("golden bless mode is forbidden")
    out = args.out.resolve()
    data, expected = prepared(out)
    if sha(out / "source" / ARENA) != data["candidate_arena_sha256"]:
        raise ValueError("snapshot arena was not restored after build")
    build_dir = out / "builds" / args.build_attempt
    build_data = json.loads((build_dir / "build.json").read_text())
    if build_data["prepared_sha256"] != sha(out / "prepared.json"):
        raise ValueError("build does not match prepared snapshot")
    if (
        build_data["arms"]["strong"]["generated_schema_sha256"]
        != build_data["arms"]["candidate"]["generated_schema_sha256"]
    ):
        raise ValueError("strong and candidate generated schemas differ")
    binaries = {}
    for arm in ("strong", "candidate"):
        binaries[arm] = {}
        for kind in ("models", "golden"):
            entry = build_data["arms"][arm]["binaries"][kind]
            binary = Path(entry["path"])
            if sha(binary) != entry["sha256"]:
                raise ValueError(f"protected {arm} {kind} binary changed")
            binaries[arm][kind] = binary
    check_non_arena(out / "source", expected)
    attempt = new_output(out / "semantic-gates" / args.attempt)
    selected = {
        arm: {
            fixture: selected_test(binaries[arm][kind], suffix)
            for fixture, (kind, suffix) in FIXTURES.items()
        }
        for arm in binaries
    }
    cpu = min(os.sched_getaffinity(0))
    write_json(
        attempt / "metadata.json",
        {
            "build_sha256": sha(build_dir / "build.json"),
            "prepared_sha256": sha(out / "prepared.json"),
            "script_sha256": sha(Path(__file__)),
            "source_manifest_sha256": data["manifest_sha256"],
            "generated_schema_sha256": build_data["arms"]["strong"][
                "generated_schema_sha256"
            ],
            "selected_tests": selected,
            "pinned_cpu": cpu,
            "deadline_seconds": 570,
        },
    )
    deadline = time.monotonic() + 570
    completed = []
    try:
        for fixture in FIXTURES:
            for arm in ("strong", "candidate"):
                kind = FIXTURES[fixture][0]
                row = run_test(
                    binaries[arm][kind], selected[arm][fixture], cpu, deadline, 60
                )
                append_row(
                    attempt / "semantics.jsonl", {"arm": arm, "fixture": fixture, **row}
                )
                completed.append({"arm": arm, "fixture": fixture})
        write_json(
            attempt / "summary.json",
            {"passed": completed, "timing_verdict": "not_sampled"},
        )
    except Exception as error:
        write_json(
            attempt / "semantics-error.json",
            {"completed": completed, "error": repr(error)},
        )
        raise
    print(attempt / "summary.json")


def measure(args):
    capped_reexec(args)
    if os.environ.get("EPATHMAP_CANONICAL_GOLDENS_BLESS"):
        raise ValueError("golden bless mode is forbidden")
    out = args.out.resolve()
    data, expected = prepared(out)
    if sha(out / "source" / ARENA) != data["candidate_arena_sha256"]:
        raise ValueError("snapshot arena was not restored after build")
    build_dir = out / "builds" / args.build_attempt
    build_data = json.loads((build_dir / "build.json").read_text())
    if build_data["prepared_sha256"] != sha(out / "prepared.json"):
        raise ValueError("build does not match prepared snapshot")
    binaries = {}
    for arm in ("strong", "candidate"):
        binaries[arm] = {}
        for kind in ("models", "golden"):
            entry = build_data["arms"][arm]["binaries"][kind]
            binary = Path(entry["path"])
            if sha(binary) != entry["sha256"]:
                raise ValueError(f"protected {arm} {kind} binary changed")
            binaries[arm][kind] = binary
    check_non_arena(out / "source", expected)
    attempt = new_output(out / "measurements" / args.attempt)
    deadline = time.monotonic() + 1740
    try:
        selected = {
            arm: {
                fixture: selected_test(binaries[arm][kind], suffix)
                for fixture, (kind, suffix) in FIXTURES.items()
            }
            for arm in binaries
        }
        survey = mpstat("ALL", 30)
        (attempt / "cpu-survey.txt").write_text(survey)
        cpu = choose_cpu(survey)
        write_json(
            attempt / "metadata.json",
            {
                "build_sha256": sha(build_dir / "build.json"),
                "script_sha256": sha(Path(__file__)),
                "source_manifest_sha256": data["manifest_sha256"],
                "binaries": build_data["arms"],
                "tests": selected,
                "cpu": cpu,
                "cpu_governors": {
                    str(number): (
                        governor.read_text().strip()
                        if (
                            governor := Path(
                                f"/sys/devices/system/cpu/cpu{number}/cpufreq/scaling_governor"
                            )
                        ).exists()
                        else "unavailable"
                    )
                    for number in cpu["siblings"]
                },
                "host_uname": dict(
                    zip(
                        ("system", "node", "release", "version", "machine"),
                        os.uname(),
                    )
                ),
                "host_load_average": os.getloadavg(),
                "parent_affinity": sorted(os.sched_getaffinity(0)),
                "survey_sha256": sha(attempt / "cpu-survey.txt"),
                "seed": 9334,
                "warmups_per_arm": 5,
                "pairs_per_fixture": 51,
                "memory_max": "5G",
                "swap_max": 0,
                "runtime_max_seconds": 1800,
            },
        )
        for fixture in FIXTURES:
            for arm in ("strong", "candidate"):
                kind = FIXTURES[fixture][0]
                row = run_test(
                    binaries[arm][kind],
                    selected[arm][fixture],
                    cpu["pinned_cpu"],
                    deadline,
                    60,
                )
                append_row(
                    attempt / "semantics.jsonl", {"arm": arm, "fixture": fixture, **row}
                )
        launch_pairs = []
        for index in range(10):
            order = (
                ("strong", "candidate") if index % 2 == 0 else ("candidate", "strong")
            )
            observed = {}
            for arm in order:
                row = run_test(
                    binaries[arm]["models"],
                    "__page_arena_9334_noop__",
                    cpu["pinned_cpu"],
                    deadline,
                    60,
                )
                append_row(
                    attempt / "launch-only.jsonl",
                    {"index": index, "arm": arm, "order": order, **row},
                )
                observed[arm] = {
                    "wall_ns": row["wall_ns"],
                    "peak_rss_kib": row["resource"]["peak_rss_kib"],
                }
            launch_pairs.append(observed)
        launch_summary = summarize(launch_pairs, ("wall_ns", "peak_rss_kib"), 19334)
        summaries = {}
        for fixture_index, fixture in enumerate(
            ("isolated", "act20k", "width4096", "nested_epathmap")
        ):
            kind = FIXTURES[fixture][0]
            raw = attempt / f"{fixture}.jsonl"
            for index in range(5):
                order = (
                    ("strong", "candidate")
                    if index % 2 == 0
                    else ("candidate", "strong")
                )
                for arm in order:
                    row = run_test(
                        binaries[arm][kind],
                        selected[arm][fixture],
                        cpu["pinned_cpu"],
                        deadline,
                        60,
                    )
                    append_row(
                        raw,
                        {
                            "phase": "warmup",
                            "index": index,
                            "arm": arm,
                            "order": order,
                            **row,
                        },
                    )
            orders = [0] * 26 + [1] * 25
            random.Random(9334 + fixture_index).shuffle(orders)
            pairs = []
            noisy = 0
            for index, reverse in enumerate(orders):
                order = ("candidate", "strong") if reverse else ("strong", "candidate")
                preflight = mpstat(",".join(map(str, cpu["siblings"])), 1)
                idles = average_idle(preflight)
                if set(idles) != set(cpu["siblings"]):
                    raise ValueError(
                        "pair preflight omitted a pinned CPU or SMT sibling"
                    )
                swapped = swap_in_use()
                busy = any(idle < 90 for idle in idles.values())
                noisy += int(swapped or busy)
                observed = {}
                for arm in order:
                    row = run_test(
                        binaries[arm][kind],
                        selected[arm][fixture],
                        cpu["pinned_cpu"],
                        deadline,
                        60,
                    )
                    append_row(
                        raw,
                        {
                            "phase": "sample",
                            "index": index,
                            "arm": arm,
                            "order": order,
                            "preflight_idle": idles,
                            "swap_in_use": swapped,
                            **row,
                        },
                    )
                    observed[arm] = {
                        "wall_ns": row["wall_ns"],
                        "peak_rss_kib": row["resource"]["peak_rss_kib"],
                        **row.get("probe", {}),
                    }
                pairs.append(observed)
            metrics = (
                (
                    "wall_ns",
                    "peak_rss_kib",
                    "append_export_ns",
                    "export_ns",
                    "freeze_ns",
                    "hash_ns",
                )
                if fixture == "isolated"
                else ("wall_ns", "peak_rss_kib")
            )
            summary = summarize(pairs, metrics, 9334 + fixture_index + 1000)
            launch_fraction = max(
                launch_summary["wall_ns"]["strong_median"]
                / summary["wall_ns"]["strong_median"],
                launch_summary["wall_ns"]["candidate_median"]
                / summary["wall_ns"]["candidate_median"],
            )
            guard = (
                "inconclusive_host_noise"
                if noisy > 5
                else "inconclusive_launch_dominated"
                if launch_fraction > 0.5
                else (
                    "within_10pct_secondary_guard"
                    if summary["wall_ns"]["bootstrap_95pct"][1] <= 1.10
                    else "outside_10pct_secondary_guard"
                )
            )
            summaries[fixture] = {
                "pairs": len(pairs),
                "busy_or_swap_pairs": noisy,
                "max_launch_fraction": launch_fraction,
                "metrics": summary,
                "secondary_wall_guard": guard,
            }
        write_json(
            attempt / "summary.json",
            {
                "fixtures": summaries,
                "launch_only": launch_summary,
                "primary_experiment_395_verdict": "pending",
                "full_models_parity": "open",
            },
        )
    except Exception as error:
        write_json(attempt / "measure-error.json", {"error": repr(error)})
        raise
    print(attempt / "summary.json")


def main():
    parser = argparse.ArgumentParser()
    modes = parser.add_subparsers(dest="mode", required=True)
    prepare_mode = modes.add_parser("prepare")
    prepare_mode.add_argument("--out", type=Path, required=True)
    build_mode = modes.add_parser("build")
    build_mode.add_argument("--out", type=Path, required=True)
    build_mode.add_argument("--attempt", type=attempt_name, required=True)
    semantics_mode = modes.add_parser("semantics")
    semantics_mode.add_argument("--out", type=Path, required=True)
    semantics_mode.add_argument("--build-attempt", type=attempt_name, required=True)
    semantics_mode.add_argument("--attempt", type=attempt_name, required=True)
    measure_mode = modes.add_parser("measure")
    measure_mode.add_argument("--out", type=Path, required=True)
    measure_mode.add_argument("--build-attempt", type=attempt_name, required=True)
    measure_mode.add_argument("--attempt", type=attempt_name, required=True)
    args = parser.parse_args()
    os.environ.update(DISPLAY="", WAYLAND_DISPLAY="", LC_ALL="C")
    if args.mode == "prepare":
        prepare(args)
    elif args.mode == "build":
        build(args)
    elif args.mode == "semantics":
        semantics(args)
    else:
        measure(args)


if __name__ == "__main__":
    main()
