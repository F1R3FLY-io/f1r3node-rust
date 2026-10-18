"""Check the exact RhoPath V2 outer grammar and lifecycle proof inventory."""

import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/rho_path_v2_obligations.generated.json"
ROCQ = "formal/rocq/stack_safe_pda/theories/RhoPathV2.v"
MODEL = "formal/tlaplus/stack_safe_pda/RhoPathV2Lifecycle.tla"
CONFIG = "formal/tlaplus/stack_safe_pda/RhoPathV2Lifecycle.cfg"
TEST = "rspace++/tests/rho_path_spec.rs"
CARGO = "rspace++/Cargo.toml"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
MUTANTS = {
    "DigestOnly": "DistinctCollisionRepresentativesPreserved",
    "Overwrite": "DistinctCollisionRepresentativesPreserved",
    "DecodeLegacy": "FullImageIdentityPreserved",
}

ROCQ_OBLIGATIONS = re.findall(
    r"\S+",
    """
parse_three_round_trip datum_empty_round_trip datum_empty_rejects_trailing_bytes
join_empty_round_trip continuation_zero_count_bytes_are_rejected
continuation_count_mismatch_is_rejected join_trailing_bytes_are_rejected
v1_v2_domains_are_prefix_disjoint datum_kind_is_distinct
continuation_count_is_eight_bytes empty_continuation_is_rejected
nonempty_continuation_has_explicit_count join_identity_is_framed
""",
)
PRINTS = [
    "datum_empty_round_trip",
    "datum_empty_rejects_trailing_bytes",
    "v1_v2_domains_are_prefix_disjoint",
    "continuation_count_is_eight_bytes",
]
INVARIANTS = re.findall(
    r"\S+",
    """
TypeInvariant FullImageCollisionSafety DistinctCollisionRepresentativesPreserved
AuthoritativeImageIsAdmitted SemanticRootMatchesImage
LaneMatchesImageAfterCommit NoPartialPublication ReplayedImageWasExported
""",
)
ACTIONS = re.findall(
    r"\S+",
    """
Init Produce Install Consume Commit Checkpoint Reset Rebuild Export Import Replay COMM
""",
)

# A concrete witness only asserts the corresponding outer-byte or parser boundary.
# The lifecycle's image/root/digest abstractions have no witness in rho_path_spec.
DIRECT = {
    "parse_three_round_trip": "v2_datum_round_trips_all_byte_values_in_each_field",
    "continuation_count_mismatch_is_rejected": "v2_continuation_rejects_count_mismatch_without_fallback",
    "v1_v2_domains_are_prefix_disjoint": "v1_and_v2_datum_namespaces_are_disjoint",
    "datum_kind_is_distinct": "v2_headers_and_escapes_fail_closed",
    "continuation_count_is_eight_bytes": "v2_continuation_has_u64_count_and_rejects_empty_groups",
    "empty_continuation_is_rejected": "v2_continuation_has_u64_count_and_rejects_empty_groups",
    "nonempty_continuation_has_explicit_count": "v2_continuation_has_u64_count_and_rejects_empty_groups",
    "join_identity_is_framed": "v2_join_frames_identity",
}
EVIDENCE = {
    "parse_three_round_trip": (
        "RhoPathV2::datum_key(&fields, &fields, &fields)",
        "RhoPathV2::parse_datum(&path).unwrap()",
        "assert_eq!(view.datum_identity(), fields)",
    ),
    "continuation_count_mismatch_is_rejected": (
        "RhoPathV2::parse_continuation(&overdeclared).is_err()",
        "RhoPathV2::parse_continuation(&underdeclared).is_err()",
    ),
    "v1_v2_domains_are_prefix_disjoint": (
        "RhoPathV1::parse_datum(&v2), Err(RhoPathError::InvalidMagic)",
        "RhoPathV2::parse_datum(&v1), Err(RhoPathError::InvalidMagic)",
    ),
    "datum_kind_is_distinct": (
        "wrong_kind[4] = RhoPathV2::JOIN_KIND",
        "Err(RhoPathError::InvalidKind(RhoPathV2::JOIN_KIND))",
    ),
    "continuation_count_is_eight_bytes": (
        "assert_eq!(&path[6..14], &2_u64.to_be_bytes())",
    ),
    "empty_continuation_is_rejected": ("Err(RhoPathError::EmptyChannelGroup)",),
    "nonempty_continuation_has_explicit_count": (
        'RhoPathV2::continuation_key(&[b"a", b"b"], b"id").unwrap()',
        "RhoPathV2::parse_continuation(&path).unwrap()",
    ),
    "join_identity_is_framed": (
        'RhoPathV2::join_key(b"channel", b"join\\0identity")',
        "RhoPathV2::parse_join(&path).unwrap()",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def names(source, pattern, label):
    found = re.findall(pattern, source, flags=re.MULTILINE)
    require(len(found) == len(set(found)), f"duplicate {label}")
    return found


def test_target(source, name):
    matches = list(
        re.finditer(rf"(?m)^[ \t]*fn[ \t]+{re.escape(name)}[ \t]*\(", source)
    )
    require(len(matches) == 1, f"missing or duplicate normal-suite test {name}")
    attrs = []
    for line in reversed(source[: matches[0].start()].splitlines()):
        line = line.strip()
        if not line.startswith("#["):
            break
        attrs.append(line)
    require("#[test]" in attrs, f"non-test executable target {name}")
    require(
        not any(a.startswith("#[ignore") for a in attrs), f"ignored-only test {name}"
    )
    require(
        not any(a.startswith(("#[cfg(", "#[cfg_attr(")) for a in attrs),
        f"conditionally absent test {name}",
    )


def test_body(source, name):
    match = re.search(rf"(?m)^[ \t]*fn[ \t]+{re.escape(name)}[ \t]*\(", source)
    require(match is not None, f"missing executable witness {name}")
    next_test = re.search(r"(?m)^#\[test\]", source[match.end() :])
    end = match.end() + next_test.start() if next_test else len(source)
    return source[match.start() : end]


def paths():
    result = [ROCQ, MODEL, CONFIG, TEST, CARGO, RUNNER]
    for stem in MUTANTS:
        base = f"formal/tlaplus/stack_safe_pda/RhoPathV2Lifecycle{stem}Mutant"
        result.extend((f"{base}.tla", f"{base}.cfg"))
    return result


def extract(sources):
    rocq = sources[ROCQ]
    model = sources[MODEL]
    config = sources[CONFIG]
    tests = sources[TEST]
    runner = re.sub(r"\\\n", " ", sources[RUNNER])

    proofs = names(
        rocq,
        r"^(?:Lemma|Theorem|Corollary|Proposition|Fact|Remark)[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*:",
        "RhoPathV2.v proof declaration",
    )
    require(
        proofs == ROCQ_OBLIGATIONS, "missing, added, or reordered RhoPathV2.v proof"
    )
    printed = names(
        rocq,
        r"^Print Assumptions[ \t]+([A-Za-z_][A-Za-z_0-9]*)\.$",
        "RhoPathV2.v closed-assumption target",
    )
    require(printed == PRINTS, "missing, added, or reordered closed-assumption target")
    require(
        not re.search(r"\b(?:Admitted|admit|Axiom|Parameter)\b", rocq),
        "RhoPathV2.v contains an admitted or axiomatic declaration",
    )
    require(
        "rocq_compile_with_closed_count" in runner
        and re.search(r"theories/RhoPathV2\.v[ \t]+4[ \t]", runner),
        "RhoPathV2.v four-closure compilation absent from normal proof runner",
    )

    specification = names(
        config, r"^SPECIFICATION[ \t]+([A-Za-z_][A-Za-z_0-9]*)$", "TLC specification"
    )
    invariants = names(
        config, r"^INVARIANT[ \t]+([A-Za-z_][A-Za-z_0-9]*)$", "TLC invariant"
    )
    require(
        specification == ["Spec"] and invariants == INVARIANTS,
        "missing, added, or reordered lifecycle TLC obligation",
    )
    require(
        len(re.findall(r"(?m)^INVARIANT\b", config)) == len(invariants)
        and len(re.findall(r"(?m)^SPECIFICATION\b", config)) == 1,
        "unparsed lifecycle TLC directive",
    )
    require(
        not re.search(r"(?m)^(?:PROPERTY|PROPERTIES|INVARIANTS)\b", config),
        "unsupported lifecycle TLC obligation directive",
    )
    require(
        re.search(r"RhoPathV2Lifecycle\.cfg[ \t]+RhoPathV2Lifecycle\.tla", runner),
        "lifecycle TLC baseline absent from normal proof runner",
    )
    for symbol in ["Spec", *INVARIANTS, *ACTIONS]:
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}(?:\([^\n]*\))?[ \t]*==", model))
            == 1,
            f"missing or duplicate lifecycle definition {symbol}",
        )
    require(
        names(
            model,
            r"^THEOREM[ \t]+Spec[ \t]*=>[ \t]*\[\]([A-Za-z_][A-Za-z_0-9]*)$",
            "lifecycle temporal assertion",
        )
        == INVARIANTS,
        "TLC invariants diverge from lifecycle temporal assertions",
    )
    for action in ACTIONS[1:]:
        require(
            re.search(
                rf"\b{re.escape(action)}(?:\(nextLane\))?\b",
                model[model.index("Next ==") : model.index("Spec ==")],
            ),
            f"lifecycle Next omits {action}",
        )

    mutation_rows = []
    for index, (stem, expected) in enumerate(MUTANTS.items(), 1):
        base = f"formal/tlaplus/stack_safe_pda/RhoPathV2Lifecycle{stem}Mutant"
        cfg, tla = f"{base}.cfg", f"{base}.tla"
        mutant_cfg = sources[cfg]
        mutant_model = sources[tla]
        require(
            names(
                mutant_cfg,
                r"^SPECIFICATION[ \t]+([A-Za-z_][A-Za-z_0-9]*)$",
                "mutant specification",
            )
            == ["Spec"]
            and names(
                mutant_cfg,
                r"^INVARIANT[ \t]+([A-Za-z_][A-Za-z_0-9]*)$",
                "mutant invariant",
            )
            == [expected],
            f"{stem} mutant config changed expected invariant",
        )
        require(
            not re.search(r"(?m)^(?:PROPERTY|PROPERTIES|INVARIANTS)\b", mutant_cfg),
            f"{stem} mutant has unsupported TLC obligation directive",
        )
        require(
            len(re.findall(rf"(?m)^{re.escape(expected)}[ \t]*==", mutant_model)) == 1,
            f"{stem} mutant lacks expected invariant definition",
        )
        if stem == "DigestOnly":
            require(
                "RhoPathV2LifecycleDigestOnlyMutant.cfg" in runner
                and "RhoPathV2LifecycleDigestOnlyMutant.tla" in runner
                and f"Invariant {expected} is violated" in runner,
                "DigestOnly causal mutant failure gate absent from normal proof runner",
            )
        else:
            require(
                f"RhoPathV2Lifecycle{stem}Mutant.cfg:RhoPathV2Lifecycle{stem}Mutant.tla:{expected}"
                in runner,
                f"{stem} causal mutant failure gate absent from normal proof runner",
            )
        mutation_rows.append(
            {
                "id": f"RPV2-MUT-{index:02d}",
                "config": cfg,
                "model": tla,
                "expected_invariant_failure": expected,
            }
        )

    require(
        not re.search(r"(?m)^autotests[ \t]*=[ \t]*false\b", sources[CARGO]),
        "rho_path_spec absent from Cargo's normal test discovery",
    )
    test_names = names(
        tests,
        r"^[ \t]*fn[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*\(",
        "rho_path_spec test name",
    )
    require(len(test_names) == 24, "rho_path_spec normal-suite count changed")
    for name in test_names:
        test_target(tests, name)
    require(
        set(DIRECT.values()) <= set(test_names),
        "missing direct RhoPath V2 executable witness",
    )
    require(set(EVIDENCE) == set(DIRECT), "unbound RhoPath V2 witness evidence")
    for obligation, target in DIRECT.items():
        body = test_body(tests, target)
        require(
            all(fragment in body for fragment in EVIDENCE[obligation]),
            f"{obligation} no longer has its concrete assertion in {target}",
        )
    require(
        re.search(
            r"rspace_preparation_run_tests[ \t]+24[ \t]+--test[ \t]+rho_path_spec",
            runner,
        ),
        "24-test rho_path_spec gate absent from normal proof runner",
    )
    require(
        'python3 "$repo_root/scripts/check-rho-path-v2-obligations.py" --check'
        in runner
        and 'python3 "$repo_root/scripts/check-rho-path-v2-obligations.py" --self-test'
        in runner,
        "RhoPath V2 inventory gate absent from normal proof runner",
    )

    return {
        "scope": "RhoPath V2 outer byte grammar and abstract two-lane lifecycle",
        "boundary": "Rust witnesses exercise selected outer codec behaviors, not the abstract lifecycle, RCI2/RMP2 payload validity, or a formal Rust refinement proof.",
        "sources_sha256": {
            path: hashlib.sha256(sources[path].encode()).hexdigest()
            for path in paths()
            if path != RUNNER
        },
        "rocq": {
            "source": ROCQ,
            "print_assumptions": printed,
            "obligations": [
                {
                    "id": f"RPV2-RC-{index:02d}",
                    "obligation": name,
                    "mode": "concrete_witness" if name in DIRECT else "proof_only",
                    "test_target": DIRECT.get(name),
                }
                for index, name in enumerate(proofs, 1)
            ],
        },
        "tlc": {
            "model": MODEL,
            "config": CONFIG,
            "actions": ACTIONS,
            "obligations": [
                {
                    "id": f"RPV2-TLA-{index:02d}",
                    "obligation": name,
                    "mode": "proof_only",
                    "test_target": None,
                }
                for index, name in enumerate(invariants, 1)
            ],
            "causal_mutants": mutation_rows,
        },
        "normal_suite": {
            "source": TEST,
            "tests": test_names,
            "direct_witnesses": sorted(set(DIRECT.values())),
        },
        "excluded_families": ["RCI2 ChannelImage.v", "RMP2 RMP2.v"],
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
            "Theorem datum_empty_round_trip :",
            "Theorem stale_datum_empty_round_trip :",
            1,
        ),
        "stale Rocq theorem",
    )
    rejected(
        ROCQ,
        sources[ROCQ] + "\nTheorem datum_empty_round_trip : True.\n",
        "duplicate Rocq theorem",
    )
    rejected(
        ROCQ,
        sources[ROCQ].replace(
            "Print Assumptions continuation_count_is_eight_bytes.\n", "", 1
        ),
        "omitted Rocq closure",
    )
    rejected(
        CONFIG,
        sources[CONFIG].replace("INVARIANT TypeInvariant\n", "", 1),
        "omitted TLC invariant",
    )
    rejected(
        CONFIG,
        sources[CONFIG] + "\nINVARIANT TypeInvariant\n",
        "duplicate TLC invariant",
    )
    rejected(
        TEST,
        sources[TEST].replace(
            "fn v2_join_frames_identity(", "#[ignore]\nfn v2_join_frames_identity(", 1
        ),
        "ignored-only Rust witness",
    )
    rejected(
        TEST,
        sources[TEST].replace(
            "fn v2_join_frames_identity(", "fn stale_join_frames_identity(", 1
        ),
        "stale Rust witness",
    )
    rejected(
        TEST,
        sources[TEST].replace("&2_u64.to_be_bytes()", "&2_u32.to_be_bytes()", 1),
        "missing concrete count-width assertion",
    )
    mutant = "formal/tlaplus/stack_safe_pda/RhoPathV2LifecycleDecodeLegacyMutant.cfg"
    rejected(
        mutant,
        sources[mutant].replace("FullImageIdentityPreserved", "TypeInvariant", 1),
        "wrong causal mutant invariant",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            'python3 "$repo_root/scripts/check-rho-path-v2-obligations.py" --check',
            "",
            1,
        ),
        "missing normal proof gate",
    )
    require(extract(sources) == baseline, "nondeterministic RhoPath V2 registry")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    sources = {path: (ROOT / path).read_text() for path in paths()}
    inventory = extract(sources)
    rendered = json.dumps(inventory, indent=2, sort_keys=True) + "\n"
    if args.write:
        (ROOT / OUTPUT).write_text(rendered)
    elif args.check:
        require(
            (ROOT / OUTPUT).read_text() == rendered,
            "generated RhoPath V2 inventory is stale",
        )
    else:
        self_test(sources, inventory)
    print(
        f"RhoPath V2 registry {'mutations passed' if args.self_test else 'current'}: {len(ROCQ_OBLIGATIONS)} Rocq, {len(INVARIANTS)} TLC, {len(MUTANTS)} mutants, 24 normal-suite tests"
    )


if __name__ == "__main__":
    try:
        main()
    except (RegistryError, KeyError, json.JSONDecodeError) as error:
        raise SystemExit(f"RhoPath V2 registry: {error}") from error
