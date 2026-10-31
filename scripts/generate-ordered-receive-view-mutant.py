"""Apply one production-source mutant of the ordered receive byte view to an exported tree.

Each mutation is the executable analogue of one closed counterexample of the Rocq module
OrderedReceiveByteViewMutants and must be killed by its named test. The generator refuses to
mutate the checkout that contains it.
"""

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VIEW = "rspace++/src/rspace/ordered_receive.rs"
PATHS = "rspace++/src/rspace/rho_path.rs"
SPEC = "rspace++/tests/ordered_receive_view_spec.rs"

MUTATIONS = {
    "zip_truncating_arity": {
        "rocq": "zip_truncating_arity_is_detected",
        "source": VIEW,
        "description": "the view alignment compares the channel count with itself, so every arity is admitted",
        "before": "        if group.channel_count() != record.arity() {\n",
        "after": "        if group.channel_count() != group.channel_count() {\n",
        "test_file": SPEC,
        "test": "admission_refuses_mismatched_arity_before_any_position",
    },
    "wrapping_peek_tail": {
        "rocq": "wrapping_peek_tail_is_detected",
        "source": VIEW,
        "description": "the expected identity end wraps instead of refusing an overflowing peek count",
        "before": (
            "            .and_then(|count| count.checked_mul(PEEK_WIDTH))\n"
            "            .and_then(|width| width.checked_add(peeks_start))\n"
        ),
        "after": (
            "            .map(|count| count.wrapping_mul(PEEK_WIDTH))\n"
            "            .map(|width| width.wrapping_add(peeks_start))\n"
        ),
        "test_file": SPEC,
        "test": "peek_count_overflow_is_checked_without_wrap",
    },
    "narrowed_count_alias": {
        "rocq": "narrowed_count_alias_is_detected",
        "source": VIEW,
        "description": "the record header's pattern count is compared modulo 2^32",
        "before": (
            "        if u64::try_from(pattern_ends.len()).ok() != Some(u64::from_le_bytes(*header)) {\n"
        ),
        "after": (
            "        if u64::try_from(pattern_ends.len()).ok()\n"
            "            != Some(u64::from_le_bytes(*header) % (1u64 << 32))\n"
            "        {\n"
        ),
        "test_file": SPEC,
        "test": "malformed_field_bounds_and_peek_tails_are_refused",
    },
    "shifted_capture_bound": {
        "rocq": "shifted_capture_bound_is_detected",
        "source": VIEW,
        "description": "every pattern slice ends one byte early, so its trailing free count shifts",
        "before": "        let finish = *self.pattern_ends.get(index)?;\n",
        "after": "        let finish = self.pattern_ends.get(index)?.checked_sub(1)?;\n",
        "test_file": SPEC,
        "test": "view_binds_align_channel_pattern_and_effective_peek_by_position",
    },
    "source_bearing_identity": {
        "rocq": "source_bearing_identity_is_detected",
        "source": VIEW,
        "description": "the record identity includes the consume source",
        "before": "    pub fn identity(&self) -> &'a [u8] { &self.record[..self.identity_end] }\n",
        "after": "    pub fn identity(&self) -> &'a [u8] { self.record }\n",
        "test_file": SPEC,
        "test": "first_representative_and_exact_source_suffix_are_retained",
    },
    "misplaced_source_split": {
        "rocq": "misplaced_source_split_is_detected",
        "source": VIEW,
        "description": "the source suffix starts at the body end instead of the identity end",
        "before": "    pub fn source_suffix(&self) -> &'a [u8] { &self.record[self.identity_end..] }\n",
        "after": "    pub fn source_suffix(&self) -> &'a [u8] { &self.record[self.body_end..] }\n",
        "test_file": SPEC,
        "test": "prepared_record_bytes_equal_the_frozen_encoders",
    },
    "normalized_peek_tail": {
        "rocq": "normalized_peek_tail_is_detected",
        "source": VIEW,
        "description": "the strictly ascending peek check accepts every pair of peeks",
        "before": (
            "            .all(|pair| matches!(pair, [left, right] if peek_value(left) < peek_value(right)))\n"
        ),
        "after": "            .all(|pair| matches!(pair, [_, _]))\n",
        "test_file": SPEC,
        "test": "malformed_field_bounds_and_peek_tails_are_refused",
    },
    "wrapping_signed_position": {
        "rocq": "wrapping_signed_position_is_detected",
        "source": PATHS,
        "description": "a receive position wraps to a 32-bit signed peek key",
        "before": "    pub fn signed_peek_key(self) -> Option<i32> { i32::try_from(self.0).ok() }\n",
        "after": "    pub fn signed_peek_key(self) -> Option<i32> { Some(self.0 as u32 as i32) }\n",
        "test_file": VIEW,
        "test": "wide_positions_convert_without_wrap",
    },
    "unframed_prefix_match": {
        "rocq": "unframed_prefix_match_is_detected",
        "source": PATHS,
        "description": "a channel frame also matches a query that is only a prefix of its bytes",
        "before": "            if frame_equals(frame, self.query) {\n",
        "after": "            if frame_equals(frame, self.query) || frame.starts_with(self.query) {\n",
        "test_file": SPEC,
        "test": "legacy_store_views_return_exact_ordered_positions_for_repeated_and_prefix_related_channels",
    },
    "profile_blind_query": {
        "rocq": "profile_blind_query_is_detected",
        "source": PATHS,
        "description": "a query of the other profile reads the view's frames instead of being refused",
        "before": (
            "            (ReceiveChannelProfile::LegacyV1, ReceiveChannelQuery::Rci2V2(_)) |\n"
            "            (ReceiveChannelProfile::Rci2V2, ReceiveChannelQuery::LegacyV1(_)) => None,\n"
        ),
        "after": (
            "            (ReceiveChannelProfile::LegacyV1, ReceiveChannelQuery::Rci2V2(raw)) |\n"
            "            (ReceiveChannelProfile::Rci2V2, ReceiveChannelQuery::LegacyV1(raw)) => {\n"
            "                Some(RhoPathChannelPositions::new(self.encoded_channels, self.channel_count, raw))\n"
            "            }\n"
        ),
        "test_file": SPEC,
        "test": "profile_queries_are_explicit",
    },
    "universal_nonempty_rule": {
        "rocq": "universal_nonempty_rule_is_detected",
        "source": VIEW,
        "description": "admission refuses every record without patterns",
        "before": (
            "        if u64::try_from(pattern_ends.len()).ok() != Some(u64::from_le_bytes(*header)) {\n"
        ),
        "after": (
            "        if pattern_ends.is_empty()\n"
            "            || u64::try_from(pattern_ends.len()).ok() != Some(u64::from_le_bytes(*header))\n"
            "        {\n"
        ),
        "test_file": SPEC,
        "test": "empty_aligned_view_has_no_bind_and_v2_empty_group_stays_refused",
    },
    "noncanonical_persist": {
        "rocq": "noncanonical_persist_is_detected",
        "source": VIEW,
        "description": "every nonzero persist byte reads as persistent",
        "before": "            Some([byte, ..]) => return Err(OrderedReceiveError::InvalidPersist(*byte)),\n",
        "after": "            Some([_, after_persist @ ..]) => (true, after_persist),\n",
        "test_file": SPEC,
        "test": "malformed_field_bounds_and_peek_tails_are_refused",
    },
}


def apply(name, tree):
    if tree.resolve() == ROOT:
        raise SystemExit("refusing to mutate the checkout that contains this generator")
    mutation = MUTATIONS[name]
    target = tree / mutation["source"]
    source = target.read_text(encoding="utf-8")
    if source.count(mutation["before"]) != 1:
        raise SystemExit(f"{name}: source anchor is absent or ambiguous in {mutation['source']}")
    target.write_text(source.replace(mutation["before"], mutation["after"], 1), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("list")
    applied = commands.add_parser("apply")
    applied.add_argument("name", choices=sorted(MUTATIONS))
    applied.add_argument("tree", type=Path)
    args = parser.parse_args()
    if args.command == "list":
        for name, mutation in MUTATIONS.items():
            print("\t".join((name, mutation["source"], mutation["test_file"], mutation["test"])))
    else:
        apply(args.name, args.tree)


if __name__ == "__main__":
    sys.exit(main())
