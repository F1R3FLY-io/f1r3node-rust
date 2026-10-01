import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/epm1_positional_obligations.generated.json"
CONFIG = "formal/tlaplus/stack_safe_pda/Epm1PositionalAssociation.cfg"
MODEL = "formal/tlaplus/stack_safe_pda/Epm1PositionalAssociation.tla"
ROCQ = "formal/rocq/actree03_completeness/theories/Epm1PositionalAssociationV6.v"
ROCQ_POLICY = "formal/rocq/actree03_completeness/theories/Epm1CursorAuthorityV2.v"
VERUS = "formal/verus/epm1_positional_association.rs"
MANIFEST = "models/tests/formal_equivalence_manifest.rs"
PRODUCTION = "models/src/rust/epathmap_trie_codec.rs"
MODULE = "models/src/rust/mod.rs"
CARGO = "models/Cargo.toml"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
POLICY_CAPSTONE = "campaign_policy_authorizes_amending_unreleased_epm1"
TLA_TARGETS = {
    "TypeInvariant": None,
    "MarkersAreZero": "epm1_map_layout_writes_zero_marker_for_every_value_occurrence",
    "LogicalOccurrencesIgnoreStorageIdentity": "positional_occurrences_ignore_shared_cow_par_addresses",
    "AssignmentPrefixIsExact": "positional_replay_preserves_root_prefix_dense_and_topology_only_nodes",
    "SuspendedFocusIsExact": None,
    "ChildErrorPrecedesDeferredAct": "child_errors_precede_deferred_act_and_noncanonical_body_errors",
    "FailureNeverPublishes": "pending_decoder_publishes_only_from_successful_finish_without_partial_result",
    "PublicationRequiresCompleteLockstep": "pending_decoder_publishes_only_from_successful_finish_without_partial_result",
    "PublicationIsExactlyOnce": None,
}
VERUS_OBLIGATIONS = {
    "positional_act_sentinel": "open spec",
    "sentinel_is_storage_identity_independent": "proof",
    "checked_next_position": "verified_exec",
    "pair_one_occurrence": "verified_exec",
    "checked_platform_count": "verified_exec",
    "publication_authorized": "verified_exec",
}
MUTANTS = {
    "Epm1PositionalSequentialMarkerMutant.cfg": "MarkersAreZero",
    "Epm1PositionalNonzeroMarkerMutant.cfg": "MarkersAreZero",
    "Epm1PositionalRootOmissionMutant.cfg": "AssignmentPrefixIsExact",
    "Epm1PositionalAddressCollapseMutant.cfg": "AssignmentPrefixIsExact",
    "Epm1PositionalSkipEndpointMutant.cfg": "AssignmentPrefixIsExact",
    "Epm1PositionalWrongEndpointMutant.cfg": "AssignmentPrefixIsExact",
    "Epm1PositionalDuplicateEndpointMutant.cfg": "AssignmentPrefixIsExact",
    "Epm1PositionalMissingBodyMutant.cfg": "AssignmentPrefixIsExact",
    "Epm1PositionalExtraBodyMutant.cfg": "AssignmentPrefixIsExact",
    "Epm1PositionalEarlyPublishMutant.cfg": "PublicationRequiresCompleteLockstep",
    "Epm1PositionalDoublePublishMutant.cfg": "PublicationIsExactlyOnce",
    "Epm1PositionalPrecedenceMutant.cfg": "ChildErrorPrecedesDeferredAct",
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def named_lines(source, pattern, label):
    found = re.findall(pattern, source, re.MULTILINE)
    require(found, f"missing {label}")
    require(len(found) == len(set(found)), f"duplicate {label}")
    return found


def test_target(source, name):
    matches = list(
        re.finditer(rf"(?m)^[ \t]*fn[ \t]+{re.escape(name)}[ \t]*\(", source)
    )
    require(len(matches) == 1, f"missing or duplicate normal-suite test: {name}")
    attrs = []
    for line in reversed(source[: matches[0].start()].splitlines()):
        line = line.strip()
        if not line.startswith("#["):
            break
        attrs.append(line)
    require("#[test]" in attrs, f"non-test executable target: {name}")
    require(
        not any(attr.startswith("#[ignore") for attr in attrs),
        f"ignored-only target: {name}",
    )
    require(
        not any(attr.startswith(("#[cfg(", "#[cfg_attr(")) for attr in attrs),
        f"conditionally absent target: {name}",
    )


def manifest_bindings(source):
    match = re.search(
        r"(?s)const EPM1_POSITIONAL_CORRESPONDENCE:.*?= &\[(.*?)\n\];", source
    )
    require(match is not None, "missing EPM1 positional correspondence array")
    pattern = re.compile(r"(?s)Epm1PositionalCorrespondence\s*\{(.*?)\}")
    blocks = list(pattern.finditer(match.group(1)))
    require(
        not pattern.sub("", match.group(1)).replace(",", "").strip(),
        "unparsed EPM1 positional correspondence row",
    )
    rows = []
    for block in blocks:
        row = {}
        for key in ("id", "theorem", "production_marker", "executable_marker"):
            field = re.search(rf'\b{key}:\s*"([^"]+)"', block.group(1))
            require(
                field is not None, f"missing {key} in EPM1 positional correspondence"
            )
            row[key] = field.group(1)
        rows.append(row)
    require(
        [row["id"] for row in rows] == [f"EPM1-POS-{i:03d}" for i in range(1, 8)],
        "missing or duplicate EPM1 positional binding ID",
    )
    return rows


def extract(sources, mutant_names):
    config = sources[CONFIG]
    model = sources[MODEL]
    rocq = sources[ROCQ]
    runner = re.sub(r"\\\n", " ", sources[RUNNER])
    specifications = named_lines(
        config, r"^SPECIFICATION[ \t]+([A-Za-z][A-Za-z0-9_]*)$", "TLC specification"
    )
    invariants = named_lines(
        config, r"^INVARIANT[ \t]+([A-Za-z][A-Za-z0-9_]*)$", "TLC invariant"
    )
    require(specifications == ["Spec"], "unexpected TLC specification")
    require(
        len(re.findall(r"(?m)^INVARIANT\b", config)) == len(invariants)
        and len(re.findall(r"(?m)^SPECIFICATION\b", config)) == 1
        and not re.search(r"(?m)^PROPERT(?:Y|IES)\b|^INVARIANTS\b", config),
        "unsupported or malformed TLC obligation directive",
    )
    require(list(TLA_TARGETS) == invariants, "missing, extra, or reordered TLC binding")
    require(
        len(re.findall(r"(?m)^Spec[ \t]*==", model)) == 1,
        "missing or duplicate TLC Spec",
    )
    for symbol in invariants:
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}[ \t]*==", model)) == 1,
            f"missing or duplicate TLC definition: {symbol}",
        )
    require(
        re.search(
            r"Epm1PositionalAssociation\.cfg\s+Epm1PositionalAssociation\.tla", runner
        )
        is not None,
        "baseline TLC model absent from proof runner",
    )
    require(
        "pub mod epathmap_trie_codec;" in sources[MODULE],
        "EPM1 production test module unregistered",
    )
    require(
        'name = "formal_equivalence_manifest"' in sources[CARGO]
        and 'path = "tests/formal_equivalence_manifest.rs"' in sources[CARGO],
        "Rocq correspondence test absent from normal models suite",
    )
    test_target(
        sources[MANIFEST],
        "every_epm1_positional_obligation_has_a_mechanical_executable_binding",
    )
    tla = []
    for position, symbol in enumerate(invariants, start=1):
        target = TLA_TARGETS[symbol]
        if target is not None:
            test_target(sources[PRODUCTION], target)
        tla.append(
            {
                "id": f"EPM1-TLA-{position:02d}",
                "symbol": symbol,
                "classification": "proof_only"
                if target is None
                else "executable_refinement",
                "target": target,
                "target_source": PRODUCTION if target is not None else None,
            }
        )
    prints = named_lines(
        rocq,
        r"^Print Assumptions[ \t]+([A-Za-z][A-Za-z0-9_]*)\.$",
        "Rocq assumption-print target",
    )
    rows = manifest_bindings(sources[MANIFEST])
    expected_prints = [row["theorem"] for row in rows]
    expected_prints.insert(6, POLICY_CAPSTONE)
    require(
        prints == expected_prints,
        "missing, extra, or reordered Rocq capstone binding",
    )
    require(
        len(re.findall(rf"(?m)^Theorem {POLICY_CAPSTONE}[ \t]*:", sources[ROCQ_POLICY]))
        == 1,
        "missing imported Rocq policy capstone",
    )
    rocq_rows = []
    for row in rows:
        theorem = row["theorem"]
        require(
            len(
                re.findall(
                    rf"(?m)^(?:Theorem|Corollary) {re.escape(theorem)}[ \t]*:", rocq
                )
            )
            == 1,
            f"missing or duplicate Rocq capstone: {theorem}",
        )
        require(
            row["production_marker"] in sources[PRODUCTION],
            f"stale production marker: {row['id']}",
        )
        test_target(sources[PRODUCTION], row["executable_marker"])
        rocq_rows.append(
            {
                "id": row["id"],
                "symbol": theorem,
                "classification": "executable_refinement",
                "target": row["executable_marker"],
                "target_source": PRODUCTION,
            }
        )
    rocq_rows.insert(
        6,
        {
            "id": "EPM1-POS-POLICY",
            "symbol": POLICY_CAPSTONE,
            "classification": "proof_only",
            "target": None,
            "target_source": None,
            "theorem_source": ROCQ_POLICY,
        },
    )
    require(
        "theories/Epm1PositionalAssociationV6.v" in runner,
        "Rocq capstone absent from proof runner",
    )
    verus = []
    declarations = re.findall(
        r"(?m)^pub[ \t]+(?:(open spec|proof)[ \t]+)?fn[ \t]+([A-Za-z][A-Za-z0-9_]*)[ \t]*\(",
        sources[VERUS],
    )
    observed = {name: kind or "verified_exec" for kind, name in declarations}
    require(len(observed) == len(declarations), "duplicate Verus obligation")
    require(
        list(observed.items()) == list(VERUS_OBLIGATIONS.items()),
        "missing, extra, reordered, or stale Verus obligation",
    )
    for position, (name, kind) in enumerate(observed.items(), start=1):
        verus.append(
            {
                "id": f"EPM1-VERUS-{position:02d}",
                "symbol": name,
                "kind": kind,
                "classification": "proof_only",
            }
        )
    require(
        "formal/verus/epm1_positional_association.rs" in runner,
        "Verus source absent from proof runner",
    )
    require(
        mutant_names == set(MUTANTS), "missing or unregistered EPM1 positional mutant"
    )
    mutants = []
    for position, (name, invariant) in enumerate(MUTANTS.items(), start=1):
        path = f"formal/tlaplus/stack_safe_pda/{name}"
        mutant = sources[path]
        specs = named_lines(
            mutant,
            r"^SPECIFICATION[ \t]+([A-Za-z][A-Za-z0-9_]*)$",
            f"{name} specification",
        )
        checks = named_lines(
            mutant, r"^INVARIANT[ \t]+([A-Za-z][A-Za-z0-9_]*)$", f"{name} invariant"
        )
        require(
            len(specs) == 1
            and checks == [invariant]
            and len(re.findall(r"(?m)^SPECIFICATION\b", mutant)) == 1
            and len(re.findall(r"(?m)^INVARIANT\b", mutant)) == 1
            and not re.search(r"(?m)^PROPERT(?:Y|IES)\b|^INVARIANTS\b", mutant),
            f"stale mutant obligation: {name}",
        )
        require(
            len(re.findall(rf"(?m)^{re.escape(specs[0])}[ \t]*==", model)) == 1,
            f"missing mutant specification: {name}",
        )
        require(
            f"{name}:{invariant}" in runner,
            f"mutant kill mapping absent from proof runner: {name}",
        )
        mutants.append(
            {
                "id": f"EPM1-MUT-{position:02d}",
                "config": path,
                "specification": specs[0],
                "expected_invariant": invariant,
            }
        )
    require(
        "Invariant ${epm1_positional_invariant} is violated" in runner,
        "mutant semantic-failure check absent from proof runner",
    )
    for mode in ("--check", "--self-test"):
        require(
            f'check-epm1-positional-obligations.py" {mode}' in runner,
            f"EPM1 registry {mode} absent from proof runner",
        )
    return {
        "tla_invariants": tla,
        "rocq_print_assumptions": rocq_rows,
        "verus_obligations": verus,
        "causal_mutants": mutants,
        "scope_limit": "Direct executable rows bind named EPM1 normal-suite tests; proof-only rows have no claimed Rust refinement. Source correspondence does not substitute for TLC, Rocq, Verus, or Cargo execution.",
        "source_sha256": {
            path: hashlib.sha256(source.encode("utf-8")).hexdigest()
            for path, source in sorted(sources.items())
            if path not in (RUNNER, CARGO)
        },
    }


def reject(mutated, mutants, expected):
    try:
        extract(mutated, mutants)
    except RegistryError as error:
        require(expected in str(error), f"wrong mutation rejection: {error}")
        return
    raise RegistryError(f"mutation unexpectedly passed: {expected}")


def self_test(sources, mutants):
    changed = dict(sources)
    changed[CONFIG] = sources[CONFIG].replace("INVARIANT MarkersAreZero\n", "", 1)
    reject(changed, mutants, "missing, extra, or reordered TLC binding")
    changed = dict(sources)
    changed[CONFIG] = sources[CONFIG] + "INVARIANT MarkersAreZero\n"
    reject(changed, mutants, "duplicate TLC invariant")
    changed = dict(sources)
    changed[MODEL] = sources[MODEL].replace("MarkersAreZero ==", "RemovedMarkers ==", 1)
    reject(changed, mutants, "missing or duplicate TLC definition")
    changed = dict(sources)
    changed[PRODUCTION] = sources[PRODUCTION].replace(
        "#[test]\n    fn epm1_map_layout_writes_zero_marker_for_every_value_occurrence",
        "#[test]\n    #[ignore]\n    fn epm1_map_layout_writes_zero_marker_for_every_value_occurrence",
        1,
    )
    reject(changed, mutants, "ignored-only target")
    changed = dict(sources)
    changed[MANIFEST] = sources[MANIFEST].replace(
        'id: "EPM1-POS-001"', 'id: "EPM1-POS-002"', 1
    )
    reject(changed, mutants, "missing or duplicate EPM1 positional binding ID")
    changed = dict(sources)
    changed[MANIFEST] = sources[MANIFEST].replace(
        'executable_marker: "epm1_map_layout_writes_zero_marker_for_every_value_occurrence"',
        'executable_marker: "missing_executable_property"',
        1,
    )
    reject(changed, mutants, "missing or duplicate normal-suite test")
    changed = dict(sources)
    changed[ROCQ] = sources[ROCQ].replace(
        "Print Assumptions positional_error_precedence_is_total.\n", "", 1
    )
    reject(changed, mutants, "missing, extra, or reordered Rocq capstone binding")
    changed = dict(sources)
    changed[VERUS] = sources[VERUS].replace(
        "pub fn checked_next_position(", "pub fn missing_next_position(", 1
    )
    reject(changed, mutants, "missing, extra, reordered, or stale Verus obligation")
    changed = dict(sources)
    path = "formal/tlaplus/stack_safe_pda/Epm1PositionalPrecedenceMutant.cfg"
    changed[path] = sources[path].replace(
        "INVARIANT ChildErrorPrecedesDeferredAct", "INVARIANT MarkersAreZero", 1
    )
    reject(changed, mutants, "stale mutant obligation")
    changed = dict(sources)
    reject(
        changed,
        mutants - {"Epm1PositionalPrecedenceMutant.cfg"},
        "missing or unregistered",
    )


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    paths = {
        CONFIG,
        MODEL,
        ROCQ,
        ROCQ_POLICY,
        VERUS,
        MANIFEST,
        PRODUCTION,
        MODULE,
        CARGO,
        RUNNER,
        *(f"formal/tlaplus/stack_safe_pda/{name}" for name in MUTANTS),
    }
    sources = {
        path: (ROOT / path).read_text(encoding="utf-8") for path in sorted(paths)
    }
    mutants = {
        path.name
        for path in (ROOT / "formal/tlaplus/stack_safe_pda").glob(
            "Epm1Positional*Mutant.cfg"
        )
    }
    payload = json.dumps(extract(sources, mutants), indent=2, sort_keys=True) + "\n"
    if args.self_test:
        self_test(sources, mutants)
        print("EPM1 positional registry: mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload, encoding="utf-8")
        print("EPM1 positional registry: written")
    else:
        require(
            (ROOT / OUTPUT).read_text(encoding="utf-8") == payload,
            "EPM1 positional registry is stale",
        )
        print("EPM1 positional registry: current")


if __name__ == "__main__":
    main()
