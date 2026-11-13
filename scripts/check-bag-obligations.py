"""Inventory Bag/checked-delta obligations and their concrete normal-suite witnesses."""

import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/bag_obligations.generated.json"
ROCQ = "formal/rocq/stack_safe_pda/theories/Bag.v"
VERUS = "formal/verus/bag_checked_delta.rs"
TEST = "rspace++/tests/bag_spec.rs"
CARGO = "rspace++/Cargo.toml"
CONFIG = "formal/tlaplus/stack_safe_pda/RSpaceBagCommit.cfg"
DELEGATE = "formal/accepted_match_obligations.generated.json"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"

# Exhaustive declared source inventory. A new declaration requires an explicit review.
ROCQ_NAMES = re.findall(
    r"\S+",
    """
set_count_is_exact sum_empty_identity sum_commutative sum_associative
join_commutative join_associative join_idempotent join_empty_identity
meet_commutative meet_associative meet_idempotent meet_empty_absorbing
join_absorbs_meet meet_absorbs_join meet_distributes_over_join
join_distributes_over_meet subtraction_conservation subtract_self_empty
meet_is_subbag_left meet_is_subbag_right cursor_step_preserves_every_other_key
empty_cursor_valid cursor_step_preserves_exact_counts
cursor_walk_preserves_exact_counts cursor_finished_is_pointwise_algebra
cursor_walk_preserves_visited cursor_walk_visits_input
cursor_walk_refines_pointwise_algebra cursor_join_refines_bag_join
cursor_meet_refines_bag_meet cursor_subtract_refines_bag_subtract
cursor_sum_refines_bag_sum
cursor_subbag_step_refines_conjunction empty_subbag_cursor_valid
cursor_subbag_step_preserves_validity cursor_subbag_finished_is_exact
cursor_subbag_walk_checks_every_key cursor_subbag_walk_refines_finite_support
cursor_count_respects_bound normalized_delta_reversible
canonical_delta_is_normalized canonical_delta_is_applicable
canonical_delta_reaches_target canonical_delta_round_trip
nonpersistent_stored_candidate_removes_one
nonpersistent_stored_candidate_is_exact
persistent_stored_candidate_is_not_removed fresh_candidate_is_not_removed
selected_removals_permutation_invariant selected_commit_exact_count
permuted_play_replay_commits_equal rejected_commit_publishes_no_partial_state
accepted_commit_publishes_exact_proposal
overflowing_endpoint_publishes_nothing
checked_cursor_sum_publishes_the_bounded_sum
wrapping_cursor_sum_violates_atomicity
""",
)

ORACLE = "bag_algebra_matches_a_btreemap_multiplicity_oracle"
DELTA = "normalized_transaction_delta_applies_and_reverts_exactly"
OVERFLOW = "sum_refuses_the_first_overflowing_endpoint_in_either_operand_order"
DIRECT = {
    "set_count_is_exact": "insert_remove_and_metadata_preserve_the_absent_zero_invariant",
    **{
        name: ORACLE
        for name in (
            "join_commutative",
            "join_associative",
            "join_idempotent",
            "meet_commutative",
            "meet_associative",
            "meet_idempotent",
            "join_absorbs_meet",
            "meet_absorbs_join",
            "meet_distributes_over_join",
            "join_distributes_over_meet",
            "subtraction_conservation",
        )
    },
    "normalized_delta_reversible": DELTA,
    **{
        name: ORACLE
        for name in (
            "canonical_delta_is_applicable",
            "canonical_delta_reaches_target",
            "canonical_delta_round_trip",
        )
    },
    "overflowing_endpoint_publishes_nothing": OVERFLOW,
    "checked_cursor_sum_publishes_the_bounded_sum": ORACLE,
}
VERUS_DECLS = [
    ("apply_count_spec", "open spec", None),
    (
        "checked_apply_count",
        "exec",
        "transaction_delta_application_is_atomic_on_underflow_and_overflow",
    ),
    ("normalized_checked_delta_reversible", "proof", DELTA),
]
NEGATIVE_CONTROL = "prefix_coupled_algebra_mutants_disagree_with_the_exact_key_oracle"


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def names(source, pattern, label):
    result = re.findall(pattern, source, flags=re.MULTILINE)
    require(len(result) == len(set(result)), f"duplicate {label}")
    return result


def test_target(source, name):
    matches = list(re.finditer(rf"(?m)^\s*fn\s+{re.escape(name)}\s*\(", source))
    require(len(matches) == 1, f"missing or duplicate normal-suite test {name}")
    attrs = []
    for line in reversed(source[: matches[0].start()].splitlines()):
        line = line.strip()
        if not line.startswith("#["):
            break
        attrs.append(line)
    require("#[test]" in attrs, f"non-test witness {name}")
    require(
        not any(a.startswith("#[ignore") for a in attrs), f"ignored-only witness {name}"
    )
    require(
        not any(a.startswith(("#[cfg(", "#[cfg_attr(")) for a in attrs),
        f"conditionally absent witness {name}",
    )


def extract(sources):
    rocq = sources[ROCQ]
    verus = sources[VERUS]
    tests = sources[TEST]
    runner = sources[RUNNER]
    actual_rocq = names(
        rocq,
        r"^(?:Theorem|Corollary|Lemma|Proposition|Fact|Remark)\s+([A-Za-z_][A-Za-z_0-9]*)\s*:",
        "Bag.v proof declaration",
    )
    require(actual_rocq == ROCQ_NAMES, "missing, added, or reordered Bag.v theorem")
    require(
        not re.search(r"\b(?:Admitted|admit|Axiom|Parameter)\b", rocq),
        "Bag.v contains an admitted or axiomatic declaration",
    )
    prints = names(
        rocq,
        r"^Print Assumptions\s+([A-Za-z_][A-Za-z_0-9]*)\s*\.",
        "Rocq Print Assumptions target",
    )
    require(not prints, "new Bag.v Print Assumptions target needs explicit binding")
    actual_verus = re.findall(
        r"(?m)^(?:pub\s+)?(?:(open spec|proof|spec)\s+)?fn\s+([A-Za-z_][A-Za-z_0-9]*)\s*\(",
        verus,
    )
    actual_verus = [(name, kind or "exec") for kind, name in actual_verus]
    # The ordinary Rust entry point is outside verus! and is not an obligation.
    require(
        actual_verus
        == [(name, kind) for name, kind, _ in VERUS_DECLS] + [("main", "exec")],
        "missing, added, or reordered Verus declaration",
    )
    require(
        "verus! {" in verus and len(re.findall(r"\bverus!\s*\{", verus)) == 1,
        "missing or duplicate Verus block",
    )
    require(
        re.search(r"(?m)^autotests\s*=\s*false\b", sources[CARGO]) is None
        and re.search(
            r"(?s)\[\[test\]\].*?name\s*=\s*\"bag_spec\".*?harness\s*=\s*false",
            sources[CARGO],
        )
        is None,
        "bag_spec is absent from normal Cargo test discovery",
    )
    require(
        'python3 "$repo_root/scripts/check-bag-obligations.py" --check' in runner
        and 'python3 "$repo_root/scripts/check-bag-obligations.py" --self-test'
        in runner,
        "Bag registry check is absent from normal proof runner",
    )
    require(
        '"$repo_root/formal/verus/bag_checked_delta.rs"' in runner
        and "theories/Bag.v" in runner,
        "Bag Rocq or Verus source absent from normal proof runner",
    )
    require(
        'python3 "$repo_root/scripts/check-accepted-match-obligations.py" --check'
        in runner
        and "RSpaceBagCommit.cfg RSpaceBagCommit.tla" in runner,
        "delegated RSpaceBagCommit gate absent from normal proof runner",
    )
    all_targets = (
        set(DIRECT.values())
        | {target for _, _, target in VERUS_DECLS if target}
        | {NEGATIVE_CONTROL}
    )
    for target in sorted(all_targets):
        test_target(tests, target)
    cfg_invariants = names(
        sources[CONFIG],
        r"^INVARIANT\s+([A-Za-z_][A-Za-z_0-9]*)\s*$",
        "RSpaceBagCommit invariant",
    )
    require(
        len(cfg_invariants) == 10,
        "RSpaceBagCommit configured invariant inventory changed",
    )
    require(
        len(re.findall(r"(?m)^INVARIANT\b", sources[CONFIG])) == len(cfg_invariants),
        "unparsed RSpaceBagCommit invariant",
    )
    delegated = json.loads(sources[DELEGATE])
    require(
        delegated.get("config") == CONFIG,
        "accepted-match registry no longer owns RSpaceBagCommit",
    )
    delegate_names = [row["obligation"] for row in delegated["tlc_invariants"]]
    require(
        cfg_invariants == delegate_names,
        "RSpaceBagCommit inventory diverges from accepted-match gate",
    )
    require(
        len(delegate_names) == len(set(delegate_names)),
        "duplicate delegated RSpaceBagCommit invariant",
    )
    rocq_rows = [
        {
            "id": f"BAG-RC-{index:02d}",
            "obligation": name,
            "mode": "concrete_witness" if name in DIRECT else "proof_only",
            "target_name": DIRECT.get(name),
            "target_source": TEST if name in DIRECT else None,
        }
        for index, name in enumerate(actual_rocq, 1)
    ]
    verus_rows = [
        {
            "id": f"BAG-VS-{index:02d}",
            "obligation": name,
            "kind": kind,
            "mode": "concrete_witness" if target else "proof_only",
            "target_name": target,
            "target_source": TEST if target else None,
        }
        for index, (name, kind, target) in enumerate(VERUS_DECLS, 1)
    ]
    return {
        "scope": "Bag multiplicity algebra and checked scalar delta",
        "boundary": "Concrete witnesses exercise Bag behavior; they do not prove Rocq/Verus refinement or arbitrary inputs.",
        "sources_sha256": {
            path: hashlib.sha256(sources[path].encode()).hexdigest()
            for path in (ROCQ, VERUS, TEST, CARGO, CONFIG, DELEGATE)
        },
        "rocq_source": ROCQ,
        "rocq_print_assumptions": prints,
        "rocq_obligations": rocq_rows,
        "verus_source": VERUS,
        "verus_obligations": verus_rows,
        "normal_suite_test_source": TEST,
        "negative_control": NEGATIVE_CONTROL,
        "delegated_tla": {
            "config": CONFIG,
            "registry": DELEGATE,
            "gate": "scripts/check-accepted-match-obligations.py",
            "invariants": cfg_invariants,
        },
    }


def self_test(sources, baseline):
    def rejected(path, replacement, label):
        changed = dict(sources)
        changed[path] = replacement
        try:
            extract(changed)
        except RegistryError:
            return
        raise RegistryError(f"mutation unexpectedly accepted: {label}")

    rejected(
        ROCQ,
        sources[ROCQ].replace(
            "Theorem set_count_is_exact :", "Theorem missing_set_count :", 1
        ),
        "omitted Rocq obligation",
    )
    rejected(
        ROCQ,
        sources[ROCQ] + "\nTheorem set_count_is_exact : True.\n",
        "duplicate Rocq obligation",
    )
    rejected(
        VERUS,
        sources[VERUS].replace(
            "pub proof fn normalized_checked_delta_reversible",
            "pub proof fn stale_delta_proof",
            1,
        ),
        "stale Verus obligation",
    )
    rejected(
        TEST,
        sources[TEST].replace(f"fn {DELTA}(", f"#[ignore]\nfn {DELTA}(", 1),
        "ignored-only witness",
    )
    rejected(
        TEST,
        sources[TEST].replace(f"fn {DELTA}(", f"fn removed_{DELTA}(", 1),
        "missing witness",
    )
    rejected(
        CONFIG,
        sources[CONFIG].replace("INVARIANT TypeInvariant\n", "", 1),
        "omitted delegated invariant",
    )
    delegated = json.loads(sources[DELEGATE])
    delegated["tlc_invariants"].pop()
    rejected(DELEGATE, json.dumps(delegated), "stale delegated registry")
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            'python3 "$repo_root/scripts/check-bag-obligations.py" --check', "", 1
        ),
        "missing normal proof gate",
    )
    require(extract(sources) == baseline, "nondeterministic Bag registry")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    paths = (ROCQ, VERUS, TEST, CARGO, CONFIG, DELEGATE, RUNNER)
    sources = {path: (ROOT / path).read_text() for path in paths}
    registry = extract(sources)
    rendered = json.dumps(registry, indent=2, sort_keys=True) + "\n"
    if args.write:
        (ROOT / OUTPUT).write_text(rendered)
    elif args.check:
        require(
            (ROOT / OUTPUT).read_text() == rendered, "generated Bag registry is stale"
        )
    else:
        self_test(sources, registry)
    print(
        f"Bag registry {'mutations passed' if args.self_test else 'current'}: {len(ROCQ_NAMES)} Rocq, {len(VERUS_DECLS)} Verus, 10 delegated TLC"
    )


if __name__ == "__main__":
    try:
        main()
    except (RegistryError, KeyError, json.JSONDecodeError) as error:
        raise SystemExit(f"Bag registry: {error}") from error
