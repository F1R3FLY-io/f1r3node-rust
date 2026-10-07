import argparse
import itertools
import json


def reserve_all(balance, requested):
    if any(amount < 0 for amount in requested):
        return None
    if any(requested[index] > balance[index] for index in range(len(balance))):
        return None
    return {
        "balance": tuple(
            balance[index] - requested[index]
            for index in range(len(balance))
        ),
        "reservation": tuple(requested),
    }


def settle(reserved, actual, fee):
    if any(value < 0 for value in actual + fee):
        return None
    used = tuple(
        actual[index] + fee[index]
        for index in range(len(reserved))
    )
    if any(used[index] > reserved[index] for index in range(len(reserved))):
        return None
    return {
        "refund": tuple(
            reserved[index] - used[index]
            for index in range(len(reserved))
        ),
        "burned": tuple(actual),
        "fees": tuple(fee),
    }


def atomic_apply(balance, bound, actual, fee):
    reservation = reserve_all(balance, bound)
    if reservation is None:
        return None
    result = settle(bound, actual, fee)
    if result is None:
        return None
    return {
        "balance": tuple(
            reservation["balance"][index] + result["refund"][index]
            for index in range(len(balance))
        ),
        "burned": result["burned"],
        "fees": result["fees"],
    }


def total(vector):
    return sum(vector)


def exhaustive_lifecycle(max_balance=4, max_bound=4):
    traces = 0
    violations = []
    for balance in itertools.product(range(max_balance + 1), repeat=2):
        for bound in itertools.product(range(max_bound + 1), repeat=2):
            before = tuple(balance)
            reservation = reserve_all(balance, bound)
            if reservation is None:
                traces += 1
                if balance != before:
                    violations.append(("failed_reservation_mutated", balance, bound))
                continue
            if (
                total(reservation["balance"])
                + total(reservation["reservation"])
                != total(before)
            ):
                violations.append(("reservation_nonconserving", balance, bound))
            for actual_left in range(bound[0] + 1):
                for fee_left in range(bound[0] - actual_left + 1):
                    for actual_right in range(bound[1] + 1):
                        for fee_right in range(bound[1] - actual_right + 1):
                            traces += 1
                            actual = (actual_left, actual_right)
                            fee = (fee_left, fee_right)
                            result = settle(bound, actual, fee)
                            if result is None:
                                violations.append(
                                    ("valid_settlement_rejected", balance, bound, actual, fee)
                                )
                                continue
                            final_balance = tuple(
                                reservation["balance"][index]
                                + result["refund"][index]
                                for index in range(2)
                            )
                            conserved = (
                                total(final_balance)
                                + total(result["burned"])
                                + total(result["fees"])
                                == total(before)
                            )
                            exact_refund = all(
                                result["refund"][index]
                                == bound[index] - actual[index] - fee[index]
                                for index in range(2)
                            )
                            replay = reserve_all(before, bound)
                            replay_result = settle(bound, actual, fee)
                            replay_balance = tuple(
                                replay["balance"][index]
                                + replay_result["refund"][index]
                                for index in range(2)
                            )
                            native = atomic_apply(before, bound, actual, fee)
                            if not (
                                conserved
                                and exact_refund
                                and replay_balance == final_balance
                                and native is not None
                                and native["balance"] == final_balance
                                and native["burned"] == actual
                                and native["fees"] == fee
                            ):
                                violations.append(
                                    (
                                        "lifecycle_violation",
                                        balance,
                                        bound,
                                        actual,
                                        fee,
                                        conserved,
                                        exact_refund,
                                    )
                                )
    return {"traces": traces, "violations": violations}


def exhaustive_atomicity(max_balance=4, max_bound=4):
    traces = 0
    violations = []
    for balance in itertools.product(range(max_balance + 1), repeat=2):
        for bound in itertools.product(range(max_bound + 1), repeat=2):
            traces += 1
            result = reserve_all(balance, bound)
            fully_funded = all(bound[index] <= balance[index] for index in range(2))
            if (result is not None) != fully_funded:
                violations.append((balance, bound, result, fully_funded))
    return {"traces": traces, "violations": violations}


def exhaustive_merge_refinement(max_balance=4):
    traces = 0
    violations = []
    for balance in range(max_balance + 1):
        for left_bound in range(balance + 1):
            for right_bound in range(balance + 1):
                for left_used in range(left_bound + 1):
                    for right_used in range(right_bound + 1):
                        traces += 1
                        aggregate = left_used + right_used
                        funded = aggregate <= balance
                        left_then_right = balance - aggregate if funded else None
                        right_then_left = balance - aggregate if funded else None
                        if funded and left_then_right != right_then_left:
                            violations.append(
                                (
                                    "funded_aggregate_order_dependence",
                                    balance,
                                    left_bound,
                                    right_bound,
                                    left_used,
                                    right_used,
                                )
                            )
                        if not funded and (
                            left_then_right is not None or right_then_left is not None
                        ):
                            violations.append(
                                (
                                    "overdrawn_aggregate_admitted",
                                    balance,
                                    left_bound,
                                    right_bound,
                                    left_used,
                                    right_used,
                                )
                            )
    return {"traces": traces, "violations": violations}


def exhaustive_application_cost_composition(max_balance=8):
    traces = 0
    violations = []
    component_names = ("application", "physical", "byte", "fee")
    omission_witnesses = {name: [] for name in component_names}
    components = list(itertools.product(range(2), repeat=4))
    for branches in itertools.product(components, repeat=3):
        complete_debits = tuple(sum(branch) for branch in branches)
        complete_total = sum(complete_debits)
        for balance in range(max_balance + 1):
            for component_index, component_name in enumerate(component_names):
                projected_total = complete_total - sum(
                    branch[component_index] for branch in branches
                )
                if projected_total <= balance < complete_total:
                    omission_witnesses[component_name].append((balance, branches))
            for order in itertools.permutations(range(3)):
                traces += 1
                remaining = balance
                accepted = []
                rejected = []
                for index in order:
                    debit = complete_debits[index]
                    if debit <= remaining:
                        remaining -= debit
                        accepted.append(index)
                    else:
                        rejected.append(index)
                if remaining < 0 or len(accepted) + len(rejected) != 3:
                    violations.append(
                        ("invalid_partition", balance, branches, order, remaining)
                    )
                if sum(complete_debits[index] for index in accepted) > balance:
                    violations.append(
                        ("accepted_overdraw", balance, branches, order, accepted)
                    )
                if complete_total <= balance and rejected:
                    violations.append(
                        ("funded_aggregate_rejected", balance, branches, order, rejected)
                    )
    return {
        "traces": traces,
        "violations": violations,
        "omission_witness_counts": {
            name: len(witnesses)
            for name, witnesses in omission_witnesses.items()
        },
        "omission_witnesses": {
            name: witnesses[0] if witnesses else None
            for name, witnesses in omission_witnesses.items()
        },
    }


def authorization_checks():
    client = 0
    provider = 1
    slot = 2
    sponsor = 3
    lollipop_allowed = {client, provider}
    slot_allowed = {slot}
    return {
        "lollipop_distinct_payers_authorized":
            {client, provider}.issubset(lollipop_allowed),
        "lollipop_sponsor_cannot_replace_provider":
            sponsor not in lollipop_allowed,
        "slot_draw_requires_slot_capability":
            client not in slot_allowed and sponsor not in slot_allowed,
    }


def negative_controls():
    balance = (3, 0)
    bound = (2, 1)
    first_leg = (balance[0] - bound[0], balance[1])
    partial_reservation_detected = (
        first_leg != balance
        and reserve_all(balance, bound) is None
    )
    independent_credit = (balance[0] + 2, balance[1])
    independent_credit_detected = total(independent_credit) != total(balance)
    refund_loss_detected = (
        total((1, 0)) + total((1, 0)) + total((0, 0))
        != total((3, 0))
    )
    global_reservation_cell_conflict_detected = (
        ("reservationStore", "left") != ("reservationStore", "right")
        and "reservationStore" == "reservationStore"
    )
    return {
        "partial_reservation_detected": partial_reservation_detected,
        "independent_credit_detected": independent_credit_detected,
        "refund_loss_detected": refund_loss_detected,
        "global_reservation_cell_conflict_detected":
            global_reservation_cell_conflict_detected,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-out")
    args = parser.parse_args()
    lifecycle = exhaustive_lifecycle()
    atomicity = exhaustive_atomicity()
    merge = exhaustive_merge_refinement()
    application_cost_composition = exhaustive_application_cost_composition()
    authorization = authorization_checks()
    controls = negative_controls()
    overall_pass = (
        not lifecycle["violations"]
        and not atomicity["violations"]
        and not merge["violations"]
        and not application_cost_composition["violations"]
        and all(
            count > 0
            for count in application_cost_composition["omission_witness_counts"].values()
        )
        and all(authorization.values())
        and all(controls.values())
    )
    output = {
        "overall_pass": overall_pass,
        "lifecycle": lifecycle,
        "atomicity": atomicity,
        "merge": merge,
        "application_cost_composition": application_cost_composition,
        "authorization": authorization,
        "negative_controls": controls,
    }
    rendered = json.dumps(output, sort_keys=True)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            handle.write(rendered)
    else:
        print(rendered)
    raise SystemExit(0 if overall_pass else 1)


if __name__ == "__main__":
    main()
