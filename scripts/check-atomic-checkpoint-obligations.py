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
        "memory_checkpoint_publishes_all_namespaces_together",
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
        "same_generation_plans_conflict_without_partial_visibility",
    ),
    (
        "GenerationAdvancesExactlyOnActivation",
        "executable_refinement",
        "lmdb_reopen_preserves_exact_catalog_and_known_root_activation",
    ),
    (
        "CommitUsesExactBase",
        "executable_refinement",
        "aba_root_reactivation_does_not_revive_an_old_plan",
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
        r"(?m)^#\[test\]\s*((?:#\[[^\n]+\]\s*)*)fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("
    )
    for match in pattern.finditer(source):
        attributes, name = match.groups()
        require(name not in targets, f"duplicate normal-suite test {name}")
        targets[name] = "#[ignore" not in attributes
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
        require(target in targets, f"missing normal-suite target: {symbol} -> {target}")
        require(
            targets[target], f"ignored-only normal-suite target: {symbol} -> {target}"
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
        "obligations": [
            {
                "kind": kind,
                "symbol": symbol,
                "mode": mapped[symbol][0],
                "test": mapped[symbol][1],
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
                    "memory_checkpoint_publishes_all_namespaces_together",
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
                (symbol, mode, "absent_test" if symbol == "TypeInvariant" else target)
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
