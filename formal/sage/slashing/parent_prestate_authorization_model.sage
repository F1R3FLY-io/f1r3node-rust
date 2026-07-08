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


def recoverable_rejected(rejected_hashes, own_hashes, current_evidence_hashes):
    covered = set(own_hashes)
    current = set(current_evidence_hashes)
    out = []
    seen = set()
    for h in sorted(rejected_hashes):
        if h in covered or h in seen or h not in current:
            continue
        seen.add(h)
        out.append(h)
    return out


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


def check_recovery_cases():
    rejected = ["h1", "h2", "h2", "h3"]
    own = ["h1"]
    current = ["h2"]
    recovered = recoverable_rejected(rejected, own, current)
    stale_recovered = "h3" in recovered
    own_recovered = "h1" in recovered
    duplicate_count = recovered.count("h2")
    holds = recovered == ["h2"] and not stale_recovered and not own_recovered and duplicate_count == 1
    return {
        "model": "sage_parent_prestate_recovered_slash_authorization",
        "recovered": recovered,
        "holds": holds,
        "stale_recovered": stale_recovered,
        "own_recovered": own_recovered,
        "duplicate_count": duplicate_count,
    }


def run_analysis(max_bond):
    auth = check_authorization_cases(max_bond)
    recovery = check_recovery_cases()
    failures = list(auth["failures"])
    if not recovery["holds"]:
        failures.append({"property": "recovery_current_evidence"})
    return {
        "summaries": [
            {
                "model": "sage_parent_prestate_authorization",
                "max_bond": max_bond,
                "cases": auth["cases"],
                "failures": len(failures),
            }
        ],
        "recovery": recovery,
        "failures": failures,
    }


def self_test():
    result = run_analysis(3)
    if result["failures"]:
        raise AssertionError("parent-pre-state authorization model failed")
    if result["recovery"]["recovered"] != ["h2"]:
        raise AssertionError("recovered slash evidence filter changed")
    return result


def print_summary(result):
    for summary in result["summaries"]:
        print(
            "model={model} max_bond={max_bond} cases={cases} failures={failures}".format(
                **summary
            )
        )
    recovery = result["recovery"]
    print(
        "recovery holds={holds} recovered={recovered} duplicate_count={duplicate_count}".format(
            **recovery
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
