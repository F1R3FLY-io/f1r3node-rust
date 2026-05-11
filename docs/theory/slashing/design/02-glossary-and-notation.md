# 02 · Glossary & Notation

This document defines every symbol, acronym, and term used in the
remainder of the design document **before** it is used. If you encounter
a term you do not recognize while reading later sections, return here.

## 2.1 Acronyms

| Acronym | Expansion                        | First-use context                                                  |
|---------|----------------------------------|--------------------------------------------------------------------|
| **PoS** | Proof of Stake                   | The consensus family this work targets (§01).                      |
| **BFT** | Byzantine Fault Tolerance        | The bound `f < n/3` from [LSP82] (§08).                            |
| **CBC** | Correct-by-Construction (Casper) | The CBC-style consensus implemented in F1R3FLY (§01).              |
| **DAG** | Directed Acyclic Graph           | The block graph; each block has zero or more parents (§03).        |
| **LMD** | Latest-Message-Driven (GHOST)    | The fork-choice rule; each validator's latest message votes (§07). |
| **RMW** | Read-Modify-Write                | The atomic primitive bug #2 protects (§05).                        |
| **TLC** | TLA+ model checker (Lamport's)   | Used for state-space exploration of TLA+ models (§10).             |
| **GFM** | GitHub Flavored Markdown         | Slug-rule for cross-doc anchor resolution.                         |
| **LTS** | Labeled Transition System        | Slashing pipeline's formal model `T = (S, L, →)` (§07; verif §3).  |
| **GHOST** | Greedy Heaviest-Observed Sub-Tree | Fork-choice rule [SZ15]; cf. **LMD-GHOST** (§07).               |
| **FFG** | Friendly Finality Gadget (Casper) | Ethereum 2.0 slashing comparison anchor (§01.5; [BG19]).          |
| **DOS** | Denial of Service                | The vector closed by bug fix #1 (§09.1, §10.1, §12.2.1).           |
| **KV**  | Key-Value                        | Store abstraction underlying the equivocation tracker (§05).       |
| **BFS** | Breadth-First Search             | Generic graph traversal; post-fix #7 uses a canonical self-chain walk (§09.8, T-9.7). |
| **TLA+** | Temporal Logic of Actions       | Lamport's specification language; checked by TLC (§10).            |
| **Rocq** | Proof assistant (formerly Coq)  | The mechanization target for proofs in `formal/rocq/`.             |
| **DFS**  | Depth-First Search               | Graph traversal alternative to BFS; appears in §11.7 (worked example trace) and detector-traversal discussion (§09.13). |
| **STRIDE** | Spoofing / Tampering / Repudiation / Information disclosure / Denial of service / Elevation of privilege | The six-bucket threat-modeling taxonomy of Howard & LeBlanc; used in `slashing-threat-model.md §2.1`. |
| **MIP**  | Mixed-Integer Programming        | Optimization formalism used in Sage's damage-optimizer and weighted-stake models (`formal/sage/slashing/`); appears in `slashing-threat-model.md §3` row 96 and `slashing-traceability.md` rows 21, 41. |
| **SMT**  | Satisfiability Modulo Theories   | The decision-procedure backend used by Apalache and similar tools; appears in `slashing-search-horizon.md §3.6`. |
| **MC**   | Model-Check                      | TLA+ filename prefix; `MC_<spec>.tla` and `MC_<spec>.cfg` instantiate `<spec>.tla` for TLC. |
| **MVar** | Mutable variable                 | Concurrency primitive providing a single-cell mutable container with atomic take/put; relevant to tracker-race discussion (§05.4). |
| **FFI**  | Foreign Function Interface       | Inter-language call boundary. Mentioned in `14a-tier-architecture.md §6.1` as a *rejected* design choice: Coq's `Extraction` target is OCaml, so linking extracted code into the Rust test build would require an OCaml/Rust FFI layer. Rholang itself is *not* an FFI consumer — it is implemented natively in the `rholang` Rust crate. |
| **LMDB** | Lightning Memory-Mapped Database | The key-value backend used by RSpace; storage layer of the slashing tracker (§05.5). |
| **RSpace** | F1R3FLY's tuple-space storage  | The persistence layer that holds DAG state, bond map, equivocation records, and coop vault (§05.5). |
| **ADR**  | Architecture Decision Record     | The DR-N entries in `15-decision-records.md`.                      |
| **OOM**  | Out of Memory                    | TLC heap-exhaustion outcome during liveness-graph construction; flagged in `slashing-verification.md §10`. |
| **PKI**  | Public-Key Infrastructure        | Authenticated identity layer; out of scope for the bisimilarity claim (T-15 holds modulo this assumption). |
| **DR-N** | Decision Record #N               | See `15-decision-records.md`.                                       |

## 2.2 Process-algebraic notation (informal)

These come from [Mil89], [Mil99], [SW01], [San98], used in the
bisimilarity discussion (§10).

| Symbol | Name                                     | Meaning                                                                                                                                                                      |
|--------|------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `→`    | LTS transition                           | `s →ℓ t` means the state machine moves from `s` to `t` under label `ℓ`. (Inline / prose form.)                                                                               |
| `⟶`    | LTS step (trace)                         | Long arrow used in code-fenced pipeline traces (e.g. `sign(v, s, b) ⟶ DAG += b`); equivalent to `→` but visually distinct in monospaced output. Matches spec §11 convention. |
| `→*`   | Multi-step                               | Reflexive-transitive closure of `→`.                                                                                                                                         |
| `~`    | Strong bisimilarity                      | Mutual simulation under matching labels.                                                                                                                                     |
| `≈`    | Weak bisimilarity                        | Mutual simulation modulo internal `τ` (silent) steps.                                                                                                                        |
| `≡_α`  | α-equivalence                            | Up to renaming of bound names (rho-calculus, [MR05a]).                                                                                                                       |
| `↓ℓ`   | Barb                                     | State can immediately perform observable action `ℓ`.                                                                                                                         |
| `⇓ℓ`   | Weak barb                                | State can perform `ℓ` after some `τ`-steps.                                                                                                                                  |
| `≈ₓ`   | Barbed equivalence                       | Bisimilarity up to barbs in set `x`.                                                                                                                                         |
| `⊤`    | Boolean true                             | Used in detection-decision predicates.                                                                                                                                       |
| `⊥`    | Boolean false / terminal absorbing state | Used in `(_, false)` returns and `Removed → ⊥` (§07).                                                                                                                        |
| `⟹`    | Logical implication                      | Used in formal LTS rules and theorem statements.                                                                                                                             |
| `∀`    | Universal quantifier                     | Used in formal blocks; rendered "for every" / "for all" in prose.                                                                                                            |
| `∃`    | Existential quantifier                   | Used in formal blocks; rendered "there exists" in prose.                                                                                                                     |
| `∅`    | Empty set / nil                          | The `EquivocationRecord` witness set after first insert; before any update.                                                                                                  |
| `∈`    | Set membership                           | Standard.                                                                                                                                                                    |
| `⊆`    | Subset (incl. equal)                     | Standard.                                                                                                                                                                    |
| `≡`    | Equivalence (mutual containment)         | Used for sets / lists where iteration order is not observable.                                                                                                               |
| `=`    | Strict equality                          | Used for natural numbers, function values, and pointwise-equal maps.                                                                                                         |
| `incl A B` | List/set containment (Rocq)         | `incl A B := ∀ x, In x A → In x B` — i.e. every element of `A` is also in `B`. Used in `Bisimulation.v`, `MainTheorem.v`, and `TwoLevelSlashing.v`; cited from §05.4.3 and §08 (T-11 statement). |
| `⊑`    | Order / refinement                       | Optional notation for ordering; used informally in §07 and §10.                                                                                                              |

## 2.3 Slashing protocol terms

| Symbol            | Name                | Meaning                                                                                                           |
|-------------------|---------------------|-------------------------------------------------------------------------------------------------------------------|
| `V`               | Validators          | Set of validator identities (modeled as `ℕ` in Rocq for decidable equality; `ByteString` in Rust/Scala).          |
| `B`               | Blocks              | Set of blocks; each `b ∈ B` has fields `(sender(b), seq(b), hash(b), J(b))`.                                      |
| `H`               | Block hashes        | Cryptographic block hashes.                                                                                       |
| `J(b) ⊆ V × H`    | Justifications      | Set of (validator, latest-block-hash) pairs cited by block `b`.                                                   |
| `BondMap : V → ℕ` | Bond map            | Partial function giving each validator's stake.                                                                   |
| `EqRec`           | Equivocation record | Triple `(v, baseSeqNum, witnesses ⊆ H)`.                                                                          |
| `E ⊆ EqRec`       | Record set          | The equivocation tracker's contents.                                                                              |
| `I ⊆ H`           | Invalid set         | Hashes of blocks marked invalid in the DAG.                                                                       |
| `A ⊆ V`           | Active set          | Validators whose bond is positive.                                                                                |
| `Sl ⊆ V`          | Slashed set         | Validators removed by a successful slash (the "ed" suffix is dropped to avoid collision with the LTS state name). |
| `C ∈ ℕ`           | Coop vault balance  | Accumulated forfeited stake.                                                                                      |
| `creator-justification` | Self-justification entry | The justification entry naming the block's *own* creator's prior block; pre-fix #6 it was the only check in `justification_regressions` for self-regression detection (§09.6). |
| `quorum`          | Active-set BFT quorum | Under `f ≤ ⌊(n−1)/3⌋`, the maximum tolerable Byzantine fraction; T-12 proves the slash closure preserves quorum.                                                                |
| `ilm`             | Invalid latest messages | The legacy DAG-side index `dag.invalid_latest_messages`; retained for historical bug-fix examples. Current slash-candidate generation uses authorized invalid-block evidence (§04, §06). |
| `Equivocation`    | Term of art         | A validator signing two distinct blocks at the same sequence number; formalized as Definition 4.1 in spec; cf. taxonomy §2.6.                                                   |
| `horizon`         | Search depth        | The depth — measured in evidence layers or sequence-number distance — up to which an adversarial campaign is materially exploitable. See `slashing-search-horizon.md §3.1` and the `HorizonCampaignDivergenceClass`/`HorizonV2DivergenceClass` Sage models. |
| `frontier`        | Unresolved-divergence set | The present set of local divergences in a search corpus that have not yet been classified (bisimilar, permitted_bug_fix, candidate_boundary, projection_risk, assumption_counterexample). See `Bisimulation.v:627-685` `*_boundary_requires_review` family. |
| `campaign`        | Composite attack sequence | A composed series of adversarial actions across multiple objectives and time windows; see `slashing-threat-model.md §3` and `slashing-search-horizon.md §3.5`. |
| `carryover policy` | Epoch-boundary evidence rule | The rule governing whether evidence accumulated in epoch `e` remains authorizable in epoch `e+1`. T-12RET formalises the temporal-retention boundary; spec §15 (Authorized Slash Evidence) is normative. |
| `retention window` | Validity interval  | The time / sequence interval during which evidence must remain replayable. Slash authorization is rejected for evidence older than the window. T-12 temporal-retention. |
| `evidence-denial min-cut` | Defensive lower bound | The smallest set of evidence edges whose removal disconnects every decisive evidence path from offender to neglecter. Sage Finding 88; threat-model row 17. |
| `projection risk` | Divergence class    | The divergence class where the mathematical model and the operational Rust projection can differ in a classified way. See `slashing-traceability.md` status `projection_risk`. |
| `objective-guided` | Sage search strategy | A Sage adversarial search that prioritizes traces by an explicit cost function (stake damaged, evidence denied, etc.); see `formal/sage/slashing/objective_frontier_model.sage`. |
| `metamorphic`     | Test technique       | A test that compares two semantically-equivalent transformations of the same input (e.g., the same closure under two reorderings) and asserts equal outcomes; see `metamorphic_graph_record_frontier.rs`. |
| `anti-monotone`   | Closure property     | "Adding reports cannot expand closure"; T-12RPT / `view_closure_reports_antimonotone`. Synonym for *antitone* in this context. |
| `slash closure`   | Reverse-reachability fixed point | `Closure₀ = DirectOffenders`; `Closureᵢ₊₁ = Closureᵢ ∪ {v ∈ V : NeglectEdges(v) ∩ Closureᵢ ≠ ∅}`. See `slashing-threat-model.md §1` and T-11 / T-12. |
| `neglect graph`   | Directed evidence graph | The graph with vertices = validators, edges = `neglecter → offender` whenever the neglecter cited an invalid offender block without an accompanying slash. §08.2. |
| `delimiter-free`  | Key-encoding scheme  | A bytestring concatenation key without separator bytes, vulnerable to prefix-collision attacks. T-5DF; §05.4.3 record-key collision. |
| `canonical visible self-chain child` | Detector input | The lowest-sequence visible self-child block above `baseSeq` in the canonical traversal; T-9.7. |
| `T-Idem`          | Slash idempotence    | "A second slash on the same validator is a no-op." Alias for legacy `T-9`. See `PoSContract.v:117`. |
| `T-Auth`          | Auth-token correctness | Family of theorems: invalid-auth slash deploys are no-ops; valid-auth execution is equivalent to ordinary slash semantics. See `MainTheorem.v:218,223`. |
| `T-LivenessGap`   | Authorized-index liveness | The proposer derives slash candidates from the authorized invalid-block evidence index, not from `invalid_latest_messages`; one slash deploy per current offender epoch. `deploy_epoch_matches_target`; Bug #14 / §9.16. |

## 2.4 Sequence-number convention

A validator `v` produces a sequence of blocks `b₀, b₁, b₂, …`,
where `seq(bᵢ) = i`. In the Rocq model, sequence numbers are
1-indexed so that `seq(b) − 1 ≥ 0`. We refer to the *base sequence*
of an `EquivocationRecord` as `baseSeq := s − 1` where `s` is the
sequence number at which the equivocation was observed.

Notation note: in **prose** we use Unicode minus (`s − 1`); in
**code-fenced excerpts** we use ASCII (`s-1` or `sn-1`) so the
listings are mechanically transcribable. The two forms denote the
same quantity.

## 2.5 LTS labels

The slashing pipeline is realized as a labeled transition system
over the abstract state
`S = (D, I, E, B, A, Sl, C)` (DAG, Invalid set, EquivocationRecords,
BondMap, Active set, Slashed set, Coop vault balance). Labels are:

| Label                             | Meaning                                                                       |
|-----------------------------------|-------------------------------------------------------------------------------|
| `sign(v, s, b)`                   | Validator `v` signs block `b` at sequence `s`.                                |
| `detect(v, s)→ub`                 | The detector returns verdict `ub : InvalidBlock` for `(v, s)`.                |
| `record(v, s−1)`                  | An `EquivocationRecord` keyed by `(v, s−1)` is created or updated.            |
| `propose(p, [SlashDeploy(b, …)])` | Proposer `p` emits a block carrying `SlashDeploy` system deploys.             |
| `executeSlash(v, ok)`             | The PoS contract executes the slash transition for `v`; returns success bool. |
| `filterFC(v)`                     | Fork-choice excludes `v`'s latest message from the GHOST estimator.           |
| `report(v, w)`                    | Validator `v` acknowledges (reports) `w`'s evidence; removes the corresponding neglect edge `v → w` from the active evidence graph. Threat-model §1. |
| `bond(v, s)`                      | Validator `v` bonds at sequence `s`; transitions `Unbonded → Bonded`.         |
| `unbond(v, s)`                    | Validator `v` initiates unbonding at sequence `s`; bond enters quarantine.    |
| `withdraw(v)`                     | Post-quarantine withdrawal; transfers the unbonded stake back to `v`'s vault. Bug #10 / §9.12. |
| `gossip(b)`                       | Block `b` is propagated to peers; honest validators eventually receive every gossiped block (gossip-fairness assumption). |

## 2.6 InvalidBlock taxonomy (and the slashable subset)

The `InvalidBlock` enum has **26** variants. **17** are *slashable*
in the current Rust source pre-fix; **18** are slashable post-fix
(bug #1 promotes `IgnorableEquivocation`). The slashable set:

```
AdmissibleEquivocation,    NeglectedEquivocation,    NeglectedInvalidBlock,
JustificationRegression,   InvalidParents,           InvalidFollows,
InvalidBlockNumber,        InvalidSequenceNumber,    InvalidShardId,
InvalidRepeatDeploy,       DeployNotSigned,          InvalidTransaction,
InvalidBondsCache,         InvalidBlockHash,         ContainsExpiredDeploy,
ContainsTimeExpiredDeploy, ContainsFutureDeploy
                                                    [+ post-fix: IgnorableEquivocation]
```

The remaining 9 variants — `IgnorableEquivocation` (pre-fix only),
`InvalidFormat`, `InvalidSignature`, `InvalidSender`,
`InvalidVersion`, `InvalidTimestamp`, `InvalidRejectedDeploy`,
`NotOfInterest`, `LowDeployCost` — are **non-slashable** because
they are *unattributable* (e.g. the block has no signature, so the
sender cannot be identified) or *cosmetic* (e.g. the deploy was
priced below the floor; rejecting it does not prove malice).

The "equivocation-class" variants are `AdmissibleEquivocation`,
`NeglectedEquivocation`, and (post-fix) `IgnorableEquivocation`.
The remaining 15 slashable variants are the **non-equivocation
slashable variants** referenced in §09 and bug-fix #3.

## 2.7 Theorem-naming convention

| Class                   | Form                   | Examples                                                                                      |
|-------------------------|------------------------|-----------------------------------------------------------------------------------------------|
| Headline theorems       | `T-N`                  | T-1 (detection soundness), T-3, T-7, T-8, T-10, T-11.                                         |
| Headline split labels   | `T-Na`, `T-Nb`, `T-Nc` | T-13a / T-13b / T-13c (bonds, records, fork-choice).  T-15a / T-15b (pipeline, composition).  |
| Named headline theorems | Mnemonic               | **T-Idem** (slash idempotence; alias T-9), **T-AuthCheck** (auth-token guard modeled by Rocq/TLA+ auth oracle). |
| Bug-fix theorems        | `T-9.M`                | T-9.1 through T-9.15.                                                                         |
| Letter-suffix theorems  | `T-N<suffix>`          | T-12C, T-12I, T-12F, T-12G, T-12A, T-12V, T-12RPT, T-12EID, T-12HYP, T-12AMP, T-12PF, T-12W, T-12R, T-12D, T-12RET, T-5DF, T-15D. See the mnemonic table below. |

`T-Idem` was historically named `T-9` but was renamed to avoid
collision with the bug-fix family `T-9.M`. The alias is preserved
in older artifacts.

### 2.7.1 T-12 / T-N letter-suffix mnemonics

The T-12 family encodes the various properties of the slash-closure
operator; the letter suffix is a one-to-three-letter mnemonic. Read a
suffix as "T-12 *of-this-property*". The table maps each suffix to
its mnemonic and the Rocq lemma (or family of lemmas) that discharges
it.

| Suffix     | Mnemonic                                            | Rocq lemma (representative)                          |
|------------|-----------------------------------------------------|------------------------------------------------------|
| **C**      | **C**losure-depth bound                              | `t_12_closure_depth_bound`                           |
| **I**      | **I**nitial-graph monotone                           | `t_12_initial_graph_monotone` / `slash_iter_initial_graph_monotone` |
| **F**      | **F**ixed-point at universe bound                    | `slash_iter_fixed_point_after_universe_bound`        |
| **G**      | **G**raph equivalence (set ↔ list view)              | `t_12_graph_equivalence`                             |
| **A**      | **A**nti-monotone reports (synonym for antitone)     | `view_closure_reports_antimonotone`                  |
| **V**      | **V**iew-merge over-approximation                    | `graph_union_closure_overapproximates_*`             |
| **RPT**    | **R**e**P**or**T** namespace exactness               | `unreported_visible_edge_remains_active`             |
| **EID**    | **E**poch / **ID**entity filter                      | `Inv_StaleEvidenceCannotSlashRebondedKey`            |
| **HYP**    | **HYP**othesis-derived corpus check                  | (Sage Finding 91 + Rust Hypothesis corpus)           |
| **AMP**    | **AMP**lification boundary                           | `weighted_amplification_boundary`                    |
| **PF**     | **P**roposer **F**airness                            | `proposer_fairness_boundary`                         |
| **W**      | **W**eighted (stake) closure / amplification          | `weighted_closure_model` family                      |
| **R**      | **R**eport growth antitone (≡ **A**; pick one)        | `view_closure_reports_antimonotone`                  |
| **D**      | **D**AG-level analog                                 | `t_9_6_dag` / DAG-level T-12 variants                |
| **RET**    | **RET**ention boundary (temporal)                     | `Inv_TemporalRetentionBoundary` family               |
| **5DF**    | **D**elimiter-**F**ree record-key injectivity (T-**5** sub-suffix) | `Inv_CanonicalRecordKeyInjective`                    |
| **15D**    | T-**15** **D**eltas (mod clause)                      | (modulo clause; see §10 of spec)                     |

**Conventional readings** worth remembering:

- "T-12W" reads "T-12 weighted closure": the weighted-stake analogue
  of the count-weighted quorum-preservation theorem.
- "T-12V" reads "T-12 view-merge": merged local views over-approximate
  each input view.
- "T-12RET" reads "T-12 retention": evidence outside the window does
  not authorize a slash.
- "T-AuthCheck" reads "T-Auth correctness" — see `T-Auth` in §2.3.

## 2.8 Rocq ↔ TLA+ ↔ Rust crosswalk (most-cited)

| Concept             | Rocq                                      | TLA+                       | Rust                                             |
|---------------------|-------------------------------------------|----------------------------|--------------------------------------------------|
| Validator           | `Validator := nat`                        | `Validators` set           | `Validator: ByteString`                          |
| Block               | `Block` (record)                          | implicit in `blocks[v][s]` | `BlockMessage` (proto)                           |
| InvalidBlock        | `InvalidBlock` (inductive, 26 ctors)      | string variant             | `InvalidBlock` (Rust enum, in `block_status.rs`) |
| EquivocationRecord  | `EqRec`                                   | `equivocationRecords` set  | `EquivocationRecord` (Rust struct)               |
| BondMap             | `BondMap := list (V * nat)`               | `bonds` function           | `BondsMap: HashMap<Validator, i64>`              |
| `is_slashable(s)`   | `is_slashable : InvalidBlock -> bool`     | (abstracted; not modeled)  | `InvalidBlock::is_slashable`                     |
| Detect equivocation | `equivocates : DAGState → V → nat → Prop` | `IsRealEquivocation(v,s)`  | `check_equivocations`                            |
| Slash effect        | `slash : PoSState → V → PoSState × bool`  | `ExecuteSlash(o)`          | `@PoS!("slash", …)`                              |

## 2.9 Cross-document conventions

- A reference like "**§09 (bug #3)**" means: see this design
  document, file `09-bug-fixes-and-rationale.md`, the entry for bug
  #3.
- A reference like "**spec §10.3**" means: see
  [`../slashing-specification.md`](../slashing-specification.md),
  section 10.3.
- A reference like "**verification §8.4**" means: see
  [`../slashing-verification.md`](../slashing-verification.md),
  section 8.4. (The shorthand "verif" appears in some places; both
  refer to the same document.)
- Citations like **[BG19]** are resolved in §13 with DOIs.

---

**Next:** [§03 — Architecture](03-architecture.md)
