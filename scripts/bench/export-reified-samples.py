import argparse
import json
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def export(path: Path, start: int, limit: int) -> None:
    result = []
    with tarfile.open(path) as archive:
        samples = sorted(
            member.name
            for member in archive.getmembers()
            if member.name.endswith("/new/sample.json")
        )
        for name in samples[start : start + limit]:
            member = archive.extractfile(name)
            if member is None:
                raise ValueError(f"missing sample file: {name}")
            sample = json.load(member)
            if len(sample["iters"]) != 100 or len(sample["times"]) != 100:
                raise ValueError(f"invalid sample count: {name}")
            result.append(
                {
                    "case": name.removeprefix("criterion/").removesuffix(
                        "/new/sample.json"
                    ),
                    "sample_path": name,
                    "samples_ns": [
                        time / iterations
                        for time, iterations in zip(sample["times"], sample["iters"])
                    ],
                }
            )
    print(json.dumps(result, separators=(",", ":")))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("archive")
    parser.add_argument("start", type=int)
    parser.add_argument("limit", type=int)
    arguments = parser.parse_args()
    export(ROOT / arguments.archive, arguments.start, arguments.limit)
