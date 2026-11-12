import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/workspace_regression_obligations.generated.json"
BINDINGS = "shared/tests/workspace_regression_formal_bindings.rs"
CATALOG = (
    "docs/design/pathmap/workspace-regression-formal-invariant-catalog-2026-08-28.md"
)
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
TLA_DIR = "formal/tlaplus/stack_safe_pda"
FAMILIES = {
    "RR": {
        "array": "REPLAY_BINDINGS",
        "model": "RSpaceRecordReplayAuthorityLifecycle",
        "targets": (
            "rspace++/tests/state/atomic_state_authority.rs",
            "rholang/tests/epathmap_replay_equivalence_spec.rs",
        ),
        "count": 13,
    },
    "FFI": {
        "array": "FFI_BINDINGS",
        "model": "RSpaceStateImportFfiBoundary",
        "targets": (
            "rspace++/libs/rspace_rhotypes/tests/affine_state_import_ffi/mod.rs",
        ),
        "count": 13,
    },
    "PT": {
        "array": "PT_BINDINGS",
        "targets": ("rspace++/tests/persistence/generation_checkpoint_properties.rs",),
        "count": 9,
    },
}
CONTROLS = {
    "RSpaceRecordReplayAuthorityLifecycleSkipResetMutant.cfg": (
        "mutant",
        "ReplayPreparationUsesRecordedBase",
        "SkipResetMutantSpec",
    ),
    "RSpaceRecordReplayAuthorityLifecycleResetMovesAuthorityMutant.cfg": (
        "action_mutant",
        "ReplayResetLeavesAuthorityUnchanged",
        "ResetMovesAuthorityMutantSpec",
    ),
    "RSpaceRecordReplayAuthorityLifecycleForeignCommitWhileResetWitness.cfg": (
        "non_vacuity_witness",
        "NeverForeignCommitWhileReplayReset",
        "Spec",
    ),
    "RSpaceRecordReplayAuthorityLifecycleForeignCommitWhilePreparedWitness.cfg": (
        "non_vacuity_witness",
        "NeverForeignCommitWhileReplayPrepared",
        "Spec",
    ),
    "RSpaceRecordReplayAuthorityLifecycleReplayCommitAcrossForeignCommitWitness.cfg": (
        "non_vacuity_witness",
        "NeverReplayCommitAcrossForeignCommit",
        "Spec",
    ),
    "RSpaceStateImportFfiBoundaryExposeMutant.cfg": (
        "mutant",
        "CapabilityOutputRequiresEncodedSuccess",
        "ExposeMutantSpec",
    ),
    "RSpaceStateImportFfiBoundaryMisclassifyMutant.cfg": (
        "mutant",
        "PublicationResponseIsTruthful",
        "MisclassifyMutantSpec",
    ),
    "RSpaceStateImportFfiBoundaryEscapeMutant.cfg": (
        "mutant",
        "NoOrdinaryUnwindEscapesBoundary",
        "EscapeMutantSpec",
    ),
    "RSpaceStateImportFfiBoundaryUsableStagedOpenMutant.cfg": (
        "mutant",
        "StageEncodingFailurePoisonsMutatedOpen",
        "UsableStagedOpenMutantSpec",
    ),
    "RSpaceStateImportFfiBoundaryDuplicatePublicationMutant.cfg": (
        "mutant",
        "PublicationIsMonotoneAndUnique",
        "DuplicatePublicationMutantSpec",
    ),
    "RSpaceStateImportFfiBoundaryNoFairnessMutant.cfg": (
        "liveness_mutant",
        "EveryStartedCallReturns",
        "NoFairnessMutantSpec",
    ),
    "RSpaceStateImportFfiBoundaryTwoPublicationsWitness.cfg": (
        "non_vacuity_witness",
        "NeverTwoPublications",
        "Spec",
    ),
}
REGISTRATIONS = {
    "rspace++/src/rspace/state/rspace_state_authority.rs": (
        '#[path = "../../../tests/state/atomic_state_authority.rs"]',
        "mod atomic_state_authority_tests;",
    ),
    "rspace++/libs/rspace_rhotypes/src/lib.rs": (
        '#[path = "../tests/affine_state_import_ffi/mod.rs"]',
        "mod affine_state_import_ffi_tests;",
    ),
    "rspace++/src/rspace/persistence/mod.rs": (
        '#[path = "../../../tests/persistence/mod.rs"]',
        "mod tests;",
    ),
    "rspace++/tests/persistence/mod.rs": ("mod generation_checkpoint_properties;",),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def source_paths():
    paths = {"Cargo.toml", BINDINGS, CATALOG, RUNNER, *REGISTRATIONS}
    for family in FAMILIES.values():
        paths.update(family["targets"])
        if "model" in family:
            for extension in ("cfg", "tla"):
                paths.add(f"{TLA_DIR}/{family['model']}.{extension}")
    paths.update(f"{TLA_DIR}/{name}" for name in CONTROLS)
    return sorted(paths)


def parse_bindings(source, array, count):
    match = re.search(
        rf"(?s)\bconst {array}: \[Binding; (\d+)\] = \[(.*?)\n\];", source
    )
    require(match is not None, f"missing {array} array")
    require(int(match.group(1)) == count, f"wrong {array} declared count")
    pattern = re.compile(r'\(\s*"([^"]+)",\s*"([^"]+)",\s*"([^"]+)",\s*\),')
    rows = [list(found.groups()) for found in pattern.finditer(match.group(2))]
    require(
        not pattern.sub("", match.group(2)).strip(),
        f"unparsed {array} binding content",
    )
    require(len(rows) == count, f"missing or duplicate {array} binding")
    return rows


def parse_config(source, label):
    lines = [line.strip() for line in source.splitlines() if line.strip()]
    specifications = [
        line.split()[1] for line in lines if line.startswith("SPECIFICATION ")
    ]
    obligations = []
    for line in lines:
        if line.startswith(("INVARIANT ", "PROPERTY ")):
            parts = line.split()
            require(len(parts) == 2, f"malformed {label} obligation")
            obligations.append((parts[0], parts[1]))
        elif line.startswith(("INVARIANT", "PROPERTY", "SPECIFICATION")):
            require(line == "SPECIFICATION Spec", f"unsupported {label} directive")
    require(specifications == ["Spec"], f"unexpected {label} specification")
    require(len(obligations) == len(set(obligations)), f"duplicate {label} obligation")
    require(
        len({name for _, name in obligations}) == len(obligations),
        f"duplicate {label} symbol",
    )
    return obligations


def test_attributes(source, name):
    declarations = list(
        re.finditer(
            rf"(?m)^[ \t]*(?:async[ \t]+)?fn[ \t]+{re.escape(name)}[ \t]*\(", source
        )
    )
    require(len(declarations) == 1, f"missing or duplicate executable target: {name}")
    before = source[: declarations[0].start()].splitlines()
    attributes = []
    for line in reversed(before):
        line = line.strip()
        if not line.startswith("#["):
            break
        attributes.append(line)
    require(
        any(
            attr == "#[test]" or attr.startswith("#[tokio::test") for attr in attributes
        ),
        f"non-test executable target: {name}",
    )
    require(
        not any(attr.startswith("#[ignore") for attr in attributes),
        f"ignored-only target: {name}",
    )
    require(
        not any(attr.startswith(("#[cfg(", "#[cfg_attr(")) for attr in attributes),
        f"conditionally absent target: {name}",
    )


def extract(sources, control_names):
    rust = sources[BINDINGS]
    catalog = sources[CATALOG]
    runner = re.sub(r"\\\n", " ", sources[RUNNER])
    catalog_rows = {}
    for match in re.finditer(r"(?m)^\|\s*((?:RR|FFI|PT)-\d{2})\s*\|([^\n]+)$", catalog):
        name = match.group(1)
        require(name not in catalog_rows, f"duplicate catalog row: {name}")
        catalog_rows[name] = match.group(2)
    test_attributes(
        rust, "every_workspace_regression_formal_obligation_has_an_executable_property"
    )
    workspace = re.search(r"(?ms)^members[ \t]*=[ \t]*\[(.*?)\]", sources["Cargo.toml"])
    require(workspace is not None, "normal workspace member list missing")
    members = set(re.findall(r'"([^"]+)"', workspace.group(1)))
    require(
        {"shared", "rholang", "rspace++", "rspace++/libs/rspace_rhotypes"} <= members,
        "binding target crate absent from the normal workspace",
    )
    require(
        "autotests = false" not in sources["shared/Cargo.toml"],
        "shared integration tests disabled",
    )
    require(
        "autotests = false" not in sources["rholang/Cargo.toml"],
        "rholang integration tests disabled",
    )
    for path, markers in REGISTRATIONS.items():
        for marker in markers:
            require(
                marker in sources[path],
                f"normal-suite registration missing: {path}: {marker}",
            )
    require(
        set(control_names) == set(CONTROLS),
        "missing or unregistered workspace mutant/witness configuration",
    )
    families = {}
    all_ids = set()
    for prefix, family in FAMILIES.items():
        rows = parse_bindings(rust, family["array"], family["count"])
        expected_ids = [
            f"{prefix}-{number:02d}" for number in range(1, family["count"] + 1)
        ]
        require(
            [row[0] for row in rows] == expected_ids,
            f"missing, duplicate, or reordered {prefix} ID",
        )
        all_ids.update(expected_ids)
        if "model" in family:
            stem = family["model"]
            config_path = f"{TLA_DIR}/{stem}.cfg"
            model_path = f"{TLA_DIR}/{stem}.tla"
            obligations = parse_config(sources[config_path], prefix)
            require(
                [row[1] for row in rows] == [symbol for _, symbol in obligations],
                f"missing, extra, or stale {prefix} formal binding",
            )
            require(
                f"{stem}.cfg" in runner and f"{stem}.tla" in runner,
                f"{prefix} absent from proof runner",
            )
            require(
                len(re.findall(r"(?m)^Spec[ \t]*==", sources[model_path])) == 1,
                f"missing or duplicate {prefix} model specification",
            )
            for _, symbol in obligations:
                require(
                    len(
                        re.findall(
                            rf"(?m)^{re.escape(symbol)}[ \t]*==", sources[model_path]
                        )
                    )
                    == 1,
                    f"missing or duplicate {prefix} model definition: {symbol}",
                )
        else:
            obligations = [("EXECUTABLE_ONLY", row[1]) for row in rows]
            require(all(row[1] == row[2] for row in rows), "PT symbol/target mismatch")
        entries = []
        for (name, symbol, target), (kind, _) in zip(rows, obligations):
            require(name in catalog_rows, f"missing catalog row: {name}")
            require(
                f"`{symbol}`" in catalog_rows[name], f"stale catalog symbol: {name}"
            )
            if prefix == "PT":
                require(
                    f"`{target}`" in catalog_rows[name], f"stale catalog target: {name}"
                )
            matches = [
                path
                for path in family["targets"]
                if re.search(
                    rf"(?m)^[ \t]*(?:async[ \t]+)?fn[ \t]+{re.escape(target)}[ \t]*\(",
                    sources[path],
                )
            ]
            require(
                len(matches) == 1, f"missing or duplicate executable target: {target}"
            )
            test_attributes(sources[matches[0]], target)
            entries.append(
                {
                    "id": name,
                    "classification": "executable_only"
                    if prefix == "PT"
                    else "executable_refinement",
                    "formal_kind": kind,
                    "symbol": symbol,
                    "target": target,
                    "target_source": matches[0],
                }
            )
        if prefix == "PT":
            require(
                len({row[2] for row in rows}) == len(rows),
                "duplicate PT executable target",
            )
        families[prefix] = entries
    require(set(catalog_rows) == all_ids, "extra or missing workspace catalog ID")
    controls = []
    for name, (kind, symbol, specification) in sorted(CONTROLS.items()):
        path = f"{TLA_DIR}/{name}"
        contents = sources[path]
        require(
            f"SPECIFICATION {specification}" in contents,
            f"stale control specification: {name}",
        )
        stem = (
            "RSpaceRecordReplayAuthorityLifecycle"
            if name.startswith("RSpaceRecordReplayAuthorityLifecycle")
            else "RSpaceStateImportFfiBoundary"
        )
        require(
            len(
                re.findall(
                    rf"(?m)^{re.escape(specification)}[ \t]*==",
                    sources[f"{TLA_DIR}/{stem}.tla"],
                )
            )
            == 1,
            f"missing or duplicate control specification: {name}",
        )
        directive = (
            "PROPERTY" if kind in ("liveness_mutant", "action_mutant") else "INVARIANT"
        )
        require(
            re.search(rf"(?m)^{directive} {re.escape(symbol)}$", contents) is not None,
            f"stale control obligation: {name}",
        )
        if kind == "non_vacuity_witness":
            require(
                parse_config(contents, name) == [("INVARIANT", symbol)],
                f"witness target mismatch: {name}",
            )
            require(
                len(
                    re.findall(
                        rf"(?m)^{re.escape(symbol)}[ \t]*==",
                        sources[f"{TLA_DIR}/{stem}.tla"],
                    )
                )
                == 1,
                f"missing or duplicate witness statement: {name}",
            )
            base_obligations = parse_config(sources[f"{TLA_DIR}/{stem}.cfg"], stem)
            require(
                symbol not in {obligation for _, obligation in base_obligations},
                f"witness statement is a base obligation: {name}",
            )
        require(name in runner, f"control absent from proof runner: {name}")
        if kind == "liveness_mutant":
            expected_output = "Temporal properties were violated"
        elif kind == "action_mutant":
            expected_output = f"Action property {symbol} is violated"
        else:
            expected_output = f"Invariant {symbol} is violated"
        if name.startswith("RSpaceStateImportFfiBoundary") and kind == "mutant":
            require(
                f"{name}:{symbol}" in runner
                and "Invariant ${state_import_ffi_invariant} is violated" in runner,
                f"control outcome unchecked in proof runner: {name}",
            )
        else:
            require(
                expected_output in runner,
                f"control outcome unchecked in proof runner: {name}",
            )
        controls.append(
            {
                "config": path,
                "kind": kind,
                "expected_obligation": symbol,
                "specification": specification,
            }
        )
    for mode in ("--check", "--self-test"):
        require(
            f'check-workspace-regression-obligations.py" {mode}' in runner,
            f"registry {mode} absent from proof runner",
        )
    return {
        "families": families,
        "controls": controls,
        "source_sha256": {
            path: hashlib.sha256(source.encode("utf-8")).hexdigest()
            for path, source in sorted(sources.items())
            if path != RUNNER
        },
    }


def reject(mutated, controls, expected):
    try:
        extract(mutated, controls)
    except RegistryError as error:
        require(expected in str(error), f"wrong mutation rejection: {error}")
        return
    raise RegistryError(f"mutation unexpectedly passed: {expected}")


def self_test(sources, controls):
    altered = dict(sources)
    altered[BINDINGS] = sources[BINDINGS].replace('"RR-01"', '"RR-02"', 1)
    reject(altered, controls, "missing, duplicate, or reordered RR ID")
    altered = dict(sources)
    altered[BINDINGS] = sources[BINDINGS].replace(
        '"CurrentRootIsVerified"', '"StaleFormalSymbol"', 1
    )
    reject(altered, controls, "missing, extra, or stale RR formal binding")
    altered = dict(sources)
    path = f"{TLA_DIR}/RSpaceStateImportFfiBoundary.cfg"
    altered[path] = sources[path].replace("INVARIANT TypeInvariant\n", "", 1)
    reject(altered, controls, "missing, extra, or stale FFI formal binding")
    altered = dict(sources)
    altered[path] = sources[path] + "INVARIANT TypeInvariant\n"
    reject(altered, controls, "duplicate FFI obligation")
    altered = dict(sources)
    altered[CATALOG] = re.sub(
        r"(?m)^\| RR-01 \|[^\n]*\n", "", sources[CATALOG], count=1
    )
    reject(altered, controls, "missing catalog row: RR-01")
    altered = dict(sources)
    path = "rspace++/tests/state/atomic_state_authority.rs"
    altered[path] = sources[path].replace(
        "#[test]\nfn memory_checkpoint_publishes_all_namespaces_together",
        "#[test]\n#[ignore]\nfn memory_checkpoint_publishes_all_namespaces_together",
        1,
    )
    reject(altered, controls, "ignored-only target")
    altered = dict(sources)
    path = "rspace++/tests/persistence/mod.rs"
    altered[path] = sources[path].replace(
        "mod generation_checkpoint_properties;", "", 1
    )
    reject(altered, controls, "normal-suite registration missing")
    altered = dict(sources)
    altered["Cargo.toml"] = sources["Cargo.toml"].replace(
        '"shared",', '"shared_missing",', 1
    )
    reject(altered, controls, "binding target crate absent from the normal workspace")
    altered = dict(sources)
    altered[BINDINGS] = sources[BINDINGS].replace(
        '"arbitrary_checkpoint_descriptor_bytes_are_total"',
        '"missing_parser_target"',
        1,
    )
    reject(altered, controls, "PT symbol/target mismatch")
    altered = dict(sources)
    altered[RUNNER] = sources[RUNNER].replace(
        'python3 "$repo_root/scripts/check-workspace-regression-obligations.py" --check',
        "",
        1,
    )
    reject(altered, controls, "registry --check absent from proof runner")
    altered = dict(sources)
    reject(
        altered,
        controls - {"RSpaceStateImportFfiBoundaryExposeMutant.cfg"},
        "missing or unregistered",
    )
    replay_model = f"{TLA_DIR}/RSpaceRecordReplayAuthorityLifecycle.tla"
    replay_config = f"{TLA_DIR}/RSpaceRecordReplayAuthorityLifecycle.cfg"
    reset_moves = (
        f"{TLA_DIR}/RSpaceRecordReplayAuthorityLifecycleResetMovesAuthorityMutant.cfg"
    )
    reset_witness = (
        "RSpaceRecordReplayAuthorityLifecycleForeignCommitWhileResetWitness.cfg"
    )
    across_witness = (
        f"{TLA_DIR}/"
        "RSpaceRecordReplayAuthorityLifecycleReplayCommitAcrossForeignCommitWitness.cfg"
    )
    altered = dict(sources)
    altered[reset_moves] = sources[reset_moves].replace(
        "PROPERTY ReplayResetLeavesAuthorityUnchanged",
        "INVARIANT ReplayResetLeavesAuthorityUnchanged",
        1,
    )
    reject(altered, controls, "stale control obligation")
    altered = dict(sources)
    altered[RUNNER] = sources[RUNNER].replace(
        "Action property ReplayResetLeavesAuthorityUnchanged is violated", "", 1
    )
    reject(altered, controls, "control outcome unchecked in proof runner")
    altered = dict(sources)
    reject(altered, controls - {reset_witness}, "missing or unregistered")
    altered = dict(sources)
    altered[replay_model] = sources[replay_model].replace(
        "NeverForeignCommitWhileReplayPrepared ==",
        "NeverForeignCommitWhileReplayPreparedRenamed ==",
        1,
    )
    reject(altered, controls, "missing or duplicate witness statement")
    altered = dict(sources)
    altered[across_witness] = sources[across_witness] + "INVARIANT TypeInvariant\n"
    reject(altered, controls, "witness target mismatch")
    altered = dict(sources)
    altered[RUNNER] = sources[RUNNER].replace(
        "Invariant NeverReplayCommitAcrossForeignCommit is violated", "", 1
    )
    reject(altered, controls, "control outcome unchecked in proof runner")
    altered = dict(sources)
    altered[replay_config] = sources[replay_config].replace(
        "PROPERTY ReplayLifecycleEventuallyCompletes",
        "INVARIANT NeverForeignCommitWhileReplayReset",
        1,
    )
    altered[BINDINGS] = sources[BINDINGS].replace(
        '"ReplayLifecycleEventuallyCompletes"',
        '"NeverForeignCommitWhileReplayReset"',
        1,
    )
    altered[CATALOG] = sources[CATALOG].replace(
        "| RR-13 | `ReplayLifecycleEventuallyCompletes` |",
        "| RR-13 | `NeverForeignCommitWhileReplayReset` |",
        1,
    )
    reject(altered, controls, "witness statement is a base obligation")


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    paths = source_paths() + ["shared/Cargo.toml", "rholang/Cargo.toml"]
    sources = {path: (ROOT / path).read_text(encoding="utf-8") for path in paths}
    controls = {
        path.name
        for stem in (
            "RSpaceRecordReplayAuthorityLifecycle",
            "RSpaceStateImportFfiBoundary",
        )
        for path in (ROOT / TLA_DIR).glob(f"{stem}*Mutant.cfg")
    }
    for stem in (
        "RSpaceRecordReplayAuthorityLifecycle",
        "RSpaceStateImportFfiBoundary",
    ):
        controls.update(
            path.name for path in (ROOT / TLA_DIR).glob(f"{stem}*Witness.cfg")
        )
    payload = json.dumps(extract(sources, controls), indent=2, sort_keys=True) + "\n"
    if args.self_test:
        self_test(sources, controls)
        print("workspace regression registry: mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload, encoding="utf-8")
        print("workspace regression registry: written")
    else:
        require(
            (ROOT / OUTPUT).read_text(encoding="utf-8") == payload,
            "workspace regression registry is stale",
        )
        print("workspace regression registry: current")


if __name__ == "__main__":
    main()
