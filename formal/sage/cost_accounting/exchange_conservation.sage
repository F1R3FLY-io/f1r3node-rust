import argparse
import itertools
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))

# ════════════════════════════════════════════════════════════════════════════
# exchange_conservation.sage — Cost-Accounted Rho Stage D: the blessed Exchange
# ════════════════════════════════════════════════════════════════════════════
#
# Adversarial search over the blessed conserving 1:1 token Exchange (spec
# "Fee conversion" cost-accounted-rho.tex:3061-3084 / DR-4):
#
#     Exchange(c, v) = for (t_c <- n_c & t_v <- n_v) { n_c ! drop t_v
#                                                    | n_v ! drop t_c }
#
# a persistent JOIN over two count-datum carrier channels n_c, n_v. The model is
# the Sage companion of:
#   - Rocq Exchange.v  (exchange_conserves_per_channel / exchange_total_conserved
#                       / exchange_requires_both_inputs)
#   - TLA+ ExchangeFlow.tla (Inv_PerChannelConservation / Inv_RequiresBothInputs)
#   - Rust exchange_conserves_per_channel (the runtime swap test)
#
# It searches all interleavings of a small operation alphabet (drain a carrier,
# fire the swap) and asserts the three Exchange safety properties hold over EVERY
# reachable interleaving:
#
#   (E1) per-channel conservation : the swap consumes exactly one datum from each
#        carrier and produces exactly one on each — each carrier still holds one
#        datum after a fired swap.
#   (E2) total conservation       : the two carriers' total token count is
#        invariant across the swap (no mint, no burn).
#   (E3) requires-both-inputs (DR-4): the join FIRES only when BOTH carriers carry
#        a datum; a one-sided (drained) carrier never triggers a swap.

INIT_C = 7
INIT_V = 11


def apply_op(state, op):
    """Apply one operation to the carrier state.

    state: dict with keys
        c_datum   : int  — the count datum on the c-carrier
        v_datum   : int  — the count datum on the v-carrier
        c_present : bool — a datum is present on the c-carrier
        v_present : bool — a datum is present on the v-carrier
        swapped   : bool — the join has fired
    op: a tuple describing the operation.
    Returns the next state (a new dict).
    """
    s = dict(state)
    kind = op[0]
    if kind == "swap":
        # DR-4 join: fire ONLY when BOTH carriers carry a datum (and not yet
        # fired). Consume one datum from each, re-emit each on the OTHER carrier
        # (1:1 swap). Each carrier still holds exactly one datum afterwards.
        if s["c_present"] and s["v_present"] and (not s["swapped"]):
            new_c = s["v_datum"]
            new_v = s["c_datum"]
            s["c_datum"] = new_c
            s["v_datum"] = new_v
            s["c_present"] = True
            s["v_present"] = True
            s["swapped"] = True
        # else: a one-sided / already-fired join is a NO-OP (no one-sided mint).
    elif kind == "drain_c":
        # Model the c-carrier becoming empty before the swap fires.
        if not s["swapped"]:
            s["c_present"] = False
    elif kind == "drain_v":
        if not s["swapped"]:
            s["v_present"] = False
    return s


def run_trace(initial, ops):
    state = dict(initial)
    history = [dict(state)]
    for op in ops:
        state = apply_op(state, op)
        history.append(dict(state))
    return state, history


def check_properties(initial, ops):
    """Return a dict of property -> bool over a single interleaving."""
    _state, history = run_trace(initial, ops)

    # (E1) per-channel conservation: in every state where the swap has FIRED,
    # each carrier still holds exactly one datum (one consumed, one produced).
    e1 = all(
        (not h["swapped"]) or (h["c_present"] and h["v_present"])
        for h in history
    )

    # (E2) total conservation: in every state where BOTH carriers carry a datum,
    # the total c_datum + v_datum equals the initial total (no mint / no burn).
    e2 = all(
        (not (h["c_present"] and h["v_present"]))
        or (h["c_datum"] + h["v_datum"] == initial["c_datum"] + initial["v_datum"])
        for h in history
    )

    # (E3) requires-both-inputs (DR-4): the swap fires (transitions swapped
    # False -> True) ONLY from a state where BOTH carriers carried a datum. We
    # check that no swap fired from a one-sided state.
    e3 = True
    sim = dict(initial)
    for op in ops:
        before_swapped = sim["swapped"]
        both_before = sim["c_present"] and sim["v_present"]
        sim = apply_op(sim, op)
        if (not before_swapped) and sim["swapped"]:
            # The swap just fired — both carriers MUST have carried a datum.
            if not both_before:
                e3 = False

    # (E4) value-exact swap: once fired, the c-carrier holds the original
    # v-value and vice versa (the spec's "swaps one c-token for one v-token").
    e4 = True
    for h in history:
        if h["swapped"]:
            if not (h["c_datum"] == initial["v_datum"] and h["v_datum"] == initial["c_datum"]):
                e4 = False

    return {
        "e1_per_channel_conservation": e1,
        "e2_total_conservation": e2,
        "e3_requires_both_inputs": e3,
        "e4_swap_exchanges_values": e4,
    }


def adversarial_search():
    """Exhaustively search all interleavings of the op alphabet and assert the
    Exchange safety properties hold over every reachable interleaving."""
    alphabet = [
        ("swap",),       # fire the conserving 1:1 join
        ("drain_c",),    # the c-carrier becomes one-sided (empty)
        ("drain_v",),    # the v-carrier becomes one-sided (empty)
    ]
    initial = {
        "c_datum": INIT_C, "v_datum": INIT_V,
        "c_present": True, "v_present": True, "swapped": False,
    }

    candidate_traces = []
    # All permutations of the alphabet, plus all length-4 ordered selections
    # (interleavings with repetition pressure — repeated swaps / drains).
    for perm in itertools.permutations(alphabet):
        candidate_traces.append(list(perm))
    for combo in itertools.product(alphabet, repeat=4):
        candidate_traces.append(list(combo))

    total = 0
    violations = []
    for ops in candidate_traces:
        total += 1
        props = check_properties(initial, ops)
        if not all(props.values()):
            violations.append({"ops": [list(o) for o in ops], "props": props})
    return {
        "traces_searched": total,
        "violations": violations,
        "all_safe": len(violations) == 0,
        "init_c": INIT_C,
        "init_v": INIT_V,
    }


def records():
    search = adversarial_search()
    # The search MUST find zero violations; surface it as the deterministic
    # witness so a regression (a real violation) flips the classification.
    assert search["all_safe"], (
        "exchange conservation interleaving search found a violation: %s"
        % json.dumps(search["violations"][:3], default=schema_json_default)
    )

    initial = {
        "c_datum": INIT_C, "v_datum": INIT_V,
        "c_present": True, "v_present": True, "swapped": False,
    }
    conserve_witness = check_properties(initial, [("swap",)])
    one_sided_c_witness = check_properties(initial, [("drain_c",), ("swap",)])
    one_sided_v_witness = check_properties(initial, [("drain_v",), ("swap",)])

    common_invariants = [
        "per_channel_conservation",
        "total_conservation",
        "requires_both_inputs",
        "swap_exchanges_values",
    ]

    return [
        record(
            "exchange",
            "confirmed_safe",
            "sage_exchange_conserves_per_channel_and_total",
            "The blessed 1:1 Exchange swap consumes one datum and produces one on EACH carrier (per-channel count preserved) and conserves the two carriers' total (no mint/burn).",
            canonical_scenario(
                "exchange_swap_conserves",
                threat_family="settlement",
                settlement={"init_c": INIT_C, "init_v": INIT_V, "swapped": [INIT_V, INIT_C]},
                concurrency={"interleavings": int(search["traces_searched"])},
                expected_invariants=common_invariants,
                expected_classification="confirmed_safe",
            ),
            {"properties": conserve_witness, "traces_searched": int(search["traces_searched"])},
            ["Rocq: exchange_conserves_per_channel", "Rocq: exchange_total_conserved",
             "TLA+: ExchangeFlow Inv_PerChannelConservation",
             "Rust: exchange_conserves_per_channel"],
        ),
        record(
            "exchange",
            "confirmed_safe",
            "sage_exchange_requires_both_inputs_no_one_sided_mint",
            "DR-4: the Exchange join fires ONLY when both carriers carry a datum; a one-sided (drained) carrier never triggers a swap — no one-sided mint.",
            canonical_scenario(
                "exchange_requires_both_inputs",
                threat_family="settlement",
                settlement={"init_c": INIT_C, "init_v": INIT_V, "one_sided": True},
                expected_invariants=common_invariants,
                expected_classification="confirmed_safe",
            ),
            {"properties_c": one_sided_c_witness, "properties_v": one_sided_v_witness},
            ["Rocq: exchange_requires_both_inputs", "Rocq: exchange_is_ca_step_not_amint",
             "TLA+: ExchangeFlow Inv_RequiresBothInputs",
             "DR-4 / TM-CA-158"],
        ),
    ]


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    output = {"records": records()}
    output["coverage_summary"] = coverage_summary(output["records"])
    text = json.dumps(output, indent=2, sort_keys=True, default=schema_json_default)
    if args.json_out:
        with open(args.json_out, "w") as handle:
            handle.write(text + "\n")
    else:
        print(text)


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
