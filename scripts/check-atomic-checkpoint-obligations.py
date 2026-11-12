import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FORMAL = Path("formal/tlaplus/stack_safe_pda")
STEM = "RSpaceAtomicCheckpointAuthority"
MODEL = FORMAL / f"{STEM}.tla"
CONFIG = FORMAL / f"{STEM}.cfg"
TEST = Path("rspace++/tests/state/atomic_state_authority.rs")
MODULE = Path("rspace++/src/rspace/state/rspace_state_authority.rs")
RUNNER = Path("scripts/check-stack-safe-pda-proofs.sh")
OUTPUT = Path("formal/atomic_checkpoint_obligations.generated.json")

BINDINGS = [
    (
        "TypeInvariant",
        "proof_only",
        None,
    ),
    (
        "CurrentRootIsVerified",
        "executable_refinement",
        "memory_checkpoint_publishes_all_namespaces_together",
    ),
    (
        "VerifiedRootsHaveCompleteClosure",
        "executable_refinement",
        "missing_or_malformed_cold_leaf_leaves_exact_pre_state",
    ),
    (
        "ContentKeysAreImmutable",
        "executable_refinement",
        "immutable_collision_leaves_exact_pre_state",
    ),
    (
        "NoUnpublishedCheckpointObjects",
        "executable_refinement",
        "plans_from_one_base_both_publish_without_partial_visibility",
    ),
    (
        "GenerationAdvancesExactlyOnActivation",
        "executable_refinement",
        "lmdb_reopen_preserves_exact_catalog_and_known_root_activation",
    ),
    (
        "CommitUsesExactBase",
        "executable_refinement",
        "a_plan_publishes_the_successor_of_its_own_base_after_root_aba",
    ),
    (
        "FailedCommitLeavesAuthorityUnchanged",
        "executable_refinement",
        "missing_or_malformed_cold_leaf_leaves_exact_pre_state",
    ),
    (
        "NoOpLeavesAuthorityUnchanged",
        "executable_refinement",
        "no_op_writes_nothing_and_does_not_advance_generation",
    ),
]

MUTANTS = {
    "RSpaceAtomicCheckpointAuthoritySplitMutant.cfg": "NoUnpublishedCheckpointObjects",
    "RSpaceAtomicCheckpointAuthorityRootOnlyMutant.cfg": "CommitUsesExactBase",
    "RSpaceAtomicCheckpointAuthorityTrustTargetMutant.cfg": "VerifiedRootsHaveCompleteClosure",
}

TEST_ASSERTIONS = {
    "memory_checkpoint_publishes_all_namespaces_together": (
        "commit_local_checkpoint(&prepared, vec![(cold.clone(), bytes)], &validator())",
        "assert_root_objects_visible(&authority, &current, &prepared, &cold);",
    ),
    "missing_or_malformed_cold_leaf_leaves_exact_pre_state": (
        'expect_err("missing cold closure must fail")',
        'expect_err("malformed cold object must fail")',
        "assert_eq!(memory_snapshot(&authority), before);",
    ),
    "immutable_collision_leaves_exact_pre_state": (
        'expect_err("immutable history collision must fail")',
        "assert_eq!(memory_snapshot(&authority), before);",
    ),
    "plans_from_one_base_both_publish_without_partial_visibility": (
        "assert_eq!(first, first_expected);",
        "expected_after_publication(&before_first, &first_plan, &first_leaf, &first_expected)",
        "assert_eq!(second, second_expected);",
        "expected_after_publication(&after_first, &second_plan, &second_leaf, &second_expected)",
    ),
    "lmdb_reopen_preserves_exact_catalog_and_known_root_activation": (
        "assert_eq!(reopened.current_verified_root().unwrap(), Some(committed.clone()));",
        "assert!(reopened.contains_verified_root(empty.root()).unwrap());",
        "assert_eq!(at_empty.generation(), committed.generation() + 1);",
    ),
    "a_plan_publishes_the_successor_of_its_own_base_after_root_aba": (
        'expect("a plan from a verified base publishes after the root ABA")',
        "assert_eq!(published, published_expected);",
        "expected_after_publication(&before_old, &old_plan, &old_leaf, &published_expected)",
    ),
    "no_op_writes_nothing_and_does_not_advance_generation": (
        "assert_eq!(observed, current_before);",
        "assert_eq!(memory_snapshot(&authority), before);",
    ),
}

HELPER_ASSERTIONS = (
    "assert_eq!(authority.current_verified_root().unwrap(), Some(root.clone()));",
    "assert!(authority.contains_verified_root(root.root()).unwrap());",
    'authority .history_store() .expect("legacy history facet") .get_one(&root.root().bytes())'
    " .unwrap() .as_deref(), prepared.root_encoding()",
    'authority .cold_store() .expect("legacy cold facet") .get_one(&cold.encode()) .unwrap()'
    " .is_some()",
)


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def config_items(source, directive):
    values = []
    for line in source.splitlines():
        if line.startswith(f"{directive} "):
            value = line[len(directive) + 1 :].strip()
            require(
                bool(re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", value)),
                f"invalid {directive}: {line}",
            )
            values.append(value)
    require(len(values) == len(set(values)), f"duplicate {directive}")
    return values


def test_targets(source):
    targets = {}
    pattern = re.compile(
        r"(?m)^#\[test\][ \t]*\n((?:#\[[^\n]+\][ \t]*\n)*)fn[ \t]+([A-Za-z_][A-Za-z0-9_]*)[ \t]*\("
    )
    for match in pattern.finditer(source):
        attributes, name = match.groups()
        require(name not in targets, f"duplicate normal-suite test {name}")
        preceding = []
        for line in reversed(source[: match.start()].splitlines()):
            if not line.startswith("#["):
                break
            preceding.append(line)
        attributes = "\n".join([*preceding, attributes])
        following = re.search(r"(?m)^(?:#\[|fn[ \t])", source[match.end() :])
        end = match.end() + following.start() if following else len(source)
        targets[name] = (attributes, source[match.start() : end])
    return targets


def extract(
    model, config, mutant_configs, bindings, test_source, module_source, runner_source
):
    require(
        not re.search(r"(?m)^(INVARIANTS|PROPERTIES)\b", config),
        "unsupported multi-line obligation directive",
    )
    specs = config_items(config, "SPECIFICATION")
    require(specs == ["Spec"], f"unexpected base specification: {specs}")
    invariants = config_items(config, "INVARIANT")
    properties = config_items(config, "PROPERTY")
    obligations = [("invariant", symbol) for symbol in invariants] + [
        ("property", symbol) for symbol in properties
    ]
    symbols = [symbol for _, symbol in obligations]
    require(bool(obligations), "base configuration has no obligations")
    require(len(symbols) == len(set(symbols)), "duplicate configured obligation")
    mapped = {symbol: (mode, target) for symbol, mode, target in bindings}
    require(len(mapped) == len(bindings), "duplicate obligation binding")
    require(set(symbols) == set(mapped), "missing or extra obligation binding")
    require(
        bool(
            re.search(
                r'#\[cfg\(test\)\]\s*#\[path = "\.\./\.\./\.\./tests/state/atomic_state_authority\.rs"\]\s*mod atomic_state_authority_tests;',
                module_source,
            )
        ),
        "normal-suite test module is not registered",
    )
    targets = test_targets(test_source)
    executable_targets = {
        target for _, mode, target in bindings if mode == "executable_refinement"
    }
    require(
        executable_targets == set(TEST_ASSERTIONS), "stale executable witness inventory"
    )
    helper = test_source.split("fn assert_root_objects_visible(", 1)
    require(len(helper) == 2, "missing exact root-visibility helper")
    helper_body = re.sub(r"\s+", " ", helper[1].split("\n#[test]", 1)[0])
    require(
        all(fragment in helper_body for fragment in HELPER_ASSERTIONS),
        "root-visibility helper lost an authority assertion",
    )
    for _, symbol in obligations:
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}\s*==", model)) == 1,
            f"missing or duplicate model definition: {symbol}",
        )
        mode, target = mapped[symbol]
        require(
            mode in {"proof_only", "executable_refinement"},
            f"invalid binding mode: {symbol}",
        )
        if mode == "proof_only":
            require(
                target is None, f"proof-only obligation has executable target: {symbol}"
            )
            continue
        require(target in targets, f"missing normal-suite target: {symbol} -> {target}")
        attributes, body = targets[target]
        require(
            not re.search(r"#\[(?:ignore|cfg|cfg_attr)\b", attributes),
            f"ignored or conditional normal-suite target: {symbol} -> {target}",
        )
        require(
            all(fragment in body for fragment in TEST_ASSERTIONS[target]),
            f"normal-suite target lost direct assertion: {symbol} -> {target}",
        )
    require(
        set(mutant_configs) == set(MUTANTS), "missing or extra mutant configuration"
    )
    mutants = []
    for filename in sorted(mutant_configs):
        source = mutant_configs[filename]
        require(
            not re.search(r"(?m)^(INVARIANTS|PROPERTIES|PROPERTY)\b", source),
            f"unsupported mutant obligation directive: {filename}",
        )
        specifications = config_items(source, "SPECIFICATION")
        require(len(specifications) == 1, f"mutant specification count: {filename}")
        require(
            len(re.findall(rf"(?m)^{re.escape(specifications[0])}\s*==", model)) == 1,
            f"missing mutant specification: {filename}",
        )
        checked = config_items(source, "INVARIANT")
        target = MUTANTS[filename]
        require(
            checked == ["TypeInvariant", target], f"mutant target mismatch: {filename}"
        )
        require(
            target in invariants, f"mutant targets unconfigured invariant: {filename}"
        )
        require(
            f"{filename}:{target}" in runner_source,
            f"mutant is absent from normal proof suite: {filename}",
        )
        mutants.append(
            {
                "config": str(FORMAL / filename),
                "config_sha256": hashlib.sha256(source.encode()).hexdigest(),
                "specification": specifications[0],
                "violated_invariant": target,
            }
        )
    require(
        f"{CONFIG.name} {MODEL.name}" in runner_source,
        "base model is absent from normal proof suite",
    )
    return {
        "model": str(MODEL),
        "config": str(CONFIG),
        "model_sha256": hashlib.sha256(model.encode()).hexdigest(),
        "config_sha256": hashlib.sha256(config.encode()).hexdigest(),
        "normal_suite_test_module": str(TEST),
        "test_sha256": hashlib.sha256(test_source.encode()).hexdigest(),
        "module_sha256": hashlib.sha256(module_source.encode()).hexdigest(),
        "obligations": [
            {
                "kind": kind,
                "symbol": symbol,
                "mode": mapped[symbol][0],
                "test": mapped[symbol][1],
                "assertions": list(TEST_ASSERTIONS[mapped[symbol][1]])
                if mapped[symbol][0] == "executable_refinement"
                else [],
            }
            for kind, symbol in obligations
        ],
        "causal_mutants": mutants,
    }


def fail_if_accepted(call):
    try:
        call()
    except RegistryError:
        return
    raise RegistryError("verifier accepted a mutated registry input")


def self_test(inputs):
    model, config, mutants, bindings, test_source, module_source, runner_source = inputs
    fail_if_accepted(
        lambda: extract(
            model,
            config + "INVARIANT TypeInvariant\n",
            mutants,
            bindings,
            test_source,
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config.replace("INVARIANT TypeInvariant\n", ""),
            mutants,
            bindings,
            test_source,
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config + "PROPERTY StaleLiveness\n",
            mutants,
            bindings,
            test_source,
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config + "INVARIANTS\nStaleSafety\n",
            mutants,
            bindings,
            test_source,
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config,
            mutants,
            bindings
            + [
                (
                    "StaleInvariant",
                    "proof_only",
                    None,
                )
            ],
            test_source,
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config,
            mutants,
            [
                (
                    symbol,
                    mode,
                    "absent_test" if symbol == "CurrentRootIsVerified" else target,
                )
                for symbol, mode, target in bindings
            ],
            test_source,
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config,
            mutants,
            bindings + [bindings[0]],
            test_source,
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config,
            mutants,
            bindings,
            test_source.replace(
                "expected_after_publication(&after_first, &second_plan, &second_leaf, &second_expected)",
                "memory_snapshot(&authority)",
                1,
            ),
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config,
            mutants,
            bindings,
            test_source.replace(
                "assert!(authority.contains_verified_root(root.root()).unwrap());",
                "assert!(true);",
                1,
            ),
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config,
            mutants,
            bindings,
            test_source.replace(
                "#[test]\nfn no_op_writes_nothing_and_does_not_advance_generation",
                "#[cfg(any())]\n#[test]\nfn no_op_writes_nothing_and_does_not_advance_generation",
                1,
            ),
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config,
            mutants,
            bindings[1:],
            test_source,
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config,
            mutants,
            bindings,
            test_source.replace(
                "#[test]\nfn memory_checkpoint_publishes_all_namespaces_together",
                "#[test]\n#[ignore]\nfn memory_checkpoint_publishes_all_namespaces_together",
            ),
            module_source,
            runner_source,
        )
    )
    fail_if_accepted(
        lambda: extract(
            model,
            config,
            {
                key: value
                for key, value in mutants.items()
                if key != next(iter(mutants))
            },
            bindings,
            test_source,
            module_source,
            runner_source,
        )
    )


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    read = lambda path: (ROOT / path).read_text()
    mutants = {
        path.name: path.read_text()
        for path in (ROOT / FORMAL).glob(f"{STEM}*Mutant.cfg")
    }
    inputs = (
        read(MODEL),
        read(CONFIG),
        mutants,
        BINDINGS,
        read(TEST),
        read(MODULE),
        read(RUNNER),
    )
    payload = json.dumps(extract(*inputs), indent=2, sort_keys=True) + "\n"
    if args.self_test:
        self_test(inputs)
        print("atomic checkpoint obligation verifier: mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload)
    else:
        require(
            (ROOT / OUTPUT).read_text() == payload,
            "generated atomic checkpoint obligation registry is stale",
        )
        print("atomic checkpoint obligation registry: current")


if __name__ == "__main__":
    main()
