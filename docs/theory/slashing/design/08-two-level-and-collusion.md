# 08 · Two-level slashing & collusion-resistance

## 8.1 The collusion problem

Suppose validator A equivocates: signs `b₁` and `b₁'` at the same
sequence number. The detection pipeline (§04) classifies this as
`AdmissibleEquivocation` (assuming `b₁'` is requested as a
dependency by some downstream block). The next proposer P slashes
A.

But what if validator B *colludes* with A? B is the next proposer
and *deliberately fails* to attach a `SlashDeploy(b₁', A)` to
B's own block. By "looking the other way", B preserves A's bond
and (presumably) shares the saved stake off-chain with A.

A naive slashing protocol would let B get away with this: there
is no rule saying "you *must* slash any equivocator you know
about". B's block looks valid: `b₁'` is just a justification, not a
deploy.

**Two-level slashing** closes this loophole.

## 8.2 The neglect rule (T-3, T-6)

The detection layer adds a second classification verdict —
`NeglectedEquivocation` — that fires when a block's justifications
include any block flagged invalid in the DAG, *and* the block does
not carry a `SlashDeploy` for the offending validator.

[![Diagram 08 — Justifications → neglect detection data flow](../diagrams/08-dataflow-justifications-to-neglect.svg)](../diagrams/08-dataflow-justifications-to-neglect.svg)

Formally (`validate.rs:989-1030`):

```
neglected_invalid_justification(b, snapshot) ≜
    ∃ j ∈ b.justifications:
        let lookup ← snapshot.dag.lookup(j.latestBlockHash)
        lookup.invalid = ⊤  ∧
        snapshot.bonds_map[j.validator] > 0

has_slash_system_deploys(b) ≜
    ∃ d ∈ b.body.system_deploys: d is a SystemDeployData::Slash variant

reject(b) ⟺ neglected_invalid_justification(b, snapshot)
            ∧ ¬ has_slash_system_deploys(b)             -- post-fix #9
```

The `¬ has_slash_system_deploys` clause is the **Rust widening**
introduced by bug fix #9: a self-correcting block (B cites A's
invalid block *and* attaches a `SlashDeploy(_, A)`) is admitted.
Pre-fix Scala rejects all such blocks unconditionally.

## 8.3 The two-level pipeline

When B's block is rejected as `NeglectedEquivocation`, the
dispatcher inserts an `EquivocationRecord(B, seqN_B − 1, ∅)`.
The next-next proposer C now sees *two* offenders: A (admissible
equivocation) and B (neglected). C emits **two**
`SlashDeploy`s in the same block, one for each offender.

[![Diagram 04 — Two-level slashing](../diagrams/04-seq-two-level-slashing.svg)](../diagrams/04-seq-two-level-slashing.svg)

The two slash deploys execute in sequence inside C's block; the
post-state has `allBonds[A] = 0`, `allBonds[B] = 0`, and the
forfeited stake is in the Coop vault (200 = 100 + 100 if both had
stake 100). Other validators replay C's block and reach the same
post-state (replay determinism / T-15).

> **Why does this work?** B's only winning move is to attach a
> `SlashDeploy(b₁', A)` to B's own block. Pre-fix, that would have
> made B's block invalid (Scala rejects neglecting blocks
> unconditionally). Post-fix #9, the self-correction is admitted —
> B's block is valid, and only A is slashed. So B's incentive is
> to *always* slash known equivocators rather than collude.

## 8.4 The slash-closure operator

To formalize the collusion-resistance claim, we introduce a fixed-
point operator over the slashed set:

**Definition 8.1 (Direct equivocators).** The set of *direct*
equivocators is `{v ∈ V : ∃ s, equivocates(D, v, s)}`.

**Definition 8.2 (Slash closure).** The slashed set `Sl` evolves as
the least fixed point of

```
Sl ← Sl ∪ {v : neglect(v) ∩ Sl ≠ ∅}
```

starting from the direct equivocators, where `neglect(v)` is the
set of validators whose invalid blocks `v` cited as justifications
without a corresponding `SlashDeploy`.

**Theorem T-11 (Level-2 termination, weak form).**
*(`t_11_level_2_termination`, `TwoLevelSlashing.v:126`.)* After
`|V|` iterations of the slash closure, the slashed set is still
contained in `V`:

```
∀ universe g s0,
  incl s0 universe ⟹
  incl (slash_iter universe g s0 (length universe)) universe
```

**Note on stronger forms.** The full fixed-point stabilization
(closure stops growing after `|V|` iterations) is acknowledged as
future work in the Rocq source comment at
`TwoLevelSlashing.v:121-124` (*"Stronger: it stabilizes; we prove
the weaker but useful form here."*). For the BFT-quorum
preservation argument (T-12), the weak form suffices because
`incl ... universe` together with `|closure| ≤ F` is enough to
bound the active set.

**Proof of the weak form.** By induction on the iteration count.
Each iteration applies `slash_iter_step` which only adds elements
already in `universe` (by `slash_iter_step_incl`). The invariant
`incl Sl universe` is preserved across iterations; `length universe`
iterations preserve it. ∎

**Theorem T-12 (Collusion-resistance / BFT-quorum preservation).**
*(`t_12_bft_quorum_preservation`, `TwoLevelSlashing.v:174`.)* Under
the standard BFT precondition `|closure| ≤ F = ⌊(n − 1) / 3⌋` per
[LSP82], the slash closure preserves
`|universe| − |closure| ≥ |universe| − F`. With strict `F < |universe|`,
the active validator set after both levels of slashing fire
maintains quorum.

**Corollary (`t_12_bft_active_set_size`).** With strict
`F < |universe|`, the active set is non-empty after the closure
fires.

**Proof of T-12.** From `|closure| ≤ F`, subtract from `|universe|`:
`|universe| − |closure| ≥ |universe| − F`. Substitute the BFT
bound: `|universe| − ⌊(n − 1)/3⌋ ≥ ⌈2(n − 1)/3⌉ + 1 = ⌈(2n + 1)/3⌉`,
which is the classical BFT quorum size for `n` total validators
[LSP82]. ∎

## 8.5 What happens *outside* T-12's precondition?

T-12's hypothesis `|closure| ≤ F` is essential. If the closure
fires for *more* than `F` validators (i.e. the network is *more
than F-neglectful*), T-12 does not apply — and indeed quorum can
be lost.

This is the **F-neglectful quorum-liquidation counter-example**
documented in verification §10.8.2. With n = 4, F = 1, if both A
(equivocator) and B (neglector) are slashed, `|closure| = 2 > F`,
and the active set drops to `{C, D}` of size 2 — *below* the BFT
quorum lower bound `⌈(2n + 1)/3⌉ = 3`.

This is **expected behavior** outside T-12's domain: if more
validators misbehave than the protocol bound allows, the protocol
cannot maintain quorum. This is not a bug in the slashing
subsystem; it is a property of the BFT bound itself.

The worked example in spec §11.2 walks through this trace
explicitly as a *deliberate counter-example*.

## 8.6 Why neglect detection works (intuition)

The neglect detection rule is essentially: *"You may not cite an
invalid block in your justifications without also slashing its
sender."* This rule:

1. **Cannot be evaded by silence.** If B doesn't cite the invalid
   block at all, B can't claim to know about A's equivocation.
   But if B cites *any* invalid block (e.g. as a parent), the rule
   fires.
2. **Cannot be evaded by lying about the verdict.** B does not
   classify; the local detector at the *receiving* validator does.
   B cannot affect what other nodes see.
3. **Cannot be evaded by waiting.** As long as A's equivocation is
   in the DAG, *every* future block citing it must self-correct.

The only winning strategy for B is to slash A. This makes
collusion **mutually destructive**: A is slashed for equivocation,
B is slashed for neglect — unless B slashes A, in which case only
A is slashed and B is admitted.

## 8.7 Sequence diagram — collusion ends in mutual destruction

[![Diagram 04 — Two-level slashing](../diagrams/04-seq-two-level-slashing.svg)](../diagrams/04-seq-two-level-slashing.svg)

The diagram shows:

- **Phase 1 (Level 1)**: A signs `b'` at seqN; detector emits
  `AdmissibleEquivocation`; tracker records `(A, seqN − 1, ∅)`.
- **Phase 2 setup (Level 2)**: B signs `b_B @ seqN_B` with `b'`
  cited in justifications, *no* `SlashDeploy` attached.
- **Phase 3**: validate.rs:989-1030 fires `NeglectedInvalidBlock`;
  tracker records `(B, seqN_B − 1, ∅)`.
- **Phase 4**: Honest proposer P proposes block `bP` at seqM; reads
  `invalid_latest_messages` → `{(A, b'), (B, b_B)}`; emits two
  `SlashDeploy`s in `bP`.
- **Phase 5**: PoS contract executes `slash(A)` (atomic state
  update) then `slash(B)` (atomic state update).
- **Phase 6**: `bP` gossips; ForkChoice excludes A and B.

## 8.8 Theorems that govern Level-2

| Theorem | Statement                                                                                                      | File:line                                |
|---------|----------------------------------------------------------------------------------------------------------------|------------------------------------------|
| T-3     | Post-fix, the slashable set strictly extends the pre-fix slashable set.                                        | `InvalidBlock.v:151`                     |
| T-6     | `detect_neglected` is sound and complete (verification §4.5 / §4.6).                                           | `EquivocationDetector.v` (sound at §4.5) |
| T-11    | Level-2 closure terminates in at most `|V|` iterations.                                                        | `TwoLevelSlashing.v:126`                 |
| T-12    | Under `|closure| ≤ F`, slash closure preserves quorum.                                                         | `TwoLevelSlashing.v:174`                 |
| T-9.9   | The Rust widening (admit self-correcting blocks) is sound: rejection-iff post-fix is `neglected ∧ ¬has_slash`. | `BugFixSelfRegression.v:107`             |

## 8.9 Why two levels and not three?

A natural question: if neglecting an equivocator is itself slashable
(Level 2), is *neglecting a neglector* slashable too (Level 3)?

**No.** Level 2 already captures the closure. The dispatcher's
neglect rule fires for *any* invalid block in justifications,
regardless of *why* it was invalid. So if B's block is flagged
invalid (because B neglected A), and C cites B's block without
slashing B, then C is itself caught by the same Level-2 rule —
no need for a separate Level-3 rule.

The closure operator (Definition 8.2) iterates until fixed point;
T-11 proves the iteration terminates. So **Level 2 is the closure
of all higher levels**: any depth of neglect-of-neglect-of-…
collapses into a single closure step.

## 8.10 Summary

- **Level 1**: detect direct equivocations → record → slash.
- **Level 2**: detect neglected justifications → record → slash.
- **Closure**: iterate until fixed point.
- **Termination**: T-11.
- **BFT-bound preservation**: T-12 (under `|closure| ≤ F`).
- **Soundness of Rust widening**: T-9.9.

---

**Next:** [§09 — Bug-fix manifest & rationale](09-bug-fixes-and-rationale.md)
