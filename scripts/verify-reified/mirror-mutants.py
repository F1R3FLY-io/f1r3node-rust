#!/usr/bin/env python3
"""Mutation controls of the mirror journal and its publication (P6 step 7b).

Each mutant reverts one rule of the mirror of a VectorDB space: the capture of the net writes of
a transaction (rholang/src/rust/interpreter/spaces/mirror.rs and the store hook of
rholang/src/rust/interpreter/spaces/generic.rs), the journal of an evaluation and its
publication with the final result (rholang/src/rust/interpreter/reified/runtime.rs and the two
settle lines of rholang/src/rust/interpreter/interpreter.rs and
rholang/src/rust/interpreter/rho_runtime.rs), or the mirror writer
(rholang/src/rust/interpreter/reified/vectordb_services.rs). The harness applies one mutant at a
time, runs the mirror tests (`cargo test -p rholang --lib --test reified_vectordb_chroma --
mirror_ vectordb_services gate_9`, with `--no-fail-fast` so that a failure of the library tests
does not hide the suite) and restores the source files. A mutant passes the control when at
least one test fails.

Rules of the harness:
  * The harness reads the source files first and restores them after every mutant, also on an
    error or an interrupt. It prints whether the restored files equal the originals.
  * Each mutation text must occur exactly once in its file. Otherwise the control fails as
    stale.
  * Unless RRS_SANDBOX=1, each cargo run is bounded by a systemd user scope (MemoryMax,
    CPUQuota, RuntimeMaxSec). With RRS_SANDBOX=1, `timeout` bounds it. TMPDIR points into
    target/, never into /tmp.
  * The suite bounds each native replay certification and each deploy by a timeout, so a
    mutant that makes a run hang fails a test. The run time limit is a backstop: a run that
    reaches it counts as a run whose tests do not pass, and the harness reports it as a hang.

Usage: python3 scripts/verify-reified/mirror-mutants.py [--log-dir DIR]
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
MIRROR = INTERPRETER / "spaces/mirror.rs"
GENERIC = INTERPRETER / "spaces/generic.rs"
RUNTIME = INTERPRETER / "reified/runtime.rs"
SERVICES = INTERPRETER / "reified/vectordb_services.rs"
EVALUATION = INTERPRETER / "interpreter.rs"
RHO_RUNTIME = INTERPRETER / "rho_runtime.rs"

# (name, file, old text, new text): each mutant reverts one rule of the mirror.
MUTANTS = (
    # Settle: a result with errors publishes its journal (M1, decision 66).
    ("publish-with-errors", RUNTIME,
     "if matches!(result, Ok(evaluation) if evaluation.errors.is_empty()) && !batches.is_empty()",
     "if matches!(result, Ok(_)) && !batches.is_empty()"),
    # Settle: an evaluation that fails with an error publishes its journal.
    ("publish-on-err", RUNTIME,
     "if matches!(result, Ok(evaluation) if evaluation.errors.is_empty()) && !batches.is_empty()",
     "if !batches.is_empty()"),
    # Play and legacy evaluation never settle, so a node never publishes its deploys.
    ("no-play-settle", EVALUATION,
     "        reducer.reified.settle_mirror_journal(&evaluation_result);\n",
     ""),
    # Native replay never settles, so a validator never publishes the deploys it certifies.
    ("no-replay-settle", RHO_RUNTIME,
     "        self.reducer.reified.settle_mirror_journal(&result);\n",
     ""),
    # A root evaluation keeps the journal that no result settled (FR-5').
    ("begin-scope-keeps-journal", RUNTIME,
     "        self.journal.clear();\n",
     ""),
    # The space job does not append the writes of its transaction to the journal.
    ("journal-not-appended", RUNTIME,
     "                        journal.record(writes);\n",
     "                        drop(writes);\n"),
    # Rule E: a record that a peek removes and stores again is upserted.
    ("rule-e-upserts", MIRROR,
     "                (Some(EventKind::Store), EventKind::Store) => store",
     "                (Some(_), EventKind::Store) => store"),
    # A record that is new and leaves in the same transaction is deleted.
    ("new-then-removed-deletes", MIRROR,
     "                (Some(EventKind::Remove), EventKind::Remove) => Some(MirrorWrite::Delete(id)),",
     "                (Some(_), EventKind::Remove) => Some(MirrorWrite::Delete(id)),"),
    # A persistent record that a COMM matches is deleted.
    ("persistent-removed", MIRROR,
     "                        if produce.persistent\n                            || incoming\n",
     "                        if incoming\n"),
    # The incoming datum of a produce-triggered fire counts as a removal.
    ("incoming-removed", MIRROR,
     "                            || incoming\n",
     ""),
    # A COMM on a channel that is not a data channel of the space removes a mirror record.
    ("foreign-channel-removed", MIRROR,
     "                            || !channels.contains(&produce.channel_hash)\n",
     ""),
    # An embedding that is not finite as f32 values is mirrored.
    ("non-finite-mirrored", MIRROR,
     "        if !element.is_finite() {\n            return None;\n        }\n",
     ""),
    # A mirror record never carries its document.
    ("document-dropped", MIRROR,
     "        document,\n    })",
     "        document: document.filter(|_| false),\n    })"),
    # A mirror record never carries its persistence.
    ("persistence-dropped", MIRROR,
     "        persistent: store.persistent,",
     "        persistent: false,"),
    # A VectorDB space without the param mirror is captured.
    ("capture-without-mirror-param", MIRROR,
     "        if !config.params.contains_key(MIRROR_PARAM) {\n            return Self::inactive();\n        }\n",
     ""),
    # The journal keeps the first write of a record id instead of the last.
    ("journal-first-write-wins", MIRROR,
     "            collection.writes.insert(*write.id(), write);",
     "            collection.writes.entry(*write.id()).or_insert(write);"),
    # The metric dot gets the distance cosine.
    ("dot-as-cosine", MIRROR,
     "            Metric::Dot => Some(MirrorMetric::InnerProduct),",
     "            Metric::Dot => Some(MirrorMetric::Cosine),"),
    # The captured stores shift by one staged index, so no store meets its receipt.
    ("capture-index-shifted", GENERIC,
     "            let index = self.plan.staged.len();",
     "            let index = self.plan.staged.len() + 1;"),
    # The writer makes one attempt only.
    ("writer-no-retry", SERVICES,
     "    pub const ATTEMPTS: u32 = 3;",
     "    pub const ATTEMPTS: u32 = 1;"),
    # A batch that fails after its retries leaves its collection clean.
    ("failed-batch-not-dirty", SERVICES,
     "                    task_progress.mark_dirty(batch.collection);\n",
     "                    drop(batch);\n"),
    # A batch that finds the queue full leaves its collection clean.
    ("full-queue-not-dirty", SERVICES,
     "                    self.progress.mark_dirty(batch.collection);\n",
     "                    drop(batch);\n"),
    # A batch writes to a collection that it never creates.
    ("collection-not-ensured", SERVICES,
     "    mirror\n        .ensure_collection(&batch.collection, batch.metric)\n        .await?;\n",
     ""),
    # A batch never deletes.
    ("deletes-skipped", SERVICES,
     "        mirror.delete(&batch.collection, &batch.deletes).await?;\n",
     ""),
)


# The longest time of one cargo run: a rebuild of the crate and the suite.
RUN_LIMIT_SECONDS = 2400


def run_tests(log: Path) -> tuple[int, list[str]]:
    """Runs the mirror tests and returns the exit status and the failures."""
    command = ["cargo", "test", "--no-fail-fast", "-p", "rholang", "--lib", "--test",
               "reified_vectordb_chroma", "--", "mirror_", "vectordb_services", "gate_9"]
    if os.environ.get("RRS_SANDBOX") != "1":
        command = ["systemd-run", "--user", "--scope", "--quiet", "-p", "MemoryMax=24G",
                   "-p", "CPUQuota=800%", "-p", f"RuntimeMaxSec={RUN_LIMIT_SECONDS}", "--",
                   *command]
    else:
        command = ["timeout", "-k", "30", str(RUN_LIMIT_SECONDS), *command]
    tmp = REPO_ROOT / "target" / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    environment = {**os.environ, "TMPDIR": str(tmp), "CARGO_INCREMENTAL": "0"}
    with log.open("w", encoding="utf-8") as handle:
        status = subprocess.run(command, cwd=REPO_ROOT, stdout=handle, stderr=subprocess.STDOUT,
                                env=environment, check=False).returncode
    failed = [line.split()[1] for line in log.read_text(encoding="utf-8").splitlines()
              if line.startswith("test ") and line.endswith("FAILED")]
    return status, failed


def suite_hangs(log: str) -> bool:
    """True when the suite started and printed no result: the run time limit ended it."""
    marker = "Running tests/reified_vectordb_chroma.rs"
    return marker in log and "test result:" not in log.split(marker, 1)[1]


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    parser.add_argument("--log-dir", type=Path,
                        default=REPO_ROOT / "target/verification/reified-rspaces/mirror-mutants")
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
            elif status != 0 and suite_hangs((args.log_dir / f"{name}.log").read_text(
                    encoding="utf-8")):
                killed += 1
                print(f"  PASS {name}: the suite hangs and reaches the run time limit")
            else:
                survived += 1
                print(f"  FAIL {name}: the mutant survives (exit {status})")
    finally:
        for path, text in originals.items():
            path.write_text(text, encoding="utf-8")
    restored = all(path.read_text(encoding="utf-8") == text for path, text in originals.items())
    print(f"mirror mutants: PASS={killed} FAIL={survived} restored={restored}")
    return 0 if survived == 0 and restored else 1


if __name__ == "__main__":
    sys.exit(main())
