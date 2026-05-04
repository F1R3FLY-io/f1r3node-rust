# 01 · Introduction & Motivation

## 1.1 What is slashing?

In a proof-of-stake (PoS) blockchain, validators put up **collateral**
(a *bond*) to participate in block production. The protocol can
*seize* that collateral when a validator misbehaves in a way that
is **cryptographically attributable** — i.e., when there is
on-chain evidence that the validator deviated from the protocol.
This punitive seizure is called **slashing**.

Slashing is the *economic-security* linchpin of a PoS protocol. The
purely-cryptographic checks (signatures, hashes, Merkle proofs) tell
you *what happened*; slashing translates those facts into a
*disincentive*: misbehavior reduces the misbehaver's stake to zero
and removes them from the active validator set.

> **Why call it "slashing"?** The metaphor is an axe falling on the
> bond. Once slashed, the validator's deposit is forfeited (typically
> transferred to a community-controlled vault) and the validator can
> no longer influence consensus until they post a fresh bond.

## 1.2 The threat model

The slashing subsystem defends against a *Byzantine* validator —
one that may sign arbitrary messages, collude with other validators,
withhold messages, or replay old messages. We assume:

| Capability of the attacker                                        | Modeled?                                                   | Section  |
|-------------------------------------------------------------------|------------------------------------------------------------|----------|
| Sign two distinct blocks at the same sequence number              | ✓ (equivocation)                                           | §04, §11 |
| Cite an invalid block as a justification *without* slashing it    | ✓ (Level-2 / neglect)                                      | §08      |
| Race two slashing inserts on different threads                    | ✓ (lock-free regression, bug #2)                           | §05, §09 |
| Send a malformed `SlashDeploy` claiming to be the system          | ✓ (auth-token guard, T-AuthCheck)                          | §06      |
| Replay an old `SlashDeploy` to slash twice                        | ✓ (slash idempotence, T-Idem)                              | §06, §11 |
| Stuff blocks with future / expired / repeat / fee-evading deploys | ✓ (15 non-equivocation slashable variants, §10.3 / bug #3) | §04, §09 |
| Skip sequence numbers under partition recovery                    | ✓ (off-by-one density, bug #7)                             | §09      |
| Self-equivocate without two distinct hashes (LMD inconsistency)   | ✓ (self-regression, bug #6)                                | §09      |

We **do not** model attacks below the consensus layer (e.g.
gossip-layer Sybil, network partitions outside the BFT bound, key
extraction from a compromised host). Those are scoped to the
networking and operational layers, respectively, and appear in the
specification's §13 scope-boundary table.

## 1.3 Design goals

The slashing subsystem must satisfy the following goals, in priority
order:

1. **Soundness.** No honest validator is ever slashed. (Theorem T-1
   in §02; *no false positives*.)
2. **Completeness.** Every detectable Byzantine action is *eventually*
   slashed. (Theorem T-2 + T-3; *no false negatives*.)
3. **Atomicity.** Concurrent detections do not lose evidence. (Bug
   fix #2 / T-9.2; *no lost hashes under thread interleaving*.)
4. **Determinism.** Replay against the same DAG produces the same
   slashing outcomes on every node. (Bisimilarity claim T-15; *no
   non-determinism in the consensus-critical path*.)
5. **Liveness.** The slash transition reaches a finite-time
   conclusion (success *or* documented error) on every input. (Bug
   fix #4 / T-9.4; *no hung deploys*.)
6. **Mutual-destruction.** A validator that *neglects to slash* a
   known equivocator is itself slashed. (Two-level closure / T-11,
   T-12; *collusion is mutually destructive*.)
7. **BFT-quorum preservation.** Under the standard BFT precondition
   `|equivocators| ≤ ⌊(n−1)/3⌋`, the slash closure preserves quorum.
   (T-12 corollary, see [LSP82] and §08.)

These goals are formalized as theorems in
[`../slashing-specification.md`](../slashing-specification.md) §4–§9
and proven in
[`../slashing-verification.md`](../slashing-verification.md).

## 1.4 Why a multi-document treatment?

The slashing subsystem is mechanized at *three* levels of abstraction:

```
   ┌────────────────────────────────────────────────────────────┐
   │  Pedagogical design        ← THIS DOCUMENT                 │
   │  (intuition, walks through, motivation, tradeoffs)         │
   └────────────────────────────┬───────────────────────────────┘
                                │
   ┌────────────────────────────▼───────────────────────────────┐
   │  Normative specification   ← slashing-specification.md     │
   │  (LTS, theorem statements, bug-fix manifest, use cases)    │
   └────────────────────────────┬───────────────────────────────┘
                                │
   ┌────────────────────────────▼───────────────────────────────┐
   │  Mechanized verification   ← slashing-verification.md +    │
   │  (Rocq proofs, TLA+ models)   formal/rocq/, formal/tlaplus/│
   └────────────────────────────────────────────────────────────┘
```

Each layer answers a different question:

- The **design document** answers: *What is this? Why was it built?
  How do the pieces fit?*
- The **specification** answers: *What exactly are the formal
  properties? What does each operation do?*
- The **verification** answers: *Why are the properties true? What
  is the proof? Where does Rocq sign off?*

Reading any one in isolation is possible; reading all three in
sequence is recommended for full understanding.

## 1.5 Related systems

| System                      | Slashing model                                                                                                                      | Reference                           |
|-----------------------------|-------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------|
| Ethereum 2.0 (Beacon Chain) | FFG slashing conditions: surround votes, double votes; `slash_validator` zeros effective balance and applies a correlation penalty. | [BG19], [ETH-SPEC]                  |
| Cosmos SDK / Tendermint     | Evidence module: light-client attacks (lunatic, equivocation, amnesia); per-validator slashing percentage.                          | [BKM18], [BBKMW20], [COSMOS-ADR009] |
| F1R3FLY (this work)         | CBC-Casper-style; equivocation + neglected-equivocation + 15 non-equivocation slashable variants; one-strike (100% bond seizure).   | This document                       |

The F1R3FLY model is closest to Ethereum FFG in spirit (slash whole
bond; remove from active set) but differs in granularity: F1R3FLY
slashes for *every* `is_slashable() = ⊤` invalid-block variant
once bug fix #3 is in place, whereas FFG slashes only for the two
double-voting and surround-vote conditions. The F1R3FLY model also
formalizes a **two-level closure** (neglecting an equivocator is
itself slashable) which Ethereum FFG does not have.

## 1.6 What this document is *not*

- **Not a tutorial.** We assume you already know what a blockchain
  is, what BFT consensus is in broad strokes, and how PoS validators
  earn rewards.
- **Not a proof.** Proofs live in `formal/rocq/slashing/theories/*.v`
  and are summarized in `../slashing-verification.md`. This document
  cites them but does not reproduce them.
- **Not an operations manual.** Operating a validator (key generation,
  bond posting, monitoring) is a separate concern handled by
  `node/` and the operations runbooks under
  [`docs/run-local/`](../../../run-local/).
- **Not a specification.** When the spec and this design document
  disagree, the spec is authoritative. (We make every effort to keep
  them in sync; if you find a discrepancy, file an issue.)

## 1.7 What follows

The next document, [§02 Glossary & notation](02-glossary-and-notation.md),
introduces every symbol, acronym, and term used in the rest of the
design — *before* we use them. Then §03 lays out the architecture
(five layers, thirteen sub-components, dependencies). After that,
each remaining section deep-dives into one layer or topic.

---

**Next:** [§02 — Glossary & notation](02-glossary-and-notation.md)
