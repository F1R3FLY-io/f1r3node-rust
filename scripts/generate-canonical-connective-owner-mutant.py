import sys
from pathlib import Path


MUTATIONS = {
    "length_only": (
        "if source_address != owner_address || source_length != owner_length {",
        "if source_length != owner_length {",
    ),
    "pointer_only": (
        "if source_address != owner_address || source_length != owner_length {",
        "if source_address != owner_address {",
    ),
    "skip_child": (
        "remaining: (position.remaining - 1) as usize,",
        "remaining: position.remaining,",
    ),
    "wrong_range": (
        "start: child.payload_start,",
        "start: child.header_start,",
    ),
    "early_exhaustion": (
        "if position.remaining == 0 {\n        None\n    } else {",
        "if position.remaining <= 1 {\n        None\n    } else {",
    ),
}


def main() -> None:
    if len(sys.argv) != 4 or sys.argv[1] not in MUTATIONS:
        raise SystemExit(
            "usage: generate-canonical-connective-owner-mutant.py "
            "{length_only|pointer_only|skip_child|wrong_range|early_exhaustion} "
            "SOURCE OUTPUT"
        )
    mutation, source_path, output_path = sys.argv[1:]
    source = Path(source_path).read_text(encoding="utf-8")
    before, after = MUTATIONS[mutation]
    if source.count(before) != 1:
        raise SystemExit(f"{mutation}: source anchor is absent or ambiguous")
    Path(output_path).write_text(source.replace(before, after, 1), encoding="utf-8")


if __name__ == "__main__":
    main()
