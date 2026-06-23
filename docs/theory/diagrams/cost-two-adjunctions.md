# The Two Adjunctions of the Cost Construction

`continued-gslt-cost-v2`'s adjunctions, as realized in Rocq (`CAAdjunctions.v`,
CL5) and the faithfulness stack (`CATranslationFaithfulness.v` / `CABisimulation.v`).

![The two adjunctions of the cost construction. Adjunction I (Free ⊣ Forget): the objects G and Cost(G) with Free = cost_install (green) and Forget = cost_forget (blue); the round-trip Forget ∘ Free = id holds, while Free ∘ Forget ≠ id (structure-preserving, behaviour-altering), both natural. Adjunction II: Cost(ρ) maps into pure ρ via the gate translation St; the achievable strengths (ca_translation_progresses, rule1..5_reachable, ca_single_gate_bisimilar) are listed in green, and the force-limit — St over-gates at a force *x, blocking a full bisimulation across a general ca_step — is boxed in red as an out-of-scope refinement.](cost-two-adjunctions.svg)

(*Source: [`cost-two-adjunctions.tex`](cost-two-adjunctions.tex) — render with `lualatex --output-format=dvi docs/theory/diagrams/cost-two-adjunctions.tex && dvisvgm --font-format=woff --exact docs/theory/diagrams/cost-two-adjunctions.dvi -o docs/theory/diagrams/cost-two-adjunctions.svg` (or `./render.sh cost-two-adjunctions.tex`).*)

Adjunction I is the *structural* split (install/strip the apparatus); Adjunction
II is the *behavioural* internalisation (the cost calculus simulates into the
Turing-complete base). The former is complete; the latter is delivered at the
strength the old model achieves, with the force-point boundary documented.
