# The Cost Endofunctor 𝔠 and its Monad Structure

`continued-gslt-cost-v2`'s central thesis, as realized in Rocq (`CostMonad.v`,
CL3+CL4): cost accounting is an endofunctor 𝔠 — indeed a monad — whose laws
descend from the two constituent monoids (`SignatureMonoid.v`, CL2).

```
                        the Cost endofunctor  𝔠 = (· × grade)
                        grade = (sig, token)   [ authority × temporal stack ]

        X ───────────────────────────────────────────────▶ 𝔠 X = X × grade
        │  cost_map f                                          │
   f ↓  │  (functor: cost_map_id, cost_map_compose)            │  cost_map f
        ▼                                                      ▼
        Y ───────────────────────────────────────────────▶ 𝔠 Y = Y × grade


   unit  η = cost_eta            multiplication  μ = cost_mu
   ┌─────────────────┐          ┌──────────────────────────────────────┐
   │  X ──η──▶ 𝔠 X    │          │   𝔠²X = X × grade × grade              │
   │  x ↦ (x, ())     │          │   ──μ──▶ 𝔠 X = X × (grade ∘ grade)     │
   │  (unmetered      │          │   (combine the two grades:            │
   │   embedding)     │          │    SAnd on sigs, ++ on token stacks)  │
   └─────────────────┘          └──────────────────────────────────────┘

   monad laws  (all up to cost_equiv, pointwise ⇒ NO funext)
   ┌────────────────────────────────────────────────────────────────────┐
   │  μ ∘ η_{𝔠X}   = id      cost_left_unit    ◀── grade_op_unit_r        │
   │  μ ∘ 𝔠(η)     = id      cost_right_unit   ◀── grade_op_unit_l        │
   │  μ ∘ μ_𝔠      = μ ∘ 𝔠(μ) cost_assoc        ◀── grade_op_assoc         │
   └────────────────────────────────────────────────────────────────────┘
                                       ╎
                  the two monoids the laws descend from (CL2)
        ┌──────────────────────────────┐  ┌─────────────────────────────┐
        │ signature commutative monoid │  │ token-stack FREE monoid     │
        │ (sig, SAnd, SUnit)  up to ≡sig│  │ (token, ++, TUnit)  Leibniz │
        │   — the authority, ∧-fused    │  │   — the temporal modulus,   │
        │   sig_monoid_comm/assoc/unit  │  │   NEVER commutative          │
        └──────────────────────────────┘  └─────────────────────────────┘

   NON-IDEMPOTENT:  cost_mu_modulus_accumulates (token_size adds under μ),
                    cost_monad_not_idempotent — metering twice ≠ metering once.
```

The `μ`-flatten of nested wrappers `𝔠²X ⇒ 𝔠X` is exactly the move the old
bare-proc `SSigned : proc → sig → system` could **not** even type (it carries a
bare proc, so `SSigned (SSigned …) …` is ill-formed). The native four-sort grammar
(`CASyntax.v`, DR-21) re-types continuations as signed terms, making `μ`
expressible — here as plain grade multiplication.
