import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FORMAL = Path("formal/tlaplus/stack_safe_pda")
STEM = "RSpaceAffineImportLifecycle"
MODEL = FORMAL / f"{STEM}.tla"
CONFIG = FORMAL / f"{STEM}.cfg"
TESTS = Path("rspace++/src/rspace/state/instances/rspace_importer_store.rs")
AUTHORITY = Path("rspace++/src/rspace/state/rspace_state_authority.rs")
INSTANCES_MODULE = Path("rspace++/src/rspace/state/instances/mod.rs")
STATE_MODULE = Path("rspace++/src/rspace/state/mod.rs")
RUNNER = Path("scripts/check-stack-safe-pda-proofs.sh")
OUTPUT = Path("formal/affine_import_obligations.generated.json")

BINDINGS = {
    "TypeInvariant": ("proof_only", ()),
    "SealRequiresCompleteClosure": (
        "executable_refinement",
        ("empty_root_requires_a_complete_page_before_atomic_publication",),
    ),
    "PublicationRequiresConsumedSeal": (
        "proof_only",
        ("empty_root_requires_a_complete_page_before_atomic_publication",),
    ),
    "CurrentRootIsVerified": (
        "executable_refinement",
        (
            "empty_root_requires_a_complete_page_before_atomic_publication",
            "malformed_page_cannot_mutate_any_store_or_publish_a_root",
        ),
    ),
    "AtMostOneRacingActivationPublishes": (
        "executable_refinement",
        ("concurrent_activation_sessions_use_generation_cas_at_publication",),
    ),
    "ActivationGenerationIsCASProtected": (
        "executable_refinement",
        ("concurrent_activation_sessions_use_generation_cas_at_publication",),
    ),
    "PreserveCurrentNeverMovesAuthority": (
        "executable_refinement",
        ("preserve_current_publication_reports_no_activation_and_cannot_move_current",),
    ),
}

WITNESS_ASSERTIONS = {
    "empty_root_requires_a_complete_page_before_atomic_publication": (
        "Err(StateImportError::IncompletePagination(_))",
        "assert!(receipt.terminal)",
        ".seal().unwrap().publish(",
        "generation:1",
    ),
    "malformed_page_cannot_mutate_any_store_or_publish_a_root": (
        "Err(StateImportError::HistoryDigestMismatch{..})",
        'assert!(!importer.history_store.as_ref().expect("legacy history facet")'
        '.non_empty().expect("history facet reads"))',
        'assert!(!importer.value_store.as_ref().expect("legacy cold facet")'
        '.non_empty().expect("cold facet reads"))',
        "assert_eq!(importer.authority.current_verified_root().unwrap(),current_before)",
    ),
    "concurrent_activation_sessions_use_generation_cas_at_publication": (
        "letfirst=stage_empty_root(",
        "letstale=stage_empty_root(",
        "RootError::RootCatalogConflict{..}",
        "assert!(first_witness.authoritative_publication_observed())",
        "assert!(!stale_witness.authoritative_publication_observed())",
    ),
    "preserve_current_publication_reports_no_activation_and_cannot_move_current": (
        "StateImportPublicationMode::PreserveCurrent",
        "assert_eq!(publication.activation,None)",
        "assert_eq!(importer.authority.current_verified_root().unwrap().unwrap(),current)",
        ".contains_verified_root(",
    ),
}

MUTANTS = {
    f"{STEM}IncompleteMutant.cfg": (
        "IncompleteMutantSpec",
        "PublishIncompleteMutant",
        "PublicationRequiresConsumedSeal",
        "incomplete",
    ),
    f"{STEM}StaleActivationMutant.cfg": (
        "StaleActivationMutantSpec",
        "PublishStaleActivationMutant",
        "AtMostOneRacingActivationPublishes",
        "stale-activation",
    ),
    f"{STEM}PreserveMutationMutant.cfg": (
        "PreserveMutationMutantSpec",
        "MoveCurrentOnPreserveMutant",
        "PreserveCurrentNeverMovesAuthority",
        "preserve-mutation",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def sha256(source):
    return hashlib.sha256(source.encode()).hexdigest()


def directives(source, name):
    matches = re.findall(rf"(?m)^{name}[ \t]+([A-Za-z][A-Za-z0-9_]*)[ \t]*$", source)
    require(
        len(matches) == len(re.findall(rf"(?m)^{name}\b", source)),
        f"unsupported {name} directive",
    )
    require(len(matches) == len(set(matches)), f"duplicate {name} directive")
    return matches


def test_bodies(source):
    require(
        "#[cfg(test)]\nmod tests {" in source, "normal-suite unit tests are disabled"
    )
    pattern = re.compile(
        r"(?m)^    (?P<attrs>(?:#\[[^\n]+\]\n    )+)fn (?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\("
    )
    matches = [
        match for match in pattern.finditer(source) if "#[test]" in match.group("attrs")
    ]
    bodies = {}
    seen = set()
    for index, match in enumerate(matches):
        name = match.group("name")
        require(name not in seen, f"duplicate normal-suite test: {name}")
        seen.add(name)
        if "#[ignore" in match.group("attrs"):
            continue
        end = matches[index + 1].start() if index + 1 < len(matches) else len(source)
        body = source[match.start() : end]
        bodies[name] = re.sub(r"\s+", "", body)
    return bodies


def extract(
    model, config, mutants, tests, authority, instances_module, state_module, runner
):
    require(directives(config, "SPECIFICATION") == ["Spec"], "wrong base specification")
    require(
        not re.search(r"(?m)^(?:INVARIANTS|PROPERTIES|PROPERTY)\b", config),
        "unsupported base obligation directive",
    )
    invariants = directives(config, "INVARIANT")
    require(
        invariants == list(BINDINGS), "missing, extra, or reordered invariant binding"
    )
    require(
        "pub mod rspace_importer_store;" in instances_module,
        "importer unit module missing",
    )
    require("pub mod instances;" in state_module, "state instances module missing")
    require(
        "pub fn seal_import(" in authority
        and "verify_state_closure(" in authority
        and "pub fn publish_import(" in authority
        and "check_generation(current.as_ref(), expected_generation)?;" in authority,
        "authoritative affine seal/publication/CAS seam missing",
    )
    flattened_runner = re.sub(r"\\\n", " ", runner)
    require(
        f"{CONFIG.name} {MODEL.name}" in flattened_runner,
        "base affine model absent from normal proof suite",
    )
    bodies = test_bodies(tests)
    obligations = []
    for index, symbol in enumerate(invariants, 1):
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}[ \t]*==", model)) == 1,
            f"missing or duplicate model invariant: {symbol}",
        )
        mode, witnesses = BINDINGS[symbol]
        require(mode in {"proof_only", "executable_refinement"}, f"bad mode: {symbol}")
        for witness in witnesses:
            require(
                witness in bodies, f"missing enabled normal-suite witness: {witness}"
            )
            for assertion in WITNESS_ASSERTIONS[witness]:
                require(
                    re.sub(r"\s+", "", assertion) in bodies[witness],
                    f"witness assertion missing: {witness}: {assertion}",
                )
        obligations.append(
            {
                "id": f"AFF-TLA-{index:02d}",
                "kind": "invariant",
                "symbol": symbol,
                "mode": mode,
                "normal_suite_witnesses": list(witnesses),
            }
        )
    require(set(mutants) == set(MUTANTS), "missing or extra affine mutant config")
    controls = []
    for index, (filename, (specification, action, invariant, label)) in enumerate(
        MUTANTS.items(), 1
    ):
        source = mutants[filename]
        require(
            directives(source, "SPECIFICATION") == [specification]
            and directives(source, "INVARIANT") == [invariant],
            f"mutant specification or checked invariant changed: {filename}",
        )
        require(
            len(re.findall(rf"(?m)^{re.escape(specification)}[ \t]*==", model)) == 1
            and len(re.findall(rf"(?m)^{re.escape(action)}\([^\n)]*\)[ \t]*==", model))
            == 1,
            f"mutant action or specification missing: {filename}",
        )
        require(
            f"{label}:{filename}:{invariant}" in flattened_runner,
            f"mutant absent from normal proof suite: {filename}",
        )
        controls.append(
            {
                "id": f"AFF-MUT-{index:02d}",
                "config": str(FORMAL / filename),
                "config_sha256": sha256(source),
                "specification": specification,
                "action": action,
                "expected_invariant": invariant,
            }
        )
    return {
        "model": str(MODEL),
        "model_sha256": sha256(model),
        "config": str(CONFIG),
        "config_sha256": sha256(config),
        "normal_suite_test_module": str(TESTS),
        "normal_suite_source_sha256": sha256(tests),
        "runtime_authority": str(AUTHORITY),
        "runtime_authority_sha256": sha256(authority),
        "dedicated_rocq_sources": [],
        "dedicated_verus_sources": [],
        "obligations": obligations,
        "causal_mutants": controls,
    }


def reject(call):
    try:
        call()
    except RegistryError:
        return
    raise RegistryError("mutated affine registry input was accepted")


def self_test(inputs):
    model, config, mutants, tests, authority, instances_module, state_module, runner = (
        inputs
    )
    reject(
        lambda: extract(
            model,
            config + "INVARIANT Missing\n",
            mutants,
            tests,
            authority,
            instances_module,
            state_module,
            runner,
        )
    )
    reject(
        lambda: extract(
            model,
            config.replace("INVARIANT TypeInvariant\n", ""),
            mutants,
            tests,
            authority,
            instances_module,
            state_module,
            runner,
        )
    )
    reject(
        lambda: extract(
            model.replace("CurrentRootIsVerified ==", "Removed =="),
            config,
            mutants,
            tests,
            authority,
            instances_module,
            state_module,
            runner,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            mutants,
            tests.replace(
                "    #[test]\n    fn concurrent_activation_sessions_use_generation_cas_at_publication",
                "    #[ignore]\n    #[test]\n    fn concurrent_activation_sessions_use_generation_cas_at_publication",
            ),
            authority,
            instances_module,
            state_module,
            runner,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            mutants,
            tests
            + "\n    #[test]\n    fn concurrent_activation_sessions_use_generation_cas_at_publication() {}\n",
            authority,
            instances_module,
            state_module,
            runner,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            mutants,
            tests.replace(
                "RootError::RootCatalogConflict { .. }", "RootError::Other { .. }"
            ),
            authority,
            instances_module,
            state_module,
            runner,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            mutants,
            tests,
            authority.replace("verify_state_closure(", "removed_closure_check("),
            instances_module,
            state_module,
            runner,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            mutants,
            tests,
            authority,
            instances_module.replace("pub mod rspace_importer_store;", ""),
            state_module,
            runner,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            mutants,
            tests,
            authority,
            instances_module,
            state_module,
            runner.replace(f"{CONFIG.name} {MODEL.name}", ""),
        )
    )
    first = next(iter(MUTANTS))
    reject(
        lambda: extract(
            model,
            config,
            {key: value for key, value in mutants.items() if key != first},
            tests,
            authority,
            instances_module,
            state_module,
            runner,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            {
                **mutants,
                first: mutants[first].replace(
                    "PublicationRequiresConsumedSeal", "CurrentRootIsVerified"
                ),
            },
            tests,
            authority,
            instances_module,
            state_module,
            runner,
        )
    )


def main():
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    require(
        not list(
            (ROOT / "formal/rocq/stack_safe_pda/theories").glob("*AffineImport*.v")
        )
        and not list((ROOT / "formal/verus").glob("*affine*import*.rs")),
        "new dedicated Rocq or Verus affine-import source needs a binding",
    )
    mutants = {
        path.name: path.read_text()
        for path in (ROOT / FORMAL).glob(f"{STEM}*Mutant.cfg")
    }
    inputs = (
        (ROOT / MODEL).read_text(),
        (ROOT / CONFIG).read_text(),
        mutants,
        (ROOT / TESTS).read_text(),
        (ROOT / AUTHORITY).read_text(),
        (ROOT / INSTANCES_MODULE).read_text(),
        (ROOT / STATE_MODULE).read_text(),
        (ROOT / RUNNER).read_text(),
    )
    payload = json.dumps(extract(*inputs), indent=2, sort_keys=True) + "\n"
    if args.self_test:
        self_test(inputs)
        print("affine import registry mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload)
    else:
        require(
            (ROOT / OUTPUT).read_text() == payload, "affine import registry is stale"
        )
        print("affine import registry current")


if __name__ == "__main__":
    main()
