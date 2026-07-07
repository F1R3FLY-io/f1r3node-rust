# Case study #9 — Scala rejects self-correcting blocks (Scala bug, Rust-fixed)

## 1 · Summary

This is the **Scala-side** bug fixed by the Rust port — the only
bug fix in the manifest where Rust is *more permissive* than Scala.
The Scala counterpart at
`MultiParentCasperImpl.scala:618-621` rejected any block whose
proposer had earlier produced an equivocation, even if the block
acknowledged the equivocation via `has_slash_system_deploys`. This
incorrectly punished *self-correcting* proposers — validators who
slashed their own past equivocations to remain in good standing.
The Rust port at `validate.rs:1018-1029` widens the check to accept
self-correcting blocks.

## 2 · Discovery technique

**Primary**: Sage `differential_bisimilarity_model.sage` produced
a witness where Rust accepted a block that Scala rejected; the
divergence was traced to the self-correcting block path.

**Corroborating**: Hypothesis `multi_epoch_state_machine.py` with
explicit `slash_self` actions corroborated that the Rust path
correctly handles self-correction.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_9
```

The fixture
[`casper/tests/slashing/pre_fix_bug_9.rs`](../../../../../casper/tests/slashing/pre_fix_bug_9.rs)
encodes the self-correcting scenario; Scala (model surrogate)
rejects; Rust (post-fix) accepts.

## 4 · Classification trace

```
threat_class       = permitted_bug_fix
                     (Scala defect fixed by Rust — direction
                      reversed from most bug fixes)
ledger_status      = confirmed_fixed_bug
                     (the Scala bug is fixed in Rust; the divergence
                      is permitted and intentional)
action             = Keep pre_fix_bug_9.rs as the surrogate-Scala
                     baseline; post-fix uc_23_self_correcting.rs
                     is the Rust anchor
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                         |
|------------------|--------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.9 (`BugFixSelfCorrecting.v` — implicit in `t_9_9_self_correcting`)                           |
| Sage witness     | `differential_bisimilarity_model.sage` finding                                                   |
| Rust regression  | `pre_fix_bug_9.rs`, `prop_t_9_9_self_correcting.rs`, `uc_23_self_correcting.rs`                  |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.10`](../../design/09-bug-fixes-and-rationale.md) |

**Stack depth: 4** (Rocq + Sage + Rust + design).

## 6 · Lessons for the methodology

1. **Differential testing surfaces both-direction defects**. Most
   bug fixes are Rust-corrects-Scala; Bug #9 is Scala-incorrect
   and Rust-permissive. The differential model finds both
   directions because it does not privilege either implementation
   as the oracle.
2. **The Rocq oracle settles the direction**. When Rust and Scala
   disagree, the Rocq oracle is the tiebreaker; it proves which
   behavior is consistent with the protocol specification.
3. **"Permitted bug fix" is symmetric**. The same classification
   applies whether Rust fixed a Scala bug or Scala-style behavior
   is being preserved; what matters is that the divergence is
   *documented*, *intentional*, and *backed by a theorem*.
