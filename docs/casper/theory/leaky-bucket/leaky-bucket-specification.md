# Leaky-Bucket Liveness, Adaptive Phlo, and Fault-Tolerance Bebugging — Design Specification

**Status:** Design proposal (unimplemented). This document is *normative for the
intended feature set*, but it is not yet backed by a machine-checked correctness
proof and no code path in the tree implements it today.

**Version 0.1 · 2026-09-16**

**Related documents:**
- `docs/casper/CONSENSUS_PROTOCOL.md` — current consensus pipeline
- `docs/casper/theory/slashing/slashing-specification.md` — current slashing model
- `docs/casper/theory/fork-choice/fork-choice-specification.md` — LMD-GHOST fork choice
- `docs/casper/theory/deploy-occurrence/deploy-occurrence-specification.md` — cross-DAG duplicate-deploy handling (the O1–O15 invariants; used to reason about clients submitting the same deploy to multiple validators to defeat censorship)
- `docs/casper/theory/cost-accounting-decision-records.md` — cost accounting DRs (DR-9 = decision to enforce token-per-COMM; per-op gas demoted to diagnostic)
- `docs/casper/theory/cost-accounting-impl/workstream-d-acceptance.md` — staged implementation plan D0–D6
- `docs/casper/theory/cost-accounting-impl/d3-replace-phlo-with-tokens.md` — the D3 workstream that realized DR-9 (replaced `phlo_limit`/`phlo_price` with per-signature token pools; sole cost authority is the acceptance gate)
- `docs/casper/theory/uptime/shard-failure-modes.md` — uptime failure modes

**Companion source anchors (all relative to repo root):**
- `casper/src/rust/estimator.rs` — parent selection
- `casper/src/rust/blocks/proposer/block_creator.rs` — deploy admission + block build
- `casper/src/rust/validate.rs` — block validation pipeline
- `casper/src/rust/finality/floor.rs` — finalized floor
- `casper/src/main/resources/PoS.rhox` — PoS Rholang macro
- `casper/src/rust/genesis/contracts/proof_of_stake.rs` — PoS Rust wiring
- `rholang/src/rust/interpreter/accounting/costs.rs` — per-op cost constants
- `rholang/src/rust/interpreter/metering.rs` — per-signature budget

---

## Table of Contents

1. [Motivation and scope](#1-motivation-and-scope)
2. [Notation and terminology](#2-notation-and-terminology)
3. [Component 0 — Leaky-bucket stake accounting](#3-component-0--leaky-bucket-stake-accounting)
4. [Component 1 — Soft phlo cap with quartic overflow pricing](#4-component-1--soft-phlo-cap-with-quartic-overflow-pricing)
5. [Component 2 — Client quality-of-service tokens](#5-component-2--client-quality-of-service-tokens)
6. [Component 3 — Variable leak rate under long-running deploys](#6-component-3--variable-leak-rate-under-long-running-deploys)
7. [Component 4 — Bebugging (fault-tolerance credit)](#7-component-4--bebugging-fault-tolerance-credit)
8. [Cross-component invariants](#8-cross-component-invariants)
9. [Data structures and interfaces](#9-data-structures-and-interfaces)
10. [Consensus messages introduced](#10-consensus-messages-introduced)
11. [Validation and admission pipeline changes](#11-validation-and-admission-pipeline-changes)
12. [Parameter table](#12-parameter-table)
13. [Test and verification plan](#13-test-and-verification-plan)
14. [Non-goals and deferred work](#14-non-goals-and-deferred-work)

---

## 1 · Motivation and scope

The current consensus and PoS layers punish attributable equivocation
(`docs/casper/theory/slashing/`) but under-punish or fail to punish
**omission-class** validator misbehavior:

| Misbehavior class | Currently detected? | Currently punished? |
|---|---|---|
| Equivocation on same seq | Yes (`equivocation_detector.rs`) | Yes (slash, bond → 0) |
| Locally invalid block | Yes | Yes |
| Refusing to author blocks (lazy validator) | No | No — epoch-mint and fee-pool distribute pro-rata by stake regardless of activity |
| Selecting only own blocks as parents | Not enforced beyond fork-choice preference | No |
| Only executing deploys targeting a subset of contracts | No | No |
| Client saturating the network with one very long deploy | Partially (byte cap) | No pricing pressure |

Meanwhile the slashing layer is **too sharp** for transient hardware faults:
a bit-flip that corrupts a signature or a state hash causes irrecoverable
bond loss, even though the fault carries no adversarial intent.

This specification introduces five inter-locking mechanisms that together
address both directions:

- **Component 0 — Leaky bucket.** An off-chain, per-validator subjective view
  of every other validator's *earning capacity*. Buckets drain over time and
  refill on block production. A drained bucket triggers a soft ejection (grace
  period), and unresolved ejection escalates to slashing. Rewards are a
  function of parent count and recency so that lazy fork-choice does not pay.
- **Component 1 — Soft phlo cap.** A step-indexed multiplier
  `Φ_soft(n)` is applied to each *fundamental step* of a deploy, on
  top of the existing per-op cost table (which assigns each op some
  number of fundamental steps). In v0.1 `Φ_soft` is flat for the
  first `k` fundamental steps and grows cubically thereafter, so the
  total bill for a deploy rises as the fourth power of fundamental
  steps beyond `k`. There is no hard cap, so any deploy can complete,
  but very long deploys become correspondingly expensive. The schedule
  is per-deploy (not per-block) so a deployer is never charged more
  because an unrelated earlier deploy in the same block ran long.
  When `Φ_soft ≡ 1` the pricing collapses to today's cost table.
- **Component 2 — Quality of Service tokens.** Clients acquire QoS tokens that
  gate new deploy submissions. Refill rate is a function of measured network
  slack; under load, admission slows for everyone but the network stays live.
- **Component 3 — Variable leak rate.** The leak rate of Component 0 slows
  while the network is executing a long-running deploy. Followers who took a
  long time processing an honestly-slow block are not penalized for it.
- **Component 4 — Bebugging.** Each validator periodically receives a
  *fault-tolerance credit* that can absorb one otherwise-slashable offense.
  Credits ensure honest validators are not punished for transient faults
  (bit flip, brief clock skew, GC pause) and, dually, keep detection paths
  exercised because credits do not silence detection, only enforcement.

Scope boundaries are stated in §14.

---

## 2 · Notation and terminology

We reuse the vocabulary of `docs/casper/GLOSSARY.md` and add:

- **Bucket capacity `C(V)`** — the maximum nominal stake credit a validator
  `V`'s bucket can hold. By default `C(V) = bond(V)`.
- **Bucket level `b_V^U(t)`** — validator `U`'s local estimate at wall-clock
  time `t` of the current level in `V`'s bucket, in the same units as bond.
- **Overflow account `o_V^U(t)`** — `U`'s local estimate of the block
  reward earned by `V` that has spilled past `V`'s bucket capacity.
  Grows monotonically as `V` produces blocks; only a successful
  `ClaimOverflow` from `V` (§3.6) debits it. Overflow is never used
  to refill `V`'s bucket.
- **Leak rate `λ(t)`** — the current per-unit-time rate at which every
  validator's bucket drains, subject to Component 3 adjustments.
- **Refill amount `R(B)`** — the reward paid to the proposer of block `B`.
  Depends on `B`'s parents (see §3.4).
- **Ejection threshold `θ_eject`** — fraction of `C(V)`; when `b_V^U(t)`
  drops below `θ_eject · C(V)` for some quorum-visible interval, `U` is
  authorized to submit an *eject-V* transaction. Default `θ_eject = 0.9`.
- **Grace period `Δ_grace`** — the number of blocks after ejection during
  which `V` is exempt from block-issuance expectations and may top up.
- **Top-up transaction** — a signed message from a previously-ejected
  validator that funds enough stake into `posVault` to be reinstated.
- **QoS token `q_c(t)`** — a unit of admission budget held by client `c`.
- **Per-step phlo multiplier `Φ_soft(n)`** — a monotonically
  non-decreasing scalar-valued function of the fundamental-step
  index `n` (0-based). The meter has only per-op visibility, so
  `Φ_soft` is applied via a **left Riemann rule**: when a deploy is
  about to execute its `j`th op, the meter reads `Φ_soft(N_j)` where
  `N_j = Σ_{i<j} c_base(op_i)` is the cumulative fundamental-step
  count *before* the op, multiplies by `c_base(op_j)` (the op's
  fundamental-step count from the cost table
  `rholang/src/rust/interpreter/accounting/costs.rs`), charges the
  result, and advances the counter by `c_base(op_j)`. The bill is
  `bill(D) = Σ_j Φ_soft(N_j) · c_base(op_j)`. When `Φ_soft ≡ 1` this
  collapses to `Σ c_base(op_j)`. Component 1 uses the per-op cost
  table as a measure of computational work done, layered *on top
  of* D3's COMM-per-token consensus billing rather than replacing
  it (see §4.6). Applied independently to each deploy; two deploys
  in the same block have independent counters. Any monotone shape
  is admissible; the canonical v0.1 shape is *constant then cubic*
  per step (see §4.2), which integrates to a quartic total-cost
  dependence beyond the knee — the original design intent for the
  fourth-power ramp.
- **Bebugging window `Δ_credit_period`** — the block-count duration
  of one bebugging window. Default: `epoch_length` (one epoch per
  window). At each window boundary every active validator's credit
  balance is reset (§7.2).
- **Bebugging credit `κ_V`** — non-negative integer count of
  remaining slashing-immunity uses for validator `V` in the current
  window. Default issuance is `κ_issue = 1` credit per window, with
  no carry-over: unused credits expire at the next window boundary.
  See §7.2 for the use-it-or-lose-it rationale.

All wall-clock quantities are subjective and per-validator; only the
*consensus decisions* that Components 0–4 authorize (eject, reinstate,
overflow claim, credit issuance) become part of state. See §8.1 for the
consensus rule that turns subjective views into agreed transitions.

---

## 3 · Component 0 — Leaky-bucket stake accounting

### 3.1 Purpose

Punish omission-class validator behavior by tying block-reward
accumulation to *actual, useful* block issuance and by draining reward
capacity over time.

### 3.2 State (off-chain, per validator)

Every running validator `U` maintains a local map

```
BucketView[U] : Validator -> (level_b, overflow_o, last_update_ts)
```

**This state does not live on chain.** It is a purely local, subjective
projection derived from the messages `U` has processed. Two validators
`U₁` and `U₂` may hold different views for the same target `V`.

**Entry lifecycle.** The set of keyed validators tracks the set of
in-play bonds — no historical tail, no LRU:

- **Insert** on first observation of a `V` that is bonded per the
  current finalized-floor state (either present at genesis or added
  via a successful bond deploy).
- **Retain** while `V` is bonded, whether active, ejected (§3.5 —
  bucket is frozen, overflow is preserved per §3.3 monotonicity),
  or unbonding-in-quarantine (§3.11 — `V` may still submit a final
  `ClaimOverflow` before bond redemption).
- **Drop** when `V`'s bond exits the system — either the unbonding
  quarantine expires and the payout completes, or a slashing
  quarantine finalises. Under both exits, any residual `o_V^U` is
  no longer redeemable by `V` (a non-bonded validator is not in
  any future claim electorate under §3.10) and can be discarded
  without loss.

Slashed validators forfeit unclaimed overflow along with their
bond; this is consistent with the existing slashing spec's
principle that misbehaviour costs the full economic stake. An
honest voluntary unbonder does not forfeit — they have the full
quarantine window to submit any final `ClaimOverflow` (subject to
§3.12's one-open-per-target rule).

Recommended concrete location: a new sibling of the existing
`FloorContext` under `casper/src/rust/finality/`, tentatively
`casper/src/rust/economics/bucket_view.rs`, with per-`Validator`
entries keyed by the same `Validator` type used by
`authority_stakes()`. Persistence is *node-local* (an in-memory
`HashMap` is sufficient at v0.1 given the bounded active-set size;
LMDB persistence is optional for warm-restart friendliness). The
map's size is bounded by `|activeValidators| + |ejectedValidators|
+ |pendingWithdrawers| + |withdrawers|` — all quantities already
bounded by PoS-genesis parameters (`number_of_active_validators`,
etc.).

### 3.3 Update rules

Between block observations, level decays at the leak rate:

```
b_V^U(t + Δt) = max(0, b_V^U(t) − λ(t) · Δt · C(V))
```

`λ(t)` is a network-observed rate (Component 3). The multiplication by
`C(V)` gives *proportional* decay — a large bond drains proportionally to
its capacity, so the ejection threshold `θ_eject · C(V)` is scale-free.

When `U` observes a valid block `B` proposed by `V`:

1. Compute `R(B)` per §3.4.
2. Let `refill_needed = C(V) − b_V^U(t)`.
3. `b_V^U ← b_V^U + min(R(B), refill_needed)`.
4. If `R(B) > refill_needed`, `o_V^U ← o_V^U + (R(B) − refill_needed)`.
5. Record `last_update_ts ← t`.

**Invariant (Overflow monotonicity).** The overflow account `o_V^U(t)`
is non-decreasing in `t` between successful `ClaimOverflow` events.
Reward flows are strictly *one-way*:

- Block reward `R(B)` fills the bucket first, then overflow.
- Rejoin top-up (§3.7) also fills the bucket first, then overflow —
  any surplus beyond the deficit `C(V) − b_V^U` at ratification time
  is credited to `o_V^U`, using the same rule as block reward.
- Overflow **never** flows back to the bucket, even when the bucket
  is below `C(V)` and the validator is at risk of ejection.
- Overflow is only debited by a `ClaimOverflow` deploy from `V`
  succeeding at the 2/3-vote threshold (§3.6).
- Overflow is **not** reset by ejection, grace, or rejoin. If `V` is
  ejected with overflow `o_V^U > 0`, that credit survives ejection
  and can be claimed by `V` after rejoin.

In particular, a validator with a large overflow account and a
draining bucket must submit `ClaimOverflow` and then `Rejoin` with a
fresh on-chain top-up — the overflow does not automatically preserve
the bucket. This keeps the two accounts semantically distinct:
*bucket* = "am I earning right now?", *overflow* = "unclaimed past
earnings".

**Determinism note.** Because different validators receive `B` at
different `t`, the *decay* they applied before observing `B` may differ.
Consensus over disagreement is handled by threshold-signed claims (§3.5,
§3.6), not by hoping views agree.

### 3.4 Reward as a function of parents

Let `B` have parents `P₁, …, P_k` where `P₁` is the LMD-GHOST head (see
`casper/src/rust/engine/multi_parent_casper/snapshot.rs::order_parents_by_ghost_head`).
Let the block's own height be `h(B)` and each parent's height be `h(P_i)`.

Define the *breadth factor* and *recency factor*:

```
breadth(B)  = Σ_{i=1..k} w_i               where w_i is a stake-weighted parent share
recency(B)  = Σ_{i=1..k} 1 / (1 + h(B) − h(P_i))
```

`w_i` uses the same finalized-floor stake weights that `Estimator::build_scores_map`
already uses. The intention is *not* to double-count the fork-choice
score; `w_i` here is a *reward attribution*, and it is normalized so that
`Σ w_i = 1` even if `k = 1`.

The reward is:

```
R(B) = R_base · f_breadth(breadth(B)) · f_recency(recency(B))
```

where `f_breadth` and `f_recency` are monotonically increasing,
saturating at 1. Concrete v0.1 shapes (subject to parameter tuning):

```
f_breadth(x) = 1 − (1 − x)^p_breadth        // encourages many independent parents
f_recency(y) = min(1, y / y_ref)            // caps the recency premium
```

**Key invariant:** a validator that only ever cites its own most-recent
block earns `R_base · f_breadth(1) · f_recency(1) = R_base · const`,
which is strictly less than the reward for citing a maximal parent set.
Concretely, a self-only proposer earns `R_base · (1 − (1 − w_self)^p_breadth)`
where `w_self` is that validator's stake share — the constant is bounded
away from 1 and drops as network stake diversifies.

Reward accumulation is entirely **inside `BucketView[U]`**; on-chain
minting is unchanged from today's epoch-mint pipeline
(`system_deploy_util.rs::generate_epoch_mint_deploy_random_seed`) *except*
that the epoch mint amount is a function of consensus-agreed overflow
claims (§3.6). See §11.4 for the wiring.

### 3.5 Ejection

**Trigger.** For any `U`, if `b_V^U(t) < θ_eject · C(V)` continuously for
`Δ_eject_observation` wall-clock time, `U` may submit an *eject-V*
transaction as an ordinary deploy.

**Content.** An eject-V transaction includes:

```
EjectV := {
    target: Validator,
    proposer: Validator,        // author = U
    proposer_view_snapshot: BlobHash,   // hash of U's serialized view of V
    round: EpochRoundNumber,
    signature: Sig,
}
```

The `proposer_view_snapshot` is a Merkle hash over `(target, level_b,
overflow_o, last_update_ts, recent_block_witnesses…)` that `U` commits
to publicly. It is inspection material for future adjudication (§3.7),
not consensus material.

**Consensus rule (threshold voting).** Each validator `W` that observes
an *eject-V* deploy consults `BucketView[W]` and casts a *yea* if its
own view agrees that `b_V^W < θ_eject · C(V)`, a *nay* otherwise. Votes
are attached to the next block `W` proposes via a new justification
extension (see §10). Ejection commits when the running yea-stake sum,
measured over the finalized-floor committee (`floor_context.rs`),
crosses `2/3` of active stake at some floor block `F` (the *ejection
floor*).

**Effect of ejection.**
1. `V` enters the *ejected* lifecycle state (new PoS state; see §9.3).
2. `V` retains bond and generation. `V` is *not* slashed.
3. During `[F, F + Δ_grace]`, `V` is exempt from Component 0's leak;
   `b_V^U` is frozen for all `U`.
4. `V` is removed from the *proposer-expected* set: heartbeat/liveness
   proposal tests do not treat `V`'s absence as evidence of stall.
5. `V` remains counted in the *stake* denominator for finality until the
   grace deadline (this preserves the current safety proof; see §8.2).

### 3.6 Overflow claims

`V` may withdraw accumulated overflow at will:

```
ClaimOverflow := {
    proposer: Validator,        // == V
    amount: B,                  // claimed overflow, in bond units
    round: EpochRoundNumber,
    signature: Sig,
}
```

Each validator `W` votes yea iff `o_V^W ≥ B`. The claim succeeds when
the yea-stake crosses `2/3` of the active-stake denominator at some
floor block. On success, `B` is *minted* from the PoS vault into `V`'s
address and `o_V^U` is debited by `B` for every honest `U` (locally, as
part of processing the successful claim).

**Bounding rule.** A single `ClaimOverflow` MUST NOT exceed
`o_max_per_claim`, a network parameter, to avoid unbounded off-chain
credit accumulation. `V` MAY submit multiple claims per epoch.

**Anti-DoS rule.** Failed claims (below the 2/3 threshold at deadline
`Δ_claim_deadline`) burn a bond-scaled *fee* from `V`. This discourages
speculative claims. The fee is credited to the coop vault.

### 3.7 Rejoin

An ejected `V` may return to service by submitting:

```
Rejoin := {
    proposer: Validator,        // == V
    topup: B,                   // paid into posVault
    round: EpochRoundNumber,
    signature: Sig,
}
```

Each `W` votes yea iff `b_V^W + topup ≥ C(V)`. The 2/3 threshold is
measured against the **ejection's retained snapshot** in
`ejectedValidators[V]` (§3.10), not against a fresh snapshot at
rejoin submission — the electorate that voted `V` out is the
electorate that decides whether `V` comes back. Only voters in that
snapshot may attach `RejoinVote` records; votes from non-snapshot
validators are ignored.

On success (2/3 snapshot stake before `V`'s grace deadline expires),
`V` returns to active status; the `topup` is debited from `V`'s
**on-chain** address and credited to `posVault`. Each honest `U`
applies the same fill-then-overflow rule used for block reward
(§3.3):

- Let `refill_U = C(V) − b_V^U(t_ratify)`. The bucket has been
  frozen since ejection (§3.5), so `b_V^U(t_ratify)` is a fixed
  subjective value per `U`.
- `b_V^U ← C(V)` (bucket filled to capacity).
- If `topup > refill_U`, `o_V^U ← o_V^U + (topup − refill_U)`
  (surplus flows to overflow).

Two observers with different subjective `b_V^U` at ejection time
will refill different amounts and credit different overflow deltas
from the same on-chain `topup` — but every `U` conserves
`refill_U + overflow_credit_U = topup`, and the divergence washes
out when `V` later submits a `ClaimOverflow` (2/3 vote adjudicates
the on-chain payout against those subjective views).

This is the only mechanism besides block reward (§3.3) that can
increase `o_V^U`. The overflow monotonicity invariant (§3.3) still
holds: overflow only ever grows here.

If `V` needs on-chain balance to fund the top-up and holds only
off-chain overflow, `V` must first submit and settle a
`ClaimOverflow` (§3.6). `V` MAY intentionally over-top-up (paying
more than the minimum needed to cross 2/3) as a way to pre-fund
their overflow account for a future `ClaimOverflow` — this is
economically neutral (`V` pays in on-chain, credits accrue
off-chain, and can be claimed back later).

**Timeout rule.** If grace expires without a successful rejoin, `V` is
**slashed** through the existing slashing pipeline (§ slashing-spec §5),
with the ejection floor and its `2/3`-threshold witness as authorization
evidence. This is the *only* way Component 0 causes slashing.

### 3.8 Interaction with existing slashing

Component 0 introduces two new offense classes and leaves
attributable equivocation slashing unchanged:

- **Grace-expiry** (§3.7): a validator that fails to rejoin
  within `Δ_grace` after ejection is slashed via the existing
  slashing pipeline, with the ejection-floor 2/3-witness as
  authorization evidence.
- **Provably-dishonest vote** (§3.14): a validator whose Component
  0 nay-vote is contradicted by its own on-chain signed ancestor
  chain is slashed. Evidence is `V`'s vote + `V`'s testimony
  block. Adjudication is fully objective (public signatures,
  public reward table).

Existing machinery is untouched:

- The `EquivocationDetector` (`casper/src/rust/equivocation_detector.rs`)
  and `SlashAuthError` predicates
  (`casper/src/rust/util/slashing_authorization.rs`) remain as-is.
- Both new offense classes route through the standard slashing
  pipeline (`docs/casper/theory/slashing/slashing-specification.md` §5).
- The bebugging credit (Component 4) applies uniformly to
  equivocation, grace-expiry, and provably-dishonest-vote
  slashings.

### 3.9 Interaction with transaction fees

**Problem.** Today's PoS (`PoS.rhox` epoch-change step 1) computes
"epoch rewards" pro-rata by *active-validator bonding proportion*
and adds them to `committedRewards`. Transaction fees flow through
this pool. If fees materially exceed the leaky-bucket reward `R(B)`,
a lazy validator continues to earn their pro-rata fee share as long
as they remain in `activeValidators`, and Component 0's bucket drain
bites only the smaller reward channel. The bucket is toothless in a
fee-dominant economy.

**Rule.** Fee eligibility is gated on being *active and not ejected*:

1. Fees and epoch mint continue to pool via `committedRewards` on the
   existing cadence.
2. At each distribution event, iterate over `activeValidators ∖ ejectedValidators`
   (using the new `ejectedValidators` PoS state from §9.3), not
   `activeValidators` directly.
3. An **ejected** validator (§3.5) forfeits its pro-rata share for
   the duration of the grace period. The forfeited share
   redistributes pro-rata among the remaining eligible validators.
4. A **slashed / quarantined** validator forfeits its share for the
   entire quarantine, as today.

**Determinism.** The gate reads only on-chain state (the PoS active
and ejected maps that Component 0 already maintains). No subjective
bucket-level enters the fee flow; the bucket only determines *when*
ejection commits, and ejection is itself the result of a 2/3 vote
(§8.1). Replay therefore agrees on the fee distribution set at every
epoch boundary.

**Economic effect.** A validator that neglects block production
faces four cumulative penalties: (i) bucket drain, (ii) ejection at
`θ_eject`, (iii) forfeiture of *all* fee and mint income during
grace, and (iv) grace-expiry slashing if unresolved. The moment
ejection commits, the validator's entire reward stream stops — the
ratio of fees to `R(B)` becomes economically irrelevant. Laziness is
uneconomic regardless of fee-market conditions.

**Timing note.** Because `committedRewards` accrues throughout the
epoch and settles at epoch close, a validator ejected mid-epoch
forfeits its share of *all* fees pooled during that epoch, not only
those pooled after ejection. This binary rule (all-or-nothing at
the distribution block) is deliberate: it is deterministic, simple,
and reinforces the anti-lazy incentive by making the punishment
sharp at the moment ejection commits.

### 3.10 Stake-weight snapshot for votes

The 2/3-threshold rules for `EjectV` (§3.5), `ClaimOverflow` (§3.6),
and `Rejoin` (§3.7) all measure yea-stake against a denominator
that must remain well-defined even when validators bond, unbond,
are ejected, or are slashed during a claim's lifetime.

**Rule.** Each claim `C` submitted in block `B_sub` carries an
implicit **stake snapshot** derived from the finalized-floor
committee (`casper/src/rust/finality/floor_context.rs::authority_stakes`)
at `B_sub`'s pre-state. The snapshot records:

- The eligible voter set (bonded, active, non-ejected validators at
  `B_sub`'s pre-state).
- Each voter's stake weight (bond value at that floor).

All votes attached to `C` at any block within its lifetime
`[B_sub, B_sub + Δ_claim_deadline]` are tallied against this
snapshot. The 2/3 threshold is computed against the snapshot's
total, not the current live stake.

**Consequences.**

1. **Unbonding does not retroactively reduce a vote's weight.** A
   validator that voted yea and then submitted an unbond request in
   a later block still contributes its snapshot weight to `C`'s
   tally. Unbonds move stake through `pendingWithdrawers` →
   `withdrawers` over quarantine (`PoS.rhox` epoch step 3), but the
   snapshot is frozen at `B_sub`.
2. **Unbonding does not lower the denominator.** A partial tally
   that would fall short of 2/3 under the snapshot cannot cross the
   threshold simply because another validator unbonded. This blocks
   an "unbond-to-trigger" attack.
3. **Bonding does not add voters mid-claim.** A validator that bonds
   after `B_sub` is not in `C`'s snapshot; its votes on `C` are
   ignored. It becomes eligible for any *new* claim submitted after
   its bonding.
4. **Post-vote unbonders remain slashable.** A validator that voted
   yea and then unbonded stays in `withdrawers` (quarantined) until
   the quarantine deadline. With `quarantine_length` set to at
   least `2 · block_retention_window` (§3.11), that deadline
   dominates `Δ_grace` and covers the full provable-evidence
   horizon, so the bond is still available to slash if the vote
   itself constituted misbehavior.

**Ejection during a claim's lifetime.** A voter that is itself
ejected between `B_sub` and vote-attachment stays in `C`'s snapshot
at its pre-ejection weight. This prevents an adversarial sequence
that first ejects a key voter to reshape the denominator of an
in-flight claim. If the ejection itself is fraudulent, it fails to
reach 2/3 under whichever snapshot governs it; if legitimate, the
ejected validator's vote on `C` is one last on-the-record action
before entering grace.

**Slashing during a claim's lifetime.** A voter slashed between
`B_sub` and claim resolution is *removed* from `C`'s snapshot with
retroactive effect, and the denominator adjusts. This is the one
case where the snapshot changes: slashing implies the voter's prior
attestations are compromised, so honoring their weight would
undermine safety. The tally re-checks the 2/3 rule under the
adjusted denominator; a claim that was passing may now fail (or
vice versa) but not both retroactively — the tally is monotone
because slashing only removes stake.

**Implementation.** The snapshot is stored inline in the claim
record when the claim enters `overflowClaims`, `ejectionEvidence`,
or open-`rejoin` sets (§9.3). It is a `Map[Validator, i64]`
mirroring `authority_stakes()` at `B_sub`'s pre-state. Vote
admission (§11.1) checks membership against the snapshot, not
against current bonds. Because the snapshot is derived from
finalized-floor state at admission, replay is deterministic.

**Ejection lifecycle: snapshot persists past commit.** An ejection
is not a transient claim — it initiates a multi-block lifecycle
(`ejected` state → grace → rejoin *or* grace-expiry slashing) that
can outlast the initial claim's `Δ_claim_deadline`. To keep this
lifecycle stable against bond turnover, the ejection's snapshot is
**retained on-chain** in the `ejectedValidators` record (§9.3)
after the ejection commits, and is reused for:

- The 2/3 threshold of the corresponding `Rejoin` claim (§3.7) —
  rather than taking a fresh snapshot at `Rejoin` submission.
- The authorization evidence for grace-expiry slashing (§3.7
  timeout rule).

Rationale: the electorate that voted V out is the electorate that
decides whether V comes back. This prevents an adversarial sequence
where a bloc unfavorable to a rejoin unbonds during the grace
period to reshape the rejoin's electorate, and it symmetrically
prevents a bloc friendly to V from bonding in during grace just to
tip the rejoin. Newly-bonded or unbonded validators during grace
remain eligible to participate in *other* concurrent claims; only
this specific rejoin is bound to the ejection's snapshot.

The slashing-retroactive-removal rule above continues to apply: if
a voter is slashed during grace, they are removed from the
ejection's retained snapshot with the same monotone-tally
consequences.

This is a targeted override of the general rule that "each claim
snapshots at its own submission" — it applies only to rejoin
claims linked to an existing ejection.

### 3.11 Extended unbonding delay

The snapshot rule in §3.10 relies on unbonders remaining slashable
during a claim's lifetime, and the ejection-lifecycle rule further
requires that a voter who unbonds after voting on an `EjectV` is
still in PoS quarantine when the corresponding `Rejoin` resolves —
possibly `Δ_grace` blocks later, plus additional time for evidence
to surface. This lets Component 0 punish adversarial vote-then-flee
sequences.

**Rule.** `quarantine_length` MUST cover the network's *slashing
evidence horizon*: the maximum wall-clock interval during which
evidence for a slashable offense can be produced and verified.
Concretely,

```
quarantine_length ≥ Δ_grace + evidence_horizon + safety_margin
```

where

- `Δ_grace` covers the full ejection→rejoin lifecycle
- `evidence_horizon` is the block-retention window (the time an
  honest node is guaranteed to keep a block available for
  cross-reference against equivocation evidence)
- `safety_margin` covers clock skew, propagation lag, and
  operator-side response time (a few epochs is typical)

**Concrete recommendation.** Given the current expected block
retention window of roughly one week, the recommended default is
`quarantine_length = 2 · block_retention_window`, i.e. **~2 weeks
of blocks** (≈ `1.2 · 10^6` blocks at 1-second block time). A ~2×
multiplier on the retention window keeps a bond attachable through
any evidence that a peer could plausibly still verify, plus one
retention window of safety margin.

Do not simply pick a large calendar figure. The lock is only
usefully doing work over the window in which the network can
*prove* misbehavior against the retained block history. Beyond
that window, the bond is idle — punishing unbonders for a
consequence the network can no longer establish.

**Rationale.** Three related arguments justify a quarantine of at
least `2 · block_retention_window`:

1. **Slashing-window coverage.** Equivocation and invalid-block
   evidence require the offending block to still be available for
   verification. If honest nodes retain blocks for one week, then
   after one week no node can attest to the evidence and slashing
   is not adjudicable. A quarantine longer than the retention
   window keeps the bond attachable for the entire *provable*
   detection horizon; a quarantine substantially beyond it just
   idles capital.

2. **Long-range attack resistance.** A retired validator's signing
   key is cryptographically valid on any alternate history that
   recognises them, but a long-range attack against F1r3node is
   primarily prevented by weak subjectivity — new nodes accept a
   known-good checkpoint younger than the retention window. The
   quarantine's contribution is secondary: it ensures that within
   the retention window, alternate-history evidence can still
   attach to slashable stake. Beyond that window, the *social*
   check (weak subjectivity) does the work, not the *economic*
   check.

3. **Ejection-lifecycle safety.** With
   `quarantine_length > Δ_grace`, a voter who unbonds immediately
   after voting on an `EjectV` is provably still in `withdrawers`
   when the ejection resolves and when any grace-expiry slashing
   fires. The 2× retention-window default dominates `Δ_grace =
   2 · epoch_length` at any reasonable epoch size.

**Precedent.** Cosmos SDK uses 21 days, motivated by the
Tendermint governance window (a shorter chain-security concern than
Component 0's). Ethereum's queue-based withdrawals produce delays
on the order of days to weeks depending on load. Silvermint's
six-month lock reflects a longer retention/replay horizon than
F1r3node currently plans; adopting that figure verbatim would idle
capital past the point where slashing can actually adjudicate.

**Migration.** Bonds cannot be retroactively locked. This policy
applies to bonds staked (or re-staked) *after* the protocol
upgrade activating Component 0. Existing bonds retain their
original quarantine terms until the validator rebonds. Genesis
documentation should surface this parameter prominently so
validators understand the commitment before entering the active
set.

**Retention floor.** `block_retention_window` is a protocol-level
constant, not a per-node operational choice. Finalized-floor
committee membership requires attesting to retention of every
block from the current finalized floor back to the start of the
retention window; a node with shorter retention is excluded from
the authoritative committee. This makes "at least one honest node
still holds every in-quarantine block" provable throughout
`quarantine_length`. Implementation touches the finalized-floor
spec (`docs/casper/theory/finalized-floor/finalized-floor-specification.md`)
— coordination task, not an open design question.

### 3.12 Claim conflicts and serialization

Two concurrent `ClaimOverflow` deploys from the same proposer `V`
draw from the same off-chain overflow account `o_V^U`. If both are
admitted and each independently crosses 2/3 — which is possible
because each honest voter `W` checks only `o_V^W ≥ B_claim` for
that claim in isolation — `V` would be paid twice for the same
earned overflow. Similar hazards exist for `EjectV` (duplicate
submissions for the same target) and `Rejoin` (spamming rejoin
attempts at different top-up amounts until one crosses).

**Rule (One-open-per-target).** For each claim family, at most
one claim may be open at a time per target validator:

- `ClaimOverflow`: one open per proposer `V`.
- `EjectV`: one open per target `V`. Observers other than the
  first submitter attach yea votes to the existing open claim
  rather than submitting duplicates (§10 vote attachment).
- `Rejoin`: one open per proposer `V` (already keyed by
  `Validator` in `pendingRejoins`, §9.3).

Enforced by keying the corresponding PoS state maps by
`Validator`:

```
"ejectionEvidence"  : Map[Validator, (EvidenceRecord, StakeSnapshot, submittedAt, ClaimId)],
"overflowClaims"    : Map[Validator, (ClaimStatus, StakeSnapshot, submittedAt, ClaimId, amount)],
"pendingRejoins"    : Map[Validator, (RejoinPayload, submittedAt)],
```

`ClaimId` is retained inside each record for cross-referencing
votes and audit logs, but it is no longer the primary key.

**Admission gate.** Deploy admission (§11.3) rejects a
`ClaimOverflow` / `EjectV` / `Rejoin` from or targeting `V` if the
corresponding map already contains an open entry for `V` — i.e.
one whose `submittedAt + Δ_claim_deadline > current_block_number`.
The rejected deploy fails at admission the same way an unfunded
deploy fails today; no state changes.

**Same-block duplicates.** Two deploys of the same family
targeting the same `V` submitted in the same block are ordered by
deploy sequence within the block; the first is admitted, later
duplicates fail the admission gate on the intra-block updated
state. This preserves determinism because deploy order within a
block is already canonical.

**Slot clearing.** When a claim resolves (2/3 pass, 2/3 fail, or
`Δ_claim_deadline` expiry), its entry is removed from the map.
`V` may then submit a fresh claim of the same family. For
`EjectV` specifically, a *pass* also creates the corresponding
`ejectedValidators` entry (§3.5) and further `EjectV(V)` are
implicitly blocked while `V` is already ejected.

**Interaction with concurrent `ClaimOverflow` growth.** If `V`
opens `ClaimOverflow(B)` and continues producing blocks while
the claim is in flight, its subjective overflow `o_V^U` at each
observer keeps growing. `V` cannot bundle the newly-earned
overflow into the open claim — it must wait for the claim to
resolve, then submit a fresh one. This is a mild inconvenience
that trades against the double-spend hazard; `V` can size its
claims generously up front to reduce the number of round-trips.

**Rationale.** The one-open-per-target rule is the cheapest
serialization mechanism that closes the double-spend hazard for
`ClaimOverflow` and prevents amplification attacks on `EjectV`
and `Rejoin`. It has no interaction with the snapshot rule
(§3.10) — each successive claim gets its own fresh snapshot at
its own `B_sub` — and no interaction with the stake-weight
tally.

### 3.13 Mandatory vote participation

**Problem.** The 2/3 vote threshold on `EjectV`, `ClaimOverflow`,
and `Rejoin` (§§3.5–3.7, 3.10) requires validators to *actively*
attach yea votes. A validator that does nothing — attaches no vote
either way — contributes zero to the tally. If enough validators
are silent, no claim ever passes. This would expose Component 0 to
a coordination failure of the prisoner's-dilemma type: silent
rejection is individually free and, in aggregate, blocks every
Component 0 transition.

**Rule.** A block proposer MUST attach a vote (`yea` or `nay`) to
every open Component 0 claim visible in its pre-state, subject to
the `N_vote_max` bound (§10). Non-attachment invalidates the
block. This turns silent-rejection into an active choice that
appears on the record.

Combined with §3.14 (provably-dishonest votes are slashable), the
equilibrium is:

- **Yea when the claim is corroborated by the voter's own signed
  history** — safe, free, honest.
- **Nay when the voter genuinely disagrees and their signatures
  don't contradict** — free (no fee), on-record.
- **Nay against the voter's own on-chain signatures** — slashable
  under §3.14.

**Why no objection fee.** An earlier draft of this specification
included a symmetric objection fee (`ε_nay · bond` debited on
nay-votes, refunded if the vote's side wins). It has been dropped
because:

- §3.14 is a strictly stronger deterrent for provably-dishonest
  votes — slashing dwarfs a fractional-of-per-mil fee.
- Genuine disagreement should be free; charging honest nays chills
  the very speech that makes the mechanism trustworthy.
- The fee added a parameter (`ε_nay`), refund accounting at claim
  resolution, and one more determinism concern, for a marginal
  deterrent that §3.14 already provides more sharply.
- The thin-testimony attack — a validator building only on its own
  blocks so that its signatures never corroborate peers' output —
  is already discouraged by §3.4's parent-aware reward function,
  which pays such a validator strictly less.

**Underlying reasoning.** Component 0 is a repeated cooperation
game with a bounded validator set. Rational long-lived validators
face reciprocity (reject my claim today and I reject yours
tomorrow), but this specification does not rely on validators
correctly reasoning about the repeated game — the combination of
mandatory participation (§3.13) and provably-dishonest-vote
slashing (§3.14) is designed to be safe under one-shot rationality
too.

**Interaction with bebugging.** A validator whose vote is corrupted
by a transient hardware fault (e.g. a bit flip in the yea/nay
bit) can absorb the resulting §3.14 slashing under a bebugging
credit (§7.3). Detection still fires; enforcement is absorbed.

**Residual risk — rejoin obstruction.** §3.14 explicitly excludes
`Rejoin` votes because they depend on the subjective `b_V^W` at
ratification, not just on public signatures. This means a
coordinated bloc can nay-vote a legitimate `Rejoin` without
tripping §3.14. The residual defense is (a) mandatory
participation forces the obstructors to be visible; (b) §3.4's
reward function penalises thin-testimony validators; (c) repeated-
game reciprocity applies. If experience shows systematic rejoin
obstruction, a targeted mechanism (see the "Rejoin-obstruction
mechanism" v-next item in §14) can
be added later.

### 3.14 Provably-dishonest votes as slashable evidence

**Insight.** A validator `V` that authors a block whose parent
closure includes a `W`-authored block `B_w` has *cryptographically
committed* to having received and validated `B_w`. `V`'s signature
on the descendant block is proof that at the time of authoring,
`V` observed `B_w` as a valid block. `V` cannot later credibly
deny knowledge of `B_w`. By induction, the ancestor closure of
any `V`-authored block includes every `W`-authored block on the
paths back to the finalized floor. `V`'s public signatures thus
bound from below the amount of `W`'s production that `V` has
observed.

**Provable lower bound on `o_W^V`.** Let `Anc_W(V)` be the set of
`W`-authored blocks in the ancestor closure of `V`'s most recent
signed block. Let `Cum_W(V) = Σ_{B ∈ Anc_W(V)} R(B)` where `R(B)`
is the block reward per §3.4. Let `ClaimedOverflow_W` be the
on-chain sum of `W`'s successful past `ClaimOverflow` amounts.
Then

```
o_W^V ≥ max(0, Cum_W(V) − C(W) − ClaimedOverflow_W)
```

The `− C(W)` accounts for the bucket portion of every observation
(the bucket holds at most `C(W)` total; the rest spills to
overflow, §3.3 update rule). The `− ClaimedOverflow_W` subtracts
`W`'s already-collected overflow. Decay is not subtracted because
overflow is monotone (§3.3 monotonicity invariant). This is a
strict, deterministic lower bound derivable purely from public
signatures and on-chain state.

**Slashable offense.** If `V` attaches a `nay` vote to a
`ClaimOverflow(W, B)` where

```
B ≤ Cum_W(V_at_vote) − C(W) − ClaimedOverflow_W
```

then `V`'s vote contradicts `V`'s own cryptographic testimony.
`V_at_vote` is `V`'s most recent signed block at or before the
block that carries the `nay` vote — `V` is credited only for
observations that predate the vote. `V` is slashable for this
offense under the existing slashing pipeline
(`docs/casper/theory/slashing/slashing-specification.md`). The
evidence is `V`'s signed vote plus `V`'s signed ancestor chain,
both public; adjudication requires no subjective input.

**Evidence submission.** Any validator (typically `W`, though
anyone may submit) packages the evidence as a system deploy:

```
DishonestVoteEvidence := {
    accuser: Validator,
    accused: Validator,        // == V
    claim_id: ClaimId,
    vote_block: BlockHash,     // block carrying V's nay vote
    testimony_block: BlockHash,// V-signed block whose ancestor closure witnesses Cum_W
    signature: Sig,
}
```

Adjudication follows the standard slashing pipeline: authorization
check (per `slashing-specification.md` §5.3), evidence verification
(compute `Cum_W` from `testimony_block`'s ancestor closure, verify
`B ≤ Cum_W − C(W) − ClaimedOverflow_W`, verify the nay), and — if
valid — zero `V`'s bond and remove `V` from the active set.

**Bebugging interaction.** A single false-nay caused by a transient
hardware fault (e.g. a bit flip in the yea/nay bit) is absorbed
under `V`'s bebugging credit (§7.3) on equal terms with other
slashable offenses. The credit is consumed; the evidence is
recorded on-chain; no bond loss. Systematic dishonest voting
exhausts the credit and reaches enforcement.

**Applies symmetrically to `EjectV` votes.** The same
provable-lower-bound reasoning applies dually to ejection votes.
An honest `yea` on `EjectV(W)` asserts `b_W^V < θ_eject · C(W)`.
If `V`'s most recent signed block builds directly on a fresh
`W`-authored block whose reward exceeds `(1 − θ_eject) · C(W)` —
enough by itself to refill `W`'s bucket above the ejection
threshold from any legal starting level — `V`'s yea vote is
provably dishonest and slashable under the same evidence
mechanism.

`Rejoin` votes are excluded from this rule in v0.1: they depend
on `b_V^W` at ratification and `topup`, and `b_V^W` is a
subjective quantity not directly derivable from parent-set
testimony. Rejoin uses the §3.10 snapshot and §3.13 mandatory
participation; residual rejoin-obstruction risk is deferred to
§14 v-next work ("Rejoin-obstruction mechanism").

**Effect on the coordination equilibrium.** Combined with §3.13,
`V` has three coherent choices:

1. **Yea when your own signatures corroborate the claim.** Safe,
   free, honest.
2. **Nay when you genuinely disagree and your signatures don't
   prove otherwise.** Free, on-record — genuine disagreement is
   never penalised.
3. **Nay against evidence your own signatures already established.**
   *Slashable* under this section.

Silent obstruction is prevented by §3.13's mandatory participation
rule; active-dishonest obstruction is prevented by this section's
slashing rule; only genuine disagreement remains — the intended
equilibrium.

---

## 4 · Component 1 — Soft phlo cap with quartic overflow pricing

### 4.1 Purpose

Bound the damage a single very long deploy can do to block latency
while keeping the network Turing-complete in practice: any deploy can
eventually finish, but very long deploys pay super-linearly.

The schedule is **per-deploy**, not per-block. The step index resets
at the start of each deploy, so a deployer pays `Φ_soft(0), Φ_soft(1), …`
for their own steps regardless of how many steps an unrelated earlier
deploy in the same block consumed. This preserves fairness: every
deployer sees the same posted schedule regardless of block-mate
behavior.

### 4.2 Pricing function

Deploy `D` executes a sequence of ops `op_0, op_1, …, op_{|D|−1}`.
Each op is a coarse-graining of `c_base(op)` *fundamental steps* per
the existing cost table (`rholang/src/rust/interpreter/accounting/costs.rs`:
multiplication = 9, sum = 3, comparison = 3, …). Component 1
preserves that coarse-graining and applies a step-indexed multiplier
`Φ_soft(n)` — but only at op boundaries, because the meter has no
per-fundamental-step visibility.

**Pricing rule (left Riemann sum).** The meter maintains a per-deploy
fundamental-step counter `step_count`, initialized to 0. For each
op in order:

```
step_count = 0
cost = 0
for j = 0 .. |D| − 1:
    op_cost      = c_base(op_j)
    incremental  = Φ_soft(step_count) · op_cost
    cost        += incremental
    step_count  += op_cost
```

Equivalently, letting `N_j = Σ_{i<j} c_base(op_i)`:

```
c_eff(op_j) = Φ_soft(N_j) · c_base(op_j)
bill(D)     = Σ_j Φ_soft(N_j) · c_base(op_j)
```

This is a **left Riemann sum** approximating
`Σ_{n=0}^{N_D − 1} Φ_soft(n)`: `Φ_soft` is sampled once at the step
count *before* the op, and that value prices the entire op. Because
`Φ_soft` is monotone non-decreasing, the left rule is a lower bound
on the true per-step integral — an op whose step range straddles a
rising region of `Φ_soft` (in particular, an op that spans the
knee `k`) is charged as if the whole op ran at the pre-op multiplier.
This is a deliberate concession to metering simplicity: no
per-fundamental-step loop is required inside `reserve_cost`.

When `Φ_soft ≡ 1` the bill collapses to `Σ c_base(op_j)` — the
per-op cost table's aggregate.

`Φ_soft: ℕ → ℝ_{≥ 1}` is a monotonically non-decreasing multiplier
fixed by the network. Any monotone shape is admissible. The canonical
v0.1 shape is *constant then cubic per fundamental step*:

```
Φ_soft(n) = c_const                                 if n ≤ k
          = c_const · (1 + α · ((n − k) / n_ref)^3)  if n >  k
```

with parameters:

- `c_const` — ordinary-regime multiplier (default 1)
- `k`       — knee (fundamental-step count where overflow pricing engages)
- `α`       — overflow coefficient
- `n_ref`   — reference step-count scale for the cubic ramp

The **per-step** growth is cubic beyond the knee. Because
`bill(D) = Σ_j Φ_soft(N_j) · c_base(op_j)` is a left Riemann sum of
`Φ_soft` over fundamental-step index, the **total** bill grows as
the fourth power of fundamental steps beyond the knee — matching the
original "quartic in steps past the soft cap" design intent. Any
deploy still terminates given sufficient client balance; the price
simply rises super-linearly for outsized programs.

`Φ_soft` is a function of fundamental-step *position*, not of op
type. Because the meter samples once per op at the pre-op step
count, an op whose fundamental-step range straddles the knee (i.e.
`N_j < k < N_j + c_base(op_j)`) is charged at the pre-op multiplier
`Φ_soft(N_j)` for the whole op. The next op then sees a step count
past `k` and is billed on the overflow ramp. The knee is therefore
"felt" between ops rather than mid-op — the granularity of the
schedule is exactly the granularity of the op sequence.

The step counter is reset to zero at the start of every deploy. Two
deploys `D_1` and `D_2` in the same block have independent counters
even if `D_2` executes immediately after `D_1`.

**Why a multiplier, not a scalar threshold.** A scalar phlo threshold
would trigger overflow suddenly at a single boundary. Making
`Φ_soft` a smooth monotone multiplier over the existing per-op cost
table lets the network shape the ramp exactly, preserves the
coarse-graining of ops as bundles of fundamental steps, and — crucially
— needs only one function evaluation and one multiplication per op
inside `reserve_cost`, so the metering hot path stays cheap.

### 4.3 Placement

`Φ_soft(n)` is applied inside the metering machine
(`rholang/src/rust/interpreter/metering.rs::MeteredMachine::reserve_cost`).
The metering machine already scopes to a single deploy — `deploy_id`
is cached at construction (`metering.rs` line 41) and does not change
during evaluation — so a new *fundamental-step counter* is a per-
`MeteredMachine` field, advanced by `c_base(op)` on each billable
op. `reserve_cost` computes the op's effective charge as
`Φ_soft(step_count) · c_base(op)` (one function evaluation, one
multiplication) against the per-signature budget, then increments
`step_count` by `c_base(op)`. No plumbing to the block builder is
required: everything Component 1 needs is deploy-local.

Child metered machines (created via `MeteredMachine::child()` for
sub-components of the same deploy) share the deploy-scoped step
counter with their parent, since they represent the same logical
deploy.

**Flat-regime hot path.** While `step_count ≤ k`, `Φ_soft` returns
`c_const` and can be inlined as a constant multiplier — the charge
becomes `c_const · c_base(op)`. Ops that cross the knee mid-execution
are still charged at the pre-op multiplier `Φ_soft(step_count)` per
the left Riemann rule (§4.2); the overflow ramp only affects
subsequent ops once `step_count > k`.

### 4.4 Determinism

`Φ_soft(n)` is a pure function of the step index, which is a
deterministic quantity given a fixed deploy body and evaluation
order. Because pricing is deploy-local, replay agrees on the schedule
without needing to reconstruct any block-level ordering effect —
`Validate::validate_block_checkpoint` already replays deploys in
canonical order and each deploy's meter runs independently.

### 4.5 Client billing

The bill under Component 1 is:

```
bill(D) = Σ_j Φ_soft(N_j) · c_base(op_j)      // left Riemann sum, §4.2
```

The **existing D3 funding mechanism is unchanged**: the deployer
signs a maximum commitment `F` on the authority lane, the
`precharge` step in `PoS.rhox` locks `F` in `posVault` before
execution, and the `refund` step returns `F − bill(D)` on success.
If execution would cross `F` (`bill(D) > F`), the deploy fails,
state effects roll back, and the deployer forfeits their
commitment — the existing failure semantics, applied to the new
schedule. Component 1's only change is the value the meter
computes for `bill(D)` while executing; the commitment, refund,
and forfeit-on-failure semantics are inherited.

**Estimation.** Exact `bill(D)` is undecidable in general (halting
problem), so the deployer estimates by running the deploy locally
against the public `Φ_soft` and `c_base` tables, then commits with
a modest safety margin. Component 1's quartic ramp past the knee
`k` means over-provisioning far into the overflow regime is costly
if the deploy actually runs there, so accurate estimation matters
more than it did under a flat schedule.

### 4.6 Interaction with D3

**D3** is the cost-accounting implementation workstream (staged in
`docs/casper/theory/cost-accounting-impl/workstream-d-acceptance.md`,
detailed in
`docs/casper/theory/cost-accounting-impl/d3-replace-phlo-with-tokens.md`)
that realized decision record **DR-9**
(`docs/casper/theory/cost-accounting-decision-records.md`). D3
removed `DeployData.phlo_limit`, `phlo_price`, and the associated
escrow / precharge-refund fan-out, replacing them with
signature-indexed token pools on channels `Σ⟦s⟧` enforced by the
per-signature acceptance gate at block assembly. Under DR-9 the
per-op cost table in `costs.rs` is **diagnostic only** — retained
for telemetry, not consensus-authoritative.

This proposal does **not** reintroduce `phlo_limit`. Component 1
layers a step-indexed multiplier `Φ_soft(n)` on top of the per-op
step counts `c_base(op)`; when `Φ_soft ≡ 1` the total billing
reduces to `Σ c_base(op_j)`. The funding model (signature-indexed
tokens, acceptance-gate enforcement) is unchanged — a rational
deployer inspects `Φ_soft` (a static, on-chain parameter set)
alongside the cost table and estimates `bill(D)` for the
anticipated fundamental-step count before signing.

**Interaction with DR-9's demotion of `c_base`.** DR-9 demoted the
per-op cost table to *diagnostic* for the base consensus billing:
every COMM costs one token, regardless of the per-op values in
`costs.rs`. Component 1 uses `c_base(op)` in a distinct role — as
a measure of *computational work done* so that the overflow ramp
engages on actual effort (multiplication is 9 units of work, sum
is 3), not just on COMM count. This layers *above* DR-9's base
billing rather than replacing it: when a deploy hits the knee `k`,
the extra charge is `Σ_j (Φ_soft(N_j) − 1) · c_base(op_j)` — zero
if `Φ_soft ≡ 1`, cumulative over ops when the ramp engages. The
per-op table thus regains a consensus role only for the *overflow
premium*, and only when the deploy actually enters the ramp
regime. Coordination with `cost-accounting-impl` maintainers to
confirm compatibility with the D3 invariants is deferred to §14
("Cost-accounting-impl D3 compatibility review").

---

## 5 · Component 2 — Client quality-of-service tokens

### 5.1 Purpose

Rate-limit deploy submission on a per-client basis when the network is
under load, without introducing hard authentication.

### 5.2 Client identity

For QoS purposes a *client* is the `Ed25519` public key that signs a
deploy. No new identity concept is introduced; the signing key already
appears in every `DeployData`.

### 5.3 Token bucket per client

Each node maintains a local map

```
QoS[node] : ClientPubKey -> (tokens_q, last_refill_ts)
```

**This is per-node, not consensus state.** Two nodes may hold different
QoS views. This is acceptable because QoS only gates the *node-local
admission* pipeline (`block_creator.rs::DeployAdmissionPolicy`); it does
not block already-included deploys from being validated.

### 5.4 Refill rate

The refill rate for client `c` at node `N` at time `t` is:

```
refill_rate(c, N, t) = q_base · f_slack(observed_slack_N(t))
```

where `observed_slack_N(t) ∈ [0, 1]` is derived from:

- Recent block build-time p95 vs `Δ_build_target`
- Recent block admission byte reservation utilization (already tracked
  by `BlockAdmissionBudget`)
- Recent finality lag from `FinalityProgress` (`heartbeat_proposer.rs`)

Concrete v0.1 shape: `f_slack(x) = x^p_slack` with `p_slack ∈ [1, 2]`.

### 5.5 Admission gate

A submitted deploy from client `c` is admitted iff `q_c(t) ≥ 1`.
Admission debits one token; refusal returns a soft-error to the
client with a `retry_after_ms` hint.

**No deploy-gossip bypass needed.** In the current tree (see
§5.6), deploys have no peer-to-peer gossip layer of their own —
they enter a single node via the gRPC `DeployService` and reach
other nodes only after a proposer includes them in a block. Blocks
gossip (via `BlockHashMessageProto` / `HasBlockProto` /
`BlockRequestProto` in `comm/src/rust/transport/grpc_transport_receiver.rs`),
but blocks are not subject to QoS. Therefore every deploy meets
the QoS gate exactly once, at its originating node.

**Cross-node duplicate submissions are not deduplicated at the
QoS layer.** A client that submits the same deploy to two or more
different validators to defeat censorship pays QoS at each node
independently — that is intentional and not a bug. Consensus-level
duplicate handling is the responsibility of the deploy-occurrence
layer (`docs/casper/theory/deploy-occurrence/deploy-occurrence-specification.md`),
which guarantees via invariants O3–O5 that at most one active
finalized occurrence exists per deploy identifier regardless of
how many validators independently include it. QoS is a per-node
admission rate limiter; the deploy-occurrence layer is the
DAG-wide deduplicator. Keeping the two concerns separate lets the
censorship-resistance strategy work (submit-to-many) without
requiring QoS to reason about DAG state.

### 5.6 Placement

The gate lives at the deploy pool's single entry point:
`casper/src/rust/util/comm/grpc_deploy_service.rs::do_deploy` (or
its Rust equivalent — the gRPC `DeployService` handler). QoS state
lives in a new module `casper/src/rust/qos/`.

**Current-tree architecture that this rides on:**

- Deploys: single-node RPC entry only (no `HasDeploy` /
  `DeployRequest` / `DeployHashMessage` wire messages exist in
  `CasperMessage.proto`).
- Blocks: gossiped via `BlockHashMessageProto` announce +
  `HasBlockProto` / `BlockRequestProto` pull. Deduplication via
  `calculate_gossip_hash` in `grpc_transport_receiver.rs`.
- Transport: gRPC over TLS (`comm/src/rust/transport/f1r3fly_tls_transport.rs`).
- Peer discovery: Kademlia (`comm/src/rust/discovery/`).

If a future protocol version introduces deploy-level gossip
(e.g. a `HasDeploy` / `DeployRequest` mechanism to shrink the
first-inclusion latency), Component 2 would need a bypass rule so
that a deploy re-received from a peer does not double-charge the
client. That is not needed today.

---

## 6 · Component 3 — Variable leak rate under long-running deploys

### 6.1 Purpose

Ensure a validator that spends time executing a legitimately-long
deploy is not falsely accused of laziness.

### 6.2 Rule

The network leak rate `λ(t)` is modulated by a *slowness credit*
accumulated during long block-build events:

```
λ(t) = λ_base · (1 − slowness_credit(t))
```

`slowness_credit(t)` is computed locally by each observer `U` from `U`'s
own record of the most recent `N_slow_window` blocks it has replayed.
Specifically, letting `τ_i` be the replay time of block `i`:

```
slowness_credit(t) = clip( median_{i ≤ t}(τ_i) / τ_ref  − 1, 0, s_max)
```

`τ_ref` is the reference build/replay time (e.g. 90th-percentile normal
block). `s_max` is a network parameter (e.g. `0.75`), bounding the
maximum leak-rate reduction.

### 6.3 Determinism concern

Because `slowness_credit(t)` is subjective per validator, so is `λ(t)`,
and so is `b_V^U(t)`. This is intentional and consistent with §3.2:
the **agreed** transitions are eject/rejoin/claim-overflow votes, which
are decided by threshold. Different subjective decays produce
different individual votes, but the 2/3 threshold rejects biased local
views that outlie the network median.

### 6.4 Attack surface and interaction with §§3.13, 3.14

Component 3 makes each observer's leak rate `λ_U(t)` a function of
that observer's local `slowness_credit`, and therefore lets a
dishonest observer inflate its subjective view of any bucket
`b_W^U`. This section enumerates the resulting attack vectors and
which of them §3.13 (mandatory participation) and §3.14
(provably-dishonest votes) close.

**Attack A — fake-slow to prevent one's own ejection.** V reports
`slowness_credit_V ≈ s_max` in an attempt to keep V's own bucket
from draining. **Does not work.** V's own bucket is adjudicated by
*other* validators' `b_V^W` values, which are driven by *their*
`λ_W`, not V's (§6.3, §8.4 "No-fake-slow" invariant). Neither
§3.13 nor §3.14 is needed to close this — the mechanism itself
is immune.

**Attack B — fake-slow to inflate one's own overflow claims.**
V reports high slowness in the hope of raising its own overflow.
**Does not work.** Overflow `o_W^U` accumulates from block-reward
events per §3.3; it is monotone in observed rewards and
independent of λ. Slowness credit only affects bucket-level
*decay*, not overflow accumulation.

**Attack C — dishonest yea on `EjectV(W)` shielded by claimed
slowness.** X inflates `slowness_credit_X` to justify voting yea
on `EjectV(W)` when W is actually productive. **Closed by §3.14.**
The provable-lower-bound rule for `EjectV` yea votes checks a
fresh W-authored block's reward directly, and reward is an
objective function of W's parent set (§3.4). A single fresh
W-block whose reward alone exceeds `(1 − θ_eject) · C(W)` refills
W's bucket above the ejection threshold from any legal starting
level — no matter what λ_X the attacker claims. §3.14 slashes.

**Attack D — dishonest nay on `EjectV(W)` shielded by claimed
slowness.** A bloc that wants to *protect* a genuinely-lazy buddy
W nay-votes W's ejection, rationalising the nays as "my λ_X is
low, so my b_W^X is still above θ_eject." **Not fully closed.**
§3.14's `EjectV` rule catches only dishonest *yea* votes — the
"attempt to eject a producer" direction — because parent-set
testimony gives a *lower bound* on `b_W^V` (rewards observed), not
an *upper bound* (decay could be arbitrarily small under any λ_V
policy). §3.13 forces the attackers to vote publicly rather than
be silent, but the vote content itself is not falsifiable from
signatures alone in this direction.

**Residual bound on Attack D.** Preventing ejection requires the
bloc to keep the yea-tally below 2/3 of the ejection claim's
snapshot (§3.10). That requires the bloc to control more than
1/3 of the finalized-floor committee's stake — a pre-existing
Byzantine-fraction assumption break, not a slowness-credit
vulnerability per se. Slowness credit provides plausible deniability
for the nay votes, but it does not lower the stake threshold
required to succeed. If experience shows this to be exploited in
practice, candidate mitigations are deferred to §14
("Dishonest-nay coverage for `EjectV`").

**Attack E — coordinated slowness to shift the *network's* view.**
Individual `λ_U` are subjective, so there is no single "network
λ" to shift. Each observer's `slowness_credit` is derived from
that observer's local replay times (§6.2); no cross-node
aggregation exists. A bloc can inflate its own `λ_X` values but
cannot inflate an honest observer's `λ_W`. The 2/3 threshold on
claim resolution therefore requires the bloc's `λ_X`-driven
subjective views to be *outvoted* by honest observers with
correct `λ_W` — which happens automatically when the honest
supermajority acts on accurate replay times.

---

## 7 · Component 4 — Bebugging (fault-tolerance credit)

### 7.1 Purpose

Absorb the first slashable offense per validator per window as a way of:

1. Not punishing transient hardware faults (bit flips, brief clock skew,
   GC pauses, kernel scheduling anomalies).
2. Exercising the detection path continuously — because credits do
   **not** silence detection, only enforcement, an honest network keeps
   its slashing pipeline live even when no adversary is present.

### 7.2 Credit issuance (use-it-or-lose-it)

Every `Δ_credit_period` blocks (a round-number of epochs), each active
validator's credit balance is **overwritten** with `κ_issue` (default
`1`). Unused credits from the previous period are discarded — credits
do not accumulate across windows.

Concretely, the transition at each period boundary is:

```
κ_V ← κ_issue    for every V ∈ activeValidators
```

not

```
κ_V ← κ_V + κ_issue                 // rejected: allows save-and-burst
```

Rationale: an accumulation model would let a validator save credits
across many quiet epochs and then commit a burst of coordinated
Byzantine actions in one epoch — the credits would absorb every
offense in the burst, and Component 0 drain would take many blocks to
catch up. Use-it-or-lose-it bounds the worst-case damage from a
Byzantine validator to `κ_issue` offenses per `Δ_credit_period`
window, uniformly, regardless of how long they behaved before.

Credits are issued deterministically at the epoch-close system deploy
that already exists in the tree
(`system_deploy_util.rs::generate_epoch_mint_deploy_random_seed`).

Credit balances live on-chain in a new PoS state field
`"bebuggingCredits" : Map[Validator, Nat]`. The system deploy that
performs the reset is a new domain-tagged deploy
(`epoch-bebugging-credit:v1`, cf. §11.4) — the reset is a
*replacement*, not an increment, and this is part of the deploy's
observable effect.

### 7.3 Credit consumption

When the slashing pipeline determines that validator `V` has committed
a slashable offense (equivocation, invalid block, or grace-expiry
under Component 0), the pipeline first inspects `κ_V`:

- If `κ_V > 0`: consume one credit (`κ_V ← κ_V − 1`), emit a
  `SlashingAbsorbedByCredit` witness, and take **no** further slashing
  action. The evidence is still recorded — see §7.5.
- If `κ_V = 0`: proceed with the current slashing transition (bond →
  0, active set removal, mint halt) per `slashing-specification.md` §5.

Credit consumption is atomic with the offending block's inclusion; a
validator cannot spend the same credit twice.

### 7.4 Interaction with quarantine

Consuming a credit does not touch the quarantine machinery in
`PoS.rhox`. The offending validator remains bonded and active. This is
deliberate: transient hardware faults do not warrant even a temporary
loss of stake or activity.

### 7.5 Detection MUST fire

Component 4 modifies the slashing enforcement transition. It does
**not** modify:

- The `EquivocationDetector` invariants
  (`casper/src/rust/equivocation_detector.rs`)
- The `SlashAuthError` authorization predicates
  (`casper/src/rust/util/slashing_authorization.rs::authorize_slash`)
- Evidence archival — the `EquivocationRecords` set continues to accumulate

Formally, letting `SlashDetect` and `SlashEnforce` be the two phases:

- With `κ_V > 0`: `SlashDetect(V) → CreditAbsorb(V)`
- With `κ_V = 0`: `SlashDetect(V) → SlashEnforce(V)` (as today)

The `slashing-verification.md` correctness theorems that concern
*detection* (T-1, T-2, …) remain unchanged. Only the enforcement-side
theorems require a new invariant of the form: *if enforcement did not
fire, `κ_V` was strictly positive at the enforcement point*.

### 7.6 Adversarial bebugging

Under the use-it-or-lose-it model (§7.2) the worst-case adversary can
absorb at most `κ_issue` slashable offenses per `Δ_credit_period`
window. There is no way to save credits for a coordinated burst.

An adversary who "spends" their credit every window costs the network
nothing per instance, but the honest validators still record the
evidence and can use the *rate of credit consumption* as a soft
signal. A validator that consumes a credit every window is
misbehaving deliberately even if not slashable. Component 0 catches
this pattern from a different angle: a validator that produces
invalid blocks does not produce *useful* blocks, its bucket drains
regardless of credits, and eventually it is ejected under §3.5.

---

## 8 · Cross-component invariants

### 8.1 Subjective → objective bridge

**Invariant (Bridge).** Any state transition that persists to on-chain
state (eject, rejoin, overflow claim, credit issuance, credit
consumption) MUST be authorized by a `2/3`-stake vote observed within
the finalized-floor committee at some floor block. Subjective bucket
levels never directly cause on-chain change.

This preserves the property that **honest replay is deterministic**: a
new node that fetches finalized state does not need to reconstruct
subjective bucket histories to catch up.

### 8.2 Safety of ejection

**Invariant (Safety).** Ejection does not remove `V`'s stake from the
finality denominator inside `[F, F + Δ_grace]`. Because the finalized
floor is monotone (`docs/casper/theory/finalized-floor/finalized-floor-specification.md`)
and `V` remains bonded, ejection cannot invalidate previously-witnessed
finality.

**Invariant (Vote-snapshot stability).** For any Component 0 claim
`C` submitted in block `B_sub`, the stake denominator used to
evaluate the 2/3-threshold is the snapshot frozen at `B_sub`
(§3.10). Bonds and unbonds occurring in `(B_sub, B_sub + Δ_claim_deadline]`
do not shift the denominator; only slashings within that window
adjust it downward. Consequently no in-flight claim can be tipped
or suppressed by a mid-lifetime unbonding event.

### 8.3 Reward conservation

**Invariant (Conservation).** The total amount `V` receives per epoch
equals

```
total_paid(V, epoch) =
      Σ (successful ClaimOverflow amounts by V in epoch)               // Component 0 channel
    + eligible(V, epoch) · (V's pro-rata share of committedRewards)    // fee + mint channel, §3.9
    − (failed-claim fees paid by V, if any)
```

where `eligible(V, epoch) ∈ {0, 1}` is `1` iff `V` was in
`activeValidators ∖ ejectedValidators` at the epoch's distribution
block. Forfeited shares (`eligible = 0`) redistribute pro-rata among
the remaining eligible validators, preserving the epoch's total
emission.

The off-chain `o_V^U` is *bounded above* by the maximum amount `V`
could have earned even if every honest observer credited every block
`V` proposed at its most generous reward. This bound is enforced via
the yea-vote threshold: a claim for `B > earned` cannot cross 2/3
because at most a byzantine minority would vote yea.

**Corollary (No off-chain bank run).** Because overflow is monotone
between claims (§3.3), a validator cannot temporarily lend overflow
to another account and pull it back. Each `o_V^U` is a per-observer
tally of earned-but-unpaid rewards; it can only be extinguished by a
successful on-chain `ClaimOverflow` transferring an equal on-chain
amount into `V`'s address.

### 8.4 Fake-slowness resistance

**Invariant (No-fake-slow).** A validator `V` cannot reduce its own
subjective leak rate to prevent its own ejection because ejection is
adjudicated on other validators' `BucketView`s. `V`'s bucket is
observed by `U ≠ V`, and their `λ_U(t)` is driven by their own
`slowness_credit(t)`, not `V`'s.

### 8.5 Slashing preservation

**Invariant (Slashing-preserving).** For every offense that the current
codebase slashes, this specification either slashes (via Component 0
grace expiry, or via unchanged equivocation slashing) or absorbs
(Component 4 credit) — never silently ignores.

### 8.6 Turing preservation

**Invariant (Turing).** For every deploy that terminates under the
current pricing schedule, the same deploy still terminates under
Component 1 as long as the deployer's balance is sufficient to cover
`bill(D)` at effective pricing. There is no hard cutoff.

---

## 9 · Data structures and interfaces

### 9.1 New Rust types

```rust
// casper/src/rust/economics/bucket_view.rs
pub struct BucketView {
    entries: HashMap<Validator, BucketEntry>,
    lambda_base: f64,
    slowness_credit: f64,
    slowness_window: VecDeque<Duration>,   // for §6.2 median
    capacity_of: fn(&Validator) -> u64,    // = current bond
}

pub struct BucketEntry {
    level_b: u64,
    overflow_o: u64,
    last_update_ts: SystemTime,
}
```

`BucketView` has methods:

- `observe_block(&mut self, proposer: &Validator, reward: u64, now: SystemTime)`
- `decay_to(&mut self, now: SystemTime)`
- `is_drained(&self, v: &Validator) -> bool`  // `b_V^U < θ_eject · C(V)`
- `overflow_for(&self, v: &Validator) -> u64`

### 9.2 Reward computation

```rust
// casper/src/rust/economics/reward.rs
pub struct RewardInputs<'a> {
    pub block: &'a BlockMessage,
    pub parent_weights: &'a HashMap<BlockHash, i64>,    // from Estimator
    pub proposer_stake_share: f64,
    pub self_hash: &'a BlockHash,
}

pub fn compute_reward(inputs: &RewardInputs, params: &LeakyBucketParams) -> u64;
```

The function is a pure function with a small parameter object; unit
tests live under `casper/src/rust/economics/tests/reward_tests.rs`.

### 9.3 PoS state additions

`casper/src/main/resources/PoS.rhox` gains fields:

```
"ejectedValidators" : Map[Validator, (graceEnd, ejectFloorHash, ejectionSnapshot)],
"bebuggingCredits"  : Map[Validator, Nat],
"ejectionEvidence"  : Map[Validator, (EvidenceRecord, StakeSnapshot, submittedAt, ClaimId)],
"overflowClaims"    : Map[Validator, (ClaimStatus, StakeSnapshot, submittedAt, ClaimId, amount)],
"pendingRejoins"    : Map[Validator, (RejoinPayload, submittedAt)],
```

Claim maps are keyed by `Validator` to enforce one-open-per-target
serialization (§3.12); `ClaimId` is retained as an inner field for
cross-referencing votes in `LeakyBucketVotes` and for audit logs.
`StakeSnapshot` is `Map[Validator, i64]` frozen from
`authority_stakes()` at the submitting block's pre-state (§3.10).
`submittedAt` is the block number of `B_sub`, used to expire open
claims after `Δ_claim_deadline`. `ejectionSnapshot` (in
`ejectedValidators`) is the snapshot retained from the committing
`EjectV` claim; it survives the initial claim's deadline and is
the electorate for the corresponding `Rejoin`. `pendingRejoins`
carries no snapshot of its own — vote admission looks it up from
`ejectedValidators[V].ejectionSnapshot`.

and `proof_of_stake.rs` (`ProofOfStake` struct) gains parallel
`initial_bebugging_credits`, `initial_ejected_validators` (usually
empty).

### 9.4 QoS state

```rust
// casper/src/rust/qos/token_bucket.rs
pub struct QosState {
    per_client: HashMap<ClientPubKey, ClientBucket>,
    q_base: f64,
    p_slack: f64,
}

pub struct ClientBucket {
    tokens: f64,
    last_refill_ts: SystemTime,
}
```

Consumed by the deploy admission RPC in
`grpc_deploy_service.rs::do_deploy` (or its Rust equivalent).

---

## 10 · Consensus messages introduced

The messages below are all *deploys* — they piggyback on the existing
deploy machinery and require no new wire types beyond a discriminator
in the deploy body.

| Message | Author | Encoded in |
|---|---|---|
| `EjectV`             | Any active validator | Deploy targeting the PoS contract, `method = "ejectValidator"` |
| `EjectVVote`         | Any active validator | Justification-side attachment (see below) |
| `ClaimOverflow`      | Target validator     | Deploy, `method = "claimOverflow"` |
| `ClaimOverflowVote`  | Any active validator | Justification-side attachment |
| `Rejoin`             | Ejected validator    | Deploy, `method = "rejoinFromEjection"` |
| `RejoinVote`         | Any active validator | Justification-side attachment |
| `BebuggingCreditIssuance` | System (epoch-mint) | System deploy at epoch close |

**Vote attachment mechanism.** Votes are per-block, not per-deploy.
When validator `W` proposes block `B`, `B.body` is extended with a
new field `LeakyBucketVotes` containing a bounded list of
`(claim_id, yea|nay)` tuples for the open claims `W` has seen.
Placement inside `Body` (extending `RhoBody` in `RhoTypes.proto`)
is required so that the votes are part of `B`'s hash preimage —
replay must reconstruct identical votes to produce the same
canonical claim tallies. This is a wire break requiring a
schema-version bump; the migration plan (activation flag, backfill,
compatibility window) is out of scope for this document. It does
not add new gossip topics — blocks already gossip via
`BlockHashMessageProto` / `HasBlockProto` / `BlockRequestProto` and
the new body field rides that channel unchanged.

Bound: `|LeakyBucketVotes| ≤ N_vote_max` per block (default 32) to keep
block size predictable. Claims older than `Δ_claim_deadline` are pruned
from the open set.

---

## 11 · Validation and admission pipeline changes

### 11.1 Block validation

`casper/src/rust/validate.rs::validate_block_checkpoint` gains two new
sub-steps *after* replay:

1. **Vote consistency.** For every vote in `B.LeakyBucketVotes`, verify:
   - The referenced `claim_id` corresponds to an open claim at `B`'s
     pre-state (not yet past `Δ_claim_deadline`, not yet decided).
   - `B`'s proposer is a member of the claim's stake snapshot (§3.10),
     with a positive snapshot weight. This replaces the earlier
     draft's "active-per-current-finalized-floor" check: an active
     validator that was not in the snapshot cannot vote, and a
     snapshot member that has since unbonded can still vote.
   - The vote has not already been recorded by this proposer for
     this `claim_id` (no double-voting).
2. **Effect application.** For each open claim, sum the yea-stake
   weights from all recorded votes using the claim's snapshot
   weights (§3.10). If the sum crosses `2/3` of the snapshot's
   total (minus any slashed voters — see §3.10 "Slashing during a
   claim's lifetime"), apply the claim's effect (eject-V, mint
   overflow to V, reinstate V, etc.) to the post-state and mark the
   claim decided.

Step 2 is a *pure* function of the DAG projection, the per-claim
snapshot (frozen at `B_sub`), and PoS state; it is therefore
replayable and part of `Validate`'s determinism contract.

### 11.2 Block creation

`casper/src/rust/blocks/proposer/block_creator.rs::PreparedUserDeploys`
extends with a new field:

```rust
pub struct PreparedUserDeploys {
    // ... existing fields ...
    pub proposer_votes: Vec<LeakyBucketVote>,
}
```

The proposer polls its local `BucketView` and open-claims set to
produce this vector before block finalization.

### 11.3 Deploy admission

`DeployAdmissionPolicy` gains QoS enforcement at the RPC entry (§5.5).
Deploys that fail QoS are rejected with a soft error and not persisted.

### 11.4 Epoch-close system deploy

`system_deploy_util.rs` gains a new system deploy kind
`epoch-bebugging-credit:v1` that issues credits per §7.2. The domain
tag is disjoint from `epoch-mint:v1` to preserve seed disjointness (a
property tested by `epoch_mint_seed_differs_per_validator`).

The existing epoch-mint deploy is *not* changed by Component 0. On-chain
minting continues to pay `epoch_phlogiston` (from
`ProofOfStake::epoch_phlogiston`), reduced when Component 0 is
enabled by exactly the expected average overflow per validator per
epoch — so total emission is preserved and the two reward channels
(epoch mint and overflow claims) are complementary rather than
additive. Component 0's rewards flow through successful
`ClaimOverflow` deploys, which are the only path that credits
Component 0 earnings on-chain.

---

## 12 · Parameter table

| Name | Symbol | Type | v0.1 default | Where defined |
|---|---|---|---|---|
| Base leak rate | `λ_base` | Rate (1/block) | `0.01` | `CasperShardConf` (new field) |
| Ejection threshold | `θ_eject` | Fraction | `0.9` | `CasperShardConf` |
| Ejection observation window | `Δ_eject_observation` | Blocks | `50` | `CasperShardConf` |
| Grace period | `Δ_grace` | Blocks | `2 · epoch_length` | `ProofOfStake` |
| Unbonding quarantine (§3.11) | `quarantine_length` | Blocks | `2 · block_retention_window` (≈ 2 weeks, or `1.2 · 10^6` blocks at 1s / 1-week retention) | `ProofOfStake` |
| Block retention window (§3.11) | `block_retention_window` | Blocks | ≈ 1 week (`6 · 10^5` at 1s blocks) | protocol constant, floor-enforced |
| Max overflow per claim | `o_max_per_claim` | Bond units | `10 · minimum_bond` | `ProofOfStake` |
| Claim deadline | `Δ_claim_deadline` | Blocks | `epoch_length` | `ProofOfStake` |
| Base reward | `R_base` | Bond units/block | `epoch_phlogiston / epoch_length / num_active` | derived |
| Breadth exponent | `p_breadth` | Real | `2.0` | `CasperShardConf` |
| Recency reference | `y_ref` | Real | `1.0` | `CasperShardConf` |
| Per-step multiplier (per deploy) | `Φ_soft(n)` | ℕ → Real ≥ 1 | constant-then-cubic (see §4.2) | `CasperShardConf` |
| Ordinary-regime multiplier | `c_const` | Real | `1.0` | `CasperShardConf` |
| Soft-cap knee (fundamental-step count) | `k` | ℕ | `10^6` | `CasperShardConf` |
| Cubic ramp reference | `n_ref` | ℕ | `10^5` | `CasperShardConf` |
| Overflow coefficient | `α` | Real | `1.0` | `CasperShardConf` |
| QoS base refill | `q_base` | Tokens/s | `1.0` | node config |
| QoS slack exponent | `p_slack` | Real | `1.0` | node config |
| Slowness max | `s_max` | Fraction | `0.75` | `CasperShardConf` |
| Slowness window | `N_slow_window` | Blocks | `64` | `CasperShardConf` |
| Credit period | `Δ_credit_period` | Blocks | `epoch_length` | `ProofOfStake` |
| Credit issue per period (use-it-or-lose-it) | `κ_issue` | Nat | `1` | `ProofOfStake` |
| Vote list bound | `N_vote_max` | Nat | `32` | `CasperShardConf` |

**All parameters MUST be part of the shard's genesis config so replay
is deterministic across nodes.**

---

## 13 · Test and verification plan

### 13.1 Unit tests

- `bucket_view` decay and refill unit tests
- `compute_reward` monotonicity in `breadth` and `recency`
- `Φ_soft(n)` is monotone non-decreasing in `n`, equals `c_const` for
  `n ≤ k`, and is continuous at the knee `n = k`
- `QosState` refill under simulated slack values
- Bebugging-credit consumption idempotence

### 13.2 Property tests (`proptest`)

- **Reward-conservation.** Total `total_paid(V, epoch)` per §8.3 never
  exceeds the invariant upper bound under any sequence of blocks.
- **Ejection-safety.** For any DAG produced by an honest majority, no
  ejection commits without a 2/3-stake witness.
- **No-fake-slow.** A validator that spuriously reports high
  `slowness_credit` cannot avoid ejection when honest observers see
  low `slowness_credit`.

### 13.3 Formal artifacts

Follow the existing `slashing-verification.md` template:

- Add a TLA+ model at `formal/tlaplus/leaky-bucket/LeakyBucket.tla` for
  the subjective→objective bridge (§8.1). Verify with TLC.
- Add Rocq theorems at `formal/rocq/leaky_bucket/theories/` for reward
  monotonicity (§3.4) and slashing preservation (§8.5).

### 13.4 End-to-end tests

Under `casper/tests/integration/`:

- **Lazy-validator scenario.** 4-validator shard, one validator refuses
  to propose. Expected: within `Δ_eject_observation · Δ_grace` blocks,
  the lazy validator is ejected and (if it does not top up) slashed.
- **Transient-fault scenario.** 4-validator shard, one validator emits
  one invalid block due to injected fault. Expected: slashing detects,
  credit absorbs, no bond loss, credit balance drops.
- **Overflow-claim scenario.** Validator earns off-chain overflow over
  10 blocks and claims. Expected: 2/3 stake votes yea and on-chain
  balance credits.
- **Long-deploy scenario.** Client submits a deploy whose
  fundamental-step count `N_D = Σ c_base(op_j)` is well past the
  knee (e.g. `N_D ≥ k + 10 · n_ref`). Expected: deploy completes;
  `bill(D) = Σ_j Φ_soft(N_j) · c_base(op_j)` grows as the fourth
  power of `N_D − k`, and far exceeds the flat-regime bill
  `N_D · c_const`; other clients see slack drop and QoS refill rate
  slow.

### 13.5 Threat-model additions

Fold into `docs/casper/theory/slashing/slashing-threat-model.md` (or a
new sibling `leaky-bucket-threat-model.md`) the following attacks:

- **Ejection-flood.** Adversary submits many spurious eject-V deploys.
  Mitigation: `EjectV` deploy costs normal admission fees; failed
  votes waste no on-chain state because vote-attachment costs blocks.
- **Overflow-inflation.** Adversary claims impossibly large overflow.
  Mitigation: 2/3 vote requirement; failed-claim fee (§3.6).
- **QoS-starvation.** Adversary consumes QoS tokens via many small
  deploys. Mitigation: QoS is per-node; each node's local capacity is
  bounded; propagation still succeeds because peer-received deploys
  bypass QoS (§5.5).

---

## 14 · Non-goals and deferred work

**Out of scope for v0.1:**

- **Client-side proof-of-work.** QoS is not proof-of-work; it does not
  authenticate the client beyond signature verification.
- **Bond redistribution.** Slashed bonds continue to flow to the coop
  vault per current slashing spec. This proposal does not touch that.
- **Cross-shard leak.** All bucket state is intra-shard. Multi-shard
  interactions are out of scope; a follower shard cannot see another
  shard's leak.
- **Formal verification.** This document does not itself contain
  proofs. Proofs are planned artifacts (§13.3) that must precede a
  mainnet-blocking upgrade.
- **Wire schema migration plan.** Component 0's vote-attachment
  (§10) is a wire break. The migration plan (which release, which
  activation flag, backfill and compatibility window) belongs in a
  decision-record document, not here.

**v-next TODOs** — deferred design work, not required for v0.1
but flagged so future maintainers know the residual gaps:

- **Cost-accounting-impl D3 compatibility review.** Component 1's
  §4.2 formula `Φ_soft(N_j) · c_base(op_j)` re-uses `c_base` as a
  measure of work done, bringing the per-op cost table back into
  consensus **only for the overflow premium** — the term
  `Σ_j (Φ_soft(N_j) − 1) · c_base(op_j)`, zero when `Φ_soft ≡ 1`.
  The design itself is settled (§4.6). What remains is
  coordination with `cost-accounting-impl` maintainers to confirm
  the hybrid is compatible with the D3 invariants (per-signature
  demand computation, replay determinism) before Component 1
  ships — a review task, not a spec-level decision.

- **Rejoin-obstruction mechanism.** §3.14 excludes `Rejoin` votes
  because they depend on the subjective `b_V^W` at ratification.
  This leaves a residual coordination-attack surface: a bloc can
  nay-vote a legitimate rejoin to keep an ejected validator out
  (pushing it toward grace-expiry slashing, from which the
  obstructors gain via bond redistribution). Mandatory
  participation (§3.13) makes the obstructors visible, and §3.4
  penalises thin-testimony strategies, but nothing objectively
  catches rejoin-nay dishonesty today. Options if experience
  shows this to be a practical problem: (a) auto-approve rejoin
  when `topup ≥ C(V)` — under that condition, `b_V^W + topup ≥ C(V)`
  holds unconditionally, so a nay-vote is provably dishonest and
  can be folded into §3.14; (b) small objection fee for `Rejoin`
  nays only (narrower version of the earlier §3.13 fee proposal,
  scoped to the actual gap); (c) positive reward for yea-voters on
  successful rejoin, funded from the topup.

- **Dishonest-nay coverage for `EjectV`** (Attack D in §6.4).
  §3.14 catches dishonest *yea* votes on `EjectV` (attempts to
  unfairly eject a producer) but not dishonest *nay* votes
  (attempts to unfairly protect a lazy peer), because parent-set
  testimony gives a lower bound on `b_W^V` but no upper bound —
  slowness credit provides plausible deniability for high `b_W^V`.
  The residual risk is bounded by the >1/3 Byzantine-fraction
  assumption: preventing legitimate ejection requires a bloc
  larger than 1/3 of the ejection snapshot's stake. Options if
  experience shows exploitation: (a) require nay-voters on
  `EjectV` to publish a *witnessing* signature — a signed block
  within some window that includes a fresh W-authored block in
  its ancestor closure sufficient to plausibly explain
  `b_W^V ≥ θ_eject · C(W)` under any legal λ; absence of such a
  witness makes the nay slashable. (b) make `slowness_credit`
  observable on-chain so an outlier reporter can be flagged.
  (c) accept the residual and rely on the >1/3 assumption.

---

**Change log.**

- **v0.1 (2026-09-16, `leakybucket` branch)** — initial draft based on
  design sketch from the branch owner. All parameters are placeholders
  pending simulation.
