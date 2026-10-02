import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/parbag_egress_obligations.generated.json"
TLA = "formal/tlaplus/stack_safe_pda"
ROCQ = "formal/rocq/stack_safe_pda/theories"
VERUS = "formal/verus/rspace_canonical_egress.rs"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
CARGO = "models/Cargo.toml"
PREPARED_TEST = "models/tests/par_protobuf_stack_safety.rs"
SLOT_TEST = "models/tests/protobuf_encoder_space.rs"
PREPARED_NAME = "par_message_round_trip_is_stack_safe_and_unbounded_by_decode_context"
SLOT_NAME = (
    "par_slot_visitor_uses_generated_sequence_and_map_order_without_descending_into_par"
)

ROCQ_SOURCES = {
    "ProtobufParAccumulation.v": (
        46,
        "e5899ed2801ef8694718091b0367518c53b85d5fce6155486ebee9682ddb74a9",
        41,
        "39c88ccfc9123fda83ec741e39f0186bb4a2040fc9aaf388f560e64186012fe4",
    ),
    "ProtobufOwnedSlots.v": (
        53,
        "295c42c919acf85e67d14d96039bd7804656b8c10e28bdd06d5a3ce435e6265c",
        51,
        "f43955a730964585ad23f301b71f9d3be6733dd1c272b3eaa655429e6c4db4aa",
    ),
    "ProtobufDecoderComposition.v": (
        26,
        "0be97982bcf8a696a697138934f92b0cd8dde50623c7eda5f01676b6cdf268e8",
        26,
        "0be97982bcf8a696a697138934f92b0cd8dde50623c7eda5f01676b6cdf268e8",
    ),
}
VERUS_COUNT = 104
VERUS_DIGEST = "63e7a770e4e96daed200166fbebd0edd49c18adb16bb1cbb2f40832a708d0f96"

FAMILIES = {
    "ParBagProjectionLifecycle": {
        "invariants": re.findall(
            r"\S+",
            "TypeInvariant BuildingIsInvisible ReadyImageIsComplete RefusalOnlyWhenTooLarge WriterCacheMatchesIdentity CloneCacheMatchesIdentity UntouchedCloneSurvivesWriterMutation EveryReadIsComplete",
        ),
        "mutant_prefix": "ParBagProjection",
        "mutants": {
            "Cap": "RefusalOnlyWhenTooLarge",
            "Partial": "ReadyImageIsComplete",
            "Stale": "WriterCacheMatchesIdentity",
            "Read": "EveryReadIsComplete",
            "CloneLoss": "UntouchedCloneSurvivesWriterMutation",
        },
    },
    "ParBagProtobufSinkPublication": {
        "invariants": re.findall(
            r"\S+",
            "TypeInvariant InvalidInputHasNoPrefix PublicImageAtomic FailedSinkIsNotPublished CommitRequiresCompleteImage AbortedPrivateFrameIsDiscarded",
        ),
        "mutant_prefix": "ParBagProtobufSink",
        "mutants": {
            "Public": "PublicImageAtomic",
            "Preflight": "InvalidInputHasNoPrefix",
            "PrematureCommit": "CommitRequiresCompleteImage",
            "Rollback": "AbortedPrivateFrameIsDiscarded",
        },
    },
    "ParBagInFrameMemo": {
        "invariants": re.findall(
            r"\S+",
            "TypeInvariant CensusPrefixIsExact CensusCompletesBeforeEmission ReplayConsumesExactOccurrencePrefix ReplayExhaustionIffEmissionComplete DirectLaneForEmptyAndSingleton MemoOnlyForCensusProvenRepeatedOwners OutputMatchesDirect CommitRequiresExactReplay MemoIsOwnerExact",
        ),
        "mutant_prefix": "ParBagInFrameMemo",
        "mutants": {
            "CensusSkip": "CensusPrefixIsExact",
            "ReplaySwap": "ReplayConsumesExactOccurrencePrefix",
            "ReplayOverrun": "ReplayConsumesExactOccurrencePrefix",
            "ReplayRemainingCommit": "CommitRequiresExactReplay",
            "EmptyMemo": "MemoOnlyForCensusProvenRepeatedOwners",
            "SingletonMemo": "MemoOnlyForCensusProvenRepeatedOwners",
            "EarlyEmission": "CensusCompletesBeforeEmission",
            "Swap": "OutputMatchesDirect",
            "Whole": "OutputMatchesDirect",
            "WrongOwner": "OutputMatchesDirect",
            "OffByOne": "OutputMatchesDirect",
        },
    },
    "RSpaceCanonicalEgress": {
        "invariants": re.findall(
            r"\S+",
            "TypeInvariant PreflightCompletesBeforeBuild PrivateImageCompletesBeforePublish VisibleImageIsAtomic MalformedInputNeverPublishes RejectedInputIsNonMutating ExactlyOnePublication",
        ),
        "mutant_prefix": "RSpaceCanonicalEgress",
        "mutants": {
            "": "VisibleImageIsAtomic",
            "Header": "PrivateImageCompletesBeforePublish",
            "Validation": "PreflightCompletesBeforeBuild",
        },
    },
}
DIRECT = {
    ("rocq", "ProtobufOwnedSlots.v", "method_child_order_commutes"): (
        SLOT_TEST,
        SLOT_NAME,
    ),
    ("rocq", "ProtobufOwnedSlots.v", "method_child_order_preserves_cardinality"): (
        SLOT_TEST,
        SLOT_NAME,
    ),
    ("tlc", "ParBagProjectionLifecycle", "ReadyImageIsComplete"): (
        PREPARED_TEST,
        PREPARED_NAME,
    ),
    ("tlc", "ParBagProjectionLifecycle", "EveryReadIsComplete"): (
        PREPARED_TEST,
        PREPARED_NAME,
    ),
    ("verus", VERUS, "prepared_projection_equals_direct"): (
        PREPARED_TEST,
        PREPARED_NAME,
    ),
}
EVIDENCE = {
    PREPARED_NAME: (
        "let message_bytes = term.protobuf_bytes()",
        "term.prepare_protobuf_projection(component_bytes)",
        "assert_eq!(message_bytes, pda_bytes)",
        "assert_eq!(term.encode_to_vec(), message_bytes",
        "assert_eq!(protobuf_encoder::encode_to_vec(&term), message_bytes",
    ),
    SLOT_NAME: (
        "try_visit_par_slots(&list",
        "assert_eq!(sequence_slots.len(), 2)",
        "assert_eq!(sequence_slots[0], &list.ps[0] as *const Par)",
        "assert_eq!(sequence_slots[1], &list.ps[1] as *const Par)",
        "assert_eq!(map_slots, expected)",
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
    values = re.findall(pattern, source, flags=re.MULTILINE)
    require(len(values) == len(set(values)), f"duplicate {label}")
    return values


def source_paths():
    result = [RUNNER, CARGO, PREPARED_TEST, SLOT_TEST, VERUS]
    result.extend(f"{ROCQ}/{name}" for name in ROCQ_SOURCES)
    for family, details in FAMILIES.items():
        result.extend((f"{TLA}/{family}.cfg", f"{TLA}/{family}.tla"))
        result.extend(
            str(path.relative_to(ROOT))
            for path in sorted(
                (ROOT / TLA).glob(f"{details['mutant_prefix']}*Mutant.cfg")
            )
        )
    return result


def test_body(source, target):
    matches = list(
        re.finditer(rf"(?m)^[ \t]*fn[ \t]+{re.escape(target)}[ \t]*\(", source)
    )
    require(len(matches) == 1, f"missing or duplicate executable witness {target}")
    start = matches[0].start()
    attrs = []
    for line in reversed(source[:start].splitlines()):
        line = line.strip()
        if not line.startswith("#["):
            break
        attrs.append(line)
    require("#[test]" in attrs, f"non-test witness {target}")
    require(
        not any(a.startswith(("#[ignore", "#[cfg(", "#[cfg_attr(")) for a in attrs),
        f"ignored or conditional witness {target}",
    )
    following = re.search(r"(?m)^#\[test\]", source[matches[0].end() :])
    end = matches[0].end() + following.start() if following else len(source)
    body = source[start:end]
    normalized = re.sub(r"\s+", "", body)
    require(
        all(
            re.sub(r"\s+", "", fragment) in normalized for fragment in EVIDENCE[target]
        ),
        f"missing concrete assertion in {target}",
    )


def binding(kind, family, symbol):
    target = DIRECT.get((kind, family, symbol))
    return {
        "mode": "concrete_witness" if target else "proof_only",
        "test_source": target[0] if target else None,
        "test_name": target[1] if target else None,
    }


def extract(sources):
    runner = re.sub(r"\\\n", " ", sources[RUNNER])
    for source, target in ((PREPARED_TEST, PREPARED_NAME), (SLOT_TEST, SLOT_NAME)):
        stem = Path(source).stem
        require(
            len(
                re.findall(
                    rf"(?s)\[\[test\]\][^[]*name\s*=\s*\"{stem}\"[^[]*path\s*=\s*\"tests/{stem}\.rs\"",
                    sources[CARGO],
                )
            )
            == 1,
            f"{stem} is absent from normal Cargo test targets",
        )
        test_body(sources[source], target)

    rocq_rows = []
    for file, (
        proof_count,
        proof_digest,
        print_count,
        print_digest,
    ) in ROCQ_SOURCES.items():
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
            f"{file} obligation inventory changed",
        )
        require(set(prints) <= set(proofs), f"{file} prints an undeclared capstone")
        require(
            not re.search(
                r"(?m)^[ \t]*(?:Admitted|Axiom|Parameter)\b|\badmit\.", content
            ),
            f"{file} contains an unproved obligation",
        )
        runner_count = re.findall(
            rf"theories/{re.escape(file)}[ \t]+([0-9]+)\b", runner
        )
        require(
            len(runner_count) == 1, f"{file} closed-context gate missing or duplicate"
        )
        require(
            int(runner_count[0]) == print_count,
            f"{file} closed-context gate does not cover every Print Assumptions target",
        )
        rocq_rows.append(
            {
                "source": path,
                "runner_closed_count": int(runner_count[0]),
                "print_assumptions": prints,
                "obligations": [
                    {
                        "id": f"PARBAG-RC-{len(rocq_rows) + 1:02d}-{index:03d}",
                        "symbol": symbol,
                        "printed": symbol in prints,
                        **binding("rocq", file, symbol),
                    }
                    for index, symbol in enumerate(proofs, 1)
                ],
            }
        )

    verus_source = sources[VERUS]
    require(
        "verus! {" in verus_source and "fn main()" in verus_source,
        "Verus proof module boundary changed",
    )
    verus_body = verus_source.split("fn main()", 1)[0]
    declarations = re.findall(
        r"(?m)^(?:pub )?(?:(open spec|proof|spec) )?fn[ \t]+([A-Za-z_][A-Za-z_0-9]*)",
        verus_body,
    )
    require(
        len(declarations) == len({name for _, name in declarations}),
        "duplicate Verus obligation",
    )
    require(
        len(declarations) == VERUS_COUNT
        and digest([f"{kind or 'exec'}:{name}" for kind, name in declarations])
        == VERUS_DIGEST,
        "Verus canonical-egress inventory changed",
    )
    require(
        f'"$repo_root/{VERUS}"' in runner,
        "Verus canonical-egress proof absent from normal runner",
    )
    verus_rows = [
        {
            "id": f"PARBAG-VS-{index:03d}",
            "symbol": name,
            "kind": kind or "exec",
            **binding("verus", VERUS, name),
        }
        for index, (kind, name) in enumerate(declarations, 1)
    ]

    model_rows = []
    mutant_rows = []
    for family_index, (family, details) in enumerate(FAMILIES.items(), 1):
        cfg_path, model_path = f"{TLA}/{family}.cfg", f"{TLA}/{family}.tla"
        config, model = sources[cfg_path], sources[model_path]
        specification = listed(
            config,
            r"^SPECIFICATION[ \t]+([A-Za-z_][A-Za-z_0-9]*)$",
            f"{family} specification",
        )
        invariants = listed(
            config, r"^INVARIANT[ \t]+([A-Za-z_][A-Za-z_0-9]*)$", f"{family} invariant"
        )
        require(
            specification == ["Spec"] and invariants == details["invariants"],
            f"{family} configured obligations changed",
        )
        require(
            len(re.findall(r"(?m)^INVARIANT\b", config)) == len(invariants)
            and not re.search(r"(?m)^(?:PROPERTY|PROPERTIES|INVARIANTS)\b", config),
            f"{family} has an unparsed TLC obligation",
        )
        for symbol in ["Spec", *invariants]:
            require(
                len(re.findall(rf"(?m)^{re.escape(symbol)}[ \t]*==", model)) == 1,
                f"{family} lacks unique definition {symbol}",
            )
        require(
            re.search(rf"{family}\.cfg[ \t]+{family}\.tla", runner),
            f"{family} baseline absent from normal runner",
        )
        model_rows.append(
            {
                "model": model_path,
                "config": cfg_path,
                "obligations": [
                    {
                        "id": f"PARBAG-TLA-{family_index:02d}-{index:02d}",
                        "symbol": symbol,
                        **binding("tlc", family, symbol),
                    }
                    for index, symbol in enumerate(invariants, 1)
                ],
            }
        )
        expected_cfgs = {
            f"{details['mutant_prefix']}{suffix}Mutant.cfg"
            for suffix in details["mutants"]
        }
        actual_cfgs = {
            Path(path).name
            for path in sources
            if path.startswith(f"{TLA}/{details['mutant_prefix']}")
            and path.endswith("Mutant.cfg")
        }
        require(
            actual_cfgs == expected_cfgs, f"{family} mutant config inventory changed"
        )
        for suffix, expected in details["mutants"].items():
            mutant_cfg = f"{TLA}/{details['mutant_prefix']}{suffix}Mutant.cfg"
            mutant = sources[mutant_cfg]
            mutant_specs = listed(
                mutant,
                r"^SPECIFICATION[ \t]+([A-Za-z_][A-Za-z_0-9]*)$",
                f"{mutant_cfg} specification",
            )
            mutant_invariants = listed(
                mutant,
                r"^INVARIANT[ \t]+([A-Za-z_][A-Za-z_0-9]*)$",
                f"{mutant_cfg} invariant",
            )
            expected_invariants = (
                ["TypeInvariant", expected]
                if family == "RSpaceCanonicalEgress"
                else [expected]
            )
            require(
                len(mutant_specs) == 1 and mutant_invariants == expected_invariants,
                f"{mutant_cfg} changed its configured failure target",
            )
            require(
                not re.search(r"(?m)^(?:PROPERTY|PROPERTIES|INVARIANTS)\b", mutant),
                f"{mutant_cfg} has an unparsed TLC obligation",
            )
            require(
                expected in invariants
                and len(
                    re.findall(rf"(?m)^{re.escape(mutant_specs[0])}[ \t]*==", model)
                )
                == 1,
                f"{mutant_cfg} has no model-backed mutant specification",
            )
            require(
                Path(mutant_cfg).name in runner,
                f"{mutant_cfg} absent from normal proof runner",
            )
            if family == "ParBagInFrameMemo" and suffix in {
                "CensusSkip",
                "ReplaySwap",
                "ReplayOverrun",
                "ReplayRemainingCommit",
                "EmptyMemo",
                "SingletonMemo",
                "EarlyEmission",
            }:
                require(
                    f"{suffix}:{Path(mutant_cfg).name}:{expected}" in runner,
                    f"{mutant_cfg} lacks exact invariant failure routing",
                )
            else:
                require(
                    f"Invariant {expected} is violated" in runner,
                    f"{mutant_cfg} lacks expected invariant failure check",
                )
            mutant_rows.append(
                {
                    "config": mutant_cfg,
                    "model": model_path,
                    "specification": mutant_specs[0],
                    "configured_invariants": mutant_invariants,
                    "expected_invariant_failure": expected,
                }
            )

    require(len(DIRECT) == 5, "unexpected direct executable correspondence count")
    return {
        "scope": "ParBag protobuf projection, sink, in-frame memo, and RSpace canonical egress",
        "boundary": "Concrete witnesses exercise selected production bytes and slot order. The canonical-egress Verus source covers bincode envelope and length arithmetic, while full encoder byte equivalence and publication remain proof-only here.",
        "source_sha256": {
            path: hashlib.sha256(sources[path].encode()).hexdigest()
            for path in source_paths()
            if path != RUNNER
        },
        "rocq": rocq_rows,
        "verus": {"source": VERUS, "obligations": verus_rows},
        "tlc": model_rows,
        "causal_mutants": mutant_rows,
        "executable_witnesses": [
            {"source": source, "test": test}
            for source, test in ((PREPARED_TEST, PREPARED_NAME), (SLOT_TEST, SLOT_NAME))
        ],
        "bincode_formal_boundary": "The canonical-egress Verus source models fixed bincode framing and preflight arithmetic; this registry has no separate formal model of the general-purpose bincode encoder.",
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

    rocq = f"{ROCQ}/ProtobufParAccumulation.v"
    rejected(
        rocq,
        sources[rocq].replace(
            "Theorem ordered_fields_refine", "Theorem stale_ordered_fields_refine", 1
        ),
        "stale Rocq declaration",
    )
    rejected(
        rocq,
        sources[rocq] + "\nTheorem ordered_fields_refine : True.\n",
        "duplicate Rocq declaration",
    )
    rejected(
        rocq,
        sources[rocq].replace("Print Assumptions ordered_fields_refine.\n", "", 1),
        "omitted Rocq capstone",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            "theories/ProtobufParAccumulation.v 41",
            "theories/ProtobufParAccumulation.v 13",
            1,
        ),
        "incomplete Rocq closed-context gate",
    )
    rejected(
        VERUS,
        sources[VERUS].replace(
            "pub proof fn prepared_projection_equals_direct",
            "pub proof fn stale_projection_proof",
            1,
        ),
        "stale Verus proof",
    )
    cfg = f"{TLA}/ParBagProjectionLifecycle.cfg"
    rejected(
        cfg,
        sources[cfg].replace("INVARIANT ReadyImageIsComplete\n", "", 1),
        "omitted TLC invariant",
    )
    rejected(
        cfg,
        sources[cfg] + "\nINVARIANT ReadyImageIsComplete\n",
        "duplicate TLC invariant",
    )
    mutant = f"{TLA}/ParBagProjectionCapMutant.cfg"
    rejected(
        mutant,
        sources[mutant].replace("RefusalOnlyWhenTooLarge", "EveryReadIsComplete", 1),
        "stale causal mutant",
    )
    rejected(
        PREPARED_TEST,
        sources[PREPARED_TEST].replace(
            f"fn {PREPARED_NAME}(", f"#[ignore]\nfn {PREPARED_NAME}(", 1
        ),
        "ignored-only executable witness",
    )
    rejected(
        PREPARED_TEST,
        sources[PREPARED_TEST].replace(
            "term.prepare_protobuf_projection(component_bytes)",
            "term.skip_protobuf_projection(component_bytes)",
            1,
        ),
        "lost concrete projection assertion",
    )
    rejected(
        PREPARED_TEST,
        sources[PREPARED_TEST].replace(
            "assert_eq!(\n                term.encode_to_vec(),\n                message_bytes,",
            "assert_ne!(\n                term.encode_to_vec(),\n                message_bytes,",
            1,
        ),
        "lost prepared/direct image equality",
    )
    extra = dict(sources)
    extra[f"{TLA}/ParBagProjectionExtraMutant.cfg"] = sources[
        f"{TLA}/ParBagProjectionCapMutant.cfg"
    ]
    try:
        extract(extra)
    except RegistryError:
        pass
    else:
        raise RegistryError("extra ParBag mutant configuration unexpectedly accepted")
    require(extract(sources) == baseline, "nondeterministic ParBag egress inventory")


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
            "generated ParBag egress inventory is stale",
        )
    else:
        self_test(sources, inventory)
    print(
        f"ParBag egress registry {'mutations passed' if args.self_test else 'current'}: 4 TLC models, {len(inventory['causal_mutants'])} mutants, 3 Rocq sources, {VERUS_COUNT} Verus declarations"
    )


if __name__ == "__main__":
    try:
        main()
    except (RegistryError, KeyError, json.JSONDecodeError) as error:
        raise SystemExit(f"ParBag egress registry: {error}") from error
