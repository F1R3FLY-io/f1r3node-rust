import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TLA = Path("formal/tlaplus/stack_safe_pda/RSpaceBagCommit.tla")
MUTANT_MODEL = TLA.with_name("RSpaceBagCommitMutants.tla")
CONFIG = TLA.with_suffix(".cfg")
ROCQ = Path("formal/rocq/stack_safe_pda/theories/RSpaceCommit.v")
RUST = Path("rspace++/tests/hot_store/accepted_match_formal_bindings.rs")
MODULE = Path("rspace++/src/rspace/hot_store.rs")
RUNNER = Path("scripts/check-stack-safe-pda-proofs.sh")
OUTPUT = Path("formal/accepted_match_obligations.generated.json")
MUTANTS = {
    "RSpaceBagCommitSplitPublicationMutant.cfg": (
        "SplitPublicationSpec",
        "SplitPublication",
        "PlayPublishesOneCompleteState",
    ),
    "RSpaceBagCommitEarlyEventMutant.cfg": (
        "EarlyEventSpec",
        "EarlyEvent",
        "EventPublicationFollowsCommit",
    ),
    "RSpaceBagCommitDropJoinMutant.cfg": (
        "DropJoinSpec",
        "DropJoin",
        "ContinuationAndJoinCommitTogether",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def unique(values, label):
    require(len(values) == len(set(values)), f"duplicate {label}")
    return values


def names(source, pattern, label):
    found = re.findall(pattern, source, re.MULTILINE)
    require(bool(found), f"missing {label}")
    return unique(found, label)


def bindings(source, array):
    match = re.search(rf"(?s)\blet {array} = \[(.*?)\n    \];", source)
    require(match is not None, f"missing {array} binding array")
    rows = []
    for block in re.findall(r"(?s)\bBinding\s*\{(.*?)\}", match.group(1)):
        fields = {}
        for field in ("id", "obligation", "target_name"):
            value = re.search(rf'\b{field}:\s*"([^"]+)"', block)
            require(value is not None, f"missing {field} in {array} binding")
            fields[field] = value.group(1)
        mode = re.search(r"\bproof_only:\s*(true|false)", block)
        require(mode is not None, f"missing proof_only in {array} binding")
        fields["mode"] = (
            "proof_only" if mode.group(1) == "true" else "executable_refinement"
        )
        target = re.search(r"\btarget:\s*([A-Za-z_][A-Za-z0-9_]*)\s*,", block)
        require(target is not None, f"missing target pointer in {array} binding")
        require(
            target.group(1) == fields["target_name"],
            f"target pointer mismatch: {fields['id']}",
        )
        rows.append(fields)
    require(bool(rows), f"empty {array} binding array")
    unique([row["id"] for row in rows], f"{array} binding ID")
    unique([row["obligation"] for row in rows], f"{array} obligation binding")
    return rows


def extract(model, config, rocq, rust, module, runner, mutant_sources):
    require(
        set(mutant_sources) == {MUTANT_MODEL.name, *MUTANTS},
        "missing or unregistered RSpaceBagCommit mutant source",
    )
    mutant_model = mutant_sources[MUTANT_MODEL.name]
    require(
        re.search(r"(?m)^EXTENDS RSpaceBagCommit\s*$", mutant_model) is not None,
        "mutant model does not extend accepted-match authority",
    )
    causal_mutants = []
    for filename, (specification, action, invariant) in MUTANTS.items():
        mutant_config = mutant_sources[filename]
        require(
            re.findall(r"(?m)^SPECIFICATION\s+(\w+)\s*$", mutant_config)
            == [specification],
            f"wrong mutant specification: {filename}",
        )
        require(
            re.findall(r"(?m)^INVARIANT\s+(\w+)\s*$", mutant_config) == [invariant],
            f"wrong mutant invariant: {filename}",
        )
        require(
            len(re.findall(rf"(?m)^{re.escape(action)}\s*==", mutant_model)) == 1,
            f"missing mutant action: {action}",
        )
        require(
            len(
                re.findall(
                    rf"(?m)^{re.escape(specification)}\s*==\s*Init\s*/\\\s*\[\]\[Next\s*\\/\s*{re.escape(action)}\]_vars\s*$",
                    mutant_model,
                )
            )
            == 1,
            f"mutant specification is not bound to {action}",
        )
        require(
            f"{filename}:RSpaceBagCommitMutants.tla:{invariant}" in runner,
            f"mutant is absent from normal proof suite: {filename}",
        )
        causal_mutants.append(
            {
                "id": f"ACM-MUT-{len(causal_mutants) + 1:02d}",
                "config": str(TLA.parent / filename),
                "config_sha256": hashlib.sha256(mutant_config.encode()).hexdigest(),
                "specification": specification,
                "action": action,
                "expected_invariant": invariant,
            }
        )
    require(
        not re.search(r"(?m)^(?:INVARIANTS|PROPERTIES)\b", config),
        "unsupported multi-line TLC obligation",
    )
    specifications = names(
        config, r"^SPECIFICATION[ \t]+([A-Za-z][A-Za-z0-9_]*)[ \t]*$", "specification"
    )
    require(
        len(re.findall(r"(?m)^SPECIFICATION\b", config)) == len(specifications)
        and specifications == ["Spec"],
        "unexpected TLC specification",
    )
    invariants = names(
        config, r"^INVARIANT[ \t]+([A-Za-z][A-Za-z0-9_]*)[ \t]*$", "TLC invariant"
    )
    require(
        len(re.findall(r"(?m)^INVARIANT\b", config)) == len(invariants),
        "unsupported TLC invariant directive",
    )
    require(not re.search(r"(?m)^PROPERTY\b", config), "unbound TLC property")
    theorems = names(rocq, r"^Theorem\s+([A-Za-z][A-Za-z0-9_]*)\s*:", "Rocq theorem")
    require(
        len(re.findall(r"(?m)^Theorem\b", rocq)) == len(theorems),
        "unsupported Rocq theorem declaration",
    )
    capstones = names(
        rocq, r"^Print Assumptions\s+([A-Za-z][A-Za-z0-9_]*)\.", "Rocq capstone"
    )
    require(
        len(re.findall(r"(?m)^Print Assumptions\b", rocq)) == len(capstones),
        "unsupported Rocq Print Assumptions target",
    )
    require(
        set(capstones) <= set(theorems), "Print Assumptions target is not a theorem"
    )
    require(
        re.search(
            r'(?s)#\[cfg\(test\)\]\s*#\[path = "\.\./\.\./tests/hot_store/accepted_match_formal_bindings\.rs"\]\s*mod accepted_match_formal_bindings_tests;',
            module,
        )
        is not None,
        "normal-suite module is not registered",
    )
    require(
        "RSpaceBagCommit.cfg RSpaceBagCommit.tla" in runner,
        "TLC model is absent from normal proof suite",
    )
    require(
        "theories/RSpaceCommit.v" in runner,
        "Rocq module is absent from normal proof suite",
    )
    require(
        re.search(
            r'grep -c .\^Closed under the global context\$.*?!= "6"', runner, re.DOTALL
        )
        is not None,
        "Rocq capstone count is not checked",
    )
    require(len(capstones) == 6, "unexpected Rocq capstone count")
    rows = {"tla": bindings(rust, "tla"), "rocq": bindings(rust, "rocq")}
    require(
        {row["obligation"] for row in rows["tla"]} == set(invariants),
        "missing or extra TLC binding",
    )
    require(
        {row["obligation"] for row in rows["rocq"]} == set(theorems),
        "missing or extra Rocq binding",
    )
    unique(
        [row["id"] for family in rows.values() for row in family],
        "cross-family binding ID",
    )
    for symbol in invariants:
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}\s*==", model)) == 1,
            f"missing or duplicate TLC definition: {symbol}",
        )
    for family in rows.values():
        for row in family:
            name = row["target_name"]
            targets = re.findall(
                rf"(?m)^#\[test\]\s*((?:#\[[^\n]+\]\s*)*)fn\s+{re.escape(name)}\s*\(",
                rust,
            )
            require(
                len(targets) == 1, f"missing or duplicate normal-suite test: {name}"
            )
            require("#[ignore" not in targets[0], f"ignored-only test: {name}")
            require("#[cfg" not in targets[0], f"feature-gated test: {name}")
    return {
        "model": str(TLA),
        "model_sha256": hashlib.sha256(model.encode()).hexdigest(),
        "config": str(CONFIG),
        "config_sha256": hashlib.sha256(config.encode()).hexdigest(),
        "rocq": str(ROCQ),
        "rocq_sha256": hashlib.sha256(rocq.encode()).hexdigest(),
        "normal_suite_test_module": str(RUST),
        "tlc_invariants": rows["tla"],
        "rocq_theorems": rows["rocq"],
        "rocq_print_assumptions": capstones,
        "causal_mutants": causal_mutants,
        "mutant_model": str(MUTANT_MODEL),
        "mutant_model_sha256": hashlib.sha256(mutant_model.encode()).hexdigest(),
    }


def reject(call):
    try:
        call()
    except RegistryError:
        return
    raise RegistryError("mutated accepted-match registry input was accepted")


def self_test(inputs):
    model, config, rocq, rust, module, runner, mutants = inputs
    reject(
        lambda: extract(
            model, config + "INVARIANT\n", rocq, rust, module, runner, mutants
        )
    )
    reject(
        lambda: extract(
            model,
            config + "INVARIANT MissingInvariant\n",
            rocq,
            rust,
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config.replace("INVARIANT TypeInvariant\n", ""),
            rocq,
            rust,
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config + "INVARIANT TypeInvariant\n",
            rocq,
            rust,
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model.replace("TypeInvariant ==", "RemovedInvariant =="),
            config,
            rocq,
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
            rocq.replace(
                "Theorem transition_is_deterministic", "Theorem removed_theorem"
            ),
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
            rocq + "\nPrint Assumptions transition_is_deterministic.\n",
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
            rocq,
            rust.replace('id: "ACM-TLA-01"', 'id: "ACM-TLA-02"'),
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rocq,
            rust.replace(
                'target_name: "bounded_multichannel_transition_refines_independent_sparse_count_oracle"',
                'target_name: "missing_test"',
                1,
            ),
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rocq,
            rust.replace(
                "#[test]\nfn bounded_multichannel_transition_refines_independent_sparse_count_oracle",
                '#[test]\n#[cfg(feature = "experiment")]\nfn bounded_multichannel_transition_refines_independent_sparse_count_oracle',
            ),
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rocq,
            rust,
            module.replace(
                '#[cfg(test)]\n#[path = "../../tests/hot_store/accepted_match_formal_bindings.rs"]',
                '#[cfg(feature = "experiment")]\n#[path = "../../tests/hot_store/accepted_match_formal_bindings.rs"]',
            ),
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rocq,
            rust.replace(
                "#[test]\nfn bounded_multichannel_transition_refines_independent_sparse_count_oracle",
                "#[test]\n#[ignore]\nfn bounded_multichannel_transition_refines_independent_sparse_count_oracle",
            ),
            module,
            runner,
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rocq,
            rust,
            module,
            runner.replace("RSpaceBagCommit.cfg RSpaceBagCommit.tla", ""),
            mutants,
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rocq,
            rust,
            module,
            runner,
            {**mutants, "RSpaceBagCommitNewMutant.cfg": ""},
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rocq,
            rust,
            module,
            runner,
            {key: value for key, value in mutants.items() if key != MUTANT_MODEL.name},
        )
    )
    first = next(iter(MUTANTS))
    reject(
        lambda: extract(
            model,
            config,
            rocq,
            rust,
            module,
            runner,
            {
                **mutants,
                first: mutants[first].replace(
                    "PlayPublishesOneCompleteState", "TypeInvariant"
                ),
            },
        )
    )
    reject(
        lambda: extract(
            model,
            config,
            rocq,
            rust,
            module,
            runner.replace(
                f"{first}:RSpaceBagCommitMutants.tla:PlayPublishesOneCompleteState", ""
            ),
            mutants,
        )
    )


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    mutant_paths = [
        MUTANT_MODEL,
        *(
            TLA.parent / path.name
            for path in (ROOT / TLA.parent).glob("RSpaceBagCommit*Mutant.cfg")
        ),
    ]
    mutant_sources = {path.name: (ROOT / path).read_text() for path in mutant_paths}
    inputs = tuple(
        (ROOT / path).read_text() for path in (TLA, CONFIG, ROCQ, RUST, MODULE, RUNNER)
    ) + (mutant_sources,)
    payload = json.dumps(extract(*inputs), indent=2, sort_keys=True) + "\n"
    if args.self_test:
        self_test(inputs)
        print("accepted-match obligation verifier: mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload)
    else:
        require(
            (ROOT / OUTPUT).read_text() == payload, "accepted-match registry is stale"
        )
        print("accepted-match obligation registry: current")


if __name__ == "__main__":
    main()
