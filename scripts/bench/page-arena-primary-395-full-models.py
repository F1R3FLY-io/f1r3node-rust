import argparse
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PRIMARY = HERE / "page-arena-primary-395.py"
BUILD_SHA256 = "2379e8eab70cfac189bf05cdbba57650d4bfdc69b637b450ab66c03f1ffeb341"
SCHEMA_SHA256 = "73a551815a287983bf04e25331eeedcb01f9e683de902bf91bc7f03e75ba5269"
LEGACY_SCRIPT_SHA256 = "efc147aeb976e2932a4f18f4883aa558b665819f7870ba1bb9c5611f0da79c94"
CAPACITY = 64
ARMS = ("strong", "candidate")

spec = importlib.util.spec_from_file_location("page_arena_primary_395", PRIMARY)
primary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(primary)
base = primary.base


def pinned(out, expected_primary_build_sha256=BUILD_SHA256):
    data, expected = primary.prepared(out)
    _, digest = primary.built(out, "b2", data)
    if digest != expected_primary_build_sha256:
        raise ValueError("primary b2 build differs from the supplied receipt")
    manifest = tomllib.loads((out / "source/models/Cargo.toml").read_text())
    if manifest["package"]["autotests"] is not False:
        raise ValueError("models integration target registry changed")
    entries = manifest["test"]
    targets = {item["name"] for item in entries}
    paths = [Path(item["path"]) for item in entries]
    if (len(entries) < 52 or len(targets) != len(entries)
            or len(set(paths)) != len(entries) or "models" in targets):
        raise ValueError("models integration target registry lost or duplicated a target")
    for path in paths:
        if (path.is_absolute() or not path.parts or path.parts[0] != "tests"
                or ".." in path.parts or path.suffix != ".rs"):
            raise ValueError(f"models integration target path is not a test source: {path}")
        relative = Path("models") / path
        source = out / "source" / relative
        if source.is_symlink() or not source.is_file() or expected.get(relative.as_posix()) != base.sha(source):
            raise ValueError(f"models integration target source differs from snapshot: {path}")
    return data, expected, targets | {"models"}


def artifacts(log, snapshot, private_target, expected_names):
    found = {}
    models_manifest = (snapshot / "models/Cargo.toml").resolve()
    for line in log.read_text().splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("reason") != "compiler-artifact" or not event.get("executable"):
            continue
        if Path(event.get("manifest_path", "")).resolve() != models_manifest:
            continue
        target = event["target"]
        name = target["name"]
        kind = target["kind"]
        if not ("test" in kind or (name == "models" and "lib" in kind)):
            continue
        path = Path(event["executable"]).resolve()
        if not path.is_relative_to(private_target) or not path.is_file():
            raise ValueError(f"unprotected models artifact path: {path}")
        if name in found and found[name] != path:
            raise ValueError(f"two executables reported for {name}")
        found[name] = path
    if set(found) != expected_names:
        raise ValueError(
            f"models target mismatch: missing={sorted(expected_names - set(found))}, "
            f"extra={sorted(set(found) - expected_names)}"
        )
    return found


def build_arm(out, attempt, arm, private_target, data, expected, names, first_schema, expected_schema_sha256):
    snapshot = out / "source"
    arena = snapshot / base.ARENA
    variant = Path(data["variants"][str(CAPACITY)][arm]["source"])
    shutil.copyfile(variant, arena)
    if base.sha(arena) != data["variants"][str(CAPACITY)][arm]["source_sha256"]:
        raise ValueError(f"{arm} arena variant did not install exactly")
    base.check_non_arena(snapshot, expected)
    base.check_patched_dependency(out, data)
    cell = attempt / arm
    cell.mkdir()
    argv = [
        "systemd-run", "--user", "--scope", "--collect",
        f"--working-directory={snapshot}",
        "-p", "MemoryMax=5G", "-p", "MemorySwapMax=0",
        "-p", "CPUQuota=100%", "-p", "TasksMax=256",
        "-p", "RuntimeMaxSec=1800",
        f"--setenv=CARGO_TARGET_DIR={private_target}",
        "--setenv=CARGO_INCREMENTAL=0", "--setenv=DISPLAY=",
        "--setenv=WAYLAND_DISPLAY=",
        "--setenv=JAVA_TOOL_OPTIONS=-Djava.awt.headless=true",
        "--setenv=LC_ALL=C",
        "cargo", "test", "--locked", "-p", "models", "--release",
        "--lib", "--tests", "--no-run", "--message-format=json",
    ]
    base.write_json(cell / "command.json", argv)
    log = cell / "build.log"
    started_ns = time.time_ns()
    with log.open("w") as stream:
        result = subprocess.run(
            argv, stdout=stream, stderr=subprocess.STDOUT, timeout=1830, check=False
        )
    base.check_non_arena(snapshot, expected)
    base.check_patched_dependency(out, data)
    if result.returncode != 0:
        raise RuntimeError(f"{arm} all-target build exit {result.returncode}: {log}")
    build_root = private_target / "release/build"
    schemas = sorted(build_root.glob("models-*/out/rhoapi_bincode_schema.rs"))
    scripts = sorted(build_root.glob("models-*/build-script-build"))
    if not schemas or not scripts:
        raise ValueError("models build-script/schema missing")
    schema = max(schemas, key=lambda path: path.stat().st_mtime_ns)
    script = max(scripts, key=lambda path: path.stat().st_mtime_ns)
    if first_schema is None and (
        schema.stat().st_mtime_ns < started_ns
        or script.stat().st_mtime_ns < started_ns
    ):
        raise ValueError("strong models build-script/schema was not freshly produced")
    if base.sha(schema) != expected_schema_sha256 or b"CS_" not in schema.read_bytes():
        raise ValueError("generated models schema differs from the supplied receipt")
    if first_schema is not None and base.sha(schema) != first_schema:
        raise ValueError("strong/candidate generated schemas differ")
    executable_paths = artifacts(log, snapshot, private_target, names)
    protected = {}
    for name, source in sorted(executable_paths.items()):
        destination = cell / name
        shutil.copy2(source, destination)
        protected[name] = {"path": str(destination), "sha256": base.sha(destination)}
    shutil.copy2(schema, cell / "rhoapi_bincode_schema.rs")
    shutil.copy2(script, cell / "models-build-script-build")
    return {
        "arena_sha256": base.sha(variant),
        "build_log_sha256": base.sha(log),
        "generated_schema_sha256": base.sha(cell / "rhoapi_bincode_schema.rs"),
        "build_script_sha256": base.sha(cell / "models-build-script-build"),
        "executables": protected,
    }


def build(args):
    out = args.out.resolve()
    data, expected, names = pinned(out, args.primary_build_sha256)
    free = shutil.disk_usage(ROOT).free
    if free < 25 * 1024**3:
        raise ValueError(f"less than 25 GiB free before all-target build: {free}")
    attempt = base.new_output(out / "full-models-parity" / args.attempt)
    try:
        private_target, copied_bytes = base.private_release_target(attempt, free)
        report = {
            "script_sha256": base.sha(Path(__file__)),
            "prepared_sha256": base.sha(out / "prepared.json"),
            "primary_build_sha256": args.primary_build_sha256,
            "expected_generated_schema_sha256": args.schema_sha256,
            "source_manifest_sha256": data["manifest_sha256"],
            "patched_dependency_manifest_sha256": data["patched_dependency_manifest_sha256"],
            "target_clone_metadata_sha256": base.sha(attempt / "target-clone-metadata.json"),
            "quarantine_sha256": base.sha(attempt / "quarantine.json"),
            "private_target_source_bytes": copied_bytes,
            "expected_targets": sorted(names),
            "arms": {},
        }
        for arm in ARMS:
            report["arms"][arm] = build_arm(
                out, attempt, arm, private_target, data, expected, names,
                report["arms"].get("strong", {}).get("generated_schema_sha256"),
                args.schema_sha256,
            )
            base.write_json(attempt / "build-progress.json", report)
        base.write_json(attempt / "build.json", report)
    except Exception as error:
        base.write_json(attempt / "build-error.json", {"error": repr(error)})
        raise
    finally:
        shutil.copyfile(out / "candidate-arena.rs", out / "source" / base.ARENA)
    pinned(out, args.primary_build_sha256)
    print(attempt / "build.json")


def built(out, attempt, data, names, expected_primary_build_sha256=BUILD_SHA256,
          expected_schema_sha256=SCHEMA_SHA256):
    path = out / "full-models-parity" / attempt / "build.json"
    report = json.loads(path.read_text())
    legacy = (
        attempt == "p1"
        and expected_primary_build_sha256 == BUILD_SHA256
        and expected_schema_sha256 == SCHEMA_SHA256
        and report["script_sha256"] == LEGACY_SCRIPT_SHA256
        and "expected_generated_schema_sha256" not in report
    )
    if (
        not (legacy or report["script_sha256"] == base.sha(Path(__file__)))
        or report["prepared_sha256"] != base.sha(out / "prepared.json")
        or report["source_manifest_sha256"] != data["manifest_sha256"]
        or report["patched_dependency_manifest_sha256"] != data["patched_dependency_manifest_sha256"]
        or report["primary_build_sha256"] != expected_primary_build_sha256
        or (not legacy and report.get("expected_generated_schema_sha256") != expected_schema_sha256)
        or report["target_clone_metadata_sha256"] != base.sha(out / "full-models-parity" / attempt / "target-clone-metadata.json")
        or report["quarantine_sha256"] != base.sha(out / "full-models-parity" / attempt / "quarantine.json")
        or set(report["arms"]) != set(ARMS)
        or set(report["expected_targets"]) != names
    ):
        raise ValueError("all-target build provenance changed")
    for arm in ARMS:
        item = report["arms"][arm]
        if item["arena_sha256"] != data["variants"][str(CAPACITY)][arm]["source_sha256"]:
            raise ValueError(f"protected {arm} arena variant changed")
        if item["generated_schema_sha256"] != expected_schema_sha256:
            raise ValueError("all-target generated schema changed")
        cell = out / "full-models-parity" / attempt / arm
        if base.sha(cell / "rhoapi_bincode_schema.rs") != item["generated_schema_sha256"]:
            raise ValueError(f"protected {arm} generated schema changed")
        if base.sha(cell / "models-build-script-build") != item["build_script_sha256"]:
            raise ValueError(f"protected {arm} build-script changed")
        if set(item["executables"]) != names:
            raise ValueError("all-target protected executable set changed")
        for name, entry in item["executables"].items():
            if base.sha(Path(entry["path"])) != entry["sha256"]:
                raise ValueError(f"protected {arm}/{name} executable changed")
        if base.sha(out / "full-models-parity" / attempt / arm / "build.log") != item["build_log_sha256"]:
            raise ValueError(f"protected {arm} build log changed")
    return report, base.sha(path)


def run_inner(args):
    if os.environ.get("PAGE_ARENA_FULL_MODELS_SCOPED") != "1":
        raise ValueError("full models parity requires its bounded headless scope")
    out = args.out.resolve()
    data, _, names = pinned(out, args.primary_build_sha256)
    build_report, build_sha = built(out, args.build_attempt, data, names,
                                   args.primary_build_sha256, args.schema_sha256)
    attempt = out / "full-models-parity" / args.build_attempt / "runs" / args.attempt
    if not attempt.is_dir():
        raise ValueError("full models run attempt was not prepared")
    deadline = time.monotonic() + 7170
    completed = []
    env = os.environ.copy()
    env.update(LC_ALL="C", DISPLAY="", WAYLAND_DISPLAY="", JAVA_TOOL_OPTIONS="-Djava.awt.headless=true")
    env.pop("EPATHMAP_CANONICAL_GOLDENS_BLESS", None)
    base.write_json(attempt / "metadata.json", {
        "build_sha256": build_sha,
        "script_sha256": base.sha(Path(__file__)),
        "target_names": sorted(names),
        "timeout_seconds_per_target": 900,
        "deadline_seconds": 7170,
    })
    try:
        for arm in ARMS:
            for name in ["models", *sorted(names - {"models"})]:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("full models parity scope deadline reached")
                binary = Path(build_report["arms"][arm]["executables"][name]["path"])
                log = attempt / f"{arm}-{name}.log"
                argv = [str(binary), "--test-threads=1"]
                with log.open("w") as stream:
                    result = subprocess.run(
                        argv, stdout=stream, stderr=subprocess.STDOUT,
                        timeout=min(900, remaining), env=env, check=False,
                    )
                if log.stat().st_size > 32 * 1024**2:
                    raise ValueError(f"unbounded test log: {log}")
                row = {
                    "arm": arm, "target": name, "command": argv,
                    "exit_code": result.returncode, "log_sha256": base.sha(log),
                }
                base.append_row(attempt / "results.jsonl", row)
                completed.append(row)
                if result.returncode != 0:
                    raise RuntimeError(f"{arm}/{name} failed with exit {result.returncode}: {log}")
        base.write_json(attempt / "summary.json", {
            "completed": len(completed),
            "expected": 2 * len(names),
            "results_sha256": base.sha(attempt / "results.jsonl"),
            "full_models_parity": "passed",
        })
    except Exception as error:
        base.write_json(attempt / "error.json", {
            "completed": len(completed), "expected": 2 * len(names),
            "error": repr(error),
        })
        raise
    print(attempt / "summary.json")


def run(args):
    out = args.out.resolve()
    data, _, names = pinned(out, args.primary_build_sha256)
    built(out, args.build_attempt, data, names,
          args.primary_build_sha256, args.schema_sha256)
    attempt = base.new_output(out / "full-models-parity" / args.build_attempt / "runs" / args.attempt)
    argv = [
        "systemd-run", "--user", "--scope", "--collect",
        f"--working-directory={ROOT}",
        "-p", "MemoryMax=5G", "-p", "MemorySwapMax=0",
        "-p", "CPUQuota=100%", "-p", "TasksMax=256",
        "-p", "RuntimeMaxSec=7200",
        "--setenv=PAGE_ARENA_FULL_MODELS_SCOPED=1",
        "--setenv=DISPLAY=", "--setenv=WAYLAND_DISPLAY=",
        "--setenv=JAVA_TOOL_OPTIONS=-Djava.awt.headless=true",
        "--setenv=LC_ALL=C",
        sys.executable, str(Path(__file__).resolve()), "run-inner",
        "--out", str(out), "--build-attempt", args.build_attempt,
        "--attempt", args.attempt,
        "--primary-build-sha256", args.primary_build_sha256,
        "--schema-sha256", args.schema_sha256,
    ]
    base.write_json(attempt / "command.json", argv)
    with (attempt / "scope.log").open("w") as stream:
        result = subprocess.run(
            argv, stdout=stream, stderr=subprocess.STDOUT,
            timeout=7230, check=False,
        )
    base.write_json(attempt / "scope-result.json", {
        "exit_code": result.returncode,
        "scope_log_sha256": base.sha(attempt / "scope.log"),
    })
    if result.returncode != 0:
        raise RuntimeError(f"full models parity scope exit {result.returncode}")
    print(attempt / "summary.json")


def main():
    parser = argparse.ArgumentParser()
    modes = parser.add_subparsers(dest="mode", required=True)
    for mode in ("build", "run", "run-inner"):
        command = modes.add_parser(mode)
        command.add_argument("--out", type=Path, required=True)
        command.add_argument("--attempt", type=base.attempt_name, required=True)
        command.add_argument("--primary-build-sha256", default=BUILD_SHA256)
        command.add_argument("--schema-sha256", default=SCHEMA_SHA256)
        if mode != "build":
            command.add_argument("--build-attempt", type=base.attempt_name, required=True)
    args = parser.parse_args()
    {"build": build, "run": run, "run-inner": run_inner}[args.mode](args)


if __name__ == "__main__":
    main()
