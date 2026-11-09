#!/usr/bin/env python3
"""Make one causal RPB3 admission mutant from the verified shared model."""

import argparse
from pathlib import Path


MUTATIONS = {
    "unchecked_length": (
        "// MUTANT_ANCHOR_UNCHECKED_LENGTH\n        if (top.frame_kind != 1 && !map_key_continue)\n            || next_cursor > top.region_end",
        "// MUTANT_ANCHOR_UNCHECKED_LENGTH\n        if (top.frame_kind != 1 && !map_key_continue)\n            || false",
    ),
    "non_progress": (
        "// MUTANT_ANCHOR_NON_PROGRESS\n            || next_cursor <= top.cursor",
        "// MUTANT_ANCHOR_NON_PROGRESS\n            || false",
    ),
    "dropped_child": (
        "// MUTANT_ANCHOR_DROPPED_CHILD\n                frames: state.frames.update(index, parent_next_frame).push(child_next_frame),",
        "// MUTANT_ANCHOR_DROPPED_CHILD\n                frames: state.frames.update(index, parent_next_frame),",
    ),
    "skipped_root": (
        "// MUTANT_ANCHOR_SKIPPED_ROOT\n        root.cursor == state.root_end",
        "// MUTANT_ANCHOR_SKIPPED_ROOT\n        true",
    ),
    "unbound_epm_snapshot": (
        "        || before.score.admission.frames.last().returns.last().body_bytes != snapshot_bytes",
        "        || false",
    ),
    "unbound_epm_unwind_checkpoints": (
        "            &&& run.checkpoints == frame.path.restore.subrange(\n"
        "                restore_len as int - run_width, restore_len as int,\n"
        "            )",
        "            &&& true",
    ),
    "unbound_epm_value_stability": (
        "        EpmLoopBranch::ScalarValueNoBody {\n"
        "            has_value, reducer_stable, root_stable,\n"
        "        } => {\n"
        "            &&& before.cursor.value_table_offset == after.cursor.value_table_offset\n"
        "            &&& before.path == after.path\n"
        "            &&& before.root_gate == after.root_gate\n"
        "            &&& root_stable == before.root_gate.stable\n"
        "            &&& before.obligation == after.obligation\n"
        "            &&& reducer_stable == if has_value { Some(root_stable) } else { None }\n"
        "            &&& after.entries_stable == (before.entries_stable && match reducer_stable {\n"
        "                Some(stable) => stable,\n"
        "                None => true,\n"
        "            })",
        "        EpmLoopBranch::ScalarValueNoBody {\n"
        "            has_value, reducer_stable, root_stable,\n"
        "        } => {\n"
        "            &&& before.cursor.value_table_offset == after.cursor.value_table_offset\n"
        "            &&& before.path == after.path\n"
        "            &&& before.root_gate == after.root_gate\n"
        "            &&& true\n"
        "            &&& before.obligation == after.obligation\n"
        "            &&& reducer_stable == if has_value { Some(root_stable) } else { None }\n"
        "            &&& after.entries_stable == (before.entries_stable && match reducer_stable {\n"
        "                Some(stable) => stable,\n"
        "                None => true,\n"
        "            })",
    ),
    "unbound_epm_push_body_end": (
        "            &&& body_range.end == after.cursor.value_table_offset",
        "            &&& true",
    ),
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("variant", choices=MUTATIONS)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    source = args.source.read_text()
    before, after = MUTATIONS[args.variant]
    count = source.count(before)
    if count != 1:
        parser.error(f"expected one {args.variant} anchor, found {count}")
    args.output.write_text(source.replace(before, after, 1))


if __name__ == "__main__":
    main()
