# The Cost Endofunctor 𝔠 and its Monad Structure

`continued-gslt-cost-v2`'s central thesis, as realized in Rocq (`CostMonad.v`,
CL3+CL4): cost accounting is an endofunctor 𝔠 — indeed a monad — whose laws
descend from the two constituent monoids (`SignatureMonoid.v`, CL2).

![The cost endofunctor 𝔠 = (· × grade) and its monad structure, as three commutative diagrams: the naturality square for the unit η (X → 𝔠X over f, with 𝔠f); the unit laws (the two triangles μ ∘ η_{𝔠X} = id and μ ∘ 𝔠η = id); and the associativity square (μ ∘ μ_{𝔠X} = μ ∘ 𝔠μ). η arrows are green (the cost-free embedding), μ arrows blue (grade combination). The side panel records grade = (sig, token), that the laws (cost_left_unit, cost_right_unit, cost_assoc) descend from the signature commutative monoid and the token-stack free monoid, and that the monad is non-idempotent — metering twice ≠ once.](cost-endofunctor-monad.svg)

(*Source: [`cost-endofunctor-monad.tex`](cost-endofunctor-monad.tex) — render with `lualatex --output-format=dvi docs/theory/diagrams/cost-endofunctor-monad.tex && dvisvgm --font-format=woff --exact docs/theory/diagrams/cost-endofunctor-monad.dvi -o docs/theory/diagrams/cost-endofunctor-monad.svg` (or `./render.sh cost-endofunctor-monad.tex`).*)

The `μ`-flatten of nested wrappers `𝔠²X ⇒ 𝔠X` is exactly the move the old
bare-proc `SSigned : proc → sig → system` could **not** even type (it carries a
bare proc, so `SSigned (SSigned …) …` is ill-formed). The native four-sort grammar
(`CASyntax.v`, DR-21) re-types continuations as signed terms, making `μ`
expressible — here as plain grade multiplication.
