import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/epathmap_core_obligations.generated.json"
ROCQ = "formal/rocq/stack_safe_pda/theories"
TLA = "formal/tlaplus/stack_safe_pda"
VERUS = "formal/verus/epathmap_canonical_worklist.rs"
CONFIG = f"{TLA}/PDAEquivalence.cfg"
MODEL = f"{TLA}/PDAEquivalence.tla"
DELEGATE = "formal/epm1_positional_obligations.generated.json"
DELEGATE_CONFIG = f"{TLA}/Epm1PositionalAssociation.cfg"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
CARGO = "models/Cargo.toml"
ALGEBRA = "models/tests/epathmap_algebra.rs"
SNAPSHOT = "models/tests/epathmap_epm1_snapshot.rs"

ROCQ_SOURCES = {
    "StackSafePDA.v": (
        10,
        "91f8205ad30400ae8f253275ce688543ce1ad2c304685937c73ded0015078edf",
        0,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    "EPathMap.v": (
        49,
        "91aacc9aa8c4c8636c666b3a3ad6e2b73e268be4f8f543b9746f094a83759815",
        9,
        "c068bcca639ab7b00d68e167de4cef8b51a88727a200a3db40705fdebcb15874",
    ),
    "EPM1.v": (
        39,
        "ba189d175a10e1626646c0c617328232c0c5770b882f9964484557d4d2e55ca8",
        7,
        "812310a9cc6a13e7ce46f46dad8e108681d65bca1f2e16b87133ebf038de8e4b",
    ),
}
VERUS_COUNT = 5
VERUS_DIGEST = "a7bc6bd2413eb756975f72914e442e27186a29b235485a6d5f10d3f8059658bf"
TLA_INVARIANTS = ["TypeInvariant", "NoInstructionUnderflow", "CompletedEquivalent"]
DIRECT = {
    ("EPathMap.v", "set_join_commutative"): (
        ALGEBRA,
        "set_algebra_uses_pathmap_lattice_operations",
    ),
    ("EPathMap.v", "set_join_idempotent"): (
        ALGEBRA,
        "set_algebra_uses_pathmap_lattice_operations",
    ),
    ("EPathMap.v", "set_subtract_removes_members"): (
        ALGEBRA,
        "set_algebra_uses_pathmap_lattice_operations",
    ),
    ("EPathMap.v", "set_subtract_keeps_members_under_longer_right_keys"): (
        ALGEBRA,
        "native_set_difference_is_key_by_key_removal_under_prefix_overlap",
    ),
    ("EPathMap.v", "prefix_overlap_witness_keeps_the_bare_member"): (
        ALGEBRA,
        "set_difference_keeps_members_under_longer_right_keys",
    ),
    ("EPathMap.v", "restrict_by_prefix_keeps_members_whose_key_the_selector_holds"): (
        ALGEBRA,
        "native_restriction_is_the_rebuilt_set_of_members_with_a_selected_prefix",
    ),
    ("EPathMap.v", "restriction_witness_keeps_the_bare_member"): (
        ALGEBRA,
        "restriction_keeps_members_whose_key_the_selector_holds",
    ),
    ("EPathMap.v", "set_subtract_leaves_no_path_where_a_member_was_removed"): (
        ALGEBRA,
        "native_set_difference_is_the_rebuilt_set_of_its_surviving_members",
    ),
    ("EPathMap.v", "removed_member_witness_leaves_no_path"): (
        ALGEBRA,
        "set_difference_leaves_no_path_where_a_member_was_removed",
    ),
    ("EPathMap.v", "unequal_overlap_is_exactly_a_conflict"): (
        ALGEBRA,
        "map_algebra_preserves_associations_and_rejects_unequal_overlaps",
    ),
    ("EPathMap.v", "unequal_meet_overlap_is_exactly_a_conflict"): (
        ALGEBRA,
        "map_algebra_preserves_associations_and_rejects_unequal_overlaps",
    ),
    ("EPathMap.v", "subtract_overlap_is_value_independent_key_mask"): (
        ALGEBRA,
        "map_algebra_preserves_associations_and_rejects_unequal_overlaps",
    ),
    ("EPathMap.v", "every_binary_operation_rejects_mixed_nonempty_modes"): (
        ALGEBRA,
        "nonempty_set_and_map_modes_cannot_mix",
    ),
    ("EPM1.v", "epm1_framed_decode_encode_identity"): (
        SNAPSHOT,
        "live_store_preserves_map_mode_and_key_value_associations_on_both_surfaces",
    ),
    ("EPM1.v", "epm1_preserves_topology_and_ordered_value_table"): (
        SNAPSHOT,
        "live_store_preserves_map_mode_and_key_value_associations_on_both_surfaces",
    ),
    ("EPM1.v", "epm1_round_trip_preserves_positional_value_association"): (
        SNAPSHOT,
        "live_store_preserves_map_mode_and_key_value_associations_on_both_surfaces",
    ),
}
EVIDENCE = {
    "set_algebra_uses_pathmap_lattice_operations": (
        "try_join(right.entry_trie())",
        "try_subtract(right.entry_trie())",
        "vec![1, 2]",
        "assert_eq!(set_values(&ab), set_values(&ba))",
        "assert_eq!(set_values(&idem), set_values(left.entry_trie()))",
    ),
    "native_set_difference_is_key_by_key_removal_under_prefix_overlap": (
        "prefix_overlapping_member()",
        "!removed.contains(&encode_trie_path(member))",
        "try_subtract(right.entry_trie())",
        "prop_assert_eq!(difference.entries_owned(), expected.clone())",
        "subtract_encoded_keys(&mask)",
        "prop_assert_eq!(masked.entries_owned(), expected)",
    ),
    "set_difference_keeps_members_under_longer_right_keys": (
        "vec![gint(1), gint(2), gint(3), list([1, 2])]",
        "vec![gint(1), gint(2), gint(3)]",
        "vec![gint(7), list([7]), list([7, 7])]",
        "vec![gint(7), list([7])]",
        "try_subtract(right.entry_trie())",
        "assert_eq!(difference.entries_owned(), expected)",
        "subtract_encoded_keys(&mask)",
        "assert_eq!(masked.entries_owned(), expected)",
    ),
    "native_restriction_is_the_rebuilt_set_of_members_with_a_selected_prefix": (
        "prefix_overlapping_member()",
        "(0..=key.len()).any(|length| selected.contains(&key[..length]))",
        "try_restrict(selector.entry_trie())",
        "prop_assert_eq!(restricted.entries_owned(), expected.entry_trie().entries_owned())",
        "prop_assert_eq!(restricted.trie_snapshot(), expected.entry_trie().trie_snapshot())",
    ),
    "restriction_keeps_members_whose_key_the_selector_holds": (
        "EPathMap::new(vec![gint(1)], vec![], false, None)",
        "vec![gint(1), gint(2), gint(3), list([1, 2])]",
        "try_restrict(selector.entry_trie())",
        "try_restrict_member_prefixes(selector.entry_trie())",
        "assert_eq!(restricted.entries_owned(), vec![gint(1)])",
        "assert_eq!(by_member_prefixes.entries_owned(), vec![gint(1)])",
        "base.entry_trie().trie_snapshot()",
    ),
    "native_set_difference_is_the_rebuilt_set_of_its_surviving_members": (
        "prefix_overlapping_member()",
        "!removed.contains(&encode_trie_path(member))",
        "for right_order in [right_members, reversed_members]",
        "try_subtract(right.entry_trie())",
        "prop_assert_eq!(difference.trie_snapshot(), expected.entry_trie().trie_snapshot())",
        "for mask in [&ascending_mask, &descending_mask]",
        "prop_assert_eq!(masked.trie_snapshot(), expected.entry_trie().trie_snapshot())",
    ),
    "set_difference_leaves_no_path_where_a_member_was_removed": (
        "bare_and_list.entry_trie().entries_owned()",
        "(&bare_and_list, &reinserted, &empty)",
        "(&wide, &evens, odds.entry_trie())",
        "(&evens, &wide, &empty)",
        "try_subtract(right.entry_trie())",
        "assert_eq!(difference.trie_snapshot(), expected.trie_snapshot())",
        "subtract_encoded_keys(&mask)",
        "assert_eq!(masked.trie_snapshot(), expected.trie_snapshot())",
    ),
    "map_algebra_preserves_associations_and_rejects_unequal_overlaps": (
        "try_join(conflict.entry_trie())",
        "try_meet(conflict.entry_trie())",
        "Err(EPathMapAlgebraError::ValueConflict { .. })",
        "try_subtract(conflict.entry_trie())",
        "assert_map_value(&masked, 2, None)",
    ),
    "nonempty_set_and_map_modes_cannot_mix": (
        "try_join(map.entry_trie())",
        "try_meet(map.entry_trie())",
        "try_subtract(map.entry_trie())",
        "try_restrict(map.entry_trie())",
        "Err(EPathMapAlgebraError::ModeMismatch { .. })",
    ),
    "live_store_preserves_map_mode_and_key_value_associations_on_both_surfaces": (
        "map.trie_snapshot().starts_with",
        "protobuf_decoder::decode_epath_map(protobuf.as_slice())",
        "assert_eq!(decoded.trie_snapshot(), map.trie_snapshot())",
        "decoded.get_map_value",
        'decoded.get_map_value(&gstring("k1"))',
        'decoded.get_map_value(&gstring("k2"))',
        "bincode::deserialize(&generated_bincode)",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def digest(names):
    return hashlib.sha256("\n".join(names).encode()).hexdigest()


def listed(source, pattern, label):
    result = re.findall(pattern, source, flags=re.MULTILINE)
    require(len(result) == len(set(result)), f"duplicate {label}")
    return result


def test_body(source, name):
    matches = list(
        re.finditer(rf"(?m)^[ \t]*fn[ \t]+{re.escape(name)}[ \t]*\(", source)
    )
    require(len(matches) == 1, f"missing or duplicate normal-suite target {name}")
    attrs = []
    for line in reversed(source[: matches[0].start()].splitlines()):
        line = line.strip()
        if not line.startswith("#["):
            break
        attrs.append(line)
    require("#[test]" in attrs, f"non-test witness {name}")
    require(
        not any(
            attr.startswith(("#[ignore", "#[cfg(", "#[cfg_attr(")) for attr in attrs
        ),
        f"ignored or conditional witness {name}",
    )
    next_test = re.search(r"(?m)^#\[test\]", source[matches[0].end() :])
    end = matches[0].end() + next_test.start() if next_test else len(source)
    body = source[matches[0].start() : end]
    require(
        all(fragment in body for fragment in EVIDENCE[name]),
        f"concrete assertion missing from {name}",
    )


def source_paths():
    return [
        RUNNER,
        CARGO,
        ALGEBRA,
        SNAPSHOT,
        VERUS,
        CONFIG,
        MODEL,
        DELEGATE,
        DELEGATE_CONFIG,
        *(f"{ROCQ}/{name}" for name in ROCQ_SOURCES),
    ]


def extract(sources):
    runner = re.sub(r"\\\n", " ", sources[RUNNER])
    for path in (ALGEBRA, SNAPSHOT):
        stem = Path(path).stem
        require(
            len(
                re.findall(
                    rf"(?s)\[\[test\]\][^[]*name\s*=\s*\"{stem}\"[^[]*path\s*=\s*\"tests/{stem}\.rs\"",
                    sources[CARGO],
                )
            )
            == 1,
            f"{stem} absent from normal Cargo tests",
        )
    for path, name in sorted(set(DIRECT.values())):
        test_body(sources[path], name)

    rocq_rows = []
    for index, (
        file,
        (proof_count, proof_digest, print_count, print_digest),
    ) in enumerate(ROCQ_SOURCES.items(), 1):
        path = f"{ROCQ}/{file}"
        content = sources[path]
        proofs = listed(
            content,
            r"^(?:Theorem|Lemma|Corollary|Proposition|Fact|Remark)[ \t]+([A-Za-z_][A-Za-z_0-9]*)",
            f"{file} proof",
        )
        prints = listed(
            content,
            r"^Print Assumptions[ \t]+([A-Za-z_][A-Za-z_0-9]*)\.$",
            f"{file} Print Assumptions",
        )
        require(
            (len(proofs), digest(proofs), len(prints), digest(prints))
            == (proof_count, proof_digest, print_count, print_digest),
            f"{file} accepted declaration inventory changed",
        )
        require(
            set(prints) <= set(proofs), f"{file} assumption target lacks declaration"
        )
        require(
            not re.search(
                r"(?m)^[ \t]*(?:Admitted|Axiom|Parameter)\b|\badmit\.", content
            ),
            f"{file} contains an unproved declaration",
        )
        require(f"theories/{file}" in runner, f"{file} absent from normal proof runner")
        if print_count:
            require(
                re.search(rf"theories/{re.escape(file)}[ \t]+{print_count}\b", runner),
                f"{file} closed-count gate changed",
            )
        rocq_rows.append(
            {
                "source": path,
                "print_assumptions": prints,
                "obligations": [
                    {
                        "id": f"EPC-RC-{index:02d}-{position:02d}",
                        "symbol": symbol,
                        "printed": symbol in prints,
                        "mode": "concrete_witness"
                        if (file, symbol) in DIRECT
                        else "proof_only",
                        "test_source": DIRECT[(file, symbol)][0]
                        if (file, symbol) in DIRECT
                        else None,
                        "test_name": DIRECT[(file, symbol)][1]
                        if (file, symbol) in DIRECT
                        else None,
                    }
                    for position, symbol in enumerate(proofs, 1)
                ],
            }
        )

    verus = sources[VERUS]
    require(
        "verus! {" in verus and "fn main()" in verus,
        "Verus worklist module boundary changed",
    )
    declarations = re.findall(
        r"(?m)^(?:pub )?(?:(open spec|proof|spec) )?fn[ \t]+([A-Za-z_][A-Za-z_0-9]*)",
        verus.split("fn main()", 1)[0],
    )
    require(
        len(declarations) == len({name for _, name in declarations}),
        "duplicate Verus worklist obligation",
    )
    require(
        len(declarations) == VERUS_COUNT
        and digest([f"{kind or 'exec'}:{name}" for kind, name in declarations])
        == VERUS_DIGEST,
        "Verus worklist declaration inventory changed",
    )
    require(
        f'"$repo_root/{VERUS}"' in runner,
        "Verus worklist proof absent from normal runner",
    )

    specification = listed(
        sources[CONFIG],
        r"^SPECIFICATION[ \t]+([A-Za-z_][A-Za-z_0-9]*)$",
        "PDA specification",
    )
    invariants = listed(
        sources[CONFIG], r"^INVARIANT[ \t]+([A-Za-z_][A-Za-z_0-9]*)$", "PDA invariant"
    )
    require(
        specification == ["Spec"] and invariants == TLA_INVARIANTS,
        "PDA configured obligations changed",
    )
    require(
        len(re.findall(r"(?m)^INVARIANT\b", sources[CONFIG])) == len(invariants)
        and not re.search(
            r"(?m)^(?:PROPERTY|PROPERTIES|INVARIANTS)\b", sources[CONFIG]
        ),
        "PDA has an unparsed configured obligation",
    )
    for symbol in ["Spec", *invariants]:
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}[ \t]*==", sources[MODEL])) == 1,
            f"PDA model lacks unique definition {symbol}",
        )
    require(
        "PDAEquivalence.cfg PDAEquivalence.tla" in runner,
        "bounded PDA proof absent from normal runner",
    )

    delegated = json.loads(sources[DELEGATE])
    require(
        DELEGATE_CONFIG in delegated.get("source_sha256", {}),
        "EPM1 positional TLA registry no longer owns the config",
    )
    require(
        delegated["source_sha256"][DELEGATE_CONFIG]
        == hashlib.sha256(sources[DELEGATE_CONFIG].encode()).hexdigest(),
        "EPM1 positional TLA registry has stale config bytes",
    )
    delegated_invariants = [row["symbol"] for row in delegated["tla_invariants"]]
    cfg_invariants = listed(
        sources[DELEGATE_CONFIG],
        r"^INVARIANT[ \t]+([A-Za-z_][A-Za-z_0-9]*)$",
        "delegated EPM1 invariant",
    )
    require(delegated_invariants == cfg_invariants, "EPM1 positional registry is stale")
    require(
        'check-epm1-positional-obligations.py" --check' in runner,
        "delegated EPM1 gate absent from normal runner",
    )

    return {
        "scope": "EPathMap algebra, EPM1 core framing, and generic stack-safe PDA",
        "boundary": "Concrete witnesses exercise selected EPathMap operations and EPM1 map snapshot round trips. The generic PDA and Verus worklist equivalence obligations are proof-only; no direct executable refinement of arbitrary finite trees is claimed.",
        "source_sha256": {
            path: hashlib.sha256(sources[path].encode()).hexdigest()
            for path in source_paths()
            if path != RUNNER
        },
        "rocq": rocq_rows,
        "verus": {
            "source": VERUS,
            "obligations": [
                {
                    "id": f"EPC-VS-{index:02d}",
                    "symbol": name,
                    "kind": kind or "exec",
                    "mode": "proof_only",
                    "test_source": None,
                    "test_name": None,
                }
                for index, (kind, name) in enumerate(declarations, 1)
            ],
        },
        "tlc": {
            "config": CONFIG,
            "model": MODEL,
            "obligations": [
                {
                    "id": f"EPC-TLA-{index:02d}",
                    "symbol": symbol,
                    "mode": "proof_only",
                    "test_source": None,
                    "test_name": None,
                }
                for index, symbol in enumerate(invariants, 1)
            ],
            "causal_mutants": [],
        },
        "delegated_epm1_positional_tla": {
            "registry": DELEGATE,
            "config": DELEGATE_CONFIG,
            "invariants": delegated_invariants,
        },
        "executable_witnesses": [
            {"source": path, "test": name}
            for path, name in sorted(set(DIRECT.values()))
        ],
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

    rocq = f"{ROCQ}/EPM1.v"
    rejected(
        rocq,
        sources[rocq].replace(
            "Theorem epm1_framed_decode_encode_identity",
            "Theorem stale_epm1_framed_decode_encode_identity",
            1,
        ),
        "stale EPM1 theorem",
    )
    rejected(
        rocq,
        sources[rocq] + "\nTheorem epm1_framed_decode_encode_identity : True.\n",
        "duplicate EPM1 theorem",
    )
    rejected(
        rocq,
        sources[rocq].replace(
            "Print Assumptions zero_count_map_framed_round_trip.\n", "", 1
        ),
        "omitted EPM1 capstone",
    )
    rejected(
        VERUS,
        sources[VERUS].replace(
            "proof fn canonical_validation_worklist_equivalent",
            "proof fn stale_validation_worklist_equivalent",
            1,
        ),
        "stale Verus worklist proof",
    )
    rejected(
        CONFIG,
        sources[CONFIG].replace("INVARIANT CompletedEquivalent\n", "", 1),
        "omitted PDA invariant",
    )
    rejected(
        CONFIG,
        sources[CONFIG] + "\nINVARIANT CompletedEquivalent\n",
        "duplicate PDA invariant",
    )
    rejected(
        ALGEBRA,
        sources[ALGEBRA].replace(
            "fn set_algebra_uses_pathmap_lattice_operations(",
            "#[ignore]\nfn set_algebra_uses_pathmap_lattice_operations(",
            1,
        ),
        "ignored-only algebra witness",
    )
    rejected(
        ALGEBRA,
        sources[ALGEBRA].replace(
            "assert_eq!(set_values(&ab), set_values(&ba))",
            "assert_eq!(set_values(&ab), set_values(&ab))",
            1,
        ),
        "lost commutativity assertion",
    )
    require(extract(sources) == baseline, "nondeterministic EPathMap core registry")


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--write", action="store_true")
    group.add_argument("--check", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    sources = {path: (ROOT / path).read_text() for path in source_paths()}
    inventory = extract(sources)
    rendered = json.dumps(inventory, indent=2, sort_keys=True) + "\n"
    if args.write:
        (ROOT / OUTPUT).write_text(rendered)
    elif args.check:
        require(
            (ROOT / OUTPUT).read_text() == rendered,
            "generated EPathMap core registry is stale",
        )
    else:
        self_test(sources, inventory)
    print(
        f"EPathMap core registry {'mutations passed' if args.self_test else 'current'}: 89 Rocq, 5 Verus, 3 bounded PDA invariants"
    )


if __name__ == "__main__":
    try:
        main()
    except (RegistryError, KeyError, json.JSONDecodeError) as error:
        raise SystemExit(f"EPathMap core registry: {error}") from error
