# Located Capabilities in Space–Time

Phlogiston capability is SPATIALLY located on signature surfaces and consumed in
TEMPORAL order (the free token-stack monoid). Rocq: `CALocatedPurses.v` (CL8);
TLA+: `formal/tlaplus/cost_accounted_rho/LocatedPurse.tla`.

```
      SPACE (signature surfaces s)                    TIME (token stack ↓)
   ───────────────────────────────────▶          consumption order (free
                                                   monoid — NOT commutative)
   surface s1 │ supply ▓▓▓▓▓  demand ▓▓        │   t = s_a : s_b : s_c : ()
   surface s2 │ supply ▓▓▓▓   demand ▓▓▓       │        │    │    │
   surface s3 │ supply ▓▓▓▓▓▓ demand ▓         │        ▼    ▼    ▼
              ╎       (local_sufficient:               consumed left-to-right,
              ╎        demand s ≤ supply s)            order PRESERVED
              ╎
      DISJOINT lanes  (draw_disjoint):  a draw on s1 leaves s2, s3 UNTOUCHED
              ╎        — the Rocq image of ChannelSeparation.lane_pool_disjoint
              ▼
      COMPOSITION (local_sufficiency_composes):
              Σ_s demand s   ≤   Σ_s supply s
        local sufficiency at every surface  ⇒  global executability
        (the TLA+ Inv_LocalSufficiencyComposes, proved per-surface in Rocq)
```

Space = the signature channels `Nt s` (disjoint capability pools, one per
surface); Time = the token stack consumed in order (the modulus `token_size`,
which `μ` accumulates). Local sufficiency composes because the lanes are disjoint
— no surface can rob another.
