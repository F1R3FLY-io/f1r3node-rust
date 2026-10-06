import argparse
import json
import sys

from sage.all import Integer, cartesian_product


def authorized(parent_bond, evidence_epoch, target_epoch, current_epoch, invalid):
    return (
        invalid
        and Integer(evidence_epoch) == Integer(current_epoch)
        and Integer(target_epoch) == Integer(current_epoch)
        and Integer(parent_bond) > 0
    )


def execute_slash(execution_bond, vault):
    execution_bond = Integer(execution_bond)
    vault = Integer(vault)
    if execution_bond <= 0:
        return Integer(0), vault, Integer(0)
    return Integer(0), vault + execution_bond, execution_bond


def canonical_slash_candidates(evidence, bonds, current_epoch):
    selected = {}
    for validator, evidence_epoch, invalid_hash, invalid in evidence:
        if not authorized(
            bonds.get(validator, 0),
            evidence_epoch,
            evidence_epoch,
            current_epoch,
            invalid,
        ):
            continue
        key = (validator, evidence_epoch)
        selected[key] = min(invalid_hash, selected.get(key, invalid_hash))
    return sorted((validator, epoch, invalid_hash) for (validator, epoch), invalid_hash in selected.items())


def check_authorization_cases(max_bond):
    failures = []
    cases = 0
    for parent_bond, ambient_bond, execution_bond in cartesian_product(
        [range(max_bond + 1), range(max_bond + 1), range(max_bond + 1)]
    ):
        for evidence_epoch, target_epoch, current_epoch, invalid in cartesian_product(
            [range(2), range(2), range(2), [False, True]]
        ):
            cases += 1
            auth = authorized(parent_bond, evidence_epoch, target_epoch, current_epoch, invalid)
            auth_with_ambient_zero = authorized(
                parent_bond, evidence_epoch, target_epoch, current_epoch, invalid
            )
            if ambient_bond == 0 and auth != auth_with_ambient_zero:
                failures.append({"property": "ambient_changed_authorization"})
            if parent_bond == 0 and auth:
                failures.append({"property": "parent_zero_authorized"})
            if (
                parent_bond > 0
                and ambient_bond == 0
                and invalid
                and evidence_epoch == current_epoch
                and target_epoch == current_epoch
                and not auth
            ):
                failures.append({"property": "ambient_zero_blocked_parent_positive"})
            post_bond, post_vault, transferred = execute_slash(execution_bond, 0)
            if execution_bond == 0 and (post_vault != 0 or transferred != 0 or post_bond != 0):
                failures.append({"property": "zero_execution_bond_transferred"})
    return {"cases": cases, "failures": failures}


def check_candidate_scan_cases():
    evidence = [
        ("v1", 0, "h2", True),
        ("v1", 0, "h1", True),
        ("v1", 0, "h2", True),
        ("v2", 0, "h3", True),
        ("v3", 1, "h4", True),
    ]
    bonds = {"v1": 10, "v2": 0, "v3": 10}
    merge_rejected_hints = [("v1", 0, "h2"), ("v2", 0, "h3")]
    selected = canonical_slash_candidates(evidence, bonds, 0)
    selected_with_hints = canonical_slash_candidates(evidence, bonds, 0)
    target_keys = [(validator, epoch) for validator, epoch, _ in selected]
    holds = (
        selected == [("v1", 0, "h1")]
        and selected_with_hints == selected
        and len(target_keys) == len(set(target_keys))
        and all(candidate[0] != "v2" for candidate in selected)
    )
    return {
        "model": "sage_parent_prestate_canonical_slash_selection",
        "selected": selected,
        "merge_rejected_hints": merge_rejected_hints,
        "holds": holds,
        "target_keys_unique": len(target_keys) == len(set(target_keys)),
        "zero_bond_excluded": all(candidate[0] != "v2" for candidate in selected),
    }


def run_analysis(max_bond):
    auth = check_authorization_cases(max_bond)
    selection = check_candidate_scan_cases()
    failures = list(auth["failures"])
    if not selection["holds"]:
        failures.append({"property": "canonical_slash_candidate_selection"})
    return {
        "summaries": [
            {
                "model": "sage_parent_prestate_authorization",
                "max_bond": max_bond,
                "cases": auth["cases"],
                "failures": len(failures),
            }
        ],
        "selection": selection,
        "failures": failures,
    }


def self_test():
    result = run_analysis(3)
    if result["failures"]:
        raise AssertionError("parent-pre-state authorization model failed")
    if result["selection"]["selected"] != [("v1", 0, "h1")]:
        raise AssertionError("canonical slash selection changed")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print(
            "model={model} max_bond={max_bond} cases={cases} failures={failures}".format(
                **summary
            )
        )
    selection = result["selection"]
    print(
        "selection holds={holds} selected={selected} target_keys_unique={target_keys_unique} zero_bond_excluded={zero_bond_excluded}".format(
            **selection
        )
    )
    if result["failures"]:
        print("first_failure property={property}".format(**result["failures"][0]))


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for parent-pre-state slash authorization")
    parser.add_argument("--max-bond", type=int, default=3)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    if args.max_bond < 0:
        parser.error("--max-bond must be non-negative")

    result = self_test() if args.self_test else run_analysis(args.max_bond)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
