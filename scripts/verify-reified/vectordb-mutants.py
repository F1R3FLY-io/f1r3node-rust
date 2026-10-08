#!/usr/bin/env python3
"""Mutation controls of the VectorDB driver and join engine (P6 steps 3 and 4).

Each mutant reverts one rule of the VectorDB discipline: in the driver
(rholang/src/rust/interpreter/spaces/disciplines/vectordb.rs) or in the join engine
(rholang/src/rust/interpreter/spaces/join.rs). The harness applies one mutant at a time, runs the
VectorDB suite (`cargo test -p rholang --test reified_vectordb`) and restores the source files. A
mutant passes the control when at least one test fails.

Rules of the harness:
  * The harness reads the source files first and restores them after every mutant, also on an
    error or an interrupt. It prints whether the restored files equal the originals.
  * Each mutation text must occur exactly once in its file. Otherwise the control fails as
    stale.
  * Unless RRS_SANDBOX=1, each cargo run is bounded by a systemd user scope (MemoryMax,
    CPUQuota). TMPDIR points into target/, never into /tmp.

Usage: python3 scripts/verify-reified/vectordb-mutants.py [--log-dir DIR]
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
SPACES = REPO_ROOT / "rholang/src/rust/interpreter/spaces"
DRIVER = SPACES / "disciplines/vectordb.rs"
ENGINE = SPACES / "join.rs"

# (name, file, old text, new text): each mutant reverts one rule of the discipline.
MUTANTS = (
    # LIST-CASCADE of the driver never searches (LM-b): a removal leaves a pending list match.
    ("driver-no-list-cascade", DRIVER,
     "        if !tail.removed {\n"
     "            return Ok(None);\n"
     "        }\n"
     "        let Some(target) = first_cascade_target(cx, cfg)? else {",
     "        if !tail.removed || tail.removed {\n"
     "            return Ok(None);\n"
     "        }\n"
     "        let Some(target) = first_cascade_target(cx, cfg)? else {"),
    # The cascade of the engine never finds a list waiter (LM-b in the engine).
    ("engine-no-list-cascade", ENGINE,
     "    ) -> Result<Option<ListFire>, SpaceError> {\n"
     "        let Some((query, k)) = self.list_waiter(cx, &waiter.slots)? else {",
     "    ) -> Result<Option<ListFire>, SpaceError> {\n"
     "        if self.vdb.is_some() {\n"
     "            return Ok(None);\n"
     "        }\n"
     "        let Some((query, k)) = self.list_waiter(cx, &waiter.slots)? else {"),
    # A list fire retires no other member (the K_retire step of OFFER-VDB and of the cascade).
    ("no-member-retire", DRIVER,
     "        if member.persist {\n"
     "            continue;\n"
     "        }\n"
     "        let index = located(cx, channel, &member.source.hash)?;",
     "        if member.persist || !member.persist {\n"
     "            continue;\n"
     "        }\n"
     "        let index = located(cx, channel, &member.source.hash)?;"),
    # The selection order is reversed: the worst eligible record comes first.
    ("selection-reversed", DRIVER,
     "            score: Some(candidate.score),\n"
     "        });\n"
     "    }\n"
     "    Ok(sorted)\n"
     "}",
     "            score: Some(candidate.score),\n"
     "        });\n"
     "    }\n"
     "    sorted.reverse();\n"
     "    Ok(sorted)\n"
     "}"),
    # A peek of the driver sends no record again (Rule E, decision 45).
    ("driver-rule-e-dropped", DRIVER,
     "            tail.owe_all(sends, meter)?;\n"
     "        }\n"
     "        record::prepay_continuation_unwrap(&continuation, meter)?;",
     "            drop(sends);\n"
     "        }\n"
     "        record::prepay_continuation_unwrap(&continuation, meter)?;"),
    # A similarity bind of the engine ignores its query and reads the canonical order.
    ("engine-bind-ignores-query", ENGINE,
     "        let query = vectordb::policy_query(cfg, policy, meter)?;\n"
     "        let entries: Vec<(usize, Option<Score>)> = match &query {",
     "        let query = vectordb::policy_query(cfg, policy, meter)?.filter(|_| false);\n"
     "        let entries: Vec<(usize, Option<Score>)> = match &query {"),
)


def run_tests(log: Path) -> tuple[int, list[str]]:
    """Runs the VectorDB suite and returns the exit status and the failed tests."""
    command = ["cargo", "test", "-p", "rholang", "--test", "reified_vectordb"]
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
                        default=REPO_ROOT / "target/verification/reified-rspaces/vectordb-mutants")
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
    print(f"vectordb mutants: PASS={killed} FAIL={survived} restored={restored}")
    return 0 if survived == 0 and restored else 1


if __name__ == "__main__":
    sys.exit(main())
