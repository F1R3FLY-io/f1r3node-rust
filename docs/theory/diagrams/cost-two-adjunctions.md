# The Two Adjunctions of the Cost Construction

`continued-gslt-cost-v2`'s adjunctions, as realized in Rocq (`CAAdjunctions.v`,
CL5) and the faithfulness stack (`CATranslationFaithfulness.v` / `CABisimulation.v`).

```
   ADJUNCTION I  —  Free ⊣ Forget   (the detachable apparatus)         CL5 ✓
   ┌──────────────────────────────────────────────────────────────────────┐
   │                  Free = cost_install  (install the unit grade)        │
   │          G ◀───────────────────────────────────────────▶ Cost(G)     │
   │                  Forget = cost_forget  (strip the grade)              │
   │                                                                       │
   │   Forget ∘ Free = id          cost_forget_install      ✓ (round-trip) │
   │   Free   ∘ Forget ≠ id        cost_install_forget_alters ✓            │
   │             "structure-preserving, behaviour-altering"                │
   │   both natural: cost_install_natural / cost_forget_natural            │
   └──────────────────────────────────────────────────────────────────────┘

   ADJUNCTION II — internalisation of Cost(G) into the TC base, up to ≈     CL5
   ┌──────────────────────────────────────────────────────────────────────┐
   │      Cost(rho) ──── St = st_tr (the gate translation) ────▶ pure rho   │
   │                                                                       │
   │   ACHIEVABLE STRENGTH (delivered):                                    │
   │     · ca_translation_progresses   — every ca_step's image steps       │
   │     · rule1..5_reachable          — the full per-rule reductions      │
   │     · ca_single_gate_bisimilar    — single-gate strong bisimulation   │
   │                  (matches the old model's guarantee)                  │
   │                                                                       │
   │   FORCE-LIMIT (docs §3a) ▓▓▓ a full strong/weak bisimulation across a │
   │     general ca_step is blocked: the gate translation OVER-GATES at a   │
   │     force *x  —  St U (gated, stuck)  ≁  Pt(st_to_proc U) (stripped).  │
   │     Needs a force-cashing translation refinement (research-grade).     │
   └──────────────────────────────────────────────────────────────────────┘
```

Adjunction I is the *structural* split (install/strip the apparatus); Adjunction
II is the *behavioural* internalisation (the cost calculus simulates into the
Turing-complete base). The former is complete; the latter is delivered at the
strength the old model achieves, with the force-point boundary documented.
