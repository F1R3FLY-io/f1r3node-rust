#!/usr/bin/env python3
"""Mutation controls of the recorded VectorDB processes (P6 step 7a).

Each mutant reverts one rule of `rho:vectordb:embed`, of `rho:vectordb:ann` or of the `config`
reply of a mirrored space: in the processes
(rholang/src/rust/interpreter/reified/system_processes.rs), in the registration of the recorded
processes (rholang/src/rust/interpreter/system_processes.rs) or in the handle
(rholang/src/rust/interpreter/reified/handle.rs). The harness applies one mutant at a
time, runs the suite of the processes and the unit tests of the services and of the `config`
reply (`cargo test -p rholang --lib --test reified_vectordb_chroma -- vectordb_
space_handle_methods_config`, with `--no-fail-fast` so that a failure of the library tests does
not hide the suite) and restores the source files. A mutant passes the control when at least
one test fails.

Rules of the harness:
  * The harness reads the source files first and restores them after every mutant, also on an
    error or an interrupt. It prints whether the restored files equal the originals.
  * Each mutation text must occur exactly once in its file. Otherwise the control fails as
    stale.
  * Unless RRS_SANDBOX=1, each cargo run is bounded by a systemd user scope (MemoryMax,
    CPUQuota, RuntimeMaxSec). With RRS_SANDBOX=1, `timeout` bounds it. TMPDIR points into
    target/, never into /tmp.
  * The suite bounds each native replay certification by a timeout, so a mutant that makes
    native replay hang fails a test. The run time limit is a backstop: a run that reaches it
    counts as a run whose tests do not pass, and the harness reports it as a hang.

Usage: python3 scripts/verify-reified/vectordb-services-mutants.py [--log-dir DIR]
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
HOST = INTERPRETER / "system_processes.rs"
PROCESSES = INTERPRETER / "reified/system_processes.rs"
HANDLE = INTERPRETER / "reified/handle.rs"

EMBED_REF = "crate::rust::interpreter::reified::system_channels::EMBED_BODY_REF"
ANN_REF = "crate::rust::interpreter::reified::system_channels::ANN_BODY_REF"
ANN_REQUEST = (
    "    let request = ann_request(runtime, agent, authority, space, channel, query, options)"
    ".await?;\n"
)
REPLAY_RETURN = (
    "        produce(&previous_output, ret).await?;\n"
    "        return Ok(previous_output);\n"
    "    }\n"
)

# (name, file, old text, new text): each mutant reverts one rule of the processes.
MUTANTS = (
    # embed is not recorded, so replay has no output to produce (decision 57).
    ("embed-not-recorded", HOST,
     f"        BodyRefs::CHROMA_QUERY,\n        {EMBED_REF},\n",
     "        BodyRefs::CHROMA_QUERY,\n"),
    # ann is not recorded, so replay has no output to produce (decision 57).
    ("ann-not-recorded", HOST,
     f"        {EMBED_REF},\n        {ANN_REF},\n    ])",
     f"        {EMBED_REF},\n    ])"),
    # Replay asks the embedder again instead of producing the recorded reply.
    ("embed-replay-recomputes", PROCESSES,
     "    if is_replay {\n" + REPLAY_RETURN + "    let output = vec![embed_reply(input).await];\n",
     "    if is_replay && false {\n" + REPLAY_RETURN
     + "    let output = vec![embed_reply(input).await];\n"),
    # Replay asks the mirror again instead of producing the recorded reply.
    ("ann-replay-recomputes", PROCESSES,
     ANN_REQUEST + "    if is_replay {\n",
     ANN_REQUEST + "    if is_replay && false {\n"),
    # Replay skips the checks of the call, so it reserves less host work than play.
    ("ann-checks-only-in-play", PROCESSES,
     ANN_REQUEST + "    if is_replay {\n" + REPLAY_RETURN,
     "    if is_replay {\n" + REPLAY_RETURN + ANN_REQUEST),
    # The node-local quota of embed is gone, so the model runs on any number of texts.
    ("quota-dropped", PROCESSES,
     "    if texts.len() > vectordb_services::MAX_TEXTS || bytes > vectordb_services::MAX_TEXT_BYTES {",
     "    if texts.is_empty() {"),
    # A vector with a NaN element is a reply value.
    ("non-finite-accepted", PROCESSES,
     "        Ok(vectors) if vectors.iter().flatten().any(|value| !value.is_finite()) => {",
     "        Ok(vectors) if vectors.is_empty() && !texts.is_empty() => {"),
    # A VectorDB space without a mirror is asked anyway.
    ("mirror-check-dropped", PROCESSES,
     "    if !config.params.contains_key(MIRROR_PARAM) {\n"
     "        return Ok(Err(ServiceReplyCode::MirrorDisabled));\n"
     "    }\n",
     ""),
    # k is not bounded by the capacity of the space.
    ("k-unbounded", PROCESSES,
     "            .filter(|k| (1..=capacity).contains(k)),",
     "            .filter(|k| *k >= 1),"),
    # The mirror receives the digest of the space instead of the channel.
    ("channel-digest-of-the-space", PROCESSES,
     "        channel: channel_digest(&CollectionKey::of_channel(channel)?),",
     "        channel: channel_digest(&CollectionKey::of_channel(space)?),"),
    # The reply keeps an id that cannot name a record.
    ("malformed-id-kept", PROCESSES,
     "        .filter(|neighbour| neighbour.id.len() == ID_LEN)",
     "        .filter(|neighbour| !neighbour.id.is_empty())"),
    # The config reply of a mirrored space has no mirror entry.
    ("config-entry-dropped", HANDLE,
     "                config_reply(space, config_map, config).map(MethodEffect::Reply)",
     "                Ok(MethodEffect::Reply(config_map.clone()))"),
)


# The longest time of one cargo run: a rebuild of the crate and the suite.
RUN_LIMIT_SECONDS = 2400


def run_tests(log: Path) -> tuple[int, list[str]]:
    """Runs the tests of the processes and returns the exit status and the failures."""
    command = ["cargo", "test", "--no-fail-fast", "-p", "rholang", "--lib", "--test",
               "reified_vectordb_chroma", "--", "vectordb_", "space_handle_methods_config"]
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
                        default=REPO_ROOT / "target/verification/reified-rspaces/vectordb-services-mutants")
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
    print(f"vectordb-services mutants: PASS={killed} FAIL={survived} restored={restored}")
    return 0 if survived == 0 and restored else 1


if __name__ == "__main__":
    sys.exit(main())
