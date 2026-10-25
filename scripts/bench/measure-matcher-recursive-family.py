#!/usr/bin/env python3

import argparse
import hashlib
import json
import math
import os
import platform
import random
import re
import statistics
import subprocess
from pathlib import Path

FAMILIES = (
    "New",
    "Send",
    "Receive",
    "Match",
    "ENot",
    "EPlus",
    "EList",
    "ETuple",
    "EMethod",
    "EMatches",
    "ESet",
    "EMap",
    "Connective::Not",
    "Connective::And",
    "Connective::Or",
)
DEPTHS = (0, 128, 512, 2048, 4096)
SAMPLES = 31
BOOTSTRAPS = 10000
SEED = 5866
THRESHOLD = 1.2
ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "target/verification/matcher-recursive-family-5866"
ROW = re.compile(
    r"CURSOR_RESOURCE\t([^\t\n]+)\t(\d+)\t(\d+)\t(\d+)\t(\d+)\t(\d+)\t(\d+)"
)
MATCHER_SOURCES = (
    "models/build.rs",
    "models/src/main/protobuf/RhoTypes.proto",
    "models/src/rust/par_bag.rs",
    "models/src/rust/rholang/match_path.rs",
    "models/src/rust/rholang/match_path_cursor.rs",
    "models/src/rust/rholang/match_region_path.rs",
    "models/src/rust/rholang/match_schema.rs",
    "models/src/rust/rholang/mod.rs",
    "rholang/src/rust/interpreter/matcher/match.rs",
    "rholang/src/rust/interpreter/matcher/spatial_matcher.rs",
    "rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs",
    "rholang/tests/matcher_recursive_family_cursor.rs",
    "rholang/tests/support/counting_alloc.rs",
    "scripts/bench/measure-matcher-recursive-family.py",
)


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def matcher_source_hashes():
    return {name: sha256_file(ROOT / name) for name in MATCHER_SOURCES}


def run(command, log, env=None):
    with log.open("w") as output:
        result = subprocess.run(
            command,
            cwd=ROOT,
            env=env,
            stdout=output,
            stderr=subprocess.STDOUT,
            check=False,
        )
    if result.returncode:
        raise RuntimeError(f"{command!r} failed with {result.returncode}; see {log}")


def build(profile):
    command = [
        "cargo",
        "test",
        "--locked",
        "-p",
        "rholang",
        "--test",
        "matcher_recursive_family_cursor",
        "--no-run",
        "--message-format=json",
    ]
    if profile == "release":
        command.insert(2, "--release")
    log = OUTPUT / f"build-{profile}.log"
    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = "2"
    run(command, log, env)
    executables = []
    for line in log.read_text().splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        target = message.get("target") or {}
        if (
            message.get("reason") == "compiler-artifact"
            and target.get("name") == "matcher_recursive_family_cursor"
            and message.get("executable")
        ):
            executables.append(message["executable"])
    if len(executables) != 1:
        raise RuntimeError(f"expected one {profile} matcher test binary in {log}")
    return executables[0]


def slope(points):
    xs = [math.log(size) for size, _ in points]
    ys = [math.log(value) for _, value in points]
    center_x = statistics.mean(xs)
    center_y = statistics.mean(ys)
    return sum((x - center_x) * (y - center_y) for x, y in zip(xs, ys)) / sum(
        (x - center_x) ** 2 for x in xs
    )


def host_metadata():
    cpu_info = Path("/proc/cpuinfo").read_text()
    cpu_model = next(
        (
            line.split(":", 1)[1].strip()
            for line in cpu_info.splitlines()
            if line.startswith("model name")
        ),
        "unknown",
    )
    governor_path = Path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
    driver_path = Path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver")
    return {
        "uname": platform.uname()._asdict(),
        "cpu_model": cpu_model,
        "affinity": sorted(os.sched_getaffinity(0)),
        "governor": governor_path.read_text().strip()
        if governor_path.exists()
        else None,
        "scaling_driver": driver_path.read_text().strip()
        if driver_path.exists()
        else None,
    }


def main():
    global OUTPUT
    parser = argparse.ArgumentParser(description="Measure recursive matcher resources")
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument(
        "--allow-dirty",
        action="store_true",
        help="record a tracked-source diff digest for an integrated candidate",
    )
    args = parser.parse_args()
    OUTPUT = args.output.resolve()
    OUTPUT.mkdir(parents=True, exist_ok=True)
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    source_diff = subprocess.check_output(
        ["git", "diff", "HEAD", "--binary"], cwd=ROOT
    )
    if source_diff and not args.allow_dirty:
        raise RuntimeError("commit the measurement probe before running the study")
    source_hashes = matcher_source_hashes()
    binaries = {profile: build(profile) for profile in ("release", "debug")}
    if matcher_source_hashes() != source_hashes:
        raise RuntimeError("matcher source changed while the resource binaries were built")
    (OUTPUT / "manifest.json").write_text(
        json.dumps(
            {
                "revision": revision,
                "tracked_source_diff_sha256": hashlib.sha256(source_diff).hexdigest(),
                "tracked_source_dirty": bool(source_diff),
                "matcher_source_sha256": source_hashes,
                "experiment_id": 366,
                "host": host_metadata(),
                "seed": SEED,
                "families": FAMILIES,
                "depths": DEPTHS,
                "samples": SAMPLES,
                "warmups": 3,
                "bootstraps": BOOTSTRAPS,
                "threshold": THRESHOLD,
                "binaries": binaries,
                "binary_sha256": {
                    profile: sha256_file(Path(binary))
                    for profile, binary in binaries.items()
                },
            },
            indent=2,
        )
        + "\n"
    )

    cases = [
        (profile, family, depth)
        for profile in binaries
        for family in FAMILIES
        for depth in DEPTHS
    ]
    random.Random(SEED).shuffle(cases)
    groups = {}
    with (OUTPUT / "samples.tsv").open("w") as samples:
        samples.write(
            "profile\tfamily\tdepth\tinput_bytes\tsample\telapsed_ns\tallocations\tallocated_bytes\tpeak_rss_kib\n"
        )
        for index, (profile, family, depth) in enumerate(cases, 1):
            key = f"{profile}-{FAMILIES.index(family)}-{depth}"
            log = OUTPUT / f"{key}.log"
            peak_log = OUTPUT / f"{key}.rss"
            env = os.environ.copy()
            env["RHOLANG_RECURSIVE_CURSOR_FAMILY"] = family
            env["RHOLANG_RECURSIVE_CURSOR_DEPTH"] = str(depth)
            command = [
                "/usr/bin/time",
                "-f",
                "PEAK_RSS_KIB\t%M",
                "-o",
                str(peak_log),
                binaries[profile],
                "--ignored",
                "--exact",
                "recursive_family_cursor_resource_probe",
                "--nocapture",
                "--test-threads=1",
            ]
            run(command, log, env)
            text = log.read_text()
            rows = ROW.findall(text)
            if len(rows) != SAMPLES:
                raise RuntimeError(
                    f"expected {SAMPLES} samples in {log}, found {len(rows)}"
                )
            if [int(row[3]) for row in rows] != list(range(SAMPLES)):
                raise RuntimeError(f"incomplete sample sequence in {log}")
            if any(row[0] != family or int(row[1]) != depth for row in rows):
                raise RuntimeError(f"wrong fixture in {log}")
            sizes = {int(row[2]) for row in rows}
            if len(sizes) != 1:
                raise RuntimeError(f"unstable encoded input size in {log}")
            peak_match = re.fullmatch(r"PEAK_RSS_KIB\t(\d+)\n?", peak_log.read_text())
            if peak_match is None:
                raise RuntimeError(f"missing process RSS in {peak_log}")
            peak = int(peak_match.group(1))
            groups[profile, family, depth] = (sizes.pop(), peak, rows)
            for row in rows:
                samples.write("\t".join((profile, *row, str(peak))) + "\n")
            samples.flush()
            print(f"{index}/{len(cases)} {profile} {family} {depth}", flush=True)

    rng = random.Random(SEED)
    failures = []
    with (OUTPUT / "fits.tsv").open("w") as fits:
        fits.write(
            "profile\tfamily\twall_exponent\twall_upper95\tallocation_exponent\tallocated_bytes_exponent\tbaseline_rss_kib\tmax_rss_kib\tverdict\n"
        )
        for profile in binaries:
            for family in FAMILIES:
                cells = [groups[profile, family, depth] for depth in DEPTHS[1:]]
                sizes = [cell[0] for cell in cells]
                if sizes != sorted(sizes) or len(set(sizes)) != len(sizes):
                    raise RuntimeError(
                        f"encoded input is not increasing for {profile} {family}"
                    )
                wall_samples = [
                    [max(1, int(row[4])) for row in cell[2]] for cell in cells
                ]
                wall = slope(
                    [
                        (size, statistics.median(values))
                        for size, values in zip(sizes, wall_samples)
                    ]
                )
                bootstrap = sorted(
                    slope(
                        [
                            (size, statistics.median(rng.choices(values, k=SAMPLES)))
                            for size, values in zip(sizes, wall_samples)
                        ]
                    )
                    for _ in range(BOOTSTRAPS)
                )
                upper = bootstrap[math.ceil(0.95 * BOOTSTRAPS) - 1]
                alloc = slope(
                    [
                        (size, statistics.median(int(row[5]) for row in cell[2]))
                        for size, cell in zip(sizes, cells)
                    ]
                )
                allocated = slope(
                    [
                        (size, statistics.median(int(row[6]) for row in cell[2]))
                        for size, cell in zip(sizes, cells)
                    ]
                )
                baseline_rss = groups[profile, family, 0][1]
                max_rss = max(cell[1] for cell in cells)
                verdict = (
                    "PASS"
                    if upper <= THRESHOLD
                    and alloc <= THRESHOLD
                    and allocated <= THRESHOLD
                    else "FAIL"
                )
                if verdict == "FAIL":
                    failures.append((profile, family))
                fits.write(
                    f"{profile}\t{family}\t{wall:.6f}\t{upper:.6f}\t{alloc:.6f}\t{allocated:.6f}\t{baseline_rss}\t{max_rss}\t{verdict}\n"
                )
    if failures:
        raise RuntimeError(
            f"resource exponent threshold failed for {failures}; see {OUTPUT / 'fits.tsv'}"
        )
    if matcher_source_hashes() != source_hashes:
        raise RuntimeError("matcher source changed during the resource study")


if __name__ == "__main__":
    main()
