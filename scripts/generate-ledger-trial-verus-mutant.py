import sys
from pathlib import Path


MUTATIONS = {
    "partial_commit": (
        "        None => return Some(TrialAxis::Count),\n    };\n",
        "        None => return Some(TrialAxis::Count),\n    };\n    usage.peak_count = count;\n",
    ),
    "unchecked_limit": (
        "    if amount > limit || used > limit - amount {",
        "    if used > u64::MAX - amount {",
    ),
    "scratch_conflation": (
        "        charge.live_scratch,\n",
        "        charge.expansions,\n",
    ),
}


def main() -> None:
    if len(sys.argv) != 4 or sys.argv[1] not in MUTATIONS:
        raise SystemExit(
            "usage: generate-ledger-trial-verus-mutant.py "
            "{partial_commit|unchecked_limit|scratch_conflation} SOURCE OUTPUT"
        )
    mutation, source_path, output_path = sys.argv[1:]
    source = Path(source_path).read_text(encoding="utf-8")
    before, after = MUTATIONS[mutation]
    if source.count(before) != 1:
        raise SystemExit(f"{mutation}: source anchor is absent or ambiguous")
    Path(output_path).write_text(source.replace(before, after, 1), encoding="utf-8")


if __name__ == "__main__":
    main()
