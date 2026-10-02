import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/rci2_channel_image_obligations.generated.json"
ROCQ = "formal/rocq/stack_safe_pda/theories/ChannelImage.v"
VERUS = "formal/verus/channel_image_cursor.rs"
TEST = "models/tests/channel_image_v2.rs"
CARGO = "models/Cargo.toml"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
ROCQ_CAPSTONES = [
    "channel_prefix_parser_is_left_inverse",
    "channel_exact_parser_is_left_inverse",
    "canonical_channel_image_is_unique_iff_semantic_par_equality",
    "root_locally_free_is_quotiented_out",
    "connective_flag_separates_otherwise_equal_channels",
    "endpoint_multiplicities_are_preserved_exactly",
    "every_successfully_decoded_channel_has_canonical_endpoints",
    "duplicate_endpoint_fails_strict_order_check",
    "permuted_endpoint_fails_strict_order_check",
    "noncanonical_rpb3_endpoint_fails_admission_check",
    "zero_multiplicity_fails_admission_check",
]
VERUS_DECLARATIONS = [
    ("checked_cursor_add", "verified_exec"),
    ("checked_bounded_advance", "verified_exec"),
    ("checked_cursor_increment", "verified_exec"),
    ("u64_be_spec", "open_spec"),
    ("executable_u64_from_be_bytes", "verified_exec"),
    ("checked_fixed_u64_read", "verified_exec"),
    ("checked_escaped_frame_step", "verified_exec"),
    ("checked_distinct_increment", "verified_exec"),
    ("exact_distinct_count", "verified_exec"),
    ("admitted_rpb3_relative_key", "open_spec"),
    ("byte_lex_less", "open_spec"),
    ("endpoint_admitted", "open_spec"),
    ("endpoints_strictly_ascending", "open_spec"),
    ("escape_rpb3_key", "open_spec"),
    ("unescape_rpb3_key", "open_spec"),
    ("unescape_nonzero_prefix", "proof"),
    ("unescape_zero_prefix", "proof"),
    ("escaped_key_round_trip", "proof"),
    ("encode_endpoint", "open_spec"),
    ("decode_endpoint", "open_spec"),
    ("admitted_endpoint_encoder_decoder_refinement", "proof"),
    ("encode_endpoints", "open_spec"),
    ("decode_endpoints", "open_spec"),
    ("encode_endpoints_preserves_length", "proof"),
    ("decode_encoded_endpoint_prefix", "proof"),
    ("admitted_endpoints_encoder_decoder_refinement", "proof"),
    ("semantic_channel_admitted", "open_spec"),
    ("encode_channel", "open_spec"),
    ("decode_channel", "open_spec"),
    ("admitted_channel_encoder_decoder_refinement", "proof"),
    ("admitted_channel_boundary_is_non_vacuous", "proof"),
]
WITNESSES = {
    "channel_prefix_parser_is_left_inverse": (
        "borrowed_cursor_takes_exactly_one_image_from_a_stream",
        "assert_eq!(consumed + second_consumed, stream.len());",
    ),
    "channel_exact_parser_is_left_inverse": (
        "decode_reconstructs_the_identity_normalized_par_without_entry_projection",
        'assert_eq!(decoded, par, "semantic round trip: {label}");',
    ),
    "canonical_channel_image_is_unique_iff_semantic_par_equality": (
        "image_is_exactly_a_function_of_par_equality",
        'assert_eq!(left, right, "image locally_free quotient: {label}");',
    ),
    "root_locally_free_is_quotiented_out": (
        "image_is_exactly_a_function_of_par_equality",
        '"root locally_free quotient: {label}"',
    ),
    "connective_flag_separates_otherwise_equal_channels": (
        "image_is_exactly_a_function_of_par_equality",
        '"connective image: {label}"',
    ),
    "endpoint_multiplicities_are_preserved_exactly": (
        "multiplicity_is_one_fixed_width_counter_not_repeated_endpoints",
        "&16u64.to_be_bytes()",
    ),
    "duplicate_endpoint_fails_strict_order_check": (
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "ChannelImageV2::try_from_bytes(&duplicate)",
    ),
    "permuted_endpoint_fails_strict_order_check": (
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "ChannelImageV2::try_from_bytes(&reversed)",
    ),
    "noncanonical_rpb3_endpoint_fails_admission_check": (
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "image_with_single_endpoint_key(bytes, &nonminimal_varint)",
    ),
    "zero_multiplicity_fails_admission_check": (
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "Err(ChannelImageError::ZeroMultiplicity)",
    ),
    "executable_u64_from_be_bytes": (
        "multiplicity_is_one_fixed_width_counter_not_repeated_endpoints",
        "&16u64.to_be_bytes()",
    ),
    "checked_escaped_frame_step": (
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "Err(ChannelImageError::InvalidEscape(0x7E))",
    ),
    "exact_distinct_count": (
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "Err(ChannelImageError::DistinctCountMismatch {",
    ),
    "endpoint_admitted": (
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "image_with_single_endpoint_key(bytes, &nonminimal_varint)",
    ),
    "endpoints_strictly_ascending": (
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "ChannelImageV2::try_from_bytes(&reversed)",
    ),
    "encode_endpoint": (
        "decode_reconstructs_the_identity_normalized_par_without_entry_projection",
        'assert_eq!(image(&decoded), encoded, "byte-stable round trip: {label}");',
    ),
    "decode_endpoint": (
        "decode_reconstructs_the_identity_normalized_par_without_entry_projection",
        'assert_eq!(decoded, par, "semantic round trip: {label}");',
    ),
    "encode_endpoints": (
        "decode_reconstructs_the_identity_normalized_par_without_entry_projection",
        'assert_eq!(image(&decoded), encoded, "byte-stable round trip: {label}");',
    ),
    "decode_endpoints": (
        "decode_reconstructs_the_identity_normalized_par_without_entry_projection",
        'assert_eq!(decoded, par, "semantic round trip: {label}");',
    ),
    "semantic_channel_admitted": (
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "Err(ChannelImageError::ZeroMultiplicity)",
    ),
    "encode_channel": (
        "decode_reconstructs_the_identity_normalized_par_without_entry_projection",
        'assert_eq!(image(&decoded), encoded, "byte-stable round trip: {label}");',
    ),
    "decode_channel": (
        "decode_reconstructs_the_identity_normalized_par_without_entry_projection",
        'assert_eq!(decoded, par, "semantic round trip: {label}");',
    ),
}
TEST_ASSERTIONS = {
    "borrowed_cursor_takes_exactly_one_image_from_a_stream": (
        "ValidatedChannelImage::take_prefix(&stream)",
        "ValidatedChannelImage::take_prefix(&stream[consumed..])",
        "Err(ChannelImageError::TrailingBytes { .. })",
    ),
    "decode_reconstructs_the_identity_normalized_par_without_entry_projection": (
        "for (label, par) in par_corpus::par_corpus()",
        'assert_eq!(admitted, encoded, "admitted byte identity: {label}");',
        'assert_eq!(decoded, par, "semantic round trip: {label}");',
    ),
    "image_is_exactly_a_function_of_par_equality": (
        "for (label, par) in par_corpus::par_corpus()",
        'assert_eq!(left, right, "image locally_free quotient: {label}");',
        '"connective image: {label}"',
    ),
    "multiplicity_is_one_fixed_width_counter_not_repeated_endpoints": (
        "&1u64.to_be_bytes()",
        "&16u64.to_be_bytes()",
        "assert_eq!(one_image.as_bytes().len(), sixteen_image.as_bytes().len());",
    ),
    "malformed_images_fail_closed_at_every_scalar_and_trie_boundary": (
        "ChannelImageV2::try_from_bytes(&duplicate)",
        "ChannelImageV2::try_from_bytes(&reversed)",
        "image_with_single_endpoint_key(bytes, &nonminimal_varint)",
        "Err(ChannelImageError::ZeroMultiplicity)",
        "Err(ChannelImageError::DistinctCountMismatch {",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def test_body(source, name):
    matches = list(re.finditer(rf"(?m)^fn {re.escape(name)}\(\)", source))
    require(len(matches) == 1, f"missing or duplicate normal-suite test: {name}")
    attrs = []
    for line in reversed(source[: matches[0].start()].splitlines()):
        if not line.strip().startswith("#["):
            break
        attrs.append(line.strip())
    require("#[test]" in attrs, f"non-test target: {name}")
    require(
        not any(attr.startswith("#[ignore") for attr in attrs),
        f"ignored-only target: {name}",
    )
    require(
        not any(attr.startswith("#[cfg") for attr in attrs),
        f"conditional target: {name}",
    )
    start = matches[0].start()
    next_test = source.find("\n#[test]", start)
    return source[start : next_test if next_test >= 0 else len(source)]


def extract(sources, tla_names, mutant_names):
    require(not tla_names, "unregistered ChannelImage TLC source")
    require(not mutant_names, "unregistered ChannelImage causal mutant")
    rocq = sources[ROCQ]
    prints = re.findall(r"(?m)^Print Assumptions ([A-Za-z][A-Za-z0-9_]*)\.$", rocq)
    require(prints == ROCQ_CAPSTONES, "missing, extra, or reordered Rocq capstone")
    require(
        len(prints) == len(re.findall(r"(?m)^Print Assumptions\b", rocq)),
        "malformed Rocq capstone",
    )
    for symbol in prints:
        require(
            len(re.findall(rf"(?m)^Theorem {re.escape(symbol)}[ \t]*:", rocq)) == 1,
            f"missing or duplicate Rocq theorem: {symbol}",
        )
    runner = re.sub(r"\\\n", " ", sources[RUNNER])
    require(
        "rocq_compile_with_closed_count theories/ChannelImage.v 11" in runner,
        "ChannelImage Rocq closed-capstone gate absent",
    )
    require(
        "--no-cheating --num-threads 1" in runner and VERUS in runner,
        "ChannelImage Verus no-cheating run absent",
    )
    verus = sources[VERUS]
    proof_block = verus.split("\n}\n\nfn main()", 1)
    require(len(proof_block) == 2, "Verus proof block or main changed")
    declarations = re.findall(
        r"(?m)^\s*(?:pub[ \t]+)?(?:(open spec|proof)[ \t]+)?fn[ \t]+([A-Za-z][A-Za-z0-9_]*)[ \t]*\(",
        proof_block[0],
    )
    observed = [
        (name, kind.replace(" ", "_") if kind else "verified_exec")
        for kind, name in declarations
    ]
    require(
        observed == VERUS_DECLARATIONS,
        "missing, extra, reordered, or stale Verus declaration",
    )
    require(
        re.search(
            r'(?s)\[\[test\]\]\s*name = "channel_image_v2"\s*path = "tests/channel_image_v2\.rs"',
            sources[CARGO],
        )
        is not None,
        "normal ChannelImage test target absent",
    )
    require(
        len(re.findall(r"(?m)^#\[test\]$", sources[TEST])) == 12,
        "ChannelImage normal-suite census changed",
    )
    require(
        not (set(WITNESSES) - (set(prints) | {name for name, _ in observed})),
        "witness bound to unconfigured obligation",
    )
    for test, fragments in TEST_ASSERTIONS.items():
        body = test_body(sources[TEST], test)
        for fragment in fragments:
            require(fragment in body, f"stale executable assertion: {test}: {fragment}")
    for symbol, (test, fragment) in WITNESSES.items():
        require(
            fragment in test_body(sources[TEST], test),
            f"stale executable witness: {symbol}",
        )
    return {
        "tla_obligations": [],
        "causal_mutants": [],
        "rocq_print_assumptions": [
            {
                "id": f"RCI2-ROCQ-{index:02d}",
                "symbol": symbol,
                "classification": "executable_refinement"
                if symbol in WITNESSES
                else "proof_only",
                "target_source": TEST if symbol in WITNESSES else None,
                "target": WITNESSES[symbol][0] if symbol in WITNESSES else None,
                "assertion": WITNESSES[symbol][1] if symbol in WITNESSES else None,
            }
            for index, symbol in enumerate(prints, 1)
        ],
        "verus_obligations": [
            {
                "id": f"RCI2-VERUS-{index:02d}",
                "symbol": symbol,
                "kind": kind,
                "classification": "executable_refinement"
                if symbol in WITNESSES
                else "proof_only",
                "target_source": TEST if symbol in WITNESSES else None,
                "target": WITNESSES[symbol][0] if symbol in WITNESSES else None,
                "assertion": WITNESSES[symbol][1] if symbol in WITNESSES else None,
            }
            for index, (symbol, kind) in enumerate(observed, 1)
        ],
        "scope_limit": "No ChannelImage-specific TLA config or causal mutant is accepted; ChannelPrefixReplacementMutants.v is a separate transaction family. Executable rows bind concrete normal-suite examples and boundaries, not universal Rocq theorems, Verus arithmetic overflow, or RPB3 parser internals. Abstract proofs and unmatched Verus steps remain proof-only. This registry checks source and runner wiring, not proof/test execution.",
        "source_sha256": {
            path: hashlib.sha256(source.encode()).hexdigest()
            for path, source in sorted(sources.items())
            if path not in (RUNNER, CARGO)
        },
    }


def reject(sources, tla, mutants, path, before, after, expected):
    changed = dict(sources)
    require(before in changed[path], f"self-test setup absent: {before}")
    changed[path] = changed[path].replace(before, after)
    try:
        extract(changed, tla, mutants)
    except RegistryError as error:
        require(expected in str(error), f"wrong mutation rejection: {error}")
        return
    raise RegistryError(f"mutation unexpectedly passed: {expected}")


def self_test(sources, tla, mutants):
    reject(
        sources,
        tla,
        mutants,
        ROCQ,
        "Print Assumptions channel_prefix_parser_is_left_inverse.\n",
        "",
        "missing, extra, or reordered Rocq capstone",
    )
    reject(
        sources,
        tla,
        mutants,
        ROCQ,
        "Print Assumptions channel_prefix_parser_is_left_inverse.\n",
        "Print Assumptions channel_prefix_parser_is_left_inverse.\nPrint Assumptions channel_prefix_parser_is_left_inverse.\n",
        "missing, extra, or reordered Rocq capstone",
    )
    reject(
        sources,
        tla,
        mutants,
        VERUS,
        "pub fn checked_fixed_u64_read(",
        "pub fn stale_fixed_u64_read(",
        "missing, extra, reordered, or stale Verus declaration",
    )
    reject(
        sources,
        tla,
        mutants,
        TEST,
        "#[test]\nfn borrowed_cursor_takes_exactly_one_image_from_a_stream",
        "#[test]\n#[ignore]\nfn borrowed_cursor_takes_exactly_one_image_from_a_stream",
        "ignored-only target",
    )
    reject(
        sources,
        tla,
        mutants,
        TEST,
        "assert_eq!(consumed + second_consumed, stream.len());",
        "assert_eq!(consumed + second_consumed, 0);",
        "stale executable witness",
    )
    reject(
        sources,
        tla,
        mutants,
        CARGO,
        'name = "channel_image_v2"',
        'name = "stale_channel_image_v2"',
        "normal ChannelImage test target absent",
    )
    reject(
        sources,
        tla,
        mutants,
        RUNNER,
        "rocq_compile_with_closed_count theories/ChannelImage.v 11",
        "rocq_compile_with_closed_count theories/ChannelImage.v 0",
        "Rocq closed-capstone gate absent",
    )
    for changed_tla, changed_mutants, expected in (
        (tla | {"ChannelImage.cfg"}, mutants, "unregistered ChannelImage TLC source"),
        (
            tla,
            mutants | {"ChannelImageMutant.cfg"},
            "unregistered ChannelImage causal mutant",
        ),
    ):
        try:
            extract(sources, changed_tla, changed_mutants)
        except RegistryError as error:
            require(
                expected in str(error), f"wrong formal inventory rejection: {error}"
            )
        else:
            raise RegistryError(f"unexpected formal inventory passed: {expected}")


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    paths = {ROCQ, VERUS, TEST, CARGO, RUNNER}
    sources = {
        path: (ROOT / path).read_text(encoding="utf-8") for path in sorted(paths)
    }
    formal = ROOT / "formal"
    tla = {path.name for path in formal.rglob("*ChannelImage*.cfg")} | {
        path.name for path in formal.rglob("*ChannelImage*.tla")
    }
    mutants = {path.name for path in formal.rglob("*ChannelImage*Mutant*")}
    payload = (
        json.dumps(extract(sources, tla, mutants), indent=2, sort_keys=True) + "\n"
    )
    if args.self_test:
        self_test(sources, tla, mutants)
        print("RCI2 ChannelImage registry: mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload, encoding="utf-8")
        print("RCI2 ChannelImage registry: written")
    else:
        require(
            (ROOT / OUTPUT).read_text(encoding="utf-8") == payload,
            "RCI2 ChannelImage registry is stale",
        )
        print("RCI2 ChannelImage registry: current")


if __name__ == "__main__":
    main()
