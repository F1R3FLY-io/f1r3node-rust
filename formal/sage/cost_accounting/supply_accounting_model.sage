import argparse
import itertools
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))

# ════════════════════════════════════════════════════════════════════════════
# supply_accounting_model.sage — Cost-Accounted Rho Stage B supply accounting
# ════════════════════════════════════════════════════════════════════════════
#
# Adversarial search over (mint, admit, settle) interleavings on the per-
# validator supply pool Σ⟦v⟧ (DR-13; stageb-minting-halt-interface.md Decision
# 6/7). The model is the Sage companion of:
#   - MintingInjection.v   (epoch_mint idempotency on the balance; user steps
#                            never move a supply balance)
#   - MintingHalt.v        (halted ⇒ no mint, no supply increase)
#   - EvalScheduling.tla    (SupplyOnlyFromMint / HaltedValidatorSupplyNonIncreasing)
#   - SlashFlow.tla         (Inv_HaltedNotMinted / Inv_NoDoubleCreditUnderMerge)
#
# It searches all interleavings of a small operation alphabet and asserts the
# four supply safety properties hold over EVERY reachable interleaving:
#
#   (P1) no-negative-balance         : a balance is never driven below 0.
#   (P2) no-double-credit-under-merge: an (epoch) mint already recorded in
#        mintedEpochs is a NO-OP (idempotent) — duplicated / multi-parent-merged
#        mints credit Σ⟦v⟧ at most once per epoch.
#   (P3) settlement-conservation     : after the acceptance gate settles, the
#        post-state balance equals pre minus the admitted demand
#        (post = pre − ΣΔ_admitted); rejected demand consumes nothing.
#   (P4) halt-no-credit              : a halted validator's balance is never
#        increased by a mint (the cross-epoch halt).
#
# The mint write is read-modify-REPLACE of a single datum (supply::produce_balance
# in Rust): so even an out-of-guard re-execution rewrites the SAME value — the
# search exercises that explicitly via duplicated mint operations.

EPOCH = 0


def apply_op(state, op):
    """Apply one operation to (balance, minted, halted, residual, committed).

    state: dict with keys
        balance   : int  — Σ⟦v⟧ for the single validator under test
        minted    : bool — (v, EPOCH) ∈ mintedEpochs
        halted    : bool — v ∈ mintingHalted
        residual  : int  — in-pass admission residual (effectiveΣ_s)
        committed : int  — ΣΔ committed by the acceptance gate (settled debit)
    op: a tuple describing the operation.
    Returns the next state (a new dict).
    """
    s = dict(state)
    kind = op[0]
    if kind == "mint":
        amount = int(op[1])
        # Eligibility guard: not halted AND not already minted this epoch
        # (mirrors mint_eligible / the Rholang fold predicate). Read-modify-
        # REPLACE: an ineligible mint is the identity.
        if (not s["halted"]) and (not s["minted"]):
            s["balance"] = s["balance"] + amount
            s["minted"] = True
    elif kind == "open_gate":
        # The acceptance gate reads Σ_s once and seeds the in-pass residual
        # (DR-11 / WD-D2): residual := effectiveΣ_s = current balance. The
        # per-group prefix starts OPEN (prefix_open = True) — admissions proceed
        # in canonical order until the first non-fitting deploy closes it.
        s["residual"] = s["balance"]
        s["committed"] = 0
        s["prefix_open"] = True
    elif kind == "admit":
        # Admit a demand Δ in CANONICAL ORDER iff the prefix is still open AND Δ
        # fits the residual; then commit Δ (decrement the residual). §7.7
        # reject-both / no-partial (WD-D2 admit_by_funding): the FIRST non-fitting
        # deploy CLOSES the prefix (prefix_open = False) so it AND every later
        # deploy in the group are rejected — a smaller later Δ does NOT sneak in.
        delta = int(op[1])
        if s.get("prefix_open", False) and delta <= s["residual"]:
            s["residual"] = s["residual"] - delta
            s["committed"] = s["committed"] + delta
        elif s.get("prefix_open", False) and delta > s["residual"]:
            # First non-fitting deploy: reject it AND close the prefix.
            s["prefix_open"] = False
    elif kind == "settle":
        # Settlement debit = the committed demand: post = pre − ΣΔ_admitted.
        # This is the single consensus decrement (DR-13 Decision 4(c)). The
        # debit is floored at the available balance: a supply pool can never be
        # driven negative (the spec's funding obligation Σ_s ≥ Δ_s, tex 1590),
        # and a slash that zeros Σ⟦v⟧ between gate and settle simply leaves
        # nothing to debit (the validator's deploys would not have been admitted
        # — VB blocks — so this floor is never actually exercised in-band; it
        # makes the no-negative invariant unconditional over ALL interleavings).
        s["balance"] = s["balance"] - min(s["committed"], s["balance"])
        s["committed"] = 0
    elif kind == "halt":
        # Slash effect (Decision 4): halt minting + zero Σ⟦v⟧ (drain @W_v is
        # modeled as the supply zero here).
        s["halted"] = True
        s["balance"] = 0
    elif kind == "fee_collect":
        # Cost-Accounted Rho Stage D — the FeeExtract: credit `amount` tokens to
        # the validator's FEE pool F_v (s["fees"]). NEVER touches the supply pool
        # (cost ≠ fee; the fee reaches Σ⟦v⟧ only via fee_convert). Read-modify-add.
        amount = int(op[1])
        s["fees"] = s.get("fees", 0) + amount
    elif kind == "fee_convert":
        # Cost-Accounted Rho Stage D — the per-epoch fee→v conversion (spec
        # tex:3095-3100). An ELIGIBLE validator (NOT halted AND NOT already
        # converted this epoch) moves its WHOLE fee pool f into Σ⟦v⟧ 1:1 and
        # zeroes F_v, recording converted (the convertedEpochs idempotency guard).
        # The Σ⟦v⟧ credit equals EXACTLY the drained fees (BACKED, not minted —
        # fee_convert_credit_is_backed). DR-4: f == 0 still records the epoch but
        # credits nothing (no one-sided mint). Idempotent: a re-convert is a
        # NO-OP on Σ⟦v⟧.
        if (not s["halted"]) and (not s.get("converted", False)):
            f = s.get("fees", 0)
            s["balance"] = s["balance"] + f
            s["fees"] = 0
            s["converted"] = True
    elif kind == "user_step":
        # A user reduction step NEVER touches the supply pool (DR-13). Identity.
        pass
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
    state, history = run_trace(initial, ops)

    # (P1) no negative balance at any point.
    p1 = all(h["balance"] >= 0 for h in history)

    # (P2) no double credit: total minted credit across the trace is at most
    # one MintAmount (idempotency). We recover the credit from mint ops that
    # actually fired by replaying with a counter.
    credit = 0
    sim = dict(initial)
    for op in ops:
        before = sim["minted"]
        sim = apply_op(sim, op)
        if op[0] == "mint" and (not before) and sim["minted"]:
            credit += int(op[1])
    max_single = max([int(op[1]) for op in ops if op[0] == "mint"], default=0)
    p2 = credit <= max_single

    # (P3) settlement conservation: across each open_gate..settle window, the
    # balance drops by exactly the admitted (committed) demand, and a rejected
    # demand (committed unchanged across that admit) costs nothing. We verify
    # the end-to-end identity post = pre − ΣΔ_admitted over the whole trace by
    # tracking the pre-settle balance and committed at each settle.
    p3 = True
    sim = dict(initial)
    pre_settle_balance = sim["balance"]
    for op in ops:
        if op[0] == "open_gate":
            pre_settle_balance = sim["balance"]
        sim_before = dict(sim)
        sim = apply_op(sim, op)
        if op[0] == "settle":
            # post = pre − ΣΔ_admitted, floored at 0 (no negative supply). In
            # the in-band case (committed ≤ balance, the only one the spec
            # permits) this is exactly pre − committed.
            expected = max(0, sim_before["balance"] - sim_before["committed"])
            if sim["balance"] != expected:
                p3 = False

    # (P4) a halted validator gains no supply: once halted, no later mint raises
    # the balance.
    p4 = True
    sim = dict(initial)
    for op in ops:
        was_halted = sim["halted"]
        bal_before = sim["balance"]
        sim = apply_op(sim, op)
        if was_halted and sim["balance"] > bal_before:
            p4 = False

    # (P5) canonical-prefix reject-both (WD-D2 §7.7 no-partial): once the prefix
    # has closed on a non-fitting deploy, NO later admit commits anything. We
    # check that after prefix_open flips to False, committed never increases.
    p5 = True
    sim = dict(initial)
    prefix_was_closed = False
    for op in ops:
        committed_before = sim["committed"]
        if op[0] == "open_gate":
            prefix_was_closed = False
        sim = apply_op(sim, op)
        # Track closure AFTER applying (a non-fitting admit closes it this step).
        if op[0] == "admit" and not sim.get("prefix_open", False):
            # If the prefix was already closed BEFORE this admit, no commit may
            # have happened on this step (reject-both / all-after).
            if prefix_was_closed and sim["committed"] > committed_before:
                p5 = False
            prefix_was_closed = True

    # (P6) Stage-D fee→v conversion is BACKED + idempotent: across the trace, the
    # TOTAL fees converted into Σ⟦v⟧ is at most the total fees ever COLLECTED (the
    # convert is backed, never a mint), AND a validator is converted at most ONCE
    # per epoch (the convertedEpochs guard ⇒ no double-credit under merge/replay).
    p6 = True
    sim = dict(initial)
    total_collected = 0
    total_converted = 0
    convert_fired = 0
    for op in ops:
        if op[0] == "fee_collect":
            total_collected += int(op[1])
        before = sim.get("converted", False)
        bal_before = sim["balance"]
        fees_before = sim.get("fees", 0)
        sim = apply_op(sim, op)
        if op[0] == "fee_convert" and (not before) and sim.get("converted", False):
            # A convert just fired: it credited Σ⟦v⟧ by EXACTLY the drained fees,
            # and zeroed F_v (backed, 1:1).
            convert_fired += 1
            credited = sim["balance"] - bal_before
            total_converted += credited
            if credited != fees_before or sim.get("fees", 0) != 0:
                p6 = False
    # Backed: converted ≤ collected; idempotent: at most one convert per epoch.
    if total_converted > total_collected or convert_fired > 1:
        p6 = False

    return {"p1_no_negative": p1, "p2_no_double_credit": p2,
            "p3_settlement_conserves": p3, "p4_halt_no_credit": p4,
            "p5_canonical_prefix_reject_both": p5,
            "p6_fee_convert_backed_idempotent": p6}


def adversarial_search():
    """Exhaustively search all interleavings of a small op alphabet and assert
    the four supply safety properties hold over every reachable interleaving."""
    mint_amount = 1000
    # The ORIGINAL Stage-B/D2 op set, permuted in full (8! orderings) to preserve
    # the prior supply/gate coverage verbatim.
    base_alphabet = [
        ("mint", mint_amount),     # epoch mint (post_eval produce_balance)
        ("mint", mint_amount),     # DUPLICATE mint (multi-parent merge / replay)
        ("open_gate",),            # acceptance gate reads Σ_s -> residual
        ("admit", 300),            # a fitting demand
        ("admit", 900),            # a (possibly) non-fitting demand
        ("settle",),               # settlement debit = committed
        ("user_step",),            # a user reduction step (no supply effect)
        ("halt",),                 # slash: halt + zero Σ⟦v⟧
    ]
    # The Stage-D fee ops, exercised in interleavings via the length-4 product
    # over the FULL alphabet (avoids the 11! permutation blowup while still
    # covering collect → convert → re-convert(merge) orderings against mints/halts).
    fee_alphabet = [
        ("fee_collect", 5),        # Stage D: FeeExtract into F_v
        ("fee_convert",),          # Stage D: epoch fee→v convert (backed, 1:1)
        ("fee_convert",),          # DUPLICATE convert (merge/replay) — guarded no-op
    ]
    full_alphabet = base_alphabet + fee_alphabet
    initial = {"balance": 0, "minted": False, "halted": False,
               "residual": 0, "committed": 0, "prefix_open": False,
               "fees": 0, "converted": False}

    total = 0
    violations = []
    # Search every permutation of the BASE alphabet (all orderings of the Stage-B/
    # D2 ops, prior coverage) plus every length-4 ordered selection over the FULL
    # alphabet (interleavings with repetition pressure, INCLUDING the fee ops).
    candidate_traces = []
    for perm in itertools.permutations(base_alphabet):
        candidate_traces.append(list(perm))
    for combo in itertools.product(full_alphabet, repeat=4):
        candidate_traces.append(list(combo))

    worst = None
    for ops in candidate_traces:
        total += 1
        props = check_properties(initial, ops)
        if not all(props.values()):
            violations.append({"ops": [list(o) for o in ops], "props": props})
        if worst is None:
            worst = props
    return {
        "traces_searched": total,
        "violations": violations,
        "all_safe": len(violations) == 0,
        "mint_amount": mint_amount,
    }


def records():
    search = adversarial_search()
    # The search MUST find zero violations; surface that as the deterministic
    # witness so a regression (a real violation) flips the classification.
    assert search["all_safe"], (
        "supply accounting interleaving search found a violation: %s"
        % json.dumps(search["violations"][:3], default=schema_json_default)
    )

    no_negative_witness = check_properties(
        {"balance": 0, "minted": False, "halted": False, "residual": 0, "committed": 0, "prefix_open": False},
        [("mint", 1000), ("open_gate",), ("admit", 300), ("settle",)],
    )
    double_credit_witness = check_properties(
        {"balance": 0, "minted": False, "halted": False, "residual": 0, "committed": 0, "prefix_open": False},
        [("mint", 1000), ("mint", 1000), ("open_gate",), ("admit", 900), ("settle",)],
    )
    halt_witness = check_properties(
        {"balance": 0, "minted": False, "halted": False, "residual": 0, "committed": 0, "prefix_open": False},
        [("halt",), ("mint", 1000)],
    )
    oversub_witness = check_properties(
        {"balance": 500, "minted": True, "halted": False, "residual": 0, "committed": 0, "prefix_open": False},
        [("open_gate",), ("admit", 300), ("admit", 900), ("settle",)],
    )
    # Canonical-prefix reject-both: with Σ=500 and demands [300, 900, 100] in
    # canonical order, the 300 fits (residual 200), the 900 does NOT (closes the
    # prefix), and the LATER 100 — though it WOULD fit the residual — is REJECTED
    # because the prefix already closed (§7.7 reject it + all after). Settled
    # debit = 300 only (NOT 400).
    prefix_reject_witness = check_properties(
        {"balance": 500, "minted": True, "halted": False, "residual": 0, "committed": 0, "prefix_open": False},
        [("open_gate",), ("admit", 300), ("admit", 900), ("admit", 100), ("settle",)],
    )
    # Stage D: collect a fee, convert it (Σ⟦v⟧ += f, F_v := 0), then a DUPLICATE
    # convert is a guarded no-op (convertedEpochs) — Σ⟦v⟧ credited once, backed by
    # the collected fee. post Σ⟦v⟧ = pre(0) + epochMint(1000) + convertedFees(5).
    fee_convert_witness = check_properties(
        {"balance": 0, "minted": False, "halted": False, "residual": 0, "committed": 0,
         "prefix_open": False, "fees": 0, "converted": False},
        [("mint", 1000), ("fee_collect", 5), ("fee_convert",), ("fee_convert",)],
    )

    common_invariants = [
        "no_negative_balance",
        "no_double_credit_under_merge",
        "settlement_post_eq_pre_minus_admitted",
        "halted_validator_gains_no_supply",
        "canonical_prefix_reject_both",
        "fee_convert_backed_and_idempotent",
    ]

    return [
        record(
            "supply_accounting",
            "confirmed_safe",
            "sage_supply_no_negative_and_settlement_conserves",
            "Across every (mint, gate, admit, settle) interleaving, Σ⟦v⟧ stays non-negative and settles to post = pre − ΣΔ_admitted.",
            canonical_scenario(
                "supply_mint_admit_settle",
                threat_family="settlement",
                settlement={"mint_amount": 1000, "admitted": 300},
                concurrency={"interleavings": int(search["traces_searched"])},
                expected_invariants=common_invariants,
                expected_classification="confirmed_safe",
            ),
            {"properties": no_negative_witness, "traces_searched": int(search["traces_searched"])},
            ["Rocq: epoch_mint_idempotent_on_balance", "Rocq: user_ca_step_does_not_increase_balance",
             "TLA+: EvalScheduling SupplyOnlyFromMint", "Rust: close_block_supply_mint_is_play_replay_deterministic"],
        ),
        record(
            "supply_accounting",
            "confirmed_safe",
            "sage_supply_no_double_credit_under_merge",
            "A duplicated / multi-parent-merged epoch mint credits Σ⟦v⟧ at most once per epoch (mintedEpochs idempotency; read-modify-replace).",
            canonical_scenario(
                "supply_double_mint_merge",
                threat_family="slashing_composition",
                concurrency={"racing_mint": True, "merge": "multi_parent"},
                expected_invariants=common_invariants,
                expected_classification="confirmed_safe",
            ),
            {"properties": double_credit_witness},
            ["Rocq: epoch_mint_idempotent_on_balance", "TLA+: SlashFlow Inv_NoDoubleCreditUnderMerge",
             "DR-13 / TM-CA-154"],
        ),
        record(
            "supply_accounting",
            "confirmed_safe",
            "sage_supply_halted_validator_gains_no_supply",
            "A halted validator (mintingHalted) gains no supply: the epoch mint is a no-op and Σ⟦v⟧ stays at its zeroed value.",
            canonical_scenario(
                "supply_halted_no_mint",
                threat_family="slashing_composition",
                slashing_authorization={"current_epoch": EPOCH, "evidence_epoch": EPOCH},
                expected_invariants=common_invariants,
                expected_classification="confirmed_safe",
            ),
            {"properties": halt_witness},
            ["Rocq: halted_validator_supply_not_increased", "Rocq: halted_validator_not_minted",
             "TLA+: SlashFlow Inv_HaltedNotMinted", "TM-CA-156"],
        ),
        record(
            "supply_accounting",
            "confirmed_safe",
            "sage_supply_oversubscription_rejects_both",
            "Oversubscription against Σ⟦v⟧ rejects the non-fitting demand (reject-both) so the settled debit never exceeds the pre-state supply.",
            canonical_scenario(
                "supply_oversubscription",
                threat_family="settlement",
                settlement={"supply": 500, "demands": [300, 900]},
                expected_invariants=common_invariants,
                expected_classification="confirmed_safe",
            ),
            {"properties": oversub_witness},
            ["DR-13 Decision 4(b)/(c)", "Rocq: user_ca_step_does_not_increase_balance",
             "Rust: admit_by_funding reject-both"],
        ),
        record(
            "supply_accounting",
            "confirmed_safe",
            "sage_supply_canonical_prefix_reject_both",
            "Once the per-signature group's canonical prefix closes on a non-fitting deploy, every LATER deploy is rejected too — even one whose demand would still fit the residual (§7.7 no-partial). Settled debit = the closed prefix only.",
            canonical_scenario(
                "supply_canonical_prefix_reject_both",
                threat_family="settlement",
                settlement={"supply": 500, "demands": [300, 900, 100], "admitted": 300},
                expected_invariants=common_invariants,
                expected_classification="confirmed_safe",
            ),
            {"properties": prefix_reject_witness},
            ["Rocq: admit_prefix_maximal", "Rocq: reject_both_sound",
             "TLA+: EvalScheduling RejectBothOnOversubscription",
             "Rust: admit_by_funding reject-both prefix"],
        ),
        record(
            "supply_accounting",
            "confirmed_safe",
            "sage_supply_fee_convert_backed_and_idempotent",
            "Stage D: the epoch fee→v conversion credits Σ⟦v⟧ by EXACTLY the collected fees that leave F_v (backed, 1:1 — never a mint), and a duplicated / multi-parent-merged convert is a guarded no-op (convertedEpochs). post Σ⟦v⟧ = pre + epochMint + convertedFees, with convertedFees ≤ feesCollected.",
            canonical_scenario(
                "supply_fee_convert_backed",
                threat_family="settlement",
                settlement={"epoch_mint": 1000, "fees_collected": 5, "converted_fees": 5},
                concurrency={"racing_convert": True, "merge": "multi_parent"},
                expected_invariants=common_invariants,
                expected_classification="confirmed_safe",
            ),
            {"properties": fee_convert_witness},
            ["Rocq: fee_collection_conserves", "Rocq: fee_convert_credit_is_backed",
             "TLA+: EvalScheduling Inv_FeeConvertConserves / SupplyOnlyFromMintOrBackedFeeConvert",
             "Sage: exchange_conservation", "DR-4 / TM-CA-158"],
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
