# Custom Validator — behavioral-contract bundle

A custom Cost-Accounted Rho validator customizes the **economics / adjudication** (in Rholang) and ships a
**proof bundle** discharging the validator behavioral contract for its decision functions. It inherits the
fixed Rust platform shell (acceptance gate, slash-authorization, equivocation detection, finalization). This
is a spec-minimal seam — there is **no plugin framework** (cost-accounted-rho §7.7: one proof-checker,
swappable economics). Background: `docs/theory/cost-accounting-impl/workstream-e-validator-contract.md` (the
contract) and decision record DR-12.

## What you supply

1. **Economic / adjudication contract (Rholang).** Your own `PoS.rhox`-shaped source plus `ProofOfStake`
   genesis parameters, deployed via `casper/src/rust/genesis/contracts/standard_deploys.rs` (the `pos`
   generator). Customize the slash effect, epoch mint, redemption, fee conversion, and close-block — **not**
   the acceptance gate. The gate is the fixed Rust proof-checker; DR-13 keeps the unforgeable per-signature
   supply pool `Σ⟦s⟧` out of Rholang, so the gate cannot live in a Rholang contract.
2. **Proof bundle.** TLA+ **plus** (Rocq **or** Lean) discharges of the obligations below for your
   admission / decision functions.

## What you inherit (do not re-implement)

The Rust platform shell enforces, uniformly, for any economic contract: the acceptance gate (`Σ_s ≥ Δ_s`,
`acceptance.rs::admit_by_funding`), the slash-authorization predicate (`validate.rs`), equivocation
detection, and the finalization oracle. **P1 (slash-authorization) and P2 (finalization safety) are
inherited** — a custom validator does not re-discharge them. The settlement-debit `checked_sub` underflow
makes an over-admitting (contract-violating) proposer's block a *detectable invalid block*, so a faulty
economic contract cannot compromise consensus safety regardless of its proof bundle.

## Obligation checklist (S1–S4 + P3)

| Clause | Obligation | Spec | Prove for your decision functions |
| --- | --- | --- | --- |
| S1 | token present per signed communication; reject malformed | §6.3 | Rocq or Lean |
| S2 | accept iff `Σ_s ≥ Δ_s`, decided before execution | §7.6 | TLA+ (arithmetic) + Rocq or Lean |
| S3 | linear no-double-spend / reject-both | §7.7 | TLA+ (arithmetic) + Rocq or Lean |
| S4 | for-comprehension = atomic funded transaction | §7.1 | Rocq or Lean (inherited if you use the platform reducer unchanged) |
| P3 | determinism / replay-equivalence of the verdict | DR-12 | TLA+ (schedule-independence) + Rocq or Lean (step determinism) — **critical** |
| P1, P2 | slash-authorization, finalization safety | DR-12 | inherited from the Rust shell |

P3 is the most important to re-discharge: a non-deterministic custom gate forks the chain.

## Bundle structure — copy and adapt the built-in

The built-in validator's contract is the worked reference (it is proven in all three provers). Copy these and
adapt them to your decision functions:

- **TLA+** — `formal/tlaplus/validator/Validator.tla`: `THEOREM`s proven deductively by TLAPS (z3 / zenon).
  Restate the funding / reject-both / slash-zeroing arithmetic of your gate and prove via `OBVIOUS` or
  `BY SMT`.
- **Rocq** — `formal/rocq/validator/theories/Contract.v`: `validator_contract_*` corollaries that re-export
  proven obligations. Prove S1–S4 + P3 for your functions; `Print Assumptions` must report "Closed under the
  global context" (axiom-free).
- **Lean** — `formal/lean/Validator/Contract.lean`: `validator_contract_built_in_*` clauses. `#print axioms`
  must show no `sorryAx`.

## Checking your bundle (LOCAL-ONLY)

Run the same local gates the built-in passes (per team policy these are never CI gates):

```bash
bash scripts/check-cost-accounted-rho-proofs.sh         # Rocq: compile + axiom-free
bash scripts/check-cost-accounted-rho-tla-invariants.sh # TLA+: TLAPS "All N obligations proved" + TLC
bash scripts/check-cost-accounted-rho-lean.sh           # Lean: build + #print axioms (no sorryAx)
```

A custom validator ships TLA+ **plus** Rocq **or** Lean (the built-in ships all three). A bundle is accepted
when its gate(s) pass with the contract obligations reported axiom-free / sorry-free.
