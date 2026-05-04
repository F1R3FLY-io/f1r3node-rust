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
| **GHOST** | Greedy Heaviest-Observed Sub-Tree | Fork-choice rule [LSZ15]; cf. **LMD-GHOST** (§07).               |
| **FFG** | Friendly Finality Gadget (Casper) | Ethereum 2.0 slashing comparison anchor (§01.5; [BG19]).          |
| **DOS** | Denial of Service                | The vector closed by bug fix #1 (§09.1, §10.1, §12.2.1).           |
| **KV**  | Key-Value                        | Store abstraction underlying the equivocation tracker (§05).       |
| **BFS** | Breadth-First Search             | Traversal algorithm in post-fix #7 (§09.7, T-9.7).                 |
| **TLA+** | Temporal Logic of Actions       | Lamport's specification language; checked by TLC (§10).            |
| **Rocq** | Proof assistant (formerly Coq)  | The mechanization target for proofs in `formal/rocq/`.             |

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
| `ilm`             | Invalid latest messages | The DAG-side index `dag.invalid_latest_messages` (§04, §06).                                                                                                                  |
| `Equivocation`    | Term of art         | A validator signing two distinct blocks at the same sequence number; formalized as Definition 4.1 in spec; cf. taxonomy §2.6.                                                   |

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
| Named headline theorems | Mnemonic               | **T-Idem** (slash idempotence; alias T-9), **T-AuthCheck** (auth-token guard; Rholang-level). |
| Bug-fix theorems        | `T-9.M`                | T-9.1 through T-9.9.                                                                          |

`T-Idem` was historically named `T-9` but was renamed to avoid
collision with the bug-fix family `T-9.M`. The alias is preserved
in older artifacts.

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
