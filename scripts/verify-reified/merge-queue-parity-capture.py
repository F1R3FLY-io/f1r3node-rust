#!/usr/bin/env python3
"""Assemble the queue parity golden of P5 from per-scenario capture fragments.

rholang/tests/reified_queue_parity.rs writes one fragment for each scenario when the
environment variable RRS_CAPTURE_QUEUE_PARITY_OUT names an absolute directory under target/.
This script checks that each scenario has exactly one fragment, adds the provenance of the
capture, and writes test-vectors/reified-queue-parity-v1.json. With --check, it compares the
assembled text with the output file instead of writing it.

Python 3 standard library only.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

CORPUS_NAME = "reified_queue_parity_v1"
SCENARIOS = (
    "fifo_and_peek_restore",
    "produce_side_peek_reoffer",
    "capped_persistent_send",
    "reentry",
    "divergent_pair",
    "persistent_operations",
    "legacy_budget",
)
METADATA_KEYS = (
    "name",
    "captured_at_revision",
    "capture_command",
    "rustc_version",
    "scenarios",
)

ROOT = Path(__file__).resolve().parent.parent.parent


def command_output(arguments):
    return subprocess.run(
        arguments, cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def load_fragment(directory, scenario):
    path = directory / f"{scenario}.json"
    if not path.is_file():
        raise SystemExit(f"missing capture fragment {path}")
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict) or list(document) != [scenario]:
        raise SystemExit(f"{path} must contain exactly the scenario {scenario}")
    return document[scenario]


def build_corpus(arguments):
    extra = sorted(
        path.stem
        for path in arguments.capture_dir.glob("*.json")
        if path.stem not in SCENARIOS
    )
    if extra:
        raise SystemExit(f"unknown capture fragments: {', '.join(extra)}")
    corpus = {
        "name": CORPUS_NAME,
        "captured_at_revision": arguments.revision or command_output(["git", "rev-parse", "HEAD"]),
        "capture_command": arguments.capture_command,
        "rustc_version": arguments.rustc_version or command_output(["rustc", "-V"]),
        "scenarios": {
            scenario: load_fragment(arguments.capture_dir, scenario) for scenario in SCENARIOS
        },
    }
    if set(corpus) != set(METADATA_KEYS):
        raise SystemExit("the corpus keys differ from the metadata keys")
    return json.dumps(corpus, sort_keys=True, indent=2, ensure_ascii=False) + "\n"


def main():
    parser = argparse.ArgumentParser(
        description="Assemble the queue parity golden of P5 from per-scenario capture fragments."
    )
    parser.add_argument("--capture-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--capture-command", required=True)
    parser.add_argument("--revision")
    parser.add_argument("--rustc-version")
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()

    text = build_corpus(arguments)
    if arguments.check:
        current = arguments.output.read_text(encoding="utf-8") if arguments.output.is_file() else None
        if current != text:
            print(f"{arguments.output} differs from the assembled capture", file=sys.stderr)
            return 1
        return 0
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = arguments.output.with_name(arguments.output.name + ".partial")
    with temporary.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)
    temporary.replace(arguments.output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
