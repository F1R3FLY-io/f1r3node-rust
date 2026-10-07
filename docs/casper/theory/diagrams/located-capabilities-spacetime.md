# Located Capabilities in Space–Time

Phlogiston capability is SPATIALLY located on signature surfaces and consumed in
TEMPORAL order (the free token-stack monoid). Rocq: `CALocatedPurses.v` (CL8);
TLA+: `formal/tlaplus/cost_accounted_rho/LocatedPurse.tla`.

![Located capabilities in space–time. On the left, SPACE: three disjoint signature surfaces s₁, s₂, s₃, each a capability pool with supply ≥ demand (local sufficiency). On the right, TIME: the token stack s_a → s_b → s_c consumed left-to-right in the free (non-commutative) monoid order. A blue draw_disjoint arrow records that a draw on one surface leaves the others untouched (ChannelSeparation.lane_pool_disjoint); a green local_sufficiency_composes arrow reaches the COMPOSITION node Σ demand ≤ Σ supply, so local sufficiency at every surface yields global executability.](located-capabilities-spacetime.svg)

(*Source: [`located-capabilities-spacetime.d2`](located-capabilities-spacetime.d2) — render with `d2 --layout elk docs/casper/theory/diagrams/located-capabilities-spacetime.d2 docs/casper/theory/diagrams/located-capabilities-spacetime.svg` (or `./render.sh located-capabilities-spacetime.d2`).*)

Space = the signature channels `Nt s` (disjoint capability pools, one per
surface); Time = the token stack consumed in order (the modulus `token_size`,
which `μ` accumulates). Local sufficiency composes because the lanes are disjoint
— no surface can rob another.
