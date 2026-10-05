import argparse
import json
import subprocess
import sys
from pathlib import Path

CORPUS_NAME = "ordinary_execution_v1"
DEFAULT_CODE_IDENTICAL_TO = "e748426"
SECTIONS = (
    ("r1", "r1_comm_produce_sort"),
    ("r2", "r2_peek_removes_and_reproduces"),
    ("r3", "r3_greedy_join_first_match"),
    ("r4", "r4_leaf_bytes"),
    ("r5", "r5_event_struct_and_proto_bytes"),
    ("r6", "r6_cost_vectors"),
    ("r7", "r7_term_prost_and_bincode_bytes"),
    ("r8", "r8_state_dump"),
    ("r9", "r9_play_replay_equality"),
    ("dc", "deterministic_candidates"),
)
METADATA_KEYS = (
    "name",
    "captured_at_revision",
    "code_identical_to",
    "capture_command",
    "rustc_version",
)

ROOT = Path(__file__).resolve().parent.parent.parent


def command_output(arguments):
    return subprocess.run(
        arguments, cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def load_fragment(directory, fragment, section):
    path = directory / f"{fragment}.json"
    if not path.is_file():
        raise SystemExit(f"missing capture fragment {path}")
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict) or list(document) != [section]:
        raise SystemExit(f"{path} must contain exactly the section {section}")
    return document[section]


def build_corpus(arguments):
    corpus = {
        "name": CORPUS_NAME,
        "captured_at_revision": arguments.revision or command_output(["git", "rev-parse", "HEAD"]),
        "code_identical_to": arguments.code_identical_to,
        "capture_command": arguments.capture_command,
        "rustc_version": arguments.rustc_version or command_output(["rustc", "-V"]),
    }
    for fragment, section in SECTIONS:
        if section in METADATA_KEYS:
            raise SystemExit(f"section {section} collides with a metadata key")
        corpus[section] = load_fragment(arguments.capture_dir, fragment, section)
    return json.dumps(corpus, sort_keys=True, indent=2, ensure_ascii=False) + "\n"


def main():
    parser = argparse.ArgumentParser(
        description="Assemble the ordinary execution regression corpus from per-test capture fragments."
    )
    parser.add_argument("--capture-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--capture-command", required=True)
    parser.add_argument("--revision")
    parser.add_argument("--code-identical-to", default=DEFAULT_CODE_IDENTICAL_TO)
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
