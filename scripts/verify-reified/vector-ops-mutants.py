#!/usr/bin/env python3
"""Mutation controls of `rho:lang:vector` (P6 step 6).

Each mutant reverts one rule of the vector operations: in the operations
(rholang/src/rust/interpreter/tensor/ops.rs), in the packing of a call
(rholang/src/rust/interpreter/reified/system_channels.rs) or in the process
(rholang/src/rust/interpreter/reified/system_processes.rs). The harness applies one mutant at a
time, runs every test whose name holds `vector_ops` (`cargo test -p rholang --lib --test
reified_vector_ops -- vector_ops`, with `--no-fail-fast` so that a failure of the library tests
does not hide the suite) and restores the source files. A mutant passes the control when at
least one test fails.

Rules of the harness:
  * The harness reads the source files first and restores them after every mutant, also on an
    error or an interrupt. It prints whether the restored files equal the originals.
  * Each mutation text must occur exactly once in its file. Otherwise the control fails as
    stale.
  * Unless RRS_SANDBOX=1, each cargo run is bounded by a systemd user scope (MemoryMax,
    CPUQuota). TMPDIR points into target/, never into /tmp.

Usage: python3 scripts/verify-reified/vector-ops-mutants.py [--log-dir DIR]
Exit codes: 0 every mutant fails at least one test, 1 a mutant survived or is stale.
Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INTERPRETER = REPO_ROOT / "rholang/src/rust/interpreter"
OPS = INTERPRETER / "tensor/ops.rs"
CHANNELS = INTERPRETER / "reified/system_channels.rs"
PROCESSES = INTERPRETER / "reified/system_processes.rs"

# (name, file, old text, new text): each mutant reverts one rule of the operations.
MUTANTS = (
    # The value of add has the narrowest kind of the inputs (decision 66: the widest).
    ("kind-narrowest", OPS,
     "    let kind = left.kind.max(right.kind);",
     "    let kind = left.kind.min(right.kind);"),
    # An integral rational leaves as an integer: the kind follows the value, not the inputs.
    ("rational-by-value", OPS,
     "fn number_par(value: &Exact, kind: Kind, meter: &ScanMeter) -> Result<Par, OpFailure> {\n"
     "    let instance = match kind {",
     "fn number_par(value: &Exact, kind: Kind, meter: &ScanMeter) -> Result<Par, OpFailure> {\n"
     "    let kind = match (kind, value.is_integer()) {\n"
     "        (Kind::Rational, true) => Kind::Integer,\n"
     "        (kind, _) => kind,\n"
     "    };\n"
     "    let instance = match kind {"),
    # norm replies the square of the norm.
    ("norm-without-root", OPS,
     "            let square = exact::dot(&vector.vector, &vector.vector, meter)?;\n"
     "            float_par(num::sqrt_to_f64(&square, meter)?, meter)",
     "            let square = exact::dot(&vector.vector, &vector.vector, meter)?;\n"
     "            float_par(num::round_to_f64(&square, meter)?, meter)"),
    # distance takes the root of the dot product instead of the squared distance.
    ("distance-is-dot", OPS,
     "            let square = exact::distance2(&left.vector, &right.vector, meter)?;",
     "            let square = exact::dot(&left.vector, &right.vector, meter)?;"),
    # Two vectors of different lengths pass, and the shorter length decides.
    ("no-dimension-check", OPS,
     "        true => Ok((left, right)),\n"
     "        false => Err(OpFailure::Reply(ReplyCode::DimensionMismatch)),",
     "        true | false => Ok((left, right)),"),
    # The reducer does not pack a call of rho:lang:vector, so the call never meets its process.
    ("vector-not-packed", CHANNELS,
     "pub const fn packs_arguments(op: u8) -> bool { is_factory_op(op) || op == VECTOR }",
     "pub const fn packs_arguments(op: u8) -> bool { is_factory_op(op) || (op == VECTOR && false) }"),
    # The reply is the bare value, without the [true, ...] shape (decision 57).
    ("reply-without-ok", OPS,
     "            vec![expr_par(ExprInstance::GBool(true)), value]",
     "            vec![value]"),
    # The process meters nothing: it ignores the host-work budget of the deploy (decision 42).
    ("process-unmetered", PROCESSES,
     "        deterministic_reduction::current().and_then(|context| context.host_work_budget()),",
     "        deterministic_reduction::current().and_then(|_| None),"),
)


def run_tests(log: Path) -> tuple[int, list[str]]:
    """Runs the tests of the vector operations and returns the exit status and the failures."""
    command = ["cargo", "test", "--no-fail-fast", "-p", "rholang", "--lib", "--test",
               "reified_vector_ops", "--", "vector_ops"]
    if os.environ.get("RRS_SANDBOX") != "1":
        command = ["systemd-run", "--user", "--scope", "--quiet", "-p", "MemoryMax=24G",
                   "-p", "CPUQuota=800%", "--", *command]
    tmp = REPO_ROOT / "target" / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    environment = {**os.environ, "TMPDIR": str(tmp), "CARGO_INCREMENTAL": "0"}
    with log.open("w", encoding="utf-8") as handle:
        status = subprocess.run(command, cwd=REPO_ROOT, stdout=handle, stderr=subprocess.STDOUT,
                                env=environment, check=False).returncode
    failed = [line.split()[1] for line in log.read_text(encoding="utf-8").splitlines()
              if line.startswith("test ") and line.endswith("FAILED")]
    return status, failed


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    parser.add_argument("--log-dir", type=Path,
                        default=REPO_ROOT / "target/verification/reified-rspaces/vector-ops-mutants")
    args = parser.parse_args()
    args.log_dir.mkdir(parents=True, exist_ok=True)
    originals = {path: path.read_text(encoding="utf-8") for path in {m[1] for m in MUTANTS}}
    killed = 0
    survived = 0
    try:
        for name, path, old, new in MUTANTS:
            text = originals[path]
            if text.count(old) != 1:
                survived += 1
                print(f"  FAIL {name}: stale ({text.count(old)} sites)")
                continue
            path.write_text(text.replace(old, new), encoding="utf-8")
            try:
                status, failed = run_tests(args.log_dir / f"{name}.log")
            finally:
                path.write_text(text, encoding="utf-8")
            if status != 0 and failed:
                killed += 1
                print(f"  PASS {name}: {len(failed)} tests fail: {', '.join(failed)}")
            else:
                survived += 1
                print(f"  FAIL {name}: the mutant survives (exit {status})")
    finally:
        for path, text in originals.items():
            path.write_text(text, encoding="utf-8")
    restored = all(path.read_text(encoding="utf-8") == text for path, text in originals.items())
    print(f"vector-ops mutants: PASS={killed} FAIL={survived} restored={restored}")
    return 0 if survived == 0 and restored else 1


if __name__ == "__main__":
    sys.exit(main())
