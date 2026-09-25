import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import subprocess
import time

from benchmark_host import cpu_snapshot, physical_topology


TEST = "protobuf_decoder_oracle::measurement::measure_decoder_cell"
WITNESS_FIELDS = (
    "protobuf_digest", "compact_digest", "bincode_digest", "locally_free",
    "connective_used", "cardinality", "error", "remaining",
)


def records(output):
    found = {}
    for line in output.splitlines():
        prefix = f"test {TEST} ... "
        if line.startswith(prefix):
            line = line[len(prefix):]
        if line.startswith("DECODER_"):
            tag, body = line.split("\t", 1)
            if tag in found:
                raise ValueError(f"duplicate {tag}")
            found[tag] = json.loads(body)
    return found


def validate_pair(left, right):
    for key in ["shape", "size", "entry", "input", "sample", "input_bytes", "input_digest"]:
        if left["DECODER_BEGIN"][key] != right["DECODER_BEGIN"][key]:
            raise ValueError(f"pair input mismatch: {key}")
    for item in (left, right):
        result = item["DECODER_RESULT"]
        witness = item["DECODER_WITNESS"]
        if result["remaining"] != witness["remaining"] or result["success"] != (witness["error"] is None):
            raise ValueError("timed/untimed observation mismatch")
    for key in WITNESS_FIELDS:
        if left["DECODER_WITNESS"][key] != right["DECODER_WITNESS"][key]:
            raise ValueError(f"frozen/planned witness mismatch: {key}")


def collect(args):
    root = Path(__file__).resolve().parents[2]
    binary = args.binary.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source_paths = [Path(__file__), root / "scripts/bench/benchmark_host.py",
                    root / "models/tests/support/protobuf_decoder_measurement.rs",
                    root / "models/tests/support/protobuf_decoder_oracle.rs",
                    root / "models/tests/support/protobuf_decoder_frozen.rs",
                    root / "models/tests/support/epm_payload_mapping.rs",
                    root / "models/codegen/schema.rs"]
    metadata = {
        "git_ref": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "binary": str(binary.relative_to(root)),
        "binary_sha256": hashlib.file_digest(binary.open("rb"), "sha256").hexdigest(),
        "sources": {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in source_paths},
        "cpu": args.cpu, "topology": str(physical_topology()),
        "lscpu": subprocess.check_output(["lscpu"], text=True),
        "uname": subprocess.check_output(["uname", "-a"], text=True).strip(),
        "governor": Path(f"/sys/devices/system/cpu/cpu{args.cpu}/cpufreq/scaling_governor").read_text().strip(),
        "memory_high": "4G", "memory_max": "6G", "swap_max": 0,
        "runtime_seconds": args.seconds, "pairs": args.pairs, "warmup_pairs": 2 if args.pairs else 0,
        "shapes": args.shapes, "sizes": args.sizes, "entries": args.entries, "inputs": args.inputs,
        "seed": 8516, "instrumented": True,
    }
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    with (output / "observations.jsonl").open("w") as rows:
        cells = itertools.product(args.shapes.split(","), map(int, args.sizes.split(",")),
                                  args.entries.split(","), args.inputs.split(","))
        for cell_index, (shape, size, entry, form) in enumerate(cells):
            rounds = [("pilot", 0)] if not args.pairs else [
                *(('warmup', n) for n in range(2)), *(('sample', n) for n in range(args.pairs))]
            for phase, sample in rounds:
                raw, idle = cpu_snapshot(args.cpu)
                (output / f"cpu-{cell_index}-{phase}-{sample}.json").write_text(raw)
                if not idle:
                    raise RuntimeError("CPU admission refused; retain partial run and choose an idle physical core")
                completed = []
                arms = ["frozen", "planned"] if (cell_index + sample) % 2 == 0 else ["planned", "frozen"]
                for arm in arms:
                    name = f"{cell_index}-{phase}-{sample}-{arm}"
                    env = [f"DECODER_MEASURE_{key}={value}" for key, value in {
                        "ARM": arm, "SHAPE": shape, "SIZE": size, "ENTRY": entry,
                        "INPUT": form, "SAMPLE": f"{phase}-{sample}",
                    }.items()]
                    command = ["systemd-run", "--user", "--wait", "--pipe", "--collect",
                        f"--unit=decoder-8516-{os.getpid()}-{name}",
                        "-p", "MemoryHigh=4G", "-p", "MemoryMax=6G", "-p", "MemorySwapMax=0",
                        "-p", "CPUQuota=100%", "-p", f"RuntimeMaxSec={args.seconds}", "-p", "LimitCORE=0",
                        f"--working-directory={root}", "env", *env, "taskset", "-c", str(args.cpu),
                        str(binary), "--exact", TEST, "--ignored", "--nocapture", "--test-threads=1"]
                    receipt = output / f"{name}.log"
                    start = time.monotonic_ns()
                    with receipt.open("w") as log:
                        result = subprocess.run(command, cwd=root, stdout=log, stderr=subprocess.STDOUT)
                    process_ns = time.monotonic_ns() - start
                    content = receipt.read_text()
                    observed = records(content)
                    if result.returncode == 0 and set(observed) != {"DECODER_BEGIN", "DECODER_RESULT", "DECODER_WITNESS"}:
                        raise ValueError("successful process lacks complete instrument records")
                    status = "complete" if result.returncode == 0 else (
                        "incomplete_witness" if "DECODER_RESULT" in observed else (
                            "incomplete_decode" if "DECODER_BEGIN" in observed else "incomplete_setup"))
                    row = {"name": name, "shape": shape, "size": size, "entry": entry,
                        "input": form, "phase": phase, "sample": sample, "arm": arm,
                        "status": status, "exit_code": result.returncode, "process_ns": process_ns,
                        "receipt_sha256": hashlib.sha256(receipt.read_bytes()).hexdigest(),
                        "command": command, "records": observed}
                    rows.write(json.dumps(row) + "\n")
                    rows.flush()
                    print(f"{name}: {shape}/{size}/{entry}/{form} {status}", flush=True)
                    if status == "complete":
                        completed.append(observed)
                    elif "Finished with result: timeout" not in content:
                        raise RuntimeError(f"non-timeout failure: {receipt}")
                if len(completed) == 2:
                    validate_pair(*completed)
                elif args.pairs:
                    raise RuntimeError("censored sampling pair; retain partial evidence, do not fabricate ratio")


def self_test():
    import copy
    witness = dict.fromkeys(WITNESS_FIELDS, 0)
    witness["error"] = None
    begin = dict.fromkeys(["shape", "size", "entry", "input", "sample", "input_bytes", "input_digest"], 1)
    value = {"DECODER_BEGIN": begin, "DECODER_RESULT": {"remaining": 0, "success": True}, "DECODER_WITNESS": witness}
    text = "\n".join(f"{k}\t{json.dumps(v)}" for k, v in value.items())
    assert records(text) == value
    assert records(f"test {TEST} ... " + text) == value
    validate_pair(value, copy.deepcopy(value))
    for key in WITNESS_FIELDS:
        changed = copy.deepcopy(value)
        changed["DECODER_WITNESS"][key] = "changed"
        try:
            validate_pair(value, changed)
        except ValueError:
            pass
        else:
            raise AssertionError(f"ignored witness mutation: {key}")
    for invalid in [text + "\n" + text]:
        try:
            records(invalid)
        except ValueError:
            pass
        else:
            raise AssertionError("duplicate records accepted")
    print("decoder resource runner self-tests passed")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--cpu", type=int)
    parser.add_argument("--shapes", default="enot")
    parser.add_argument("--sizes", default="16,64,256,1024,2048,4096")
    parser.add_argument("--entries", default="par")
    parser.add_argument("--inputs", default="fragmented")
    parser.add_argument("--seconds", type=int, default=180)
    parser.add_argument("--pairs", type=int, choices=[0, 7, 31], default=0)
    args = parser.parse_args()
    if args.self_test:
        self_test()
    else:
        if args.binary is None or args.output is None or args.cpu is None:
            parser.error("--binary, --output and --cpu are required")
        if args.seconds <= 0:
            parser.error("--seconds must be positive")
        collect(args)
