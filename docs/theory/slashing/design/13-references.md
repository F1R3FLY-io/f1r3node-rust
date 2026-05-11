# 13 · References

All references cited in this design document set, with DOIs where
available. DOIs and arXiv links have been verified against the
upstream specification at
[`../slashing-specification.md`](../slashing-specification.md) §14
(which was itself audited across six review passes).

## 13.1 Casper / FFG / GHOST / consensus

- **[BG19]** V. Buterin and V. Griffith.
  *Casper the Friendly Finality Gadget*.
  arXiv:1710.09437, 2019.
  [doi:10.48550/arXiv.1710.09437](https://doi.org/10.48550/arXiv.1710.09437)

- **[BHKPQRSWZ20]** V. Buterin, D. Hernandez, T. Kamphefner,
  K. Pham, Z. Qiao, D. Ryan, J. Sin, Y. Wang, Y. X. Zhang.
  *Combining GHOST and Casper*.
  arXiv:2003.03052, 2020.
  [doi:10.48550/arXiv.2003.03052](https://doi.org/10.48550/arXiv.2003.03052)

- **[Z16]** V. Zamfir.
  *The History of Casper* (Parts 1–5).
  Medium, 2016. (No DOI — Medium article series.)
  [https://medium.com/@Vlad_Zamfir/the-history-of-casper-part-1-59233819c9a9](https://medium.com/@Vlad_Zamfir/the-history-of-casper-part-1-59233819c9a9)

- **[CBCCoq20]** E. Li, T. F. Șerbănuță, D. Diaconescu, V. Zamfir,
  G. Roșu. *Formalizing Correct-by-Construction Casper in Coq*.
  IEEE International Conference on Blockchain and Cryptocurrency
  (ICBC) 2020.
  [doi:10.1109/ICBC48266.2020.9169468](https://doi.org/10.1109/ICBC48266.2020.9169468)

## 13.2 BFT consensus / Tendermint / evidence modules

- **[BKM18]** E. Buchman, J. Kwon, Z. Milosevic.
  *The latest gossip on BFT consensus*.
  arXiv:1807.04938, 2018.
  [doi:10.48550/arXiv.1807.04938](https://doi.org/10.48550/arXiv.1807.04938)

- **[ABPT19]** Y. Amoussou-Guenou, A. Del Pozzo,
  M. Potop-Butucaru, S. Tucci-Piergiovanni.
  *Correctness of Tendermint-Core Blockchains*.
  OPODIS 2018, LIPIcs 125, 16:1–16:16, 2019.
  [doi:10.4230/LIPIcs.OPODIS.2018.16](https://doi.org/10.4230/LIPIcs.OPODIS.2018.16)

- **[BBKMW20]** S. Braithwaite, E. Buchman, I. Konnov,
  Z. Milosevic, I. Stoilkovska, J. Widder, A. Zamfir.
  *Formal Specification and Model Checking of the Tendermint
  Blockchain Synchronization Protocol*. FMBC 2020, OASIcs 84, paper 10.
  [doi:10.4230/OASIcs.FMBC.2020.10](https://doi.org/10.4230/OASIcs.FMBC.2020.10)

- **[CL99]** M. Castro and B. Liskov.
  *Practical Byzantine Fault Tolerance*.
  OSDI 1999, 173–186.
  [https://www.usenix.org/conference/osdi-99/practical-byzantine-fault-tolerance](https://www.usenix.org/conference/osdi-99/practical-byzantine-fault-tolerance)

  > Canonical journal version: M. Castro and B. Liskov,
  > *Practical Byzantine Fault Tolerance and Proactive Recovery*,
  > ACM Transactions on Computer Systems 20(4):398–461, 2002,
  > [doi:10.1145/571637.571640](https://doi.org/10.1145/571637.571640).

## 13.3 Byzantine fault tolerance — foundational

- **[LSP82]** L. Lamport, R. Shostak, M. Pease.
  *The Byzantine Generals Problem*.
  ACM TOPLAS, 4(3):382–401, 1982.
  [doi:10.1145/357172.357176](https://doi.org/10.1145/357172.357176)

  > Source of the BFT bound `f < n/3` used in T-12.

## 13.4 Process calculus / bisimulation / rho-calculus

- **[Mil89]** R. Milner. *Communication and Concurrency*.
  Prentice-Hall, 1989. ISBN 978-0131149847. (Book; no DOI.)

- **[Mil99]** R. Milner. *Communicating and Mobile Systems: The
  π-Calculus*. Cambridge University Press, 1999.
  ISBN 978-0521643207. (Book; no DOI.)

- **[SW01]** D. Sangiorgi and D. Walker. *The π-Calculus: A Theory
  of Mobile Processes*. Cambridge University Press, 2001.
  ISBN 978-0521781770. (Book; no DOI.)

- **[San98]** D. Sangiorgi.
  *On the bisimulation proof method*.
  *Mathematical Structures in Computer Science*, 8(5):447–479, 1998.
  [doi:10.1017/S0960129598002527](https://doi.org/10.1017/S0960129598002527)

- **[MR05a]** L. G. Meredith and M. Radestock.
  *A Reflective Higher-order Calculus*.
  *Electronic Notes in Theoretical Computer Science*, 141(5):49–67, 2005.
  [doi:10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

  > The rho-calculus that underlies Rholang. Cited for α-equivalence
  > on Rholang names in the bisimilarity discussion (§10).

- **[MR05b]** L. G. Meredith and M. Radestock.
  *Namespace Logic: A Logic for a Reflective Higher-Order Calculus*.
  TGC 2005, LNCS 3705, 353–369.
  [doi:10.1007/11580850_19](https://doi.org/10.1007/11580850_19)

- **[Lyb22]** S. Lybech.
  *Encodability and Separation for a Reflective Higher-Order Calculus*.
  arXiv:2209.02356, 2022.
  [doi:10.48550/arXiv.2209.02356](https://doi.org/10.48550/arXiv.2209.02356)

## 13.5 Formal verification of distributed systems

- **[WWPTWEA15]** J. R. Wilcox, D. Woos, P. Panchekha, Z. Tatlock,
  X. Wang, M. D. Ernst, T. Anderson.
  *Verdi: A Framework for Implementing and Formally Verifying
  Distributed Systems*. PLDI 2015, 357–368.
  [doi:10.1145/2737924.2737958](https://doi.org/10.1145/2737924.2737958)

- **[GKMB17]** V. B. F. Gomes, M. Kleppmann, D. P. Mulligan,
  A. R. Beresford. *Verifying Strong Eventual Consistency in
  Distributed Systems*. PACMPL, 1(OOPSLA):109, 2017.
  [doi:10.1145/3133933](https://doi.org/10.1145/3133933)

## 13.6 Block-DAG / GHOST extensions

- **[SZ15]** Y. Sompolinsky, A. Zohar.
  *Secure High-Rate Transaction Processing in Bitcoin*. FC 2015,
  LNCS 8975, 507–527.
  [doi:10.1007/978-3-662-47854-7_32](https://doi.org/10.1007/978-3-662-47854-7_32)

  > Source of the GHOST (Greedy Heaviest-Observed Sub-Tree)
  > fork-choice rule used in §07 (T-10). Full version: IACR ePrint
  > 2013/881, [https://eprint.iacr.org/2013/881](https://eprint.iacr.org/2013/881).

- **[LSZ15]** Y. Lewenberg, Y. Sompolinsky, A. Zohar.
  *Inclusive Block Chain Protocols*. FC 2015, LNCS 8975, 528–547.
  [doi:10.1007/978-3-662-47854-7_33](https://doi.org/10.1007/978-3-662-47854-7_33)

  > Block-DAG follow-on; cited where the design discusses inclusive
  > DAG variants (not the GHOST fork-choice rule itself).

## 13.7 Concurrency and consistency foundations

- **[HW90]** M. P. Herlihy and J. M. Wing.
  *Linearizability: A Correctness Condition for Concurrent Objects*.
  ACM Transactions on Programming Languages and Systems 12(3):463–492,
  1990.
  [doi:10.1145/78969.78972](https://doi.org/10.1145/78969.78972)

  > Authoritative reference for linearizability, the correctness
  > condition discharged by the atomic Read-Modify-Write contract
  > used in Bug #2 / T-9.2 (§05 storage).

## 13.8 Proof-of-Stake economic security

- **[Sal21]** F. Saleh.
  *Blockchain Without Waste: Proof-of-Stake*.
  Review of Financial Studies 34(3):1156–1190, 2021.
  [doi:10.1093/rfs/hhaa075](https://doi.org/10.1093/rfs/hhaa075)

  > Cited from §01.1 (motivation): formalizes the rational-adversary
  > model where the slash penalty must dominate the expected attack
  > profit. Background for the *Economic and game-theoretic threats*
  > section of [`../slashing-threat-model.md`](../slashing-threat-model.md).

## 13.9 Tools and proof assistants

- **[Rocq]** The Rocq Prover Development Team.
  *The Rocq Prover Reference Manual*, version 9.1.x, 2025.
  [https://rocq-prover.org](https://rocq-prover.org)
  (Zenodo DOI for the v8.20 baseline: [doi:10.5281/zenodo.14542673](https://doi.org/10.5281/zenodo.14542673).)

- **[Lamport02]** L. Lamport.
  *Specifying Systems: The TLA+ Language and Tools for Hardware and
  Software Engineers*. Addison-Wesley, 2002. ISBN 0-321-14306-X.

  > Authoritative reference for TLA+ and TLC; cited from §10
  > whenever a TLA+ construct is introduced.

- **[YML99]** Y. Yu, P. Manolios, L. Lamport.
  *Model Checking TLA+ Specifications*.
  CHARME 1999, LNCS 1703, 54–66.
  [doi:10.1007/3-540-48153-2_6](https://doi.org/10.1007/3-540-48153-2_6)

- **[Apalache19]** I. Konnov, J. Kukovec, T.-H. Tran.
  *TLA+ Model Checking Made Symbolic*.
  Proc. ACM Programming Languages 3(OOPSLA):123:1–30, 2019.
  [doi:10.1145/3360549](https://doi.org/10.1145/3360549)

  > SMT-backed alternative to TLC; used optionally for symbolic
  > verification of selected invariants.

- **[Kani]** Kani Rust Verifier. AWS Labs.
  [https://model-checking.github.io/kani/](https://model-checking.github.io/kani/)

- **[Miri]** R. Jung, J.-H. Jourdan, R. Krebbers, D. Dreyer et al.
  *Miri — an interpreter for Rust's mid-level intermediate
  representation*.
  [https://github.com/rust-lang/miri](https://github.com/rust-lang/miri)

- **[SageMath]** The Sage Developers.
  *SageMath, the Sage Mathematics Software System*, version 10.x.
  [https://www.sagemath.org](https://www.sagemath.org)

- **[proptest]** A. Gallant et al.
  *proptest — Hypothesis-style property-testing for Rust*.
  [https://github.com/proptest-rs/proptest](https://github.com/proptest-rs/proptest)

- **[loom]** Tokio Contributors.
  *loom — Permutation-testing tool for concurrent Rust*.
  [https://github.com/tokio-rs/loom](https://github.com/tokio-rs/loom)

- **[Hypothesis]** D. R. MacIver et al.
  *Hypothesis — Property-based testing for Python*.
  [https://hypothesis.readthedocs.io](https://hypothesis.readthedocs.io)

- **[cargo-fuzz]** Rust Fuzzing Authority.
  *cargo-fuzz — libFuzzer integration for Cargo*.
  [https://github.com/rust-fuzz/cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)

## 13.10 Reference implementations

- **[ETH-SPEC]** Ethereum Foundation. *Phase 0 — Honest Validator*
  and *Phase 0 — Beacon Chain*. `ethereum/consensus-specs`,
  accessed 2026-05-01. (No DOI; specifications repository.)
  - [validator.md](https://github.com/ethereum/consensus-specs/blob/master/specs/phase0/validator.md)
  - [beacon-chain.md](https://github.com/ethereum/consensus-specs/blob/master/specs/phase0/beacon-chain.md)

- **[COSMOS-ADR009]** Cosmos SDK Working Group.
  *ADR 009: Evidence Module*. (No DOI; ADR document.)
  [https://github.com/cosmos/cosmos-sdk/blob/main/docs/architecture/adr-009-evidence-module.md](https://github.com/cosmos/cosmos-sdk/blob/main/docs/architecture/adr-009-evidence-module.md)

## 13.11 Citation usage map

| Reference           | Where cited in this design                                                                 |
|---------------------|--------------------------------------------------------------------------------------------|
| **[BG19]**          | §01.5 Related systems (FFG comparison); cited from §01.5.                                  |
| **[BHKPQRSWZ20]**   | §01.5 (FFG + GHOST combined).                                                              |
| **[Z16]**           | §01.1 (Casper history).                                                                    |
| **[CBCCoq20]**      | §01.4 (formal verification of CBC).                                                        |
| **[BKM18]**         | §01.5 (Tendermint BFT); §12.5 (safety/liveness tradeoffs).                                 |
| **[ABPT19]**        | §01.5 (Tendermint correctness); §12.5.                                                     |
| **[BBKMW20]**       | §01.5 (Tendermint formal model).                                                           |
| **[CL99]**          | §08 (PBFT).                                                                                |
| **[LSP82]**         | §08.4 (BFT bound `f < n/3`); T-12 corollary.                                               |
| **[Mil89]**         | §02.2 (process-algebraic notation); §10.10 (weak bisimulation).                            |
| **[Mil99]**         | §10.10 (π-calculus weak bisimulation).                                                     |
| **[SW01]**          | §10.10 (π-calculus theory).                                                                |
| **[San98]**         | §10.10 (bisimulation proof method); used in `Bisimulation.v`.                              |
| **[MR05a]**         | §06.7 (capability security via unforgeable names); §10.9 (α-equivalence on Rholang names). |
| **[MR05b]**         | §06 (namespace logic — referenced for further reading).                                    |
| **[Lyb22]**         | §10 (encodability of rho-calculus).                                                        |
| **[WWPTWEA15]**     | §01.4 (formal-methods precedent).                                                          |
| **[GKMB17]**        | §10 (eventual-consistency proof methodology).                                              |
| **[SZ15]**          | §02.1 (GHOST acronym); §07.1 (GHOST fork-choice rule); T-10.                               |
| **[LSZ15]**         | Block-DAG / inclusive-chain background; cited where DAG-variant fork-choice is discussed.  |
| **[HW90]**          | §05 (atomic Read-Modify-Write contract); T-9.2 (linearizability of tracker insertion).     |
| **[Sal21]**         | §01.1 (rational-adversary model motivating slashing); threat-model §5.A.                   |
| **[Rocq]**          | Every Rocq proof artifact (`formal/rocq/slashing/theories/*.v`); §10 (mechanization).      |
| **[Lamport02]**     | §10 (TLA+ language and TLC).                                                               |
| **[YML99]**         | §10 (TLC model-checking algorithm).                                                        |
| **[Apalache19]**    | §10 (optional symbolic TLA+ verification).                                                 |
| **[Kani]**          | §14 (Rust symbolic verification of slashing-authorization paths).                          |
| **[Miri]**          | §14 (UB detection in concurrency-sensitive code).                                          |
| **[SageMath]**      | §14 (Sage exploratory and threat models).                                                  |
| **[proptest]**      | §14 (Rust property-based tests T-1..T-15).                                                 |
| **[loom]**          | §14 (concurrent-interleaving exploration for T-9.2).                                       |
| **[Hypothesis]**    | §14 (Python-side property-based exploration corpus).                                       |
| **[cargo-fuzz]**    | §14 (libFuzzer-driven differential and roundtrip targets in `fuzz/`).                      |
| **[ETH-SPEC]**      | §01.5 (Ethereum slashing reference).                                                       |
| **[COSMOS-ADR009]** | §01.5 (Cosmos evidence module reference).                                                  |

## 13.12 Companion documents (cross-internal)

These are *not* external references but companion documents in
this repository. They are listed here for completeness:

- [`../slashing-specification.md`](../slashing-specification.md) —
  Normative specification.
- [`../slashing-verification.md`](../slashing-verification.md) —
  Proof artifact (Rocq + TLA+).
- [`../README.md`](../README.md) — Index for the slashing
  documentation directory.
- [`../diagrams/`](../diagrams/) — **Eleven** PlantUML source files
  plus rendered SVGs; see [§README diagram table](README.md#diagrams).
- [`../slashing-search-horizon.md`](../slashing-search-horizon.md) —
  Defensive search-horizon program.
- [`../slashing-threat-model.md`](../slashing-threat-model.md) —
  Threat model.
- [`../slashing-traceability.md`](../slashing-traceability.md) —
  Sage/Hypothesis finding ledger.

## 13.13 Changelog

- **2026-05-02** — Initial pedagogical design document; thirteen
  files plus README in `docs/theory/slashing/design/`.
- **2026-05-03** — Added §14 test plan (example-based + property-
  based + cross-implementation + TLA+ + pre-fix regressions);
  doc set is now fourteen files plus README.
- **2026-05-08** — Added `14a-tier-architecture.md` (triple-bisim
  tier architecture); design-doc set now fifteen files plus README.
- **2026-05-09** — Bugs #11..#16 propagated through §09 with their
  Rocq theorems (`T-9.11`..`T-9.15`, `T-Auth`,
  `deploy_epoch_matches_target`) and TLA+ invariants; numbering gap
  at §9.12 closed by renumbering §9.13..§9.20 → §9.12..§9.19; bug
  ledger headline table at §9.1 corrected to use `T-9.13` for
  Bug #12 and `T-LivenessGap` for Bug #14.
- **2026-05-11** — Citation correctness pass: `[LSZ15]` (Inclusive
  Block-DAG) split from `[SZ15]` (Sompolinsky/Zohar GHOST paper,
  DOI _32); `[CBCCoq20]` gained authors and DOI; tool citations
  added (TLA+/Specifying Systems, Apalache, Kani, Miri, SageMath,
  proptest, loom, Hypothesis, cargo-fuzz, Rocq Reference Manual);
  Herlihy/Wing linearizability and Saleh PoS-economics added.

---

**Next:** [§14 — Test plan](14-test-plan.md)
