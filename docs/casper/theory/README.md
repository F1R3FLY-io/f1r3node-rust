# Casper Theory Dossiers

Four dossier areas hold the formal treatment of the Casper consensus.
Each of the first three areas follows one triad shape: a normative
specification, a literate glossary, and a verification dossier that maps
the mechanized proofs.

| Area | Specification | Glossary | Verification |
|---|---|---|---|
| Fork choice ("Ghosting") | [spec](./fork-choice/fork-choice-specification.md) | [glossary](./fork-choice/fork-choice-glossary.md) | [dossier](./fork-choice/fork-choice-verification.md) |
| Finalized floor | [spec](./finalized-floor/finalized-floor-specification.md) | [glossary](./finalized-floor/finalized-floor-glossary.md) | [dossier](./finalized-floor/finalized-floor-verification.md) |
| Merge algebra | [spec](./merge-algebra/merge-algebra-specification.md) | [glossary](./merge-algebra/merge-algebra-glossary.md) | [dossier](./merge-algebra/merge-algebra-verification.md) |

The slashing subsystem carries the largest dossier:

- [slashing/](./slashing/README.md) — specification, verification,
  threat model, traceability, and search horizon
- [slashing/design/](./slashing/design/README.md) — the ordered design
  record, chapters 01–15
- [slashing/methodology/](./slashing/methodology/README.md) — the
  verification methodology library (attack modeling, case studies,
  differential and metamorphic testing, formal methods, randomized
  search, Sage models, tutorials)

The formal artifacts these dossiers cite live under `formal/**` and stay
platform-owned. [docs/formal-verification.md](../../formal-verification.md)
is their umbrella index.

[← Back to the Casper documentation map](../README.md)
