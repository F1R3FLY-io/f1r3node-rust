# 10 · Differential divergence after the Rust–Scala port

## Status

The former Rust–Scala weak-barbed-bisimilarity development is retired. It was
a migration-time differential oracle, not a permanent protocol theorem. The
cost-accounted-rho node has consensus-visible state, authority, settlement,
and replay behavior with no Scala implementation to which the current Rust
node can be structurally bisimilar.

The retired result comprised T-13a/b/c, T-14, T-15a/b, the former
`Bisimulation.v` theory, and their Rust projection tests. Git history retains
those artifacts. They are not part of the current proof gate and must not be
cited as evidence for the current node.

This removal is the decision recorded by
[DR-6](15-decision-records.md#dr-6--removal-of-the-rustscala-bisimilarity-2026-05-29)
and the removal note in the
[verification dossier](../slashing-verification.md#8--differential-divergence-calculus).

## Current correctness authority

The current mechanized headline is `main_slashing_algorithm_correct` in
`formal/rocq/slashing/theories/MainTheorem.v`. It composes the slashing
pipeline obligations that remain meaningful for the Rust protocol:

- admissible and ignorable equivocations produce authorized evidence;
- accepted slash effects zero the intended bond generation;
- evidence recording and fork-choice exclusion agree with the slash verdict;
- forfeited stake moves through the authenticated PoS transition;
- two-level closure, validator lifetime, duplicate evidence, and arithmetic
  boundaries satisfy their named invariants.

The normative statement and its assumptions are maintained in the
[slashing specification](../slashing-specification.md#9--headline-correctness-statement)
and [verification dossier](../slashing-verification.md).

## Retained differential method

Cross-implementation traces remain useful for discovering porting defects, but
their results are classified rather than presumed equivalent. The retained
divergence calculus assigns every observed disagreement to one of four classes:

1. no divergence;
2. a permitted, documented bug-fix delta;
3. a candidate boundary caused by an explicit protocol hypothesis;
4. an unexpected divergence that blocks acceptance until it is explained.

The executable classifier lives in
`casper/tests/slashing/divergence_class.rs`. Differential, metamorphic, Sage,
and property-based campaigns feed that classifier. A new disagreement is never
silently added to the permitted set; it requires a decision record, a protocol
argument, and corresponding proof and regression evidence.

## Review rule

Do not infer current Rust correctness from historical agreement with Scala.
Reviewers must instead follow each current claim through the normative
specification, its Rocq or TLA+ theorem, the executable correspondence test,
and the aggregate verification gate. Historical bisimilarity is evidence about
the completed port only.

**Previous:** [§09 — Bug fixes and rationale](09-bug-fixes-and-rationale.md)

**Next:** [§11 — Worked examples](11-worked-examples.md)
