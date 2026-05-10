# 04 · Detection & Pipeline

## 4.1 The pipeline at a glance

A slashing event is the composition of six per-component transitions:

```
sign(v, s, b)          ⟶ DAG += b                  — validator emits a block
detect(v, s) ⟶ ub      ⟶ verdict ub : InvalidBlock — detector classifies
record(v, s-1)         ⟶ E += (v, s-1, witnesses)  — tracker stores evidence
propose(p, [SlashDeploy(b, …)]) ⟶ block + system deploy — proposer attaches slash
executeSlash(v, true)  ⟶ BondMap[v] := 0           — PoS contract fires
filterFC(v)            ⟶ fork choice excludes v    — GHOST stops counting v
```

Each step is the topic of a later section:

| Step           | What it does                 | Section                                |
|----------------|------------------------------|----------------------------------------|
| `sign`         | Validator emits a block.     | (out of scope; consensus)              |
| `detect`       | Verdict-emitting predicates. | §04 (this file)                        |
| `record`       | Tracker-store insertion.     | [§05](05-storage-and-records.md)       |
| `propose`      | `SlashDeploy` assembly.      | [§06](06-proposing-and-effect.md)      |
| `executeSlash` | PoS contract execution.      | [§06](06-proposing-and-effect.md)      |
| `filterFC`     | Fork-choice exclusion.       | [§07](07-fork-choice-and-lifecycle.md) |

This document covers `detect` in depth and explains how the other
steps compose into a single end-to-end pipeline (Diagram 02).

## 4.2 The detection function — formal

### Definition 4.1 (Equivocation in the DAG)

Validator `v` *equivocates* at sequence number `s` in DAG state
`D` iff there exist two distinct blocks `b₁, b₂ ∈ D` with
`sender(bᵢ) = v`, `seq(bᵢ) = s`, and `hash(b₁) ≠ hash(b₂)`.

We write `equivocates(D, v, s) : Prop` for the predicate. Rocq:
`DAGState.v:106` (`equivocates`); the boolean counterpart
`equivocates_b` (`DAGState.v:99`) is proven equivalent at
`equivocates_dec` (line 109).

### Definition 4.2 (Requested as dependency)

A block `b` is *requested as a dependency* in `D` iff some other
block `b' ∈ D` has `b.hash` in its justifications. We write
`requestedAsDep(D, b)`.

The Rocq mechanization passes `d := requestedAsDep(D, b)` as a
boolean parameter to `check_equivocations` rather than recomputing
it (the upstream block-processor knows the flag and forwards it).

### Definition 4.3 (Detection rules)

Given DAG state `S` and an arriving block `b` with `sender(b) = v`,
`seq(b) = s`:

```
detect(S, b) =
  | not equivocates(S, v, s)               ⟹ Valid
  | requestedAsDep(S, b)                   ⟹ AdmissibleEquivocation
  | otherwise                              ⟹ IgnorableEquivocation

detectNeglected(S, b) =
  | (v, s-1) ∈ E ∧ detectableInView(S, b, v, s-1)
      ∧ bondedIn(b, v)                     ⟹ NeglectedEquivocation
  | otherwise                              ⟹ unchanged
```

### Theorems

The detector is **sound** and **complete**:

- **T-1 (Detection soundness).** *(`detection_sound`,
  `EquivocationDetector.v:91`.)* For every state, validator,
  sequence number, and block, if `detect` returns
  `AdmissibleEquivocation`, then `equivocates` holds.
- **T-2 (Detection completeness).** *(`detection_complete`,
  `EquivocationDetector.v:111`.)* If `equivocates(S, v, s)` and
  `b ∈ D` with `sender(b) = v`, `seq(b) = s`, then `detect`
  returns either `AdmissibleEquivocation` or `IgnorableEquivocation`.
- **T-3 (Slashable iff in slashable set).** *(`slashable_post_fix_extends_pre_fix`,
  `InvalidBlock.v:151`.)* Post-fix #1, the slashable set strictly
  includes the pre-fix slashable set (by adding
  `IgnorableEquivocation`).
- **T-6 (Neglect detection sound + complete).** *(`detect_neglected_sound`,
  `EquivocationDetector.v` §4.5; `detect_neglected_complete` §4.6.)*
  Verdict `NeglectedEquivocation` fires iff an existing
  `EquivocationRecord` is detectable from `b`'s latest-message
  justification view, the recorded offender remains bonded in `b`'s
  bonds cache, and the block has not already acknowledged the offender
  by removing/slashing it. A direct citation to one offending block is
  only a special case of the Rust `is_equivocation_detectable` search;
  the production rule can also use detected hashes and nested
  latest-message pointers.

The fixed detector's latest-message contribution rule is:

```
function detectable_from_view(view, record):
    distinct_children ← ∅

    for each latest_message in deterministic_order(view):
        if latest_message.hash ∈ record.detected_hashes:
            return true

        contribution ← reachable_offender_child(latest_message, record)
        if contribution = missing_pointer:
            continue

        if contribution = child(h):
            distinct_children ← distinct_children ∪ {h}

        if |distinct_children| ≥ 2:
            return true

    return false
```

Mathematically:

```
detectable(view) ≜ detected_hash_seen(view) ∨ |distinct_child_hashes(view)| ≥ 2
```

This is the T-9.11 rule. It preserves the old verdict for complete
latest-message views while fixing two Rust-only bugs: missing pointers
are non-contributing rather than fatal, and duplicate paths to one child
count once.

## 4.3 The pipeline — sequence diagram

The end-to-end flow for an admissible equivocation:

[![Diagram 02 — Admissible equivocation slash flow](../diagrams/02-seq-admissible-equivocation.svg)](../diagrams/02-seq-admissible-equivocation.svg)

> **Reading the diagram.** Six phases:
> 1. Validator A signs block `b₁` honestly → admitted.
> 2. Validator A signs `b₁'` at the same seq → equivocation detected.
> 3. The orchestrator inserts an `EquivocationRecord(A, seqN − 1, ∅)`.
> 4. The next proposer P reads authorized current-epoch invalid-block evidence → emits `SlashDeploy(b₁', P, targetEpoch, …)`.
> 5. PoS Rholang executes the slash atomically → bond → 0.
> 6. The block is gossiped; ForkChoice re-reads the bonds map → A's latest message is filtered.

## 4.4 The detection algorithm — literate pseudocode

The detector is the `check_equivocations` function in
`equivocation_detector.rs:24-104`. Here is the algorithm in literate
style.

We begin by extracting what we need from the input block. The
*creator-justification* is the block's own previous block hash (i.e.,
the latest block by the same sender that this block claims to follow).

```
function check_equivocations(b: Block, requested_as_dep: bool, snapshot: CasperSnapshot)
                          → InvalidBlock ∪ {Valid}:
    creator_justification ← extract_creator_justification(b)
```

We look up the *latest message* this validator has emitted, as known
to the snapshot's DAG view. If our own creator-justification matches
the latest message, this block is honest.

```
    latest ← snapshot.dag.latest_message_hash(b.sender)
    if creator_justification = latest:
        return Valid
```

Otherwise, the validator's chain has *forked*: this block points to
some non-latest predecessor. That is exactly what an equivocation
looks like.

```
    if requested_as_dep:
        return AdmissibleEquivocation
    else:
        return IgnorableEquivocation
```

The verdict is `AdmissibleEquivocation` iff the equivocation was
*solicited* (some other block already cited this hash), and
`IgnorableEquivocation` otherwise. Pre-fix bug #1 (§09) drops
ignorable equivocations silently — a DOS vector. Post-fix, both
are recorded identically.

> **Why this distinction?** The `requested_as_dep` flag tells us
> whether the equivocation has *consequences* for other validators.
> If yes, the network has already absorbed the bad block (some
> validator cited it as a parent), so the equivocation is *committed*
> on the DAG. If no, the bad block arrived unsolicited and could
> in principle be quietly dropped — but doing so creates a DOS
> vector (the attacker can flood the network with no cost). Bug
> fix #1 closes this loophole.

## 4.5 The dispatcher — what happens after `detect`

The `MultiParentCasperImpl.handle_invalid_block` dispatcher (Rust:
`multi_parent_casper_impl.rs:1018-1112`) receives the verdict and
decides what to do. The relevant branches:

```
match verdict:
    Valid                          → DAG.insert(b, invalid = false)
    AdmissibleEquivocation         → tracker.insert_equivocation_record(v, s-1, ∅)
                                   → DAG.insert(b, invalid = true)
    IgnorableEquivocation          → log("did not add block")  (pre-fix)
                                   → tracker.insert_equivocation_record(...)  (post-fix #1)
    NeglectedEquivocation          → tracker.insert_equivocation_record(B, seqN_B-1, ∅)
                                   → DAG.insert(b_B, invalid = true)
    ib if is_slashable(ib)         → DAG.insert(b, invalid = true) (pre-fix)
                                   → tracker.insert+update_record (post-fix #3)
    other (non-slashable)          → DAG.insert(b, invalid = true)
```

[![Diagram 05 — Generic invalid-block dispatch (post-fix #3)](../diagrams/05-seq-invalid-block-dispatch-fixed.svg)](../diagrams/05-seq-invalid-block-dispatch-fixed.svg)

The post-fix dispatcher (after bug #3) routes every
`is_slashable() = ⊤` variant through the same record-creation path,
guaranteeing that *every* slashable invalid block enters the
slashing pipeline. Pre-fix, only `AdmissibleEquivocation` and
`IgnorableEquivocation` (when handled) reached the tracker; the
other 15 slashable variants were merely flagged invalid in the DAG
and would only get slashed if a future proposer happened to surface
the offender's invalid latest message.

## 4.6 Two-level detection: the neglected-equivocation path

Once `(A, baseSeq) ∈ E` (the tracker has a record for A), any
*future* block `b_B` whose latest-message view makes A's equivocation
detectable while A remains bonded is itself slashable unless the block
acknowledges/slashes A. A direct citation to A's invalid block is a
common test witness, but production Rust also accepts nested
latest-message evidence and previously detected hashes. This is the
**two-level** closure: B's neglect of A is itself a form of collusion,
and is itself slashed.

The data flow that powers neglect detection:

[![Diagram 08 — Justifications → neglect detection](../diagrams/08-dataflow-justifications-to-neglect.svg)](../diagrams/08-dataflow-justifications-to-neglect.svg)

The validate-time logic at `validate.rs:989-1030`:

```
for each justification j ∈ b_B.justifications:
    look up snapshot.dag[j.latestBlockHash]
    if that block is flagged invalid:
        invalidJustifications += j

neglected ← exists j ∈ invalidJustifications:
              snapshot.bonds_map[j.validator] > 0

has_slash ← scan b_B.body.system_deploys for SystemDeployData::Slash

reject ⟺ neglected ∧ ¬has_slash    -- post-fix #9
```

The post-fix `¬has_slash` clause is the *Rust widening* of bug #9
(§09): a block that *self-corrects* by attaching its own
`SlashDeploy` for the neglected justification's validator is
admitted. Scala rejects it; Rust admits it. This is the only
deliberate divergence in the bisimilarity claim.

## 4.7 What the pipeline gives you

Once the pipeline has fired end-to-end:

1. The offender's bond is 0 in the on-chain `state.allBonds`.
2. The offender is not in `state.activeValidators`.
3. The forfeited stake is in the Coop vault.
4. The offender's latest message is filtered from the fork-choice
   estimator on every subsequent fork-choice round.
5. The `EquivocationRecord(offender, baseSeq, witnesses)` remains in
   the tracker store as evidence (it is *never* removed; record
   monotonicity / T-4).

These five together compose the formal **slash effect** that
spec §5.2 defines and verification §6 proves.

---

**Next:** [§05 — Storage & records](05-storage-and-records.md)
