import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/free_map_journal_obligations.generated.json"
CONFIG = "formal/tlaplus/stack_safe_pda/FreeMapJournal.cfg"
MODEL = "formal/tlaplus/stack_safe_pda/FreeMapJournal.tla"
ROCQ = "formal/rocq/stack_safe_pda/theories/FreeMapJournal.v"
VERUS = "formal/verus/free_map_transaction.rs"
RUNTIME = "rholang/src/rust/interpreter/matcher/free_map_transaction.rs"
MODULE = "rholang/src/rust/interpreter/matcher/mod.rs"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
MCRL2_ROOT = "formal/mcrl2/stack_safe_pda/"
MCRL2 = {
    "implementation": MCRL2_ROOT + "free_map_journal_impl.mcrl2",
    "specification": MCRL2_ROOT + "free_map_journal_spec.mcrl2",
    "no_leak": MCRL2_ROOT + "free_map_journal_no_leak.mcf",
    "or_leak_mutant": MCRL2_ROOT + "free_map_journal_or_leak_mutant.mcrl2",
    "partial_apply_mutant": MCRL2_ROOT + "free_map_journal_partial_apply_mutant.mcrl2",
}
TLA_BINDINGS = {
    "TypeInvariant": None,
    "SnapshotJournalLockstep": "free_map_transaction_trace_refines_snapshot_oracle",
    "CheckpointShape": None,
    "CheckpointSnapshotsAgree": "nested_checkpoints_restore_exact_maps_without_discarding_outer_history",
    "ActiveRootCanBeRestored": "root_refusal_and_unwind_restore_the_entry_map",
    "CompletedStateIsClosed": "successful_root_retains_values_and_clears_history",
    "ValidationIsAtomic": None,
    "JournalBoundedByWrites": None,
    "ActionWitnessesAreNonVacuous": None,
    "ReverseUndoMutationIsDetected": None,
    "RollbackAllFrameMutationIsDetected": None,
    "ApplyBeforeValidationMutationIsDetected": None,
    "DropWithoutUndoMutationIsDetected": None,
}
ROCQ_CAPSTONES = [
    "journal_connective_control_refines_snapshot_semantics",
    "journal_transaction_refines_snapshot_transaction",
]
VERUS_DECLARATIONS = [
    ("len", "verified_exec"),
    ("valid", "open_spec"),
    ("state_at", "open_spec"),
    ("new", "verified_exec"),
    ("checkpoint", "verified_exec"),
    ("write", "verified_exec"),
    ("rollback", "verified_exec"),
    ("take_delta_and_rollback", "verified_exec"),
    ("abort", "verified_exec"),
    ("finish", "verified_exec"),
    ("exec_write_refines_spec", "verified_exec"),
    ("exec_rollback_refines_spec", "verified_exec"),
    ("exec_take_delta_and_rollback_refines_spec", "verified_exec"),
    ("exec_finish_refines_spec", "verified_exec"),
    ("abort_guard_restores_root_spec", "verified_exec"),
    ("undo_len_after_completed_writes", "open_spec"),
    ("undo_len_tracks_completed_writes", "proof"),
    ("delta_len_is_bounded_by_segment_writes", "proof"),
]
VERUS_BINDINGS = {
    "checkpoint": "nested_checkpoints_restore_exact_maps_without_discarding_outer_history",
    "write": "free_map_transaction_trace_refines_snapshot_oracle",
    "rollback": "nested_checkpoints_restore_exact_maps_without_discarding_outer_history",
    "take_delta_and_rollback": "delta_extraction_moves_final_writes_and_restores_the_baseline",
    "abort": "root_refusal_and_unwind_restore_the_entry_map",
    "finish": "successful_root_retains_values_and_clears_history",
    "exec_write_refines_spec": "free_map_transaction_trace_refines_snapshot_oracle",
    "exec_rollback_refines_spec": "nested_checkpoints_restore_exact_maps_without_discarding_outer_history",
    "exec_take_delta_and_rollback_refines_spec": "delta_extraction_moves_final_writes_and_restores_the_baseline",
    "exec_finish_refines_spec": "free_map_transaction_trace_refines_snapshot_oracle",
    "abort_guard_restores_root_spec": "root_refusal_and_unwind_restore_the_entry_map",
}
TEST_ASSERTIONS = {
    "free_map_transaction_trace_refines_snapshot_oracle": (
        "run_trace_against_snapshot_oracle(&trace, true);",
        "run_trace_against_snapshot_oracle(&trace, false);",
    ),
    "nested_checkpoints_restore_exact_maps_without_discarding_outer_history": (
        "transaction.rollback(inner);",
        "transaction.rollback(outer);",
        "assert_eq!(transaction.free_map(), &baseline);",
    ),
    "root_refusal_and_unwind_restore_the_entry_map": (
        "assert!(transaction.finish(false).is_none());",
        "assert_eq!(refused.free_map, baseline);",
        'assert_unwind_probe(&child, "root-write"',
    ),
    "successful_root_retains_values_and_clears_history": (
        "assert_eq!(transaction.journal_len(), 2);",
        "assert!(transaction.finish(true).is_some());",
        "assert_eq!(context.free_map.get(&1), Some(&value(10)));",
    ),
    "delta_extraction_moves_final_writes_and_restores_the_baseline": (
        "let delta = transaction.take_delta_and_rollback(checkpoint);",
        "assert_eq!(transaction.free_map(), &baseline);",
        "assert_eq!(delta.as_map().get(&1), Some(&value(12)));",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def lines(source, directive):
    found = re.findall(rf"(?m)^{directive}[ \t]+([A-Za-z][A-Za-z0-9_]*)$", source)
    require(len(found) == len(set(found)), f"duplicate {directive}")
    require(
        len(found) == len(re.findall(rf"(?m)^{directive}\b", source)),
        f"malformed {directive}",
    )
    return found


def test_body(source, name):
    matches = list(re.finditer(rf"(?m)^    fn {re.escape(name)}\(\)", source))
    require(len(matches) == 1, f"missing or duplicate normal-suite test: {name}")
    prefix = source[: matches[0].start()].splitlines()
    attrs = []
    for line in reversed(prefix):
        if not line.strip().startswith("#["):
            break
        attrs.append(line.strip())
    require("#[test]" in attrs, f"non-test target: {name}")
    require(
        not any(a.startswith("#[ignore") for a in attrs), f"ignored-only target: {name}"
    )
    require(
        not any(a.startswith("#[cfg") for a in attrs), f"conditional target: {name}"
    )
    start = matches[0].start()
    next_test = source.find("    #[test]", start + 1)
    return source[start : next_test if next_test >= 0 else len(source)]


def extract(sources, mutant_names):
    config = sources[CONFIG]
    model = sources[MODEL]
    runner = re.sub(r"\\\n", " ", sources[RUNNER])
    require(lines(config, "SPECIFICATION") == ["Spec"], "stale TLC specification")
    invariants = lines(config, "INVARIANT")
    require(
        invariants == list(TLA_BINDINGS), "missing, extra, or reordered TLC invariant"
    )
    require(
        not re.search(r"(?m)^PROPERT(?:Y|IES)\b|^INVARIANTS\b", config),
        "unregistered TLC directive",
    )
    for symbol in ["Spec", *invariants]:
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}[ \t]*==", model)) == 1,
            f"missing or duplicate TLC definition: {symbol}",
        )
    require(
        'tlc_run "$tla_metadir" FreeMapJournal.cfg FreeMapJournal.tla' in runner,
        "FreeMap TLC baseline absent from normal proof runner",
    )
    require(
        "mod free_map_transaction;" in sources[MODULE],
        "runtime unit tests unregistered",
    )
    require(
        "rholang_run_tests 7 --lib free_map_transaction::tests" in runner,
        "runtime unit tests absent from normal runner",
    )
    for name, fragments in TEST_ASSERTIONS.items():
        body = test_body(sources[RUNTIME], name)
        for fragment in fragments:
            require(fragment in body, f"stale executable assertion: {name}: {fragment}")
    helper = sources[RUNTIME].split("fn run_trace_against_snapshot_oracle(", 1)
    require(len(helper) == 2, "missing trace refinement helper")
    for fragment in (
        "assert_eq!(delta.as_map(), &expected_delta);",
        "assert_eq!(replay, before);",
        "assert_eq!(transaction.free_map(), &oracle);",
        "assert_eq!(context.free_map, expected);",
    ):
        require(
            fragment in helper[1].split("    #[test]", 1)[0],
            f"stale trace refinement assertion: {fragment}",
        )
    tla_rows = []
    for index, symbol in enumerate(invariants, 1):
        target = TLA_BINDINGS[symbol]
        tla_rows.append(
            {
                "id": f"FMJ-TLA-{index:02d}",
                "symbol": symbol,
                "classification": "executable_refinement" if target else "proof_only",
                "target": target,
                "target_source": RUNTIME if target else None,
            }
        )
    rocq = sources[ROCQ]
    prints = re.findall(r"(?m)^Print Assumptions ([A-Za-z][A-Za-z0-9_]*)\.$", rocq)
    require(prints == ROCQ_CAPSTONES, "missing, extra, or reordered Rocq capstone")
    require(
        len(prints) == len(re.findall(r"(?m)^Print Assumptions\b", rocq)),
        "malformed Rocq capstone",
    )
    for symbol in prints:
        require(
            len(re.findall(rf"(?m)^Theorem {symbol}[ \t]*:", rocq)) == 1,
            f"missing or duplicate Rocq theorem: {symbol}",
        )
    require(
        "theories/FreeMapJournal.v" in runner and "free_map_closed_capstones" in runner,
        "Rocq closed-capstone gate absent",
    )
    require(
        '"2"' in runner.split('if [[ "$free_map_closed_capstones" != ', 1)[1][:20],
        "Rocq closed-capstone count changed",
    )
    verus = sources[VERUS]
    declarations = re.findall(
        r"(?m)^\s*pub[ \t]+(?:(open spec|proof)[ \t]+)?fn[ \t]+([A-Za-z][A-Za-z0-9_]*)[ \t]*\(",
        verus,
    )
    observed = [
        (name, (kind.replace(" ", "_") if kind else "verified_exec"))
        for kind, name in declarations
    ]
    require(
        observed == VERUS_DECLARATIONS,
        "missing, extra, reordered, or stale Verus obligation",
    )
    require(
        "--no-cheating --num-threads 1" in runner and VERUS in runner,
        "Verus no-cheating run absent",
    )
    verus_rows = []
    for index, (symbol, kind) in enumerate(observed, 1):
        target = VERUS_BINDINGS.get(symbol)
        verus_rows.append(
            {
                "id": f"FMJ-VERUS-{index:02d}",
                "symbol": symbol,
                "kind": kind,
                "classification": "executable_refinement" if target else "proof_only",
                "target": target,
                "target_source": RUNTIME if target else None,
            }
        )
    require(
        "proc Impl =" in sources[MCRL2["implementation"]],
        "missing mCRL2 implementation",
    )
    require(
        "proc Spec =" in sources[MCRL2["specification"]], "missing mCRL2 specification"
    )
    require(
        "[ true* . (or_binding_leak + partial_apply) ] false"
        in sources[MCRL2["no_leak"]],
        "stale mCRL2 no-leak property",
    )
    require(
        mutant_names
        == {
            "free_map_journal_or_leak_mutant.mcrl2",
            "free_map_journal_partial_apply_mutant.mcrl2",
        },
        "missing or unregistered mCRL2 causal mutant",
    )
    for mutant, marker in (
        ("or_leak_mutant", "or_binding_leak . publish_binding"),
        ("partial_apply_mutant", "partial_apply . tau . reject"),
    ):
        require(marker in sources[MCRL2[mutant]], f"stale mCRL2 {mutant}")
    for marker in (
        "free_map_journal_impl.mcrl2",
        "free_map_journal_spec.mcrl2",
        "free_map_journal_no_leak.mcf",
        "for mutant in or_leak partial_apply; do",
        "expected FreeMap ${mutant} mutant to break bisimulation",
        "expected FreeMap ${mutant} mutant leak witness",
    ):
        require(
            marker in runner,
            f"mCRL2 proof or causal mutant absent from runner: {marker}",
        )
    return {
        "tla_invariants": tla_rows,
        "rocq_print_assumptions": [
            {
                "id": f"FMJ-ROCQ-{i:02d}",
                "symbol": symbol,
                "classification": "proof_only",
            }
            for i, symbol in enumerate(prints, 1)
        ],
        "verus_obligations": verus_rows,
        "mcrl2_obligations": [
            {
                "id": "FMJ-MCRL2-01",
                "kind": "branching_bisimulation",
                "classification": "proof_only",
                "sources": [MCRL2["implementation"], MCRL2["specification"]],
            },
            {
                "id": "FMJ-MCRL2-02",
                "kind": "no_binding_leak",
                "classification": "proof_only",
                "sources": [MCRL2["implementation"], MCRL2["no_leak"]],
            },
            {
                "id": "FMJ-MCRL2-03",
                "kind": "or_leak_causal_mutant",
                "classification": "proof_only",
                "source": MCRL2["or_leak_mutant"],
            },
            {
                "id": "FMJ-MCRL2-04",
                "kind": "partial_apply_causal_mutant",
                "classification": "proof_only",
                "source": MCRL2["partial_apply_mutant"],
            },
        ],
        "scope_limit": "Executable rows identify direct normal-suite transaction assertions for concrete operations. Finite TLA state properties, Rocq theorems, Verus abstract lemmas, and mCRL2 equivalence/no-leak are proof-only. Registry checks source and runner wiring, not proof execution.",
        "source_sha256": {
            path: hashlib.sha256(source.encode()).hexdigest()
            for path, source in sorted(sources.items())
            if path not in (RUNNER, MODULE)
        },
    }


def reject(sources, mutant_names, path, before, after, expected):
    mutated = dict(sources)
    require(before in mutated[path], f"self-test setup absent: {before}")
    mutated[path] = mutated[path].replace(before, after, 1)
    try:
        extract(mutated, mutant_names)
    except RegistryError as error:
        require(expected in str(error), f"wrong mutation rejection: {error}")
        return
    raise RegistryError(f"mutation unexpectedly passed: {expected}")


def self_test(sources, mutant_names):
    reject(
        sources,
        mutant_names,
        CONFIG,
        "INVARIANT SnapshotJournalLockstep\n",
        "",
        "missing, extra, or reordered TLC invariant",
    )
    reject(
        sources,
        mutant_names,
        CONFIG,
        "INVARIANT SnapshotJournalLockstep\n",
        "INVARIANT SnapshotJournalLockstep\nINVARIANT SnapshotJournalLockstep\n",
        "duplicate INVARIANT",
    )
    reject(
        sources,
        mutant_names,
        MODEL,
        "SnapshotJournalLockstep ==",
        "RemovedLockstep ==",
        "missing or duplicate TLC definition",
    )
    reject(
        sources,
        mutant_names,
        ROCQ,
        "Print Assumptions journal_transaction_refines_snapshot_transaction.\n",
        "",
        "missing, extra, or reordered Rocq capstone",
    )
    reject(
        sources,
        mutant_names,
        VERUS,
        "pub fn exec_rollback_refines_spec(",
        "pub fn stale_rollback_refines_spec(",
        "missing, extra, reordered, or stale Verus obligation",
    )
    reject(
        sources,
        mutant_names,
        RUNTIME,
        "    #[test]\n    fn nested_checkpoints_restore_exact_maps_without_discarding_outer_history",
        "    #[test]\n    #[ignore]\n    fn nested_checkpoints_restore_exact_maps_without_discarding_outer_history",
        "ignored-only target",
    )
    reject(
        sources,
        mutant_names,
        RUNTIME,
        "assert_eq!(transaction.free_map(), &baseline);",
        "assert_eq!(transaction.free_map(), &wrong);",
        "stale executable assertion",
    )
    reject(
        sources,
        mutant_names,
        RUNNER,
        'tlc_run "$tla_metadir" FreeMapJournal.cfg FreeMapJournal.tla',
        'tlc_run "$tla_metadir" Other.cfg Other.tla',
        "TLC baseline absent",
    )
    reject(
        sources,
        mutant_names,
        MCRL2["no_leak"],
        "or_binding_leak + partial_apply",
        "or_binding_leak",
        "stale mCRL2 no-leak property",
    )
    reject(
        sources,
        mutant_names,
        MCRL2["or_leak_mutant"],
        "or_binding_leak . publish_binding",
        "publish_binding",
        "stale mCRL2 or_leak_mutant",
    )
    try:
        extract(sources, mutant_names | {"free_map_journal_unknown_mutant.mcrl2"})
    except RegistryError as error:
        require(
            "missing or unregistered mCRL2 causal mutant" in str(error),
            f"wrong mutant inventory rejection: {error}",
        )
    else:
        raise RegistryError("unregistered mCRL2 causal mutant passed")


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    paths = {CONFIG, MODEL, ROCQ, VERUS, RUNTIME, MODULE, RUNNER, *MCRL2.values()}
    sources = {
        path: (ROOT / path).read_text(encoding="utf-8") for path in sorted(paths)
    }
    mutant_names = {
        path.name
        for path in (ROOT / MCRL2_ROOT).glob("free_map_journal_*_mutant.mcrl2")
    }
    payload = (
        json.dumps(extract(sources, mutant_names), indent=2, sort_keys=True) + "\n"
    )
    if args.self_test:
        self_test(sources, mutant_names)
        print("FreeMap journal registry: mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload, encoding="utf-8")
        print("FreeMap journal registry: written")
    else:
        require(
            (ROOT / OUTPUT).read_text(encoding="utf-8") == payload,
            "FreeMap journal registry is stale",
        )
        print("FreeMap journal registry: current")


if __name__ == "__main__":
    main()
