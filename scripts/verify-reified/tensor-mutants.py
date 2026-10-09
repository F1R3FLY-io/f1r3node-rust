#!/usr/bin/env python3
"""Mutation controls of the VectorDB numeric core (P6 step 2, verification.md section 14.5).

Each mutant reverts one rule of rholang/src/rust/interpreter/tensor/. The harness applies
one mutant at a time, runs the tensor tests (`cargo test -p rholang --lib -- tensor::`) and
restores the source files. A mutant passes the control when at least one tensor test fails.

Rules of the harness:
  * The harness reads the source files first and restores them after every mutant, also on an
    error or an interrupt. It prints whether the restored files equal the originals.
  * Each mutation text must occur exactly once in its file. Otherwise the control fails as
    stale.
  * Unless RRS_SANDBOX=1, each cargo run is bounded by a systemd user scope (MemoryMax,
    CPUQuota). TMPDIR points into target/, never into /tmp.

Usage: python3 scripts/verify-reified/tensor-mutants.py [--log-dir DIR]
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
TENSOR = REPO_ROOT / "rholang/src/rust/interpreter/tensor"
EXACT = TENSOR / "exact.rs"
NUM = TENSOR / "num.rs"

# (name, file, old text, new text): each mutant reverts one rule of the core.
MUTANTS = (
    ("cosine-negative-case-reversed", EXACT,
     "                        // Both negative: cos ≥ t when cos² ≤ t².\n"
     "                        true => order != Ordering::Greater,",
     "                        // Both negative: cos ≥ t when cos² ≤ t².\n"
     "                        true => order != Ordering::Less,"),
    ("cosine-order-swapped-norms", EXACT,
     "                let left = product(&product(u, u, meter)?, n_prime, meter)?;\n"
     "                let right = product(&product(v, v, meter)?, n, meter)?;",
     "                let left = product(&product(u, u, meter)?, n, meter)?;\n"
     "                let right = product(&product(v, v, meter)?, n_prime, meter)?;"),
    ("ties-round-up", NUM,
     "        Ordering::Equal => match quotient.is_even() {\n"
     "            true => quotient,\n"
     "            false => quotient + 1u8,\n"
     "        },\n"
     "    }\n"
     "}",
     "        Ordering::Equal => quotient + 1u8,\n"
     "    }\n"
     "}"),
    ("dyadic-sum-keeps-unit", EXACT,
     "                self.sum = aligned + term;\n"
     "                self.unit = Some(exponent);",
     "                self.sum = aligned + term;"),
    ("euclidean-radius-unsquared", EXACT,
     "                let limit = product(radius, radius, meter)?;\n"
     "                compare(distance2, &limit, meter)? != Ordering::Greater",
     "                compare(distance2, radius, meter)? != Ordering::Greater"),
    # P6 step 8 (design v3 section 6): the dot product of dyadic vectors sums its terms in f64,
    # one rounding after each term, as a float kernel does.
    ("dot-f64-accumulation", EXACT,
     "            let mut sum = DyadicSum::new();\n"
     "            for (x, y) in left.iter().zip(right) {\n"
     "                costs::reserve_product(&x.mantissa, &y.mantissa, meter)?;\n"
     "                sum.add(\n"
     "                    &x.mantissa * &y.mantissa,\n"
     "                    exponent_sum(x.exponent, y.exponent)?,\n"
     "                    meter,\n"
     "                )?;\n"
     "            }\n"
     "            sum.value(meter)\n"
     "        }\n"
     "        _ => {\n"
     "            let (left, right) = (a.rationals(meter)?, b.rationals(meter)?);\n"
     "            let mut sum = Exact::zero();\n"
     "            for (x, y) in left.iter().zip(right.iter()) {\n"
     "                costs::reserve_rational(x, y, meter)?;\n"
     "                let product = x * y;",
     "            let mut sum = 0f64;\n"
     "            for (x, y) in left.iter().zip(right) {\n"
     "                costs::reserve_product(&x.mantissa, &y.mantissa, meter)?;\n"
     "                let mut term = DyadicSum::new();\n"
     "                term.add(\n"
     "                    &x.mantissa * &y.mantissa,\n"
     "                    exponent_sum(x.exponent, y.exponent)?,\n"
     "                    meter,\n"
     "                )?;\n"
     "                sum += round_to_f64(&term.value(meter)?, meter)?;\n"
     "            }\n"
     "            Exact::from_float(sum).ok_or(NumError::NonFiniteValue)\n"
     "        }\n"
     "        _ => {\n"
     "            let (left, right) = (a.rationals(meter)?, b.rationals(meter)?);\n"
     "            let mut sum = Exact::zero();\n"
     "            for (x, y) in left.iter().zip(right.iter()) {\n"
     "                costs::reserve_rational(x, y, meter)?;\n"
     "                let product = x * y;"),
)


def run_tests(log: Path) -> tuple[int, list[str]]:
    """Runs the tensor tests and returns the exit status and the failed tests."""
    command = ["cargo", "test", "-p", "rholang", "--lib", "--", "tensor::"]
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
                        default=REPO_ROOT / "target/verification/reified-rspaces/tensor-mutants")
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
    print(f"tensor mutants: PASS={killed} FAIL={survived} restored={restored}")
    return 0 if survived == 0 and restored else 1


if __name__ == "__main__":
    sys.exit(main())
