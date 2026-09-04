# The Gated Interaction-Cut (a Cost-Accounted COMM)

The five gated COMM rules (`CAReduction.v`) in interaction-cut form: a redex fires
only when a co-present token authorizes it (the no-leak invariant). GAP-2 is
dissolved — the continuation keeps its OWN seal, with no SAnd re-seal.

![The gated interaction-cut (Rule 1, atomic s): the signed redex { for(y ← x){T} ∣ x!(U) }ₛ in parallel with the token stack s : S reduces, by a single green gated COMM in which the gate s consumes exactly one authorizing token (no-leak), to the continuation T{@U/y} — which keeps its OWN seal (GAP-2: no SAnd re-seal) — in parallel with the tail S. The grade of the step is the consumed signature s.](interaction-cut.svg)

(*Source: [`interaction-cut.puml`](interaction-cut.puml) — render with `plantuml -tsvg docs/casper/theory/diagrams/interaction-cut.puml` (or `./render.sh interaction-cut.puml`).*)

The five gated COMM rules, by (signature, processes, token) shape:

| rule | signature | receiver / sender | token(s) | translation fires |
|------|-----------|-------------------|----------|-------------------|
| Rule 1 | atomic `s` | whole redex | `s : t` | 2 COMMs |
| Rule 2 | `s₁ ∧ s₂` | whole redex | `s₁:t₁, s₂:t₂` | 3 COMMs (nested) |
| Rule 3 | `s₁ ∧ s₂` | whole redex | `(s₁∧s₂) : t` | 4 COMMs (Split) |
| Rule 4 | `s₁, s₂` | split | `(s₁∧s₂) : t` | 4 COMMs (Split) |
| Rule 5 | `s₁, s₂` | split | `s₁:t₁, s₂:t₂` | 3 COMMs |

Subject reduction + no-leak: `WrappingSubjectReduction.v`; each rule's pure-rho simulation: `rule1..5_reachable` (`CATranslationFaithfulness.v`).

Interaction-cut = the receiver `for(y←x){T}` and sender `x!(U)` annihilate on the
shared channel `x`, contracting to `T{@U/y}` (Milner's pseudo-application: the
quoted payload `@U` substitutes the bound name), gated by exactly one token.
