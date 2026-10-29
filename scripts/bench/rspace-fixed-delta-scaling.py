"""Measurement driver for task 7929's preregistered fixed-delta trusted persistence scaling experiment.

Collection launches the release rspace++ lib test binary's ignored ``scale_child`` once per child
(build, verify or time mode), pinned with taskset and admitted by CPU idle snapshots, validates every
``SCALE_`` record strictly against the v1 record contract, retains every failure and writes the
summary tables.  Analysis evaluates H1 and gates G1-G10 of the reviewed plan (Part 2, sections
5.5-5.7) from the summaries alone, so ``--analyze-only --check`` reproduces it from committed files.
"""

import argparse
from collections import Counter, defaultdict
import contextlib
import dataclasses
import datetime
import fnmatch
import functools
import hashlib
import io
import json
import math
import os
from pathlib import Path
import random
import re
import shlex
import shutil
import stat
import statistics
import subprocess
import sys
import tempfile
import time

import numpy as np

SCRIPT_DIRECTORY = Path(__file__).resolve().parent
REPOSITORY_ROOT = SCRIPT_DIRECTORY.parents[1]
sys.path.insert(0, str(SCRIPT_DIRECTORY))

from benchmark_host import cpu_snapshot
from paired_benchmark_statistics import stratified_loglog_slope

TASK = 7929
SEED = 7929
REPLICATES = 10_000
IDLE_SNAPSHOTS = 30
CHILD_TEST = "rspace::persistence::tests::trusted_scaling_resource::scale_child"
CHILD_ARGUMENTS = ("--exact", CHILD_TEST, "--ignored", "--nocapture", "--quiet", "--test-threads=1")
CHILD_ENVIRONMENT = ("RSPACE_SCALE_CELL", "RSPACE_SCALE_MODE", "RSPACE_SCALE_FIXTURE", "RSPACE_SCALE_SCRATCH")
RECORD_PREFIX = "SCALE_"
RECORD_VERSION = "v1"
U64_MAX = 2**64 - 1
OOM_STATUSES = (137, -9)
PROBLEM_LIMIT = 50
SOURCE_FILES = (
    "scripts/bench/rspace-fixed-delta-scaling.py",
    "scripts/bench/paired_benchmark_statistics.py",
    "scripts/bench/benchmark_host.py",
    "rspace++/tests/persistence/trusted_scaling_resource.rs",
    "rspace++/tests/persistence/trusted_scaling_properties.rs",
)

EXPONENT_BOUND = 0.25
LINEAR_WORK_BOUND = 1.25
RSS_GROWTH_BOUND = 1_048_576
METADATA_BYTE_BOUND = 518_500
METADATA_STEP_BOUND = 103
V1_MANIFEST_BASE_BYTES = 96
V1_MANIFEST_ENTRY_BYTES = 56
C_STAR_QUARTER_POWERS = range(-40, 81)
C_STAR_GRID = tuple(2.0 ** (power / 4) for power in C_STAR_QUARTER_POWERS)

WARMUP_ROUNDS = 3
DECISION_ROUNDS = 31
DECISION_VERIFY_RUNS = 2
PILOT_TIME_PROCESSES = 2
CONTROL_TIME_PROCESSES = 5
CONTROLS = ("NC-SCAN", "NC-CLONE", "NC-ORDER", "NC-V1")
SAMPLE_POPULATIONS = {"decision": "decision", "pilot": "pilot", "controls": "control"}
DECISION_GATES = ("H1", "G1", "G2", "G3", "G4", "G5", "G6", "G7", "G8", "G9")
CONTROL_EXPECTATIONS = {
    "NC-SCAN": "the CPU and rchar exponents' lower bounds exceed 0.25 on U and R; G1 still passes",
    "NC-CLONE": "the allocation and CPU exponents' lower bounds exceed 0.25",
    "NC-ORDER": "G1 and SW-01 fail at every cell, and the CPU exponent's lower bound on U exceeds 0.25",
    "NC-V1": "on R, metadata bytes equal 96 + 56n and the CPU exponent's lower bound exceeds 0.25",
}

BASE_HISTORY = 2**10
U_KEYS = (2**10, 2**12, 2**14, 2**16, 2**18)
R_KEYS = 2**12
R_HISTORIES = (2**8, 2**10, 2**12, 2**14, 2**16)
DL_KEYS = 2**14
D_DELTAS = (1, 4, 16, 64, 256)
L_DELETIONS = (1, 4, 16, 64, 256)
E_KEYS = (2**10, 2**14, 2**18)
RESERVE_GROUPS = 36
WARMUP_COMMITS = 4
TIMED_COMMITS = 32
EVICT_DEFERRED = 8
EVICT_TIMED = 16
CONTROL_TIMED = 8
CONTROL_U_KEYS = (2**10, 2**14, 2**18)
CONTROL_R_HISTORIES = (2**8, 2**12, 2**16)
AXES = ("U", "R", "D", "L", "E")
STRATUM_KEYS = {"U": "keys", "R": "history", "D": "delta", "L": "deletion", "E": "keys"}
ADDITIVE_UNITS = {"U": "ns per key", "R": "ns per state"}
CELL_KEYS = ("axis", "keys", "reserve", "history", "delta", "deletion", "writer", "case",
             "warmup", "deferred", "timed", "seed")
CELL_TEXT_KEYS = ("axis", "writer", "case")


def phase_major(phases, suffixes):
    return tuple(f"{phase}_{suffix}" for phase in phases for suffix in suffixes)


VERIFY_PHASES = ("seal", "bind", "prepare", "publish")
TIME_PHASES = VERIFY_PHASES + ("release",)
VERIFY_PROBES = ("alloc_count", "alloc_bytes", "alloc_peak", "alloc_max", "syscr", "syscw", "rchar", "wchar")
CLOCKS = ("thread_ns", "process_ns", "wall_ns")
IO_METRICS = ("syscr", "syscw", "rchar", "wchar")
ALLOCATION_METRICS = ("alloc_count", "alloc_bytes")
SEALING_COUNTERS = ("loaded_nodes", "edited_nodes", "emitted_nodes", "exact_path_chunks")
LABELS = ("warmup", "deferred", "reclaim", "timed")
FILESYSTEMS = ("ext4", "xfs", "btrfs", "other")
PROBE_FIELD_PATTERNS = ("*_alloc_*", "*_sys*", "*_rchar", "*_wchar")
RECORD_KEYS = {
    "SCALE_HEADER": ("mode", "cell"),
    "SCALE_FIXTURE": ("keys_total", "state_objects", "state_bytes", "history", "union_objects",
                      "pack_bytes", "index_bytes", "records", "build_ns"),
    "SCALE_VERIFY": ("commit", "label", "target", "cur_refs", "cur_exp", "cur_quoted", "ret_refs",
                     "ret_exp", "ret_quoted", "meta_refs", "meta_exp", "meta_quoted", "logical_writes",
                     "metadata_bytes", "fresh_objects", "fresh_bytes", "pack_delta", "index_delta",
                     "live_packs", "old_path", "new_path", "deleted_objects", "loaded_nodes",
                     "edited_nodes", "emitted_nodes", "exact_path_chunks",
                     *phase_major(VERIFY_PHASES, VERIFY_PROBES), "unlinked"),
    "SCALE_SAMPLE": ("commit", "label", "target", *phase_major(TIME_PHASES, CLOCKS), "rss_after"),
    "SCALE_PROCESS": ("mode", "commits", "admission_ns", "rss_start", "rss_end", "hwm_window",
                      "hwm_reset", "swap", "live_packs", "filesystem", "probe_syscr", "probe_syscw",
                      "probe_rchar", "probe_wchar", "probe_alloc_count", "probe_alloc_bytes"),
}
TEXT_FIELDS = {
    "SCALE_HEADER": frozenset({"mode", "cell"}),
    "SCALE_FIXTURE": frozenset(),
    "SCALE_VERIFY": frozenset({"label", "target", "unlinked"}),
    "SCALE_SAMPLE": frozenset({"label", "target"}),
    "SCALE_PROCESS": frozenset({"mode", "filesystem"}),
}
COMMIT_TAGS = {"verify": "SCALE_VERIFY", "time": "SCALE_SAMPLE"}
PROCESS_PROBE_KEYS = tuple(key for key in RECORD_KEYS["SCALE_PROCESS"] if key.startswith("probe_"))
DETERMINISTIC_VERIFY_FIELDS = tuple(
    key for key in RECORD_KEYS["SCALE_VERIFY"]
    if not any(fnmatch.fnmatchcase(key, pattern) for pattern in PROBE_FIELD_PATTERNS))
INTEGER_PATTERN = re.compile(r"0|[1-9][0-9]*")
DIGEST_PATTERN = re.compile(r"[0-9a-f]{64}")
RECLAIM_INTENT = "reclaim.v1"
PRIVATE_DIRECTORY_MODE = 0o700
CHILD_ASSERTIONS = {
    "G5": "pack and index growth equal the encoded appends",
    "G7": "the sealed metadata objects equal the writer model's bytes",
    "G9": "the publication effects are the trusted effects",
}
PACK_FILE_PATTERN = re.compile(r"pack-[0-9]{20}\.rsp")
INDEX_FILE_PATTERN = re.compile(r"index-[0-9]{20}\.rsi")

TIMING_COLUMNS = CLOCKS + phase_major(TIME_PHASES, CLOCKS)
FIXTURE_COLUMNS = ("cell_id", "axes", "cell", "status", "sequence", "exit_status", "duration_ns",
                   *RECORD_KEYS["SCALE_FIXTURE"], "file_count", "fixture_bytes", "files")
VERIFY_COLUMNS = ("cell_id", "axes", "run", "sequence", "duration_ns", *RECORD_KEYS["SCALE_VERIFY"],
                  *(f"process_{key}" for key in RECORD_KEYS["SCALE_PROCESS"]))
SAMPLE_COLUMNS = ("cell_id", "axes", "population", "round", "sequence", *RECORD_KEYS["SCALE_SAMPLE"])
PROCESS_COLUMNS = ("cell_id", "axes", "population", "round", "sequence", "exit_status", "failure",
                   "duration_ns", "commits", "timed_commits", *TIMING_COLUMNS, "deferred_thread_ns",
                   "reclaim_thread_ns", "rss_start", "rss_end", "hwm_window", "hwm_reset", "swap",
                   "live_packs", "admission_ns", "filesystem", "target_sha256")
FAILURE_COLUMNS = ("sequence", "label", "cell_id", "mode", "population", "index", "kind", "exit_status",
                   "duration_ns", "reason")
TABLE_COLUMNS = {
    "fixtures.tsv": FIXTURE_COLUMNS,
    "verify.tsv": VERIFY_COLUMNS,
    "samples.tsv": SAMPLE_COLUMNS,
    "process-summary.tsv": PROCESS_COLUMNS,
    "failures.tsv": FAILURE_COLUMNS,
}
ANALYSIS_INPUTS = ("metadata.json", "fixtures.tsv", "verify.tsv", "process-summary.tsv")


class RecordError(ValueError):
    kind = "missing"


class MalformedRecord(RecordError):
    kind = "malformed"


class ChildTimeout(Exception):
    def __init__(self, output):
        super().__init__("the child exceeded its timeout")
        self.output = output


class IdleAdmissionError(RuntimeError):
    pass


class CollectionRefused(RuntimeError):
    pass


def parse_record(line):
    """Parse one tab-separated SCALE_ record line into (tag, typed fields), refusing any deviation."""
    fields = line.split("\t")
    tag = fields[0]
    if tag not in RECORD_KEYS:
        raise MalformedRecord(f"unknown record tag {tag!r}")
    version = fields[1] if len(fields) > 1 else None
    if version != RECORD_VERSION:
        raise MalformedRecord(f"{tag} carries version {version!r}, expected {RECORD_VERSION!r}")
    pairs = []
    for field in fields[2:]:
        name, separator, value = field.partition("=")
        if not separator:
            raise MalformedRecord(f"{tag} field {field!r} is not key=value")
        pairs.append((name, value))
    names = tuple(name for name, _ in pairs)
    expected = RECORD_KEYS[tag]
    if names != expected:
        missing = [name for name in expected if name not in names]
        extra = [name for name in names if name not in expected]
        repeated = sorted(name for name, count in Counter(names).items() if count > 1)
        if missing or extra or repeated:
            raise MalformedRecord(f"{tag} keys differ: missing {missing}, extra {extra}, repeated {repeated}")
        raise MalformedRecord(f"{tag} keys are reordered")
    return tag, {name: convert_field(tag, name, value) for name, value in pairs}


def convert_field(tag, name, value):
    if name in TEXT_FIELDS[tag]:
        if name == "mode":
            valid = value in (("build", "verify", "time") if tag == "SCALE_HEADER" else ("verify", "time"))
        elif name == "cell":
            valid = bool(value)
        elif name == "label":
            valid = value in LABELS
        elif name == "target":
            valid = DIGEST_PATTERN.fullmatch(value) is not None
        elif name == "filesystem":
            valid = value in FILESYSTEMS
        else:
            valid = unlinked_is_valid(value)
        if not valid:
            raise MalformedRecord(f"{tag} {name}={value!r} is invalid")
        return value
    if INTEGER_PATTERN.fullmatch(value) is None or int(value) > U64_MAX:
        raise MalformedRecord(f"{tag} {name}={value!r} is not an unsigned integer")
    number = int(value)
    if tag == "SCALE_SAMPLE" and name.endswith("_ns") and number == 0:
        raise MalformedRecord(f"{tag} {name} is not positive")
    if name == "hwm_reset" and number > 1:
        raise MalformedRecord(f"{tag} hwm_reset={number} is neither 0 nor 1")
    return number


def unlinked_is_valid(value):
    if value == "-":
        return True
    names = value.split(";")
    return all(names) and names == sorted(set(names))


def child_records(output):
    """Parse every line that starts with SCALE_; every other line (libtest's terse framing included) is ignored."""
    return [parse_record(line) for line in output.split("\n") if line.startswith(RECORD_PREFIX)]


def validate_child_records(mode, cell, records):
    """Require exactly the records the contract prescribes for this mode and cell, in order."""
    if not records:
        raise RecordError("the child printed no SCALE_ records")
    first_tag, header = records[0]
    if first_tag != "SCALE_HEADER":
        raise RecordError(f"the first record is {first_tag}, not SCALE_HEADER")
    if header["mode"] != mode or header["cell"] != cell["cell"]:
        raise RecordError(f"SCALE_HEADER names mode {header['mode']!r} and cell {header['cell']!r}")
    body = records[1:]
    if mode == "build":
        tags = [tag for tag, _ in body]
        if tags != ["SCALE_FIXTURE"]:
            raise RecordError(f"build mode printed {tags} after its header, expected one SCALE_FIXTURE")
        return {"fixture": body[0][1]}
    if not body or body[-1][0] != "SCALE_PROCESS":
        raise RecordError("SCALE_PROCESS is not the last record")
    process = body[-1][1]
    commits = body[:-1]
    stray = sorted({tag for tag, _ in commits if tag != COMMIT_TAGS[mode]})
    if stray:
        raise RecordError(f"{mode} mode printed unexpected records {stray}")
    indices = [record["commit"] for _, record in commits]
    repeated = sorted(index for index, count in Counter(indices).items() if count > 1)
    if repeated:
        raise MalformedRecord(f"duplicate commit indices {repeated}")
    labels = expected_labels(cell)
    if indices != list(range(len(labels))):
        raise RecordError(f"{len(indices)} commit records do not number the {len(labels)} commits 0.. in order")
    if [record["label"] for _, record in commits] != labels:
        raise RecordError("commit labels differ from the cell's schedule")
    if process["mode"] != mode or process["commits"] != len(labels):
        raise RecordError(f"SCALE_PROCESS names mode {process['mode']!r} and {process['commits']} commits")
    if mode == "time" and any(process[key] for key in PROCESS_PROBE_KEYS):
        raise RecordError("time mode reported a nonzero probe calibration")
    return {"commits": [record for _, record in commits], "process": process}


def expected_labels(cell):
    labels = ["warmup"] * cell["warmup"]
    if cell["case"] == "evict":
        labels += ["deferred"] * cell["deferred"] + ["reclaim"]
    return labels + ["timed"] * cell["timed"]


def cell_string(cell):
    return ",".join(f"{key}={cell[key]}" for key in CELL_KEYS)


def parse_cell_string(text):
    pairs = [pair.partition("=") for pair in text.split(",")]
    if tuple(name for name, _, _ in pairs) != CELL_KEYS or not all(separator for _, separator, _ in pairs):
        raise ValueError(f"cell string {text!r} does not list {CELL_KEYS}")
    cell = {}
    for name, _, value in pairs:
        if name in CELL_TEXT_KEYS:
            cell[name] = value
        elif INTEGER_PATTERN.fullmatch(value):
            cell[name] = int(value)
        else:
            raise ValueError(f"cell string {text!r} has a non-integer {name}")
    return cell


def validate_cell(cell):
    """Refuse a cell the scale child's parser would refuse (child B design, sections 2 and 3)."""
    commits = len(expected_labels(cell))
    problems = []
    if cell["axis"] not in AXES or cell["writer"] not in ("v1", "v2") or cell["case"] not in ("ordinary", "evict"):
        problems.append("unknown axis, writer or case")
    if cell["keys"] < 1 or cell["history"] < 1 or cell["timed"] < 1:
        problems.append("keys, history and timed must be positive")
    if (cell["delta"] > 0) == (cell["deletion"] > 0):
        problems.append("exactly one of delta and deletion must be positive")
    if (cell["deferred"] > 0) != (cell["case"] == "evict"):
        problems.append("deferred commits exist exactly in the evict case")
    if cell["deletion"] > 0:
        if cell["reserve"] % cell["deletion"] or commits > cell["reserve"] // cell["deletion"]:
            problems.append("the reserve does not hold one aligned group per commit")
    elif cell["reserve"] != 0 or commits * cell["delta"] > cell["keys"]:
        problems.append("a replacement cell replaces a key twice or carries a reserve")
    if problems:
        raise ValueError(f"invalid cell {cell_string(cell)}: {'; '.join(problems)}")


def make_cell(axis, keys, history, delta=1, deletion=0, reserve=0, writer="v2", case="ordinary",
              warmup=WARMUP_COMMITS, deferred=0, timed=TIMED_COMMITS):
    parameters = {"axis": axis, "keys": keys, "reserve": reserve, "history": history, "delta": delta,
                  "deletion": deletion, "writer": writer, "case": case, "warmup": warmup,
                  "deferred": deferred, "timed": timed, "seed": SEED}
    validate_cell(parameters)
    return {"id": f"{axis}-k{keys}-h{history}-d{delta}-m{deletion}-{writer}-{case}", "axes": [axis],
            "cell": cell_string(parameters), **parameters}


def merge_cell(cells, candidate):
    for cell in cells:
        if all(cell[key] == candidate[key] for key in CELL_KEYS if key != "axis"):
            cell["axes"].append(candidate["axis"])
            return
    cells.append(candidate)


def axis_members(cells, axis):
    return sorted((cell for cell in cells if axis in cell["axes"]), key=lambda cell: cell[STRATUM_KEYS[axis]])


def decision_cells():
    cells = []
    for keys in U_KEYS:
        merge_cell(cells, make_cell("U", keys, BASE_HISTORY))
    for history in R_HISTORIES:
        merge_cell(cells, make_cell("R", R_KEYS, history))
    for delta in D_DELTAS:
        merge_cell(cells, make_cell("D", DL_KEYS, BASE_HISTORY, delta=delta))
    for deletion in L_DELETIONS:
        merge_cell(cells, make_cell("L", DL_KEYS, BASE_HISTORY, delta=0, deletion=deletion,
                                    reserve=RESERVE_GROUPS * deletion))
    for keys in E_KEYS:
        merge_cell(cells, make_cell("E", keys, BASE_HISTORY, case="evict", deferred=EVICT_DEFERRED,
                                    timed=EVICT_TIMED))
    return cells


def pilot_cells():
    cells = decision_cells()
    chosen = []
    for axis in AXES:
        members = axis_members(cells, axis)
        for cell in (members[0], members[-1]):
            if all(cell["id"] != other["id"] for other in chosen):
                chosen.append(cell)
    return chosen


def control_cells(control):
    cells = []
    if control == "NC-V1":
        for history in CONTROL_R_HISTORIES:
            merge_cell(cells, make_cell("R", R_KEYS, history, writer="v1", timed=CONTROL_TIMED))
        return cells
    for keys in CONTROL_U_KEYS:
        merge_cell(cells, make_cell("U", keys, BASE_HISTORY, timed=CONTROL_TIMED))
    for history in CONTROL_R_HISTORIES:
        merge_cell(cells, make_cell("R", R_KEYS, history, timed=CONTROL_TIMED))
    return cells


def build_protocol(mode, control):
    """Return the frozen protocol for a population; analysis refuses metadata that records any other."""
    if mode == "decision" and control is None:
        cells, verify_runs = decision_cells(), DECISION_VERIFY_RUNS
        populations = [("warmup", WARMUP_ROUNDS), ("decision", DECISION_ROUNDS)]
    elif mode == "pilot" and (control is None or control in CONTROLS):
        cells, verify_runs = (pilot_cells() if control is None else control_cells(control)), 1
        populations = [("pilot", PILOT_TIME_PROCESSES)]
    elif mode == "controls" and control in CONTROLS:
        cells, verify_runs = control_cells(control), 1
        populations = [("control", CONTROL_TIME_PROCESSES)]
    else:
        raise ValueError(f"unknown population mode {mode!r} with control {control!r}")
    return {
        "task": TASK,
        "mode": mode,
        "control": control,
        "seed": SEED,
        "child_test": CHILD_TEST,
        "record_version": RECORD_VERSION,
        "verify_runs": verify_runs,
        "time_populations": [{"population": name, "rounds": rounds} for name, rounds in populations],
        "sample_population": SAMPLE_POPULATIONS[mode],
        "idle_snapshots": IDLE_SNAPSHOTS,
        "replicates": REPLICATES,
        "bounds": {
            "exponent_upper95": EXPONENT_BOUND,
            "linear_work_upper95": LINEAR_WORK_BOUND,
            "rss_growth_upper97_5_bytes": RSS_GROWTH_BOUND,
            "metadata_bytes": METADATA_BYTE_BOUND,
            "metadata_step_work": METADATA_STEP_BOUND,
            "v1_manifest_bytes": [V1_MANIFEST_BASE_BYTES, V1_MANIFEST_ENTRY_BYTES],
        },
        "c_star_grid": {"base": 2, "exponent_step": 0.25, "lowest_exponent": C_STAR_QUARTER_POWERS[0] / 4,
                        "highest_exponent": C_STAR_QUARTER_POWERS[-1] / 4},
        "cells": cells,
    }


def sha256_file(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def sequence_digest(targets):
    return hashlib.sha256("\n".join(targets).encode()).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def canonical_json(value):
    return json.loads(json.dumps(value, allow_nan=False))


def single_line(text, limit=4000):
    return " ".join(str(text).split())[:limit]


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def table_value(value):
    if value is None:
        return ""
    if isinstance(value, bool):
        raise TypeError("booleans are not table values")
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        if not math.isfinite(value):
            raise ValueError("non-finite table value")
        return str(int(value)) if value.is_integer() else repr(value)
    return str(value)


class TableWriter:
    def __init__(self, path, columns):
        self.columns = columns
        self.known = frozenset(columns)
        self.handle = path.open("w", encoding="utf-8", newline="")
        self.handle.write("\t".join(columns) + "\n")
        self.handle.flush()

    def write(self, row):
        unknown = set(row) - self.known
        if unknown:
            raise ValueError(f"row carries unknown columns {sorted(unknown)}")
        values = [table_value(row.get(column)) for column in self.columns]
        if any(character in value for value in values for character in "\t\r\n"):
            raise ValueError("a table value contains a tab or a line break")
        self.handle.write("\t".join(values) + "\n")

    def flush(self):
        self.handle.flush()

    def close(self):
        self.handle.close()

    def __enter__(self):
        return self

    def __exit__(self, *details):
        self.close()


def read_table(path, columns):
    lines = path.read_text(encoding="utf-8").split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    if not lines or tuple(lines[0].split("\t")) != columns:
        raise ValueError(f"{path.name} does not carry the expected columns")
    rows = []
    for number, line in enumerate(lines[1:], start=2):
        values = line.split("\t")
        if len(values) != len(columns):
            raise ValueError(f"{path.name}:{number} has {len(values)} fields, expected {len(columns)}")
        rows.append(dict(zip(columns, values)))
    return rows


def read_optional(path):
    try:
        return path.read_text().strip()
    except OSError:
        return None


def cgroup_limits(cgroup):
    if cgroup is None:
        return None
    unified = [line[3:] for line in cgroup.splitlines() if line.startswith("0::")]
    if len(unified) != 1:
        return None
    base = Path("/sys/fs/cgroup") / unified[0].lstrip("/")
    return {"path": unified[0], "memory.max": read_optional(base / "memory.max"),
            "memory.swap.max": read_optional(base / "memory.swap.max")}


class LinuxHost:
    def governor(self, cpu):
        return read_optional(Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor"))

    def filesystem(self, path):
        return subprocess.check_output(["stat", "-f", "-c", "%T", str(path)], text=True).strip()

    def facts(self, binary, directory, cpu):
        cpufreq = Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq")
        cgroup = read_optional(Path("/proc/self/cgroup"))
        return {
            "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPOSITORY_ROOT, text=True).strip(),
            "binary": str(binary),
            "binary_sha256": sha256_file(binary),
            "source_sha256": {name: sha256_file(REPOSITORY_ROOT / name) for name in SOURCE_FILES},
            "governor": read_optional(cpufreq / "scaling_governor"),
            "energy_performance_preference": read_optional(cpufreq / "energy_performance_preference"),
            "boost": read_optional(Path("/sys/devices/system/cpu/cpufreq/boost")),
            "cpu_boost": read_optional(cpufreq / "boost"),
            "scaling_driver": read_optional(cpufreq / "scaling_driver"),
            "perf_event_paranoid": read_optional(Path("/proc/sys/kernel/perf_event_paranoid")),
            "output_filesystem": self.filesystem(directory),
            "cgroup": cgroup,
            "cgroup_limits": cgroup_limits(cgroup),
            "lscpu": subprocess.check_output(["lscpu"], text=True),
            "uname": subprocess.check_output(["uname", "-a"], text=True).strip(),
        }


def launch_child(command, environment, timeout):
    try:
        completed = subprocess.run(command, env=environment, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, timeout=timeout, check=False)
    except subprocess.TimeoutExpired as error:
        raise ChildTimeout((error.output or b"").decode("utf-8", errors="replace")) from None
    return completed.returncode, completed.stdout.decode("utf-8", errors="replace")


def await_idle(cpu, record, sampler=cpu_snapshot):
    for attempt in range(IDLE_SNAPSHOTS):
        raw, idle = sampler(cpu)
        record(attempt, raw, idle)
        if idle:
            return
    raise IdleAdmissionError(
        f"CPU contention after {IDLE_SNAPSHOTS} snapshots; partial population retained, not qualified")


def record_idle(directory, label, attempt, raw, eligible):
    (directory / f"cpu-{label}-attempt-{attempt}.json").write_text(raw)
    with (directory / "idle-decisions.tsv").open("a") as output:
        output.write(f"{label}\t{attempt}\t{int(eligible)}\n")


def collection_guard(environ, governor, pilot):
    if environ.get("SCALE_CAPPED") != "1":
        raise CollectionRefused("run inside the prescribed systemd resource envelope with SCALE_CAPPED=1")
    if not pilot and governor != "performance":
        raise CollectionRefused(
            f"decision data require the performance governor; the measured CPU reads {governor!r}")


def classify_child(mode, cell, returncode, timed_out, output):
    """Return (records, None) for a valid child or (None, failure); every deviation is a failure."""
    try:
        records, problem = validate_child_records(mode, cell, child_records(output)), None
    except RecordError as error:
        records, problem = None, error
    if timed_out:
        kind, reason = "timeout", "the child exceeded --child-timeout and was killed"
    elif returncode in OOM_STATUSES:
        kind, reason = "oom", f"exit status {returncode}: killed by SIGKILL (out-of-memory kill)"
    elif returncode != 0:
        kind, reason = "exit", f"exit status {returncode}"
    elif problem is not None:
        return None, {"kind": problem.kind, "reason": str(problem)}
    else:
        return records, None
    if problem is not None:
        reason = f"{reason}; {problem}"
    return None, {"kind": kind, "reason": reason}


def failure_text(failure):
    return "" if failure is None else single_line(f"{failure['kind']}: {failure['reason']}")


@dataclasses.dataclass
class ChildOutcome:
    sequence: int
    label: str
    status: str
    duration_ns: int
    records: dict | None
    failure: dict | None


class ChildRunner:
    def __init__(self, binary, directory, cpu, timeout, environ, launcher, sampler, log, failure_table):
        self.binary = binary
        self.directory = directory
        self.cpu = cpu
        self.timeout = timeout
        self.environ = environ
        self.launcher = launcher
        self.sampler = sampler
        self.log = log
        self.failure_table = failure_table
        self.sequence = 0
        self.launched = 0
        self.failures = []

    def run(self, mode, population, index, cell, fixture):
        sequence = self.sequence
        self.sequence += 1
        label = f"{sequence:05d}-{mode}-{population}-{index:02d}-{cell['id']}"
        await_idle(self.cpu, lambda attempt, raw, idle: record_idle(self.directory, label, attempt, raw, idle),
                   self.sampler)
        environment = {name: value for name, value in self.environ.items() if name not in CHILD_ENVIRONMENT}
        environment.update(RSPACE_SCALE_CELL=cell["cell"], RSPACE_SCALE_MODE=mode, RSPACE_SCALE_FIXTURE=str(fixture))
        scratch = None
        if mode != "build":
            scratch = self.directory / "scratch" / label
            scratch.mkdir(mode=PRIVATE_DIRECTORY_MODE)
            environment["RSPACE_SCALE_SCRATCH"] = str(scratch)
        command = ["taskset", "-c", str(self.cpu), str(self.binary), *CHILD_ARGUMENTS]
        timed_out = False
        self.launched += 1
        start = time.monotonic_ns()
        try:
            returncode, output = self.launcher(command, environment, self.timeout)
        except ChildTimeout as timeout:
            returncode, output, timed_out = None, timeout.output, True
        finally:
            duration = time.monotonic_ns() - start
            if scratch is not None:
                shutil.rmtree(scratch)
        status = "timeout" if timed_out else str(returncode)
        records, failure = classify_child(mode, cell, returncode, timed_out, output)
        exported = " ".join(f"{name}={environment[name]}" for name in CHILD_ENVIRONMENT if name in environment)
        self.log.write("".join([
            f"=== child {label} ===\n",
            f"command: {shlex.join(command)}\n",
            f"environment: {exported}\n",
            f"status: {status} duration_ns={duration} outcome={failure_text(failure) or 'ok'}\n",
            "--- output ---\n",
            output if output.endswith("\n") or not output else output + "\n",
            f"=== end child {label} ===\n",
        ]))
        if failure is not None:
            self.record_failure(sequence=sequence, label=label, cell_id=cell["id"], mode=mode,
                                population=population, index=index, kind=failure["kind"], exit_status=status,
                                duration_ns=duration, reason=failure["reason"])
        return ChildOutcome(sequence, label, status, duration, records, failure)

    def record_failure(self, **failure):
        entry = {column: failure.get(column) for column in FAILURE_COLUMNS}
        entry["reason"] = single_line(entry["reason"])
        self.failures.append(entry)
        self.failure_table.write(entry)
        self.failure_table.flush()


def fixture_files(directory):
    files = {}
    for path in sorted(directory.rglob("*")):
        if path.is_symlink() or not (path.is_file() or path.is_dir()):
            raise ValueError(f"fixture entry {path.name} is not a regular file or directory")
        if path.is_file():
            files[path.relative_to(directory).as_posix()] = {"bytes": path.stat().st_size,
                                                             "sha256": sha256_file(path)}
    return files


def fixture_row(cell, outcome, fixture, runner, recorded):
    row = {"cell_id": cell["id"], "axes": ",".join(cell["axes"]), "cell": cell["cell"], "sequence": outcome.sequence,
           "exit_status": outcome.status, "duration_ns": outcome.duration_ns}
    try:
        files = fixture_files(fixture)
    except (OSError, ValueError) as error:
        files, problem = {}, f"the fixture cannot be hashed: {error}"
    else:
        problem = None if files else "the build child left no fixture files"
    row.update(file_count=len(files), fixture_bytes=sum(entry["bytes"] for entry in files.values()),
               files=json.dumps(files, sort_keys=True, separators=(",", ":")))
    if outcome.failure is not None:
        row["status"] = outcome.failure["kind"]
    elif problem is not None:
        row["status"] = "fixture"
        runner.record_failure(sequence=outcome.sequence, label=outcome.label, cell_id=cell["id"], mode="build",
                              population="build", index=0, kind="fixture", exit_status=outcome.status,
                              duration_ns=outcome.duration_ns, reason=problem)
    else:
        row["status"] = "ok"
        row.update(outcome.records["fixture"])
        recorded[cell["id"]] = files
    return row


def verify_rows(cell, run, outcome):
    process = {f"process_{key}": value for key, value in outcome.records["process"].items()}
    for commit in outcome.records["commits"]:
        yield {"cell_id": cell["id"], "axes": ",".join(cell["axes"]), "run": run, "sequence": outcome.sequence,
               "duration_ns": outcome.duration_ns, **commit, **process}


def sample_rows(cell, population, round_index, outcome):
    for commit in outcome.records["commits"]:
        yield {"cell_id": cell["id"], "axes": ",".join(cell["axes"]), "population": population,
               "round": round_index, "sequence": outcome.sequence, **commit}


def commit_total(commit, clock):
    return sum(commit[f"{phase}_{clock}"] for phase in TIME_PHASES)


def summarize_time_process(records):
    """Per-process statistics: medians over the timed commits of the five phases' per-commit sums."""
    commits = records["commits"]
    process = records["process"]
    timed = [commit for commit in commits if commit["label"] == "timed"]
    deferred = [commit_total(commit, "thread_ns") for commit in commits if commit["label"] == "deferred"]
    reclaim = [commit_total(commit, "thread_ns") for commit in commits if commit["label"] == "reclaim"]
    summary: dict[str, int | float | str | None] = {"commits": len(commits), "timed_commits": len(timed)}
    for clock in CLOCKS:
        summary[clock] = statistics.median(commit_total(commit, clock) for commit in timed)
    for phase in TIME_PHASES:
        for clock in CLOCKS:
            summary[f"{phase}_{clock}"] = statistics.median(commit[f"{phase}_{clock}"] for commit in timed)
    summary["deferred_thread_ns"] = statistics.median(deferred) if deferred else None
    summary["reclaim_thread_ns"] = reclaim[0] if len(reclaim) == 1 else None
    for key in ("rss_start", "rss_end", "hwm_window", "hwm_reset", "swap", "live_packs", "admission_ns",
                "filesystem"):
        summary[key] = process[key]
    summary["target_sha256"] = sequence_digest([commit["target"] for commit in commits])
    return summary


def process_row(cell, population, round_index, outcome):
    row = {"cell_id": cell["id"], "axes": ",".join(cell["axes"]), "population": population, "round": round_index,
           "sequence": outcome.sequence, "exit_status": outcome.status, "failure": failure_text(outcome.failure),
           "duration_ns": outcome.duration_ns}
    if outcome.records is not None:
        row.update(summarize_time_process(outcome.records))
    return row


def run_population(protocol, directory, runner, tables, recorded, progress):
    cells = protocol["cells"]
    for cell in cells:
        fixture = directory / "fixtures" / cell["id"]
        fixture.mkdir(mode=PRIVATE_DIRECTORY_MODE)
        outcome = runner.run("build", "build", 0, cell, fixture)
        tables["fixtures.tsv"].write(fixture_row(cell, outcome, fixture, runner, recorded))
        tables["fixtures.tsv"].flush()
        runner.log.flush()
    progress(f"built {len(cells)} fixtures")
    for run in range(1, protocol["verify_runs"] + 1):
        for cell in cells:
            outcome = runner.run("verify", "verify", run, cell, directory / "fixtures" / cell["id"])
            if outcome.records is not None:
                for row in verify_rows(cell, run, outcome):
                    tables["verify.tsv"].write(row)
            tables["verify.tsv"].flush()
            runner.log.flush()
        progress(f"completed verify run {run}/{protocol['verify_runs']}")
    order = random.Random(SEED)
    for population in protocol["time_populations"]:
        name, rounds = population["population"], population["rounds"]
        for round_index in range(rounds):
            visit = list(cells)
            order.shuffle(visit)
            for cell in visit:
                outcome = runner.run("time", name, round_index, cell, directory / "fixtures" / cell["id"])
                tables["process-summary.tsv"].write(process_row(cell, name, round_index, outcome))
                if outcome.records is not None:
                    for row in sample_rows(cell, name, round_index, outcome):
                        tables["samples.tsv"].write(row)
                tables["process-summary.tsv"].flush()
                tables["samples.tsv"].flush()
                runner.log.flush()
            progress(f"completed {name} round {round_index + 1}/{rounds}")


def changed_fixtures(directory, recorded):
    changed = []
    for cell_id, files in recorded.items():
        try:
            current = fixture_files(directory / "fixtures" / cell_id)
        except (OSError, ValueError):
            current = None
        if current != files:
            changed.append(cell_id)
    return changed


def write_manifest(directory):
    files = {path.name: {"bytes": path.stat().st_size, "sha256": sha256_file(path)}
             for path in sorted(directory.iterdir()) if path.is_file() and path.name != "manifest.json"}
    write_json(directory / "manifest.json", {"task": TASK, "files": files})


def refresh_manifest(directory):
    path = directory / "manifest.json"
    if not path.is_file():
        write_manifest(directory)
        return
    manifest = json.loads(path.read_text(encoding="utf-8"))
    analysis = directory / "analysis.json"
    manifest.setdefault("files", {})["analysis.json"] = {"bytes": analysis.stat().st_size,
                                                         "sha256": sha256_file(analysis)}
    write_json(path, manifest)


def print_progress(message):
    print(message, flush=True)


def collect(binary, directory, cpu, mode, control, child_timeout, *, environ=None, host=None,
            launcher=launch_child, sampler=cpu_snapshot, progress=print_progress):
    """Collect one population (decision, pilot or controls) into a new directory and analyze it."""
    environ = os.environ if environ is None else environ
    host = LinuxHost() if host is None else host
    collection_guard(environ, host.governor(cpu), mode == "pilot")
    protocol = build_protocol(mode, control)
    if not binary.is_file():
        raise CollectionRefused(f"the test binary {binary} does not exist")
    directory.mkdir(parents=True, exist_ok=False)
    filesystem = host.filesystem(directory)
    if filesystem in ("tmpfs", "ramfs"):
        directory.rmdir()
        raise CollectionRefused(f"the output directory is on {filesystem}; fixtures and scratch must not be")
    metadata = {
        "task": TASK,
        "pilot_only": mode == "pilot",
        "cpu": cpu,
        "child_timeout_seconds": child_timeout,
        "command": list(sys.argv),
        "environment": {name: environ.get(name) for name in ("SCALE_CAPPED", "TMPDIR")},
        "python": sys.version,
        "numpy": np.__version__,
        "started_utc": utc_now(),
        "host": host.facts(binary, directory, cpu),
        "protocol": protocol,
        "collection": {"completed": False},
    }
    write_json(directory / "metadata.json", metadata)
    (directory / "idle-decisions.tsv").write_text("label\tattempt\teligible\n")
    (directory / "scratch").mkdir(mode=PRIVATE_DIRECTORY_MODE)
    (directory / "fixtures").mkdir(mode=PRIVATE_DIRECTORY_MODE)
    fatal = None
    recorded = {}
    with contextlib.ExitStack() as stack:
        log = stack.enter_context((directory / "children.log").open("w", encoding="utf-8"))
        tables = {name: stack.enter_context(TableWriter(directory / name, columns))
                  for name, columns in TABLE_COLUMNS.items()}
        runner = ChildRunner(binary, directory, cpu, child_timeout, environ, launcher, sampler, log,
                             tables["failures.tsv"])
        try:
            run_population(protocol, directory, runner, tables, recorded, progress)
        except BaseException as error:
            fatal = error
        for cell_id in changed_fixtures(directory, recorded):
            runner.record_failure(label=f"rehash-{cell_id}", cell_id=cell_id, mode="rehash", kind="fixture-changed",
                                  reason="a fixture file changed or vanished during the collection")
    try:
        raw, _ = sampler(cpu)
        (directory / "cpu-after.json").write_text(raw)
        after = None
    except Exception as error:
        after = single_line(f"{type(error).__name__}: {error}")
    metadata["collection"] = {
        "completed": fatal is None,
        "interrupted": None if fatal is None else single_line(f"{type(fatal).__name__}: {fatal}"),
        "children": runner.launched,
        "failures": runner.failures,
        "fixtures_unchanged": all(failure["kind"] != "fixture-changed" for failure in runner.failures),
        "cpu_after_error": after,
        "finished_utc": utc_now(),
    }
    write_json(directory / "metadata.json", metadata)
    with contextlib.suppress(OSError):
        (directory / "scratch").rmdir()
    if fatal is not None:
        print(f"collection interrupted: {fatal!r}", file=sys.stderr, flush=True)
    try:
        analysis = analyze_directory(directory)
        write_json(directory / "analysis.json", analysis)
    finally:
        write_manifest(directory)
    if fatal is not None:
        raise fatal
    return analysis


def slope_summary(strata, groups):
    summary = {"strata": list(strata), "samples": [len(group) for group in groups]}
    try:
        result = stratified_loglog_slope(list(strata), [list(group) for group in groups],
                                         replicates=REPLICATES, seed=SEED)
    except ValueError as error:
        summary["error"] = str(error)
    else:
        summary.update({key: result[key] for key in ("estimate", "lower95", "upper95", "replicates", "seed")})
    return summary


def arm_slope(cells, values_by_cell, axis):
    members = axis_members(cells, axis)
    summary = slope_summary([cell[STRATUM_KEYS[axis]] for cell in members],
                            [values_by_cell.get(cell["id"], []) for cell in members])
    summary["cells"] = [cell["id"] for cell in members]
    return summary


def bounded_arms(cells, values_by_cell, axes, bound):
    arms = {}
    for axis in axes:
        arm = arm_slope(cells, values_by_cell, axis)
        arm["passed"] = "error" not in arm and arm["upper95"] <= bound
        arms[axis] = arm
    return arms


@functools.lru_cache(maxsize=None)
def median_rank_pairs(size):
    draws = np.random.default_rng(SEED).integers(0, size, size=(REPLICATES, size))
    draws.sort(axis=1)
    low, high = draws[:, (size - 1) // 2].copy(), draws[:, size // 2].copy()
    low.setflags(write=False)
    high.setflags(write=False)
    return low, high


def resampled_medians(ordered, low, high):
    return (ordered[low] + ordered[high]) / 2


def median_interval(values):
    """Median with a 95% percentile bootstrap interval (10,000 resamples, seed 7929)."""
    if not values:
        return {"n": 0, "error": "no observations"}
    ordered = np.sort(np.asarray(values, dtype=np.float64))
    lower, upper = np.quantile(resampled_medians(ordered, *median_rank_pairs(len(ordered))), [0.025, 0.975])
    return {"n": len(values), "median": statistics.median(values), "lower95": float(lower), "upper95": float(upper)}


def rss_growth_bound(largest, smallest):
    """Two-sample bootstrap of median(largest) - median(smallest); passes when its 97.5% bound is at most 1 MiB."""
    if not largest or not smallest:
        return {"error": "a stratum has no processes", "passed": False}
    generator = np.random.default_rng(SEED)
    resampled = []
    for values in (largest, smallest):
        ordered = np.sort(np.asarray(values, dtype=np.float64))
        draws = generator.integers(0, len(ordered), size=(REPLICATES, len(ordered)))
        draws.sort(axis=1)
        resampled.append(resampled_medians(ordered, draws[:, (len(ordered) - 1) // 2], draws[:, len(ordered) // 2]))
    lower, upper = np.quantile(resampled[0] - resampled[1], [0.025, 0.975])
    return {"difference_of_medians": statistics.median(largest) - statistics.median(smallest),
            "lower2_5": float(lower), "upper97_5": float(upper), "bound": RSS_GROWTH_BOUND,
            "replicates": REPLICATES, "seed": SEED, "passed": float(upper) <= RSS_GROWTH_BOUND}


def additive_threshold(strata, groups):
    """Return (c*, grid index, upper95 at c*): the smallest grid c whose added c*x lifts upper95 above 0.25.

    c* is 0.0 when the unshifted upper95 already exceeds 0.25 and None when no grid point does.
    """
    baseline = stratified_loglog_slope(strata, groups, replicates=REPLICATES, seed=SEED)["upper95"]
    if baseline > EXPONENT_BOUND:
        return 0.0, None, baseline
    for index, coefficient in enumerate(C_STAR_GRID):
        shifted = [[value + coefficient * stratum for value in group] for stratum, group in zip(strata, groups)]
        upper = stratified_loglog_slope(strata, shifted, replicates=REPLICATES, seed=SEED)["upper95"]
        if upper > EXPONENT_BOUND:
            return coefficient, index, upper
    return None, None, None


def c_star_summary(cells, values_by_cell, axis):
    members = axis_members(cells, axis)
    strata = [cell[STRATUM_KEYS[axis]] for cell in members]
    groups = [values_by_cell.get(cell["id"], []) for cell in members]
    summary = {"axis": axis, "unit": ADDITIVE_UNITS[axis], "strata": strata}
    try:
        coefficient, index, upper = additive_threshold(strata, groups)
    except ValueError as error:
        summary["error"] = str(error)
        return summary
    largest = statistics.median(groups[-1])
    summary.update(
        c_star=coefficient, grid_index=index, at_grid_floor=index == 0, baseline_exceeds=coefficient == 0.0,
        upper95_at_c_star=upper, largest_stratum=strata[-1], largest_stratum_median_ns=largest,
        percent_of_largest_stratum_median=None if coefficient is None else 100 * coefficient * strata[-1] / largest)
    return summary


def group_rows(rows):
    grouped = defaultdict(list)
    for row in rows:
        grouped[row["cell_id"]].append(row)
    return grouped


def run_rows(verify_by_cell, cell_id, run):
    return [row for row in verify_by_cell.get(cell_id, []) if row["run"] == str(run)]


def timed_rows(verify_by_cell, cell_id):
    return [row for row in verify_by_cell.get(cell_id, []) if row["label"] == "timed"]


def successful_processes(process_rows, population):
    grouped = defaultdict(list)
    for row in process_rows:
        if row["population"] == population and row["exit_status"] == "0" and not row["failure"]:
            grouped[row["cell_id"]].append(row)
    return {cell_id: sorted(rows, key=lambda row: int(row["round"])) for cell_id, rows in grouped.items()}


def failure_is_expected(protocol, failure):
    return (protocol["mode"] == "controls" and protocol["control"] == "NC-ORDER"
            and failure.get("mode") == "verify" and failure.get("kind") == "exit")


def qualification_problems(metadata, protocol, fixture_rows, verify_rows_all, process_rows):
    problems = []
    if metadata.get("protocol") != protocol:
        problems.append("metadata.json records a protocol other than the frozen one")
    collection = metadata.get("collection") or {}
    if collection.get("completed") is not True:
        problems.append(f"the collection did not complete: {collection.get('interrupted')}")
    for failure in collection.get("failures") or []:
        if not failure_is_expected(protocol, failure):
            problems.append(f"failed child {failure.get('label')}: {failure.get('kind')}: {failure.get('reason')}")
    cells = protocol["cells"]
    identifiers = {cell["id"] for cell in cells}
    fixtures = group_rows(fixture_rows)
    for cell in cells:
        rows = fixtures.get(cell["id"], [])
        if len(rows) != 1 or rows[0]["status"] != "ok":
            problems.append(f"{cell['id']}: no single successful fixture build")
    populations = {entry["population"]: entry["rounds"] for entry in protocol["time_populations"]}
    processes = defaultdict(list)
    for row in process_rows:
        if row["cell_id"] not in identifiers or row["population"] not in populations:
            problems.append(f"unexpected time process {row['cell_id']} {row['population']} round {row['round']}")
        processes[row["cell_id"], row["population"]].append(row)
    for name, rounds in populations.items():
        for cell in cells:
            rows = processes.get((cell["id"], name), [])
            if sorted(int(row["round"]) for row in rows) != list(range(rounds)):
                problems.append(f"{cell['id']}: {name} rounds are not exactly 0..{rounds - 1}")
            failed = [row["round"] for row in rows if row["exit_status"] != "0" or row["failure"]]
            if failed:
                problems.append(f"{cell['id']}: {name} rounds {failed} failed")
    runs = {str(run) for run in range(1, protocol["verify_runs"] + 1)}
    for row in verify_rows_all:
        if row["cell_id"] not in identifiers or row["run"] not in runs:
            problems.append(f"unexpected verify record {row['cell_id']} run {row['run']}")
            break
    if not (protocol["mode"] == "controls" and protocol["control"] == "NC-ORDER"):
        verify_by_cell = group_rows(verify_rows_all)
        for cell in cells:
            for run in range(1, protocol["verify_runs"] + 1):
                if not run_rows(verify_by_cell, cell["id"], run):
                    problems.append(f"{cell['id']}: verify run {run} has no records")
    return problems


def verify_run_problem(cell, rows):
    labels = expected_labels(cell)
    if not rows:
        return "no SCALE_VERIFY records"
    if [int(row["commit"]) for row in rows] != list(range(len(labels))):
        return "the records are not one per commit in order"
    if [row["label"] for row in rows] != labels:
        return "the labels differ from the cell's schedule"
    for row in rows:
        if int(row["cur_exp"]) != int(row["old_path"]) + int(row["new_path"]) + int(row["deleted_objects"]):
            return f"commit {row['commit']}: cur_exp differs from old_path + new_path + deleted_objects"
        if cell["deletion"] == 0 and int(row["deleted_objects"]) != 0:
            return f"commit {row['commit']}: a replacement commit reports a deleted closure"
        if row["process_mode"] != "verify" or int(row["process_commits"]) != len(labels):
            return f"commit {row['commit']}: the process record names another mode or commit count"
    return None


def incomplete_verify_runs(cells, verify_runs, verify_by_cell):
    return [f"{cell['id']} run {run}" for cell in cells for run in range(1, verify_runs + 1)
            if len(run_rows(verify_by_cell, cell["id"], run)) != len(expected_labels(cell))]


def child_assertion(gate, cells, verify_runs, verify_by_cell):
    """A gate condition the verify child asserts at every commit; it holds exactly when every verify run completed."""
    incomplete = incomplete_verify_runs(cells, verify_runs, verify_by_cell)
    return {"condition": CHILD_ASSERTIONS[gate],
            "asserted_by": "the verify child at every commit; a violation fails the child",
            "incomplete_verify_runs": incomplete[:PROBLEM_LIMIT], "passed": not incomplete}


def exact_work_gate(protocol, verify_by_cell):
    problems = []
    for cell in protocol["cells"]:
        for run in range(1, protocol["verify_runs"] + 1):
            problem = verify_run_problem(cell, run_rows(verify_by_cell, cell["id"], run))
            if problem is not None:
                problems.append(f"{cell['id']} run {run}: {problem}")
    return {"verify_runs": protocol["verify_runs"], "problem_count": len(problems),
            "problems": problems[:PROBLEM_LIMIT], "passed": not problems}


def determinism_gate(protocol, verify_by_cell, process_rows):
    cells = protocol["cells"]
    run_mismatches = []
    if protocol["verify_runs"] >= 2:
        for cell in cells:
            first = run_rows(verify_by_cell, cell["id"], 1)
            for run in range(2, protocol["verify_runs"] + 1):
                other = run_rows(verify_by_cell, cell["id"], run)
                if (not first or len(first) != len(other)
                        or any(any(left[key] != right[key] for key in DETERMINISTIC_VERIFY_FIELDS)
                               for left, right in zip(first, other))):
                    run_mismatches.append(f"{cell['id']} run {run}")
    chains = {}
    for cell in cells:
        rows = run_rows(verify_by_cell, cell["id"], 1)
        chains[cell["id"]] = sequence_digest([row["target"] for row in rows]) if rows else None
    chain_mismatches = [f"{row['cell_id']} {row['population']} round {row['round']}" for row in process_rows
                        if chains.get(row["cell_id"]) is None or row["target_sha256"] != chains[row["cell_id"]]]
    return {
        "compared_verify_fields": list(DETERMINISTIC_VERIFY_FIELDS),
        "verify_runs_identical": None if protocol["verify_runs"] < 2 else not run_mismatches,
        "verify_run_mismatches": run_mismatches[:PROBLEM_LIMIT],
        "time_chain_mismatch_count": len(chain_mismatches),
        "time_chain_mismatches": chain_mismatches[:PROBLEM_LIMIT],
        "passed": not run_mismatches and not chain_mismatches,
    }


def net_allocation(row, metric):
    net = sum(int(row[f"{phase}_{metric}"]) for phase in VERIFY_PHASES) - int(row[f"process_probe_{metric}"])
    if net < 0:
        raise ValueError(f"negative net {metric} at {row['cell_id']} run {row['run']} commit {row['commit']}: {net}")
    return net


def net_io(row, metric):
    probe = int(row[f"process_probe_{metric}"])
    total = 0
    for phase in VERIFY_PHASES:
        net = int(row[f"{phase}_{metric}"]) - probe
        if net < 0:
            raise ValueError(f"negative net {metric} at {row['cell_id']} run {row['run']} commit {row['commit']} "
                             f"phase {phase}: {net}")
        total += net
    return total


def negative_net_value(verify_by_cell, metric, value):
    try:
        for rows in verify_by_cell.values():
            for row in rows:
                value(row, metric)
    except ValueError as error:
        return str(error)
    return None


def run_metric_values(cells, verify_by_cell, run, metric, value):
    return {cell["id"]: [float(value(row, metric)) for row in run_rows(verify_by_cell, cell["id"], run)
                         if row["label"] == "timed"] for cell in cells}


def per_commit_gate(protocol, verify_by_cell, metrics, value):
    """Slopes of per-commit net values: per verify run, each stratum's T timed commits; every run must pass."""
    cells = protocol["cells"]
    results = {}
    for metric in metrics:
        error = negative_net_value(verify_by_cell, metric, value)
        if error is not None:
            results[metric] = {"error": error, "passed": False}
            continue
        runs = {}
        for run in range(1, protocol["verify_runs"] + 1):
            arms = bounded_arms(cells, run_metric_values(cells, verify_by_cell, run, metric, value), ("U", "R"),
                                EXPONENT_BOUND)
            runs[str(run)] = {"arms": arms, "passed": all(arm["passed"] for arm in arms.values())}
        results[metric] = {"runs": runs, "passed": all(entry["passed"] for entry in runs.values())}
    return {"statistic": "upper95", "bound": EXPONENT_BOUND,
            "samples": "per verify run, the T per-commit net values of each stratum's timed commits",
            "metrics": results, "passed": all(result["passed"] for result in results.values())}


def io_gate(protocol, verify_by_cell):
    gate = per_commit_gate(protocol, verify_by_cell, IO_METRICS, net_io)
    appends = child_assertion("G5", protocol["cells"], protocol["verify_runs"], verify_by_cell)
    return {**gate, "appends": appends, "passed": gate["passed"] and appends["passed"]}


def memory_gate(cells, samples, process_rows, verify_rows_all, failures):
    arms = {}
    for axis in ("U", "R"):
        members = axis_members(cells, axis)
        if len(members) < 2:
            arms[axis] = {"error": f"axis {axis} has fewer than two strata", "passed": False}
            continue
        smallest, largest = members[0], members[-1]
        growth = {cell["id"]: [int(row["rss_end"]) - int(row["rss_start"]) for row in samples.get(cell["id"], [])]
                  for cell in (smallest, largest)}
        arm = rss_growth_bound(growth[largest["id"]], growth[smallest["id"]])
        arm.update(smallest=smallest["id"], largest=largest["id"])
        arms[axis] = arm
    swapped = [f"{row['cell_id']} {row['population']} round {row['round']}" for row in process_rows
               if row["swap"] not in ("", "0")]
    swapped += sorted({f"{row['cell_id']} verify run {row['run']}" for row in verify_rows_all
                       if row["process_swap"] != "0"})
    killed = [failure.get("label") for failure in failures if failure.get("kind") == "oom"]
    return {"arms": arms, "swap_nonzero": swapped[:PROBLEM_LIMIT], "oom_killed": killed[:PROBLEM_LIMIT],
            "passed": all(arm["passed"] for arm in arms.values()) and not swapped and not killed}


def v1_manifest_bytes(cell, commit):
    return V1_MANIFEST_BASE_BYTES + V1_MANIFEST_ENTRY_BYTES * (cell["history"] + commit + 1)


def crossover_summary(cells, verify_by_cell):
    strata = []
    for cell in axis_members(cells, "R"):
        rows = timed_rows(verify_by_cell, cell["id"])
        if not rows:
            continue
        written = statistics.median(int(row["metadata_bytes"]) for row in rows)
        flat = statistics.median(v1_manifest_bytes(cell, int(row["commit"])) for row in rows)
        entry = {"cell": cell["id"], "writer": cell["writer"], "history": cell["history"],
                 "metadata_bytes_median": written, "v1_manifest_bytes_median": flat}
        if cell["writer"] == "v2":
            entry["v1_over_v2"] = flat / written if written else None
            entry["crossover_entries"] = max(
                0, math.floor((written - V1_MANIFEST_BASE_BYTES) / V1_MANIFEST_ENTRY_BYTES) + 1)
        strata.append(entry)
    return {"definition": "per R stratum over its timed verify commits: the measured median metadata bytes, the "
                          "V1 manifest bytes 96 + 56 * (history + commit + 1) of the same commits, and for V2 the "
                          "smallest retained-entry count whose V1 manifest exceeds the measured V2 median",
            "strata": strata}


def metadata_gate(protocol, verify_by_cell):
    cells = protocol["cells"]
    problems = []
    for cell in cells:
        rows = verify_by_cell.get(cell["id"], [])
        if not rows:
            problems.append(f"{cell['id']}: no verify records")
        for row in rows:
            written = int(row["metadata_bytes"])
            where = f"{cell['id']} run {row['run']} commit {row['commit']}"
            if cell["writer"] == "v2":
                if written > METADATA_BYTE_BOUND:
                    problems.append(f"{where}: {written} metadata bytes exceed {METADATA_BYTE_BOUND}")
                if 2 + int(row["meta_exp"]) > METADATA_STEP_BOUND:
                    problems.append(f"{where}: 2 + meta_exp exceeds {METADATA_STEP_BOUND}")
            elif written != v1_manifest_bytes(cell, int(row["commit"])):
                problems.append(f"{where}: {written} V1 manifest bytes differ from 96 + 56 * entries")
    model = child_assertion("G7", cells, protocol["verify_runs"], verify_by_cell)
    return {"v2_metadata_byte_bound": METADATA_BYTE_BOUND, "v2_step_work_bound": METADATA_STEP_BOUND,
            "v1_manifest_bytes": "96 + 56 * (history + commit + 1)", "problem_count": len(problems),
            "problems": problems[:PROBLEM_LIMIT], "model": model,
            "crossover": crossover_summary(cells, verify_by_cell), "passed": not problems and model["passed"]}


def delta_deletion_gate(cells, cpu, verify_by_cell):
    if not (axis_members(cells, "D") or axis_members(cells, "L")):
        return {"applicable": False, "passed": None}
    arms = bounded_arms(cells, cpu, ("D", "L"), LINEAR_WORK_BOUND)
    problems = []
    for cell in axis_members(cells, "L"):
        rows = verify_by_cell.get(cell["id"], [])
        if not rows:
            problems.append(f"{cell['id']}: no verify records")
        for row in rows:
            deleted = int(row["deleted_objects"])
            where = f"{cell['id']} run {row['run']} commit {row['commit']}"
            if deleted <= 0:
                problems.append(f"{where}: deleted_objects is not positive")
            elif int(row["cur_exp"]) - int(row["old_path"]) - int(row["new_path"]) != deleted:
                problems.append(f"{where}: cur_exp - old_path - new_path differs from deleted_objects")
    return {"statistic": "upper95", "bound": LINEAR_WORK_BOUND, "arms": arms, "problem_count": len(problems),
            "problems": problems[:PROBLEM_LIMIT],
            "passed": all(arm["passed"] for arm in arms.values()) and not problems}


def reclaimed_names_are_exact(unlinked):
    names = unlinked.split(";")
    packs = sum(PACK_FILE_PATTERN.fullmatch(name) is not None for name in names)
    indexes = sum(INDEX_FILE_PATTERN.fullmatch(name) is not None for name in names)
    return RECLAIM_INTENT in names and packs >= 1 and indexes == 1 and packs + indexes + 1 == len(names)


def eviction_gate(protocol, samples, verify_by_cell):
    members = axis_members(protocol["cells"], "E")
    if not members:
        return {"applicable": False, "passed": None}
    problems = []
    differences = {}
    for cell in members:
        for run in range(1, protocol["verify_runs"] + 1):
            rows = run_rows(verify_by_cell, cell["id"], run)
            if sum(row["label"] == "reclaim" for row in rows) != 1:
                problems.append(f"{cell['id']} run {run}: not exactly one reclaim commit")
            for row in rows:
                where = f"{cell['id']} run {run} commit {row['commit']}: unlinked={row['unlinked']!r}"
                reclaim = row["label"] == "reclaim"
                if reclaim == (row["unlinked"] == "-"):
                    problems.append(f"{where} at a {row['label']} commit")
                elif reclaim and not reclaimed_names_are_exact(row["unlinked"]):
                    problems.append(f"{where} is not the superseded pack files, one index file and {RECLAIM_INTENT}")
        processes = samples.get(cell["id"], [])
        try:
            deferred = [float(row["deferred_thread_ns"]) - float(row["thread_ns"]) for row in processes]
            reclaim = [float(row["reclaim_thread_ns"]) - float(row["thread_ns"]) for row in processes]
        except ValueError as error:
            differences[cell["id"]] = {"error": str(error)}
            continue
        differences[cell["id"]] = {"deferred_minus_timed_ns": median_interval(deferred),
                                   "reclaim_minus_timed_ns": median_interval(reclaim)}
    effects = child_assertion("G9", members, protocol["verify_runs"], verify_by_cell)
    return {"problem_count": len(problems), "problems": problems[:PROBLEM_LIMIT], "trusted_effects": effects,
            "within_process_thread_cpu_differences": differences, "passed": not problems and effects["passed"]}


def control_gate(protocol, cpu, verify_by_cell):
    if protocol["mode"] != "controls":
        return {"applicable": False, "passed": None,
                "reason": "each control is its own --controls population; see that population's analysis.json"}
    cells, control = protocol["cells"], protocol["control"]
    conditions, observations, exponents = {}, {}, {}

    def lower_bound_exceeds(name, values_by_cell, axis):
        arm = arm_slope(cells, values_by_cell, axis)
        exponents[f"{name}_{axis}"] = arm
        exceeds = "error" not in arm and arm["lower95"] > EXPONENT_BOUND
        conditions[f"{name}_lower95_exceeds_{EXPONENT_BOUND}_on_{axis}"] = exceeds

    def verify_values(metric, value):
        error = negative_net_value(verify_by_cell, metric, value)
        if error is not None:
            observations[f"{metric}_error"] = error
            return {}
        return run_metric_values(cells, verify_by_cell, 1, metric, value)

    if control == "NC-SCAN":
        rchar = verify_values("rchar", net_io)
        for axis in ("U", "R"):
            lower_bound_exceeds("cpu", cpu, axis)
            lower_bound_exceeds("rchar", rchar, axis)
        observations["g1_passes_at_every_cell"] = all(
            verify_run_problem(cell, run_rows(verify_by_cell, cell["id"], 1)) is None for cell in cells)
    elif control == "NC-CLONE":
        allocations = {metric: verify_values(metric, net_allocation) for metric in ALLOCATION_METRICS}
        for axis in ("U", "R"):
            lower_bound_exceeds("cpu", cpu, axis)
            for metric in ALLOCATION_METRICS:
                lower_bound_exceeds(metric, allocations[metric], axis)
    elif control == "NC-ORDER":
        conditions["g1_fails_at_every_cell"] = all(
            verify_run_problem(cell, run_rows(verify_by_cell, cell["id"], 1)) is not None for cell in cells)
        lower_bound_exceeds("cpu", cpu, "U")
        observations["sw01"] = "not evaluated here: SW-01 is a normal-suite property run against the mutant export"
    else:
        conditions["v1_metadata_bytes_equal_96_plus_56_entries"] = all(
            bool(verify_by_cell.get(cell["id"]))
            and all(int(row["metadata_bytes"]) == v1_manifest_bytes(cell, int(row["commit"]))
                    for row in verify_by_cell[cell["id"]])
            for cell in cells)
        lower_bound_exceeds("cpu", cpu, "R")
    detected = all(conditions.values())
    return {"applicable": True, "control": control, "expectation": CONTROL_EXPECTATIONS[control],
            "conditions": conditions, "observations": observations, "exponents": exponents,
            "detected": detected, "passed": detected}


def spread(values):
    if not values:
        return {"n": 0}
    return {"n": len(values), "median": statistics.median(values), "minimum": min(values), "maximum": max(values)}


def descriptive_summary(protocol, samples, verify_by_cell, fixture_rows, process_rows, cpu):
    cells = protocol["cells"]
    fixtures = group_rows(fixture_rows)
    all_processes = group_rows(process_rows)
    per_cell = {}
    for cell in cells:
        processes = samples.get(cell["id"], [])
        verified = verify_by_cell.get(cell["id"], [])
        timed = timed_rows(verify_by_cell, cell["id"])
        attempted = all_processes.get(cell["id"], [])
        per_cell[cell["id"]] = {
            "axes": cell["axes"],
            "cell": cell["cell"],
            "sample_processes": len(processes),
            "timing_ns": {column: median_interval([float(row[column]) for row in processes])
                          for column in TIMING_COLUMNS},
            "admission_ns": median_interval([float(row["admission_ns"]) for row in processes]),
            "sealing_counters": {name: spread([int(row[name]) for row in timed]) for name in SEALING_COUNTERS},
            "live_packs": sorted({int(row["live_packs"]) for row in processes}
                                 | {int(row["process_live_packs"]) for row in verified}),
            "hwm_reset": dict(sorted(Counter(row["hwm_reset"] for row in processes).items())),
            "feasibility": {
                "build_duration_ns": [int(row["duration_ns"]) for row in fixtures.get(cell["id"], [])],
                "verify_duration_ns_max": max((int(row["duration_ns"]) for row in verified), default=None),
                "time_duration_ns_max": max((int(row["duration_ns"]) for row in attempted), default=None),
                "time_hwm_window_max": max((int(row["hwm_window"]) for row in attempted if row["hwm_window"]),
                                           default=None),
                "verify_hwm_window_max": max((int(row["process_hwm_window"]) for row in verified), default=None),
            },
        }
    return {"per_cell": per_cell, "c_star": {axis: c_star_summary(cells, cpu, axis) for axis in ("U", "R")}}


def analyze(metadata, fixture_rows, verify_rows_all, process_rows):
    recorded = metadata.get("protocol") or {}
    protocol = build_protocol(recorded.get("mode"), recorded.get("control"))
    cells, mode = protocol["cells"], protocol["mode"]
    failures = (metadata.get("collection") or {}).get("failures") or []
    problems = qualification_problems(metadata, protocol, fixture_rows, verify_rows_all, process_rows)
    verify_by_cell = group_rows(verify_rows_all)
    samples = successful_processes(process_rows, protocol["sample_population"])
    cpu = {cell_id: [float(row["thread_ns"]) for row in rows] for cell_id, rows in samples.items()}
    wall = {cell_id: [float(row["wall_ns"]) for row in rows] for cell_id, rows in samples.items()}
    cpu_arms = bounded_arms(cells, cpu, ("U", "R"), EXPONENT_BOUND)
    wall_arms = bounded_arms(cells, wall, ("U", "R"), EXPONENT_BOUND)
    gates = {
        "H1": {"metric": "per-commit thread CPU ns, median over each process's timed commits", "statistic": "upper95",
               "bound": EXPONENT_BOUND, "arms": cpu_arms, "passed": all(arm["passed"] for arm in cpu_arms.values())},
        "G1": exact_work_gate(protocol, verify_by_cell),
        "G2": determinism_gate(protocol, verify_by_cell, process_rows),
        "G3": {"metric": "per-commit wall ns, median over each process's timed commits", "statistic": "upper95",
               "bound": EXPONENT_BOUND, "arms": wall_arms,
               "passed": all(arm["passed"] for arm in wall_arms.values())},
        "G4": per_commit_gate(protocol, verify_by_cell, ALLOCATION_METRICS, net_allocation),
        "G5": io_gate(protocol, verify_by_cell),
        "G6": memory_gate(cells, samples, process_rows, verify_rows_all, failures),
        "G7": metadata_gate(protocol, verify_by_cell),
        "G8": delta_deletion_gate(cells, cpu, verify_by_cell),
        "G9": eviction_gate(protocol, samples, verify_by_cell),
        "G10": control_gate(protocol, cpu, verify_by_cell),
    }
    qualified = not problems
    if mode == "decision":
        accepted = qualified and all(gates[name]["passed"] is True for name in DECISION_GATES)
    elif mode == "controls":
        accepted = qualified and gates["G10"]["passed"] is True
    else:
        accepted = False
    return {
        "task": TASK,
        "mode": mode,
        "control": protocol["control"],
        "decision_data": mode == "decision",
        "pilot_only": mode == "pilot",
        "sample_population": protocol["sample_population"],
        "qualified": qualified,
        "qualification_problem_count": len(problems),
        "qualification_problems": problems[:PROBLEM_LIMIT],
        "gates": gates,
        "descriptive": descriptive_summary(protocol, samples, verify_by_cell, fixture_rows, process_rows, cpu),
        "accepted": accepted,
    }


def analyze_directory(directory):
    metadata = json.loads((directory / "metadata.json").read_text(encoding="utf-8"))
    return analyze(metadata, read_table(directory / "fixtures.tsv", FIXTURE_COLUMNS),
                   read_table(directory / "verify.tsv", VERIFY_COLUMNS),
                   read_table(directory / "process-summary.tsv", PROCESS_COLUMNS))


def failed_gates(analysis):
    if analysis["mode"] == "controls":
        return [] if analysis["gates"]["G10"]["passed"] else ["G10 (the control was not detected)"]
    return [name for name in DECISION_GATES if analysis["gates"][name]["passed"] is not True]


def check_directory(directory):
    """Verify the manifest digests of present files and that the recomputed analysis matches analysis.json."""
    problems = []
    manifest_path = directory / "manifest.json"
    listed = {}
    if manifest_path.is_file():
        listed = json.loads(manifest_path.read_text(encoding="utf-8")).get("files") or {}
    else:
        problems.append("manifest.json is absent")
    for name in (*ANALYSIS_INPUTS, "analysis.json"):
        if name not in listed:
            problems.append(f"manifest.json does not list {name}")
    for name, entry in sorted(listed.items()):
        if Path(name).name != name or name == "manifest.json":
            problems.append(f"manifest.json lists an invalid name {name!r}")
            continue
        path = directory / name
        if not path.is_file():
            print(f"skipped (absent): {name}")
            continue
        if sha256_file(path) != entry.get("sha256"):
            problems.append(f"sha256 mismatch: {name}")
    missing = [name for name in ANALYSIS_INPUTS if not (directory / name).is_file()]
    if missing:
        problems.append(f"the analysis inputs {missing} are absent")
        return None, problems
    analysis = analyze_directory(directory)
    existing = directory / "analysis.json"
    if not existing.is_file():
        problems.append("analysis.json is absent")
    elif json.loads(existing.read_text(encoding="utf-8")) != canonical_json(analysis):
        problems.append("the recomputed analysis differs from analysis.json")
    if not analysis["qualified"]:
        problems.append("the population is not qualified")
    if analysis["mode"] == "pilot":
        problems.append("a pilot population is never decision data")
    elif not analysis["accepted"]:
        problems.append(f"failed: {', '.join(failed_gates(analysis))}")
    return analysis, problems


def gate_state(gate):
    if gate.get("applicable") is False:
        return "not applicable"
    return "passed" if gate["passed"] else "failed"


def print_report(analysis):
    print(f"mode={analysis['mode']} control={analysis['control']} qualified={analysis['qualified']} "
          f"accepted={analysis['accepted']}")
    for name, gate in analysis["gates"].items():
        print(f"  {name}: {gate_state(gate)}")
    for problem in analysis["qualification_problems"][:10]:
        print(f"  not qualified: {problem}")


def exit_status(analysis):
    if analysis["mode"] == "pilot":
        return 0 if analysis["qualified"] else 1
    return 0 if analysis["accepted"] else 1


SYNTHETIC_PROBES = {"syscr": 2, "syscw": 0, "rchar": 118, "wchar": 0, "alloc_count": 2, "alloc_bytes": 24}
SYNTHETIC_RSS = 100 * 2**20
SYNTHETIC_FAULTS = ("silent", "omit-last", "omit-commit", "duplicate-commit", "malformed", "exit", "timeout", "oom")
SYNTHETIC_BUILD_FAULTS = ("silent", "omit-last", "malformed", "exit", "timeout", "oom")
SYNTHETIC_FIXTURE_FILES = ("catalog.v1", "index-00000000000000000001.rsi", "pack-00000000000000000001.rsp")
SYNTHETIC_RECLAIMED = "index-00000000000000000001.rsi;pack-00000000000000000001.rsp;reclaim.v1"
TERSE_COUNTS = "0 ignored; 0 measured; 0 filtered out; finished in 0.01s"


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def refuses(action, error_type, message):
    try:
        action()
    except error_type:
        return
    raise AssertionError(message)


def replaced(line, old, new):
    require(old in line, f"{old!r} does not occur in the record")
    return line.replace(old, new, 1)


def format_record(tag, values):
    return "\t".join([tag, RECORD_VERSION, *(f"{key}={values[key]}" for key in RECORD_KEYS[tag])])


def synthetic_target(cell_text, commit):
    return hashlib.sha256(f"{cell_text}#{commit}".encode()).hexdigest()


def synthetic_scale(cell, exponent):
    return ((cell["keys"] / 2**10) * (cell["history"] / 2**8) * max(cell["delta"], cell["deletion"])) ** exponent


def synthetic_fixture(cell):
    total = cell["keys"] + cell["reserve"]
    return {"keys_total": total, "state_objects": 3 * total, "state_bytes": 160 * total, "history": cell["history"],
            "union_objects": 3 * total + cell["history"], "pack_bytes": 200 * total, "index_bytes": 48 * total,
            "records": 3 * total + cell["history"] + 2, "build_ns": 7_000_000}


def synthetic_verify(cell, cell_text, commit, label):
    deleted = 2 * cell["deletion"] - 1 if cell["deletion"] else 0
    values = {"commit": commit, "label": label, "target": synthetic_target(cell_text, commit), "cur_refs": 41,
              "cur_exp": 12 + deleted, "cur_quoted": 960, "ret_refs": 21, "ret_exp": 6, "ret_quoted": 480,
              "meta_refs": 14, "meta_exp": 7, "meta_quoted": 2600, "logical_writes": 81,
              "metadata_bytes": v1_manifest_bytes(cell, commit) if cell["writer"] == "v1" else 2296,
              "fresh_objects": 9, "fresh_bytes": 1400, "pack_delta": 2100, "index_delta": 432, "live_packs": 1,
              "old_path": 6, "new_path": 6, "deleted_objects": deleted, "loaded_nodes": 9, "edited_nodes": 7,
              "emitted_nodes": 7, "exact_path_chunks": 3}
    for phase in VERIFY_PHASES:
        active = phase == "publish"
        values.update({
            f"{phase}_alloc_count": 40 + (SYNTHETIC_PROBES["alloc_count"] if active else 0),
            f"{phase}_alloc_bytes": 4096 + (SYNTHETIC_PROBES["alloc_bytes"] if active else 0),
            f"{phase}_alloc_peak": 2048,
            f"{phase}_alloc_max": 512,
            f"{phase}_syscr": SYNTHETIC_PROBES["syscr"] + (3 if active else 0),
            f"{phase}_syscw": SYNTHETIC_PROBES["syscw"] + (4 if active else 0),
            f"{phase}_rchar": SYNTHETIC_PROBES["rchar"] + (4096 if active else 0),
            f"{phase}_wchar": SYNTHETIC_PROBES["wchar"] + (6144 if active else 0),
        })
    values["unlinked"] = SYNTHETIC_RECLAIMED if label == "reclaim" else "-"
    return values


def synthetic_sample(cell, cell_text, commit, label, exponent):
    weight = {"warmup": 3, "deferred": 2, "reclaim": 4, "timed": 1}[label]
    base = round(10_000 * synthetic_scale(cell, exponent))
    values = {"commit": commit, "label": label, "target": synthetic_target(cell_text, commit)}
    for position, phase in enumerate(TIME_PHASES, start=1):
        thread = base * weight * position
        values.update({f"{phase}_thread_ns": thread, f"{phase}_process_ns": thread, f"{phase}_wall_ns": 2 * thread})
    values["rss_after"] = SYNTHETIC_RSS
    return values


def synthetic_process(mode, commits):
    verify = mode == "verify"
    return {"mode": mode, "commits": commits, "admission_ns": 25_000_000, "rss_start": SYNTHETIC_RSS,
            "rss_end": SYNTHETIC_RSS, "hwm_window": SYNTHETIC_RSS + 2**20, "hwm_reset": 1, "swap": 0,
            "live_packs": 1, "filesystem": "ext4",
            **{f"probe_{name}": value if verify else 0 for name, value in SYNTHETIC_PROBES.items()}}


def synthetic_lines(mode, cell_text, exponent):
    cell = parse_cell_string(cell_text)
    labels = expected_labels(cell)
    lines = [format_record("SCALE_HEADER", {"mode": mode, "cell": cell_text})]
    if mode == "build":
        lines.append(format_record("SCALE_FIXTURE", synthetic_fixture(cell)))
    elif mode == "verify":
        lines += [format_record("SCALE_VERIFY", synthetic_verify(cell, cell_text, commit, label))
                  for commit, label in enumerate(labels)]
        lines.append(format_record("SCALE_PROCESS", synthetic_process(mode, len(labels))))
    else:
        lines += [format_record("SCALE_SAMPLE", synthetic_sample(cell, cell_text, commit, label, exponent))
                  for commit, label in enumerate(labels)]
        lines.append(format_record("SCALE_PROCESS", synthetic_process(mode, len(labels))))
    return lines


def terse_output(lines):
    """Stdout of a passing ``--quiet`` libtest run whose test printed ``lines``."""
    return ("\nrunning 1 test\n" + "".join(f"{line}\n" for line in lines)
            + f".\ntest result: ok. 1 passed; 0 failed; {TERSE_COUNTS}\n\n")


def terse_failure(lines):
    return ("\nrunning 1 test\n" + "".join(f"{line}\n" for line in lines)
            + f"\nthread '{CHILD_TEST}' panicked at rspace++/tests/persistence/trusted_scaling_resource.rs:1:1:\n"
            + f"synthetic verify failure\n{CHILD_TEST} --- FAILED\n\nfailures:\n    {CHILD_TEST}\n\n"
            + f"test result: FAILED. 0 passed; 1 failed; {TERSE_COUNTS}\n\n")


def synthetic_output(fault, lines):
    match fault:
        case None:
            pass
        case "silent":
            return 0, f"\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; {TERSE_COUNTS}\n\n"
        case "timeout":
            raise ChildTimeout("\nrunning 1 test\n" + "".join(f"{line}\n" for line in lines[:2]))
        case "oom":
            return -9, "\nrunning 1 test\n" + "".join(f"{line}\n" for line in lines[:2])
        case "exit":
            return 101, terse_failure(lines[:2])
        case "omit-last":
            lines = lines[:-1]
        case "omit-commit":
            lines = lines[:2] + lines[3:]
        case "duplicate-commit":
            lines = lines[:2] + lines[1:]
        case "malformed":
            lines = [lines[0], lines[1] + "\textra=1", *lines[2:]]
        case _:
            raise ValueError(f"unknown synthetic fault {fault!r}")
    return 0, terse_output(lines)


class SyntheticChild:
    """Deterministic stand-in for the Rust scale child; faults are keyed by (mode, cell string, launch ordinal)."""

    def __init__(self, exponent=0.0, faults=None):
        self.exponent = exponent
        self.faults = dict(faults or {})
        self.ordinals = Counter()
        self.launches = 0

    def __call__(self, command, environment, timeout):
        self.launches += 1
        require(command[:2] == ["taskset", "-c"] and tuple(command[4:]) == CHILD_ARGUMENTS, "child command shape")
        mode = environment["RSPACE_SCALE_MODE"]
        cell_text = environment["RSPACE_SCALE_CELL"]
        fixture = Path(environment["RSPACE_SCALE_FIXTURE"])
        if mode == "build":
            require("RSPACE_SCALE_SCRATCH" not in environment, "a build child received a scratch directory")
            require(fixture.is_dir() and not any(fixture.iterdir()), "a build fixture directory exists and is empty")
            require(private_directory(fixture), "a build fixture directory is open to the group or others")
            for name in SYNTHETIC_FIXTURE_FILES:
                (fixture / name).write_text(f"{name} {cell_text}\n")
        else:
            scratch = Path(environment["RSPACE_SCALE_SCRATCH"])
            require(scratch.is_dir() and not any(scratch.iterdir()), "a scratch directory exists and is empty")
            require(private_directory(scratch), "a scratch directory is open to the group or others")
            for name in SYNTHETIC_FIXTURE_FILES:
                shutil.copyfile(fixture / name, scratch / name)
        ordinal = self.ordinals[mode, cell_text]
        self.ordinals[mode, cell_text] += 1
        fault = self.faults.get((mode, cell_text, ordinal))
        return synthetic_output(fault, synthetic_lines(mode, cell_text, self.exponent))


def private_directory(path):
    return stat.S_IMODE(path.stat().st_mode) & 0o077 == 0


class SyntheticHost:
    def governor(self, cpu) -> str | None:
        return "performance"

    def filesystem(self, path):
        return "ext2/ext3"

    def facts(self, binary, directory, cpu):
        return {"synthetic": True, "binary": str(binary), "cpu": cpu}


class PowersaveHost(SyntheticHost):
    def governor(self, cpu) -> str | None:
        return "powersave"


def idle_sampler(cpu) -> tuple[str, bool]:
    return json.dumps({"synthetic": True, "cpu": cpu}), True


def self_test_root():
    value = os.environ.get("TMPDIR")
    if not value:
        raise SystemExit("--self-test needs TMPDIR set to an existing directory outside /tmp")
    resolved = Path(value).resolve()
    if any(resolved == base or base in resolved.parents for base in {Path("/tmp"), Path("/tmp").resolve()}):
        raise SystemExit(f"--self-test refuses TMPDIR {resolved}: it resolves under /tmp")
    if not resolved.is_dir():
        raise SystemExit(f"--self-test needs TMPDIR {resolved} to be an existing directory")
    return resolved


def check_record_contract():
    cell = make_cell("U", 2**10, BASE_HISTORY)
    text = cell["cell"]
    labels = expected_labels(cell)
    valid = {
        "SCALE_HEADER": {"mode": "verify", "cell": text},
        "SCALE_FIXTURE": synthetic_fixture(cell),
        "SCALE_VERIFY": synthetic_verify(cell, text, 0, "warmup"),
        "SCALE_SAMPLE": synthetic_sample(cell, text, 0, "warmup", 0.0),
        "SCALE_PROCESS": synthetic_process("verify", len(labels)),
    }
    for tag, values in valid.items():
        require(parse_record(format_record(tag, values)) == (tag, values), f"{tag} does not round-trip")
    line = format_record("SCALE_VERIFY", valid["SCALE_VERIFY"])
    fields = line.split("\t")
    target = valid["SCALE_VERIFY"]["target"]
    malformed = [
        "\t".join(["SCALE_COMMIT", *fields[1:]]),
        "\t".join([fields[0], "v2", *fields[2:]]),
        "\t".join([fields[0], *fields[2:]]),
        "\t".join(fields[:-1]),
        "\t".join([*fields, "extra=1"]),
        "\t".join([*fields[:2], fields[3], fields[2], *fields[4:]]),
        "\t".join([*fields[:3], fields[2], *fields[4:]]),
        replaced(line, "\tcur_refs=41\t", "\tcur_refs=4.1\t"),
        replaced(line, "\tcur_refs=41\t", "\tcur_refs=-41\t"),
        replaced(line, "\tcur_refs=41\t", "\tcur_refs=041\t"),
        replaced(line, "\tcur_refs=41\t", "\tcur_refs=\t"),
        replaced(line, "\tcur_refs=41\t", "\tcur_refs\t"),
        replaced(line, "\tcur_refs=41\t", "\tcur_refs=0x29\t"),
        replaced(line, "\tcur_refs=41\t", f"\tcur_refs={U64_MAX + 1}\t"),
        replaced(line, "\tlabel=warmup\t", "\tlabel=cold\t"),
        replaced(line, f"\ttarget={target}\t", f"\ttarget={target.upper()}\t"),
        replaced(line, f"\ttarget={target}\t", f"\ttarget={target[:-2]}\t"),
        replaced(line, "\tunlinked=-", "\tunlinked=pack-1;index-1"),
        replaced(line, "\tunlinked=-", "\tunlinked=index-1;;pack-1"),
        format_record("SCALE_SAMPLE", dict(valid["SCALE_SAMPLE"], seal_thread_ns=0)),
        format_record("SCALE_PROCESS", dict(valid["SCALE_PROCESS"], hwm_reset=2)),
        format_record("SCALE_PROCESS", dict(valid["SCALE_PROCESS"], filesystem="tmpfs")),
        format_record("SCALE_PROCESS", dict(valid["SCALE_PROCESS"], mode="build")),
        format_record("SCALE_PROCESS", dict(valid["SCALE_PROCESS"], probe_alloc_bytes="24.0")),
        format_record("SCALE_HEADER", {"mode": "replay", "cell": text}),
        format_record("SCALE_HEADER", {"mode": "time", "cell": ""}),
        format_record("SCALE_FIXTURE", dict(valid["SCALE_FIXTURE"], build_ns="1e6")),
    ]
    for record in malformed:
        refuses(lambda: parse_record(record), MalformedRecord, f"accepted a malformed record {record[:72]!r}")

    header = ("SCALE_HEADER", {"mode": "verify", "cell": text})
    commits = [("SCALE_VERIFY", synthetic_verify(cell, text, commit, label)) for commit, label in enumerate(labels)]
    process = ("SCALE_PROCESS", synthetic_process("verify", len(labels)))
    complete = [header, *commits, process]
    require(len(validate_child_records("verify", cell, complete)["commits"]) == len(labels), "complete verify set")
    other = make_cell("U", 2**12, BASE_HISTORY)
    incomplete = {
        "no records": [],
        "a missing header": complete[1:],
        "another cell's header": [("SCALE_HEADER", {"mode": "verify", "cell": other["cell"]}), *complete[1:]],
        "another mode's header": [("SCALE_HEADER", {"mode": "time", "cell": text}), *complete[1:]],
        "a missing process record": complete[:-1],
        "a missing commit": [header, *commits[:5], *commits[6:], process],
        "a duplicate commit": [header, *commits[:6], commits[5], *commits[6:], process],
        "a relabeled commit": [header, ("SCALE_VERIFY", dict(commits[0][1], label="timed")), *commits[1:], process],
        "a stray sample": [header, *commits, ("SCALE_SAMPLE", synthetic_sample(cell, text, 0, "warmup", 0.0)),
                           process],
        "a second header": [header, *commits, header, process],
        "a short process count": [header, *commits, ("SCALE_PROCESS", synthetic_process("verify", len(labels) - 1))],
    }
    for name, records in incomplete.items():
        refuses(lambda: validate_child_records("verify", cell, records), RecordError, f"accepted {name}")
    refuses(lambda: validate_child_records("verify", cell, incomplete["a duplicate commit"]), MalformedRecord,
            "a duplicate commit index is not refused as malformed")
    samples = [("SCALE_SAMPLE", synthetic_sample(cell, text, commit, label, 0.0))
               for commit, label in enumerate(labels)]
    timing_header = ("SCALE_HEADER", {"mode": "time", "cell": text})
    timing = synthetic_process("time", len(labels))
    validate_child_records("time", cell, [timing_header, *samples, ("SCALE_PROCESS", timing)])
    for probe in PROCESS_PROBE_KEYS:
        refuses(lambda: validate_child_records("time", cell, [timing_header, *samples,
                                                                ("SCALE_PROCESS", dict(timing, **{probe: 1}))]),
                RecordError, f"time mode accepted a nonzero {probe}")
    build_header = ("SCALE_HEADER", {"mode": "build", "cell": text})
    fixture = ("SCALE_FIXTURE", synthetic_fixture(cell))
    validate_child_records("build", cell, [build_header, fixture])
    for records in ([build_header], [build_header, fixture, fixture], [build_header, fixture, process]):
        refuses(lambda: validate_child_records("build", cell, records), RecordError,
                "build mode accepted a missing or extra record")
    framed = terse_output([format_record(*record) for record in complete])
    require(len(validate_child_records("verify", cell, child_records(framed))["commits"]) == len(labels),
            "libtest's terse framing hid a record")
    pretty = framed.replace("\nSCALE_HEADER", f"\ntest {CHILD_TEST} ... SCALE_HEADER", 1)
    require(pretty != framed, "the pretty-formatter prefix was not applied")
    refuses(lambda: validate_child_records("verify", cell, child_records(pretty)), RecordError,
            "a header behind libtest's pretty test-start prefix was accepted")
    print(f"record contract: {len(valid)} record kinds accepted; {len(malformed)} malformed records and "
          f"{len(incomplete) + len(PROCESS_PROBE_KEYS) + 4} incomplete record sets refused")


def check_cells_and_guard():
    cells = decision_cells()
    require(len(cells) == 21 and len({cell["id"] for cell in cells}) == 21
            and len({cell["cell"] for cell in cells}) == 21, "the decision population has 21 distinct cells")
    shared = {cell["id"]: cell["axes"] for cell in cells if len(cell["axes"]) > 1}
    require(shared == {"U-k4096-h1024-d1-m0-v2-ordinary": ["U", "R"], "U-k16384-h1024-d1-m0-v2-ordinary": ["U", "D"]},
            f"shared cells {shared}")
    for axis, strata in (("U", U_KEYS), ("R", R_HISTORIES), ("D", D_DELTAS), ("L", L_DELETIONS), ("E", E_KEYS)):
        require(tuple(cell[STRATUM_KEYS[axis]] for cell in axis_members(cells, axis)) == strata,
                f"axis {axis} strata")
    require(all(cell["reserve"] == RESERVE_GROUPS * cell["deletion"] and cell["delta"] == 0
                for cell in axis_members(cells, "L")), "L cells carry 36 groups and no replacement")
    require(all((cell["warmup"], cell["deferred"], cell["timed"], cell["case"]) == (4, 8, 16, "evict")
                for cell in axis_members(cells, "E")), "E cells use W=4, K=8, T=16")
    require(all((cell["warmup"], cell["deferred"], cell["timed"], cell["case"]) == (4, 0, 32, "ordinary")
                for cell in cells if cell["axis"] != "E"), "ordinary cells use W=4, T=32")
    require(all(cell["seed"] == SEED and parse_cell_string(cell["cell"]) == {key: cell[key] for key in CELL_KEYS}
                for cell in cells), "cell strings round-trip with seed 7929")
    require(len(build_protocol("pilot", None)["cells"]) == 10, "the pilot visits each axis's extremes")
    for control in CONTROLS:
        controlled = build_protocol("controls", control)["cells"]
        writers = {cell["writer"] for cell in controlled}
        require(all(cell["timed"] == CONTROL_TIMED and cell["warmup"] == WARMUP_COMMITS for cell in controlled),
                f"{control} cells use T=8")
        if control == "NC-V1":
            require(len(controlled) == 3 and writers == {"v1"} and all(cell["axes"] == ["R"] for cell in controlled),
                    "NC-V1 uses three V1 cells on R")
        else:
            require(len(controlled) == 6 and writers == {"v2"}, f"{control} uses the U and R extremes and middles")
    refuses(lambda: build_protocol("decision", "NC-SCAN"), ValueError, "a decision population accepted a control")
    refuses(lambda: collection_guard({}, "performance", False), CollectionRefused, "missing SCALE_CAPPED accepted")
    refuses(lambda: collection_guard({"SCALE_CAPPED": "0"}, "performance", True), CollectionRefused,
            "SCALE_CAPPED=0 accepted")
    refuses(lambda: collection_guard({"SCALE_CAPPED": "1"}, "powersave", False), CollectionRefused,
            "powersave accepted")
    refuses(lambda: collection_guard({"SCALE_CAPPED": "1"}, None, False), CollectionRefused,
            "an unread governor accepted")
    collection_guard({"SCALE_CAPPED": "1"}, "powersave", True)
    collection_guard({"SCALE_CAPPED": "1"}, "performance", False)
    with contextlib.redirect_stderr(io.StringIO()):
        for arguments in (["--pilot", "--analyze-only", "--output", "unused"],
                          ["--pilot", "--check", "--output", "unused"],
                          ["--output", "unused", "--binary", "unused"]):
            refuses(lambda: main(arguments), SystemExit, f"the command line {arguments} was accepted")
    print("cells and guard: 21 decision cells (U(2^12)=R(2^10), U(2^14)=D(1)), 10 pilot cells, 6 or 3 control cells; "
          "SCALE_CAPPED and governor guards refuse")


def check_idle_admission():
    for decisions in ([True], [False, False, True], [False] * IDLE_SNAPSHOTS):
        pending, observed = iter(decisions), []
        try:
            await_idle(2, lambda attempt, raw, idle: observed.append((attempt, raw, idle)),
                       lambda cpu: (str(cpu), next(pending)))
        except IdleAdmissionError:
            require(not any(decisions), "an idle snapshot was refused")
        else:
            require(decisions[-1], "contention was admitted")
        require(observed == [(index, "2", idle) for index, idle in enumerate(decisions)], "idle decisions recorded")

    def broken_sampler(cpu):
        raise OSError("snapshot unavailable")

    refuses(lambda: await_idle(2, lambda *details: None, broken_sampler), OSError,
            "a snapshot failure permitted measurement")
    print("idle admission: idle, delayed and contended snapshots; a snapshot failure refuses measurement")


def check_bootstraps():
    generator = random.Random(SEED)
    smallest = [SYNTHETIC_RSS + generator.randrange(-2**16, 2**16) for _ in range(DECISION_ROUNDS)]
    unchanged = rss_growth_bound(list(smallest), smallest)
    require(unchanged["passed"] and unchanged["difference_of_medians"] == 0, f"zero growth refused: {unchanged}")
    grown = rss_growth_bound([value + 2 * 2**20 for value in smallest], smallest)
    require(not grown["passed"] and grown["lower2_5"] > RSS_GROWTH_BOUND, f"2 MiB growth accepted: {grown}")
    constant = median_interval([5.0] * 7)
    require(constant["lower95"] == constant["upper95"] == constant["median"] == 5.0, "constant interval")
    ranked = median_interval([float(value) for value in range(1, 32)])
    require(ranked["lower95"] < ranked["median"] < ranked["upper95"], "the interval brackets the median")
    print(f"delta-RSS bootstrap: zero growth passes (upper97.5 {unchanged['upper97_5']:.0f} B); "
          f"+2 MiB fails (upper97.5 {grown['upper97_5']:.0f} B)")


def check_additive_threshold():
    strata = [2**10, 2**12, 2**14]
    generator = random.Random(SEED)
    base = [[100_000 * (1 + generator.uniform(-0.02, 0.02)) for _ in range(3)] for _ in strata]
    thresholds = []
    for term in (0.0, 2**-12, 2**-8, 2**-6, 2**-4, 2**-2, 1.0, 4.0, 16.0):
        groups = [[value + term * stratum for value in group] for stratum, group in zip(strata, base)]
        coefficient, _, _ = additive_threshold(strata, groups)
        thresholds.append(math.inf if coefficient is None else coefficient)
    require(all(later <= earlier for earlier, later in zip(thresholds, thresholds[1:])),
            f"c* grew with the additive term: {thresholds}")
    require(math.isfinite(thresholds[0]) and thresholds[0] > 0 and thresholds[-1] == 0.0,
            f"c* endpoints: {thresholds}")
    print(f"c* monotonicity: additive terms 0..16 ns per unit give non-increasing c* "
          f"{thresholds[0]:.4g} .. {thresholds[-1]:g}")


def check_gate_rules():
    cell = make_cell("U", 2**10, BASE_HISTORY)
    record = synthetic_verify(cell, cell["cell"], 0, "warmup")
    process = synthetic_process("verify", len(expected_labels(cell)))
    row = {"cell_id": cell["id"], "run": "1", **{key: str(value) for key, value in record.items()},
           **{f"process_{key}": str(value) for key, value in process.items()}}
    require(net_allocation(row, "alloc_count") == 4 * 40 and net_allocation(row, "alloc_bytes") == 4 * 4096,
            "the trace probe is not subtracted exactly once per commit")
    require([net_io(row, metric) for metric in IO_METRICS] == [3, 4, 4096, 6144],
            "the /proc/self/io probe is not subtracted once per phase")
    short = dict(row, seal_alloc_count="0", bind_alloc_count="0", prepare_alloc_count="0", publish_alloc_count="1")
    refuses(lambda: net_allocation(short, "alloc_count"), ValueError, "a negative net allocation count was clamped")
    gate = per_commit_gate({"cells": [cell], "verify_runs": 1}, {cell["id"]: [short]}, ALLOCATION_METRICS,
                           net_allocation)
    require(not gate["passed"] and "error" in gate["metrics"]["alloc_count"], "a negative net allocation passed G4")
    starved = dict(row, seal_rchar=str(SYNTHETIC_PROBES["rchar"] - 1))
    refuses(lambda: net_io(starved, "rchar"), ValueError, "a negative net rchar was clamped")
    pack, index = "pack-00000000000000000002.rsp", "index-00000000000000000001.rsi"
    require(reclaimed_names_are_exact(SYNTHETIC_RECLAIMED)
            and reclaimed_names_are_exact(f"{index};pack-00000000000000000001.rsp;{pack};{RECLAIM_INTENT}"),
            "an exact reclaimed set was refused")
    for unlinked in (f"{index};{pack}", f"{pack};{RECLAIM_INTENT}", f"catalog.v1;{index};{pack};{RECLAIM_INTENT}",
                     f"{index};index-00000000000000000002.rsi;{pack};{RECLAIM_INTENT}"):
        require(not reclaimed_names_are_exact(unlinked), f"an inexact reclaimed set was accepted: {unlinked}")
    print("gate rules: allocations net of the trace probe once per commit, I/O net of one probe per phase, "
          "negative nets fail closed; the reclaimed set must be packs, one index and reclaim.v1")


def check_child_failures(workspace):
    directory = workspace / "children"
    (directory / "scratch").mkdir(parents=True)
    (directory / "idle-decisions.tsv").write_text("label\tattempt\teligible\n")
    built = directory / "fixture"
    built.mkdir(mode=PRIVATE_DIRECTORY_MODE)
    for name in SYNTHETIC_FIXTURE_FILES:
        (built / name).write_text(f"{name}\n")
    cell = make_cell("U", 2**10, BASE_HISTORY)
    kinds = {"silent": "missing", "omit-last": "missing", "omit-commit": "missing", "duplicate-commit": "malformed",
             "malformed": "malformed", "exit": "exit", "timeout": "timeout", "oom": "oom"}
    cases = [(mode, population, fault) for mode, population, faults in
             (("verify", "verify", SYNTHETIC_FAULTS), ("time", "decision", SYNTHETIC_FAULTS),
              ("build", "build", SYNTHETIC_BUILD_FAULTS)) for fault in faults]
    child = SyntheticChild()
    ordinals = Counter()
    for mode, _, fault in cases:
        child.faults[mode, cell["cell"], ordinals[mode]] = fault
        ordinals[mode] += 1

    def fixture_for(mode, number):
        if mode != "build":
            return built
        fresh = directory / f"fixture-{number}"
        fresh.mkdir(mode=PRIVATE_DIRECTORY_MODE)
        return fresh

    inherited = {"RSPACE_SCALE_SCRATCH": str(directory / "inherited"), "RSPACE_SCALE_MODE": "inherited"}
    with (directory / "children.log").open("w", encoding="utf-8") as log, \
            TableWriter(directory / "failures.tsv", FAILURE_COLUMNS) as failures:
        runner = ChildRunner(workspace / "synthetic-binary", directory, 2, 60.0, inherited, child, idle_sampler, log,
                             failures)
        for number, (mode, population, fault) in enumerate(cases):
            outcome = runner.run(mode, population, number, cell, fixture_for(mode, number))
            require(outcome.records is None and outcome.failure is not None
                    and outcome.failure["kind"] == kinds[fault], f"{mode} fault {fault} produced {outcome.failure}")
        for mode, population in (("build", "build"), ("verify", "verify"), ("time", "decision")):
            clean = runner.run(mode, population, 90, cell, fixture_for(mode, 90))
            require(clean.records is not None and clean.failure is None,
                    f"a clean {mode} child failed: {clean.failure}")
        launches = child.launches

        def broken_sampler(cpu):
            raise OSError("snapshot unavailable")

        runner.sampler = broken_sampler
        refuses(lambda: runner.run("time", "decision", 91, cell, built), OSError,
                "a snapshot failure launched a child")
        runner.sampler = lambda cpu: ("{}", False)
        refuses(lambda: runner.run("time", "decision", 92, cell, built), IdleAdmissionError,
                "contention launched a child")
        require(child.launches == launches, "a child ran without idle admission")
    rows = read_table(directory / "failures.tsv", FAILURE_COLUMNS)
    require([row["kind"] for row in rows] == [kinds[fault] for *_, fault in cases],
            "every failure is retained in order")
    require(not any((directory / "scratch").iterdir()), "scratch directories outlived their children")
    log_text = (directory / "children.log").read_text(encoding="utf-8")
    require(log_text.count("=== end child ") == len(cases) + 3, "every child is logged")
    print(f"failing closed: {len(cases)} faulted build, verify and time children (no test run, missing last record, "
          "missing commit, duplicate commit, malformed record, exit, timeout, OOM kill) retained as failures; "
          "failed idle admission launches nothing")


def check_synthetic_collections(workspace):
    binary = workspace / "synthetic-binary"
    binary.write_bytes(b"synthetic test binary\n")
    capped = {"SCALE_CAPPED": "1"}

    def run(name, mode, control, child, environ=capped, host=None):
        return collect(binary, workspace / name, 2, mode, control, 60.0, environ=environ,
                       host=SyntheticHost() if host is None else host, launcher=child, sampler=idle_sampler,
                       progress=lambda message: None)

    refuses(lambda: run("refused-uncapped", "decision", None, SyntheticChild(), environ={}), CollectionRefused,
            "collection ran without SCALE_CAPPED=1")
    refuses(lambda: run("refused-governor", "decision", None, SyntheticChild(), host=PowersaveHost()),
            CollectionRefused, "decision collection ran under the powersave governor")
    require(not (workspace / "refused-uncapped").exists() and not (workspace / "refused-governor").exists(),
            "a refused collection created its output directory")
    recovered = {}
    for exponent in (0.0, 0.1, 1.0):
        name = f"exponent-{exponent}"
        analysis = run(name, "decision", None, SyntheticChild(exponent))
        gates = analysis["gates"]
        for gate in ("H1", "G3"):
            for axis in ("U", "R"):
                arm = gates[gate]["arms"][axis]
                require(abs(arm["estimate"] - exponent) < 1e-4 and abs(arm["upper95"] - arm["estimate"]) < 1e-9,
                        f"{gate} {axis} recovered {arm.get('estimate')} for exponent {exponent}")
            require(gates[gate]["passed"] == (exponent <= EXPONENT_BOUND), f"{gate} verdict at exponent {exponent}")
        for axis in ("D", "L"):
            require(abs(gates["G8"]["arms"][axis]["estimate"] - exponent) < 1e-4, f"G8 {axis} at exponent {exponent}")
        require(analysis["qualified"],
                f"a clean synthetic population is not qualified: {analysis['qualification_problems']}")
        require(all(gates[gate]["passed"] for gate in ("G1", "G2", "G4", "G5", "G6", "G7", "G8", "G9")),
                f"a clean synthetic population failed a gate: {failed_gates(analysis)}")
        require(analysis["accepted"] == (exponent <= EXPONENT_BOUND), f"acceptance at exponent {exponent}")
        with contextlib.redirect_stdout(io.StringIO()):
            _, problems = check_directory(workspace / name)
        require((not problems) == (exponent <= EXPONENT_BOUND), f"--check at exponent {exponent}: {problems}")
        recovered[exponent] = analysis
    flat = workspace / "exponent-0.0"
    c_star = recovered[0.0]["descriptive"]["c_star"]
    require(all(c_star[axis]["c_star"] and c_star[axis]["c_star"] > 0 for axis in ("U", "R")),
            f"c* missing: {c_star}")
    for archived in ("samples.tsv", "children.log"):
        (flat / archived).unlink()
    printed = io.StringIO()
    with contextlib.redirect_stdout(printed):
        _, problems = check_directory(flat)
    require(not problems and "skipped (absent): samples.tsv" in printed.getvalue(),
            f"--check over the committed summaries alone: {problems}")
    stored = json.loads((flat / "analysis.json").read_text(encoding="utf-8"))
    stored["accepted"] = not stored["accepted"]
    write_json(flat / "analysis.json", stored)
    with contextlib.redirect_stdout(io.StringIO()):
        _, problems = check_directory(flat)
    require("sha256 mismatch: analysis.json" in problems
            and "the recomputed analysis differs from analysis.json" in problems,
            f"a tampered analysis.json passed --check: {problems}")
    (flat / "fixtures.tsv").write_text((flat / "fixtures.tsv").read_text(encoding="utf-8") + "truncated\n",
                                       encoding="utf-8")
    with contextlib.redirect_stdout(io.StringIO()):
        refuses(lambda: check_directory(flat), ValueError, "a malformed summary table was analyzed")

    first = decision_cells()[0]["cell"]
    evicting = axis_members(decision_cells(), "E")[0]["cell"]
    faulted = run("faulted", "decision", None, SyntheticChild(faults={("time", first, WARMUP_ROUNDS + 5): "omit-last",
                                                                        ("verify", first, 1): "omit-commit",
                                                                        ("verify", evicting, 0): "omit-commit"}))
    require(not faulted["qualified"] and not faulted["accepted"] and not faulted["gates"]["G1"]["passed"]
            and not faulted["gates"]["H1"]["passed"], "a population with failed children was accepted")
    asserted = {"G5": faulted["gates"]["G5"]["appends"], "G7": faulted["gates"]["G7"]["model"],
                "G9": faulted["gates"]["G9"]["trusted_effects"]}
    require(all(not condition["passed"] and not faulted["gates"][gate]["passed"]
                for gate, condition in asserted.items()),
            f"an incomplete verify run passed a child-asserted condition: {asserted}")
    require(asserted["G9"]["incomplete_verify_runs"] == [f"{axis_members(decision_cells(), 'E')[0]['id']} run 1"],
            f"G9 checked the verify runs of cells off the E axis: {asserted['G9']}")
    retained = read_table(workspace / "faulted" / "failures.tsv", FAILURE_COLUMNS)
    require([(row["mode"], row["kind"]) for row in retained]
            == [("verify", "missing"), ("verify", "missing"), ("time", "missing")],
            f"failures retained: {retained}")
    with contextlib.redirect_stdout(io.StringIO()):
        _, problems = check_directory(workspace / "faulted")
    require(any("not qualified" in problem for problem in problems), "--check passed a failed population")

    ordered = build_protocol("controls", "NC-ORDER")["cells"]
    detected = run("nc-order", "controls", "NC-ORDER",
                   SyntheticChild(1.0, faults={("verify", cell["cell"], 0): "exit" for cell in ordered}))
    require(detected["qualified"] and detected["gates"]["G10"]["detected"] and detected["accepted"],
            f"NC-ORDER evidence was not detected: {detected['gates']['G10']['conditions']}")
    versioned = run("nc-v1", "controls", "NC-V1", SyntheticChild(1.0))
    require(versioned["accepted"] and versioned["gates"]["G7"]["passed"], "NC-V1 evidence was not detected")
    missed = run("nc-scan", "controls", "NC-SCAN", SyntheticChild(0.0))
    require(missed["qualified"] and not missed["gates"]["G10"]["detected"] and not missed["accepted"],
            "a flat population detected NC-SCAN")
    pilot = run("pilot", "pilot", None, SyntheticChild(0.0), host=PowersaveHost())
    require(pilot["qualified"] and not pilot["accepted"] and pilot["pilot_only"], "a pilot became decision data")
    estimates = ", ".join(f"{analysis['gates']['H1']['arms']['U']['estimate']:.6f}"
                          for analysis in recovered.values())
    print(f"synthetic collections: exponents 0, 0.1, 1 recovered as {estimates} "
          "through collection, summaries and analysis; --check passes only accepted, untampered populations; "
          "controls detect or miss as expected; the pilot is never accepted")


def self_test():
    root = self_test_root()
    started = time.monotonic()
    check_record_contract()
    check_cells_and_guard()
    check_idle_admission()
    check_bootstraps()
    check_additive_threshold()
    check_gate_rules()
    with tempfile.TemporaryDirectory(dir=root, prefix="rspace-fixed-delta-scaling-self-test-") as name:
        workspace = Path(name)
        check_child_failures(workspace)
        check_synthetic_collections(workspace)
    print(f"rspace fixed-delta scaling driver self-tests passed in {time.monotonic() - started:.1f} s")


def main(argv=None):
    parser = argparse.ArgumentParser(description="Task 7929 fixed-delta trusted persistence scaling driver.")
    parser.add_argument("--binary", type=Path, help="release rspace++ lib test binary (collection)")
    parser.add_argument("--output", type=Path, help="new population directory (collection) or one to analyze")
    parser.add_argument("--cpu", type=int, default=2, help="measured CPU for taskset and idle admission")
    parser.add_argument("--pilot", action="store_true", help="collect a pilot population; never decision data")
    parser.add_argument("--controls", choices=CONTROLS, help="collect the named negative-control population")
    parser.add_argument("--analyze-only", action="store_true", help="recompute analysis.json from the summaries")
    parser.add_argument("--check", action="store_true",
                        help="verify digests and the recomputed analysis; nonzero on any failure")
    parser.add_argument("--self-test", action="store_true", help="run the in-process self-tests")
    parser.add_argument("--child-timeout", type=float, metavar="SECONDS", help="per-child timeout (collection)")
    args = parser.parse_args(argv)
    if args.self_test:
        self_test()
        return 0
    if args.output is None:
        parser.error("--output is required")
    analyze_only = args.analyze_only or args.check
    if args.pilot and analyze_only:
        parser.error("a pilot is not an acceptance population")
    if analyze_only:
        if args.binary is not None or args.controls is not None or args.child_timeout is not None:
            parser.error("--analyze-only and --check read the recorded protocol; "
                         "omit --binary, --controls and --child-timeout")
        directory = args.output.resolve()
        if args.check:
            try:
                analysis, problems = check_directory(directory)
            except (OSError, ValueError, KeyError) as error:
                print(f"check failed: {type(error).__name__}: {error}")
                return 1
            if analysis is not None:
                print_report(analysis)
            for problem in problems:
                print(f"check failed: {problem}")
            if not problems:
                print("check passed")
            return 1 if problems else 0
        analysis = analyze_directory(directory)
        write_json(directory / "analysis.json", analysis)
        refresh_manifest(directory)
        print_report(analysis)
        return exit_status(analysis)
    if args.binary is None or args.child_timeout is None:
        parser.error("collection requires --binary and --child-timeout")
    if not (math.isfinite(args.child_timeout) and args.child_timeout > 0):
        parser.error("--child-timeout must be a positive number of seconds")
    if args.cpu < 0:
        parser.error("--cpu must name a CPU")
    mode = "pilot" if args.pilot else "decision" if args.controls is None else "controls"
    analysis = collect(args.binary.resolve(), args.output.resolve(), args.cpu, mode, args.controls,
                       args.child_timeout)
    print_report(analysis)
    return exit_status(analysis)


if __name__ == "__main__":
    raise SystemExit(main())
