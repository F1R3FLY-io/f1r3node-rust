# Slashing — Mechanized Rocq Proofs

This directory contains the kernel-checked formal verification of the slashing
logic. The companion mathematical exposition lives in
`docs/theory/slashing/slashing-verification.md`; this README is a quick
operator's guide.

## Building

```sh
# Generate the Makefile from _CoqProject (one-time)
coq_makefile -f _CoqProject -o Makefile

# Compile under resource limits (per CLAUDE.md)
systemd-run --user --scope \
            -p MemoryMax=96G -p CPUQuota=1800% \
            -p IOWeight=30 -p TasksMax=200 \
            make -j1
```

The `-j1` flag is mandatory: several modules (notably `Bisimulation.v`,
`TwoLevelSlashing.v`) are memory-intensive (peak ~12 GB each).

## Verifying the trust base

After a successful build:

```sh
coqtop -batch -load-vernac-source theories/MainTheorem.v \
       -e 'Print Assumptions main_bisimilarity_theorem.'
```

The output must be `Closed under the global context`. Any custom axiom,
parameter, or `Admitted` in the trust base is a verification failure.
Search-horizon witnesses from Sage, Hypothesis, fuzzing, Kani, or TLA+
do not change the Rocq trust base by themselves. They become Rocq work only
after Rust traceability promotes them to a stable theorem, counterexample,
or permitted bug-fix delta; no admissions or uncited axioms may be used for
that promotion.

## Module dependency graph

```
                   ┌──────────────┐
                   │  Validator   │
                   └──────┬───────┘
                          │
        ┌─────────────────┼─────────────────┐
        ▼                 ▼                 ▼
   ┌─────────┐      ┌──────────┐    ┌───────────────┐
   │  Block  │      │  PoSCtrt │    │  EquivocRec   │
   └────┬────┘      └─────┬────┘    └───────┬───────┘
        │                 │                 │
        ├─────────────────┴─────────────────┤
        ▼                                   ▼
   ┌──────────┐                       ┌─────────────┐
   │ InvBlock │                       │  DAGState   │
   └─────┬────┘                       └──────┬──────┘
         │                                   │
         └───────────────────────────────────┤
                                             ▼
                              ┌──────────────────────────┐
                              │   EquivocationDetector   │
                              └──────────────────────────┘

(See _CoqProject for the full graph including SlashDeploy, BlockCreator,
 ForkChoice, TwoLevelSlashing, ValidatorLifetime, BugFix*, Bisimulation,
 MainTheorem.)
```

## Mapping to the verification document

Every theorem stated in `docs/theory/slashing/slashing-verification.md` carries
a `(name, file:line)` anchor pointing into this directory. The reverse mapping
(from Rocq identifier to verification-doc section) is in §11.2 of the
verification document.

## Pedigree of contributions

Per the `[San98] [LSP82] (a)/(b)/(c)/(d)` classification scheme used in the
cost-accounting precedent:

- **(a) Direct mechanizations** — Rocq encodings of paper algorithms (e.g.
  `EquivocationDetector.detect`, `prepare_slashing_deploys`).
- **(b) Verifications of paper algorithms** — `EquivocationDetector` soundness
  and completeness (T-1, T-2), `slash` zeros bond (T-7).
- **(c) Proof-original extensions** — bisimilarity Rust ~~ Scala (T-13–T-15);
  proven bug-fix deltas (T-9.1–T-9.15, including T-9.10' / T-9.10″ for the
  withdrawal flow), plus current-epoch slash authorization, checked sequence
  arithmetic, duplicate-justification rejection, and auth-token no-op
  wrappers.
- **(d) Citable-axiom-gated** — none in the consensus-critical path; all
  classical lemmas appear in the trust base only.

See verification doc §1.5 for the full pedigree table.
