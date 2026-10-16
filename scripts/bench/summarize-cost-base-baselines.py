import hashlib
import json
import re
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VECTORS = ROOT / "test-vectors"
PROTOCOL = json.loads(
    (VECTORS / "benchmarks/reified-rspaces-protocol-v1.json").read_text()
)
ARCHIVES = {
    "model_lmdb": VECTORS / "cost-base-model-lmdb-068d.tar.gz",
    "rspace": VECTORS / "cost-base-rspace-068d.tar.gz",
}
LOGS = {
    "model": (
        "model_lmdb",
        "cost-base-model-timed-run.log",
        "cost-base-model-idle-before-run.log",
    ),
    "lmdb": (
        "model_lmdb",
        "cost-base-lmdb-timed-run.log",
        "cost-base-lmdb-idle-before-run.log",
    ),
    "rspace": (
        "rspace",
        "cost-base-rspace-timed-run.log",
        "cost-base-rspace-idle-retry-4.log",
    ),
}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def archive_file(archive: tarfile.TarFile, name: str) -> bytes:
    member = archive.extractfile(name)
    if member is None:
        raise ValueError(f"missing archive file: {name}")
    return member.read()


def cases(archive: tarfile.TarFile) -> list[dict]:
    result = []
    for member in archive.getmembers():
        if not member.name.endswith("/new/sample.json"):
            continue
        sample = json.loads(archive_file(archive, member.name))
        estimate_path = member.name.replace("/sample.json", "/estimates.json")
        estimates = json.loads(archive_file(archive, estimate_path))
        count = len(sample["iters"])
        if count != 100 or len(sample["times"]) != count:
            raise ValueError(f"invalid sample count: {member.name}")
        result.append(
            {
                "name": member.name.removesuffix("/new/sample.json"),
                "raw_sample_path": member.name,
                "sample_count": count,
                "mean_ns": estimates["mean"]["point_estimate"],
                "median_ns": estimates["median"]["point_estimate"],
            }
        )
    return sorted(result, key=lambda case: case["name"])


def run_record(archive: tarfile.TarFile, log_name: str, idle_name: str) -> dict:
    log = archive_file(archive, log_name)
    idle = archive_file(archive, idle_name)
    text = log.decode()
    rss_match = re.search(r"Maximum resident set size \(kbytes\): (\d+)", text)
    status_match = re.search(r"Exit status: (\d+)", text)
    cpu_idle = {}
    for line in idle.decode().splitlines():
        fields = line.split()
        if len(fields) > 3 and fields[0] == "Average:" and fields[1] in ("0", "1"):
            cpu_idle[fields[1]] = float(fields[-1])
    if rss_match is None or status_match is None or status_match.group(1) != "0":
        raise ValueError(f"invalid benchmark log: {log_name}")
    if set(cpu_idle) != {"0", "1"} or min(cpu_idle.values()) < 90:
        raise ValueError(f"invalid CPU idle gate: {idle_name}")
    return {
        "exit_code": 0,
        "run_log_sha256": digest(log),
        "idle_log_sha256": digest(idle),
        "pre_run_idle_percent_by_cpu": cpu_idle,
        "time_max_rss_kib": int(rss_match.group(1)),
    }


def main() -> None:
    for count in (10000, 100000):
        source = PROTOCOL["inputs"][f"atoms_{count}"]
        if digest((ROOT / source["path"]).read_bytes()) != source["sha256"]:
            raise ValueError(f"input hash differs: {count}")
    for key in ("model_codec_harness", "lmdb_harness", "rspace_harness"):
        source = PROTOCOL[key]
        if digest((ROOT / source["source"]).read_bytes()) != source["sha256"]:
            raise ValueError(f"harness hash differs: {key}")
    with (
        tarfile.open(ARCHIVES["model_lmdb"]) as model_lmdb_archive,
        tarfile.open(ARCHIVES["rspace"]) as rspace_archive,
    ):
        archives = {"model_lmdb": model_lmdb_archive, "rspace": rspace_archive}
        case_groups = {name: cases(archive) for name, archive in archives.items()}
        if len(case_groups["model_lmdb"]) != 16 or len(case_groups["rspace"]) != 7:
            raise ValueError("unexpected benchmark case count")
        runs = {
            name: run_record(archives[archive_name], log_name, idle_name)
            for name, (archive_name, log_name, idle_name) in LOGS.items()
        }
        report = {
            "source_revision": PROTOCOL["sources"]["cost_base"]["revision"],
            "toolchain": PROTOCOL["execution"]["cost_base_rustc_version"],
            "cpu_affinity": PROTOCOL["execution"]["cpu_affinity"],
            "cpu_governor": PROTOCOL["execution"]["cpu_governor"],
            "memory_limit_bytes": PROTOCOL["execution"]["memory_limit_bytes"],
            "input_sha256": {
                str(count): PROTOCOL["inputs"][f"atoms_{count}"]["sha256"]
                for count in (10000, 100000)
            },
            "harness_sha256": {
                "model_codec": PROTOCOL["model_codec_harness"]["sha256"],
                "lmdb": PROTOCOL["lmdb_harness"]["sha256"],
                "rspace_source": PROTOCOL["rspace_harness"]["sha256"],
                "rspace_prepared": PROTOCOL["rspace_harness"][
                    "cost_base_prepared_sha256"
                ],
            },
            "commands": {
                "model": PROTOCOL["model_codec_harness"]["cost_base_command"],
                "lmdb": PROTOCOL["lmdb_harness"]["cost_base_command"],
                "rspace": PROTOCOL["rspace_harness"]["cost_base_command"],
            },
            "runs": runs,
            "archives": {
                name: {
                    "path": str(path.relative_to(ROOT)),
                    "sha256": digest(path.read_bytes()),
                    "cases": len(case_groups[name]),
                    "raw_samples": sum(
                        case["sample_count"] for case in case_groups[name]
                    ),
                }
                for name, path in ARCHIVES.items()
            },
            "cases": sorted(
                case_groups["model_lmdb"] + case_groups["rspace"],
                key=lambda case: case["name"],
            ),
            "shape_coverage": {
                "measured": [
                    "wide flat Par",
                    "shared symbol prefix",
                    "ordered Par expression sequence",
                ],
                "unsupported": [
                    "deep nested Par",
                    "duplicate-heavy Par",
                    "stable-tie sorter",
                ],
                "source_evidence": [
                    PROTOCOL["model_codec_harness"]["source"],
                    PROTOCOL["rspace_harness"]["source"],
                ],
            },
        }
        output = VECTORS / "cost-base-baselines-068d.json"
        output.write_text(json.dumps(report, indent=2) + "\n")
        print(f"{output.relative_to(ROOT)}: {len(report['cases'])} cases")


if __name__ == "__main__":
    main()
