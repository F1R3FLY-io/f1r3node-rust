# The Gated Interaction-Cut (a Cost-Accounted COMM)

The five gated COMM rules (`CAReduction.v`) in interaction-cut form: a redex fires
only when a co-present token authorizes it (the no-leak invariant). GAP-2 is
dissolved — the continuation keeps its OWN seal, with no SAnd re-seal.

```
   Rule 1 (atomic s, single token) — the canonical interaction-cut:

        { for(y ← x){T}  |  x!(U) }_s      ∥      s : S
        └──────── signed redex ───────┘         └─ token stack ─┘
                      │                                │
                      │  ╴╴╴╴╴╴ the gate s consumes ╶╶╶┘
                      ▼   one authorizing token (no-leak: needs it)
              T{@U / y}                          ∥      S
        └─ continuation keeps ─┘                    └ tail remains ┘
           its OWN seal (GAP-2: no SAnd re-seal)

   grade of the step = s   (CAGradedTransition.graded_step — the consumed sig)

   The 5 rules by (signature, processes, token) shape:
   ┌────────┬───────────┬──────────────┬───────────────┬──────────────────┐
   │        │ signature │ receiver/send│ token(s)      │ translation fires │
   ├────────┼───────────┼──────────────┼───────────────┼──────────────────┤
   │ Rule 1 │ atomic s  │ whole redex  │ s : t         │ 2 COMMs           │
   │ Rule 2 │ s1 ∧ s2   │ whole redex  │ s1:t1, s2:t2  │ 3 COMMs (nested)  │
   │ Rule 3 │ s1 ∧ s2   │ whole redex  │ (s1∧s2) : t   │ 4 COMMs (Split)   │
   │ Rule 4 │ s1, s2    │ split        │ (s1∧s2) : t   │ 4 COMMs (Split)   │
   │ Rule 5 │ s1, s2    │ split        │ s1:t1, s2:t2  │ 3 COMMs           │
   └────────┴───────────┴──────────────┴───────────────┴──────────────────┘
        subject reduction + no-leak: WrappingSubjectReduction.v
        each rule's pure-rho simulation:  rule1..5_reachable (CATranslationFaithfulness.v)
```

Interaction-cut = the receiver `for(y←x){T}` and sender `x!(U)` annihilate on the
shared channel `x`, contracting to `T{@U/y}` (Milner's pseudo-application: the
quoted payload `@U` substitutes the bound name), gated by exactly one token.
