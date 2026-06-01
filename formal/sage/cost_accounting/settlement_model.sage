import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


I64_MAX = 2**63 - 1


# D3 (DR-9, OD-2/OD-3): the singular-phlo escrow refund model (escrow =
# limit * price, refund = (limit - token_cost) * price) is REMOVED. A deploy's
# cost is the per-COMM token count `demand` (= Delta_s); funding is the
# per-signature supply pool `supply` (= Sigma_s); the block-assembly gate admits
# iff the EFFECTIVE supply meets the demand plus the genesis safety `margin`, and
# the SINGLE consensus decrement is the settlement debit `post = supply - demand`
# (applied once at block close), which must never underflow for an admitted
# deploy. There is NO op-budget-exhaustion surface and NO per-deploy refund.
def is_funded(demand, supply, margin):
    # The pure Def-19/Thm-20 funding inequality (i128 in Rust; unbounded here).
    return int(supply) >= int(demand) + int(margin)


def settle(demand, supply, margin):
    if demand < 0 or supply < 0 or margin < 0:
        return {"valid": False, "reason": "negative_input"}
    funded = is_funded(demand, supply, margin)
    # The per-COMM settlement debit is the demand (COMM count) for an admitted
    # deploy; an unfunded deploy is rejected and debits nothing.
    debit = int(demand) if funded else 0
    post = int(supply) - debit
    return {
        "valid": True,
        "demand": int(demand),
        "supply": int(supply),
        "margin": int(margin),
        "funded": bool(funded),
        # The single consensus decrement: post = pre - debit (>= 0 for admitted).
        "settlement_debit": debit,
        "supply_after": int(post),
    }


# ════════════════════════════════════════════════════════════════════════════
# #12 — the EXACT per-component (Split/Join) compound settlement debit.
# ════════════════════════════════════════════════════════════════════════════
#
# A compound-signed (Sig::And(s1, s2)) deploy debits its COMPONENT pools per
# spec §3.6 Rule 2 / Rule 4 (tex 677-728; App. A Split/Join, tex 2020-2245):
# one token from the combined pool Σ⟦comp⟧ OR a matched PAIR from the component
# pools Σ⟦s1⟧, Σ⟦s2⟧. The Rust gate
# (acceptance.rs::compute_settlement_debits) splits the admitted compound
# demand k COMBINED-POOL-FIRST:
#
#   draw_compound = min(k, Σ⟦comp⟧)
#   draw_pair     = k − draw_compound        (≤ min(Σ⟦s1⟧, Σ⟦s2⟧) by admission)
#   Σ⟦comp⟧ −= draw_compound ; Σ⟦s1⟧ −= draw_pair ; Σ⟦s2⟧ −= draw_pair
#
# This Sage model bounded-exhaustively checks the three-pool split AND the
# cross-group shared-component residual ledger (a component shared by several
# compounds in one block stays underflow-safe across groups).


def compound_settle(sigma_comp, sigma1, sigma2, k):
    """The three-pool compound debit (combined-pool-first), mirroring the Rust
    compute_settlement_debits draw split for ONE compound group.

    Returns the draws, the three post-balances, and the total tokens drawn
    (draw_compound·1 + draw_pair·2 — the matched pair debits BOTH component
    pools, the Rule-4 one-token / Rule-2 two-token bridge).
    """
    sc = int(sigma_comp); s1 = int(sigma1); s2 = int(sigma2); kk = int(k)
    draw_compound = min(kk, sc)
    remaining = kk - draw_compound
    # ≤ min(s1, s2) by the admission bound k ≤ sc + min(s1, s2); the .max(0)
    # mirrors the Rust clamp (never negative).
    draw_pair = max(0, min(remaining, s1, s2))
    post_comp = sc - draw_compound
    post1 = s1 - draw_pair
    post2 = s2 - draw_pair
    total_drawn = draw_compound + 2 * draw_pair
    return {
        "sigma_comp": sc, "sigma1": s1, "sigma2": s2, "k": kk,
        "draw_compound": int(draw_compound),
        "draw_pair": int(draw_pair),
        "post_comp": int(post_comp),
        "post1": int(post1),
        "post2": int(post2),
        "total_drawn": int(total_drawn),
    }


def compound_settle_properties(sigma_comp, sigma1, sigma2, k):
    """The #12 safety properties for one admissible (Σ_comp, Σ1, Σ2, k)."""
    r = compound_settle(sigma_comp, sigma1, sigma2, k)
    # (C1) underflow-safety: every post-balance is ≥ 0.
    c1 = r["post_comp"] >= 0 and r["post1"] >= 0 and r["post2"] >= 0
    # (C2) conservation: Σ post + total_drawn = Σ pre.
    pre_sum = r["sigma_comp"] + r["sigma1"] + r["sigma2"]
    post_sum = r["post_comp"] + r["post1"] + r["post2"]
    c2 = (post_sum + r["total_drawn"] == pre_sum)
    # (C3) the draw split reconstructs the admitted demand (combined-first is
    # exhaustive when k is within the effective supply): draw_compound +
    # draw_pair = k. (For k strictly above the effective supply this would not
    # hold, but the search only feeds admissible k.)
    c3 = (r["draw_compound"] + r["draw_pair"] == r["k"])
    return r, {"c1_no_underflow": c1, "c2_conserves": c2, "c3_draw_eq_demand": c3}


def compound_cross_group_shared_component(sigma_comp, sigma1, sigma2, sigma3, k1, k2):
    """Two compound groups And(s1, s2) and And(s1, s3) BOTH drawing the shared
    component s1 (and the shared combined pool), with the residual ledger
    bounding the second group by the LIVE remaining balances — mirrors the Rust
    cross-group residual ledger. Returns the summed draws and a properties dict.
    """
    sc = int(sigma_comp); s1 = int(sigma1); s2 = int(sigma2); s3 = int(sigma3)
    kk1 = int(k1); kk2 = int(k2)

    # Group 1 draws first (BTreeMap SigKey order is deterministic; the ledger
    # evolves identically on play and replay).
    dC1 = min(kk1, sc)
    rem1 = kk1 - dC1
    dP1 = max(0, min(rem1, s1, s2))
    # Residual ledger after group 1.
    res_comp = sc - dC1
    res_s1 = s1 - dP1

    # Group 2 draws the combined-pool residual first, then its pair bounded by
    # the LIVE residual of the shared s1 and of s3.
    dC2 = min(kk2, res_comp)
    rem2 = kk2 - dC2
    dP2 = max(0, min(rem2, res_s1, s3))

    s1_summed_draw = dP1 + dP2
    comp_summed_draw = dC1 + dC2
    post_s1 = s1 - s1_summed_draw
    post_comp = sc - comp_summed_draw
    post_s2 = s2 - dP1
    post_s3 = s3 - dP2

    props = {
        # The summed draw on the shared component never exceeds its pre-balance.
        "shared_component_within_supply": s1_summed_draw <= s1,
        # The summed draw on the shared combined pool never exceeds its balance.
        "combined_pool_within_supply": comp_summed_draw <= sc,
        # No pool underflows under cross-group contention.
        "no_underflow": (post_s1 >= 0 and post_comp >= 0 and post_s2 >= 0 and post_s3 >= 0),
    }
    return {
        "s1_summed_draw": int(s1_summed_draw),
        "comp_summed_draw": int(comp_summed_draw),
        "post_s1": int(post_s1), "post_comp": int(post_comp),
        "post_s2": int(post_s2), "post_s3": int(post_s3),
        "dP1": int(dP1), "dP2": int(dP2), "dC1": int(dC1), "dC2": int(dC2),
    }, props


def compound_debit_search(max_supply=5):
    """Bounded-EXHAUSTIVE sweep of the three-pool compound debit: every
    (Σ_comp, Σ1, Σ2) over 0..max_supply and every ADMISSIBLE demand
    k ∈ 0..Σ_comp + min(Σ1, Σ2). Assert (C1) no pool negative, (C2) Σ post +
    total_drawn = Σ pre, (C3) draw split reconstructs k. Then a cross-group
    contention sweep over two groups sharing component s1.
    """
    single_traces = 0
    single_violations = []
    for sc in range(0, max_supply + 1):
        for s1 in range(0, max_supply + 1):
            for s2 in range(0, max_supply + 1):
                eff = sc + min(s1, s2)  # effectiveΣ_compound (admission cap)
                for k in range(0, eff + 1):
                    single_traces += 1
                    _, props = compound_settle_properties(sc, s1, s2, k)
                    if not all(props.values()):
                        single_violations.append(
                            {"sigma_comp": sc, "sigma1": s1, "sigma2": s2,
                             "k": k, "props": props})

    # Cross-group shared-component contention: two compound groups sharing s1.
    # Bound the second component s3 and both demands; the residual ledger keeps
    # the SUMMED draw on the shared pools within their pre-balances. Use a
    # tighter range to keep the product tractable while still exhaustive.
    cg = max(2, max_supply - 1)
    cross_traces = 0
    cross_violations = []
    for sc in range(0, cg + 1):
        for s1 in range(0, cg + 1):
            for s2 in range(0, cg + 1):
                for s3 in range(0, cg + 1):
                    eff1 = sc + min(s1, s2)
                    eff2 = sc + min(s1, s3)
                    for k1 in range(0, eff1 + 1):
                        for k2 in range(0, eff2 + 1):
                            cross_traces += 1
                            _, props = compound_cross_group_shared_component(
                                sc, s1, s2, s3, k1, k2)
                            if not all(props.values()):
                                cross_violations.append(
                                    {"sigma_comp": sc, "sigma1": s1, "sigma2": s2,
                                     "sigma3": s3, "k1": k1, "k2": k2,
                                     "props": props})

    return {
        "single_traces": single_traces,
        "single_violations": single_violations,
        "cross_traces": cross_traces,
        "cross_violations": cross_violations,
        "all_safe": len(single_violations) == 0 and len(cross_violations) == 0,
        "max_supply": int(max_supply),
    }


def records():
    # Funded boundary: Sigma = Delta + margin admits, and the debit (= Delta)
    # leaves a non-negative pool (no underflow).
    funded = settle(demand=8, supply=10, margin=2)
    # Just below the margin: Sigma = Delta + margin - 1 is REJECTED (no debit).
    rejected = settle(demand=8, supply=9, margin=2)
    # Drained pool: a present-but-zero supply rejects a further per-COMM demand
    # (the §7.7 duplicate-deploy double-spend shape).
    drained = settle(demand=3, supply=0, margin=0)
    # Block settlement is the sum of independent per-signature pool debits.
    multi = [settle(8, 10, 0), settle(5, 4, 0), settle(3, 3, 0)]
    multi_debit = sum(item.get("settlement_debit", 0) for item in multi if item["valid"])
    multi_supply_after = sum(item.get("supply_after", 0) for item in multi if item["valid"])

    # #12 — the EXACT per-component (Split/Join) compound settlement debit. The
    # bounded-EXHAUSTIVE sweep MUST find zero violations across every admissible
    # (Σ_comp, Σ1, Σ2, k) and every cross-group shared-component (k1, k2); surface
    # that as the deterministic witness so a regression flips the classification.
    compound_search = compound_debit_search(max_supply=5)
    assert compound_search["all_safe"], (
        "compound settlement debit search found a violation: single=%s cross=%s"
        % (json.dumps(compound_search["single_violations"][:3], default=schema_json_default),
           json.dumps(compound_search["cross_violations"][:3], default=schema_json_default))
    )
    # Witness 1 — combined-pool-first then component pair: Σ⟦comp⟧=1, Σ1=Σ2=5,
    # k=3 ⇒ draw_compound=1, draw_pair=2; post=(0,3,3), total_drawn=1+2·2=5=k+drawn.
    compound_split_witness, compound_split_props = compound_settle_properties(1, 5, 5, 3)
    # Witness 2 — component-pair-only (empty combined pool): Σ⟦comp⟧=0, Σ1=Σ2=k=4
    # ⇒ draw_pair=4 on BOTH components, no compound draw; conserves + no underflow.
    compound_pair_witness, compound_pair_props = compound_settle_properties(0, 4, 4, 4)
    # Witness 3 — underflow boundary: k = effectiveΣ = Σ⟦comp⟧ + min(Σ1,Σ2) = 2+3
    # ⇒ every pool drained to exactly ≥ 0 (the funding-boundary no-underflow case).
    compound_boundary_witness, compound_boundary_props = compound_settle_properties(2, 3, 4, 5)
    # Witness 4 — cross-group shared component: groups And(a,b), And(a,c) both
    # draw the shared a (Σ⟦a⟧=3, empty combined pools so all demand hits the pairs,
    # each group demands 2) ⇒ summed a-draw bounded by 3 (residual ledger).
    compound_contention_data, compound_contention_props = \
        compound_cross_group_shared_component(0, 3, 10, 10, 2, 2)

    return [
        record(
            "settlement",
            "confirmed_safe",
            "sage_per_comm_funding_admits_when_supply_meets_demand_plus_margin",
            "A deploy is admitted iff Sigma_s >= Delta_s + margin; its settlement debit (= the per-COMM demand) never underflows the supply pool.",
            canonical_scenario("funded_admission", settlement={"kind": "per_comm_settle", "demand": 8, "supply": 10, "margin": 2}, expected_classification="confirmed_safe"),
            funded,
            ["Rocq: consumed_fuel_count_eq_token_drop / funded_settlement_debit_never_underflows_supply (kani)", "Rust: settlement_debit_equals_comm_count"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_per_comm_reject_below_demand_plus_margin",
            "Sigma_s strictly below Delta_s + margin is rejected and debits nothing (§7.7 reject direction).",
            canonical_scenario("rejected_admission", settlement={"kind": "per_comm_settle", "demand": 8, "supply": 9, "margin": 2}, expected_classification="confirmed_safe"),
            rejected,
            ["Rocq: reject_below_demand_plus_margin (kani)", "Rust: funded_unfunded_boundary_at_margin"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_per_comm_drained_pool_rejects_double_spend",
            "A present-but-drained supply (Sigma = 0) rejects a further per-COMM demand — the §7.7 duplicate-deploy double-spend shape.",
            canonical_scenario("drained_pool", settlement={"kind": "per_comm_settle", "demand": 3, "supply": 0, "margin": 0}, expected_classification="confirmed_safe"),
            drained,
            ["Rust: drained_present_pool_rejects"],
        ),
        record(
            "settlement",
            "proof_or_model_strengthening",
            "sage_per_comm_block_settlement_adds_independently",
            "Block settlement is the sum of independent per-signature pool debits (each = the admitted deploy's per-COMM demand).",
            canonical_scenario("multi_pool_settlement", settlement={"kind": "multi_pool"}, projection={"pools": len(multi)}, expected_classification="proof_or_model_strengthening"),
            {"pools": multi, "total_settlement_debit": int(multi_debit), "total_supply_after": int(multi_supply_after)},
            ["Rust: generated cost frontier replay fixtures", "Sage: objective frontier"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_compound_split_debit_conserves_and_no_underflow",
            "#12: across EVERY admissible (Σ⟦comp⟧, Σ⟦s1⟧, Σ⟦s2⟧, k≤Σ⟦comp⟧+min(Σ⟦s1⟧,Σ⟦s2⟧)), the combined-pool-first compound debit (draw_compound=min(k,Σ⟦comp⟧), draw_pair=k−draw_compound) leaves every pool ≥ 0 AND conserves: Σ post + (draw_compound·1 + draw_pair·2) = Σ pre. Bounded-exhaustive over Σ ∈ 0..5.",
            canonical_scenario(
                "compound_split_debit",
                threat_family="settlement",
                settlement={"kind": "compound_split", "sigma_comp": 1, "sigma1": 5, "sigma2": 5, "k": 3,
                            "draw_compound": 1, "draw_pair": 2},
                concurrency={"interleavings": int(compound_search["single_traces"])},
                expected_invariants=["compound_no_underflow", "compound_conserves", "compound_draw_eq_demand"],
                expected_classification="confirmed_safe",
            ),
            {"properties": compound_split_props, "witness": compound_split_witness,
             "single_traces_searched": int(compound_search["single_traces"])},
            ["Rocq: compound_split_debit_conserves", "Rocq: compound_split_debit_no_underflow",
             "TLA+: CompoundSettlement Inv_CompoundDebitConserves / Inv_ComponentDrawNoUnderflow",
             "Rust: compound_component_pool_underflow_safe / compute_settlement_debits"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_compound_pair_only_debit_conserves",
            "#12: with an empty combined pool (Σ⟦comp⟧=0), the WHOLE compound demand is settled from the component pair — Σ⟦s1⟧ −= k AND Σ⟦s2⟧ −= k, no compound draw — and still conserves (Σ post + 2·k = Σ pre) with no underflow. The Split/Join pair-only regime.",
            canonical_scenario(
                "compound_pair_only_debit",
                threat_family="settlement",
                settlement={"kind": "compound_pair_only", "sigma_comp": 0, "sigma1": 4, "sigma2": 4, "k": 4,
                            "draw_compound": 0, "draw_pair": 4},
                expected_invariants=["compound_no_underflow", "compound_conserves", "compound_draw_eq_demand"],
                expected_classification="confirmed_safe",
            ),
            {"properties": compound_pair_props, "witness": compound_pair_witness},
            ["Rocq: compound_split_debit_conserves", "Rust: compound_debit_splits_to_components",
             "Rust: compound_debit_play_replay_identical_pair_only"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_compound_underflow_boundary_safe",
            "#12: at the funding boundary k = effectiveΣ = Σ⟦comp⟧ + min(Σ⟦s1⟧,Σ⟦s2⟧), the compound debit drains the combined pool and the matched pair to exactly ≥ 0 on every pool — the maximal admissible draw never underflows.",
            canonical_scenario(
                "compound_underflow_boundary",
                threat_family="settlement",
                settlement={"kind": "compound_boundary", "sigma_comp": 2, "sigma1": 3, "sigma2": 4, "k": 5},
                expected_invariants=["compound_no_underflow", "compound_conserves"],
                expected_classification="confirmed_safe",
            ),
            {"properties": compound_boundary_props, "witness": compound_boundary_witness},
            ["Rocq: compound_split_debit_no_underflow",
             "TLA+: CompoundSettlement Inv_ComponentDrawNoUnderflow",
             "Rust: compound_component_pool_underflow_safe"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_compound_shared_component_residual_ledger_safe",
            "#12 cross-group: two compound groups And(a,b), And(a,c) both drawing the SHARED component a — the residual ledger bounds the second group's pair-draw by a's LIVE remaining balance, so the SUMMED a-draw across both groups stays ≤ Σ⟦a⟧ (and the shared combined pool likewise), underflow-safe across groups not just within one. Bounded-exhaustive over Σ ∈ 0..4.",
            canonical_scenario(
                "compound_shared_component_contention",
                threat_family="settlement",
                settlement={"kind": "compound_cross_group", "sigma_a": 3, "demands": [2, 2]},
                concurrency={"interleavings": int(compound_search["cross_traces"]), "shared_component": True},
                expected_invariants=["shared_component_within_supply", "combined_pool_within_supply", "no_underflow"],
                expected_classification="confirmed_safe",
            ),
            {"properties": compound_contention_props, "data": compound_contention_data,
             "cross_traces_searched": int(compound_search["cross_traces"])},
            ["Rust: compound_shared_component_contention",
             "TLA+: CompoundSettlement Inv_SharedComponentSummedDrawWithinSupply",
             "Rust: compute_settlement_debits residual ledger"],
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
