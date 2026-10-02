import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FORMAL = Path("formal/tlaplus/stack_safe_pda")
STEM = "RSpaceSplitIndexAuthority"
MODEL = FORMAL / f"{STEM}.tla"
CONFIG = FORMAL / f"{STEM}.cfg"
RUST = Path("rspace++/src/rspace/record_index.rs")
MODULE = Path("rspace++/src/rspace/mod.rs")
RUNNER = Path("scripts/check-stack-safe-pda-proofs.sh")
OUTPUT = Path("formal/split_index_authority_obligations.generated.json")

BINDINGS = {
    "TypeInvariant": ("proof_only", None),
    "PrivateStagingIsInvisible": (
        "executable_refinement",
        "distinct_identities_cannot_overwrite_one_canonical_primary_record",
    ),
    "SealRequiresCompleteStaging": ("proof_only", None),
    "PublishedAuthorityIsComplete": ("proof_only", None),
    "VisibleRootsAreAtomic": ("proof_only", None),
    "PublicationRequiresSealedAuthority": ("proof_only", None),
    "PublicationIsExactlyOnce": ("proof_only", None),
    "CountersCoPublishWithCanonicalRecords": ("proof_only", None),
    "IdentityBindingsCoPublishWithGroups": ("proof_only", None),
}
MUTANTS = {
    "RSpaceSplitIndexAuthorityPartialRootsMutant.cfg": (
        "PartialRootsMutantSpec",
        "PublishPartialRootsMutant",
        "VisibleRootsAreAtomic",
        "partial-roots",
    ),
    "RSpaceSplitIndexAuthorityIncompleteSealMutant.cfg": (
        "IncompleteSealMutantSpec",
        "SealIncompleteAuthorityMutant",
        "SealRequiresCompleteStaging",
        "incomplete-seal",
    ),
    "RSpaceSplitIndexAuthorityPublishBeforeSealMutant.cfg": (
        "PublishBeforeSealMutantSpec",
        "PublishBeforeSealMutant",
        "PublicationRequiresSealedAuthority",
        "publish-before-seal",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def directives(source, name):
    found = re.findall(rf"(?m)^{name}[ \t]+([A-Za-z][A-Za-z0-9_]*)[ \t]*$", source)
    require(
        len(found) == len(re.findall(rf"(?m)^{name}\b", source)),
        f"unsupported {name} directive",
    )
    require(len(found) == len(set(found)), f"duplicate {name} directive")
    return found


def test_body(source, name):
    found = list(
        re.finditer(rf"(?m)^    #\[test\][ \t]*\n    fn {re.escape(name)}\s*\(", source)
    )
    require(len(found) == 1, f"missing or duplicate normal-suite test: {name}")
    beginning = found[0].start()
    next_test = re.search(r"(?m)^    #\[test\]", source[found[0].end() :])
    ending = found[0].end() + next_test.start() if next_test else len(source)
    body = source[beginning:ending]
    require("#[ignore" not in body and "#[cfg" not in body, f"disabled test: {name}")
    return re.sub(r"\s+", "", body)


def extract(model, config, rust, module, runner, mutant_configs):
    runner = re.sub(r"\\\n", " ", runner)
    require(directives(config, "SPECIFICATION") == ["Spec"], "wrong base specification")
    require(
        not re.search(r"(?m)^(?:INVARIANTS|PROPERTIES|PROPERTY)\b", config),
        "unsupported base obligation",
    )
    invariants = directives(config, "INVARIANT")
    require(
        invariants == list(BINDINGS), "missing, extra, or reordered invariant binding"
    )
    require(
        "pub(crate) mod record_index;" in module
        and "#[cfg(test)]\nmod tests {" in rust,
        "normal-suite unit module is not registered",
    )
    require(
        "let mut replacement = self.clone();" in rust
        and "*self = replacement;" in rust,
        "private replacement and one-root publication are not bound",
    )
    require(
        f"{CONFIG.name} {MODEL.name}" in runner,
        "base TLC model is absent from normal proof suite",
    )
    obligations = []
    for index, symbol in enumerate(invariants, 1):
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}[ \t]*==", model)) == 1,
            f"missing or duplicate model invariant: {symbol}",
        )
        mode, target = BINDINGS[symbol]
        if target:
            body = test_body(rust, target)
            require(
                "leterror=store.set_group(" in body
                and ".unwrap_err();" in body
                and "assert!(Arc::ptr_eq(&before_group,store.groups.get(&prefix).unwrap()))"
                in body
                and "assert!(Arc::ptr_eq(&before_record,store.records.get(record_path.as_ref()).unwrap()))"
                in body
                and "assert!(Arc::ptr_eq(&before_identity,store.identities.get(identity_path.as_ref()).unwrap()))"
                in body
                and "assert_eq!(store.group_count(),before_group_count)" in body
                and "assert_eq!(store.group_values(&prefix),Some(vec![7])" in body
                and "assert_eq!(store.record_count(),1)" in body,
                f"normal-suite refinement assertions were removed: {target}",
            )
        obligations.append(
            {
                "id": f"SIDX-TLA-{index:02d}",
                "kind": "invariant",
                "symbol": symbol,
                "mode": mode,
                "test": target,
            }
        )
    require(set(mutant_configs) == set(MUTANTS), "missing or extra mutant config")
    controls = []
    for index, (filename, (specification, action, invariant, label)) in enumerate(
        MUTANTS.items(), 1
    ):
        source = mutant_configs[filename]
        require(
            directives(source, "SPECIFICATION") == [specification]
            and directives(source, "INVARIANT") == ["TypeInvariant", invariant],
            f"wrong mutant specification or invariant: {filename}",
        )
        require(
            len(re.findall(rf"(?m)^{re.escape(action)}[ \t]*==", model)) == 1
            and len(re.findall(rf"(?m)^{re.escape(specification)}[ \t]*==", model))
            == 1,
            f"mutant action or specification is missing: {filename}",
        )
        require(
            f"{label}:{filename}:{invariant}" in runner,
            f"mutant is absent from normal proof suite: {filename}",
        )
        controls.append(
            {
                "id": f"SIDX-MUT-{index:02d}",
                "config": str(FORMAL / filename),
                "config_sha256": hashlib.sha256(source.encode()).hexdigest(),
                "specification": specification,
                "action": action,
                "expected_invariant": invariant,
            }
        )
    return {
        "model": str(MODEL),
        "model_sha256": hashlib.sha256(model.encode()).hexdigest(),
        "config": str(CONFIG),
        "config_sha256": hashlib.sha256(config.encode()).hexdigest(),
        "runtime": str(RUST),
        "runtime_sha256": hashlib.sha256(rust.encode()).hexdigest(),
        "obligations": obligations,
        "causal_mutants": controls,
    }


def reject(call):
    try:
        call()
    except RegistryError:
        return
    raise RegistryError("mutated split-index input was accepted")


def self_test(inputs):
    model, config, rust, module, runner, mutants = inputs
    reject(
        lambda: extract(
            model, config + "INVARIANT Missing\n", rust, module, runner, mutants
        )
    )
    reject(
        lambda: extract(
            model,
            config.replace("INVARIANT TypeInvariant\n", ""),
            rust,
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model.replace("VisibleRootsAreAtomic ==", "Removed =="),
            config,
            rust,
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rust.replace("*self = replacement;", ""),
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rust.replace("assert_eq!(store.record_count(), 1);", ""),
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rust.replace("&before_identity,", "&before_record,", 1),
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rust,
            module,
            runner.replace(
                "RSpaceSplitIndexAuthority.cfg RSpaceSplitIndexAuthority.tla", ""
            ),
            mutants,
        )
    )
    first = next(iter(MUTANTS))
    reject(
        lambda: extract(
            model,
            config,
            rust,
            module,
            runner,
            {key: value for key, value in mutants.items() if key != first},
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rust,
            module,
            runner,
            {
                **mutants,
                first: mutants[first].replace("VisibleRootsAreAtomic", "TypeInvariant"),
            },
        )
    )


def main():
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    mutants = {
        path.name: path.read_text()
        for path in (ROOT / FORMAL).glob(f"{STEM}*Mutant.cfg")
    }
    inputs = tuple(
        (ROOT / path).read_text() for path in (MODEL, CONFIG, RUST, MODULE, RUNNER)
    ) + (mutants,)
    payload = json.dumps(extract(*inputs), indent=2, sort_keys=True) + "\n"
    if args.self_test:
        self_test(inputs)
        print("split-index authority mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload)
    else:
        require((ROOT / OUTPUT).read_text() == payload, "split-index registry is stale")
        print("split-index authority registry current")


if __name__ == "__main__":
    main()
