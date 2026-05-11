# 01 · STRIDE and attack trees

> *“The most important thing about security is what you didn't think
> of.”* — Bruce Schneier, *Beyond Fear*, 2003 [Sch03].

This chapter explains the methodology's threat-modeling layer.
Correctness verification (the previous chapters) answers *“does the
system do what it's supposed to do?”*; threat modeling answers
*“what is the adversary trying to do?”* and *“which correctness
violations are exploitable, and how?”*.

The slashing methodology uses **STRIDE** [HL06] as the
threat-taxonomy framework and **attack trees** [Sch99] as the
decomposition framework. Both are documented in detail in
[`../../slashing-threat-model.md`](../../slashing-threat-model.md);
this chapter is the pedagogical companion that explains *how* to
use them.

Organization:

- [§1 — STRIDE taxonomy applied to slashing](#1--stride-taxonomy-applied-to-slashing)
- [§2 — Attack trees — building blocks](#2--attack-trees--building-blocks)
- [§3 — Pseudocode for attack-tree construction](#3--pseudocode-for-attack-tree-construction)
- [§4 — From attack tree to test target](#4--from-attack-tree-to-test-target)
- [§5 — Pitfalls](#5--pitfalls)
- [§6 — Related work](#6--related-work)

---

## 1 · STRIDE taxonomy applied to slashing

STRIDE [HL06] decomposes threats into six categories:

| Letter | Class                  | Slashing-domain meaning                                                                                  |
|--------|------------------------|----------------------------------------------------------------------------------------------------------|
| **S**  | Spoofing               | Impersonating a validator's signing key; forging a SlashDeploy by another validator                       |
| **T**  | Tampering              | Modifying a recorded EquivocationRecord; altering bond amounts outside protocol transitions               |
| **R**  | Repudiation            | Claiming a validator did not produce a block they did produce                                              |
| **I**  | Info disclosure        | Leaking validator identities or stake amounts through side channels                                       |
| **D**  | Denial of service      | Preventing a slash from being recorded; saturating the detector with malformed inputs                      |
| **E**  | Elevation of privilege | Causing a SlashDeploy from a non-system source to be accepted as a system deploy                          |

Three of these (S, T, E) are *direct* slashing risks; the others
(R, I, D) become slashing risks through the consensus layer (e.g.
DoS can prevent neglect detection, which is itself a slashable
offense).

### 1.1 The STRIDE-per-component pass

The methodology's pedagogical STRIDE pass walks every component in
the slashing topology
([`../../diagrams/01-component-overview.svg`](../../diagrams/01-component-overview.svg))
and asks one question per category:

```
algorithm stride_pass(components : List(Component)) → ThreatList:
    let threats ← []
    for each c in components:
        for each cat in {S, T, R, I, D, E}:
            let question ← stride_question_for(c, cat)
            let answer   ← engineer_response(question)
            if answer = "yes, this is a credible threat":
                threats.append((c, cat, answer.description, answer.severity))
    return threats
```

The full output is the table in
[`../../slashing-threat-model.md §2.1`](../../slashing-threat-model.md);
the same threats reappear as test targets in
[`../../design/14-test-plan.md`](../../design/14-test-plan.md) and as
attack-tree leaves in [§2](#2--attack-trees--building-blocks).

### 1.2 Example: STRIDE on the `EquivocationsTracker`

| Cat | Threat                                                              | Coverage                                                                |
|-----|---------------------------------------------------------------------|-------------------------------------------------------------------------|
| S   | A non-validator inserts a record                                    | Insertion gated on system-deploy auth; covered by `prop_t_auth_check`   |
| T   | Records are overwritten concurrently                                 | Bug #2; covered by Loom + TLA⁺ `ConcurrentTracker.tla`                  |
| R   | A validator denies its equivocation                                  | Records are signed by the recording validator; non-repudiable           |
| I   | Tracker leaks which validators have been observed                    | Out of scope — tracker state is public on the DAG                       |
| D   | Tracker insertion hangs under load                                   | Bounded-time insertion proven in `prop_t_4_record_uniqueness`           |
| E   | A non-system deploy modifies the tracker                              | Tracker mutation gated by `system_deploy_data` discriminator             |

Every cell either resolves to a proven property (with citation), a
covered test, or an out-of-scope clause. The methodology forbids
empty cells.

---

## 2 · Attack trees — building blocks

An attack tree [Sch99] decomposes an adversarial **goal** into the
**means** by which it might be achieved. The root is the goal; the
leaves are atomic adversarial capabilities; internal nodes are
combinators (AND, OR, sequential AND).

### 2.1 The four root goals

The slashing methodology defines four root goals an adversary might
pursue:

```
Goal G₁: slash an honest validator (𝖡ₛ)
Goal G₂: prevent slashing of an equivocator (𝖡𝖼)
Goal G₃: halt the slashing pipeline (𝖡ℓ)
Goal G₄: cause a slash without controlling the validator's key (𝖡ₐ)
```

(Symbols `𝖡ₛ, 𝖡𝖼, 𝖡ℓ, 𝖡ₐ` are defined in
[`../01-philosophy.md §1`](../01-philosophy.md).)

### 2.2 Anatomy of an attack tree for G₂

```
Goal G₂: prevent slashing of an equivocator
        │
        ├── OR: prevent detection
        │       ├── AND: control validator's view to omit the equivocation
        │       │       ├── partition the network (DoS)
        │       │       └── force the proposer to receive only the honest block
        │       └── exploit detector partiality (Bug #11) ← FIXED
        │
        ├── OR: prevent record creation
        │       ├── AND: race the lock-free tracker (Bug #2) ← FIXED
        │       └── AND: exhaust storage (DoS)
        │
        ├── OR: prevent SlashDeploy proposing
        │       ├── AND: control the next K proposers via censorship
        │       └── AND: cause proposer's `prepare_slashing_deploys` to fail
        │               └── feed unbonded proposer (Bug #8) ← FIXED
        │
        ├── OR: prevent SlashDeploy receipt
        │       ├── AND: cause receiver to reject the SlashDeploy
        │       │       ├── invalid epoch (Bug #15) ← FIXED
        │       │       ├── stale evidence rebond (Bug #13) ← FIXED
        │       │       └── unauthorized received slash (Bug #12) ← FIXED
        │       └── AND: race the receive path
        │
        └── OR: prevent PoS effect
                ├── AND: PoS transfer fails silently (Bug #4, Bug #10) ← FIXED
                └── AND: PoS Rholang contract DoS
```

Each leaf is either a known-and-fixed bug (with citation) or an
open threat that the threat coverage matrix in
[`../../slashing-threat-model.md §3`](../../slashing-threat-model.md)
maps to a defensive mechanism.

### 2.3 Why the tree is OR-dominant

Most internal nodes are OR (the adversary needs to succeed at
*one* of the children). This is the **classical asymmetry** between
defender and attacker [Sch99]: the defender must close *every*
branch, but the attacker needs only one. This asymmetry is the
reason a single missing test (e.g. for unbonded-proposer
`prepare_slashing_deploys`) can defeat an otherwise comprehensive
defense.

The methodology's response is to **enumerate every leaf** in
the tree, assign a coverage artifact to each, and verify the
artifact is non-trivial. This is the
[`../../slashing-threat-model.md §3 — Threat Coverage Matrix`](../../slashing-threat-model.md).

---

## 3 · Pseudocode for attack-tree construction

The methodology's attack-tree construction loop is:

```
algorithm build_attack_tree(goal : AdversaryGoal) → AttackTree:
    let tree ← Tree(root = goal)
    let queue ← [goal]
    while queue not empty:
        let node ← pop(queue)
        let children ← brainstorm_means(node)
        for each child in children:
            tree.add_edge(node, child)
            if child is atomic_capability(adversary):
                tree.mark_leaf(child)
            else:
                queue.append(child)
    return tree

algorithm brainstorm_means(node : AttackTreeNode) → List(AttackTreeNode):
    ▸ ask: what are the *means* by which an adversary could achieve `node`?
    ▸ structure each means as either a single capability (leaf)
      or a conjunction / disjunction of sub-goals
    ▸ output each means as a candidate child node
```

The `brainstorm_means` step is the **creative** part of the process.
The methodology pairs the engineer with the threat model
([`../../slashing-threat-model.md`](../../slashing-threat-model.md))
to ensure no STRIDE category is silently omitted.

### 3.1 Stopping condition

A leaf is **atomic** when it represents a single adversary
capability that cannot meaningfully be decomposed further. The
methodology's leaves are:

| Atomic capability                                  | Adversary's tool                                            |
|----------------------------------------------------|-------------------------------------------------------------|
| Equivocate at a particular `(v, seq)`              | Sign two blocks at `seq` (key control of `v`)               |
| Withhold a block from a subset of validators       | Network-layer partition or censorship                       |
| Delay gossip past a deadline                       | Network-layer delay                                          |
| Replay an old SlashDeploy                          | Re-broadcast a previously-seen system deploy                 |
| Construct a malformed BlockMessage                 | Bit-level manipulation of the wire format                    |
| Drive the detector through a particular DAG shape  | Choice of justifications when proposing                      |
| Cause an arithmetic overflow                       | Choice of seq number / epoch / bond at boundary             |

These leaves are the **inputs** to the structured fuzzing layer (see
[`../randomized-search/03-coverage-guided-fuzzing.md`](../randomized-search/03-coverage-guided-fuzzing.md))
and to the adversarial-search Sage models (see
[`02-adversarial-search.md`](./02-adversarial-search.md)).

---

## 4 · From attack tree to test target

Every leaf in the attack tree becomes a test target. The pipeline is:

```
algorithm leaves_to_targets(tree : AttackTree) → TestSpec:
    let targets ← []
    for each leaf in tree.leaves:
        let coverage ← lookup_coverage(leaf, design/14-test-plan.md)
        if coverage = ∅:
            (* leaf is uncovered — methodology forbids this state *)
            raise UncoveredLeafError(leaf)
        targets.append((leaf, coverage))
    return TestSpec(targets)
```

The lookup table is the threat coverage matrix in
[`../../slashing-threat-model.md §3`](../../slashing-threat-model.md);
every leaf maps to one or more of:

- A Rocq theorem.
- A TLA⁺ invariant.
- A Sage finding (FINDINGS.md entry).
- A Hypothesis state machine.
- A proptest property.
- A use-case test.
- A pre-fix bug regression.
- A Kani harness.
- A fuzz target.
- A documented out-of-scope clause.

The methodology's invariant is: **every leaf in the tree is covered
by at least one of these artifacts**, and the coverage is documented
in the matrix.

### 4.1 The methodology's coverage rule

> A leaf without coverage is a **defect in the methodology**, not in
> the code. The leaf must either be covered or moved to the explicit
> out-of-scope list.

This rule is what makes the threat model **complete by construction**;
it does not prove the system is secure, but it makes the auditor's
question *“what about attack X?”* answerable in O(1) time.

---

## 5 · Pitfalls

### 5.1 Pitfall: STRIDE-by-rote without context

A STRIDE pass that mechanically asks the six questions for every
component without considering context yields uninformative threats
(e.g. spoofing on a component that has no signing key).

**Mitigation**: every STRIDE pass in this development is preceded by
a 1-paragraph "context note" for each component describing what
secrets / state / privileges it holds. The questions are then
parameterized by the context.

### 5.2 Pitfall: attack tree explosion

An attack tree built without an atomicity criterion grows
exponentially. The slashing tree at `../../slashing-threat-model.md
§2.2` is deliberately pruned at ≤ 4 levels.

**Mitigation**: the atomicity criterion is *“can the engineer
imagine a test target for this leaf?”* — if yes, the leaf is
atomic; if no, the leaf is decomposed further.

### 5.3 Pitfall: defenses outside the model

A defense that lives outside the modeled subsystem (e.g. operator
discipline, network-layer rate limiting) is not testable here. The
methodology lists such defenses but does not credit them as primary
coverage.

**Mitigation**: see
[`../../slashing-threat-model.md §6 — Residual Boundaries`](../../slashing-threat-model.md);
out-of-scope defenses are documented but not relied on for the
threat coverage matrix.

---

## 6 · Related work

- **STRIDE**: Howard & LeBlanc [HL06].
- **Attack trees**: Schneier [Sch99].
- **Threat modeling as a process**: Shostack [Sho14].
- **MITRE ATT&CK** (an industrial threat-knowledge base): Strom *et
  al.* [Str18].
- **Blockchain-specific threat models**: Atzei *et al.* [ABC17].

DOIs in [`../references.md`](../references.md).

---

## 7 · Next chapter

[`02-adversarial-search.md`](./02-adversarial-search.md) — once the
attack tree is built, the next question is *how to search the
adversary's strategy space efficiently*. Objective-guided search
(damage optimizers, deep-threat sweeps) is the answer.
