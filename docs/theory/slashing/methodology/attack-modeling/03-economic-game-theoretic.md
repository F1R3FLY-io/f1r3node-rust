# 03 · Economic and game-theoretic threats

> *“There is no such thing as a free lunch.”* — Aphorism popularized
> by Milton Friedman, 1975 [Fri75].

This chapter explains the methodology's treatment of **economic
threats**: adversaries who follow the protocol as specified but
exploit its incentives. The slashing subsystem's primary defenses
are economic — the bond at risk is the deterrent — and economic
threats are therefore *first-class* threats.

This chapter is the pedagogical companion to
[`../../slashing-threat-model.md §5.A`](../../slashing-threat-model.md);
that document is the catalog, this one is the *how to think about
it*.

Organization:

- [§1 — Why economic threats are a separate layer](#1--why-economic-threats-are-a-separate-layer)
- [§2 — The rational adversary](#2--the-rational-adversary)
- [§3 — Bribery and coalition formation](#3--bribery-and-coalition-formation)
- [§4 — Long-range attacks](#4--long-range-attacks)
- [§5 — Censorship as attack](#5--censorship-as-attack)
- [§6 — Withholding as attack](#6--withholding-as-attack)
- [§7 — Nothing at stake](#7--nothing-at-stake)
- [§8 — What the methodology can and cannot prove](#8--what-the-methodology-can-and-cannot-prove)
- [§9 — Related work](#9--related-work)

---

## 1 · Why economic threats are a separate layer

A correctness violation is a behavior the system **cannot** allow;
an economic threat is a behavior the system **can** allow under
specification but that the adversary is *incentivized* to perform.
The two are distinct:

| Aspect             | Correctness threat                         | Economic threat                                                           |
|--------------------|---------------------------------------------|---------------------------------------------------------------------------|
| Violates protocol? | Yes                                          | No — operates entirely within the specified behavior                       |
| Defense            | Code fix, theorem strengthening              | Parameter tuning (bond size), incentive redesign, social/operational measures |
| Verified by        | Rocq / TLA⁺ / Kani / proptest                | Game-theoretic argument; sometimes mechanism-design analysis              |
| Falsifiable by     | A counterexample trace                       | A profitable deviation in a model                                          |
| Catastrophic if    | A bug allows a slashing-rule violation       | The bond size is too small relative to the gain from misbehavior         |

The slashing methodology defends against correctness threats
*mechanically*. It defends against economic threats by **bounding
the maximum gain** an adversary can extract within the specified
behavior; if the gain bound is below the bond at risk, the rational
adversary's expected utility from misbehavior is negative.

### 1.1 The methodology's limits

The methodology can prove:

- For a given bond `B`, the maximum profit an adversary can extract
  from a single equivocation is `≤ f(B)`.
- The bond `B` is forfeited on every detected equivocation.

The methodology **cannot** prove:

- That `B` is large enough to deter every conceivable adversary
  (because adversary preferences are subjective).
- That an irrational adversary will not act anyway (e.g. a
  reputation-burning suicide attack [Vit19]).

These limits are documented honestly. The slashing development
defers bond-sizing to governance and bounds the *technical* attack
surface independently.

---

## 2 · The rational adversary

The *rational adversary* assumption is the workhorse of cryptoeconomic
analysis [BMM15]. A rational adversary:

- maximizes expected utility,
- prices the value of their bond against the gain from misbehavior,
- assumes other validators are rational by symmetry,
- discounts future rewards by a discount factor `δ ∈ (0, 1]`.

### 2.1 The methodology's rationality model

The slashing methodology models the rational adversary as follows:

```
algorithm rational_adversary_utility(σ : Strategy, params : Params) → Utility:
    let gain     ← expected_immediate_gain(σ, params)
    let bond_at_risk ← bond_of(σ.actor)
    let detection_p  ← probability_of_detection(σ, params)
    let slash_share  ← slash_share_of_bond(σ.actor, params)
    return gain − detection_p · bond_at_risk · slash_share
```

The rational adversary plays strategy `σ` only if
`rational_adversary_utility(σ) > 0`. The methodology's bound `f(B)`
is the smallest bond `B` for which `utility < 0` for every modeled
strategy `σ`.

### 2.2 The known bounds

The slashing development establishes (informally; the formal
mechanization is partial):

| Strategy class                                | Maximum gain bound                                                   |
|-----------------------------------------------|----------------------------------------------------------------------|
| Single equivocation                           | `≤ 1 × block_reward + 1 × MEV_window`                                  |
| Chain neglect amplification (chain of `k`)    | `≤ k × neglect_reward` — bounded by depth `≤ n − 1` (T-11)             |
| Withholding evidence within `k` rounds        | `≤ k × proposer_reward` — bounded by detection deadline                |
| Long-range attack                             | Unbounded in pure protocol; bounded by social-consensus checkpoint     |

The bond `B` must exceed the maximum gain across all rows for the
rational adversary's utility to be negative on every strategy.

---

## 3 · Bribery and coalition formation

A bribery attack [Vit19] is when the adversary pays a third-party
validator to misbehave on the adversary's behalf. The bribed
validator's bond is at risk, not the adversary's.

### 3.1 The model

```
algorithm bribery_attack_utility(σ : Strategy, params : Params) → ⟨A_util, B_util⟩:
    let bribe       ← params.bribe_amount
    let A_gain      ← attacker_gain(σ, params)
    let B_bond      ← bond_of(σ.bribed_validator)
    let detection_p ← probability_of_detection(σ, params)
    let A_util ← A_gain − bribe
    let B_util ← bribe − detection_p · B_bond
    return (A_util, B_util)
```

The bribery is feasible iff both `A_util > 0` and `B_util > 0`. The
methodology bounds the bribery feasibility by ensuring
`detection_p · B_bond > B_util_max(σ)` for every modeled `σ`. This
is the standard *anti-bribery bond-sufficient* condition.

### 3.2 Defense

The methodology's primary defense is **detection completeness**
(T-2: every real equivocator is eventually recorded). If detection
is complete and `B_bond > B_util_max(σ)`, the bribed validator has
negative expected utility and refuses the bribe.

Detection completeness is therefore not only a correctness property —
it is an *economic* defense. This is one of the methodology's
recurring patterns: correctness invariants double as economic
guarantees.

---

## 4 · Long-range attacks

A long-range attack [BLM18] exploits the fact that, once a validator
has unbonded and withdrawn, their bond is no longer at risk. If
they keep their key, they can later sign a fork from before their
withdrawal — at zero economic cost.

### 4.1 The model

The attack is feasible iff:

1. The adversary controls keys of validators whose total stake at
   some historical block exceeded `2n/3`.
2. Those validators have all unbonded and withdrawn.
3. No social-consensus checkpoint locks the chain past that block.

### 4.2 Defense

The slashing methodology *does not* defend against long-range attacks
at the protocol level. The defense is **social** (governance-enforced
checkpoints, weak subjectivity [Vit15]). This is documented honestly
in
[`../../slashing-threat-model.md §5.A.3`](../../slashing-threat-model.md)
as **explicitly out of scope** for the slashing subsystem.

The methodology's contribution to the long-range defense is
**unbonding lockup**: a validator that unbonds remains slashable
for a configurable lockup period, during which evidence of past
equivocation can still trigger a slash. This narrows but does not
close the long-range attack window.

---

## 5 · Censorship as attack

A censoring proposer refuses to include slash deploys in its
blocks, preventing slashing of a colluding validator. This is a
*omission* attack — the proposer does nothing wrong by individual
action but, in aggregate, prevents the system from operating.

### 5.1 The model

The attack is feasible iff:

1. The adversary controls `> 2n/3` of proposer slots (rare; requires
   majority stake), or
2. The adversary controls some smaller fraction *and* the slash
   deadline is short enough that minority censorship can succeed.

### 5.2 Defense

The methodology relies on **proposer rotation** and the **two-level
slashing** mechanism: any proposer that omits a slashable record in
its justifications is *itself* slashable for neglect (T-6, T-11).
The chain of censoring proposers therefore self-amplifies — each
new censoring proposer joins the closure and is slashed.

The bound on censorship depth is the BFT bound: if fewer than `n/3`
proposers are censoring, the slash will eventually go through. This
is Theorem T-12 in the verification doc.

---

## 6 · Withholding as attack

Withholding is a generalized form of censorship where the proposer
also withholds the evidence itself, not just the slash deploy. If
no honest validator has seen the offending block, no slash can be
proposed.

### 6.1 The model

The attack is feasible iff:

1. The offender publishes the equivocating block to *only* a subset
   of validators, and
2. The recipients collude to not gossip it onward, and
3. The receiver count is below the detection threshold.

### 6.2 Defense

The methodology defends through **gossip protocol assumptions**:
under the synchrony hypothesis [DLS88], every honest validator
eventually sees every published block. Withholding is therefore
*temporary* — the attack succeeds only until the gossip protocol
completes.

The slashing methodology's bound on withholding is the
**accountability-gap** model
(`evidence_visibility_model.sage`). The model computes the worst-case
gap between partial-visibility closure and full-visibility closure;
the gap is bounded by the gossip delay.

---

## 7 · Nothing at stake

The classical "nothing at stake" [BG19] objection to early
proof-of-stake designs is that, on a fork, a validator can sign
*both* branches at zero cost because no bond is forfeited unless
the validator is caught equivocating.

### 7.1 The defense

The slashing methodology's defense is exactly the slashing protocol
itself: signing two distinct blocks at the same sequence number is
the canonical equivocation; the validator is slashed. The bond
forfeit is the cost; the nothing-at-stake objection fails because
the stake *is* at stake.

This makes the slashing subsystem the **load-bearing** defense
against nothing-at-stake; a bug in slashing is simultaneously a
correctness bug *and* an economic vulnerability. This is the
underlying reason the methodology invests so heavily in slashing
verification.

---

## 8 · What the methodology can and cannot prove

### 8.1 Can prove (with citations)

| Claim                                                                                                              | Authority                            |
|--------------------------------------------------------------------------------------------------------------------|--------------------------------------|
| Every equivocator with stake at risk is eventually slashed                                                          | T-2 + T-6 (Rocq) + TLA⁺ liveness     |
| The slash forfeits the entire bond                                                                                  | T-7 (Rocq) + Kani harnesses          |
| The closure of neglecters is bounded by `n − 1`                                                                     | T-11 (Rocq) + Sage finding #11       |
| BFT quorum is preserved under `f < n/3`                                                                              | T-12 (Rocq) + TLA⁺ `Inv_BFTBound`    |
| Authorization rejects every form documented in §4 of the threat model                                                | T-9.8 / T-Auth (Rocq) + Kani         |

### 8.2 Cannot prove

| Claim                                                                                                              | Why                                                                       |
|--------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------|
| The bond size is large enough for any specific adversary                                                            | Depends on adversary preferences; subjective                              |
| No off-chain bribery succeeds                                                                                      | Requires modeling off-chain communication channels                         |
| Long-range attacks are economically infeasible                                                                      | Long-range defense is social, not protocol                                |
| MEV (maximum extractable value) is bounded                                                                          | MEV models depend on application-layer details                            |
| Validators will not collude socially despite the slashing risk                                                       | Out of model                                                              |

The methodology's contribution is to make this distinction
**explicit** — every claim is either inside the formal scope (with
a Rocq / TLA⁺ / Kani / Sage citation) or outside it (with an
explicit out-of-scope marker).

---

## 9 · Related work

- **Cryptoeconomic security**: Buterin [Vit15], Bonneau *et al.*
  [BMM15].
- **Long-range attacks**: Bano *et al.* [BLM18].
- **Bribery attacks**: McCorry *et al.* [McC19].
- **Mechanism design for blockchain**: Roughgarden [Rou21].
- **Game-theoretic analysis of Casper**: Buterin & Griffith [BG19].
- **Synchrony assumptions in distributed consensus**: Dwork *et al.*
  [DLS88].
- **Weak subjectivity**: Buterin [Vit15], Pass & Shi [PS17].

DOIs in [`../references.md`](../references.md).

---

## 10 · Next chapter

[`../pipeline/01-witness-to-source-rule.md`](../pipeline/01-witness-to-source-rule.md)
— the **pipeline** layer of the methodology. Once a witness exists
(from any of the previous chapters' tools), the pipeline rules
govern what happens to it next.
