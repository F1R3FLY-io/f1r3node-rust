# The Consensus-Change Register

### A classification and risk report on the consensus-changing data-model, wire-format, and acceptance changes of the `rho-native-semantics-review` campaign

| | |
|---|---|
| **Document class** | Engineering report — classification and risk analysis. **LIVING**: amended, never closed. |
| **Register anchor** | `7293d57c` (`f1r3node-rust-mettail`, branch `feature/mettail`) — the point at which the consensus surfaces were last reviewed as a set; the original derivation range was `7293d57c..dc383ed1` (78 consensus-path commits), extended by later landings named per entry. |
| **Inclusion criterion** | **May-change-consensus only** (owner ruling, 2026-08-03): data-model, wire-format, acceptance, ruled-semantic, and token-model metering changes. Bug fixes, optimizations, and equivalence-proven stack-safe conversions are retired to [Appendix B.1](#b1-retired-register-entries). |
| **Companion surface** | `mettail-rust`, branch `feature/rho-native-set-automata`. |
| **Companion reports** | the [stack-safety report](../design/stack-safety/stack-safety-report-2026-07-29.md) and the [PathMap report](../design/pathmap/pathmap-report-2026-08-03.md), which carry the equivalence evidence this register's retirements cite. |
| **Audience** | F1r3node consensus reviewers deciding whether to accept the fork risk of a coordinated protocol-version bump. |
| **Date** | 2026-07-29, re-scoped to the final criterion 2026-08-03 |
| **Maintenance** | [§7](#7-maintenance). Adding an entry is filling the form in [Appendix A](#appendix-a--the-entry-template). |

---

## Notation and abbreviations

★ **Read this table first if any short form below is unfamiliar.** Conceptual definitions —
*consensus-breaking*, *post-state hash*, *verdict*, *fork*, *Lane B* / *Lane P* — are given in full
in [§2](#2-background-and-definitions), which every axis table refers back to.

| Short form | Expansion | Where it matters here |
|---|---|---|
| **CBR** | *Consensus-Breaking Record* — the identifier prefix of every entry. `CBR-0NN` numbers a Surface-N entry; `CBR-LNN` a Surface-L one. Retired identifiers keep resolving via [Appendix B.1](#b1-retired-register-entries). | [§4](#4-the-register) |
| **SHA** | *Secure Hash Algorithm*; used throughout as the customary shorthand for a git commit object name. | the `Commit(s)` cell of every entry |
| **AST** | *abstract syntax tree* | the normalizer entries |
| **LMDB** | *Lightning Memory-Mapped Database* — the cold store's backing key-value store. | [§2.5](#25-the-two-wire-formats) |
| **URI** | *uniform resource identifier* — here a `rho:id:…` registry URI. | **CBR-030** |
| **PDA** | *pushdown automaton* — the explicit heap-backed machine used by generated stack-safe traversals. | **CBR-044** |
| **SCC** | *strongly connected component* — a maximal dependency-graph region whose members are mutually reachable. | retired **CBR-023** evidence |
| **EPM1** | *EPathMap format, version 1* — the versioned homogeneous PathMap trie snapshot used by both codecs. | **CBR-044** |
| **TRIE** | not an acronym — typographic emphasis for the prefix-compressed trie that is the EPathMap representation. | **CBR-041** |
| **D3 / DR-9 / OD-1 / OD-3** | the token cost model's design-record identifiers (`docs/theory/cost-accounting-impl/d3-replace-phlo-with-tokens.md`). | [§2.2](#22-language-and-runtime-vocabulary), [§3.3](#33-how-each-axis-value-was-established) |
| **COMM** | the produce/consume synchronisation event — the token model's consensus cost unit. | [§2.2](#22-language-and-runtime-vocabulary) |
| **FFI** | *foreign function interface* | §6.3 |
| **CI** | *continuous integration* | §6.3 |
| **FIPS** | *Foreign-language Interoperability Problem Statement* — the design-document series governing MeTTaIL's foreign-language terms (⚠ not the U.S. Federal Information Processing Standards). | **CBR-L07** authority |
| **E2E** | *end-to-end* | entry evidence blocks |
| **BLAKE2 / Blake2b256** | the BLAKE2 cryptographic hash family; `Blake2b256` is the event-hash and post-state instance. | [§2.3](#23-consensus-vocabulary) |
| **IEEESTD** | not an abbreviation — a path component of IEEE's DOI namespace, reproduced verbatim so the DOI resolves. | [References](#references) |
| **RSS** | *resident set size* — the capped physical-memory footprint of the measured runs. | entry evidence blocks |
| **TSV** | *tab-separated values* — the durable measurement files the evidence links. | **CBR-044** |
| **DFA** | *deterministic finite automaton* | **CBR-L14** lexer account |
| **RAII** | *resource acquisition is initialisation* — the scope-tied resource idiom named in one exemption row. | [Appendix B.2](#b2-the-original-commit-level-exemptions) |

⚠ **This document also uses ALL-CAPS as emphasis inside verbatim quotations of commit messages**;
such tokens (appearing in [Appendix B](#appendix-b--the-exemption-table)'s quoted commit subjects
and retired-row headlines) are ordinary English words set in capitals for stress, not acronyms. They are
enumerated so a reader does not search for an expansion that does not exist:

| Token | It is simply the word | Where |
|---|---|---|
| **UNSPELLABLE** | *unspellable* | this section; B.2 quoted subjects |
| **NEIGHBOURS** | *neighbours* | B.2 quoted subjects |
| **POSITIONALLY** | *positionally* | entry evidence quotations |
| **DISCHARGEABLE** | *dischargeable* | entry evidence quotations |
| **SUBTRACTIVELY** | *subtractively* | entry evidence quotations |
| **REGRESSIVELY** | *regressively* | **CBR-L08** acceptance cell |
| **UNREPAIRED** | *unrepaired* | the retired CBR-028 row's headline |

★ The closed vocabularies that *are* meaningful in capitals are defined in
[§2](#2-background-and-definitions): the axis verdicts `MOVES` / `NO` / `N/A`
([§4.2](#42-entry-template)), the directions ([§2.6](#26-direction-of-change)), the evidence grades
([§2.7](#27-evidence-grade--and-the-word-potentially)), the provenance tags **DERIVED** /
**MEASURED** / **CITED** / **UNVERIFIED** ([§2.8](#28-provenance-tags)), and the retirement and
exemption reasons ([§3.2](#32-inclusion-and-exclusion-criteria)).

---

## Abstract

The `rho-native-semantics-review` campaign landed 111 commits on the F1r3node consensus
implementation and a parallel body of work on MeTTaIL's second implementation of Rholang. This
report derives, from the git record, the subset of that work which **may actually change
consensus** — the inclusion criterion the owner fixed on 2026-08-03: data-model, wire-format,
acceptance, and ruled-semantic changes qualify; **bug fixes and optimizations are not
consensus-breaking for this register's purpose**, and stack-safe conversions proven equivalent to
their recursive counterparts definitely are not. Each qualifying change is classified against six
independent axes: computed value, verdict, serialized bytes (per lane), post-state hash, accepted
programs, and metering under the token cost model.

**Result: 19 entries** — 14 on the F1r3node node, 5 on MeTTaIL's Rholang; **18 landed, 1 in
flight**. The core is the EPathMap data-model lineage (CBR-011/012/013 — the trie ruling's stages —
and CBR-041/042/043 culminating in **CBR-044**, the EPM1 wire transition, on which six of the seven
axes move), one wire-schema addition (CBR-014), four ruled semantic/acceptance changes (CBR-002,
CBR-027 with its genesis partner CBR-030, CBR-037), the additive method surface (CBR-024/025), and
the Surface-L acceptance set (L07, L08 in flight, L10, L11, L14). **The metering axis was re-derived
under the D3 token model** (consensus cost = committed COMM count; per-op prices are diagnostics):
**no kept entry moves it**, and the register's one historical `UNVERIFIED` cell resolved in the same
derivation. **45 further changes were examined and retired** with typed reasons — 34 bug fixes, 3
measured-neutral optimizations, 5 equivalence-proven conversions, 2 dormant additions, and the
formerly-open wire-asymmetry hazard, closed against CBR-044 — each a one-line row in
[Appendix B.1](#b1-retired-register-entries) whose full historical body remains in git history.
21 commit-level exemptions from the original sweep are retained in
[Appendix B.2](#b2-the-original-commit-level-exemptions) as the negative results that make the
criterion checkable.

**The headline risk is not any single entry; it is their conjunction.** `Validate::version`
(`casper/src/rust/validate.rs:273`) compares block versions for **exact equality** against a
genesis-anchored constant. There is no activation-height machinery and no per-feature gate, so these
changes cannot be rolled out independently: they ship together, as one coordinated protocol-version
bump, or not at all. The owner has ruled the network **pre-production**, which is what licenses the
landed entries to sit unactivated; the bump itself belongs to F1r3node.

---

## 1. Introduction

### 1.1 The problem

A blockchain node is a deterministic function from a block and a pre-state to a post-state.
Consensus is the agreement of independent implementations, or independent builds of one
implementation, on that function. A change that moves the function's *observable* output for some
input can split the network — and the splits are not all of one kind: a refusal every validator
reaches identically is a detectable, slashable fault, while a silently different computed value is a
safety fork in which two honest nodes hold divergent, individually plausible histories.

This register carries the campaign's changes that **may actually change consensus** — deliberate
data-model, wire-format, acceptance, and ruled semantic transitions. Its criterion is the owner's
2026-08-03 ruling (§3.2): repairs that make a wrong answer right, optimizations measured to move
nothing, and stack-safe conversions with machine-checked equivalence evidence are *not* carried as
entries, because they do not represent consensus decisions a reviewer must weigh — they are retired
with typed reasons so the account stays checkable.

### 1.2 Contributions

1. A **six-axis classification** of consensus visibility (§2.4), separating properties a single
   phrase like "consensus-breaking" conflates, with the propagation between axes made explicit and
   the metering axis defined under the current token cost model.
2. An explicit account of the **two wire formats** a `Par` crosses (§2.5), including the field-order
   asymmetry that produced a measured, round-trip-invisible defect class.
3. A **derived** register (§3, §4): 19 entries, each with all six axes answered, a stated blast
   radius, a direction, an evidence grade, and — where one exists — the owner ruling that authorised
   it, quoted verbatim with its date.
4. The **negative results**: 45 retired entries with typed reasons (Appendix B.1) and 21 commit-level
   exemptions (Appendix B.2), which are what make the inclusion criterion checkable rather than
   merely asserted.

---

## 2. Background and definitions

A reviewer may not share this campaign's vocabulary. Every term and acronym used later is defined here
first.

### 2.1 The two implementations under discussion

| Surface | Repository / branch | Role |
|---|---|---|
| **N** — the node | `f1r3node-rust-mettail`, `feature/mettail` | **The** consensus implementation. Scala support is dropped; no Scala artifact is normative and none is cited in this report. A change here moves live consensus. |
| **L** — the language | `mettail-rust`, `feature/rho-native-set-automata` | A **second** implementation of Rholang (`languages/src/rholang.rs`), hosted in the MeTTaIL/PraTTaIL language workbench and intended to become Rholang 1.4. It does not run consensus today. A divergence here is a **future** fork, not a present one — but it is exactly the kind of divergence that is cheap to fix now and catastrophic to discover after adoption. |

The register carries both, with the surface as a column, because the standard the campaign works to is
that MeTTaIL's Rholang must be a **superset** of upstream Rholang: it must accept everything upstream
accepts and compute the same value, with divergence permitted only where upstream has a bug, and only
by explicit ruling.

### 2.2 Language and runtime vocabulary

- **Rholang** — the reflective, higher-order, concurrent process language the node executes, descended
  from the asynchronous polyadic $`\pi`$-calculus [Milner1992] by way of the reflective
  $`\rho`$-calculus [Meredith2005].
- **Deploy** — a signed unit of work: a source string with its signature and metadata, submitted to a
  node, *normalized* to a term, and reduced.
- **`Par`** — the term type. A Rholang process is a parallel composition of sends, receives, `new`
  bindings, `match`es, bundles, connectives, expressions and unforgeable names; `Par` is the protobuf
  message carrying all of them.
- **Redex** — a *reducible expression*: a subterm to which an operational rule applies.
- **Tuplespace / RSpace** — the shared, content-addressed store of resting *data* (produces) and resting
  *continuations* (consumes).
- **COMM event** — the synchronisation of a produce with a consume on the same channel, firing the
  consume's continuation. *Which* COMM fires, and with *which* data, is the central nondeterminism a
  consensus protocol must pin down.
- **Guard** (`where` clause) — a boolean side-condition on a receive or a `match` case, deciding whether
  a candidate COMM or branch is admissible.
- **Normalization** — the compile step from source to `Par`: de Bruijn indexing of binders,
  free-variable numbering, and the canonical sort. Free-variable numbering is consensus-visible: a
  mis-threaded normalizer emits a plausible term with wrong indices and **fails silently**.
- **Substitution** — replacing a bound variable by its value inside a term. ★ Its *result* is what gets
  serialized, and therefore what gets **signed**.
- **Spatial matching** — the relation deciding whether a pattern matches a target, and with what
  bindings. It decides COMM firing (via `Matcher::get`) and `match` branch selection.
- **`FreeMap`** — the matcher's accumulator: a map from a pattern's free-variable *level* to the term
  bound at it. Its final contents become the continuation's environment.
- **Token cost (D3)** — the consensus unit of computational cost. Under the token model
  (`docs/theory/cost-accounting-impl/d3-replace-phlo-with-tokens.md`, DR-9/OD-3; merged at
  `eec6e323`), **consensus consumed cost is the count of committed COMM events, exactly one token
  each** — `reconcile_lane` tallies 1 per committed `BillableKind::Comm` and 0 for every other kind
  (`rholang/src/rust/interpreter/accounting/mod.rs`, the `reconcile_lane` comment block). Per-op
  weights (`Primitive`/`Reduction`/`Substitution`) survive only as **diagnostics** in the event
  log/digest; `phlo_limit`/`phlo_price` are deleted from the wire; funding is the per-signature
  token supply via the acceptance gate; accepted user deploys run unmetered-for-liveness (OD-1).
  "Phlogiston" survives only as the historical name of the renewable resource. **Metering** in
  this register means the token-model surface: the committed COMM count and the
  funding/settlement machinery.
- **PathMap / `EPathMap`** — a trie-shaped Rholang collection; `EPathMap` is its `ExprInstance` arm.
  **`EZipper`** is a cursor into one.

### 2.3 Consensus vocabulary

- **Play** — the proposer's execution of a deploy while building a block.
- **Replay** — every other validator's re-execution of the same deploy against the block's recorded COMM
  trace, comparing the outcome.
- **Post-state hash**, written $`h_{\mathrm{post}}`$ — the Merkle root of the tuplespace *after* the
  block's deploys have run, committed in the block header. Two nodes computing different
  $`h_{\mathrm{post}}`$ for the same block hold different chains.
- **Fork** — used strictly here to mean a *safety* fork: honest nodes disagree about the canonical
  chain, and neither block is detectably invalid.
- **Slashable fault** — a replay disagreement that every honest validator reaches *identically*,
  producing `InvalidBlock::InvalidTransaction`, for which `is_slashable()`
  (`casper/src/rust/block_status.rs:183`) answers true. This is liveness-plus-penalty, **not** a silent
  safety fork. ⚠ It is not benign: the proposer skips checkpoint validation on its own block, so it
  alone keeps a block everyone else slashes it for. **CITED** (`80f5e5d3`).
- **Liveness split** — one node aborts the *process* where another completes. In Rust a stack overflow
  is a `SIGSEGV` on the guard page, **not** an `Err`: it is uncatchable, unreachable by metering
  (Casper installs `Cost::unsafe_max()` deliberately, for liveness), and its threshold depends on the
  thread's stack size. Two nodes with different stack budgets therefore disagree about whether a deploy
  is *executable at all*.
- **Coordinated version bump** — the only rollout mechanism available. `Validate::version`
  (`casper/src/rust/validate.rs:273`) is exact equality against a genesis-anchored constant. **DERIVED**
  (read at `casper/src/rust/validate.rs`, `pub fn version(b: &BlockMessage, version: i64) -> bool`).

### 2.4 Consensus-breaking, defined — and the six axes

> **Definition.** A change is **consensus-breaking** if there exists a deploy $`d`$ and a reachable
> pre-state $`S`$ such that a node running the new code and a node running the old code, both honest
> and both given $`(d, S)`$, disagree on at least one of: the value computed, the COMM or branch
> verdict taken, the bytes serialized on either wire, the post-state hash, whether $`d`$ is admitted
> at all, or the token cost charged.

Write $`M_{\mathrm{old}}`$ and $`M_{\mathrm{new}}`$ for the two nodes' observable behaviour — so
$`M(d, S)`$ is everything the node exhibits when it runs deploy $`d`$ against pre-state $`S`$ — and
write $`\pi_i`$ for the projection onto axis $`i`$:

```math
\mathsf{Breaking}
\;\;\equiv\;\;
\exists\, d,\; S,\; i \in \{1,\dots,6\}
\;:\;
\pi_i\!\bigl(M_{\mathrm{old}}(d, S)\bigr)
\;\neq\;
\pi_i\!\bigl(M_{\mathrm{new}}(d, S)\bigr)
```

Two consequences a reviewer should hold onto.

1. **The definition is existential.** One witnessing deploy suffices. "The suite is green" is therefore
   never a proof of non-breakage; it is evidence about a corpus, and each entry records *which* corpus.
2. **Repair and regression satisfy the same predicate.** A change making a wrong answer right is
   consensus-breaking in exactly the technical sense of one making a right answer wrong. The two are
   separated by the **Direction** field (§2.6), not by the definition.

#### Figure 1 — the six consensus surfaces and the fault each produces

![The six consensus surfaces](figures/consensus-axes.svg)

*Source: [`figures/consensus-axes.puml`](figures/consensus-axes.puml).*

| # | Axis | The question it answers | Canonical failure |
|---|---|---|---|
| 1 | **Computed value** | What term does a redex reduce to? | Two nodes put different data on a channel. |
| 2 | **Verdict** | Which COMM fires? Which `match` branch reduces? Does a guard admit? | Two nodes take different control-flow paths from identical state. |
| 3 | **Serialized bytes** | What bytes represent this term — on Lane B, and on Lane P? | Event hashes differ; a block encodes differently; the signed preimage moves. |
| 4 | **Post-state hash** | What root is committed? | Replay's $`h_{\mathrm{post}}`$ differs from the block's $`\Rightarrow`$ **safety fork**. |
| 5 | **Accepted programs** | Is this deploy admitted at all? | A *decidable* refusal $`\Rightarrow`$ a failed deploy; a *process abort* $`\Rightarrow`$ a liveness split. |
| 6 | **Metering** | Does the committed COMM count, or the funding/settlement surface, move? | A token-budget boundary fires on one node and not another — which would turn into an Axis-2 divergence. |

⚠ **Axis 6 under the token model — the attribution rule, defined here so no entry re-derives it.**
M records a movement of the *committed COMM count for executions whose verdict trace is otherwise
unchanged*, or of the funding/settlement surface. A divergence that flows through a changed value
or verdict (a deploy that now fails fires different COMMs) is filed on axes 1–2, never double-
counted on M. Per-op prices are diagnostics and cannot move M. Budgets belong to F1r3node
(`wallet.txt`); no entry introduces a metering surface. ⚠ Under OD-1 accepted user deploys run
unmetered-for-liveness, so the metering-to-verdict composition edge below is **latent**: it can fire
only for budgeted (system/gate) execution, not for an accepted user deploy.

**How the axes compose.** The dependency has a direction:

```math
\text{value} \;\longrightarrow\; \text{bytes} \;\longrightarrow\; \text{hash}
\qquad
\text{verdict} \;\longrightarrow\; \text{hash}
\qquad
\text{metering} \;\longrightarrow\; \text{verdict}
```

But the converse fails in both interesting directions, which is precisely why the axes must be answered
separately rather than summarised:

- **Bytes can move with no value moving.** `7dcff96f` (**CBR-014**) appends four zero bytes to a bincode
  `EZipper` record while every previously-representable zipper computes the identical value, and while
  the *protobuf* encoding of that same zipper is byte-identical.
- **A verdict can move with no value moving, and a value with no verdict.** **CBR-011** moves the
  identity relation (verdict) of path maps while every computed value stays fixed; the register's
  retired history carries the converse (retired entries CBR-005/CBR-007,
  [Appendix B.1](#b1-retired-register-entries)).

#### Figure 3 — from an axis change to a fork

![From an axis change to a fork](figures/divergence-flow.svg)

*Source: [`figures/divergence-flow.puml`](figures/divergence-flow.puml).*

### 2.5 The two wire formats

⚠ **A `Par` is serialized by two independent codecs, in two different field orders.** Conflating them is
the commonest way to mis-review a change in this area, so both are answered separately in every axis
table.

| | **Lane B — bincode** | **Lane P — prost** |
|---|---|---|
| Format | bincode 1.3.3, default config (legacy fixint, little-endian) | protobuf 3 |
| Field order | **serde declaration order** — the order fields appear in the generated struct | **ascending minimum tag**; prost emits every plain field first, then every `oneof` |
| Where that order is recorded | nowhere at run time; recovered from the `FileDescriptorSet` prost already dumps | the `.proto` tag numbers |
| Reaches | the **cold store** (LMDB, `rspace++/src/rspace/serializers/`) and the **event hash** (`rspace++/src/rspace/hashing/stable_hash_provider.rs`) | the **block body** (`CasperMessage.proto`) and the **signed preimage**, `sort_match(p).term.encode_to_vec()` (`rholang/.../cost_accounting/sig.rs`) |
| `locally_free` | blanked to eight zero bytes | real `bytes` — 12 fields, asserted `ProtobufKind::Bytes` |
| Depth policy | symmetric: both directions unbounded | historically **asymmetric** (the stock decoder capped recursion at 100 levels while the encoder capped nothing); **symmetric at the anchor** — the generated decode PDA reads without a recursion budget (**CBR-044**) |

#### Figure 2 — the two wire formats

![The two wire formats](figures/wire-formats.svg)

*Source: [`figures/wire-formats.puml`](figures/wire-formats.puml).*

The hazard the figure encodes, stated once so no entry restates it:

> ⚠⚠ `TaggedContinuation` declares its `oneof` **before** `guard`. A generator reusing one field-order
> table for both drivers produces, for protobuf, **a 95-byte encoding with its halves exchanged** —
> identical length, identical byte multiset, invisible to a length check *and to a round trip*. For
> protobuf the correct order is *the opposite* of the correct serde order.
> **MEASURED** (`c28f4cf6`, finding 1; `7c74260d`; reproduced permanently as generator mutation M2 in
> `56fb1fd0` — difference at byte 0, both encodings 1140 bytes, same byte multiset, halves exchanged).

This is why **round-trip is not the codec obligation**: a codec that encodes differently from its
predecessor but decodes its own output round-trips perfectly *and forks*. The obligation is
**differential byte identity against a retained, undriftable oracle** — the compiler-generated derive,
which is kept compiled for exactly that purpose.

The historical asymmetry in the last table row was recorded as open hazard CBR-028 and is **closed
against [CBR-044](#cbr-044)** — the retired row is [Appendix B.1](#b1-retired-register-entries).

### 2.6 Direction of change

| Direction | Meaning |
|---|---|
| **REGRESSIVE** | A previously-succeeding thing now fails. ★ Reviewers weigh this most heavily. |
| **PERMISSIVE** | A previously-failing thing now succeeds. |
| **CORRECTIVE** | A previously-*wrong* answer is now right: the program succeeded before and succeeds now, but the answer moved. |
| **NEUTRAL** | No observable behaviour moves. The entry exists because the change sits on a consensus path and its neutrality is a **measured claim**, not an assumption. |
| **DIVERGENT** | A deliberate, ruled departure from the reference implementation. |
| **CONVERGENT** | ★ A previously-recorded **DIVERGENT** departure is *withdrawn*: the two implementations now agree where they did not. A distinct direction, not a spelling of CORRECTIVE, because the axis movement is measured against *the other implementation* rather than against this one's own prior answer. (The vocabulary's one historical instance is a since-retired Surface-L float entry, [Appendix B.1](#b1-retired-register-entries).) |

### 2.7 Evidence grade — and the word "potentially"

The user's requirement is that a divergence *reachable in principle but unwitnessed* be labelled as such:
neither upgraded to "does break" nor dismissed as "does not". That is what this column carries, and it
is the report's central evidentiary distinction.

| Grade | Meaning | Example |
|---|---|---|
| **WITNESSED** | A concrete program, term, or byte string is known that exhibits the divergence. | **CBR-044** — pinned byte goldens move with the EPM1 transition. |
| **MECHANISM-ONLY** | The mechanism is proven and the path is reachable by an ordinary deploy, but no witnessing program has been exhibited. | **CBR-013** — the bulk-reader collapse's recursive canonicality, proven at the mechanism. |
| **LATENT** | The mechanism exists, but reaching it needs a capability an ordinary deploy lacks, or a structural argument shows the path is not walked. | **CBR-L14** — the byte-literal surface lands ahead of any chain that could run it. |
| **DORMANT** | The changed code has no caller at all, established **mechanically**, not by intention. | the retired prost-encoder row ([Appendix B.1](#b1-retired-register-entries)). |
| **NEUTRALITY-MEASURED** | The claim is that *nothing* moves, and that claim is itself the measurement. | the retired event-hash-encoder row ([Appendix B.1](#b1-retired-register-entries)). |
| **UNVERIFIED** | Not established. An honest gap; every occurrence is counted in §6.4. |  |

### 2.8 Provenance tags

Every factual claim in this report carries one of:

| Tag | Meaning |
|---|---|
| **DERIVED** | Established here by static analysis: reading the diff, the type, the call graph, or the `.proto`. |
| **MEASURED** | Established by running something: a test that went RED then green, a byte diff, a bisected stack ceiling, a benchmark. |
| **CITED** | Quoted from a commit message or an owner ruling. The underlying work was performed by the cited author; this report reproduces rather than re-derives it. |
| **UNVERIFIED** | Named, not established. |

⚠ The distinction between **MEASURED** and **CITED** matters for this report specifically: the campaign's
commit messages carry unusually detailed measurements, and this register did **not** re-run them. Where a
number appears, its tag says whether this author observed it or is quoting the commit that did.

---

## 3. Method

### 3.1 How the change set was derived

The candidate list supplied to this report was treated as a set of hypotheses, not as a census. The set
below was derived independently and then reconciled against it.

**Step 1 — fix the range.** The register anchor is `7293d57c`, the merge point at which the previous
consensus cluster was consolidated and reviewed. Everything after it is unreviewed. **MEASURED**:
`git rev-list --count 7293d57c..HEAD` = **109** at `8853f839`, and **111** at `dc383ed1` after
**CBR-007** landed mid-authoring — see §6.1(4).

**Step 2 — define the consensus-critical path set** $`\mathcal{P}`$. A path is consensus-critical if
code under it can move any of the six axes:

```text
models/src/                      the term type, the codecs, the sorter, the path-map integration
models/src/main/protobuf/        the wire schema itself
models/codegen/, models/build.rs the GENERATOR that emits the bincode/protobuf tables and PDAs
rholang/src/rust/interpreter/    the normalizer, the reducer, the matcher, substitution, the printer
rho-pure-eval/src/               the GUARD EVALUATOR — decides `where` verdicts
rspace++/src/                    the tuplespace, the event hashes, the cold store
casper/src/                      block admission, replay comparison, validation
shared/src/                      the printer's Audience and the shared error surfaces
```

⚠ ★ **This path set is itself a corrected artefact, and the correction is a result.** The first version
omitted `rho-pure-eval/src/` — the component that **decides guard verdicts**. Two entries live there
(`eaa44c2f` in **CBR-002**, `a3fd6fe4` in **CBR-023**) and were found only because the entry set was
assembled from commit *messages* as well as from paths. A reviewer should read §3.4(1) with that in
mind: the class is not hypothetical, it fired on this very sweep.

**Step 3 — enumerate.** `git log 7293d57c..HEAD -- <paths>` over that set yields **76** commits at
`8853f839`, **78** at `dc383ed1`. **MEASURED.**

**Step 4 — read every one.** Each commit's full message and file list was read. This campaign's commit
messages state consensus visibility explicitly and in detail, which makes them primary evidence; where a
message asserts neutrality, the assertion was checked against the diff's file list and, where the claim
was byte identity, against the named golden or differential test.

**Step 5 — classify.** Each commit is either an **entry** (it moves at least one axis for at least one
program) or an **exemption** (it does not), and every exemption carries a typed reason and the evidence
discharging it. The partition is exact and is reproduced in [Appendix B](#appendix-b--the-exemption-table).

**Step 6 — sweep the companion surface.** `mettail-rust`'s campaign window holds **236** commits, of
which **149** touch `languages/src`, `rholang-runtime/src`, `macros/src` or `ast/src`. ⚠ **This sweep is
a targeted selection, not an exact partition**: the eleven Surface-L entries were found by semantic
keyword search and by reading the campaign ledger, and the remaining 133 commits are neither entries nor
explicit exemptions. This asymmetry is a stated limitation — see §6.5 — and closing it is the first
extension §7's gate should be given.

**Step 7 — capture what had not landed.** Four changes were in flight when the sweep began; at this
revision one remains: the kv element-category gate (**CBR-L08**). The checked-arithmetic repair
landed (**CBR-027**), the `attempt_opt` combinator landed and was later retired as a bug fix, and
the open wire-asymmetry hazard (formerly CBR-028) is closed against **CBR-044**.

**Step 8 — re-scope to may-change-consensus (2026-08-03).** The owner ruled that **bug fixes and
optimizations are not consensus-breaking for this register's purpose**, and that stack-safe
conversions proven equivalent to their recursive counterparts definitely do not change consensus —
while **data-model, wire-format, and deliberate semantic changes** (the EPathMap representation and
EPM1 being the named exemplar) are what this register exists to carry. Every entry was re-classified
under that criterion; the 45 that no longer qualify are retired to typed rows in
[Appendix B.1](#b1-retired-register-entries), and their full bodies remain in git history at the
pre-refactor revision.

### 3.2 Inclusion and exclusion criteria

A change is a **register entry** if and only if it *may actually change consensus*: it moves an
axis of §2.4 for some deploy and reachable pre-state, **and** it is not excluded by the owner's
2026-08-03 ruling. The ruling excludes, categorically: **bug fixes** (a wrong answer made right —
even though such a change satisfies §2.4's predicate, it is not treated as consensus-breaking
here), **optimizations** (no observable movement, measured), and **equivalence-proven stack-safe
conversions**. What remains is deliberate consensus content: data-model and wire-format
transitions, acceptance-set changes (new or removed syntax and refusals), ruled semantic changes,
and metering-surface changes under the token model.

**Algorithm 1 (CLASSIFY-CHANGE).** The decision procedure, in literate form; each fragment names
the judgement it encodes.

```pseudocode
⟨Classify one landed change⟩ ≡
    ⟨Does any §2.4 axis move for some deploy and pre-state?⟩
    ⟨If not: exempt with a commit-level reason⟩            ── the §3.2 enum below
    ⟨If it moves only as a repair or an optimization:
      retire with a typed reason⟩                          ── B.1; ruling 2026-08-03
    ⟨Otherwise: full entry⟩                                ── Appendix A form, six axes,
                                                           ── direction, grade, authority

⟨If it moves only as a repair or an optimization: retire with a typed reason⟩ ≡
    BUG_FIX_RULED_NONCONSENSUS      ── a wrong answer became right; cite the ruling
    OPTIMIZATION_MEASURED_NEUTRAL   ── performance work, movement measured absent
    EQUIVALENCE_PROVEN              ── stack-safe conversion with an oracle-backed
                                    ── equivalence (bytes, verdicts, charges)
    DORMANT                         ── no caller, established mechanically
    CLOSED_BY_CBR-044               ── a recorded hazard mooted by the EPM1 transition
    ── A retired row keeps its FORMER ID forever: identifiers are never reused,
    ── and every historical citation must keep resolving (Appendix B.1).
```

The original commit-level partition (steps 1–5) additionally admits, on the entry side:

- corrections (a wrong answer becomes right) on equal footing with regressions;
- **liveness** changes, because a node that aborts where another completes is not agreeing;
- **additive** surface (a new method, a new field) — because acceptance is an axis: a program that
  previously failed to normalize now runs.

**Excluded**, with the reason recorded rather than the commit silently dropped:

| Exclusion reason | Meaning |
|---|---|
| `TESTS_ONLY` | Touches only `tests/`, fixtures, or `#[cfg(test)]` code. |
| `DOCS_ONLY` | Touches only comments, documentation, or audit records. |
| `HYGIENE` | Imports, formatting, lints, clippy, `-D warnings`, dead-code deletion with no live caller. |
| `BYTE_NEUTRAL_MEASURED` | Restructures a consensus path with byte identity asserted by a named golden or differential. |
| `CHARGE_NEUTRAL_MEASURED` | Moves work on a metered path with charge count, order and value asserted unchanged. |
| `VERDICT_NEUTRAL_MEASURED` | ★ Restructures a **verdict-deciding** path with the verdict relation asserted unchanged by a named differential. Added 2026-07-29 for `383a8b56`, which moves `rho-pure-eval`'s evaluator — the component that decides `where` verdicts — onto a shared trampoline. *Byte* identity is not the claim that matters there; *verdict* identity is, and collapsing the two would have made the exemption say something it could not support. ⚠ Adding a variant is a code change in the gate's `CLOSED_REASONS` and is therefore reviewed, which is the property [§7.2](#7-maintenance) clause 4 exists to have. |
| `DEP_BUMP_BYTE_NEUTRAL` | ★ A dependency-version change with a named differential showing published bytes unmoved. Reserved by [§7.5](#7-maintenance) extension 2 and now spellable, because `Cargo.lock` is inside the derived path set; **CBR-L06** proves a `prost` or `thiserror` bump alone can move published bytes. No commit carries it yet. |
| `DORMANT` | Adds code with no caller, established mechanically. |
| `INFRA` | Build, CI, or tooling. |
| `SUPERSEDED` | Wholly subsumed by a later commit that is itself an entry. |

### 3.3 How each axis value was established

| Axis | Primary method |
|---|---|
| 1 · computed value | **DERIVED** from the diff: does the changed expression produce a different term for some input? |
| 2 · verdict | **DERIVED** from the call graph: does the changed code reach `Matcher::get` / `check_commit` / `eval_match`? |
| 3 · bytes | **DERIVED** from which codec the change sits in (§2.5), then **CITED** from the golden or differential test the commit names. |
| 4 · post-state hash | **DERIVED**: an Axis-1 or Axis-2 move on a path reaching `produce`/`consume` implies an Axis-4 move. |
| 5 · acceptance | **DERIVED** from the refusal or ceiling introduced or removed. |
| 6 · metering | **DERIVED under the token model (§2.2)**: does the change alter the committed COMM count for an execution whose verdict trace is otherwise unchanged, or touch the funding/settlement surface? Per-op `reserve_*` sites are diagnostics and do not move this axis. Every kept entry's M cell was re-derived under this rule on 2026-08-03. |

### 3.4 ★ False-negative risk of this method — the classes this sweep would miss

A method that cannot state what it would miss is not a method. Five classes:

1. **A consensus-visible change outside $`\mathcal{P}`$.** The path set is a hypothesis about where
   consensus lives. A change in `comm/`, `node/`, `block-storage/`, `crypto/`, or a **dependency
   version bump in `Cargo.toml`** could move bytes or verdicts and would not be enumerated. ⚠ This is
   not hypothetical: **CBR-L06** exists precisely because a derived `Debug` implementation reaches
   published bytes, so *a `prost` or `thiserror` version bump alone* is a consensus change under this
   report's own definition — and no path in $`\mathcal{P}`$ would show it.
2. **A change whose commit message asserts neutrality falsely.** Step 4 checks assertions against the
   diff's file list and the named tests, but it does not re-run the goldens. A commit that claims byte
   identity, names a test, and is wrong would pass this sweep. Mitigation: every such claim is tagged
   **CITED**, never **MEASURED**, so a reviewer can see exactly which claims rest on the author's
   measurement rather than this report's.
3. **An emergent divergence with no single owning commit.** Two individually byte-neutral changes can
   compose into a non-neutral one — for instance a reordering in one commit and a sort-stability change
   in another. A per-commit sweep cannot see it. Mitigation: none. Named as a gap.
4. **Generated code.** `models/build.rs` emits the wire tables into `OUT_DIR`. A change to the
   *generator* is in $`\mathcal{P}`$, but a change to `prost-build`, to `rustc`'s derive expansion, or
   to the descriptor pass is not. ⚠ This class has a measured near-miss: emitting any
   `cargo:rerun-if-changed` switches cargo to watching only those paths, which left a **stale table in
   `OUT_DIR` while the build reported success** — a silent, byte-visible divergence between the
   generator in the tree and the table in the binary. **CITED** (`903cefb3`).
5. **Uncommitted work.** Both trees had substantial uncommitted changes while this report was written,
   and several agents were editing concurrently. In-flight entries describe **intended** behaviour
   verified against a working tree that has since moved.

---

## 4. The register

### 4.1 Summary — the register at a glance

Scan this table; read only the bodies you need.

**Axis cells** — **V**alue · **T** verdict · **B** bytes on Lane B (bincode) · **P** bytes on Lane P
(prost) · **H** post-state hash · **A** acceptance · **M** metering.
`●` moves · `○` does not move · `·` N/A · `?` UNVERIFIED.

**Surface** — `N` = F1r3node (the consensus implementation) · `L` = MeTTaIL's Rholang (a divergence here
is a *future* fork, not a present one).

**Grade** — the evidentiary column of §2.7: `W` WITNESSED · `M` MECHANISM-ONLY · `L` LATENT ·
`D` DORMANT · `NM` NEUTRALITY-MEASURED · `?` UNVERIFIED.

| ID | S | Headline | Commit(s) | V | T | B | P | H | A | M | Direction | Grade |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| [CBR-002](#cbr-002) | N | An undecidable `where` guard is **refused**, not silently false | `6ab1c78b`, `eaa44c2f` | · | ○ | ○ | ○ | ○ | ● | · | REGRESSIVE | **W** |
| [CBR-011](#cbr-011) | N | A pathmap's identity stops depending on insertion order | `478102a4` | ○ | ● | ○ | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-012](#cbr-012) | N | The shadow `Vec` deleted — non-ground pathmap order follows the trie | `9994a75b` | ○ | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-013](#cbr-013) | N | The two bulk trie readers collapse to the key walk | `c705776c` | ● | ● | ○ | ○ | ○ | ○ | ○ | CORRECTIVE | **M** |
| [CBR-014](#cbr-014) | N | `EZipper.cursor_kind` — a **new proto field**; four bytes on Lane B | `7dcff96f` | ● | ● | ● | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-024](#cbr-024) | N | `last` joins the method table | `2fee67fa` | · | · | · | · | · | ● | · | PERMISSIVE | **W** |
| [CBR-025](#cbr-025) | N | Trie enumeration: `getPath` / `toNextLeaf` / `leafCount` | `98d2422d` | · | · | · | · | · | ● | · | PERMISSIVE | **W** |
| [CBR-027](#cbr-027) | N | GInt `+` and `-` stop wrapping on overflow | `6ff46f8a` ⚠, `fd5474ab` | ● | ● | ● | ● | ● | ○ | ○ | REGRESSIVE | **W** |
| [CBR-030](#cbr-030) | N | `NonNegativeNumber.rho`'s overflow guard becomes **total** — the genesis term moves | `e3a4494b`, `719f2432` | ● | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-037](#cbr-037) | N | The `EPathMap` tag-8 trie-key **reader** becomes total — the writer was unlimited by requirement | `063974c5` | ○ | ● | ○ | ○ | ○ | ● | ○ | PERMISSIVE | **W** |
| [CBR-041](#cbr-041) | N | An `EPathMap` serializes on the prost wire as the TRIE — its own byte array `U(m)` at field 8 — for every map; the tag-1 list arm is deleted | `1b576c90` | ○ | ● | ○ | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-042](#cbr-042) | N | …and on the **bincode** wire too — `U(m)` verbatim and contiguous, then the values (FORM ②), so the reader never decodes a trie key | `3a32cf07` | ○ | ○ | ● | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-043](#cbr-043) | N | …but of **the entries that surface WRITES**. FORM ② keyed lf-blanked values by the *unblanked* entries, putting an entry's `locally_free` on the event hash | `8cf0b770` | ○ | ○ | ● | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-044](#cbr-044) | N | EPathMap becomes a homogeneous PathMap set/map and both codecs carry one versioned EPM1 trie snapshot; generated protobuf PDAs remove the read ceiling | `26876b65` | ● | ● | ● | ● | ● | ● | ○ | CORRECTIVE | **W** |
| [CBR-L07](#cbr-l07) | L | `List.last()` in MeTTaIL's Rholang | `bbceb6d9`, `6e543c01` | · | · | · | · | · | ● | · | PERMISSIVE | **W** |
| [CBR-L08](#cbr-l08) | L | The kv element-category gate — a Name in a kv slot is refused, not silently dropped | *in flight* | ● | ● | ● | ● | ● | ● | ○ | REGRESSIVE | **W** |
| [CBR-L10](#cbr-l10) | L | A pathmap's entries come from a projection, not a field | `832d510f` | ○ | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-L11](#cbr-l11) | L | The `UInt32` acceptor is narrowed to canonical spellings (the cluster's deliberate acceptance decision; its round-trip bug fixes are retired) | `4aa64cb6` | · | ○ | ○ | ○ | ○ | ● | · | REGRESSIVE | **W** |
| [CBR-L14](#cbr-l14) | L | `Bytes` becomes a real byte sequence with a real surface — `![Vec<u8>]` plus the `b"deadbeef"` literal | `713e0364`, `5a9efa00`, `93155150`, `3aea562f` | ● | ● | ● | ● | ● | ● | ○ | CORRECTIVE | **L** |

**Totals — 19 entries**: **14 on Surface N, 5 on Surface L**; **18 landed, 1 in flight**
(**CBR-L08**); zero open hazards. By evidence grade: **17 WITNESSED**, 1 MECHANISM-ONLY
(**CBR-013**), 1 LATENT (**CBR-L14**). By direction: **11 CORRECTIVE, 4 PERMISSIVE, 4 REGRESSIVE**.
Axis cells reading `UNVERIFIED`: **0** — the register's one historical `?` cell (CBR-L07 metering)
resolved under the token model (§3.3). The 45 retired entries are
[Appendix B.1](#b1-retired-register-entries); 19 + 45 = 64 historical identifiers, none reused.

### 4.2 Entry template

Every entry below follows the fixed form specified in [Appendix A](#appendix-a--the-entry-template):
a header table, then **(a) the issue**, **(b) how it (potentially) breaks consensus** — the full axis
table, the concrete disagreement, the blast radius, and the chain-history question — then **(c) why the
change was necessary or correct**, and finally the evidence. Axis cells use the closed vocabulary
`MOVES` / `NO` / `N/A`.

---

## 4.3 Surface N — the F1r3node consensus implementation
---

### CBR-002

**An undecidable `where` guard is refused at compile time, not silently answered `false`.**

| | |
|---|---|
| Commit(s) | `6ab1c78b` (the refusal), `eaa44c2f` (the derived undecidable class) |
| Status | LANDED |
| Direction | REGRESSIVE |
| Evidence grade | WITNESSED |
| Files | `rholang/src/rust/interpreter/guard.rs` (new), `.../compiler/normalizer/processes/p_input_normalizer.rs`, `p_match_normalizer.rs`, `.../matcher/match.rs`, `.../reduce.rs`, `.../errors.rs` |

#### (a) The issue

`rho-pure-eval` — the guard evaluator — answers `UnsupportedExpression` for six `ExprInstance` arms it
cannot evaluate. The guard site mapped that answer to `false`, which is also what a guard that *was*
evaluated and *was refuted* returns. So "the decider reached no verdict" and "the predicate does not
hold" were **one observation**, and a `where` clause using `.nth()`, `%%`, `++`, `--`, a path map or a
zipper silently admitted nothing, forever, with no diagnostic.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A — no value is computed differently; the deploy no longer runs. |
| 2 · verdict | NO — *"No program that admitted a COMM before admits a different one now; the refused set and the fired set are disjoint."* **CITED**. |
| 3 · bytes (Lane B) | NO |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | NO for programs that still run; the refused programs produce no state at all. |
| 5 · accepted programs | **MOVES** — this is the entry. A program that previously normalized, ran, and admitted nothing now **fails**. |
| 6 · metering | N/A — the refusal precedes any charge. |

**The disagreement.** A deploy whose `where` guard mentions `x.nth(0) == 1` normalizes and runs on an
old node (admitting nothing, forever) and is **rejected** by a new node. Because the refusal is a pure
function of the guard *term* — same answer on every node, no rollback, no dependence on which data
happened to be resting — every honest new node refuses identically. Mixed-version, that is a
`ReplayStatusMismatch` on `is_failed`, hence `InvalidBlock::InvalidTransaction`, hence **slashable**.
It is not a silent safety fork.

**Blast radius.** Any deploy whose `for … where` or `match … where` guard contains `EMethodBody`,
`EPercentPercentBody`, `EPlusPlusBody`, `EMinusMinusBody`, `EPathmapBody` or `EZipperBody`. Reachable by
an ordinary deploy: **yes**. ★ `EMatchesBody` is deliberately **not** in the refused class — both guard
sites inject `SpatialMatcherOracle`, so `matches` guards stay compilable (see **CBR-003**), and a test
pins that they do.

**Could live chain state have been produced under the old behaviour?** ⚠ **Not settleable from inside
the repository, and the answer matters more here than elsewhere**, because this direction is
REGRESSIVE: a deploy that used to be *accepted-and-inert* becomes *rejected*. The settling query: scan
every historical `ProcessedDeploy.deploy.data.term` for a `Receive.condition` or `MatchCase.condition`
whose term contains any of the six refused arms. If the count is zero, the change is
acceptance-neutral on the existing chain. **UNVERIFIED** here.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** The failure mode is the worst one a distributed system has: a
receive that rests forever, a node that exits 0, and **no diagnostic produced anywhere**. A deployer
cannot distinguish "my guard is false" from "your node cannot evaluate my guard". The prior campaign
already ruled that an abstention indistinguishable from a verdict *is itself the defect*.

**Why refuse rather than decide more.** Teaching `rho-pure-eval` the missing methods would remove the
problem rather than report it, and it was rejected on a ground that is **not effort**: guard evaluation
is **unmetered**. `check_commit` holds no cost handle and runs $`\prod_j |\mathrm{pool}_j|`$ times per
consume. Arithmetic is safe under that ($`\Theta(1)`$ per node, node count fixed by the source), but
`nth` / `slice` / `union` / `toByteArray` are $`\Theta(|\mathrm{data}|)`$ in **attacker-supplied
run-time data**. Implementing them would turn a silent guard into an unmetered denial-of-service
surface. It would also only *move* the subset boundary — the silence returns for whatever is still left
out — so the refusal is needed anyway. **CITED**.

**Why compile time rather than raising inside the matcher.** A guard decides whether a COMM fires, so
raising from `check_commit` puts a new failure path through `consume`/`produce` in **both** the play and
the replay space, where the comm-event sequence must be reproduced exactly, from a partially-mutated
space. The compile-time refusal is a pure function of the guard term: no rollback, no dependence on
resting data. It is also the only option that does not change `Match::check_commit`'s signature — a
trait implemented outside this workspace. **CITED**.

**Why two layers.** The refusal is applied at *compile* (`p_input_normalizer`, `p_match_normalizer`) and
at *reduce* (`eval_receive`, `eval_match`). The reduce layer is not belt-and-braces: embedders build
`Receive.condition` directly and hand the `Par` to `Reduce`, and for them the compile layer does not
exist.

**The boundary is drawn and pinned.** Decider gaps (the answer does not exist on this node) are refused;
**data-dependent failures** (`UnboundVariable`, `OperatorTypeMismatch`, `DivisionByZero`,
`ArithmeticOverflow`, `NotASingleValue`, `MissingExprInstance`) are **not** — `x / y` is fine until `y`
is 0, so no compile-time gate can decide them, and their COMM verdict is unchanged.
`a_data_dependent_failure_is_not_refused_at_compile_time` pins the boundary so it cannot be read as an
oversight. **CITED**.

**Authority.** No owner ruling on this specific change. The commit records the version obligation
explicitly: *"`Validate::version` … is exact equality against the genesis-anchored version — no
per-feature gate, no height-conditioned switch — so this ships as a coordinated version bump, as
`ca84d535` did. It is NOT behind a default-off flag: a dormant flag is unshipped work."* **CITED**.

#### Evidence

- The undecidable class is **derived from the evaluator's own arms** — an exhaustive `ExprInstance`
  match with no catch-all, in both places, so a new variant fails to compile rather than falling
  silently into the permissive branch. **DERIVED** (`eaa44c2f`).
- `the_refusal_fires_before_any_matcher_is_consulted` **measures** that the refusal precedes
  `check_commit`, with a live control proving the counter moves for a decidable guard — so it holds for
  every `Match` implementation, including embedder-supplied ones. **MEASURED**.
- `the_refusal_message_depends_on_nothing_but_the_guard` pins that the message is a pure function of the
  guard term (clause label + fixed per-variant phrase + the method's own name). This matters because the
  message is a candidate for the replay-compared `error_message` path (see **CBR-016**). **MEASURED**.

---

### CBR-011

**A pathmap's identity stops depending on the order its entries were written in.**

| | |
|---|---|
| Commit(s) | `478102a4` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/pathmap_crate_type_mapper.rs`, `models/src/rust/rhoapi_ext.rs` |

#### (a) The issue

`EPathMap`'s `PartialEq`, `Hash` and `Ord` read the `ps` **`Vec`** — i.e. the order a producer happened
to write entries in — while `encode_raw` emitted a ground map as proto field 8, the **trie's own key
stream**, and `serialize` emitted both through the same trie. **The wire and the comparators already
disagreed.** Two producers could build one map, agree byte-for-byte on the consensus encoding *and* on
the event hash, and still fail to recognise each other's value in a `HashSet`.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | NO — no value changes; the *equivalence class* changes. |
| 2 · verdict | **MOVES** — `==`, set membership, and pattern equality on path maps. |
| 3 · bytes (Lane B) | NO — *"Every committed golden is unchanged"*. **CITED**. |
| 3 · bytes (Lane P) | NO — same. |
| 4 · post-state hash | **MOVES** — a `HashSet`/`Map` keyed by a path map now dedups where it did not. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

**The disagreement.** Two path maps with the same entries in different insertion order compare unequal
on an old node and equal on a new one. A `match` on `{| 1, 2 |}` against a target built as `{| 2, 1 |}`
takes a different branch.

**Blast radius.** Any program comparing, matching, or set-inserting path maps. Reachable by an ordinary
deploy: **yes**.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: replay history under an instrumented build counting `EPathMap::eq` calls
whose two operands have equal entry *sets* and unequal `ps` order. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A set whose identity depends on insertion order is an artifact of
storing a set in a `Vec`, not a decision anyone made. Two honest producers of the same map disagree
about whether they built the same thing.

**★ This commit imposes no canonicity; it removes a competitor.** The trie's children are indexed **by
byte**, so a read-zipper walk over it *is* byte-lexicographic by construction. Nothing selects that
order and nothing could select a different one. The `Vec`'s insertion order was a *second* order
shadowing it, and the comparators were the last readers of it. They now read $`U(m)`$, the field-8
path stream the wire has used all along. **CITED**.

**What is deliberately left alone, and why.** `Ord` was **not** moved: *"moving it moves sort orders,
which IS consensus-visible, and it collides with the [pre-existing `Ord`-includes-`locally_free`] wart.
It is a separate ruling with a separate cost."* **CITED**. ⚠ This is a disclosed *incompleteness*, not a
completed repair.

⚠ **A landmine named rather than removed.** `PathMap::hash` and `PathMap::merkleize` are **not used and
must never be**: `pathmap-0.2.2/src/lib.rs:15-21` selects gxhash (AES intrinsics) normally and *"a simple
XOR hasher"* on `riscv64`/miri, so both are **architecture-dependent** and would make consensus identity
depend on the machine that computed it. Neither has a call site; the note exists so the first one is a
deliberate act rather than an accident. **CITED**.

**Authority.** ★ Owner ruling, **2026-07-28**, relayed verbatim in the campaign ledger:
*"`EPathMap` must be a trie map. Not a list of values, not a list of key-value pairs, not an `EMap`."*
**CITED**. This entry is stage S1 of that ruling; **CBR-012** and **CBR-013** are S2 and S3.

#### Evidence

- **Exhaustive** over all $`2^9 = 512`$ subsets of the nine-element codec alphabet (both arms, both
  colliding arities, the nesting and sign edges), each in three constructions — forward, reversed,
  duplicated. Deterministic: no seed, no regressions file. Four legs (`==`, `Hash`, serde bytes, prost
  bytes) must all agree. **MEASURED**.
- Two anti-vacuity legs: 511 adjacent subsets must be **separated** by all four, and the bare element `1`
  must stay distinct from its singleton list `[1]` (`03 02` versus `03 02 00`). **MEASURED**.
- Goldens unchanged: `protobuf_goldens_epathmap_fixtures`, `serde_bincode_goldens_epathmap_fixtures`,
  `serde_json_goldens_epathmap_fixtures`, `event_hash_goldens_produce`, `event_hash_goldens_consume`.
  **CITED**.

---

### CBR-012

**The shadow `Vec` is deleted — a non-ground pathmap's wire, serde preimage and sort score follow the trie.**

| | |
|---|---|
| Commit(s) | `9994a75b` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/main/protobuf/RhoTypes.proto`, `models/src/rust/rhoapi_ext.rs`, `models/src/rust/rholang/bincode_schema.rs`, `bincode_encoder.rs`, `sorter/sort_combine.rs`, `models/src/rust/spliced_event_bytes.rs`, `models/src/rust/canonical_path.rs`, `rholang/src/rust/interpreter/reduce.rs` (and five more) |

#### (a) The issue

**CBR-011** made the comparators read the trie. This deletes the `Vec` itself, so there is no second
order left to read. With it go the canonicalisation fork (four consumers each carrying an "if this map is
ground, read the entries off a trie instead" branch), the `ps_make_mut` bypass and its `debug_assert`
policing, the cached-bytes validity checks, and a raw-pointer arena in `bincode_encoder` that existed only
because a ground map's canonical `ps` was *constructed* at serialize time.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | NO |
| 2 · verdict | **MOVES** — `Ord` moves for non-ground maps, hence sorted-container order. |
| 3 · bytes (Lane B) | **MOVES** — for **non-ground** maps only. |
| 3 · bytes (Lane P) | **MOVES** — for **non-ground** maps only. |
| 4 · post-state hash | **MOVES** — event-hash preimages follow. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

★ **The population boundary, quoted exactly**, because it is the whole review proposition:

> *"GROUND maps do not move. They already encoded as proto field 8 (`U(m)`), and their serde preimage was
> already `ground_canonical_ps`. Both were already deduplicated and order-insensitive.
> NON-GROUND maps move. Their only encoding is tag 1, `repeated Par`, written in `ps` order, so
> re-ordering `ps` re-orders the wire: prost bytes, serde bytes, event-hash preimages and
> sorted-container order all follow. `Ord` moves for the same reason."* **CITED** (`9994a75b`).

**The disagreement.** A non-ground path map (one containing a connective or a free variable — i.e. any
path map used as a *pattern*) built by two producers in different orders now encodes identically; against
an old node it encodes differently, and the event hash follows. Safety fork.

**Blast radius.** Every program constructing or matching a **non-ground** path map. Reachable by an
ordinary deploy: **yes**.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: scan historical `ProduceEventProto`/`ConsumeEventProto` preimages for an
`EPathMap` with `connective_used = true` or a non-empty `remainder`. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** The wire and the comparators disagree, permanently, and the
disagreement is invisible: both are individually self-consistent.

**Why deletion rather than a canonicalizer.** *"The projection IS that trie read, for every map, so all
four collapse to one expression. A separate canonicalizer could now only be the identity, which is why it
is deleted rather than kept as a no-op."* **CITED**.

⚠ **A limit, stated so it is not over-claimed.** *"Non-ground canonical identity is SYNTACTIC — byte-lex
over the entries' escape-arm encodings — and NOT semantic. Two non-ground entries can be different terms
that match the same things, and no representation collapses those, because pattern equivalence is
undecidable in general. The gain is order-insensitivity and deduplication, which is real."* **CITED**.

**Authority.** Same owner ruling as **CBR-011** (2026-07-28). The commit records: *"⚠ THE VERSION
CONSTANT IS DELIBERATELY NOT TOUCHED … Bumping it is a network-coordination act that belongs to
F1r3node, not a code act … The code is live either way; there is no feature flag, no cargo feature, no
env var and no dual path anywhere in this change."* **CITED**.

#### Evidence

- A measured finding that **contradicted the plan** is recorded in the commit rather than smoothed over:
  `decode_trie_path` **is partial** on `encode_trie_path`'s image. **MEASURED** — and it is the reason
  **CBR-028** exists.
- `bincode_encoder_space` measures the consequence of deleting the arena: a ground map's steady-state encode
  allocates **zero** bytes, down from one allocation. **MEASURED**.

---

### CBR-013

**The two bulk trie readers collapse to one, and the survivor reads the KEYS.**

| | |
|---|---|
| Commit(s) | `c705776c` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | MECHANISM-ONLY |
| Files | `models/src/rust/pathmap_crate_type_mapper.rs`, `models/src/rust/pathmap_integration.rs` |

#### (a) The issue

Two bulk readers existed: one produced every `EPathMap` the reducer hands back to a program, the other
produced the serde / event-hash preimage. They agreed **only while**
$`\forall (k,v).\; \mathrm{encode\_trie\_path}(v) = k`$ — so the tree carried an invariant, a
`debug_assert`, a divergence type and a renderer whose whole job was to keep two answers to one question
in agreement. That invariant had already been violated twice.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — only for a map holding a **root-key** entry, which the value reader kept and the key reader skips. |
| 2 · verdict | **MOVES** — same population. |
| 3 · bytes (Lane B) | NO — *"Every committed golden is unchanged: prost, bincode, serde-JSON, the produce/consume EVENT HASHES, the sorter canonical forms, and the Par byte goldens."* **CITED**. |
| 3 · bytes (Lane P) | NO — same. |
| 4 · post-state hash | NO — same. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

**The disagreement.** `PathMap::iter()` **yields** a value at the empty (root) key while `to_next_val()`
**skips** it. A map holding a root value would be reported by the old value-side reader and dropped by
the new key-side one. ★ That population is empty in practice — `encode_trie_path` never emits an empty
key — which is why the grade is MECHANISM-ONLY rather than WITNESSED. Also: *"$`\mathtt{decode} \circ \mathtt{encode}`$ is the
codec's canonical fixed point, so an entry read through its key comes back recursively canonical, nested
maps included. The value side reproduced whatever order its producer used."* **CITED**.

**Blast radius.** Maps containing a root-key entry (unreachable via `encode_trie_path`) and nested maps
whose inner ordering was producer-dependent. Reachable by an ordinary deploy: **the nested-canonicity
half, yes**; the root-key half, no.

**Could live chain state have been produced under the old behaviour?** The goldens are unchanged, so
**no bytes** differ; the residual question is whether a nested map's *reported* contents differ. Settling
query: as **CBR-012**. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Two answers to one question, kept in agreement by an assertion —
which is exactly the shape that had already produced two live defects. ★ *"The reducer's answer and the
event-hash preimage are now ONE TRAVERSAL rather than two that must be kept in agreement."* **CITED**.

**★ An honest weakening is recorded rather than hidden.** The differential test
`the_two_trie_readers_agree_on_every_subset` was **renamed** to
`the_bulk_converter_is_the_key_walk_on_every_subset`, *"because that is now weaker as a differential and
saying so is better than letting the name promise a check it no longer performs."* **CITED**. Every stale
statement of the old two-reader story was corrected in place.

**Authority.** Same owner ruling as **CBR-011**; this is stage S3. The commit states: *"No re-encoding,
so no consensus version bump is needed and none is made."* **CITED**.

#### Evidence

- 3618 passed / 37 skipped, workspace — **identical to `478102a4`**. **CITED**.

---

### CBR-014

**`EZipper.cursor_kind` — a new protobuf field; four bytes appended on Lane B.**

| | |
|---|---|
| Commit(s) | `7dcff96f` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/main/protobuf/RhoTypes.proto`, `models/src/lib.rs`, `models/src/rust/pathmap_integration.rs`, `pathmap_zipper.rs`, `canonical_path.rs`, `spliced_event_bytes.rs`, `sorter/expr_sort_matcher.rs`, `rholang/src/rust/interpreter/reduce.rs`, `fused_pathmap_chain.rs` |

#### (a) The issue

`EZipper.current_path` stores per-element segments but not the split/bare discriminator (see
**CBR-010**), so a cursor's identity was **lossy**. `cursor_kind` (a `uint32`) makes it explicit, with
four values — `SPLIT` (0), `BARE` (1), `PREFIX` (2), and one more — and `CursorKind::from_wire`
**rejects** an unknown value rather than coercing it.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — `descendFirst` / `descendIndexedBranch` / `toNextSibling` / `toPrevSibling` now reach **BARE** entries, which they never could. |
| 2 · verdict | **MOVES** — same population. |
| 3 · bytes (Lane B) | **MOVES** — `ezipper.bincode.bin` **833 $`\rightarrow`$ 837**: four zero bytes appended at the end. Nothing before offset 833 moved. `ezipper.json` **5090 $`\rightarrow`$ 5110**. **CITED**. |
| 3 · bytes (Lane P) | **NO** — ★ *"`ezipper.protobuf.bin` UNCHANGED. prost omits a default-valued scalar, so the CONSENSUS encoding of every pre-existing zipper is byte-identical, and every EZipper serialized before this field decodes to exactly the prior semantics."* **CITED**. |
| 4 · post-state hash | **MOVES** — the event hash reads the bincode lane. ⚠ But see below: **every** event-hash golden is unchanged, because the `EZipper` fixture is not in them. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

★ **This is the clearest illustration in the register of why the two lanes must be answered
separately**: the same field addition moves Lane B by four bytes and moves Lane P by zero.

**The disagreement.** A deploy that stores an `EZipper` in the tuplespace produces a cold-store record
four bytes longer on a new node, and an event hash over a datum containing an `EZipper` differs. Safety
fork on the bincode lane; **no** disagreement on the block wire.

**Blast radius.** Programs that put an `EZipper` on a channel. Reachable by an ordinary deploy: **yes**.
Ground-list navigation is byte-identical, *"because a map with no bare entries makes the PREFIX probe
miss and fall through to exactly the split key."* **CITED**.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: scan historical event-hash preimages and cold-store records for an
`EZipper`. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A cursor that cannot say which of two entries it is focused on
must guess, and four navigation methods could not reach bare entries at all.

**Why an ADDITIVE field rather than a trailing pseudo-segment.** *"Category C decides it: 24 sites index
the cursor POSITIONALLY BY ELEMENT, and a trailing `[0x00]` pseudo-segment would appear as an extra
'element' in every one of them. An ADDITIVE field touches none of them."* **CITED**.

**Why `uint32` and not a proto `enum`.** `models/build.rs` applies
`.enum_attribute(".rhoapi", "#[derive(Eq, Ord, PartialOrd)]")` and `#[repr(C)]` to every enum in the
package (load-bearing for the generated `oneof`s), and both collide with prost's own derives.
Introducing this package's first proto enum would mean restructuring the pass that rewrites the derives
of **every** generated type — a consensus-critical codegen path — *"to gain nothing on the wire, since a
proto3 enum field and a uint32 field are the same varint."* **CITED**.

**★ The sort score tree is deliberately NOT extended**, and the reason is exactly the review criterion:
*"adding a leaf would move the score of every zipper-bearing Par."* `cursor_kind` is propagated through
the sorted clone — so sorting cannot silently reset a cursor — and left out of the score. **CITED**.

**Authority.** No owner ruling.

#### Evidence

- Golden-by-golden accounting, **MEASURED** and quoted: prost unchanged; bincode 833 $`\rightarrow`$ 837 with nothing
  before offset 833 moved; JSON 5090 $`\rightarrow`$ 5110 with nothing before it moved; **every other golden**
  (`e6a_index`, `locally_free`, `nested`, `remainder_connective`) unchanged across all three encodings;
  every event-hash golden unchanged; **zero insert-side bytes**.
- `spliced_event_bytes::emit_ezipper` mirrors the new field so the hand-rolled event-hash emitter stays
  byte-identical to `bincode::serialize`. **CITED**.
- `navigation_reaches_both_arms` asserts both halves. **CITED**.

---

### CBR-024

**`last` joins the method table.**

| | |
|---|---|
| Commit(s) | `2fee67fa` |
| Status | LANDED |
| Direction | PERMISSIVE |
| Evidence grade | WITNESSED |
| Files | `rholang/src/rust/interpreter/reduce.rs` |

#### (a) The issue

Rholang had no way to name the last element of a sequence. `last` is added to the reducer's
`method_table` (55 entries).

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A — no existing expression changes value. |
| 2 · verdict | N/A |
| 3 · bytes (Lane B) | N/A |
| 3 · bytes (Lane P) | N/A |
| 4 · post-state hash | N/A |
| 5 · accepted programs | **MOVES** — `xs.last()` previously raised "method not found" and now evaluates. |
| 6 · metering | N/A — under the token model (§2.2) a method call is a diagnostic `Primitive` event contributing zero consensus cost; no committed COMM count and no funding surface moves. Re-derived 2026-08-03. |

**The disagreement.** `[1,2,3].last()` fails on an old node, evaluates to `3` on a new one. Because the
old behaviour is a deterministic error, this is a slashable-fault class rather than a silent fork.

**Blast radius.** Programs calling `.last()`. Reachable: **yes**, but only by programs written after the
feature exists — no *existing* program can call it.

**Could live chain state have been produced under the old behaviour?** **No** — a method that does not
exist cannot have succeeded. **DERIVED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Nothing, in the safety sense. This is a capability addition, and
it is in the register because *acceptance is an axis*.

**The historical pricing decision.** Under the phlo-era model this entry deliberately reused
`nth_method_call_cost()` rather than minting a price (*"pricing is a consensus decision this change
does not make"* — **CITED**). Under the token model that constant is a diagnostic weight: the
decision survives as diagnostic-label hygiene (`last` never drifts from `nth`'s label), and the
consensus content of this entry is the **acceptance** axis alone.

**Authority.** No owner ruling. *"⚠ NO CONSENSUS VERSION BUMP. `Validate::version` is exact equality …
so a bump is not this change's to make."* **CITED**.

#### Evidence

- ★ The **discriminator** test, and why a weaker fixture would have been worthless:
  `eval_of_last_method_is_the_final_element_and_not_the_first` asserts `[111, 222, 333].last()` is `333`
  while the *same* list's `.nth(0)` is `111`, in one test, plus an explicit `assert_ne!`. *"A
  `[1].last() == 1` fixture would pass under BOTH the last- and first-element readings and assert
  nothing."* **CITED**.
- `eval_of_last_method_on_the_empty_list_agrees_with_nth_zero_exactly` compares the two errors **for
  equality**, not each against a pattern. **CITED**.
- Carrier coverage (tuple, byte array, empty byte array) and refusals (arity, wrong carrier) both named.
  **CITED**.

---

### CBR-025

**Trie enumeration over `EPathMap`: `getPath`, `toNextLeaf`, `leafCount`.**

| | |
|---|---|
| Commit(s) | `98d2422d` |
| Status | LANDED |
| Direction | PERMISSIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/pathmap_native_query.rs`, `models/src/rust/pathmap_zipper.rs`, `rholang/src/rust/interpreter/reduce.rs` |

#### (a) The issue

An `EZipper` could navigate a path map but could not report *where it was*, step to the next leaf, or
count leaves. Three methods are added.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A — *"NO change to how any existing method behaves."* **CITED**. |
| 2 · verdict | N/A |
| 3 · bytes (Lane B) | N/A — no `.proto` change, no new `ExprInstance`, no new field on any message. |
| 3 · bytes (Lane P) | N/A — same. |
| 4 · post-state hash | N/A |
| 5 · accepted programs | **MOVES** — the method table grows by exactly three entries. |
| 6 · metering | N/A — the three charge sites are diagnostic `Primitive` events under the token model (§2.2); zero consensus cost. Re-derived 2026-08-03. |

★ *"THE CONSENSUS SURFACE CHANGE IS ADDITIVE ONLY — verifiable without reading the diff twice."*
**CITED**. `getPath` only **reads out** a field already on the message and already serialized
(`RhoTypes.proto:352`, `repeated bytes current_path`), decoding it with the same `decode_trie_path` codec
the file already uses.

**Blast radius.** Programs calling the three new methods. **Could live chain state have been produced
under the old behaviour?** **No.** **DERIVED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A trie you cannot enumerate is a trie you cannot use as one.

**Why nothing new was built.** All three surface capability the `pathmap` crate already had
(`ZipperMoving::path()`, `ZipperIteration::to_next_val()`, `ZipperMoving::val_count()`). The only
reachability change in `models` is that one previously-dead decoder (`unflatten_segments`, carrying
`#[allow(dead_code)]`) becomes live — it is the inverse of `flatten_segments`. **CITED**.

**The historical metering derivation** (phlo-era, retained as diagnostic-path documentation): the
proportional `leafCount` charge used `reserve_incremental_primitive` because `reserve_primitive`
rejects a non-positive charge with `BugFoundError` — measured, not assumed. **CITED**. Under the
token model these reservations are diagnostics; the consensus content of this entry is the
**acceptance** axis alone.

**Authority.** No owner ruling.

#### Evidence

- `subtrie_value_count` also replaces the `collect_subtrie_values(..).len()` shape, *"which cloned every
  `Par` in a subtrie only to discard them."* **CITED**.

---

### CBR-027

**GInt `+` and `-` stop wrapping on overflow.**

| | |
|---|---|
| Commit(s) | `6ff46f8a` — *fix(reduce)!: Int `+` and `-` are CHECKED — the reducer no longer wraps* ⚠ **does not compile as committed**; `fd5474ab` — *fix(reduce): repair the arms `6ff46f8a` scattered — it parsed but could not compile* |
| Status | LANDED (`fd5474ab`; ⚠ `6ff46f8a` does not compile as committed — both hunks landed in the wrong `match` arms, ten E0425 — and `fd5474ab` rolled forward with the placement repair; recorded for bisecting reviewers) |
| Direction | REGRESSIVE |
| Evidence grade | WITNESSED |
| Files | `rholang/src/rust/interpreter/reduce.rs` at `fd5474ab` — `combine_plus`'s `GInt` arm at **3396–3404** (`checked_add` at **3398**), `combine_minus`'s at **3516–3524** (`checked_sub` at **3518**), the shared rationale comment at **3491–3502**. **MEASURED** (file read at that ref). |

#### (a) The issue

The reducer's `GInt` addition and subtraction use `i64::wrapping_add` / `wrapping_sub`, so
$`2^{63}-1 + 1`$ silently evaluates to $`-2^{63}`$. **Multiplication and unary negation on the same
type are CHECKED** and raise `ReduceError`, as is integer division by zero
(`"Division by zero"`) and $`\mathrm{i64::MIN} / -1`$ (`"Arithmetic overflow in division"`) —
verified in the tree. **DERIVED**. So the reducer is internally inconsistent; and F1r3node's own guard
evaluator disagrees with F1r3node's own reducer on the same expression, a contradiction an in-tree
conformance test already records. **CITED**.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — ★ this is the entry. `i64::MAX + 1` stops being `i64::MIN` and becomes an error. |
| 2 · verdict | **MOVES** — the deploy fails where it previously produced a (wrong) value. |
| 3 · bytes (Lane B) | **MOVES** — a deploy that fails writes a different deploy log. |
| 3 · bytes (Lane P) | **MOVES** — same. |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | NO — normalization is unchanged; the *reduction* fails. |
| 6 · metering | NO — per-op arithmetic costs are diagnostic under the token model (§2.2), and no funding/settlement surface moves; the COMM-count consequence of the new failure path is the axis-1/2 divergence already recorded, not a metering change. |

**The disagreement.** `@"out"!(9223372036854775807 + 1)` sends `-9223372036854775808` on an old node and
**fails the deploy** on a new one. Since the failure is deterministic across validators, this is a
slashable-fault class in a mixed-version network rather than a silent fork.

**Blast radius.** Any deploy performing 64-bit integer addition or subtraction that overflows. Reachable
by an ordinary deploy: **yes, trivially.**

**Could live chain state have been produced under the old behaviour?** ⚠ **This is the entry where the
question is sharpest**, because the direction is REGRESSIVE and the old behaviour produced a *value* that
may be committed. Settling query: replay the chain under an instrumented build counting `GInt` `+`/`-`
evaluations where `checked_add`/`checked_sub` would return `None`. **Any non-zero count is a program
whose historical result this change would alter.** **UNVERIFIED**, and it should be run before this
change ships.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Silent wraparound in a financial-contract language, on the two
most-used arithmetic operators, while the neighbouring operators on the same type refuse. A contract
computing a balance can produce a negative number from two positive ones and nothing reports it.

**Why checked-with-an-error rather than saturating, promoting, or matching upstream.** The alternatives
were put to the owner explicitly and two were declined:

- *Match upstream for now and file the bug* — declined. It reproduces a defect already identified as one.
- *Fix it behind an explicit divergence record* — declined in favour of the plain fix.

★ **A qualification this register owes the reviewer, and which the campaign itself flagged.** This is the
only change in the campaign that *alters computed values on a consensus lane by design* rather than by
correcting an outright defect, and it is where two of the owner's own standing rules pull against each
other: **"we can do better than upstream"** and **"semantics may not diverge"**. The campaign's own
assessment, recorded before the ruling: *"it's the one where 'we can do better than upstream' and
'semantics may not diverge' genuinely pull against each other, and you resolved it toward correctness."*
**CITED** (campaign ledger, 2026-07-29).

**Authority — the ruling, verbatim, with its date.**

> **2026-07-29T17:53:39Z** — asked *"Under your rule this is an upstream BUG. But fixing it CHANGES
> COMPUTED VALUES,"* the owner selected:
>
> **"Fix it — checked, with a clear error"**
>
> described in the option as: *"Make `+` and `-` consistent with `*` and unary `-`: checked arithmetic
> raising a `ReduceError` that names the operation and the operands. ⚠ This changes computed values for
> any program that overflows, so it is a consensus change, not a diagnostic one — the same class of
> ruling as #148."*

The governing general rule, **2026-07-29T16:55:56Z**:

> *"We can handle errors better than upstream Rholang, do not necessarily restrict your options to what
> upstream supports. We should support everything upstream supports correctly, but should fix any bugs
> that upstream has and make it more debuggable (e.g. better error handling, more specific and clearer
> error messages, etc.)"*

#### Evidence

- The pre-change inconsistency is **DERIVED**, pinned at `61a53157` (the last commit before the
  repair): `wrapping_add` at `rholang/src/rust/interpreter/reduce.rs:3397`, `wrapping_sub` at
  `:3489`, beside checked division-by-zero and $`\mathrm{i64::MIN}/-1`$ guards in the same `match`
  family.
- The overflow error message is a **pure function of the term** (operands are the two `GInt`
  payloads; no environment read), so it carries no ordering nondeterminism onto block-resident
  bytes. **DERIVED** at `fd5474ab`.
  **CBR-016**'s determinism obligation"* — **came true**, and is discharged below.

#### Drift check at the landing commit (2026-07-29)

This entry said **IN FLIGHT — not present in the tree at the time of writing** for less than a day.
`6ff46f8a` landed it, and the entry did not move with it. What follows is the re-derivation, and the
first two rows are the ones the entry's own closing paragraph asked for.

| re-check item the entry named | disposition at `6ff46f8a` |
|---|---|
| Is the error raised **before** the cost reservation ($`\Rightarrow`$ metering cell becomes MOVES)? | ★ **NOT DISCHARGEABLE at this commit** — see the compile finding below. The `GInt` arm of `combine_plus` at `6ff46f8a:3394-3398` still reserves `sum_cost()` and contains **no** checked call at all, so there is no ordering to read. The metering cell stays `NO` on the strength of the *design*, and is **UNVERIFIED against code** until the repair lands. |
| Does the message include the operands ($`\Rightarrow`$ **CBR-016** determinism obligation)? | **YES — and the obligation is discharged.** The committed strings are `"Arithmetic overflow in addition: {lhs} + {rhs} is not representable as an Int (64-bit signed)"` and the subtraction twin. `lhs` and `rhs` are the `i64` payloads of the two `ExprInstance::GInt`s in the term, so the message is a **pure function of the term** — no environment read, no `PRETTY_PRINTER_OUTPUT_TRIM_AFTER`, no host-local ordering. It reaches a block only through `SystemDeployPlatformFailure::UnexpectedSystemErrors`, whose `Display` is `"Caught errors in Rholang interpreter {:?}"`, i.e. `Debug` on `Vec<InterpreterError>`; that is environment-independent for the same reason. **DERIVED**. |

⚠⚠ **And a finding the entry could not have predicted: `6ff46f8a` as committed does not compile.**

Both hunks landed in the **wrong `match` arm**. At `6ff46f8a:3503-3513` the `checked_add` block — whose
body names `lhs` and `rhs` — sits inside the following (⚠ tagged `text`, not `rust`: the `…` is this
report's elision of the `ok_or_else` body, and the block is quoted *because* it does not compile, so
demanding that it parse as Rust would be demanding that the defect not be a defect):

```text
(ExprInstance::GBigInt(b1), ExprInstance::GBigInt(b2)) => {
    let result = lhs.checked_add(rhs).ok_or_else(|| { … })?;
    self.metering
        .reserve_primitive(bigint_subtraction_cost(b1.len(), b2.len()))?;
    make_bigint_expr(subtract_twos_complement(&b1, &b2), "-")
}
```

where the bindings are `b1` and `b2`. `checked_sub` at `:3626` landed inside the `==` operator's body
the same way. The result is **ten `E0425 cannot find value` errors** and
`error: could not compile 'rholang' (lib)` — a **hard** compile error, not a lint, so
`-D warnings` is irrelevant to it. **MEASURED**:
`RUSTFLAGS="-C target-feature=+aes,+sse2 -D warnings" cargo check --release -p rholang --all-targets`
against an export of the ref, and confirmed by reading the file at the ref directly. A repair was in the
working tree at the time that paragraph was written — it *moves* both hunks into the `GInt` arms of
`combine_plus` / `combine_minus` — and was **uncommitted**, so it was not citable and this row was owed a
re-derivation when it landed. ★ **It has landed, as `fd5474ab`, and the re-derivation follows.**

#### The repair (`fd5474ab`), and what it discharges

**One sentence: `fd5474ab` moves `6ff46f8a`'s two checked-arithmetic blocks into the `GInt` arms they
were written for, so the intent of `6ff46f8a` compiles for the first time.**

⚠ **A broken commit in history is a fact a bisecting reviewer needs, and this register states it rather
than smoothing it over.** `git bisect` over any range spanning `6ff46f8a..fd5474ab^` will hit a commit
where `cargo check -p rholang` fails outright; a bisect script that treats a build failure as
*"skip"* will silently drop it, and one that treats it as *"bad"* will blame it for whatever it was
actually bisecting. Neither reading is wrong about the commit — the commit really is broken — but a
reviewer who does not know it will spend the failure budget on the instrument instead of the defect.
**The behavioural content of the entry is unchanged**: `6ff46f8a` and `fd5474ab` together are one change,
and the axis table above describes that change. No axis cell moves *because of* the repair; one cell
becomes readable.

**How the damage happened, and why it is a methodological finding rather than a slip.** `reduce.rs` was
shared with a concurrent agent's uncommitted work (roughly +340 lines earlier in the file), so the two
arms were staged by **filtering `git diff -U0` hunks by content** and applying them with
`git apply --cached --unidiff-zero`. Zero-context hunks carry **no anchor**, and the `+`-side line numbers
had been computed against the *worktree*; applied to a HEAD-based file they landed about 107 lines too
low, scattering six hunks across sibling `match` arms. **CITED** (`fd5474ab`). ★ The finding: the
verification step that was run — `rustc -Zparse-crate-root-only` — **accepted the damaged file**, because
scattering statements across sibling `match` arms breaks *name resolution*, not *parsing*. An instrument
chosen for a defect class it cannot detect reports green. That is the same shape as
[§6.2](#62--known-false-claim-in-a-shipped-commit--disclosed-and-now-measured)'s finding and as the two
mutation experiments of [§8](#8-conclusions) conclusion 3 that *"reported green because they had not
applied"*.

★ **How the repair was rebuilt — anchor-based, so the failure mode is not reachable.** `6ff46f8a^`'s
`reduce.rs` was taken and its two `wrapping_add(rhs)` / `wrapping_sub(rhs)` `GInt` arms replaced by
**exact, asserted-unique string match**. No line numbers participate. It was staged with
`git hash-object -w` plus `git update-index --cacheinfo`, so the working tree was never modified and the
concurrent agent's edit was undisturbed. **CITED**.

**What the repair discharges — the row [§6.4](#64-unverified-budget) was waiting for.**

| obligation | disposition at `fd5474ab` |
|---|---|
| **Axis 6 (metering) must return to `NO` by measurement.** | ★ **DISCHARGED.** `combine_plus`'s `GInt` arm reads `self.metering.reserve_primitive(sum_cost())?;` at `:3397` and `let result = lhs.checked_add(rhs)` at `:3398`; `combine_minus`'s reads `reserve_primitive(subtraction_cost())?` at `:3517` and `checked_sub` at `:3518`. The reservation therefore **precedes** the fallible arithmetic in both arms, exactly as the design claimed, so an overflowing addition is charged identically to a succeeding one and no charge or ordering moves. `sum_cost()` and `subtraction_cost()` are both `Cost::create(3, …)` (`accounting/costs.rs:91`, `:93`) and neither was touched. **DERIVED** (both refs read directly). The summary-table cell moves `?` $`\rightarrow`$ `○` and the `UNVERIFIED` budget falls from **2 to 1**. |
| **The `Files` cell must name coordinates that exist.** | **DISCHARGED** — the header table now cites `fd5474ab` coordinates and records the `6ff46f8a` ones as the *wrong-arm* positions they were, so both refs are readable rather than one being silently overwritten. |
| **Does the error message stay a pure function of the term?** | **UNCHANGED and re-verified.** `fd5474ab` reports the two arms are **byte-identical** to the working-tree text that compiled and passed `reduce_spec` 133/133 and `rholang_numeric_eval_spec` 25/25 — 158 tests — established by extracting each arm from both files and comparing. **CITED.** The strings are the same ones the drift-check row above analysed, so the **CBR-016** determinism obligation stays discharged. |
| **The chain-history query** (`checked_add`/`checked_sub` would return `None` on replay). | ⚠ **STILL NOT RUN.** `fd5474ab` does not touch it. It remains the register's most overdue obligation and is why this entry still ranks third in [§5.2](#52-the-highest-risk-entries). **UNVERIFIED.** |

⚠ **Counting the broken commits: this is the first of two in the register.** The second is `2eebf722`,
recorded in **[CBR-L09](#b1-retired-register-entries)**, which committed a *deliberately-broken RED probe* as if it were a
fix. The two share neither a repository nor an author's intent, but they share a mechanism —
**a staging technique that moves content without an anchor, used on a file another agent was editing** —
and that is worth naming once as a class rather than twice as an accident. Neither was caught by CI,
because in both cases the commit that broke the build and the commit that repaired it landed inside the
same CI window.

★ **The methodological lesson, recorded because this campaign has now paid for it twice in one day.**
The first attempt to establish the compile state used `git archive <ref>` into a scratch directory —
correct in refusing to mutate the working tree, and **insufficient**: the root `Cargo.toml` carries a
`[patch]` section marked `HELD LOCAL — DO NOT COMMIT` pointing at an unpublished parser worktree that the
`cost_accounting` normalizers require, so an archived ref **cannot** compile until the overlay is copied
in **and** its three relative `path =` deps are made absolute or the export is re-parented. A baseline
that does not build measures nothing. *When a premise looks refuted, suspect the instrument once before
suspecting the claim* — and say which instrument was cleared. Here both were: dependency resolution
succeeded (so the overlay was in force) and the failure was in `rholang`'s own source (so the archive was
faithful), which is what promotes this from a suspected instrument fault to a measured defect.

#### ★ This entry is the gate's first witness

[§7.2](#7-maintenance) specifies a drift gate and records it as **DESIGNED, NOT BUILT**;
[§7.4](#7-maintenance) requires it to be shown RED before it is trusted.
**This entry is a real member of the class it is meant to catch**, and it is worth more than a synthetic
cell:

| gate clause | would it have fired? |
|---|---|
| 2 · **coverage** ($`\mathcal{O} \subseteq \mathcal{E} \uplus \mathcal{X}`$) | **YES.** `6ff46f8a` touches `rholang/src/rust/interpreter/reduce.rs`, a consensus-critical path, and appeared in no `[[entry]]` and no `[[exempt]]` row. It would have failed *naming the SHA*, which is exactly the requirement of [§7.1](#7-maintenance). |
| 5 · **complete axis answers** | No — all seven cells were present. The staleness was in `Commit(s)`, `Status` and `Files`, which no specified clause reads. ⚠ **A gap in the design**, recorded here: an entry can be *stale about which commit it describes* and pass every clause of §7.2. |
| 6 · **UNVERIFIED budget** | Not at the drift; **YES** once the metering cell became `UNVERIFIED against code`; and **YES again**, in the other direction, when `fd5474ab` returned it to `NO`. The budget went $`1 \to 2 \to 1`$ over two days, and each step is a visible diff. That is the clause working in both directions, which is stronger evidence than a one-way trip: a budget that can only rise is a ratchet, not a measurement. |

**Time to drift: under one day**, in a document whose §7 is titled *"how an omission fails loudly"*. It
did not fail loudly; it was found by a reader who happened to be measuring something else. That is the
argument for building the gate, restated as an incident rather than as a principle.

★ **And the entry drifted a second time, in the same way, before the gate was built.** `fd5474ab` landed
the repair; this entry did not move with it, and its `Status` read *"LANDED ⚠ and the landed commit does
not compile"* — a statement that had become false — until this revision. Clause 2 would **not** have fired
the second time, because `fd5474ab` touches a file already covered by this entry's row; the clause that
would have fired is the proposed **clause 9 (citation freshness)** of
[§7.4](#7-maintenance), because the `Files` cell named `checked_add` at
`3504` — a coordinate that resolves, at the entry's own newest SHA, to a line that no longer contains it.
**That is a second, independent witness for clause 9, and it is the first witness that clause 2 alone is
not sufficient.**

---

### CBR-030

**`NonNegativeNumber.rho`'s `add` stops detecting overflow by observing the wrap, and the genesis term moves.**

| | |
|---|---|
| Commit(s) | `e3a4494b` — *fix(genesis)!: NonNegativeNumber's `add` guards the way its own `sub` does*; `719f2432` — *test(casper)!: RETRACT `e3a4494b`'s genesis-hash table — the instrument is run-varying, and the normalized term is not* |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `casper/src/main/resources/NonNegativeNumber.rho:32` (the new total guard; **:22** before the edit), the sibling `sub` guard at **:46**, and the `lastNonce` occurrences of the same literal at **:8** and **:64**. The pin: `casper/tests/genesis/contracts/genesis_overflow_guard_shape.rs` (new in `719f2432`, 71 lines). **DERIVED** (read at `HEAD` and at `e3a4494b`). |

#### (a) The issue

The genesis contract `NonNegativeNumber.rho` detected addition overflow by **observing the wrap**:

```text
contract this(@"add", @x, success) = {
  if (x >= 0) {
    for(@v <- @(*MergeableTag, *valueStore)){
      if (v + x >= v) {                                  // ⚠ NonNegativeNumber.rho:22, before
        @(*MergeableTag, *valueStore)!(v + x) | success!(true)
      } else {
        //overflow
        @(*MergeableTag, *valueStore)!(v) | success!(false)
      }
    }
  }
}
```

`v + x >= v` can be false **only when `v + x` has already wrapped** round to something smaller than `v`.
The guard is therefore not merely stylistically odd — it is *defined* in terms of the reducer's wrapping
behaviour, and it is reachable only because that behaviour existed. **CBR-027** removed it: `6ff46f8a` /
`fd5474ab` made `GInt` `+` **checked**, so an overflowing `v + x` now raises `ReduceError`. ⚠ A guard that
must *evaluate* `v + x` in order to learn whether `v + x` is representable cannot work under checked
arithmetic **by construction** — the sum it needs as evidence is the sum that aborts the deploy.

⚠ **This is the composition hazard [§5.5](#55-the-conjunction-risk) is about, and it is the register's
first concrete instance of one.** Neither **CBR-027** nor this entry is dangerous in isolation.
**CBR-027** alone leaves a genesis contract whose overflow path aborts the deploy where it previously
reported `false`; this entry alone would be an unmotivated rewrite. Together they are a repair. A review
that weighed them independently would have found nothing wrong with either.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — against the immediately-preceding code (**CBR-027** landed, this not yet), an overflowing `balance!("add", …)` raised and now answers `success!(false)` with the balance left at `v`. ⚠ Against the code *before* **CBR-027**, the observable answer is the **same** `false` — this entry restores what `+`'s checking took away. Both comparisons matter and they differ, which is why the mechanism paragraph names its baseline. |
| 2 · verdict | **MOVES** — the `if` decides on `x <= 9223372036854775807 - v` instead of `v + x >= v`, so the branch is selected by a different predicate; and on an overflowing input the reducer no longer raises out of the `if` at all. |
| 3 · bytes (Lane B, bincode) | **MOVES** — the genesis deploy carries the contract's **source term**, and its normalized `Par` changes: blake2b256 `d9ce2e4d…` (length 2653) $`\rightarrow`$ `a537547892…` (length 2652). **MEASURED** (`719f2432`). |
| 3 · bytes (Lane P, prost) | **MOVES** — the same normalized `Par` is what is protobuf-encoded; the length figures above *are* the prost encoding's length. |
| 4 · post-state hash | **MOVES** — the stored continuation differs, so the genesis checkpoint root differs. ⚠ **The magnitude is real but the genesis `post_state_hash` is not a sound instrument for it** — see the evidence section. The block hash moves too, and the deploy signature over the term is recomputed at build time. |
| 5 · accepted programs | NO — the edited contract still normalizes and deploys. **MEASURED**: 14 genesis deploys, `is_failed = false` on all 14, both before and after (`e3a4494b`). |
| 6 · metering | NO — the guard's charge is unchanged: `sum_cost()` and `subtraction_cost()` are both `Cost::create(3, …)` (`accounting/costs.rs:91`, `:93`) and `comparison_cost()` is `Cost::create(3, …)` (`:112`), so one arithmetic operation plus one comparison costs 6 in both forms. **DERIVED**. ⚠ The *deploy's total* does differ on an overflowing input, because the new form completes the `else` branch where the old form aborted — but that is Axis 1/2 propagating into a total, not a charge site or a price moving, and this register reads Axis 6 as the latter (see the note under [§2.4](#24-consensus-breaking-defined--and-the-six-axes)). |

**The disagreement.** Two nodes disagree **at genesis, on every chain built from this source tree**, and
they disagree about the *root itself* rather than about the outcome of some later deploy: a node built
before `e3a4494b` and a node built after it normalize `NonNegativeNumber.rho` to different `Par`s, commit
different genesis post-states, and therefore reject each other's genesis block. This is a **safety fork**
of the strongest available kind — not "under some input", but *unconditionally*, before any user deploy
exists. It is also the least alarming kind, for the reason below.

**Blast radius.** The whole chain, and **only across a genesis boundary**. Every node in a network must
agree on genesis or it cannot join, so this cannot produce a *silent* divergence between running peers: a
mixed-genesis network fails to form rather than forming and then splitting. The class of program is
"every program", and the class of *deploy* that could witness a difference in `add`'s behaviour is any
`balance!("add", x, …)` whose `v + x` exceeds $`2^{63}-1`$.

##### ★★ What does NOT move — the registry URI and the insertion signature, PROVEN rather than assumed

⚠ **This is the most load-bearing negative result in the entry**, because if it were false the change would
require **re-signing a genesis contract** — an operation needing the deployer's private key, which no
reviewer of this document has, and which would put the change out of reach rather than merely making it
expensive.

`NonNegativeNumber.rho` is registered with `rho:registry:insertSigned:secp256k1`, and the signature it
presents is verified against a hash of a **three-element tuple**, not of the contract:

| side | site | what is hashed |
|---|---|---|
| verifier (in Rholang) | `casper/src/main/resources/Registry.rho:586`, inside `contract insertSigned` at **:571** | `blake2b256!((timestamp, deployerPubKey, version).toByteArray(), *hashCh)`, then `secpVerify!(hash, sig, pubKeyBytes, …)` at **:591** |
| signer (in Rust) | `casper/src/rust/util/rholang/registry_sig_gen.rs:206-215` | an `ETuple` of `(args.timestamp, GByteArray(pub_key.bytes), last_nonce)`, protobuf-encoded, `Blake2b256`-hashed |

$`\Rightarrow`$ **The signed tuple is (deploy timestamp, deployer public key, nonce/version) and covers
NEITHER the contract body NOR the `data` payload.** All three components are compile-time constants for
this contract — `NON_NEGATIVE_NUMBER_PK` at
`rholang/src/rust/interpreter/merging/mergeable_tags.rs:29` and `NON_NEGATIVE_NUMBER_TIMESTAMP` at
**:31**, with `last_nonce = Self::MAX_LONG` (`registry_sig_gen.rs:203`) — and none of them is
touched by an edit to the contract's guard. Therefore:

- the **signature remains valid** and no re-signing is required;
- the **registry URI** is unchanged, because it is `build_uri(blake2b256(pubKeyBytes))` — a function of the
  deployer key alone;
- the **`IntegerAdd` mergeable tag's unforgeable name** is unchanged, because it is derived from
  $`(\mathrm{PK}, \mathrm{TIMESTAMP})`$ only, and this edit adds no `new`, so the allocation sequence of
  unforgeable names inside the contract is untouched.

**DERIVED** (all five sites read at `HEAD`). ⚠ **The deploy signature over the term is a different object
and it *does* move** — it is recomputed at build time from
$`(\mathrm{PK}, \mathrm{TIMESTAMP}, \mathrm{term})`$, so it is *derived*, not pinned, and its movement is
a consequence of the term's movement
rather than an independent axis. Confusing the two is the mistake this subsection exists to prevent: one
signature covers the term and is regenerated, the other covers a constant tuple and is not.

**Could live chain state have been produced under the old behaviour?** ⚠ **This question factors into two,
and the answers differ.**

1. *Was the old `add` reachable?* **Yes, trivially** — `NonNegativeNumber` backs `MakeMint`'s `deposit`
   (`MakeMint.rho:133` calls `balance!("add", amount, …)` and branches on the boolean it gets back), so
   every mint deposit runs this guard. But under the **wrapping** reducer the old guard *worked*: it
   returned `false` on overflow. So no wrong answer was committed by the old form on the old reducer.
2. *Is there a window in which the guard was broken?* **Yes — `6ff46f8a`/`fd5474ab` to `e3a4494b`.**
   In that window an overflowing deposit aborts the deploy instead of reporting failure. Settling query:
   the same replay walker **CBR-027** needs — count `GInt` `+` evaluations where `checked_add` returns
   `None` — restricted to deploys reaching `NonNegativeNumber`'s `add`. **UNVERIFIED**, and cheap, because
   the window is a few hours of one day's commits and no network ran in it.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A genesis contract whose overflow guard is inoperative. Concretely:
under checked `+`, an overflowing `deposit` **aborts the deploy** rather than returning `false`, so
`MakeMint`'s caller never receives the boolean it branches on, and the failure surfaces as a system deploy
error rather than as the domain-level "that deposit would overflow" the contract was written to report.
Leaving it would mean shipping **CBR-027** with a known-broken genesis contract downstream of it.

**Why the total form rather than the alternatives.**

| alternative | why rejected |
|---|---|
| **Leave `if (v + x >= v)` and revert `+` to wrapping.** | Reverts **CBR-027**, whose ruling is on the record. It also keeps a guard whose correctness is a property of the *reducer* rather than of the *contract*. |
| **Catch the `ReduceError` in Rholang.** | Rholang has no exception form; there is nothing to catch with. |
| **Promote the accumulator to `BigInt`.** | A carrier change to a genesis contract, and `combine_plus` has **no coercing arm** — `(GInt(_), other)` falls to `OperatorExpectedError` (`reduce.rs:3470-3475`, pinned at `719f2432`; the cell read `:3574-3578` until the drift gate was built, which is the `-` operator's twin arm rather than `combine_plus`'s) — so a mixed `Int`/`BigInt` call fails identically before and after. It would change the contract's interface, not just its guard. **DERIVED**. |
| **Introduce a named `Int` maximum instead of the bare literal.** | Rholang has no named `Int` maximum — zero hits for `maxint` / `max_int` / `MAX_VALUE` / `Int.max` in any `.rho` file — so this would be a **language** change. The same literal already appears twice in this very file as the registry `lastNonce` (`:8`, `:64`). **MEASURED** (`e3a4494b`). |

★ **The primary justification is fourteen lines below the change, in the same contract.** `sub` was
**already** written in the total form — `if (x <= v)` decides a predicate *before* evaluating `v - x`
(`NonNegativeNumber.rho:46`). `add` was the **outlier in its own contract**, and this makes the two
consistent. That is a stronger argument than any external principle, because it means the fix is not a new
convention imposed on the file but the file's own convention applied to the one place that had escaped it.
`sub` was separately verified safe under checked arithmetic: with $`0 \le x \le v`$ the difference
$`v - x`$ lies in $`[0, v]`$, so it can neither underflow nor overflow, and its false branch restores `v`
exactly as `add`'s does. **DERIVED**.

**Why the total form is total — the proof obligation, discharged.** The new guard evaluates
$`2^{63} - 1 - v`$, which must not itself overflow or underflow:

- $`v \ge 0`$. The initializer's `match init { Int => … ; _ => … }` stores an `Int` **or else** `0`, so
  the value store never holds another carrier and never holds a negative — the contract's own name is its
  invariant. **DERIVED**.
- $`x \ge 0`$ by the enclosing `if (x >= 0)`, which is unchanged.
- Hence $`0 \le v \le 2^{63}-1`$ gives $`0 \le 2^{63}-1-v \le 2^{63}-1`$: representable, so the
  subtraction cannot fail and the comparison **always decides**.

Formally, the guard is the predicate $`P(v, x) \equiv x \le (2^{63}-1) - v`$, which is equivalent to
$`v + x \le 2^{63}-1`$ over the integers but — unlike the old form — is computed entirely inside the
representable range:

```math
P(v,x) \iff v + x \le 2^{63}-1
\qquad\text{for all } v, x \in \bigl[0,\, 2^{63}-1\bigr]
```

**Sibling enumeration — the count is ONE.** `rg 'if\s*\([^)]*\+' casper/src/**/*.rho` returns three hits:
this one, and `ListOps.rho:197` / `:236` (`if (sc == cc + 1)`), which compare a completion counter against
a list length and are **not** wrap detects. `MakeMint.rho:133`'s `deposit` does not do its own wrap
detection — it calls `balance!("add", amount, *addSuccessCh)` and branches on the boolean that comes back,
so it is *downstream* of this one fix rather than a second instance of it. **MEASURED** (`e3a4494b`).

**Authority — the ruling, verbatim, with its date.**

> **2026-07-29** — *"okay, use `if (x <= 9223372036854775807 - v)`, that is the right way"*.

The governing general rule is **CBR-027**'s, quoted there: upstream is a floor on semantics, and its bugs
are to be fixed rather than reproduced. This entry is the downstream consequence of acting on it.

#### Evidence

⚠⚠ **THE GENESIS HASHES ARE NOT CITED HERE, AND THE REASON IS THE MOST IMPORTANT LINE IN THIS ENTRY.**
`e3a4494b`'s commit message justified the change with a measured before/after table of `block_hash` /
`pre_state_hash` / `post_state_hash` read off `GenesisBuilder::build_genesis_with_parameters(None)`.
**`719f2432` retracts that table, and this register does not reproduce it.** The instrument is
**run-varying at byte-identical source with fixed parameters**: `genesis_builder.rs:215` pins
`timestamp: 0` and the validators are the static `DEFAULT_VALIDATOR_KEY_PAIRS`, yet six builds produced
**six** `post_state_hash` values —

```text
42e2c0cb…   7b65f154…      ← the two in e3a4494b's retracted table (different source)
eb464221…   207e6cf4…      ← IDENTICAL source, verified byte-for-byte, four further answers
84e9a576…   833a41ef…
```

— three of those pairs differing with **nothing changed**. A before/after read off that instrument cannot
be distinguished from its own noise. **MEASURED** (`719f2432`). ★ Note what this does *not* invalidate:
the table's **conclusion** (the post-state moves) is true, for the independent reason that the normalized
term moves. A wrong instrument can reach a right conclusion, and saying so is not a defence of the
instrument.

★ **The sound instrument, and it is now pinned in the tree.** In the same two runs that disagreed on
`post_state_hash`, the blake2b256 of the **normalized `Par`** of `NonNegativeNumber.rho` agreed exactly.
It is deterministic, and it is the term that actually enters the tuplespace, so it is what a
consensus-visible change to a genesis contract should be measured and pinned on:

| | normalized `Par`, blake2b256 of the protobuf encoding | length (bytes) |
|---|---|---|
| before `e3a4494b` | `d9ce2e4db81db24237c01ed3ffb6d02de4fc4b6fbf4f2710e55bb6c0cfbcd9a0` | 2653 |
| after `e3a4494b` | `a537547892a0006becf965d755dacce71eccae56c49ff2e05b40df0b648751a2` | 2652 |

**MEASURED** (`719f2432`).

★★ **The term got one byte SMALLER while the file grew by 972 bytes, and that asymmetry is itself the
proof that the twelve added comment lines are not consensus-visible.** Comments do not survive
normalization; only the guard expression moved, and it moved from `v + x >= v` to
`x <= 9223372036854775807 - v` — a different but very slightly shorter tree. Had the file's growth shown
up in the term at all, the pin would have been measuring the comment rather than the change.

**The pin's guard was watched RED at its subject, and the first attempt went red on the wrong assertion —
which is reported rather than quietly retried.** Rewriting the bound to `9223372036854775808` made the
**parser** refuse (`NumberOutOfRange`, since $`2^{63}`$ is not an `Int` literal — a small independent
confirmation that the language rejects it), so the failure arrived from the `normalize` `expect` rather
than from the hash comparison, and proved nothing about the pin. It was then driven red properly with a
**logically equivalent** rewrite, `if (9223372036854775807 - v >= x)`:

```text
★★ NonNegativeNumber.rho's NORMALIZED TERM changed, so the genesis post-state changed.
   That is a consensus-visible change and it owes a
   docs/consensus/consensus-change-register.md entry.
   If the change is intended, set EXPECTED = "bc2cfbda…" …
     left:  ("bc2cfbda…", 2652)
    right:  ("a5375478…", 2652)
```

★ **Catching a logically equivalent rewrite is correct behaviour, not over-sensitivity: consensus agrees
on terms, not on semantics.** Two contracts that mean the same thing normalize to different `Par`s, commit
different roots, and fork. A pin that ignored equivalent rewrites would be pinning the wrong thing.
**MEASURED** (`719f2432`).

⚠ **The genesis specs did not verify this and cannot.** Measured with `--no-capture`:
`non_negative_number_spec` and `make_mint_spec` collect **zero** assertions and never report
`has_finished`, so `RhoSpec::run_tests` iterates an empty map and passes **vacuously**. Their *"2 passed"*
is not evidence about the overflow path. **MEASURED** (`e3a4494b`, and re-confirmed in `719f2432`'s
verification run). The `genesis_overflow_guard_shape` pin is what actually gates the change.

**Verified** on a post-`cargo clean` cold tree: `-p casper --test mod -E 'test(/genesis_overflow_guard_shape/)
or test(make_mint_spec) or test(non_negative_number_spec)'` — 6 run, 6 passed; `-p rholang --test
rholang_numeric_eval_spec` 27/27. **CITED** (`719f2432`).

★ **This entry closes a gap the register reported about itself.** Before `719f2432`, **no** test in the
tree pinned any genesis-visible artefact — `rg` for a `post_state_hash` literal returned zero hits. One
now does, and it is the artefact that is stable enough to pin. ⚠ The pin is **expected** to go red on any
future body edit; that is its job, and it prints the new hash so the constant can be updated in the same
commit that edits the contract. It is the first piece of **machine** drift detection anywhere in this
register's subject matter — see [§7.5](#7-maintenance) extension 5.

---

### CBR-037

**The `EPathMap` tag-8 trie-key READER becomes total — a writer that was unlimited by requirement had a reader capped at 32 collection levels, so both nodes could write bytes neither could read.**

| | |
|---|---|
| Commit(s) | `063974c5` |
| Status | LANDED |
| Direction | PERMISSIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/rhoapi_ext.rs` (the tag-8 read arm at **:1069**), `models/src/rust/canonical_path.rs` (the deleted `COLLECTION_DEPTH_LIMIT` at **:151**, and `SCANNER_STACK_CEILING` with it) |

#### (a) The issue

`rhoapi_ext.rs` at **:1069** — the tag-8 read arm — called `decode_trie_path`, which was capped by
`canonical_path.rs` at **:151** `COLLECTION_DEPTH_LIMIT = 32`. Its **writer**, `encode_trie_path`, is
**total and unlimited by requirement (R3F-2)**. $`\Rightarrow`$ A producer that can emit what its consumer
refuses.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **NO** — every previously-accepted byte string decodes bit-identically; the deep round trip is asserted a **byte-level fixed point at depths 33…384**. **MEASURED**. |
| 2 · verdict | **MOVES** — accept/reject moves, `Err(DepthLimitExceeded)` $`\rightarrow`$ `Ok`. ★ This is the axis the change **exists** to move. |
| 3 · bytes (Lane B, bincode) | **NO** — the encoder is untouched; `U(m)` framing, key grammar and arm selection unchanged. `serializer_par_byte_goldens` **13/13**. **MEASURED**. |
| 3 · bytes (Lane P, prost) | **NO** — same reason. **MEASURED**. |
| 4 · post-state hash | **NO** for previously-accepted inputs. ⚠ For *newly*-accepted ones a post-state now **exists** where a rejection stood, which belongs to the verdict axis and is counted there rather than twice. |
| 5 · accepted programs | **MOVES** — programs are accepted that were refused: any contract whose ground map carries an entry deeper than **32** collection levels. |
| 6 · metering | **NO** — the deleted work is one `u32` compare per level; no charge site and no price changed. **DERIVED**. |

**The disagreement.** Validator *X* pre-`063974c5`, *Y* post. A deploy builds a ground `EPathMap` with a
**40-level** nested-tuple entry; the produce's `Par` serialises through the field-8 arm and the bytes enter
the block. *Y* decodes; *X* answers `DepthLimitExceeded`, `merge_field` propagates it, and *X* declares the
block **invalid**. **Same block, opposite validity** $`\Rightarrow`$ a **safety fork**.

★★ **And the pre-existing form is the more interesting half, so it is recorded rather than superseded:
BOTH *X* and *Y* already wrote such bytes and NEITHER could read them.** The term was a node-local
**liveness trap before it was a fork** — the asymmetry existed at every version, and the upgrade converts a
shared inability into a disagreement.

**Blast radius.** Any contract whose ground map nests beyond 32 collection levels — reachable by an ordinary
deploy, and *writable* by every version.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: attempt `decode_trie_path` over every historical `EPathMap` trie key and count
`DepthLimitExceeded`. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** The 2026-07-29 ruling is that there is **no artificial depth cap
for consensus**, so a writer able to emit what its reader refuses *is* the defect and the direction of
repair is a **total reader**.

★★ **The cap's own justification was MEASURED FALSE on this path, which is what makes deletion right rather
than merely permitted.** It read *"today's effective prost envelope"* — i.e. that some lower prost ceiling
would bind first. Tag 8 is `bytes`; `prost::encoding::bytes::merge` reads a length and copies, so the
payload costs **zero** nested-message levels. At depth 400 the refusal came from the **trie** codec, while
the identical depth through **tag 1** gave `RecursionLimitReached` — **same message type, two fields, two
ceilings.** $`\Rightarrow`$ This is not a one-level shuffle of the binding constraint; **there was nothing
behind the cap.**

**Why this repair rather than the alternatives.**

| alternative | why rejected |
|---|---|
| **Raise the cap.** | Any finite cap reproduces the asymmetry at a new depth, and R3F-2 makes the writer unlimited *by requirement*. |
| **Cap the writer to match.** | It would make a required-total function partial, and would reject terms already on chain. |

★ **Totality was achieved SUBTRACTIVELY — no new acceptance rule was added**, which is why permissiveness
is *excluded* rather than hoped for: the refusals for **truncated runs**, **reserved tags** and **nested
`0x0F` escapes** are each **re-asserted** at depths 33 and 64. The bound is now
$`|\mathrm{frames}| \le |\mathrm{input}|/2`$ with the bomb-safe preallocation guard retained.

★★ **Deleting the cap also deleted two things that had been resting on it**, and both are findings rather
than tidying: `SCANNER_STACK_CEILING`, whose stated derivation **was a function of the cap** and so had no
independent basis; and `DecMachine::depth`, which turned out to be a **duplicate** of
`col_or_region_frames` differing only by a latent off-by-one that **refused an empty list at the boundary**.

**Authority.** The 2026-07-29 ruling, that there is no artificial depth cap for consensus.

**★ Sibling enumeration, ON A NAMED AXIS. Count: 4 on the axis of reader/writer depth asymmetries in the
canonical path codec** — this is the **fourth and last**, which is why the entry's headline says *closed*
rather than *fixed*. The axis is DERIVED: it is the set of `(writer, reader)` pairs over the trie-key
grammar where the writer's domain exceeds the reader's.

#### Evidence

- Deep round trip a **byte-level fixed point at depths 33…384**; refusals re-asserted for truncated runs,
  reserved tags and nested escapes at depths **33** and **64**. **MEASURED**.
- The envelope claim refuted: at depth 400, tag 8 refused in the **trie** codec while tag 1 gave
  `RecursionLimitReached` — two ceilings on two fields of one message type. **MEASURED**.
- `serializer_par_byte_goldens` **13/13**. **MEASURED**.

---

### CBR-041

**An `EPathMap` serializes on the prost wire as the TRIE — its own byte array — for every map. The tag-1 list arm is deleted.**

| | |
|---|---|
| Commit(s) | `1b576c90` (the repair), `698406a3` (the `U(m)` memo it rests on), `87ba591c` (the depth measurement that bounded it) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/rhoapi_ext.rs` (`encode_raw`, `encoded_len`, the `path_stream` memo, `EntryTrie::cmp`), `models/src/main/protobuf/RhoTypes.proto` (field 8's contract), `models/src/rust/canonical_path.rs` (`decode_trie_path`'s stale depth prose), `rholang/tests/pathmap_escape_depth_reachability.rs` (the measurement) |

#### (a) The issue

An `EPathMap` stores a `pathmap::PathMap<Par>` — a prefix-compressed byte-node trie-map. Proto field 8 already carried the trie's own canonical byte array `U(m)`, but **only when `eval_stable_epathmap(self)` held**. Every other map emitted `repeated Par` at tag 1: the trie flattened to a list, each entry re-encoded from scratch, and re-filed entry by entry on the far side at a cost of one `encode_trie_path` per entry to rebuild what the sender had already computed.

The fork discarded, at the wire boundary, exactly the properties the trie exists to provide — the key order it maintains by construction, and the encoded form it already holds — and then paid to rebuild them.

#### (b) The disagreement

A node on the old code and a node on the new code emit different prost bytes for any **non-ground** `EPathMap`. The prost encoding is what `cost_accounting/sig.rs` **signs**, so this is a signature difference and a post-state-hash difference, not a representation detail. `models/tests/sorter_canonical_golden.rs` names the hazard in its own failure text: *"a difference here is a consensus fork, not a performance regression"*. Reachable by an ordinary deploy — `{| … |}` is surface syntax.

#### (c) The change, and what did NOT move

`encode_raw` emits fields 3, 4, 5, 8 in ascending tag order at prost-derive parity on the skip-at-default rule; field 8 is `U(m)` for every non-empty map. `encoded_len` is its exact term-for-term twin. `encode_raw_fields` / `encoded_len_fields` are deleted — with no fork there is no caller, and a second unreachable copy of logic prost requires to agree byte-for-byte is an invitation to drift.

`merge_field` keeps **both** read arms. Tag 1 is read-only legacy; a reader is never narrowed.

★ **`U(m)` is `pathmap`'s own `.paths` payload with the compressor removed.** `serialize_paths` emits exactly these bytes (`u32-LE keylen ‖ key`, zipper order) and then hands them to zlib-ng at level 7, whose output is a function of the compressor build rather than of the trie — which disqualifies it as a consensus preimage and is why `U(m)` is hand-rolled. So this is not a private invention; it is the crate's format, uncompressed.

#### (d) The axes

| axis | cell | why |
|---|---|---|
| Value | `NO` | `encode_trie_path` is injective and trie order is byte-lexicographic by construction, so `U(m)` and the entry set determine each other. Nothing a program can observe moves. |
| Verdict | `MOVES` | Ordering reaches spatial matching via `ScoredTerm::sort_vec`; 2 of 120 sorter golden entries moved. ⚠ Their **score trees are identical** in want and got — what moved is the tie-break *input*, not the comparator. |
| Bytes (bincode) | `NO` | `bincode_encoder`/`bincode_decoder` still emit `u64 n ‖ n × Par`; every bincode golden unmoved. **This is the half of the mandate still owed** — see (i). |
| Bytes (prost) | `MOVES` | The axis. Ground maps are **byte-identical** (they emitted field 8 alone already, and still do). |
| Post-state hash | `MOVES` | The prost encoding is the signed preimage. |
| Acceptance | `NO` | Strictly permissive — see (e). |
| Metering | `MOVES` | `encoded_len` is charged twice per substitution; it changes from Σ per-entry lengths to the length of the shared key stream. |

#### (e) Acceptance is permissive, and it is measured

Reading tag 8 means `decode_trie_path` per key, whose escape arm re-decodes a $`\neg\mathtt{eval\_stable}`$ entry through prost. That is a **fresh** `DecodeContext` — the full 100-level budget from zero — whereas tag 1 spent $`W \geq 3`$ levels of the outer decode's budget before reaching the entry. `rholang/tests/pathmap_escape_depth_reachability.rs` searches both ceilings rather than transcribing them:

```
escape-arm read ceiling ........ 32
tag-1 prost ingress ceiling .... 31
```

$`\Rightarrow`$ every entry tag 1 could deliver, tag 8 can read. **No term that decoded before stops decoding.** ⚠ The headroom is **one** level, not the three a $`W \geq 3`$ argument predicts. The inequality is what the conclusion needs and it holds, but the margin is thin and is filed as **measured**, not derived.

#### (f) Could live chain state have been produced under the old behaviour?

Reachable by any finalised deploy containing a non-ground pathmap. Settled under the owner's 2026-07-30 pre-production ruling, as [CBR-040](#b1-retired-register-entries) (f) settles its own. `unverified_budget` is unchanged.

#### (g) Evidence

**MEASURED.** Exactly **2 of 15** goldens moved — `locally_free.protobuf.bin` and `remainder_connective.protobuf.bin` — and both are maps carrying non-default metadata fields, i.e. precisely the class `eval_stable_epathmap` excluded from field 8. The three **ground** prost goldens (`e6a_index`, `nested`, `ezipper`) and **every** bincode and JSON golden came back **UNMOVED**.

★ **That unmoved set is the anti-vacuity control.** A change that moved everything would mean the emitter had drifted rather than the fork having been removed, and the two claims are indistinguishable without it. `REMAINDER_CONNECTIVE_ENCODED_LEN` re-pinned $`23 \rightarrow 19`$; `LOCALLY_FREE_ENCODED_LEN` unchanged at 36 — a coincidence of length, not of content.

⚠ **MEASURED FALSE, recorded so it cannot be revived as a justification: prefix sharing.** `path_stream_of` writes each key **in full**. `ezipper.protobuf.bin` carries `04 01 61 04 01 78 00` and `04 01 61 04 01 79 00` — a shared 3-byte prefix, both written whole. The trie is prefix-compressed *in memory*; `U(m)` is not. The gain here is **canonicity and single-sourcing**, not compression.

#### (h) Authority

Owner ruling, 2026-07-31, verbatim: *"I mean serialization of pathmap should use its byte array serialization directly, regardless the format"*; *"why the hell would you turn the trie-map into a list!"*; *"Every other surface needs to serialize the trie-map! Holy shit, a trie is its own data structure with its own properties!"*

#### (i) Residuals

1. **The bincode half of the ruling was discharged by FORM ② (CBR-042) and both halves were then
   superseded by EPM1 (CBR-044)**, which carries the topology arena and the values with no parallel
   key stream — resolving the size question FORM ② left open (the wire lineage is the
   [PathMap report §5.2](../design/pathmap/pathmap-report-2026-08-03.md#52-the-wire-lineage-and-the-epm1-format)).
2. **The parked trie cursor proved unnecessary** and was deleted with the arm's successors: the
   memoized `U(m)` slice made an interleaved key/value cursor structurally pointless.
3. **`eval_stable_epathmap` survives as a fold, not a discriminant.** It is no longer imported by `rhoapi_ext` — that is what makes reintroducing the fork by reflex impossible — but it remains `canonical_path`'s recursion cut and `entries_stable()`'s own consumer, and is still computed **exactly**, because it comes free from the encoder's verdict rather than from a second walk that could form a second opinion.
4. **The network-version constant is deliberately not bumped in the code commit**, per the standing convention; coordinating it is a separate act.

---

### CBR-042

**…and on the bincode wire too. An `EPathMap` writes its trie's own byte array `U(m)` verbatim and contiguously, then its values — and the reader never decodes a trie key.**

| | |
|---|---|
| Commit(s) | `3a32cf07` (the four surfaces, one commit), `1b576c90` (the prost half this completes), `87ba591c` (the depth measurement that shaped it) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/rhoapi_ext.rs` (`EntryTrie`'s `Serialize`/`Deserialize`, `from_path_stream_and_values`, `insert_encoded`, the tag-8 arm), `models/src/rust/pathmap_crate_type_mapper.rs` (`PathFrames`, `PathFrameError`), `models/src/rust/rholang/bincode_schema.rs` (`EPATHMAP_PROGRAM`, `PathmapPs`, `pathmap_ps`), `models/src/rust/rholang/bincode_encoder.rs` (`open_pathmap`), `models/src/rust/rholang/bincode_decoder.rs` (`Op::Pathmap{Start,Build}`, `Reader::byte_slice`), `models/tests/epathmap_bincode_is_the_path_stream.rs` (the new gate) |

#### (a) The issue

[CBR-041](#cbr-041) made the **prost** wire trie-native and said so in its own residual (i)-1: *"the bincode surface is NOT yet trie-native."* So one value had two serializations of two different kinds — a trie on one wire, a flattened list on the other — and the list one is what the **event-hash** preimages and the **cold store** are built from. The owner's ruling names no format: *"Every other surface needs to serialize the trie-map! Holy shit, a trie is its own data structure with its own properties!"*

#### (b) The disagreement

A node on the old code and a node on the new code produce different bincode for **any** non-empty `EPathMap`. That is not a representation detail: `stable_hash_provider::hash` is Blake2b256 over `bincode(channel)` / `bincode(datum)`, so a moved encoding is a moved **event hash**, and the cold-store leaf bytes enter the history trie, so it is a moved **post-state hash**. Reachable by an ordinary deploy — `{| … |}` is surface syntax.

#### (c) The change — FORM ②, and why the values ride along

```text
EPathMap serde/bincode ::= u64-LE |U(m)| ‖ U(m)      ← the trie's byte array, VERBATIM and CONTIGUOUS
                           u64-LE n     ‖ n × Par    ← the values
                           locally_free ‖ connective_used ‖ remainder
```

`EntryTrie::serialize` writes those first two lines as a positional 2-tuple, which bincode frames with **nothing of its own**, so they are literally consecutive bytes. The serde struct is still **4 fields**; the pair lives inside slot 0.

★ **The naive move — `U(m)` ALONE — is what CBR-041 (i)-1 correctly refused, and FORM ② is not that move.** A `U(m)`-only encoding must reconstruct entries by `decode_trie_path` per key, whose `0x0F` escape arm re-decodes a ¬`eval_stable` entry through prost's recursion-limited decoder. Carrying the values keeps the reader on the existing iterative, depth-**unlimited** machinery, so `decode_trie_path` is **never called on this surface** and no ceiling is inherited. The price is `8 + |U(m)|` bytes per map, and it is the price the ruling accepted.

⚠ **SPLIT, never interleaved**, and that is measured rather than stylistic. An interleaved key/value form needs a live trie read-zipper in the encoder — built, and parked as `Op::EntryPaths` at **3 allocations / 1408 B** on a warm encode where `bincode_encoder_space::the_steady_state_allocation_table` requires **zero**, two of them inside `read_zipper()` where they cannot be pooled away. Split, the encoder emits one **memoized** slice (`EntryTrie::path_stream`, a `memcpy`) plus the projection it was already emitting: the space gate stays **9/9**. And an interleaved form would destroy the property that makes this *the trie's byte array* at all — `U(m)` would no longer appear contiguously.

★ **ONE reader, not two.** `EntryTrie::from_path_stream_and_values` serves both the derived `Deserialize` and `bincode_decoder`'s `Op::PathmapBuild`. Two hand-written bulk readers of one wire shape is the defect `c705776c` closed. The **framing** is single-sourced too: `PathFrames` is the read twin of `path_stream_of`, and proto field 8's tag-8 arm now reads through it as well, at identical dispositions — one definition of `U(m)`'s framing in the tree, two policies over it.

⚠ **A disagreeing peer stream is RE-FILED, never rejected.** That is `adopt_trie`'s policy and tag-8's, and it is an *acceptance* decision rather than a taste: a node that refuses a byte string its peers accept has forked. Malformed framing, a key that is not `encode_trie_path` of its value, a frame count that disagrees with the value count — every one yields the trie built from the values.

#### (d) The axes

| axis | cell | why |
|---|---|---|
| Value | `NO` | `encode_trie_path` is injective and trie order is byte-lexicographic by construction, so `U(m)` and the entry set determine each other. The values beside it are the projection this surface already emitted. Nothing a program can observe moves. |
| Verdict | `NO` | **Measured.** Matching is structural; `ScoredTerm::sort_vec` reads `Ord`, which reads the trie KEYS and did not move; store channel hashes select a *bucket*, not an *order*. `models/tests/golden/sorter_canonical_forms.txt` came back **UNMOVED** — its last change is still `1b576c90` — and that is the direct control, because that same golden moved on 2 of 120 entries when CBR-041 touched this relation. |
| Bytes (bincode) | `MOVES` | **The axis.** All five bincode goldens and all five JSON goldens moved. |
| Bytes (prost) | `NO` | **Measured by SHA-256, not by length**: all five prost goldens byte-for-byte identical, all five `encoded_len` pins unchanged. See (g). |
| Post-state hash | `MOVES` | Event hashes are Blake2b256 over bincode preimages; cold-store leaf bytes enter the history trie. |
| Acceptance | `NO` | `decode_trie_path` is never called, so no depth ceiling is introduced — see (e). And a disagreeing stream re-files rather than being refused. |
| Metering | `NO` | Metering reads prost `Message::encoded_len` (`rholang/src/rust/interpreter/accounting/costs.rs:67`), which this does not touch. No cost in the tree is computed from a bincode length. |

★ **The four surfaces, pinned to the landed tree.** `models/src/rust/rholang/bincode_schema.rs:366` is the `FieldKind::Bytes` that opens `EPATHMAP_PROGRAM`; `models/src/rust/rholang/bincode_encoder.rs:477` is the single `put_bytes` that emits `U(m)`; `models/src/rust/rholang/bincode_decoder.rs:1642` is the zero-copy `byte_slice` that reads it back; `models/src/rust/rhoapi_ext.rs:1106` is the one shared reader both decode paths call; and `models/src/rust/pathmap_crate_type_mapper.rs:525` is the one splitter all three readers of `U(m)`'s framing go through.

#### (e) Acceptance is unchanged, and the ceiling it avoids is re-measured rather than cited

The claim *"no depth ceiling"* is only worth as much as the cliff it is measured against, so `models/tests/epathmap_bincode_is_the_path_stream.rs` re-measures both halves in one file rather than citing the other one:

```text
decode_trie_path(escape-arm key), depth  4 ....... Ok      ← the codec works
decode_trie_path(escape-arm key), depth 34 ....... Err     ← the cliff is real
decode_trie_path(escape-arm key), depth 64 ....... Err
cold_decode ∘ cold_encode, depths 4 / 34 / 64 .... byte-level FIXED POINT
cold_decode ∘ cold_encode, depth 4,096 .......... byte-level FIXED POINT
```

★ **The fixture is the ESCAPE-ARM shape, and that was a measured correction.** A *ground* deep entry is keyed by the structural `0x0C` arm, which is iterative and capped by nothing, so a ground ladder would pass against a key-reading decoder too and would prove nothing. The ladder is carried on `ESet([EList[…GInt(1)…]])` — the same term family `rholang/tests/pathmap_escape_depth_reachability.rs` measures the ceiling on.

$`\Rightarrow`$ **no term that round-tripped before stops round-tripping.** And the re-filing disposition cannot narrow anything either, because the two branches are provably the **same trie**: on the agreeing branch every frame *equals* `encode_trie_path(value)`, which is the key `EntryTrie::from` would have used.

#### (f) Could live chain state have been produced under the old behaviour?

Reachable by any finalised deploy containing a pathmap. Settled under the owner's 2026-07-30 pre-production ruling, as [CBR-040](#b1-retired-register-entries) (f) and [CBR-041](#cbr-041) (f) settle their own. `unverified_budget` is unchanged.

#### (g) Evidence

**MEASURED.** Exactly **10 of 15** goldens moved — every bincode golden and every JSON golden:

| fixture | bincode | JSON | prost |
|---|---|---|---|
| `e6a_index` | $`3{,}689 \rightarrow 4{,}029`$ B (**+340**) | $`36{,}894 \rightarrow 42{,}146`$ B | $`335 \rightarrow \mathbf{335}`$ |
| `nested` | $`625 \rightarrow 687`$ B (**+62**) | $`5{,}242 \rightarrow 6{,}713`$ B | $`30 \rightarrow \mathbf{30}`$ |
| `ezipper` | $`837 \rightarrow 875`$ B (**+38**) | $`5{,}110 \rightarrow 5{,}819`$ B | $`40 \rightarrow \mathbf{40}`$ |
| `locally_free` | $`317 \rightarrow 356`$ B (**+39**) | $`1{,}697 \rightarrow 2{,}155`$ B | $`36 \rightarrow \mathbf{36}`$ |
| `remainder_connective` | $`229 \rightarrow 248`$ B (**+19**) | $`1{,}247 \rightarrow 1{,}465`$ B | $`19 \rightarrow \mathbf{19}`$ |

★ **The five unmoved prost goldens are the anti-vacuity control, and they are compared by SHA-256 rather than by length** — a length-only comparison would accept a same-size permutation, which is exactly the class of drift a byte register exists to catch. A change that had moved them too would mean the prost emitter had drifted rather than the bincode surface having been converted, and the two claims are indistinguishable without the control.

**A second control, on the event-hash legs.** `PRODUCE_INDEX_RS1_HASH_HEX` (`9991c1b0…` $`\rightarrow`$ `b94d5aaa…`) and `PRODUCE_INDEX_RS2_HASH_HEX` (`b86bf7a0…` $`\rightarrow`$ `c5458866…`) moved; `INDEX_CHANNEL_HASH_HEX` and `CONSUME_DISCOVERY_HASH_HEX` did **not**. The channel is a `GString`, not a map — so the movement is confined to the leg that actually carries an `EPathMap`, which is what distinguishes a targeted change from a global one.

**A third, arithmetic.** The three cold-store leaf pins moved by exactly **+46 B** each (`PAR_DATUM` $`4{,}153 \rightarrow 4{,}199`$; `PAR_DATUMS` $`4{,}285 \rightarrow 4{,}331`$; `PAR_CONTS` $`4{,}529 \rightarrow 4{,}575`$). The fixture holds two maps — a bare `EPathmapBody` and one inside an `EZipper` — whose framed key streams are $`8 + 12`$ and $`8 + 18`$ bytes. $`(8+12) + (8+18) = 46`$. The delta is derived, not observed.

**Test totals.** `models` **469 passed / 0 failed** (baseline **459**, plus the 10 assertions of the new file); `bincode_encoder_space` **9/9** with the warm allocation count still **0**; `bincode_encoder_differential` 13/13 and `bincode_decoder_differential` 13/13, whose oracles are the derived `Serialize` and the derived `Deserialize` respectively — which is why all four surfaces had to land in one commit; `rspace_plus_plus` 336/336; `rholang` `epathmap_replay_equivalence_spec` 1/1 and `trie_entry_invariant_spec` 8/8.

#### (h) Authority

The same standing owner ruling [CBR-041](#cbr-041) (h) quotes, whose scope is *"regardless the format"* and *"Every other surface"* — the prost half discharged it in part; this discharges the rest of its *shape* obligation.

#### (i) Residuals — resolved

1. **The `locally_free` exposure this entry's first filing recorded as a "stated cost" was a broken
   invariant** — the standing rule in `models/src/rust/rholang/bincode_schema.rs` that
   `locally_free` *"must not reach an RSpace channel hash"* — and it is repaired by
   [CBR-043](#cbr-043): one function $`U`$, applied to the value this surface writes.
2. **The $`U(m)`$-alone size question is closed by EPM1** ([CBR-044](#cbr-044)): the snapshot
   carries the compact topology arena and the values once, with no parallel key stream, so the
   FORM ② size cost (+8 + $`|U(m)|`$ per map, measured +340/+62/+38/+39/+19 B on the five goldens)
   is gone and nothing further is owed.

---

### CBR-043

**`U` applied to the entries this surface WRITES. FORM ② emitted the key stream of the entries the map *stores* beside values it writes `locally_free`-blanked — and a key derived from the unblanked entries put an entry's bitset on the event hash.**

| | |
|---|---|
| Commit(s) | `8cf0b770`, correcting `3a32cf07` ([CBR-042](#cbr-042)) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/rhoapi_ext.rs` (`EntryTrie::{bincode_trie, bincode_view, bincode_path_stream, blanked_trie}`, the `bincode_trie` memo cell and its four invalidations, `Serialize`, `drain_owned_pars`), `models/src/rust/rholang/bincode_schema.rs` (`pathmap_ps`, `PathmapPs::Stored`), `models/tests/epathmap_bincode_is_the_path_stream.rs` (§4, the acceptance properties), `models/tests/epathmap_canonical_fixtures.rs` (the entry-level widening), `models/tests/epathmap_wrapper_cell.rs` (the layout oracle), `models/tests/bincode_encoder_space.rs` (the lf-bearing allocation row) |

#### (a) The issue

`models/src/rust/rholang/bincode_schema.rs` states a rule that predates all of this work:

> `locally_free` is transient analysis data that must not reach an RSpace channel hash.

It is why `models/build.rs` injects `serialize_with = serialize_as_empty_bytes` on twelve `.rhoapi` `locally_free` fields, and why `EPathMap`'s hand-written `Serialize` blanks its own. The serde/bincode surface has therefore **always written lf-blanked entries**.

[CBR-042](#cbr-042) put the trie's key stream on that surface as well — and took it from `EntryTrie::path_stream()`, the keys of the entries the map **stores**. Those two things are not the same whenever an entry carries `locally_free`, because an entry is **keyed** by `encode_trie_path`, whose `0x0F` escape arm files a ¬`eval_stable` entry as its canonical **prost** bytes, and prost *retains* the bitset.

★ **The root cause, stated once and generally: field-wise blanking cannot keep a DERIVED quantity consistent with the fields it was derived from.** The twelve injected sites blank every `locally_free` *field*. Nothing blanked the *key computed from those fields*, because the key is not a field — and every one of those twelve sites is individually correct. The defect is not at any of them; it is at the point where a derived quantity was emitted beside operands that had been normalized without it.

#### (b) How it breaks consensus

| Axis | Answer | Why |
|---|---|---|
| Value | **NO** | The entry set is untouched and so is every decoded value. The reader is not modified at all — `EntryTrie::from_path_stream_and_values` always re-filed from the VALUES — so the map a peer's stream denotes is what it denoted before. `epathmap_wrapper_cell`'s round-trip proptest (`de.ps() == map.ps()`) and `rholang`'s `epathmap_replay_equivalence_spec` **1/1** both hold. |
| Verdict | **NO** | Bincode reaches a verdict only through the RSpace store's channel *hashes*, which select a bucket rather than an order; spatial matching is structural; `ScoredTerm::sort_vec` reads `Ord`, which reads the trie **keys** — `path_stream()`, untouched. |
| Bytes (B) | **MOVES** | ★ the axis. |
| Bytes (P) | **NO** | **MEASURED BY SHA-256.** All five prost goldens byte-for-byte unmoved; all five prost `encoded_len` pins unchanged. `EPathMap::encode_raw` still reads `path_stream()`, and that is *structural*: prost retains `locally_free`, so it writes the entries as stored and must key them as stored. |
| Post-state | **MOVES** | Event hashes are Blake2b256 over bincode preimages, so a moved encoding is a moved hash for any datum carrying an lf-bearing `EPathMap`. |
| Acceptance | **NO** | The reader is not modified. `decode_trie_path` is still never called here; a disagreeing peer stream still RE-FILES rather than being rejected; re-measured green at depths **4 / 34 / 64 / 4,096**. |
| Metering | **NO** | Metering reads prost `Message::encoded_len` (`rholang` `accounting/costs.rs:67,98,99,298`), which is on the unmoved axis above. |

**The concrete disagreement.** Two `EPathMap`s differing *only* in one entry's `locally_free` — a bound-variable `EVar`, non-ground by content either way, so both take the same escape arm and the bitset is the sole difference:

```text
  produce hash, lf-bearing entry   e48b249cb7b829c5f087a947299d617248782946adc121d8ee4c39ee4a766b9a
  produce hash, cleared   entry    7259192343a4e7c8b15fa1ff4a54c24069713efbeaf659f84e786636c965953a
```

⚠ **And the same map hashed differently before and after a cold-store round trip** — `e48b249c…` in play, `7259192343…` after the store, because the store returns the blanked entries and they re-key. **That is a play/replay divergence**: replay and play disagree about the identity of one event. It is not a fixed-point curiosity, and [CBR-042](#cbr-042) (i)-1 — which filed it as a stated cost of a convergence — is corrected there.

**Blast radius.** Every produce, consume and cold-store leaf whose datum carries an `EPathMap` with `locally_free` anywhere in an entry. Ground maps — every map the byte-golden fixtures build except one — are unaffected, which is why the exposure was survivable long enough to be filed as a footnote.

#### (c) Why the change is correct

**There is one function `U`.** It is `pathmap_crate_type_mapper::path_stream_of`: a read-zipper walk over a trie yielding `repeat( u32-LE keylen ‖ key )`. [`EntryTrie::path_stream`] is `U` of the entries a value **stores**; `EntryTrie::bincode_trie().path_stream()` is `U` of the entries the serde surface **writes**. Same `U`, the surface's own argument — not a second key stream, not a second canonical form, and not a dual path: each surface has exactly one, selected by what that surface writes.

$`\Rightarrow`$ prost keeps `path_stream()` **because prost writes the stored entries**, bitsets and all. The two accessors exist so that neither surface can pick up the other's by reflex.

**⚠⚠ The memo holds the whole TRIE, not the key stream — and that was measured, not anticipated.** Blanking can **reorder**: `eval_stable_par` requires `locally_free.is_empty()` at every level, so an lf bit moves an otherwise-stable entry from the structural arm to the `0x0F` escape arm, and an escape key sorts nowhere near the structural key its blanked twin gets. A memo holding only the keys therefore emits blanked key `i` beside stored value `j` — two halves of one tuple disagreeing about which entry is which. The first form of this repair did exactly that, and the entry-level event-hash leg described under (e) is what caught it.

**★ The blanking function IS the surface.** `blank` is not a hand-written "clear every `locally_free`" walk. Such a walk would be a *second opinion* about what this surface writes: it would have to enumerate the twelve injected sites, and it would go stale the moment a thirteenth appeared — **silently**, because a stale blanker still produces a well-formed key stream. Instead each entry is run through the surface itself, `bincode_encoder::encode_into` then `Par::cold_decode`, whose byte-for-byte agreement with the derived `Serialize`/`Deserialize` is pinned by `bincode_encoder_differential` and `bincode_decoder_differential`. A thirteenth site is followed automatically and cannot drift.

⚠ Both halves are the **trampolined** codecs. The derived pair is $`\Theta(\mathrm{depth})`$ on the native stack and this runs on entries of unbounded depth; the temporary trie is torn down with `drain_owned_pars` + `dismantle_all` for the same reason, since `<Par as Drop>` is itself a recursive traversal.

#### (d) ★ The $`O(1)`$ discriminator, and the guard that is UNSOUND

`bincode_trie()` must answer *"is blanking the identity here?"* without walking anything, or `bincode_encoder_space`'s zero-allocation requirement fails. The obvious guard is `EntryTrie::union_locally_free` — a fold that is already maintained — and it is **wrong**:

| shape | `union_lf` empty? | `entries_stable` | `\|U(stored)\|` | `\|U(blanked)\|` |
|---|---|---|---|---|
| ground (control) | yes | **true** | 12 B | 12 B |
| flat lf entry | no | false | 18 B | 15 B |
| **nested lf entry** | **yes** | false | **31 B** | **28 B** |
| **lf inside a plain `EList`** | **yes** | false | **22 B** | **7 B** |

`union_locally_free` folds the entries' **top-level** `Par::locally_free` only. It is not hereditary — not through a nested `EPathMap`, and not even through an `EList`. On the last two rows it answers *"no `locally_free` anywhere"* while the key stream moves; the fourth is the sharpest, because blanking makes that entry `eval_stable` and its key changes **arm**, `0x0F` $`\rightarrow`$ structural.

`entries_stable` **is** sound, and hereditarily so by construction rather than by inspection. `eval_stable_par` demands `locally_free.is_empty()` at *every* level of the stable alphabet: the `Par` itself, `EList`, `ETuple`, and a nested `EPathMap` through `eval_stable_epathmap`, which checks that map's own bitset and then recurses into its `entries_stable()`. Hence

```math
\texttt{entries\_stable} \;\Longrightarrow\; \forall e.\; \mathrm{lf}(e) = \varnothing
\;\Longrightarrow\; \mathrm{blank}(e) = e
\;\Longrightarrow\; U(\mathrm{blanked}) = U(\mathrm{stored})
```

It is **conservative, never wrong**: an entry unstable for some other reason (an `EVar`, a `Send`) takes the memo path, and if blanking turns out to be the identity there too the memo records `None` and the accessor still returns a borrow. The table above is landed as an executable counter-measurement — the count of shapes on which `union_locally_free` is wrong is asserted as **exactly 2**, so a future change that made the fold hereditary fails the test and forces this section to be re-read.

#### (e) ★ The options considered, and why Option 1

| # | Option | Verdict |
|---|---|---|
| **1** | The surface writes `U` of the entries **it writes** | ★ **ADOPTED.** One `U`, the surface's own argument. Protobuf bytes cannot move, because prost's argument is unchanged. |
| 2 | Stop blanking `locally_free` on the serde surface — write the real bitsets | **REJECTED.** It repairs the *inconsistency* by making the key honest about the values, and in doing so puts transient analysis data on an RSpace channel hash **deliberately**, which is the rule in `bincode_schema.rs` inverted rather than kept. It also moves all ten serde goldens instead of two. |
| 3 | Make the trie key itself `locally_free`-independent — drop the bitset from the escape arm's prost payload for every surface | **REJECTED, and it is the one that looks cleanest.** The escape-arm payload *is* proto field 8's content, so this moves **prost** bytes: `locally_free.protobuf.bin` would move, and with it the `encoded_len` that metering reads. It converts a serde-local repair into a change on the metered wire. |

$`\Rightarrow`$ Option 1 is the only one under which the **control** — five prost goldens unmoved by SHA-256 — can hold at all. Options 2 and 3 are distinguished by *which* wire they disturb, and both disturb one that has no reason to move.

#### (f) The two controls, and why one alone would not do

**A negative control on the reader.** The pre-FORM-② deserializer was `EntryTrie::from(values)` — the values, re-filed, with no key stream consulted. FORM ②'s reader is `from_path_stream_and_values`, which computes `encode_trie_path(value)` per value and **re-files on disagreement**. On the lf-bearing fixture the two produce **byte-identical** values, because the re-filing branch *is* the old constructor. That is what makes `axis_value = NO` a measurement rather than an inference: the exposure was always in the **writer**, never in what any reader accepts.

**A key-independent control on prost.** `prost(cold_decode(cold_encode(m))) == prost(m)` is **NOT** a property of this surface and CBR-043 does not make it one — see (i)-1. The cause is measured and has nothing to do with keys: a round trip's prost image is *exactly* the prost image of the fully lf-cleared fixture, so dropping `locally_free` is the only thing the trip does. Without this control, the surviving prost inequality would look like a residue of the repair.

#### (g) Evidence

**The five acceptance properties**, each with a control, in `models/tests/epathmap_bincode_is_the_path_stream.rs` §4:

1. two maps differing only in an entry's `locally_free` produce the **same** produce hash — with the *stored* key streams asserted still different (so it is not comparing a map with itself) and an anti-vacuity leg asserting the hash still separates two different entry sets;
2. the produce hash is **invariant under a cold-store round trip**, with a ground control;
3. `cold_encode` is a byte-level fixed point on the **first** application. This assertion was `assert_ne!` and read *"the lf-bearing map is the case that takes two rounds"*;
4. the prost non-goal, measured, with the isolation described in (f);
5. the key/value-disagreement test is unchanged — a hostile stream **RE-FILES**.

**What moved and what did not.** ★ All five prost goldens **UNMOVED** by SHA-256. Exactly **2 of 10** serde goldens moved, and they are the only fixture carrying `locally_free`; the other eight came back unmoved, confining the blast radius. Both deltas are **derived**:

* `locally_free.bincode.bin` **$`356 \rightarrow 352`$ B**. The removed field is the entry `Par`'s `locally_free`, prost tag 9 wiretype 2, on the wire as `4a 02 00 01` — one tag byte, one length byte, two payload bytes — inside the escape-arm key payload. $`|U(m)|`$ falls $`31 \rightarrow 27`$; the `u32` frame header is unchanged; the whole encoding falls by the same 4.
* `locally_free.json` **$`2155 \rightarrow 2117`$ B**. JSON renders $`U(m)`$ as a decimal array, one element per line at six-space indent, so each removed element costs `len(decimal) + len(",\n      ")`: $`(2{+}8) + (1{+}8) + (1{+}8) + (1{+}8) = 37`$, plus one digit lost where a frame length fell $`13 \rightarrow 9`$. $`37 + 1 = 38`$.

⚠ **The four pinned event hashes did NOT move** (`PRODUCE_INDEX_RS1/RS2`, `INDEX_CHANNEL_HASH`, `CONSUME_DISCOVERY_HASH`) — the E-6a fixture carries no *entry-level* `locally_free`, so it could not exercise the path. That is recorded as a **gap in the fixtures**, not as evidence of neutrality, and it is why `epathmap_canonical_fixtures`'s end-to-end leg was widened: it tagged only the wrapping `Par`'s own bitset — the **map** level, which `serialize_as_empty_bytes` blanks directly and which never reaches a trie key — and so passed throughout the window in which FORM ② was putting an entry's bitset on that very hash. Both levels are tagged now, as separate `Produce`s so a failure names which level moved.

`models/tests/golden/sorter_canonical_forms.txt` **UNMOVED**; its last change is still `1b576c90`. That is the verdict control, and the same one CBR-042 used — it is sensitive, having moved on 2 of 120 entries when [CBR-041](#cbr-041) touched this relation.

★ **The seams, pinned to the landed tree.** `models/src/rust/rhoapi_ext.rs:294` is the memo cell — an `Option<Arc<EntryTrie>>` and not a byte string, which is the reordering finding in one declaration; `models/src/rust/rhoapi_ext.rs:482` is the $`O(1)`$ discriminator; `models/src/rust/rhoapi_ext.rs:538` is `blanked_trie`, where the blanking function *is* the surface; `models/src/rust/rhoapi_ext.rs:1419` is the serde seam reading **both** halves off one trie; and `models/src/rust/rholang/bincode_schema.rs:471` is the trampolined encoder's twin of that seam.

⚠ **The unmoved axis, cited positively rather than left as an absence.** `models/src/rust/rhoapi_ext.rs:1845` is `encode_ground_field8(self.path_stream(), …)` — proto field 8 still reading the **stored** key stream, which is *why* the five prost goldens are byte-identical. And `models/src/rust/rholang/bincode_schema.rs:75` is the rule itself, in words the tree already carried before any of this: `locally_free` is transient analysis data that must not reach an RSpace channel hash. `rholang/src/rust/interpreter/accounting/costs.rs:67` is the metering cell's evidence, unchanged from [CBR-042](#cbr-042): the charge reads prost `encoded_len`, never a serde length.

**Space.** `bincode_encoder_space::the_steady_state_allocation_table` **9/9**, and it gained a row that `nonground_pathmap` could not have covered — that fixture's entries are `GInt`s, so it takes the $`O(1)`$ arm and never touches the memo:

```text
  lf_bearing_pathmap   223 B | machine 0 allocs 0 B | derived 1 allocs 223 B
```

**Test totals.** `models` **473 passed / 2 ignored across 37 targets** (baseline **469 / 2 / 37**); `bincode_encoder_differential` 13/13 and `bincode_decoder_differential` 13/13; `protobuf_encoder_differential` 13/13; `serializer_par_byte_goldens` 7/7; `epathmap_spliced_event_bytes` 11/11; `rholang` `epathmap_replay_equivalence_spec` 1/1, `trie_entry_invariant_spec` 8/8, `epathmap_charge_trace_spec` 9/9; `cargo check -p casper -p rholang --tests` clean.

#### (h) Authority

The owner's ruling on this repair, adopted verbatim as its justification: *"there is one function `U`, applied to the value this surface writes. The serde/bincode surface already writes lf-blanked entries — it did so before FORM ② — so it must write the key stream OF THOSE ENTRIES. That is not a second `U(m)`; it is the same `U` with the surface's own argument."* The dual-path objection raised against it in an earlier brief was a misreading and was withdrawn in the same ruling.

#### (i) Residuals

1. ⚠ **`prost(cold_decode(cold_encode(m))) == prost(m)` is a NON-GOAL, not a bug — measured, and recorded here so it is not re-opened.** For a map carrying a **map-level** `locally_free` (proto tag 3) the serde surface blanks that field while prost retains it, so the round trip returns a map whose prost encoding is shorter by exactly that field. This is the serialize-only asymmetry working as designed and it **predates FORM ② entirely**; it is *independent of anything about keys*, and the isolation in (f) is the evidence — the round trip's prost image is exactly the lf-cleared fixture's.

2. **Blanking costs one pass per lf-bearing map, once.** `blanked_trie()` re-encodes and re-decodes each entry and files a throwaway trie; the result is memoized on the value, so warm encodes stay at zero allocations for **every** shape. A map that never carries `locally_free` never pays it and never stores the second copy — the memo records `None`.

3. **[CBR-042](#cbr-042)'s residuals 2, 3 and 4 are untouched.** The size win, the missing `protobuf_decode.rs`, and the network-version constant are all unchanged by this repair.

---

### CBR-044

**EPathMap becomes a homogeneous PathMap set/map and serializes as one versioned EPM1 trie image; the
generated protobuf PDA removes the recursive read ceiling.**

| | |
|---|---|
| Commit(s) | `26876b65` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/rhoapi_ext.rs`, `models/src/rust/epathmap_trie_codec.rs`, `models/src/rust/rholang/protobuf_encoder.rs`, `protobuf_decoder.rs`, `bincode_encoder.rs`, `bincode_decoder.rs`, and generated traversal code in `models/codegen/schema.rs` |

#### (a) The issue

The earlier CBR-041–043 sequence made the old entry-set representation progressively more trie-native,
but it still described a flattened value sequence beside `U(m)`, and it could not model a trie-map
whose keys and values are distinct. It also left protobuf decoding behind prost's recursive
`DecodeContext` ceiling. A direct map-value table is safe only when each value is decoded by the same
generated, depth-unlimited PDA as an ordinary `Par`.

The old model also collapsed “no associated value” into “no topology.” PathMap can carry a terminating
path without a value; that topology participates in prefix operations and serialization and therefore
must participate in identity.

#### (b) How it changes consensus

| Axis | Answer | Why |
|---|---|---|
| Value | **MOVES** | `{| a, b |}` is a `PathMap<()>` set; `{| k1: v1, k2: v2 |}` is a `PathMap<Par>` map. Map lookup returns the associated value. |
| Verdict | **MOVES** | Spatial lookup, restriction, join, meet, subtraction, and zipper operations now distinguish membership, association, and value-free topology. |
| Bytes (B) | **MOVES** | Serde/bincode field `ps` is one EPM1 byte array containing mode, ACTree03 topology, and the map-value table. |
| Bytes (P) | **MOVES** | Protobuf writes the same EPM1 array at field 9. Fields 1 and 8 remain legacy-read inputs and are not emitted. |
| Post-state | **MOVES** | Event hashes and cold-store leaves are functions of bincode. |
| Acceptance | **MOVES** | Valid depth-4,096 values and unknown-group shapes decode iteratively; mixed set/map storage is rejected; neutral empty selects a mode only on typed insertion. |
| Metering | **NO** | Re-derived 2026-08-03 under the token model (§2.2): `encoded_len` is not a consensus-cost input — consensus cost is the committed COMM count, which this representation change does not alter for any fixed verdict trace; no funding/settlement surface moves. |

The blast radius is every block, signature, event, or cold-store record containing an EPathMap, plus
deep protobuf input formerly stopped by the recursive read guard. This is a coordinated wire transition
and requires the normal activation/version decision before interoperating with an older node.

#### (c) Why the change is correct

The representation is `Empty | Set(PathMap<()>) | Map(PathMap<Par>)`. `Empty` resolves the
`{| |}` ambiguity without selecting the wrong algebra. The first typed insertion selects a mode;
non-empty modes cannot mix. A selected empty trie carrying terminating topology keeps its mode.
The public aliases and compatibility entry points are likewise specialized:
`RholangSetPathMap`, `RholangMapPathMap`, `set_trie`, and the two explicitly set-only mapper
directions. A map-mode value can no longer be accidentally routed through a generically named
set conversion that panics after erasing the distinction at the API boundary.

EPM1 is not a list encoding. Its payload is PathMap's compact ACTree03 arena plus values in arena ordinal
order. Serialization calls the trie accessor and generated stack-safe value codec directly. Equality,
hashing, and ordering walk terminating topology before values. Set algebra uses PathMap's lawful unit
lattice. Map overlaps compare exact `Par` values; subtraction treats the right trie as a key mask. The
prior global `Lattice for Par` was removed because arbitrary terms do not form that lattice.

Canonical-key stability classification is a heap-state PDA as well. Unary chains advance without
allocation; only siblings are deferred. Removing its former 64-frame native-recursion budget closes
the final artificial traversal threshold on the EPathMap key path without changing its predicate.

Generic method dispatch is part of the carrier transition rather than an `EMap` compatibility
layer. Exact-key `get`/`getOrElse` use the `PathMap<Par>` value slot; `contains`, `delete`, `set`,
`keys`, and `size` operate on the selected trie directly. Set-mode `get` and map insertion into a
selected set fail closed instead of manufacturing mixed membership. Neutral empty selects map mode
on its first `set`; a set insertion selects `PathMap<()>`. List-valued keys are encoded as one exact
key for these methods and are split into relative segments only by zipper/path APIs.

The dense decoder initially materialized ACTree03 internal compression nodes as terminating paths; the
corrected iterative walk emits only structural leaves and value-bearing nodes. The 15-case EPM1 suite
catches that distinction.

The PathMap subtrie iterator's keys are relative to its zipper focus. The first native query helper
decoded those suffixes as absolute keys and therefore dropped the cursor prefix from returned `Par`
values. The corrected helper reattaches the prefix in one reusable byte buffer; the 18-case query suite
is identical to the retained absolute-key scan, including result order.

#### Evidence

**MEASURED**, under RSS-capped systemd scopes:

- EPM1 snapshot 15/15; PathMap-native zipper/topology 7/7; algebra 6/6.
- Depth-4,096 protobuf and bincode EPM1 round trips succeed on a 256 KiB thread stack.
- Protobuf stack-safety 6/6; the production gate has 30 + 6 converted depth/width subjects and zero
  tripwire subjects.
- Production-reader registry 4/4, play/replay closure 1/1 at depth 4,096, trie escape/classifier
  stack tests 6/6, producer-shape registry 3/3, and malformed-shape reachability 5/5.
- Native PathMap query identity 18/18, including the relative-key/absolute-key subtrie correction;
  set mapper/invariant/query suites 61/61, 8/8, and 13/13.
- Native generic collection methods 2/2: map `get`/`getOrElse`/`contains`/`set`/`delete`/`keys`/`size`,
  set `contains`/`delete`, mixed-mode refusal, and neutral-empty map specialization. The linked
  MeTTaIL conformance target is 64 passed, 0 failed, 5 intentional ignores; all former C4 carrier
  failures now execute against `EPathmapBody`/`EZipperBody`.
- MeTTaIL lowering checkpoint `252a4011` constructs `EPathMap::new` for set mode and
  `EPathMap::new_map` for map mode directly from the iterative lowerer's arity-aware continuation;
  it does not lower through `EMap` or a `Vec<Par>` compatibility representation.
- The Par-typed cold-store leaf pins move with EPM1 and are re-derived by SHA-256, not length alone:
  `PAR_DATUM` $`4{,}199 \rightarrow 3{,}776`$ bytes (`f08ae1fe…c49c0`), `PAR_DATUMS` $`4{,}331 \rightarrow 3{,}908`$
  (`66aae765…640c01`), and `PAR_CONTS` $`4{,}575 \rightarrow 4{,}152`$ (`852dd38e…eec4ce`). The equal
  423-byte reduction is witnessed independently through all three cold-store roots carrying the
  same two-map fixture.
- Rocq kernel-checks three files with no admissions or axioms: the generic PDA equivalence,
  EPathMap mode/algebra laws, and the structural EPM1 envelope/value-table model. The latter proves
  canonical base-128 framing, exact topology and ordered-value preservation, ordinal uniqueness and
  range, malformed/truncated/trailing rejection, and equality of generated-PDA versus recursive
  value bodies. PathMap's ACTree03 parser remains an explicit executable boundary. Z3 finds no mode
  counterexample; TLC explores 2,816 distinct states with no error.
- At 1,024 entries the explicit list projection is 78.619× the EPM1 set size and 7.557× the EPM1
  map size; on the superseding warm rerun, projected lookup is 8,060.736× and 6,183.905× slower
  than native indexed lookup (full benchmark: the
  [PathMap report §5.4](../design/pathmap/pathmap-report-2026-08-03.md#54-epm1-performance-at-fixed-scale)
  and its durable TSV).
- Pinned implementation coordinates at `26876b65`: the carrier is
  `models/src/rust/epathmap_trie_codec.rs:36`, direct EPM1 encoding starts at
  `models/src/rust/epathmap_trie_codec.rs:240`, iterative ACTree03 reconstruction at
  `models/src/rust/epathmap_trie_codec.rs:547`, the exact-key PathMap query seam is
  `models/src/rust/rhoapi_ext.rs:699`, and the kernel-checked generic PDA equivalence theorem is
  `compile_run_equivalence` at line 84 of `formal/rocq/stack_safe_pda/theories/StackSafePDA.v`.
- The generated `Par::Ord` PDA is consensus-reachable through map-mode EPathMap algebra. At the
  registered commit, `EntryTrie::exact_map_value_eq` compares values at overlapping PathMap keys and
  the ordinary metered evaluator reaches it through pathmap union, intersection, graft, and
  `joinInto`. A differing value changes success into `ReduceError`, so both value and verdict are
  observable. The canonical sorter is not this edge: its tie-break compares generated protobuf bytes.
  The machine index pins the comparison seam and a production evaluator caller rather than inferring
  reachability from the generated impl's existence. At the registered commit those fixed-SHA
  coordinates are `models/src/rust/rhoapi_ext.rs:1049` for the value comparison and
  `rholang/src/rust/interpreter/reduce.rs:4696` for the evaluator's union edge.
- The generated comparator is checked against the recursive descriptor oracle for every pair in a
  deterministic 48-term corpus, carries depth 4,096 on a 256 KiB thread stack, and is an instance of the
  admission-free Rocq `compile_run_equivalence` theorem. This closes the reachability question inside
  CBR-044; it does not create a second consensus entry or change any axis classification.

**Subsequent refinements and revalidation (consolidated).** Three later commits are byte-neutral
refinements inside this transition — the zero-copy ACT decode (`9b3792ac`) and the reverse-zipper
pretty-printer walk (`2902f0d0`), plus the structural EPM1 proof replacement (`87e514b6`) — each
with its own pointer-identity, reverse-equivalence, or theorem/manifest binding and full relevant
suite matrix; all three are reported in the
[PathMap report §5.2 and §5.7](../design/pathmap/pathmap-report-2026-08-03.md). The independent
closure revalidation at `e67a6aaa` (`MemoryMax=4G`, zero swap, one Cargo job) passed **84/84**
focused EPathMap/codec/manifest tests, **7/7** census/registry, the stack gate **8/8 active**, and
the Rocq/Z3/TLC bundle (2,816 distinct TLC states), with 34 + 6 converted subjects and zero
tripwires — a revalidation of the registered representation and bytes, not a further change.
The 2026-08-04 proof refinement separately passed Rocq/Z3/TLC, the formal manifest 5/5, EPM1
15/15, and codec 7/7 in zero-swap cgroups. It changes no byte, value, verdict, accepted input,
post-state, or metering rule and therefore does not create another register entry.
**This entry also closes the formerly-open wire-asymmetry hazard CBR-028**: the generated decode
PDAs remove the read ceiling, making writer and reader symmetric; the retired row is
[Appendix B.1](#b1-retired-register-entries).

The evaluator follow-up `7b25df5a` is likewise a byte- and value-neutral refinement inside CBR-044.
It replaces a temporary forward `Vec<&Par>` with the already-proven reverse trie visitors; an
external neutral/set/map regression preserves evaluated map associations, and the reverse-order
visitor regressions pass for shared-prefix and dense topologies. It changes no wire field, accepted
input, verdict, post-state, or metering rule, so it neither changes the seven-axis classification
above nor creates a second register entry. The capped evidence and exact auxiliary-space delta are
reported in [PathMap §5.7](../design/pathmap/pathmap-report-2026-08-03.md#57-reverse-zipper-totality).

#### Authority and residuals

The owner required direct trie serialization, specialized set/map modes, ordinary Rust stacks, no
traversal-depth workarounds, and full equivalence evidence. The implementation is **LANDED**. Network
activation/version selection remains a deployment decision, not an implementation-status qualifier.
Legacy protobuf fields are read-only migration inputs; removing them is a later compatibility decision.

---

## 4.4 Surface L — MeTTaIL's Rholang

---

### CBR-L07

**`List.last()` in MeTTaIL's Rholang.**

| | |
|---|---|
| Commit(s) | `bbceb6d9` (the projection), `6e543c01` (execution) |
| Status | LANDED |
| Direction | PERMISSIVE |
| Evidence grade | WITNESSED |
| Files | `languages/src/rholang.rs`, `rholang-runtime/src/` |

#### (a) The issue

The Surface-L twin of **CBR-024**. ⚠ *"the conservativity claim was FALSE and it found a superset
breach"*: the `last` **string literal becomes a terminal, so the lexer must emit it, so the word can
never be an identifier**. **CITED** (campaign ledger).

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A |
| 2 · verdict | N/A |
| 3 · bytes (Lane B) | N/A |
| 3 · bytes (Lane P) | N/A |
| 4 · post-state hash | N/A |
| 5 · accepted programs | **MOVES**, ★ **in both directions**: `xs.last()` now evaluates, **and** a program using `last` as an identifier no longer parses. |
| 6 · metering | N/A — resolved 2026-08-03 under the token model (§2.2): method pricing is a diagnostic label on either surface, so the historical question (does MeTTaIL's `last` reuse `nth`'s price?) has no consensus content. This closed the register's one `UNVERIFIED` cell. |

**The disagreement.** MeTTaIL against upstream Rholang, in **both** directions — a superset breach that
was measured rather than assumed. That is a REGRESSIVE component inside a PERMISSIVE change.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** MeTTaIL lacks a projection upstream has (see **CBR-024**).

**★ The disclosed cost.** The reservation is structural: *"44 method-shaped rules; **no generic method
production**"* against *"55 `table.insert` in `reduce.rs`'s `method_table`"* — the method API is
**hardcoded into the grammar**, so every method addition reserves a word. That is an open architectural
finding, not a defect of this change. **CITED**.

**Authority.** ★ The owner approved on the FIPS's own usage: the FIPS writes `trace.last()` literally.
**CITED** (campaign ledger, 2026-07-28).

---

### CBR-L08

**The kv element-category gate — `{| @a : @b |}` refused, not silently emptied.**

| | |
|---|---|
| Commit(s) | **IN FLIGHT** — ruled and dispatched; describes *intended* behaviour. |
| Status | IN FLIGHT |
| Direction | **REGRESSIVE** |
| Evidence grade | WITNESSED |
| Files | `macros/src/gen/runtime/wpda_codegen/` — the element-category gate at `wpda_walker.rs:15667` (`if !is_kv`) |

#### (a) The issue

The element-category gate is **disabled for key/value slots** (`if !is_kv`), so a Name in a `Proc` slot
is **silently dropped**. Measured: `{| @a : @b |}` parses to
`CastPathmap(PathmapLit(PathMapLit(HashMapLit({}))))` — **an empty map, with the `@a : @b` entry
absent.** The map sibling `{ @a : @b }` drops identically. Four live kv slots are affected.
`{| *@a : *@b |}` **retains**; category is the sole discriminator. **MEASURED** (campaign ledger,
2026-07-29).

⚠ Separately and under the same repair: `{| 1 |}` currently yields `{|1:1|}` — the key duplicated into
the value — which matches **neither** upstream's flat semantics **nor** the owner's unset ruling.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — the map is no longer silently empty. |
| 2 · verdict | **MOVES** — a match against a path-map literal. |
| 3 · bytes (Lane B) | **MOVES** |
| 3 · bytes (Lane P) | **MOVES** |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | ★ **MOVES, REGRESSIVELY** — a form that **parses today will stop parsing**. |
| 6 · metering | NO |

**The disagreement.** Against upstream: upstream **rejects** `{@a : @b}` with a generic `UnexpectedVar`
(`Validated::Fail`); MeTTaIL silently **emptied the map**. ★ So *"MeTTaIL was **unsound rather than a
superset**, and refusing loudly and specifically is both the compatible answer and the better one."*
**CITED** (campaign ledger, 2026-07-29).

**Blast radius.** Every program using a Name (`@p`) in a kv slot of a map or path-map literal.
Reachable: **yes**. ⚠ ★ **This is the direction reviewers care about most**, and the register states it
plainly: source that parses today will be refused.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Silent data loss at the **parser**, which is the worst place for
it: `#116`'s trie map is built from those entries, `#125`'s set semantics cannot be tested if entries
vanish, and `#74`'s unset-versus-`Nil` distinction is already broken before the normalizer sees the
term.

**Why refuse rather than widen the category.** Because upstream decides it. Upstream's `key_value_pair`
is `seq(field('key', $._proc), ':', field('value', $._proc))` — **both slots `Proc`, both rejecting a
bare Name**. So MeTTaIL's pathmap kv slots are `Proc`, *"and the declared category was right all
along."* **DERIVED** from the normative grammar. **CITED**.

**Why the fix lands at the kv gate rather than in the pathmap.** *"since pathmap kv now has the same
semantics as map kv, the fix is the **kv gate** — all four live kv slots at once, not a pathmap-specific
patch."* **CITED**.

**★ The three rulings compose without conflict**, which is the argument that the repair is principled
rather than local:

| form | disposition | source |
|---|---|---|
| `{\| @a : @b \|}` | **refuse** — Name in a `Proc` slot | this ruling, via upstream map |
| `{\| *@a : *@b \|}` | accept | both operands are `Proc`s |
| `{\| @a \|}` | **refuse** | same category rule, bare form |
| `{\| *@a \|}` | accept, value **UNSET** | `#74` + upstream's flat form |
| `{\| 1 \|}` | accept, value **UNSET** | `#74` |
| `{\| 1 : 2 \|}` | accept | this ruling |

**Authority — the ruling, verbatim, with its date.**

> **2026-07-29T16:42:44Z** — *"Upstream rholang does not support k:v pairs for pathmap but it does for
> maps, use the same semantics for pathmap k:v pairs."*

And the earlier ruling this composes with, **2026-07-29T14:04:34Z**:

> *"pathmap literals do not implicitly carry multiplicity, but their key->values can be used to model
> multiplicity … I told you not to introduce unit into rholang grammar, treat unit values as unset
> (which differ from Nil values which are explicitly set)"*

#### Evidence

- The measurement that established the drop is **pinned as measured, not endorsed**: the row's original
  job was the `|}` fork's stability, and the empty-map result was noticed in passing. **CITED**.
- ⚠ **What would change if the implementation diverges from this design**: if the repair widens the
  element category instead of refusing, Axis 5 flips from REGRESSIVE to PERMISSIVE and MeTTaIL becomes a
  strict superset of upstream **in a way upstream does not sanction**. **Re-check when the commit
  lands.**

---

### CBR-L10

**A pathmap's entries come from a projection, not a field.**

| | |
|---|---|
| Commit(s) | `832d510f` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `rholang-runtime/src/` — four sites |

#### (a) The issue

The Surface-L mirror of **CBR-012**: four sites read a pathmap's entries from a stored `Vec` field rather
than from the trie projection.

#### (b) How it (potentially) breaks consensus

Axis profile is **CBR-012**'s, on Surface L: value NO, verdict MOVES, both byte lanes MOVE for non-ground
maps, hash MOVES, acceptance NO, metering NO.

#### (c) Why the change was necessary or correct

It keeps the two implementations' path-map identity in step. Its derivation is the strong part: the four
sites were **derived from the `models` commit's own deletion list**, not searched for. ⚠ **A disclosed
gap in the same breath**: *"two CI jobs lack the f1r3node sibling checkout entirely"*, so the
cross-repository agreement this entry rests on is **not gated in CI**. **CITED** (campaign ledger).

**Authority.** Follows the `EPathMap`-must-be-a-trie-map ruling (**CBR-011**).

---

### CBR-L11

**The `UInt32` acceptor is narrowed to canonical spellings.**

| | |
|---|---|
| Commit(s) | `4aa64cb6` |
| Status | LANDED |
| Direction | REGRESSIVE |
| Evidence grade | WITNESSED |
| Files | `languages/src/*.rs`, `macros/src/gen/syntax/display.rs` |

#### (a) The issue

The `UInt32` literal acceptor admitted spellings its `Display` never writes, so one denotation had
two accepted surfaces. The acceptor is **narrowed** to exactly the spellings the printer emits — a
deliberate acceptance-set decision (the same class as **CBR-002**'s ruled refusal), not a repair of
a wrong answer.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A — every accepted program computes what it did. |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | NO |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | NO |
| 5 · accepted programs | **MOVES** — a program using a non-canonical `UInt32` spelling parsed before and is refused now. |
| 6 · metering | N/A |

**The disagreement.** A non-canonical spelling normalizes on an old build and is refused on a new
one. Surface L does not run consensus today, so this is a future-fork exposure of the acceptance
axis, recorded now while it is cheap.

**Blast radius.** Programs using boundary spellings of `UInt32` literals. Reachable: **yes**.

#### (c) Why the change was necessary or correct

A parser that accepts text its printer never emits has **two spellings for one denotation** — the
canonical-member problem. Narrowing to the printed surface makes the canonical member the accepted
member. The gate lands with the change so the next divergence fails *at the grammar*.

⚠ **Slimmed under the 2026-08-03 criterion.** This identifier formerly covered a six-commit
display/parse round-trip cluster; the five repair commits (`ab13aee0`, `9485d372`, `f2f2351b`,
`5a5cc9b0`, `7244058b`) are retired as bug fixes to [Appendix B.1](#b1-retired-register-entries),
and this entry keeps only the cluster's deliberate acceptance decision. The row's axis profile and
direction were re-derived accordingly and are flagged for owner review.

**Authority.** No owner ruling on the individual repair; the narrowing follows the superset
standard's canonical-member requirement.

---

### CBR-L14

**MeTTaIL's `Bytes` becomes a real byte sequence with a real surface — `![Vec<u8>] as Bytes` plus the `b"deadbeef"` literal — which removes a spurious reading of every string literal, makes `GByteArray` reachable from source for the first time, and closes three byte methods and one rational operator.**

| | |
|---|---|
| Commit(s) | `713e0364` (carrier + literal + `LiteralFamily::Custom`), `5a9efa00` (byte methods), `93155150` (`BigRat %`), `3aea562f` (census re-derivation) — all in `mettail-rust` on `feature/rho-native-set-automata`. Supersedes the HELD state recorded in `2eebf722`. |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | **LATENT** — the mechanism is loaded but no reachable program contained a `Bytes` term before `713e0364`, so nothing on the wire moved. |
| Files | `mettail-rust` at `93155150`: `languages/src/rholang.rs` (`![Vec<u8>] as Bytes`; `literals { Bytes { … } }`; the `Mod` `BigRat` arm; three `M*` method rules + three congruences), `languages/src/rholang/runtime.rs` (`fold_hex_to_bytes`, `fold_bytes_to_hex`, `fold_to_utf8_bytes`, `fold_bytes_nth`, the `CastBytes` arm of `fold_proc_length`), `rholang-runtime/src/rholang_ast.rs` (`lower_arm_cast_bytes`; three `self.method(…)` arms), `rholang-runtime/src/rholang_ast/recursive_oracle.rs` (three `lower_arm_*`), `macros/src/gen/native/mod.rs::is_byte_vector`, `macros/src/gen/mod.rs::generate_literal_label`, `macros/src/gen/runtime/wpda_codegen/prefix.rs` (`LiteralFamily::Custom`, `literal_family_for_category`), `macros/src/gen/syntax/display.rs` (the byte-vector Display arm). Upstream: `models/src/main/protobuf/RhoTypes.proto` at **:230-232**; `rholang/src/rust/interpreter/reduce.rs` at **:3435**, **:4670**, **:4753**, **:4849**, **:4893**, **:4948**, **:8775**, **:9337-9405**; `pretty_printer.rs` at **:2860**; `models/src/rust/string_ops.rs` at **:17-28**. **DERIVED** (read at those refs). |

#### (a) The issue

Three distinct defects at one site, all created by the DECLARATION rather than chosen as behaviour.

**1. `![String] as Bytes` made every string literal ambiguous.** Both `Str` and `Bytes` were string-shaped, so `macros/src/gen/types/enums.rs` emitted a `StringLit` variant for each, and every `"…"` in the language had two readings — `CastStr` and `CastBytes`. It was cohort 9 of `languages/tests/rholang_semantic_predicate_ambiguity.rs`'s D02 golden, `[StringLit] CastStr vs CastBytes`. Upstream cannot express such an ambiguity: `RhoTypes.proto` at **:230-232** carries `string g_string = 3` and `bytes g_byte_array = 25` as TWO DISTINCT types, and the consensus grammar (`rholang-tree-sitter/grammar.js` at **:435-436**) offers `string_literal` and `uri_literal` only, with `ByteArray` at **:424** a TYPE NAME in `simple_type`. **DERIVED.**

**2. A declared `literals { … }` block was silently discarded by the parser.** `classify_literal_patterned` (`macros/src/gen/runtime/wpda_codegen/prefix.rs`) resolved a category's `LiteralFamily` from its `NativeKind` alone. Any carrier outside the built-in families returned `None`, and the rule fell through to `AtomicShape::NonAtomic` — after the block had been parsed, validated, desugared into a `TokenDef`, and compiled into the lexer DFA. The token was produced and nothing could consume it. This is why `2eebf722` measured `Bytes` as having no surface at all rather than merely no literal. **MEASURED** (`2eebf722`: `gen_rholang_prop::bytes_display_parse_roundtrip` — `arb_bytes produced unparseable surface term ""`, eleven rows).

**3. `Bytes` was unreachable, so every byte method's gap was invisible.** Walking every `fn <name>_method` in `reduce.rs` and keeping those with a `GByteArray` arm gives EIGHT: `nth` (**:4670**), `last` (**:4753**), `toByteArray` (**:4815**), `hexToBytes` (**:4849**), `bytesToHex` (**:4893**), `toUtf8Bytes` (**:4948**), `length` (**:8775**), `slice` (**:8857**). ⚠ **The count is axis-dependent and every previously-quoted figure was correct on some other axis** — [CBR-L13](#b1-retired-register-entries) records "1 of 9" on the byte-PRODUCING axis (methods $`\cup`$ five crypto system processes); the byte-NAMED axis gives "1 of 4"; the `GByteArray`-arm axis gives 8. **The axis nobody had measured is the one that mattered: of those eight, the number that ACCEPTED a `Bytes` receiver in MeTTaIL's host fold lane was ZERO** — `fold_proc_length` matched `CastStr`/`CastList`/`CastMap`/`CastBag`/`CastSet` and not `CastBytes`; `LNth`/`LLast` matched only `Proc::CastList`. Since `length`/`nth`/`last` ARE keys of the reducer's `method_table`, the two lanes DISAGREED: the machine answered a value, the fold answered `error`. **MEASURED** (RED, `93155150`'s parent: `` `b"dead".length()` … left: None, right: Some(2) ``).

**4. `BigRat %` was missing.** Measured over the full 13-operator × 6-carrier cross product (78 cells), four cells did not answer in their operand carrier: `BigRat %`, `Float %`, `Float bitand`, `Float bitor`. Only `BigRat %` is a gap against upstream, whose `combine_mod` `GBigRat` arm (`reduce.rs` at **:3435-3444**) answers the rational ZERO for a non-zero divisor. **MEASURED** (`languages/tests/rholang_arith_carrier_matrix.rs`).

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES**, in two places, both CORRECTIONS. (i) `b"…".length()` / `.nth(i)` / `.last()` answered the `error` term in the host fold lane and now answer what the reducer answers — the lanes were in disagreement and now agree. (ii) `6r % 3r` answered `error` and now answers `0r`, which is what `combine_mod`'s `GBigRat` arm answers. Elsewhere NO: the reduction relation is untouched. |
| 2 · verdict | **MOVES** — a `Bytes` and a `Str` of the same content were previously INDISTINGUISHABLE (both lowered through `new_gstring_par`; [CBR-L13](#b1-retired-register-entries) corrected the target and this completes it) and are now distinct, so an `==` or a spatial match between them answered *equal* and now answers *unequal*. Latently — see the grade. |
| 3 · bytes (Lane B, bincode) | **MOVES** for any `Bytes` term: the AST payload changes from `Bytes::StringLit(String)` to `Bytes::BytesLit(Vec<u8>)`. **MEASURED: no golden moved** — `rholang-codegen`'s `a_s5_5_byte_identity_pins` (2 rows) and `a_s5_6_byte_identity_pins` (3 rows) pass unedited, because no pinned term contains a `Bytes`. |
| 3 · bytes (Lane P, prost) | **MOVES** for any `Bytes` term reaching the wire — `ExprInstance` field **25** (`g_byte_array`, length-delimited bytes) rather than field **3** (`g_string`, length-delimited UTF-8). ⚠ Additionally, the payload is now the bytes THEMSELVES rather than a UTF-8 re-encode of a `String`, so a non-UTF-8 sequence such as `b"80c328fe"` is expressible for the first time — it could not be held by the old carrier at all. |
| 4 · post-state hash | **MOVES** — strictly downstream of the bytes. **MEASURED: no fingerprint moved** (`s6`/driver fingerprints in `a_s5_6_byte_identity_pins`, `rho_rholang_conformance` 23 rows). |
| 5 · accepted programs | **MOVES — and this is the largest axis here.** ADDED: `b"<even run of hex digits>"` (previously not a term: Rholang has no juxtaposition, so an identifier abutted to a string literal did not parse); `"…".hexToBytes()`, `b"…".bytesToHex()`, `"…".toUtf8Bytes()`. REMOVED: nothing — but the READING SET shrinks, since a `"…"` literal no longer has a `CastBytes` reading. RESERVED: three new keywords (`hexToBytes`, `bytesToHex`, `toUtf8Bytes`), since every method terminal is keyword-reserved in this grammar. |
| 6 · metering | **NO** — ⚠ **filed as `NO`, not `N/A`; see the editorial note above.** The three routed methods reach `reduce.rs`'s existing `hex_to_bytes_cost` / `bytes_to_hex_cost` handlers **unchanged**, so no cost function is added, altered, or bypassed — a *measured* non-movement rather than an inapplicable axis. ★ Contrast [CBR-024](#cbr-024) / [CBR-025](#cbr-025), which move this axis by adding a **new charge site for a new method**: three methods are added here and all three reuse existing prices. MeTTaIL itself carries no metering surface **by ruling** — budgets are F1r3node's (`wallet.txt`) — and that ruling is why nothing was built, not why the axis has no answer. |

**The disagreement.** A node on the old code and a node on the new code would disagree on the post-state of any program containing a `Bytes` term: the old node writes field 3 with the literal's UTF-8 bytes, the new node writes field 25 with the byte sequence, and the two `Par`s hash differently. They would also disagree on `6r % 3r` (`error` vs `0r`) and on `b"dead".length()` (`error` vs `2`) in the host lane. Fault class: **safety fork**.

**Blast radius.** Every program containing a `Bytes` term, a byte method, or a rational modulo. ★ **The `Bytes` half of that set was EMPTY before `713e0364`, for a structural reason rather than by luck**: a `"…"` literal elects `Str`, and `CastBytes` had no other construction path in the spec — MeTTaIL had none of upstream's byte-producing builtins. That is exactly the enumeration [CBR-L13](#b1-retired-register-entries) recorded, and this entry closes it. The `6r % 3r` half is reachable by an ordinary deploy but answered `error`, which no correct program depends on.

**Could live chain state have been produced under the old behaviour?** **NO, and it is settled from inside the repository.** No MeTTaIL term has entered a block, and on Surface L the `Bytes` construction path did not exist. For the record, the query that would settle it if it had: scan every deploy term for a `GByteArray` expression whose provenance is a MeTTaIL-compiled `Par` — i.e. `SELECT … FROM deploys WHERE term LIKE '%g_byte_array%'` against the block store, cross-referenced with the MeTTaIL-compiled deploy set (currently empty).

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Three things, each independently sufficient. (i) A wire model in which `Bytes` and `Str` are the same type — the distinction `RhoTypes.proto` makes is simply lost, and [CBR-L13](#b1-retired-register-entries)'s corrected lowering target cannot express a non-UTF-8 byte array. (ii) Every string literal in the language has a spurious second reading, which the disambiguator must clean up on every parse; the ambiguity was created by the declaration and was never a behaviour anyone chose. (iii) `Bytes` remains a category that is constructible in Rust and neither writable nor printable in the language — a broken $`\mathrm{Display} \rightarrow \mathrm{parse}`$ invariant for a whole category.

**Why this repair rather than the alternatives.**

- **`0x…` as the literal spelling — REFUTED, and my own earlier argument for it was INVERTED.** I argued for `0x…` on upstream-alignment grounds. MeTTaIL already spends `0x…` on THREE hex INTEGER forms (`Int`, `BigInt`, `BigRat` radix patterns in the same `literals` block), and upstream Rholang has **zero** `0x…` syntax anywhere. It is refuted twice over.
- **"A builtin plus a type pattern, NOT a literal" — REJECTED.** This was `2eebf722`'s own guess at the faithful shape, and it answers the wrong question: a builtin gives a byte array a PRODUCER, and what was missing is a SURFACE — something `Display` can write and the parser can read back. (The builtins are added here too, so the alternative is subsumed rather than dismissed.)
- **Suppressing the failing Display-roundtrip rows — REJECTED.** They are the only thing that noticed the defect. `2eebf722` refused this and was right to.
- **A new `NativeKind` variant per exotic carrier — REJECTED** in favour of electing `LiteralFamily::Custom` from the DECLARATION. A per-carrier enumeration is a hand-maintained mirror of an open set; keying on "did this category declare a `literals { … }` block with an `eval`?" is the author's own statement that a surface exists, and it leaves every carrier that deliberately has none untouched (`Set`, `Pathmap`, `ReadZipper`, `WriteZipper` are all `NativeKind::Other` and declare no block).
- **Repairing upstream's `hexToBytes` decoder — REJECTED, and this is the one place where "fix upstream's bugs" does NOT apply.** `StringOps::unsafe_decode_hex` filters out non-hex characters and left-pads an odd-length result (`"hello world"` $`\Rightarrow`$ `[0xed]`), explicitly matching Scala's `Base16.unsafeDecode`. It is consensus-reachable — `Registry.rho` calls `hexToBytes` on public keys — so its decoder decides computed values on a live path. A deliberate, documented, load-bearing behaviour is not a bug to fix. Reproduced exactly and pinned so nobody cleans it up.
- **Declaring `Float %` — REJECTED as a divergence dressed as completeness.** Upstream refuses it *explicitly*, with a message (`reduce.rs` at **:3425**, `"Modulus not defined on floating point"`). ⚠ Note this is the one place the IEEE-754 disposition does not extend: float ÷0 answers IEEE-754 (pinned: `inf`, `-inf`, `NaN`) and [IEEE754-2019](#references) §5.3.1 does define `remainder`, but upstream declines it and upstream is the floor on SEMANTICS.
- **Declaring `Float bitand`/`bitor` — REJECTED by ruling, not omitted.** Upstream has NO bitwise operators anywhere (derived: zero occurrences across `f1r3node-rust-mettail`, none in the consensus grammar), so these are MeTTaIL-only and the disposition is ours. A bitwise op on an IEEE-754 float has no arithmetic reading; the only implementable one masks the bit PATTERN, which is not a function of the represented VALUE (`0.0` and `-0.0` are equal with different patterns), breaking the property every other arm has.

**★ Upstream is internally inconsistent on the byte-array SURFACE, and this rules rather than replicates.** `pretty_printer.rs` at **:2860** prints `GByteArray(bs) => Ok(hex::encode(bs))` — **bare hex, which upstream's own grammar cannot read** — while `par_to_sexpr.rs` at **:107** spells the same value `0x…`. Upstream prints a value in two forms, neither parseable by upstream. `b"…"` frames **exactly** `hex::encode`'s digits, so the digits agree byte for byte with what upstream prints and with what `hexToBytes` reads, and the round trip upstream loses is recovered. Classified **BUG FIX**, never DIVERGENT.

**Authority.** Owner rulings, verbatim with their dates:

> *"How are `Str` and `Bytes` modeled by upstream Rholang? We should construct our rholang spec to align with upstream's in that regard."* — 2026-07-29
>
> *"If upstream Rholang already has syntax and semantics for bytes, we should use the same"* — 2026-07-29
>
> *"yes, dispatch the carrier change to `Vec<u8>`"* — 2026-07-29
>
> On the literal spelling, after asking how C++ does it: **`b"deadbeef"`, implemented via `LiteralFamily::Custom`** — 2026-07-29
>
> *"We should support everything upstream supports correctly, but should fix any bugs that upstream has and make it more debuggable."* — standing
>
> On the missing arithmetic operators: **"declare them explicitly"** — 2026-07-30

**★ Sibling enumeration, ON FOUR NAMED AXES.**

- **Axis: categories declaring `![String]` where a byte sequence was meant.** **Count: 1 on this axis** — only `rholang` declares a `Bytes` category (`grep -rn 'as Bytes' languages/src/*.rs`); `rhocalc` and the other eleven bundled languages have none. The sibling that would have shared the defect does not exist.
- **Axis: categories whose declared `literals { … }` block was silently discarded by the family lookup.** **Count: 1 on this axis** (`Bytes`) — and the *repair* is general: `literal_family_for_category` is now the single election site and any future carrier outside the built-in families is covered automatically. The DERIVED gate (`rholang_literal_surface_census.rs`) computes the domain from the grammar, so a new such category joins it without an edit.
- **Axis: `method_table` entries with a `GByteArray` arm that MeTTaIL cannot reach.** **Count: 8 before, 1 after** — `slice` alone remains, and its absence is not a byte gap: it is missing for `String`, `List` and `ByteArray` alike, so declaring it means one rule with three carrier arms plus a keyword reservation affecting all categories. Reported, not smuggled in. The five crypto builtins of [CBR-L13](#b1-retired-register-entries)'s axis (`sha256Hash`, `keccak256Hash`, `blake2b256Hash`, `secp256k1Verify`, `ed25519Verify`) are unforgeable CHANNELS, not method-table entries, and remain absent for the same reason.
- **Axis: operator × carrier cells that do not answer in their operand carrier.** **Count: 4 of 78 before, 3 after** — and the surviving three are RULED, not missing. ⚠ The four cells #115 named (`BigRat -`, `UInt32 -`, `UInt32 *`, `UInt32 /`) all compute and preserve their carrier; the filing's premise is refuted by measurement.

**★ Where this entry claims something needed no change, the GUARD is named.**

| claim | falsifier |
|---|---|
| no serialized byte or fingerprint moved | `rholang-codegen`'s `a_s5_5_byte_identity_pins` (2) + `a_s5_6_byte_identity_pins` (3); `rholang-runtime`'s `rho_rholang_conformance` (23) |
| the `Str`/`Bytes` ambiguity is unspellable, not merely unelected | `rholang_byte_literal.rs::a_string_literal_has_no_byte_array_reading` |
| every declared literal has a reachable surface | `rholang_literal_surface_census.rs::every_category_declaring_a_literal_has_a_reachable_surface` — domain DERIVED from the reconstructed `LanguageDef`, floored against vacuity |
| `hexToBytes`'s filter-and-pad oddity is preserved exactly | `rholang_byte_methods.rs::hex_to_bytes_filters_and_pads_exactly_as_upstream_does` |
| `Float %`/`bitand`/`bitor` stay `error` BY RULING | `rholang_arith_carrier_matrix.rs::RULED_NON_PRESERVING`, checked in BOTH directions — a ruled cell that STARTS preserving its carrier also fails |
| float ÷0 answers IEEE-754 | `rholang_arith_carrier_matrix.rs::float_division_by_zero_answers_ieee_754` |
| every exact carrier fails closed on ÷0 and %0 | `::modulo_and_division_by_zero_fail_closed_at_the_exact_carriers` (9 cells) |
| the two `Fixed` divergences are known and unrepaired | `::the_two_fixed_point_divergences_are_pinned_with_upstreams_answer`, with upstream's answer written down |

★ **All eight falsifiers are executable test functions**, which is why this row set is admissible as a
falsifier table at all. ⚠ Contrast the defect recorded against
[CBR-L13](#b1-retired-register-entries)'s evidence: three of the six pin sites `2eebf722` named are **prose references, not
tests**. A named falsifier that is not a test is the coverage-overclaim shape
[§7.8.6](#7-maintenance) treats as the register's own recurring failure, so the
distinction is worth asserting positively here rather than assumed.

#### Evidence

**DERIVED** — the D02 unresolvable-ambiguity golden moved **$`9 \rightarrow 8`$**, re-derived by rebuilding rather than edited: `warning[D02] (Rholang): 8 unresolvable ambiguity in 1 categories`. ⚠ Row 9 (`[StringLit] CastStr vs CastBytes`) was **REMOVED, not RESOLVED** — the other eight are inherent grammar conflicts settled by a tropical weight, while row 9 was a declaration artefact whose losing reading is now unconstructible.

**MEASURED** — the guards, RED then green. Census gate, under a revert of the carrier and the literals block:

```text
thread 'every_category_declaring_a_literal_has_a_reachable_surface' panicked at
languages/tests/rholang_literal_surface_census.rs:109:5:
`Bytes` must DECLARE its literal surface, and it does not. Derived domain:
["BigInt", "BigRat", "Fixed", "Float", "Int"].
```

The derived domain moves $`5 \rightarrow 6`$. Byte rows, under the same revert, do not compile: `error[E0599]: no variant … named BytesLit found for enum mettail_languages::rholang::Bytes`. Byte methods, before the fold arms: `` `b"dead".length()` must be 2 … left: None, right: Some(2) ``, and `` `"deadbeef".hexToBytes()` must parse: "1:12: … found identifier `hexToBytes`" ``. `BigRat %`, before the arm: `1 of 78 cells do not preserve their operand carrier and are NOT among the ruled exceptions: 6r % 3r answered error (want BigRat)`.

**MEASURED** — the acceptance matrix at `93155150`: `languages` 1072 rows / 17 suites green; `rholang-runtime` 107 rows green; `rholang-codegen` byte identity 5/5, zero fingerprints moved; `macros` 438 green after the census re-derivation. Derived counts that moved and why: `gen_rholang_rewrite` $`171 \rightarrow 174`$ (+3 congruences), `gen_rholang_unit` $`176 \rightarrow 179`$ (+3 rules), `rholang_dovetail_fold` $`6 \rightarrow 7`$, `factoring.rs` method cohort $`44 \rightarrow 47`$ (three `LoneRootChild` singletons, still zero factorable groups).

**UNVERIFIED** — the two `Fixed` divergences surfaced by the matrix are reported, pinned, and NOT repaired, because each moves a computed value and needs an owner ruling: (1) `Fixed %` computes $`a - \operatorname{trunc}_{p}(a/b)\,b`$ where upstream computes the integer-quotient remainder (`7.00p2 % 3.00p2` is `0.01p2` here, `1.00p2` upstream; `7.50p2 % 2.00p2` is `0p0` here, `1.50p2` upstream) — not an arithmetic slip, since `checked_rem` is the matched pair of `checked_div` and the two satisfy $`q b + r = a`$; (2) mixed scales are accepted here (`7.00p2 + 3.000p3` $`\Rightarrow`$ `10.000p3`) and refused upstream (`OperatorExpectedError`).

★ **This entry CLOSES [open question 8](#63-known-open-questions)** — *"MeTTaIL cannot construct a byte array at
all … so `"deadbeef".hexToBytes()` is unsayable"* — whose premise is now false by measurement. ⚠ The
closure is **partial and the residue is named**: construction is possible and three byte methods exist, but
**5 of the 9** byte-producing surfaces on [CBR-L13](#b1-retired-register-entries)'s axis are still absent, all five being
unforgeable crypto **channels** rather than method-table entries. **MEASURED** (the question's premise) +
**DERIVED** (the residue, from the sibling enumeration above).

---

## 5. Risk analysis

### 5.1 Aggregate axis exposure

Projected from the 19 rows of §4.1 (each column counts `●` cells):

| Axis | entries that move it | share of the 19 |
|---|---:|---:|
| computed value (V) | **7** | 37 % |
| verdict (T) | **12** | 63 % |
| bytes, Lane B (B) | **10** | 53 % |
| bytes, Lane P (P) | **8** | 42 % |
| post-state hash (H) | **12** | 63 % |
| accepted programs (A) | **9** | 47 % |
| metering (M) | **0** | 0 % |

The metering row is a **result of the 2026-08-03 re-derivation**, falsifiable per entry: each kept
entry's M cell carries its one-line derivation against the token model (§2.2), and a future entry
that genuinely moves the committed COMM count or the funding surface re-opens the row.

### 5.2 The highest-risk entries

1. **CBR-044** — the EPM1 wire transition: six axes move, the blast radius is every block,
   signature, event, and cold-store record containing an EPathMap, and it is the entry the
   coordinated version bump exists for. Its equivalence and round-trip evidence is the deepest in
   the register (entry body + the PathMap report).
2. **CBR-027 + CBR-030** — the conjunction instance: the checked-arithmetic ruling forces the
   genesis contract's overflow guard total, so the genesis term moves; neither is reviewable alone.
   CBR-027 also carries the register's sharpest chain-history question (§5.4).
3. **CBR-L08** — the one in-flight entry, and REGRESSIVE: source that parses today will be refused.
   It ships only inside the same coordinated bump.

### 5.3 Direction profile

Projected from §4.1: **11 CORRECTIVE** (the data-model lineage and schema additions, corrective in
the sense that the representation now matches the ruling, while remaining deliberate transitions),
**4 PERMISSIVE** (CBR-024, CBR-025, CBR-037, CBR-L07), **4 REGRESSIVE** (CBR-002, CBR-027, CBR-L08,
CBR-L11). Total 19.

### 5.4 The chain-history questions

Two kept entries carry a question about *history* that cannot be settled from inside the repository,
both consolidated here so a reviewer can commission them together:

| entry | query | status |
|---|---|---|
| **CBR-027** | replay the chain under an instrumented build counting `GInt` `+`/`-` evaluations where `checked_add`/`checked_sub` would return `None`; any non-zero count is a program whose historical result the change alters | **UNVERIFIED** — should run before the bump |
| **CBR-030** | whether any live state referenced the previous `NonNegativeNumber.rho` term digest | subsumed by the pre-production ruling; re-opens if the ruling changes |

The owner's pre-production ruling is what makes both questions advisory rather than blocking; they
are retained because the ruling, not the evidence, is what discharges them.

### 5.5 The conjunction risk

No activation-height machinery exists: `Validate::version` is exact equality, so the 19 entries ship
as one coordinated protocol-version bump. The reviewer's object of study is therefore the
**conjunction**: if entry $`i`$ carries residual risk $`r_i`$, the bump carries
$`1 - \prod_i (1 - r_i)`$, and the CBR-027/CBR-030 pair is the register's concrete demonstration
that entries interact (§5.2).

---

## 6. Threats to validity

### 6.1 What this report did not do

1. **It did not re-run the campaign's measurements.** Every number quoted from a commit message is tagged
   **CITED**, not **MEASURED**. A commit that measured wrongly, or that measured a different thing from
   what its prose says, would pass this sweep.
2. **It did not read generated code.** The wire tables in `OUT_DIR` are produced by `models/build.rs`;
   this report read the generator, not its output.
3. **It did not verify the one remaining in-flight entry's tests.** **CBR-L08** describes
   *intended* behaviour and states what would change if the implementation diverges from the
   design. (CBR-027, in flight when first written, has since landed and its entry is measured.)
4. ★ **The tree moved under it.** **CBR-007** was "in flight" when its entry was first written and
   **landed during authoring** (`b219e199` at 14:42, `dc383ed1` at 14:44). Its entry was rewritten from
   the shipped commits; three of its axis values and its entire evidence section are now **measured**
   rather than predicted. Any other entry could move the same way between this writing and a review —
   which is §7's whole argument.

### 6.2 ★ Known-false claim in a shipped commit — disclosed, and now measured

⚠⚠ **`f5fd6c34`'s commit body contains a claim that was FALSE when it was written.** It asserts, in the
commit message, in `aggregate_updates`' own doc comment, and in `metrics_constants.rs`, that the restored
duplicate-variable guard is a defence-in-depth backstop *expected to be silent forever*:

> *"Whole `-p rholang` suite, 49 targets, 1267 passed / 0 failed / 8 ignored: the refusal marker appears
> exactly TWICE, and both firings are inside `tests/matcher_state_isolation.rs` … **Zero refusals from any
> production path.** That is the direct evidence that verdicts did not move — not a proxy."*

**Why it is false, and it is a premise error rather than a measurement error.** The argument had four
clauses; three are true. The third — *"each level occupies at most one pattern position by linearity"* —
is **a property of ONE free map**, and the matcher has several in flight. `~P` and `P \/ Q` bodies are
each normalized against a **fresh** `FreeMap` which `combine_p_negation` then **discards**
(`p_negation_normalizer.rs:10-13, 65-89`), so a free variable under `~` is `FreeVar(0)` in a numbering
nobody kept — while the enclosing pattern's level 0 is a *different* variable in the *same shared*
`SpatialMatcherContext::free_map`.

**★ The magnitude, measured — the whole `-p rholang -p models` suite, marker grepped:**

| build | firings | breakdown |
|---|---|---|
| before `b219e199` | **8** | **SIX from PRODUCTION paths** — three verdict-level matcher fixtures and **all three** end-to-end reductions — plus the two deliberate direct-call fixtures in `matcher_state_isolation.rs`. |
| with `b219e199` | **2** | **only** those two deliberate fixtures. |

Identical in release. **The claim was wrong by six.** **MEASURED** (`b219e199`, `dc383ed1`).

**Why it is nonetheless not a defect in what shipped.** `match_function`'s isolation was working exactly
as `f5fd6c34` specified; what was wrong was the **generalisation** from the measured corpus to "any
production path", and the linearity premise it rested on. The fix `f5fd6c34` made is correct; its entry is retired as a bug fix
([Appendix B.1](#b1-retired-register-entries)).

**★ The remedy, and why history was not rewritten.** Per the project's standing rule that published
history is not rewritten, the correction is **not** an amendment to `f5fd6c34`. `dc383ed1` — a
documentation-only commit — records it **at the two addresses that carry the claim**: `aggregate_updates`'
doc comment and the refusals-counter comment in `metrics_constants.rs`. A reviewer reading either site
now finds the correction beside the claim rather than discovering the discrepancy themselves, which is
the worst way to find it.

**What a reviewer should take from this.** Not that the measurement was sloppy — it was exact for the
corpus it ran on — but that **a corpus measurement was quoted as a universal**. That is the same failure shape as §6.5(2), and it is why every number in this report carries a
**CITED** / **MEASURED** tag saying who observed it.

### 6.3 Known open questions

| # | Question | Where it came from | Status |
|---|---|---|---|
| 1 | Can `Compiler::top_level_error`'s `HashMap`-ordered variable list reach the persisted `ProcessedDeploy::system_deploy_error`? **64 calls on one source produced 2 distinct strings.** | the campaign's admission audit (E102) | ⚠ **OPEN.** If yes, it is an unregistered ordering nondeterminism inside block-resident bytes. |
| 2 | `#109 residual 3` — a peer-steerable `decode_trie_path(..).unwrap_or(..)` via an `EZipper.current_path` that is not a valid codec path. | campaign ledger | **RULED** *"stuck term"* (owner, 2026-07-29), **not started**; becomes an entry when it lands. |
| 3 | Two CI jobs lack the f1r3node sibling checkout, so the cross-repository agreement **CBR-L10** rests on is not gated. | campaign ledger | ⚠ **OPEN.** |
| 4 | **How should a `GByteArray` render?** Upstream is internally inconsistent (bare hex vs `0x…`), and neither form is parseable by upstream's own grammar. | **CBR-L14** evidence | ⚠ **OPEN** — a grammar decision; the held `![Vec<u8>] as Bytes` carrier is blocked on it. |
| 5 | **Is the test genesis builder's `post_state_hash` non-determinism confined to the test builder?** Six builds produced six distinct hashes at byte-identical source; the leading hypothesis is `GenesisParameters`' seeded `HashMap<PublicKey, i64>` iteration. | **CBR-030** evidence | ⚠ **OPEN, and the most consequential**: a network whose genesis post-state depends on the run cannot agree on genesis. |

Questions this table formerly carried that are **closed**: the EPathMap read-ceiling reachability
and the FFI too-deep-`Par` panics (both mooted by CBR-044's ceiling-free generated readers), the
`NaN`-comparison split (ruled and landed with the retired float entry: comparison arms follow
IEEE 754 §5.11 while the carrier keeps reflexive structural identity — making the carrier follow
IEEE was measured to stop the rewrite engine terminating), the byte-array construction gap (closed
by CBR-L14), and MeTTaIL `last` pricing (resolved under the token model, §3.3).

### 6.4 UNVERIFIED budget

**Zero axis cells read `UNVERIFIED`.** The one historical `?` cell — CBR-L07's metering — resolved
under the token-model re-derivation (§3.3). What remains unverified is **history, not code**: the
chain-history queries of §5.4, unanswerable from inside the repository by construction and
discharged in practice by the pre-production ruling.

### 6.5 Coverage asymmetry between the two surfaces (historical)

⚠ **The original Surface-N sweep was an exact partition; the Surface-L sweep was not** — retained
as the honest account of how the change set was derived (the mechanised gate that once enforced
the partition was removed with the 2026-08-03 re-scope; §7).

| | Surface N | Surface L |
|---|---|---|
| Commits in the campaign window | 111 | 236 |
| Commits touching the path set | **78** | 149 |
| Covered by the original sweep's entries | **57 in-range SHAs** | 16 in-range SHAs |
| Explicitly exempted with a reason | **21** | **0** |
| Partition exact? | **Yes** — 57 + 21 = 78 | **No** — 133 commits are neither an entry nor an exemption |

The Surface-L entries are a **targeted selection** of semantics-moving changes found by keyword and by
reading the campaign ledger, not an exhaustive partition. A Surface-L change that moves an axis and does
not use this report's search vocabulary **would be missed**. Closing this is the first extension §7's
gate should be given.

### 6.6 Method false-negatives

Restated from §3.4, with one now confirmed by the sweep itself:

1. ★ **A consensus-visible change outside $`\mathcal{P}`$** — **CONFIRMED.** The initial path set omitted
   `rho-pure-eval/src`, which contains the **guard evaluator** — the component that decides `where`
   verdicts. Two entries live there (`eaa44c2f` in **CBR-002**; `a3fd6fe4` in a retired conversion row, [Appendix B.1](#b1-retired-register-entries)) and were found
   only because the entry set was assembled from commit *messages* as well as from paths. The path set in
   §3.1 has been corrected; the *general* risk stands, and the retired `Debug`-derives row shows its sharpest form: under
   this report's own definition, **a `prost` or `thiserror` version bump alone** is a consensus change,
   and no source path would show it.
2. **A false neutrality assertion** in a commit message — see §6.1(1).
3. **An emergent divergence with no single owning commit** — two individually byte-neutral changes
   composing into a non-neutral one. A per-commit sweep cannot see it. **No mitigation. Named as a gap.**
4. **Generated code** — the `cargo:rerun-if-changed` stale-`OUT_DIR` defect (evidence in the retired event-hash-encoder row) is the
   measured instance of this class.
5. **Uncommitted and concurrent work** — both trees had substantial uncommitted changes and several
   agents were editing concurrently while this was written. the retired environment-variable row's evidence records a concrete
   instance of the harm: a concurrent whole-file rewrite silently removed a consensus-relevant hunk, and
   it was caught by the **build**, not by review.


---

## 7. Maintenance

The register is **LIVING** and its maintenance contract is four lines:

1. **A may-change-consensus change lands (criterion §3.2) $`\rightarrow`$ a §4.1 row and an Appendix-A-form entry
   land with it**, answering all six axes (metering under the token model), a direction, a grade,
   and the authorising ruling verbatim where one exists.
2. **A change examined and found non-qualifying gets a typed row** — commit-level (Appendix B.2
   vocabulary) or entry-level retirement (Appendix B.1 vocabulary) — with the evidence pointer that
   discharges it.
3. **Identifiers are never reused.** A retired entry keeps its `Former ID` row forever; a superseded
   claim is annotated, never overwritten silently.
4. **Figures in prose are projections of §4.1** and are recomputed from the table whenever a row
   changes — never incremented.

★ **The mechanised drift gate this register once specified and carried
(`casper/tests/consensus_change_register_gate.rs` and its machine index `register.toml`) was removed
on 2026-08-03 by owner ruling** — it was judged very fragile and extremely coupled to this epic,
without contributing to long-term stability — and no replacement gate is designed. The contract
above is the maintenance mechanism.

---

## 8. Conclusions

1. The register holds **19** may-change-consensus entries derived from the campaign record: **14**
   on the F1r3node node, **5** on MeTTaIL's Rholang; **18 landed, 1 in flight**. **45** examined
   changes are retired with typed reasons and **21** commit-level exemptions are retained — the
   negative results that make the criterion checkable.
2. **The axes are genuinely independent and must be reviewed separately.** CBR-014 moves four bytes
   on the bincode lane and zero on the protobuf lane for the same field addition; CBR-011 moves the
   identity relation while every computed value stays fixed; CBR-044 moves six of seven axes while
   the seventh (metering) is proven still by the token model's own definition.
3. **The dominant object of review is the EPM1 transition** (CBR-041 $`\rightarrow`$ CBR-044): a representation and
   wire change whose equivalence evidence — byte goldens by SHA-256, ceiling-free round trips at
   depth 4,096, Rocq/Z3/TLC — is summarized in the entries and carried in full by the PathMap
   report.
4. **Four entries are REGRESSIVE** (§5.3) and are named plainly; one of them (CBR-L08, in flight)
   refuses source that parses today.
5. **The metering axis moves in no kept entry.** Under the token model, consensus cost is the
   committed COMM count; every historical per-op "charge site" claim in this register was a
   diagnostic-weight claim, and the one `UNVERIFIED` cell dissolved with the same derivation.
6. **The rollout is a conjunction** (§5.5): exact-equality version validation means the 19 ship as
   one coordinated bump, and the CBR-027/CBR-030 pair is the in-register proof that entries
   interact.

**Recommendation to the reviewer.** Weigh CBR-044's equivalence evidence first, then the
CBR-027/CBR-030 conjunction (running §5.4's replay query if the pre-production ruling ever
weakens), then the acceptance-set entries as a block.

---

## References

Abbreviations used in the entries below:

| abbreviation | expansion |
|---|---|
| ACNS | Applied Cryptography and Network Security (conference) |
| CRYPTO | International Cryptology Conference |
| LNCS | Lecture Notes in Computer Science (Springer series) |
| MD5 | Message-Digest Algorithm 5 (named only inside the BLAKE2 paper's title) |

- [Milner1992] R. Milner, J. Parrow, D. Walker. *A Calculus of Mobile Processes, I & II.* Information and
  Computation 100(1), 1992. DOI: [10.1016/0890-5401(92)90008-4](https://doi.org/10.1016/0890-5401(92)90008-4)
  and [10.1016/0890-5401(92)90009-5](https://doi.org/10.1016/0890-5401(92)90009-5).
- [Meredith2005] L. G. Meredith, M. Radestock. *A Reflective Higher-order Calculus.* Electronic Notes in
  Theoretical Computer Science 141(5), 2005, 49–67. DOI:
  [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
- [Knuth1984] D. E. Knuth. *Literate Programming.* The Computer Journal 27(2), 1984, 97–111. DOI:
  [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97). — the form of §3.2's Algorithm 1.
- [Aumasson2013] J.-P. Aumasson, S. Neves, Z. Wilcox-O'Hearn, C. Winnerlein. *BLAKE2: Simpler, Smaller,
  Fast as MD5.* ACNS 2013, LNCS 7954, 119–135. DOI:
  [10.1007/978-3-642-38980-1_8](https://doi.org/10.1007/978-3-642-38980-1_8). — `Blake2b256`, the
  event-hash and post-state instance named in §2.3.
- [Merkle1988] R. C. Merkle. *A Digital Signature Based on a Conventional Encryption Function.*
  CRYPTO '87, LNCS 293, 369–378. DOI:
  [10.1007/3-540-48184-2_32](https://doi.org/10.1007/3-540-48184-2_32). — the post-state hash's tree
  structure.

**In-repository sources.** `casper/src/rust/validate.rs`, `casper/src/rust/block_status.rs`,
`rspace++/src/rspace/hashing/stable_hash_provider.rs`, `models/src/main/protobuf/RhoTypes.proto`,
`models/src/main/protobuf/CasperMessage.proto`,
`rholang/src/rust/interpreter/accounting/mod.rs` (the token model's `reconcile_lane`),
`docs/theory/cost-accounting-impl/d3-replace-phlo-with-tokens.md`. The normative Rholang grammar for
Surface-L conformance is `rholang-rs/rholang-tree-sitter/grammar.js`.

---

## Appendix A — the entry template

★ **Copy this verbatim for entry $`N+1`$.** The headings are fixed so the next author fills a form
rather than invents a shape. Do not add or remove sections; if a section does not apply, say so in it.

````markdown
### CBR-0NN

**One sentence: what changed.**

| | |
|---|---|
| Commit(s) | `sha` — or **IN FLIGHT** / **DESIGNED, NOT LANDED** / **OPEN, UNREPAIRED** |
| Status | LANDED / IN FLIGHT / OPEN |
| Direction | REGRESSIVE / PERMISSIVE / CORRECTIVE / NEUTRAL / DIVERGENT |
| Evidence grade | WITNESSED / MECHANISM-ONLY / LATENT / DORMANT / NEUTRALITY-MEASURED / UNVERIFIED |
| Files | `path/to/file.rs:LINE`, … |

#### (a) The issue

What was wrong, then the mechanism with `file:line`. State the defect, not the repair.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | MOVES / NO / N/A / UNVERIFIED — one clause of justification |
| 2 · verdict | … |
| 3 · bytes (Lane B, bincode) | … |
| 3 · bytes (Lane P, prost) | … |
| 4 · post-state hash | … |
| 5 · accepted programs | … |
| 6 · metering | … |

**The disagreement.** What would two nodes disagree about, and under what input? Name the fault class:
safety fork / slashable fault / liveness split.

**Blast radius.** What class of program. Is it reachable by an ordinary deploy?

**Could live chain state have been produced under the old behaviour?** If it cannot be settled from
inside the repository, **say so and give the query that would settle it.**

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.**

**Why this repair rather than the alternatives.** Name the alternatives and why each was rejected.

**Authority.** Owner ruling quoted **verbatim with its date**, or "No owner ruling."

**★ Sibling enumeration, ON A NAMED AXIS.** *"What else has this shape?"* — and the shape must be
spelled. Write **Count: N on axis A**, never a bare **Count: N**. ⚠ A count with no axis recorded is the
same defect as a number with no subject: [CBR-006](#b1-retired-register-entries)'s count was `1` and *correct* on the axis of
`ExprInstance` arms, while the axis that mattered was **pattern position**, where the count is also 1 and
is a different 1. See [§7.6](#7-maintenance) finding 5.

**★ If the entry claims something *needed no change*, name the GUARD.** A test that fails if the unchanged
thing turns out to need changing. A justification with no falsifier inoculates the next reader against
looking, which is drift class 5.

#### Evidence

Quote actual numbers. The RED, the measurement, the acceptance matrix. Tag each **DERIVED** /
**MEASURED** / **CITED** / **UNVERIFIED**.
````

---

## Appendix B — the exemption table

### B.1 Retired register entries

The 45 entries retired under the 2026-08-03 inclusion criterion (§3.2). Each keeps its **former
identifier forever** — identifiers are never reused, and a historical citation of any `CBR-*` below
resolves to this table. Full bodies remain in git history at the pre-refactor revision of this file.
Reasons are the closed retirement enum of §3.2; the evidence column points at where the discharging
material now lives.

| Former ID | SHA(s) | Subject (headline at retirement) | Typed reason | Evidence pointer |
|---|---|---|---|---|
| `CBR-001` | `6bc58743` | A `where` guard participates in candidate selection, not only approval | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-003` | `99b7b1c4` | `matches` guards become decidable (injected spatial-match oracle) | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-004` | `b0f18672` | Thirteen `ExprInstance` arms were missing from the spatial matcher | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-005` | `f5fd6c34`, `eaa905fe` | A matcher attempt owns its own `FreeMap`; the multiplicity guard un-vacuumed | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-006` | `8853f839` | A `matches` pattern is substituted at `depth + 1` | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-007` | `b219e199`, `dc383ed1` | A connective attempt owns its own `FreeMap` — the isolation law at four more sites | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-008` | `a1feb437` | A relative path below the root built a key no `Par` can have | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-009` | `ce7bb4fd` | `setSubtrie` stopped dropping bare source entries | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-010` | `0a6d2ce0`, `5aacebc3` | Path readers ask the codec instead of guessing | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-015` | `76de7d44` | `cursor_kind` reaches the printer, whose output is replay-compared | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-016` | `ca84d535`, `4290303c` | An environment variable leaves the consensus byte path | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-017` | `a4c23a58` | The `New` bind bound becomes an interval; the clamp had moved bytes | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-018` | `bd7cb45f` | The pretty printer renders its `match` target | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-019` | `00ff9187`, `c28f4cf6`, `903cefb3` | The channel leg of every event hash routed through a new encoder | `EQUIVALENCE_PROVEN` | neutrality evidence (exhaustive differential, generator mutations M1–M3) summarized in the stack-safety report §5.3–§5.4 |
| `CBR-019b` | `7c74260d`, `56fb1fd0` | A trampolined prost encoder — built, gated, dormant | `DORMANT` | no caller, established mechanically; superseded by the generated Message family (SS-A8) |
| `CBR-020` | `9a5521a2`, `2bcfaf87`, `000b95d7` | The cold-store read path becomes fallible and heap-bounded | `EQUIVALENCE_PROVEN` | language identity: 1.88 M malformed inputs agree with the derived oracle (stack-safety report §5.3.3) |
| `CBR-021` | `b961d7c4` | A malformed consume refuses instead of killing the process | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-022` | `a09f1de2`, `3b265eb7` | Deploy admission owns its discard — a 43,565-byte deploy stops aborting the node | `EQUIVALENCE_PROVEN` | three-leg byte-neutrality; stack-safety report §5.5.3(a) |
| `CBR-023` | prior set; extended by `mettail-rust@b0aa4e09` | The $`\Theta(\mathrm{depth})`$ conversion programme — living commit set | `EQUIVALENCE_PROVEN` | oracle-gated conversion programme; SS-G7 adds executable oracles and an admission-free Rocq SCC-machine theorem; stack-safety report §5 and its fix register |
| `CBR-026` | `2087c043` | `E(S)` — the enabled-rendezvous query and firing a named selection | `DORMANT` | additive trait API with zero consensus-path callers, established mechanically; re-enters the register if wired |
| `CBR-028` | *not repaired* | OPEN, UNREPAIRED — write-unbounded / read-bounded on a consensus wire | `CLOSED_BY_CBR-044` | the write-unbounded/read-bounded prost asymmetry is mooted by CBR-044's generated decode PDAs (depth-4,096 round trip, no recursion budget); closure recorded in the PathMap report §8 |
| `CBR-029` | `d8e95fb0` | The pretty printer renders a receive's `where` guard | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-031` | `0b270eca` | A `matches` pattern's `=x` reaches the enclosing `locally_free` | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-032` | `084c93b5` | The binder shift emitted the shifted position as the value | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-033` | `8fc9afc9` | A resting send carries its reason — a diagnostic proven off the byte path | `OPTIMIZATION_MEASURED_NEUTRAL` | entry body at the pre-refactor revision (git history) |
| `CBR-034` | `7c0cfd0a` | `TreeHashMap` `update`-after-`delete` resurrected the key — the updater tested the leaf, not the key | `BUG_FIX_RULED_NONCONSENSUS` | the "re-prices an existing operation" claim dissolved under the token model — the added contains is a diagnostic Primitive; the genesis-term movement is common to genesis-contract bug fixes |
| `CBR-035` | `87ee699c` | A walk elimination in the generated `Clone` — behaviourally byte-identical | `OPTIMIZATION_MEASURED_NEUTRAL` | entry body at the pre-refactor revision (git history) |
| `CBR-036` | `88ec2734`, `9442f76b` | The DESCEND BUDGET — one `descend` walks `k+1` cut-set levels; 5.91× fewer trampoline re-entries | `OPTIMIZATION_MEASURED_NEUTRAL` | entry body at the pre-refactor revision (git history) |
| `CBR-038` | `d7818967`, `b75aa6a0` | The escape arm's prost encode was a remotely triggerable abort — and a second, unnamed recursion beside it | `EQUIVALENCE_PROVEN` | bytes preserved by construction (prost writers); member of the conversion programme |
| `CBR-039` | `e93f0222`, `0075ded5` | ∅ gets one spelling at the constructor; the `union` half was reverted, witness landed | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-040` | `6192b4b9`, `fa234cdd` | Sibling order was not a total function of the term; it is now — `ScoredTerm::sort_vec` tie-breaks on the bytes the element emits | `BUG_FIX_RULED_NONCONSENSUS` | sibling-order totality repair (SS-Y4); stack-safety report §5.6.9 |
| `CBR-045` | `ff244c69` | Genesis deploy-log order becomes a canonical function of event protobuf bytes while replay remains a function of the event multiset | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-046` | `0e487d4a` | A dense EPathMap set no longer panics the reverse zipper walk used by the pretty printer | `BUG_FIX_RULED_NONCONSENSUS` | reverse-zipper totality repair; PathMap report §5.7 |
| `CBR-047` | `3dda2186` | A substituted `locally_free` prefix has one canonical empty-set spelling | `BUG_FIX_RULED_NONCONSENSUS` | `models/tests/bit_vector_canonicity.rs` measures the 3-byte protobuf and event-hash movement; `rholang/tests/reduce_spec.rs::eval_of_to_byte_array_method_on_any_process_should_substitute_before_serialization` pins the corrected 18-byte value and replay-visible event |
| `CBR-048` | `dd11241d` | Replay COMM choice stops inheriting `Counter` / `HashMap` iteration order | `BUG_FIX_RULED_NONCONSENSUS` | `rspace++/tests/replay_comm_order.rs` checks all 720 insertion orders under adversarial hash collisions, total-order laws, both ingress sites, and multiplicity preservation; existing replay 24/24 and guarded play/replay 15/15 suites pass |
| `CBR-049` | `5d511a10` | Default test genesis stops inheriting process-random validator and funded-vault fixture keys | `BUG_FIX_RULED_NONCONSENSUS` | the shared deterministic keyspace reaches both Genesis cohorts; post-state golden `28ca4bcf…925ca` passes in 4/4 independent processes at 14.58–14.82 s each under the capped harness |
| `CBR-L01` | `3ff1c98b`, `f586e138` | Equal operator precedence becomes representable; Rholang's ladder corrected | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-L02` | `0f3d298c` | The substrate lane stops answering "false" for a guard it could not decide | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-L03` | `69c66cd1` | A residual binder rests the COMM, whatever the formula collapsed to | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-L04` | `ac7f71af`, `6e6639ee` | `!?` query bind executes; its lowering stops being hash-ordered | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-L05` | `03ec33de`, `826bb96e`, `11472763` | Published lookahead bytes stop carrying host-local order | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-L06` | `2d0ec9b1`, `df57a828` | Published diagnostics stop being derived `Debug` dumps | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-L09` | `b77e657c`, `ab885336` ⚠, `19510082` | ★ DIVERGENCE WITHDRAWN AND WIDENED — every float arithmetic arm ($`+`$, $`-`$, $`\times`$, $`\div`$, unary $`-`$) answers IEEE 754, and comparison follows §5.11 | `BUG_FIX_RULED_NONCONSENSUS` | IEEE 754 convergence (all five float arithmetic arms + §5.11 comparison split) |
| `CBR-L12` | `f5b2e820` | A pathmap's `EMap` pair order stops being a function of the process's hash seed | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |
| `CBR-L13` | `ef49d8c2` | `Bytes` lowers to `GByteArray` (field 25), not `GString` (field 3) | `BUG_FIX_RULED_NONCONSENSUS` | entry body at the pre-refactor revision (git history) |

### B.2 The original commit-level exemptions

Every commit in `7293d57c..dc383ed1` touching the consensus-critical path set, that is **not** a
register entry. The partition is **exact**: **57 entry SHAs + 21 exemptions = 78**. (`b219e199` and
`dc383ed1` joined the entry side when **CBR-007** landed during authoring; neither is exempt.) Each row
carries a typed reason from the closed enum of §3.2 and the evidence discharging it.

★ These are the **negative results**. They are what makes the inclusion criterion checkable: a sweep that
listed only its positives could not be distinguished from one that included everything it read.

| # | SHA | Subject | Reason | Evidence discharging it |
|---|---|---|---|---|
| 1 | `1112dcc8` | the retired unconditional-terminate rule is now UNSPELLABLE — two dead composers DELETED | `HYGIENE` | The deleted rule's output was always in `encode_trie_path`'s image, so `entry_key_is_in_codec_image` was **vacuous on exactly this output**: *"no assertion placed on this expression could have failed."* No caller. **CITED**. |
| 2 | `cf35ab53` | six `PartialEq` impls stop being exhaustive by fiat | `BYTE_NEUTRAL_MEASURED` | *"The 36 same-variant arms are byte-identical to before; behaviour is unchanged for every variant that exists."* The change turns a future 37th variant into `E0004`. Reflexivity and `Eq`/`Hash` agreement asserted for all 54 variants, with 1,352 ordered cross-variant pairs required unequal as the anti-vacuity control. **CITED**. |
| 3 | `f0eb7e5f` | two unused imports that make CI's `-D warnings` red | `HYGIENE` | Imports only. |
| 4 | `20c9aa88` | rewrite every panic-expecting test so none expects a panic | `TESTS_ONLY` | Two production entry points gain fallible forms; *"no consensus-visible behaviour change: every existing caller still gets the identical panic with a byte-identical message."* ⚠ The one genuinely consensus-visible member was **deliberately deferred** and became **CBR-021**. **CITED**. |
| 5 | `0e0f9719` | #127 and #136 are ONE function — the panic is an assertion, not a refusal | `BYTE_NEUTRAL_MEASURED` | *"The consensus version is NOT bumped and the match relation is unchanged: every `.expect` fires exactly where its `.unwrap` fired, on inputs no well-formed term can produce."* The consensus reachability probe is a **measurement, not a false zero**: five deploy-writable receive shapes replay clean, and splicing a depth-34 payload through the identical harness turns the replay RED. **CITED**. |
| 6 | `ec247023` | "it must not touch the global store" was reading its NEIGHBOURS | `TESTS_ONLY` | A test isolation defect that was **live-RED in f1r3node CI** because CI runs `cargo test` (threads in one process) while the campaign's headline counts came from nextest (process per test). Six siblings audited. **CITED**. |
| 7 | `a365a5d1` | three recorded pretty-printer counterexamples become named tests | `TESTS_ONLY` | Promotes proptest seeds to `#[test]`s. Establishes that `New::bind_count` is protobuf `int32`, so *"the printer's domain is `i32`, not `0..`. Totality over the wire type is a consensus obligation."* — an argument, not a change. **CITED**. |
| 8 | `ab1908e0` | drop the `serde::Serialize` imports the channel-bound change orphaned | `HYGIENE` | Imports only; follows **CBR-019**. |
| 9 | `254f489d` | the trie entry invariant becomes executable | `TESTS_ONLY` | Turns cardinality-only assertions into content assertions. Went **RED 7-of-8** and caught the defect that became **CBR-009**. **CITED**. |
| 10 | `21017a17` | R3 second table — five `NewBindRange` rows become CALLS | `TESTS_ONLY` | Mutation-table execution. Measured slope: interval **32 B flat**; materialised **4 B/name**. Feeds **CBR-017**'s evidence. **CITED**. |
| 11 | `a8ec319f` | R3 — the printer's 14 hand-run mutations become CALLS | `TESTS_ONLY` | One row had gone stale. The `ParK` "equivalent mutant" becomes a **theorem**, executed over all $`8! = 40{,}320`$ permutations of six length vectors (241,920 comparisons). **CITED**. |
| 12 | `e2b6c688` | R10 — the `.incoming` staging rule's two recorded claims become executed | `TESTS_ONLY` | One of the two claims was **FALSE**. `test_scratch.rs` restored byte-identically, sha256 checked. **CITED**. |
| 13 | `29263381` | the evaluator twin is not a "faithful copy" — measured, and now pinned | `TESTS_ONLY` | A provenance claim corrected; no production code changes. **CITED**. |
| 14 | `40bb088b` | the printer oracle's "verbatim" becomes CHECKED — and it was wrong | `TESTS_ONLY` | 4 of 10 functions byte-identical under the declared rename, 6 carrying deviations. ★ The **reason** it matters is recorded and is a real methodological finding: *"if the oracle drifts toward the machine … every green result becomes worthless without anyone noticing."* **CITED**. |
| 15 | `516bd2ee` | the RAII sweep finishes; `cargo clippy --workspace` goes green | `HYGIENE` | 37 `mutable_key_type` findings resolved at the **root cause** (`EPathMap`'s `OnceLock` is derived state read by neither `PartialEq` nor `Hash`) via one `clippy.toml` entry rather than 37 site `#[allow]`s. `ConnArm::Ground`'s two spellings shown **byte-identical** before collapsing. **CITED**. |
| 16 | `892b74e8` | a scratch directory that cleans up without a `Drop` that never runs | `INFRA` | Rust runs no destructors for statics. Measured: 20 tests $`\rightarrow`$ 20 directories $`\rightarrow`$ 72 MB of tmpfs; after, zero bytes. Test infrastructure only. **CITED**. |
| 17 | `779bf881` | the oracle's citations become checked, and two of them were wrong | `TESTS_ONLY` | "VERBATIM" overclaimed on 22 of 23 blocks; one block carried an undocumented hand edit. *"the block's meaning is unchanged and the differential's results stand."* **CITED**. |
| 18 | `b9aaa3d4` | the full bound-site enumeration, and a correction | `DOCS_ONLY` | Documentation in `cold_store_decode.rs`. |
| 19 | `96ca51a0` | leg-2 execution record — three falsification experiments | `DOCS_ONLY` | Audit record plus a test-corpus edit. Records that F2 (*"an owned `Env` per work item is acceptable"*) was **REFUTED** by measurement. **CITED**. |
| 20 | `550b967a` | leg-2 stage A — harness prerequisites and the canonical child-slot table | `TESTS_ONLY` | Adds `substitute_descends_into` as *"a checkable record"*; the record's content is reproduced verbatim from production, and *"Reproducing that verbatim is a requirement, not an oversight: descending would change substituted bytes, hence signed bytes."* **CITED**. |
| 21 | `caadf839` | stage 1 — close the coverage gap that let the bare-element key defect survive | `TESTS_ONLY` | Test module only. Produced the witnesses that made **CBR-010** reviewable, including *"asking for the bare `1` returns the singleton list `[1]` — a wrong ANSWER, not a miss."* **CITED**. |

⚠ **This table covers Surface N only.** Surface L is not yet an exact partition; see §6.5.

---

### B.2a The exemptions the derived path set added

[§7.7.2](#7-maintenance) replaced §3.1's
hand-listed path set with a derived one, and the derived set is a **strict superset**: 81 commits in
`7293d57c..dc383ed1` against the hand list's 78. Appendix B's claim `57 + 21 = 78` is a statement about the
*hand-listed* set and remains exactly true of it; this table closes the partition over the **derived** set,
so the two can be read side by side rather than one silently replacing the other.

★ These three are what a *computed* path set sees and a hand-listed one did not. None moves an axis — which
is the point: the derivation's surplus is real, and it is small and classifiable.

| # | SHA | Subject | Reason | Evidence discharging it |
|---|---|---|---|---|
| 22 | `8fb813a7` | every `models` test target is declared, none compiled twice | `INFRA` | `models/Cargo.toml` plus one new test file. No `models/src`, `models/build.rs` or `models/build/` path is touched, so no codec, sorter or wire table can move. **DERIVED** (file list). |
| 23 | `09b80afe` | *"safe to regenerate, not worth tracking"* was FALSE — five counterexamples were being discarded | `TESTS_ONLY` | `.gitignore` plus two corpus files. A shrunk counterexample is **not regenerable**, which is why it is tracked, and it is read only by the proptest harness. **DERIVED** (file list). |
| 24 | `decda6dd` | the massif heap profile — 923× less allocation churn | `INFRA` | A `[[bench]]` declaration and a massif harness. Benches are outside the derived path set; the commit reaches the obligation set **only** through the `models/Cargo.toml` declaration hunk. **DERIVED** (file list). |

---


---

*The register's identifiers are stable and never reused. The 2026-08-03 re-scope retired 45 entries
to B.1 and removed the mechanised drift gate with its machine index; the pre-refactor revision, with
every retired body and the gate's specification, remains in git history.*
