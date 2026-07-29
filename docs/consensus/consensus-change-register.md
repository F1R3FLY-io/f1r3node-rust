# The Consensus-Change Register

### A classification and risk report on the consensus-visible changes of the `rho-native-semantics-review` campaign

| | |
|---|---|
| **Document class** | Engineering report — classification and risk analysis. **LIVING**: amended, never closed. |
| **Register anchor** | `7293d57c` (`f1r3node-rust-mettail`, branch `feature/mettail`) — the last point at which the consensus surfaces were reviewed as a set. |
| **Range analysed** | `7293d57c..dc383ed1` — 111 commits, of which **78** touch a consensus-critical path — plus three changes in flight at the time of writing. |
| **Companion surface** | `mettail-rust`, branch `feature/rho-native-set-automata`, campaign window 2026-07-25 .. 2026-07-29. |
| **Audience** | F1r3node consensus reviewers deciding whether to accept the fork risk of a coordinated protocol-version bump. |
| **Date** | 2026-07-29 |
| **Maintenance** | [§7](#7-maintenance--how-an-omission-fails-loudly). Adding an entry is filling the form in [Appendix A](#appendix-a--the-entry-template). |

---

## Abstract

The `rho-native-semantics-review` campaign landed 109 commits on the F1r3node consensus implementation
and a parallel body of work on MeTTaIL's second implementation of Rholang. This report derives, from the
git record rather than from a summary, the subset of that work which is **consensus-visible**, and
classifies each member against six independent axes: computed value, verdict, serialized bytes,
post-state hash, accepted programs, and metering.

**Result: 40 consensus-visible changes** — 29 on the F1r3node node itself, 11 on MeTTaIL's Rholang.
Of these, **36 are landed, 3 are in flight**, and 1 is an open, unrepaired hazard recorded so it is not
lost. **Nineteen** move bytes on the bincode lane and **eighteen** on the protobuf lane; **twenty** move
a *verdict*; **twenty-eight** move the *post-state hash*; **twelve** move *acceptance*; **two** move
*metering*. One (**CBR-L09**) is a deliberate, owner-ruled divergence from the reference implementation
and is marked as such. A further **21 commits touching consensus-critical paths were examined and
rejected** as not consensus-visible; each is listed with its typed reason in
[Appendix B](#appendix-b--the-exemption-table), so that a reviewer can judge whether the sweep applied a
discriminating criterion or an inclusive one. On Surface N the partition is **exact**: 57 entry SHAs
plus 21 exemptions equals the 78 commits in range.

**The headline risk is not any single entry; it is their conjunction.** `Validate::version`
(`casper/src/rust/validate.rs:273`) compares block versions for **exact equality** against a
genesis-anchored constant. There is no activation-height machinery and no per-feature gate, so these 40
changes cannot be rolled out independently: they ship together, as one coordinated protocol-version
bump, or not at all. No entry in this report bumps a version; that act belongs to F1r3node.

**The report's central evidentiary distinction is *witnessed* versus *mechanism-only*.** `CBR-005`
shipped on a proven mechanism with a production refusal counter reading exactly zero and **no failing
test**; `CBR-007` has a real Rholang program taking the wrong `match` branch. Those are different
propositions and a reviewer must weigh them differently, so the grade is a column of the summary table,
not a remark in the prose. **32 entries are WITNESSED, 3 are MECHANISM-ONLY**, 2 are LATENT, 2 are
DORMANT, and 1 rests on a measured neutrality claim.

**What cannot be settled from inside the repository** is, for eleven entries, whether live chain state
was produced under the old behaviour. Each such entry states the query over the block store that would
settle it, and §5.4 consolidates them — nine share a single artefact. Two further disclosures belong in
an abstract rather than a footnote: **a shipped commit contains a claim that is currently false**
(§6.2), and **the Surface-L sweep is a targeted selection rather than an exact partition** (§6.5). Both
are reported as threats to validity ([§6](#6-threats-to-validity)), not concealed.

---

## 1. Introduction

### 1.1 The problem

A blockchain node is a deterministic function from a block and a pre-state to a post-state. Consensus is
the agreement of independent implementations, or independent builds of one implementation, on that
function. Any change to the node that moves the function's *observable* output for some input is
therefore capable of splitting the network — and the splits are not all of one kind. A change that makes
a validator refuse a deploy every other validator refuses identically produces a detectable, slashable
fault; a change that makes it compute a different value from the same inputs produces a **silent safety
fork**, in which two honest nodes hold divergent, individually plausible histories.

This campaign was a semantics review, and it found a great deal. Repairs to the spatial matcher, the
guard evaluator, the tuplespace's candidate selection, the path-map codec, the pretty printer, the
cold-store decoder and the event-hash encoder all landed within four days. Most of them are corrections
— they make a wrong answer right — but *correcting a wrong answer is consensus-breaking in exactly the
same technical sense as introducing a wrong one*. A reviewer needs both facts at once: that the change
is right, and that it moves the function.

### 1.2 Why a register, and why a living one

A one-off review document is a photograph. It is accurate on the day it is written and drifts from that
moment on, because the thing it describes — a derived set, "all consensus-visible changes since the last
review" — keeps growing while the document does not.

★ **This campaign encountered that exact failure class six times in a single session**, each time in a
different disguise: a hand-maintained language list that a concurrent agent "completed" for the fourth
time rather than deriving; a census kept in prose; a narrow scan sitting 505 lines from the wide one in
the same file; a gate that used a line count as a proxy for a property; a set of rule-index pins one of
which **silently retargeted onto a different rule and stayed green**; and a naming convention asserted
in documentation and enforced nowhere. In every case the repair was the same shape: **derive the set
rather than list it, or make the wrong form unspellable.**

A register that only discipline keeps honest will therefore drift, and this document does not ask a
reviewer to trust that it will not. [§7](#7-maintenance--how-an-omission-fails-loudly) specifies a gate
that makes an omission **fail loudly** — a SHA-keyed, typed exception table with a non-vacuity floor —
and argues it against the alternatives.

### 1.3 Contributions

1. A **six-axis classification** of consensus visibility (§2.4), separating properties that a single
   phrase like "consensus-breaking" conflates, with the propagation between axes made explicit.
2. An explicit account of the **two wire formats** a `Par` crosses, and of the field-order asymmetry
   between them that has already produced one measured, round-trip-invisible defect (§2.5).
3. A **derived** change set (§3, §4): 40 entries, each with all six axes answered, a stated blast
   radius, a direction, an evidence grade, and — where one exists — the owner ruling that authorised it,
   quoted verbatim with its date.
4. The **negative result**: 21 examined-and-rejected commits with typed reasons (Appendix B), which is
   what makes the inclusion criterion checkable rather than merely asserted.
5. A **drift-detection design** (§7) siting the gate, its data format, its failure modes, and its
   anti-vacuity argument.

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
- **Phlogiston** — Rholang's unit of computational cost. **Metering** is the act of charging it.
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
> at all, or the phlogiston charged.

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
| 4 | **Post-state hash** | What root is committed? | Replay's $`h_{\mathrm{post}}`$ differs from the block's ⇒ **safety fork**. |
| 5 | **Accepted programs** | Is this deploy admitted at all? | A *decidable* refusal ⇒ a failed deploy; a *process abort* ⇒ a liveness split. |
| 6 | **Metering** | How much phlogiston, in what order? | Out-of-phlogiston on one node and not another — which turns into an Axis-2 divergence. |

⚠ **Axis 6 is about charges, not budgets.** Budgets belong to F1r3node (`wallet.txt`). A metering cell
in this register records that a *charge* moved; it never records a budget decision, and no entry
introduces a metering surface.

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
- **A verdict can move with no value moving, and a value with no verdict.** `f5fd6c34` (**CBR-005**)
  changes *bound values* and demonstrably not verdicts; **CBR-007** changes *verdicts*, and the values it
  then binds are the ones that were always correct.

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
| `locally_free` | blanked to eight zero bytes | real `bytes` — 12 fields, asserted `ProstKind::Bytes` |
| Depth policy | symmetric: both directions unbounded | **asymmetric**: the decoder caps recursion at 100 levels (`DecodeContext`); the encoder caps nothing |

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

The asymmetry in the last table row is itself an open register entry — **CBR-028**.

### 2.6 Direction of change

| Direction | Meaning |
|---|---|
| **REGRESSIVE** | A previously-succeeding thing now fails. ★ Reviewers weigh this most heavily. |
| **PERMISSIVE** | A previously-failing thing now succeeds. |
| **CORRECTIVE** | A previously-*wrong* answer is now right: the program succeeded before and succeeds now, but the answer moved. |
| **NEUTRAL** | No observable behaviour moves. The entry exists because the change sits on a consensus path and its neutrality is a **measured claim**, not an assumption. |
| **DIVERGENT** | A deliberate, ruled departure from the reference implementation. |

### 2.7 Evidence grade — and the word "potentially"

The user's requirement is that a divergence *reachable in principle but unwitnessed* be labelled as such:
neither upgraded to "does break" nor dismissed as "does not". That is what this column carries, and it
is the report's central evidentiary distinction.

| Grade | Meaning | Example |
|---|---|---|
| **WITNESSED** | A concrete program, term, or byte string is known that exhibits the divergence. | **CBR-007** — a real Rholang program takes the wrong `match` branch. |
| **MECHANISM-ONLY** | The mechanism is proven and the path is reachable by an ordinary deploy, but no witnessing program has been exhibited. | **CBR-005** — cumulative `FreeMap` snapshots proven; the production refusal counter reads exactly zero. |
| **LATENT** | The mechanism exists, but reaching it needs a capability an ordinary deploy lacks, or a structural argument shows the path is not walked. | **CBR-021** — no deploy can build a malformed consume; only the FFI can. |
| **DORMANT** | The changed code has no caller at all, established **mechanically**, not by intention. | **CBR-019b** — the prost encoder. |
| **NEUTRALITY-MEASURED** | The claim is that *nothing* moves, and that claim is itself the measurement. | **CBR-019** — byte identity against the derived `Serialize` over an exhaustive corpus. |
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
models/build/, models/build.rs   the GENERATOR that emits the wire tables
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

**Step 7 — capture what has not landed.** Four changes were in flight when the sweep began: the
`attempt_opt` combinator (**CBR-007**), the checked-arithmetic repair (**CBR-027**), the kv
element-category gate (**CBR-L08**), and the partial-operation disposition repair. ★ **CBR-007** landed
during authoring (`b219e199`, `dc383ed1`) and its entry is now written from *measured* rather than
*intended* behaviour; the other three were verified against the **working tree**, and each entry says
so. One further item (**CBR-028**) is an open hazard
that is deliberately *not* repaired and is recorded so that it is not lost.

### 3.2 Inclusion and exclusion criteria

**Included** if and only if there exists a deploy and a reachable pre-state on which old and new
observably differ on some axis — the definition of §2.4. This admits:

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
| 6 · metering | **DERIVED** from whether a `reserve_*` / `Cost::` site was added, removed, or re-priced. |

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
| [CBR-001](#cbr-001) | N | A `where` guard participates in candidate **selection**, not only approval | `6bc58743` | ● | ● | ○ | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-002](#cbr-002) | N | An undecidable `where` guard is **refused**, not silently false | `6ab1c78b`, `eaa44c2f` | · | ○ | ○ | ○ | ○ | ● | · | REGRESSIVE | **W** |
| [CBR-003](#cbr-003) | N | `matches` guards become decidable (injected spatial-match oracle) | `99b7b1c4` | ● | ● | ○ | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-004](#cbr-004) | N | Thirteen `ExprInstance` arms were missing from the spatial matcher | `b0f18672` | ● | ● | ○ | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-005](#cbr-005) | N | A matcher attempt owns its own `FreeMap`; the multiplicity guard un-vacuumed | `f5fd6c34`, `eaa905fe` | ● | ○ | ○ | ○ | ● | ○ | ○ | CORRECTIVE | **M** |
| [CBR-006](#cbr-006) | N | A `matches` pattern is substituted at `depth + 1` | `8853f839` | ● | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **M** |
| [CBR-007](#cbr-007) | N | A connective attempt owns its own `FreeMap` — the isolation law at four more sites | `b219e199`, `dc383ed1` | ● | ● | ○ | ○ | ● | ○ | ○ | PERMISSIVE | **W** |
| [CBR-008](#cbr-008) | N | A relative path below the root built a key no `Par` can have | `a1feb437` | ● | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-009](#cbr-009) | N | `setSubtrie` stopped dropping bare source entries | `ce7bb4fd` | ● | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-010](#cbr-010) | N | Path readers ask the codec instead of guessing | `0a6d2ce0`, `5aacebc3` | ● | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-011](#cbr-011) | N | A pathmap's identity stops depending on insertion order | `478102a4` | ○ | ● | ○ | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-012](#cbr-012) | N | The shadow `Vec` deleted — non-ground pathmap order follows the trie | `9994a75b` | ○ | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-013](#cbr-013) | N | The two bulk trie readers collapse to the key walk | `c705776c` | ● | ● | ○ | ○ | ○ | ○ | ○ | CORRECTIVE | **M** |
| [CBR-014](#cbr-014) | N | `EZipper.cursor_kind` — a **new proto field**; four bytes on Lane B | `7dcff96f` | ● | ● | ● | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-015](#cbr-015) | N | `cursor_kind` reaches the printer, whose output is replay-compared | `76de7d44` | · | ○ | ● | ○ | ● | ○ | ○ | CORRECTIVE | **L** |
| [CBR-016](#cbr-016) | N | An environment variable leaves the consensus byte path | `ca84d535`, `4290303c` | · | ○ | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-017](#cbr-017) | N | The `New` bind bound becomes an interval; the clamp had moved bytes | `a4c23a58` | · | ○ | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-018](#cbr-018) | N | The pretty printer renders its `match` target | `bd7cb45f` | · | ○ | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-019](#cbr-019) | N | The channel leg of every event hash routed through a new encoder | `00ff9187`, `c28f4cf6`, `903cefb3` | ○ | ○ | ○ | ○ | ○ | ○ | ○ | NEUTRAL | **NM** |
| [CBR-019b](#cbr-019b) | N | A trampolined **prost** encoder — built, gated, **dormant** | `7c74260d`, `56fb1fd0` | · | · | · | ○ | · | ○ | · | NEUTRAL | **D** |
| [CBR-020](#cbr-020) | N | The cold-store read path becomes fallible and heap-bounded | `9a5521a2`, `2bcfaf87`, `000b95d7` | ○ | ○ | ○ | ○ | ○ | ● | ○ | PERMISSIVE | **W** |
| [CBR-021](#cbr-021) | N | A malformed consume refuses instead of killing the process | `b961d7c4` | · | ○ | ○ | ○ | ○ | ● | ○ | PERMISSIVE | **L** |
| [CBR-022](#cbr-022) | N | Deploy admission owns its discard — a 43,565-byte deploy stops aborting the node | `a09f1de2`, `3b265eb7` | ○ | ○ | ○ | ○ | ○ | ● | ○ | PERMISSIVE | **W** |
| [CBR-023](#cbr-023) | N | The $`\Theta(\mathrm{depth})`$ conversion programme — sixteen commits | see body | ○ | ○ | ○ | ○ | ○ | ● | ○ | PERMISSIVE | **W** |
| [CBR-024](#cbr-024) | N | `last` joins the method table | `2fee67fa` | · | · | · | · | · | ● | ● | PERMISSIVE | **W** |
| [CBR-025](#cbr-025) | N | Trie enumeration: `getPath` / `toNextLeaf` / `leafCount` | `98d2422d` | · | · | · | · | · | ● | ● | PERMISSIVE | **W** |
| [CBR-026](#cbr-026) | N | `E(S)` — the enabled-rendezvous query and firing a **named** selection | `2087c043` | · | · | · | · | · | ● | · | PERMISSIVE | **D** |
| [CBR-027](#cbr-027) | N | GInt `+` and `-` stop wrapping on overflow | *in flight* | ● | ● | ● | ● | ● | ○ | ○ | REGRESSIVE | **W** |
| [CBR-028](#cbr-028) | N | **OPEN, UNREPAIRED** — write-unbounded / read-bounded on a consensus wire | *not repaired* | · | · | · | ● | ● | ● | · | — | **W** |
| [CBR-L01](#cbr-l01) | L | Equal operator precedence becomes representable; Rholang's ladder corrected | `3ff1c98b`, `f586e138` | ● | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-L02](#cbr-l02) | L | The substrate lane stops answering "false" for a guard it could not decide | `0f3d298c` | · | ● | ○ | ○ | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-L03](#cbr-l03) | L | A residual binder rests the COMM, whatever the formula collapsed to | `69c66cd1` | · | ● | ○ | ○ | ● | ○ | ○ | REGRESSIVE | **W** |
| [CBR-L04](#cbr-l04) | L | `!?` query bind executes; its lowering stops being hash-ordered | `ac7f71af`, `6e6639ee` | ● | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-L05](#cbr-l05) | L | Published lookahead bytes stop carrying host-local order | `03ec33de`, `826bb96e`, `11472763` | ○ | ○ | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-L06](#cbr-l06) | L | Published diagnostics stop being derived `Debug` dumps | `2d0ec9b1`, `df57a828` | ○ | ○ | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-L07](#cbr-l07) | L | `List.last()` in MeTTaIL's Rholang | `bbceb6d9`, `6e543c01` | · | · | · | · | · | ● | ? | PERMISSIVE | **W** |
| [CBR-L08](#cbr-l08) | L | The kv element-category gate — a Name in a kv slot is refused, not silently dropped | *in flight* | ● | ● | ● | ● | ● | ● | ○ | REGRESSIVE | **W** |
| [CBR-L09](#cbr-l09) | L | **DELIBERATE DIVERGENT** — float $`\div 0`$ answers `error`, not $`\pm\infty`$ | *pre-existing, ruled kept* | ● | ● | ● | ● | ● | ○ | ○ | DIVERGENT | **W** |
| [CBR-L10](#cbr-l10) | L | A pathmap's entries come from a projection, not a field | `832d510f` | ○ | ● | ● | ● | ● | ○ | ○ | CORRECTIVE | **W** |
| [CBR-L11](#cbr-l11) | L | The literal-domain and canonical-surface repairs | six commits, see body | ● | ○ | ● | ● | ● | ● | ○ | CORRECTIVE | **W** |

**Totals — 40 entries**: 29 on Surface N, 11 on Surface L; **36 landed, 3 in flight**, 1 open and
unrepaired. By evidence grade: **32 WITNESSED**, 3 MECHANISM-ONLY, 2 LATENT, 2 DORMANT,
1 NEUTRALITY-MEASURED. Exactly **one** axis cell is `UNVERIFIED` (**CBR-L07**, metering). Aggregated in
[§5](#5-risk-analysis).

### 4.2 Entry template

Every entry below follows the fixed form specified in [Appendix A](#appendix-a--the-entry-template):
a header table, then **(a) the issue**, **(b) how it (potentially) breaks consensus** — the full axis
table, the concrete disagreement, the blast radius, and the chain-history question — then **(c) why the
change was necessary or correct**, and finally the evidence. Axis cells use the closed vocabulary
`MOVES` / `NO` / `N/A` / `UNVERIFIED`.

---

## 4.3 Surface N — the F1r3node consensus implementation
---

### CBR-001

**A `where` guard participates in candidate SELECTION, not only approval.**

| | |
|---|---|
| Commit(s) | `6bc58743` (fix), `5d37f67e` (end-to-end witness) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `rspace++/src/rspace/space_matcher.rs`, `rspace++/src/rspace/candidate_order.rs` (new), `rspace++/src/rspace/rspace.rs`, `rspace++/src/rspace/replay_rspace.rs` |

#### (a) The issue

The tuplespace's candidate search found the **first spatially matching** datum for each bind, then
consulted the `where` guard once, at the end, as an *approval*. If the guard rejected, the whole COMM
was abandoned — even when a *different* admissible assignment of the same resting data would have
satisfied it. The guard was a filter applied after the choice instead of a constraint on the choice.

Three separable behaviour changes land together, each named in the commit with its own blast radius:
guarded receives fire where they used to stall; **guard-free multi-bind** receives also fire where they
used to stall; and a receive taking two data from one channel no longer duplicates one. **CITED**.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — a COMM that now fires delivers data that previously stayed resting. |
| 2 · verdict | **MOVES** — this is the entry's whole content: *when a COMM fires*. |
| 3 · bytes (Lane B, bincode) | NO — no encoding changed; the *sequence* of encoded events changed. |
| 3 · bytes (Lane P, prost) | NO — same. |
| 4 · post-state hash | **MOVES** — a fired COMM leaves a different tuplespace. |
| 5 · accepted programs | NO — every program still normalizes and runs. |
| 6 · metering | NO — no charge site changed. |

**The disagreement.** A pre-upgrade node and a post-upgrade node given the identical resting state and
the identical receive `for (x <- @"c" & y <- @"d") where G(x,y)` reach different tuplespaces: the old
node leaves the data resting, the new node consumes it and runs the continuation. On replay this is an
immediate post-state-root mismatch — a **safety fork**, not a slashable fault, because the two nodes
produce *different valid-looking* histories rather than one detectably invalid block.

**Blast radius.** Every deploy containing a guarded receive, and every deploy containing a multi-bind
receive whose first spatial match on one channel excludes a match on another. Reachable by an ordinary
deploy: **yes** — this is plain Rholang surface syntax.

**Could live chain state have been produced under the old behaviour?** ⚠ **Not settleable from inside
the repository.** The query that would settle it: scan the chain's `ProcessedDeploy` deploy logs for any
block whose deploy source contains a `where` clause on a `for`/`contract`, or a multi-bind `for` with
`&`, and check whether any such receive is still resting in a historical post-state. Concretely — run
over the block store: `for each block b, for each ProcessedDeploy p in b: parse p.deploy.data.term;
report if the AST contains ReceiveBind count > 1 or a non-empty Receive.condition`. **UNVERIFIED** here.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A guard is a *constraint*, and a search that consults it only at
the leaf is not searching the constrained space — it is searching the unconstrained space and then
rejecting. The observable consequence was measured on the settlement demo: **12 settlements and 8
stalls**, where the outcome was decided by "which datum the canonical order happened to present first",
and that canonical order is a hash over payload bytes that include a per-produce `random_state`
**nobody wrote**. **MEASURED** (`5d37f67e`). That is not a semantics; it is a coin flip whose bias
depends on unforgeable-name entropy. Leaving it means the language's `where` clause does not mean what
its own documentation says.

**Why this repair rather than the alternatives.** Two alternatives were available and both were
rejected on stated grounds:

1. *Retry the whole consume on guard rejection.* Rejected: it re-enters the search from scratch, so it
   does not enumerate assignments — it re-derives the same first match.
2. *Filter the pool before the search.* Rejected: a guard is generally **cross-bind**
   (`G(x, y)` mentions two binds), so no per-bind filter can decide it.

**★ The determinism argument is the part a reviewer should check hardest**, because it is where this
change could have been a *silent* fork rather than a repair. Candidate order was previously a **private
method of `RSpace`**; the replay space used **raw store order**. That asymmetry survived only because
replay filters its pools down to the produces the recorded COMM consumed, usually leaving one candidate
per bind. It stops surviving the moment the matcher can select a candidate other than the first spatial
match, because `COMM::new` **sorts its produce refs** — so two selections that *permute* the same data
across binds build the **same COMM event**, slip past the trace assertion, and bind the receive's
variables the other way round. That is a silent post-state divergence with no detectable event.
**CITED** — and it is why the fix necessarily includes hoisting the order into
`rspace::candidate_order` and adopting it in **both** spaces, plus applying the guard on the replay
consume path (which previously did not, and did not need to, while a guard rejection could only mean
"no COMM at all"). With both in place: replay's pool is a subsequence of play's, play's selection
survives the filter, and any admissible selection lexicographically smaller in the filtered pool would
also have been reachable in the unfiltered one. **Replay therefore selects exactly what play selected.**

**Authority.** No owner ruling; the commit flagged itself for consensus review rather than landing
quietly: *"⚠ CONSENSUS-AFFECTING. This changes WHEN A COMM FIRES. Three separable behaviour changes land
here; each is called out below with its blast radius so consensus review can weigh them independently.
Flagged for consensus review rather than landed quietly."* **CITED** (`6bc58743`, 2026-07-26).

#### Evidence

- `rspace++/tests/guarded_matching_tests.rs` 15/15; `replay_rspace_tests` 24/24; `cargo test -p
  rspace_plus_plus` 319 tests, 0 failures. **MEASURED** (`6bc58743`).
- `5d37f67e` reproduces the demo's 12-settlements-and-8-stalls split **deterministically** and shows
  both settle post-fix.
- Play/replay agreement is asserted, not argued: `enabled_rendezvous_spec.rs` t6 (see **CBR-026**) runs
  one enumeration through both spaces.

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

### CBR-003

**`matches` guards become decidable — a caller-injected spatial-match oracle.**

| | |
|---|---|
| Commit(s) | `99b7b1c4` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `rho-pure-eval/src/oracle.rs` (new), `rholang/src/rust/interpreter/matcher/match.rs`, `.../reduce.rs` |

#### (a) The issue

`rho-pure-eval` refused `ExprInstance::EMatchesBody` outright with `UnsupportedExpression`, so **every
spatial `where` guard failed shut whatever its truth value**. It could not call the matcher directly:
the crate dependency runs the other way (`rholang` → `rho-pure-eval`, whose only dependencies are
`models`, `shared` and `num`), so a direct call would be a dependency cycle. The matcher is injected by
the caller instead: `SpatialMatch` is a pure/total/deterministic oracle trait, `NoSpatialMatch` is the
absent oracle, and `eval_with(par, env, &dyn SpatialMatch)` is the new entry point with
`eval(p, e) = eval_with(p, e, &NoSpatialMatch)` preserving every pre-existing caller byte-unchanged.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — the continuation of a now-admitted COMM runs and computes. |
| 2 · verdict | **MOVES** — a `where … matches …` guard that always failed can now succeed. |
| 3 · bytes (Lane B) | NO |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | **MOVES** — a COMM that now fires leaves a different tuplespace. |
| 5 · accepted programs | NO — such programs already normalized and ran. |
| 6 · metering | NO — the two rholang call sites build a **fresh** `SpatialMatcherContext` per question and drop it; guard evaluation remains unmetered, unchanged. |

**The disagreement.** `for (@x <- @"c") where x matches Int` on an old node never fires; on a new node it
fires when the arriving datum is an integer. Post-state roots diverge immediately. Because the two
nodes' behaviour differs on a *successful* guard rather than on an error, there is no `is_failed`
mismatch to catch it: this is a **safety fork**, not a slashable fault.

**Blast radius.** Every deploy whose `for … where` or `match … where` guard contains a `matches`
expression. Reachable by an ordinary deploy: **yes**.

**Could live chain state have been produced under the old behaviour?** ⚠ **Not settleable from inside
the repository.** Settling query: count historical `ProcessedDeploy` terms whose `Receive.condition` or
`MatchCase.condition` contains an `EMatchesBody`. **UNVERIFIED** here.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** `where p matches q` is the guard form the language's own
documentation advertises for structural side-conditions, and it was **inert**. Worse, it was inert in
the *fail-shut* direction, which is the direction a reviewer is least likely to notice: the program does
not crash, it simply never proceeds.

**Why an injected oracle rather than a direct call, a feature flag, or moving the matcher.** The
dependency runs `rholang → rho-pure-eval`; a direct call is a cycle, and moving the matcher into
`rho-pure-eval` would drag `models`' whole spatial-matching surface into a crate whose defining property
is that it is *pure*. The injection keeps `rho-pure-eval` pure and total while making the answer
available. The `NoSpatialMatch` default preserves every pre-existing caller **exactly**, which is what
makes the change reviewable: only the two rholang guard sites move.

**★ The subtlety worth checking.** The `EMatches` arm mirrors `Reduce::combine_matches` **minus its two
`substitute_and_charge` calls**: the target *is* evaluated (env-resolved), and the pattern is passed
through verbatim because its free variables are binders. That is sound because `eval_receive` already
substitutes the whole guard at depth 1 and `substitute`'s own `EMatchesBody` arm descends into **both**
operands at that same depth — so the guard's pattern has already had exactly the depth-1 substitution
`combine_matches` would apply, and `maybe_substitute_var` is the identity at `depth ≠ 0`. **DERIVED**
(commit message; the underlying depth claim is itself corrected by **CBR-006**, which found that the
`EMatches` *pattern* slot needs `depth + 1`, not the enclosing depth — the two entries must be read
together).

**What is deliberately preserved.** *"Guard-failure semantics are untouched: `false`, non-boolean and
eval-error still all collapse to guard-fail (consensus-visible, and deliberately preserved)."*
**CITED**. Diagnosability was added only at a non-consensus layer — a `DEBUG` `tracing` event naming the
two silent cases, changing no return value.

**Authority.** Plan-approved: *"mettail-rust scratchpad/flt_lookahead_plan.md §18.2 D3 (approved)"*.
**CITED**.

#### Evidence

- 14 new tests in `rho-pure-eval` (52/52 green), including an `eval` vs `eval_with(&NoSpatialMatch)`
  equivalence battery over **every arm family** — the executable form of "every pre-existing caller is
  byte-unchanged". **MEASURED**.
- 9 new `rholang` unit tests on `guard_passes`, 5 new end-to-end `reduce_spec` tests exercising the real
  reducer plus RSpace for both call sites. **MEASURED**.

---

### CBR-004

**Thirteen `ExprInstance` arms were missing from the spatial matcher; a pattern containing any of them matched nothing, silently.**

| | |
|---|---|
| Commit(s) | `b0f18672` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `rholang/src/rust/interpreter/matcher/spatial_matcher.rs`, `models/src/rust/rholang/par_children.rs` |

#### (a) The issue

`SpatialMatcher<Expr, Expr>::spatial_match` handled **14 of the 36** `ExprInstance` variants and dropped
every other pair into `_ => None`. In that position a catch-all is not a default — it is a decision
taken silently: the pattern matches nothing, the COMM never fires, the receive rests forever, the node
exits 0, and no diagnostic is produced anywhere. `spatial_match` decides COMM firing (`Matcher::get`,
the `rspace_plus_plus::rspace::r#match::Match` impl the tuple space consults, walks straight into it),
so the decision is consensus-visible. Eleven of the thirteen are now implemented; two are excluded **by
measurement**, not by omission.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — a continuation that never ran now runs. |
| 2 · verdict | **MOVES** — the entry's content. |
| 3 · bytes (Lane B) | NO |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | NO — the programs normalized fine; they just never matched. |
| 6 · metering | NO |

**The disagreement.** `for (@{x - y} <- @"c")` — a receive whose pattern contains an `EMinus` — rests
forever on an old node and fires on a new one. Safety fork, as **CBR-003**.

**Blast radius.** Any pattern (in `for`, `contract`, or `match`) containing `EMinus`, `ELt`, `ELte`,
`EGt`, `EGte`, `EEq`, `ENeq`, `EAnd`, `EOr` and the two unary arms. Reachable by an ordinary deploy:
**yes**.

**⚠ A real gap remains, stated rather than papered over.** `EPathmapBody` is **not** descended into.
`{| a, b, ...rest |}` is ordinary surface syntax and the collection normalizer sets its
`connective_used` from its elements exactly as it does for a set, so
`for (@{| x, ...rest |} <- ch)` **normalizes fine and then matches nothing**. Closing it needs
substitution descent first, and then a decision about a path map's entry-multiset semantics under
matching — *"whether `{| 1, 1 |}` and `{| 1 |}` are the same pattern, and in what canonical order a
bound `...rest` is reassembled. That order is the ground-map canonical form whose bytes are the
event-hash preimage, so it is a consensus decision, not an implementation detail."* **CITED**.
`EZipperBody` is additionally unreachable as a pattern: no normalizer path constructs one.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: scan historical deploy terms for a `ReceiveBind.pattern` or
`MatchCase.pattern` containing any of the eleven newly-implemented arms. **UNVERIFIED** here.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A pattern language in which nine of the standard binary
comparison and arithmetic operators silently fail to match is not a pattern language; it is a trap. And
the trap is silent in the fail-shut direction, so it is invisible to every test that asserts a COMM
*does* fire and to every deployer who has not read the matcher.

**Why implement eleven and exclude two.** The eleven have *identical `p1`/`p2` shape and identical role*
to the seven binary arms already present; adding them is filling a table, not inventing a semantics. The
two exclusions are **derived, not chosen**: a `for` bind is substituted **before** it is stored in
RSpace and matched **after**, over the same bytes. `substitute_descends_into` declines exactly
`EPathmapBody` and `EZipperBody`, so a `VarRef` or a shifted `BoundVar` inside a path map is still in
its *pre-substitution* form when the matcher reaches it — **descending would bind out of stale bytes**.
The invariant is now checked in both crates:
`models::rust::rholang::par_children::spatial_match_descends_into` is exhaustive with **no `_` arm**,
modelled on `substitute_descends_into` beside it. **DERIVED**.

**Why the fix cannot silently go short again.** The iteration set of the test is the **generated**
variant table — `EXPR_INSTANCE_VARIANT_COUNT` is `EXPR_INSTANCE_VARIANTS.len()`, never a literal — so a
37th variant fails the suite until it has a representative, and therefore until somebody has decided
what the matcher does with it. It cannot be satisfied vacuously: the loop that checks second-operand
refusal **names the nineteen arms it exercised and asserts the list**. **DERIVED**.

**Authority.** No owner ruling; the commit is marked `!` (breaking) and states the consensus visibility
in its opening paragraph.

#### Evidence

- Both operands descended into, and a **second-operand mismatch is refused** — asserted rather than
  assumed, per arm. **MEASURED**.
- The two exclusions are cross-checked against `substitute_descends_into` in `models`, so the two crates
  cannot drift. **DERIVED**.

---

### CBR-005

**A matcher attempt owns its own `FreeMap`; the duplicate-variable guard stops being a tautology; `panic!` becomes a refusal.**

| | |
|---|---|
| Commit(s) | `f5fd6c34` (`match_function` isolation + guard + `panic!`→`None`), `eaa905fe` (the disjunction arm) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | MECHANISM-ONLY |
| Files | `rholang/src/rust/interpreter/matcher/list_match.rs`, `.../matcher/spatial_matcher.rs`, `.../metrics_constants.rs` |

#### (a) The issue

Two halves of one defect.

**Half 1 — a vacuous guard.** `aggregate_updates`' duplicate-variable check collected `added_vars` into
a `HashSet` and compared its length against `added_vars.iter().collect::<HashSet<_>>().len()` — identical
cardinality **by construction**. The check could never fire. The reference implementation keeps
multiplicity (a sequence, not a set), so the check is real there.

**Half 2 — no per-attempt isolation.** `list_match` builds `cloned_self` once, moves it into the closure,
and **every attempt in the whole bipartite search mutates that one map**. Each claimed match's snapshot
is therefore **cumulative**: it carries the bindings written by every *failed* attempt that ran before
it. The duplicates the repaired guard can now see are the artifact of that.

**★ The two are one change.** The vacuous guard was **load-bearing**: it is the only reason the `?` on
`aggregate_updates` never short-circuits, and therefore the only reason the cumulative snapshots never
changed a match **verdict**. Landing the guard without the isolation converts a value-only latent
divergence into a **verdict-level regression**. **CITED**.

`eaa905fe` is the same invariant at a distinct site: the `ConnOrBody` arm snapshotted `free_map`,
restored it on success, and **skipped the restore on failure** because the `?` returned `None` from the
closure. A branch that bound a free variable and then refused left that binding behind.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — a continuation's environment could carry a stale binding written by a failed attempt. |
| 2 · verdict | NO — **measured**, not argued: see Evidence. |
| 3 · bytes (Lane B) | NO — no encoding changed. The *value* a continuation receives can change, and that value is later serialized; the codec is untouched. |
| 3 · bytes (Lane P) | NO — same. |
| 4 · post-state hash | **MOVES** — a continuation that receives a different binding writes different data. |
| 5 · accepted programs | NO |
| 6 · metering | NO — *"there is no cost accounting anywhere in `matcher/`, so these cannot move metering"* (of the new observability counters). **CITED**. |

**The disagreement, concretely.** `aggregate_updates` folds the per-attempt snapshots with
`HashMap::extend` (later wins) in the caller's `matches` order. `matches` is a
`BTreeMap<Indexed<T>, _>` (`maximum_bipartite_match.rs:14`) whose `Ord` is derived over `value` first —
**structural `Par` ordering, which is prost's field-declaration derive, not the canonical Rholang sort
and not chronological**. So when a cumulative snapshot carried a stale value for a level, **which of the
two values reached the continuation was decided by a `.proto` field number.** That is not a semantics.
**CITED** — and it is the concrete reason this is a correction and not a tidy-up.

Measured mechanism, printed verbatim by the row-2 diagnostic on
`matching_two_lists_in_parallel_should_work`:

```text
free_maps = [ {1 -> 8}, {0 -> 7, 1 -> 8} ]
```

The second map is cumulative: it carries the first match's binding as well as its own. Level 1 is
claimed twice. **MEASURED**.

**Blast radius.** Any receive or `match` whose pattern is a **list, set or map with multiple binding
positions**, where the bipartite search makes more than one attempt. Reachable by an ordinary deploy:
**yes**. Reachability is **MECHANISM-ONLY**: the mechanism is proven and the path is ordinary,
but no program was exhibited whose *delivered value* differs.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: replay historical blocks under an instrumented build that records, per
`ListMatch` invocation, whether more than one attempt ran and whether the winning snapshot's key set
strictly exceeds the winning attempt's own writes. A non-zero count is a witness. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Two things, and only the second is obvious. (i) A continuation
can receive a binding that no successful match ever produced. (ii) ★ More seriously — the guard was the
*only* thing preventing (i) from becoming a verdict change, and it was **vacuous by construction**. Any
future edit that made `added_vars` a sequence, or that made the fold order chronological, would have
silently promoted a value divergence into a verdict divergence. The system was one refactor away from a
fork and the check that would have caught it could not fire.

**Why `panic!` → `return None` is not optional.** `Matcher::get` receives `BindPattern`s deserialised
from tuplespace history, **including peer-served state**, and pattern free-variable linearity is
enforced at **normalization** — which a deserialised pattern never went through on this node.
Un-vacuuming the guard while keeping the `panic!` would have converted a dead check into a
**remotely-triggerable node crash**. `ListMatch`/`SpatialMatcher` have no error channel and widening
them to `Result` would change every signature in the module, so the refusal goes through the `Option`
the function already returns — the same posture as `b961d7c4`'s malformed consume (**CBR-021**).
⚠ This is a stated **DIVERGENCE** from the reference implementation, which used a `BugFoundError`.
**CITED**.

**Why the snapshot is taken on exactly one arm.** That is a proof, not a heuristic: `guard(t == p)` and
`guard(locally_free(t, 0).is_empty())` cannot reach `free_map`, and `connective_used(p)` is the
predicate the function already branches on. Cost delta on the connective arm: success is net zero (the
exit clone becomes an entry clone), failure is `+1` clone — and under isolation the clone stops
**growing**, because today's exit clone copies the *cumulative* map. Net **7.96% faster**. **MEASURED**.

**What is deliberately not changed.** No consensus version bump, no write-side cap, no pathmap sorting —
`aggregate_updates` keeps folding in the existing `matches` `BTreeMap` order. `rholang/tests/matcher/
match_test.rs` is **byte-untouched**: it is the 49-test acceptance oracle, *"and one must not edit one's
own oracle"*. **CITED**.

**Authority.** Owner-approved on a proven mechanism with no failing test. The subsequent ruling that
governs the follow-on (**CBR-007**) is quoted there.

#### Evidence

- ★ **The row-2 discriminator was built and measured, not argued.** Four builds:

  | build | the 8 named tests | the other 41 | RED 1 | RED 2 |
  |---|---|---|---|---|
  | HEAD | green | green | red | red |
  | guard only, NO isolation | **RED (all 8)** | green | red | green |
  | isolation only, NO guard | green | green | green | red |
  | composite (this commit) | green | green | green | green |

  Row 2 is the discriminator: a "fix" that merely re-hides the duplicates leaves row 2 all-green — the
  guard would still be unable to fire. **MEASURED** (`f5fd6c34`).
- ★ **The refusals counter reads zero on every production path.** Whole `-p rholang` suite, 49 targets,
  1267 passed / 0 failed / 8 ignored: the refusal marker appears exactly **twice**, both inside
  `tests/matcher_state_isolation.rs`, from the two fixtures built by hand to prove the backstop can fire
  at all. *"That is the direct evidence that verdicts did not move — not a proxy — and it simultaneously
  proves the counter is wired rather than merely declared."* **MEASURED**.
- ⚠⚠ **THE ZERO-REFUSALS CLAIM WAS FALSE WHEN IT WAS WRITTEN — now proven, not argued.** Re-measured
  over the whole `-p rholang -p models` suite: **8 firings at the pre-fix HEAD, SIX of them from
  PRODUCTION paths** (three verdict-level fixtures and all three end-to-end reductions), plus the two
  deliberate direct-call fixtures. **Exactly 2 after CBR-007**, in both debug and release. So the
  assertion was wrong **by six**. It is not a defect in what shipped — `match_function`'s isolation was
  working exactly as specified — and **history was not rewritten**: `dc383ed1` records the correction at
  the two addresses that carry the claim (`aggregate_updates`' doc comment and the refusals-counter
  comment). See §6.2 and **CBR-007**. **MEASURED** (`b219e199`, `dc383ed1`).
- `eaa905fe`: two REDs flipping with **two controls green on both sides**, so the flip cannot be
  explained by "the arm was rewritten". The important control is
  `control_the_disjunction_verdict_is_unchanged`. Leaked binding printed verbatim:
  `left: {0: Par { exprs: [GInt(7)] }}` against `right: {}`. **MEASURED**.

---

### CBR-006

**A `matches` pattern is substituted at `depth + 1`.**

| | |
|---|---|
| Commit(s) | `8853f839` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | MECHANISM-ONLY |
| Files | `rholang/src/rust/interpreter/substitute_combine.rs`, `substitute_drive.rs`, `substitute_oracle.rs` |

#### (a) The issue

`p_matches_normalizer::combine_p_matches` normalizes the right-hand side of `P matches Q` under
`input.bound_map_chain.push()`, in a fresh `FreeMap`, keeping the *target*'s free map.
`EMatches::pattern` is therefore a **nested pattern one binding level deeper** — the same status
`ReceiveBind::patterns` and `MatchCase::pattern` have. Substitution did not treat it that way: every
child slot of every `ExprInstance` arm was visited at the enclosing `ctx`.

Two `BoundMapChain` lookups decide what a name inside the pattern can mean. A plain `x` goes through
`get`, which reads the **current scope only**, so it is a fresh binding occurrence and can never be a
`BoundVar` pointing outside. `=x` goes through `find`, which walks the whole chain, and is emitted as
`VarRef { depth }` carrying the chain distance. So **`VarRef` is the only construct that can name an
outer binder from inside a `matches` pattern** — and `maybe_substitute_var_ref` fires only when
`term.depth == ctx.depth`. Visiting the pattern at the enclosing depth stranded exactly those `VarRef`s:
`substitute(matches_par(var_ref(1, 2)), depth = 1, env)` returned the `VarRef` untouched where the
environment binding was due.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — the substituted term differs. |
| 2 · verdict | **MOVES** — a pattern whose `VarRef` is now resolved matches a different set of targets. |
| 3 · bytes (Lane B) | **MOVES** — the substituted term is what gets encoded. |
| 3 · bytes (Lane P) | **MOVES** — ★ *"Substituted bytes are signed bytes."* **CITED**. |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | NO — normalization is unchanged. |
| 6 · metering | NO — `substitute_and_charge` charges `encoded_len()` of the **result**, once, outside the recursion; a different result is a different charge only in the sense that the *value* changed. ⚠ See below. |

⚠ **A qualification on the metering cell.** The charge is `subst_term.encoded_len()`. If the substituted
term now differs, its encoded length differs, so the *amount* charged differs. The charge **count** and
**order** are unchanged; the **value** follows the term. This is not an independent metering change — it
is Axis 1 propagating into Axis 6 — but a reviewer counting charges should know it. **DERIVED**.

**The disagreement.** `new x in { … for (@y <- ch) { if (y matches =x) { … } } … }` — a `matches`
pattern naming an outer binder. On an old node the `VarRef` is not substituted and the `matches` test
compares against an unresolved reference; on a new node it compares against the bound value. Different
branch, different state, **safety fork**.

**Blast radius.** Any term whose `matches` pattern contains `=x` (a `VarRef`) naming a binder outside
the pattern. Reachable by an ordinary deploy: **yes** — `=x` is ordinary Rholang. Reachability is
MECHANISM-ONLY: the mechanism is proven end-to-end by a unit-level witness
(`substitute(matches_par(var_ref(1, 2)), …)`), but no full deploy was exhibited.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: scan historical deploy terms for an `EMatches` whose `pattern` subtree
contains a `VarRefBody`. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** `=x` inside `matches` is the *only* way a program can say "the
same value as the outer binder", and it did not work. The failure is silent: the pattern is well-formed,
the term normalizes, and the comparison simply asks the wrong question.

**Why `has_locally_free` needed no change, and why that is correct rather than a shortcut.** It computes
this node's `locally_free` and `connective_used` from the **target alone**. Because a plain `x` in the
pattern is a *fresh binder* (the `get` lookup above), the pattern contributes nothing to the enclosing
scope's free variables. `matcher::spatial_matcher`'s `EMatchesBody` arm and
`par_children::substitute_descends_into` already say so; this commit adds the third leg — the **per-slot
depth** — that those two were describing. **DERIVED**.

**★ Sibling enumeration, because this campaign's most-repeated failure is the sibling-blind patch.**
`EMatches` is the **only** `ExprInstance` arm whose `has_locally_free` reads a *proper subset* of the
slots substitution descends into. The other 18 multi-slot arms either OR/union every operand (16 binary,
2 unary) or read a cached summary field computed over all of them (`EList`, `ETuple`, `ESet`, `EMap`,
`EMethod`, `EPathmap`, `EZipper`). **Count: 1.** **DERIVED**.

**Why the fix is single-sourced.** A per-slot depth is exactly the kind of fact a worklist driver and a
recursive oracle can drift apart on. `expr_arm_pattern_slots` names the pattern slots once;
`substitute_drive`'s `descend_expr` and `substitute_oracle`'s `expr_recursive` both read it — the
failure mode `substitute_combine`'s split/rebuild table exists to prevent.

**Authority.** No owner ruling. The commit records the version obligation and declines to act on it:
*"No consensus version is bumped here; that call is not this commit's to make."* **CITED**.

#### Evidence

- The witness is unit-level and exact: `substitute(matches_par(var_ref(1, 2)), depth = 1, env)` returned
  the `VarRef` untouched pre-fix. **MEASURED**.
- Driver and oracle are compared over the substitution corpus, so the per-slot depth cannot drift
  between the two lanes. **MEASURED**.

---

### CBR-007

**A connective attempt owns its own `FreeMap` — a negation that SUCCEEDS was handing out bindings it never made.**

| | |
|---|---|
| Commit(s) | `b219e199` (the combinator and the four sites), `dc383ed1` (the correction record — documentation only) |
| Status | **LANDED** 2026-07-29T14:42:44−04:00 |
| Direction | PERMISSIVE — it *un-refuses* matches that should have fired |
| Evidence grade | **WITNESSED** |
| Files | `models/src/rust/utils.rs` (`IsolatableState`, `isolate_free_map`, `Attempt`, `attempt_opt`, `attempt_opt_keeping_bindings`); `rholang/src/rust/interpreter/matcher/spatial_matcher.rs`. 1,717 insertions / 39 deletions across 5 files, of which three are new test files. |

#### (a) The issue

`SpatialMatcherContext::free_map` is a plain `&mut` field, so **a refused attempt is destructive**:
whatever it wrote is simply still there. Matching is a search, and a search that cannot undo a step
reports the wrong answer.

**CBR-005** fixed this at `list_match::match_function` — the bipartite-matcher boundary — and at the
disjunction's branches. **Four sites remained, all interior to a single `spatial_match(Par, Par)` call**,
which is exactly why `match_function`'s entry snapshot cannot see them:

| # | Site | Escapes on | Disposition | Why |
|---|---|---|---|---|
| 1 | `ConnAndBody`'s conjunct `try_fold` | **REFUSAL** | `attempt_opt_keeping_bindings`, around the **whole fold** | The conjuncts share one free map on purpose (`p_conjunction_normalizer.rs:12-14`) and **stand or fall together**; a per-conjunct restore would revert only the conjunct that refused. |
| 2 | `ConnNotBody`, inner **succeeded** | **REFUSAL** | `attempt_opt` | A negation binds nothing, ever. |
| 3 | ★ `ConnNotBody`, inner refused mid-bind | **SUCCESS** | `attempt_opt` | **The worst of the four**: a negation that *succeeds* hands its caller a binding it had no right to make. |
| 4 | ★ `for sp in sub_pars(..)` — the retry loop | **SUCCESS** | `attempt_opt_keeping_bindings` | **Not an arm** — it is the loop that *drives* the arms, so no arm-level snapshot can see it. |

★ **Why inverting through a bare `Option` hid it.** In the `ConnNotBody` arm, *"the inner attempt
refused"* and *"the negation succeeded"* are **both** `Some(())`; *"the inner attempt succeeded"* and
*"the negation refused"* are **both** `None`. Two distinct facts per cell, one of which is about state,
silently dropped. `Attempt::{Bound, Refused}` separates them. **CITED** (`b219e199`).

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — downstream of the verdict: which branch reduces determines what is computed. |
| 2 · verdict | **MOVES** — ★ **MEASURED, with an ordinary Rholang program.** This is the axis that separates this entry from **CBR-005**. |
| 3 · bytes (Lane B) | NO — no encoding changed. |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | **MOVES** — by the same route as Axis 1. |
| 5 · accepted programs | NO |
| 6 · metering | NO — *"there is no cost accounting anywhere under `matcher/`, so nothing here can move metering"*; **no new counters** were added. **CITED**. |

★ **The witness, measured at the pre-fix HEAD** (`b219e199`):

```text
match { @0!(7) | @1!(6) } { @0!(~{x /\ 8}) | @1!(~{y /\ 9}) => A   _ => B }
```

reduces down the **fall-through** case. The first case is the correct one: `~{x /\ 8}` against `7`
demands `8`, refuses, so the negation succeeds. Equivalently

```text
{ @0!(7) | @1!(6) } matches { @0!(~{x /\ 8}) | @1!(~{y /\ 9}) }   ⇒  FALSE
```

where **`true` is correct**. The mechanism is mechanical rather than hand-chosen: `~P` and `P \/ Q`
bodies are normalized against a **fresh** `FreeMap` which `combine_p_negation` then **discards**, so a
free variable under `~` is `FreeVar(0)` in a numbering nobody kept — and level 0 is very probably
somebody *else's* level in the shared map it leaks into. Two sibling sub-patterns each containing a
binding negation therefore both leak level 0, `first_duplicate_added_var` sees the repeat,
`aggregate_updates` refuses, and the whole list match refuses. Per-`delta`:

```text
before b219e199:   d1 = {0 -> 7},  d2 = {0 -> 6}   =>  duplicate  =>  REFUSE  =>  B runs   (wrong)
after  b219e199:   d1 = {},        d2 = {}         =>  disjoint   =>  commit  =>  A runs   (right)
```

**CITED** (`dc383ed1`). The `Set` route (`ESetBody`'s `ListMatch<Par>`) does the same. One-sibling and
non-binding variants are correct on both sides: the defect is specific to a **binding body in two
siblings**.

**The disagreement.** An old node evaluates the `matches` above to `false` and takes the fall-through
`match` case; a new node evaluates `true` and takes the first case. Different continuation, different
tuplespace, **safety fork**.

**★ Blast radius — narrower than the design first claimed, and the narrowing is derived.** The reachable
surfaces are **exactly `match` case patterns and the right-hand side of `matches`** — **not COMM**.
`fail_on_invalid_connective` rejects `~` and `\/` in `for` / `contract` patterns at depth 0, and every
nested pattern position is compared by structural equality. **CITED**. Reachable by an ordinary deploy:
**yes**, within that surface.

**Could live chain state have been produced under the old behaviour?** ⚠ **Not settleable from inside the
repository** — this is the entry the campaign flagged explicitly as unanswerable. Settling query: scan
chain history and the deploy corpus for `MatchCase` patterns and `EMatches` right-hand sides whose
`connectives` contain a `ConnNotBody` or `ConnOrBody` whose body carries a `FreeVar` or has
`connective_used == true`. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Two things, and the second is the sharper argument.

1. A `match` takes the **wrong branch** on an ordinary Rholang program, and `matches` answers `false`
   where `true` is correct.
2. ★ **CBR-005 shipped a live backstop that fired on correct programs until this landed.** The
   `panic!`→`None` refusal in `aggregate_updates` was reachable *because* `~`/`\/` fresh-numbering
   produces apparent duplicates while the leak persists. This is now **measured, not argued**: see the
   refusals-counter table below and §6.2.

**Why the combinator rather than four copies of snapshot/restore.** The law lives at **one address** —
`models::rust::utils::isolate_free_map`, beside the `FreeMap` it is about — and `rholang` adds a
**one-line** `impl IsolatableState for SpatialMatcherContext`. A `rholang`-local copy beside the `models`
one is precisely `a1feb437`'s three-copies defect (**CBR-008**). ★ The `// NOT FULLY IMPLEMENTED` stub at
`models/src/rust/utils.rs:260-262` is **retired at the address it was written** rather than orphaned; it
could not have been repaired in place, because it took an *already-evaluated* `Option<()>`, so
restoration was unspellable and `operation.map(|_| ())` was the identity. **Caller count 0 → 6.**
**CITED**.

**Why two wrappers and not one.** They are the two dispositions the matcher actually has, not variations
on a theme: a negation and a disjunction **bind nothing by construction** — the normalizer discards
their free maps — so they *probe*; a conjunction and an accepted `sub_pars` candidate **bind for real**,
so they *commit*.

**★ Why `Attempt<T>` is not decoration, and why it is the OPPOSITE of the `#33` defect.** Task `#33`
*collapsed* a disposition (`Err(_) => false`) and lost a guarded process. `Attempt<T>` carries strictly
**more** than `Option<T>`; the projection back (`into_option`) is explicit and total; and the negation's
verdict is bit-for-bit the same function of the inner verdict. **Only the state effect is dropped, which
is the point.** **CITED**.

**Why no snapshot at the direct `spatial_match(Par, Par)` entry.** Every production entry
(`Matcher::get`, `spatial_match_result`, `Reduce::eval_match`, `eval_matches`,
`SpatialMatcherOracle::matches`) reads `free_map` only when the top-level verdict is `Some`, so a leak
escaping on top-level refusal is unobservable — and an entry snapshot **hides the leak at the exit
instead of reverting it where it was made.** Row 2′ of the acceptance matrix measures exactly that.
**CITED**.

**Why a proved elision rather than isolating everything.** Site 4 elides its snapshot on exactly the
seven arms that *provably cannot reach* `free_map` — `Conn{Bool,Int,String,Uri,ByteArray}`, `VarRefBody`,
`None` are each a pure `single_expr(&target)` read or a bare `None`. It is written as **"elide on exactly
these"** rather than "isolate on exactly those", so a **new** connective variant lands in the isolated
branch **by default**.

**What deliberately did not change.** No consensus version bump. No write-side cap. No pathmap sorting —
`aggregate_updates` keeps folding in the existing `matches` `BTreeMap` order. No new counters.
`rholang/tests/matcher/match_test.rs` (the 49-test acceptance oracle) and
`rholang/tests/matcher_disjunction_isolation.rs` are **byte-untouched**. `run_first`
(`models/src/rust/utils.rs`, `// STUBBED OUT`, zero callers) is **flagged, not fixed** — out of scope,
and disclosed rather than silently swept in.

**Authority.** ★ Owner ruling, verbatim, **2026-07-29T16:31:01Z**:

> *"Go with the principled solution that is best for long-term stability and expressibility that aligns
> with the philosophy of mettail/prattail. Do not make a pragmatic decision, hack, or workaround, fix
> the issues correctly."*

The decision put to the owner was framed explicitly as an extension of the **CBR-005** approval to a
**verdict-visible** change: *"You approved #144 on a proven mechanism with no failing test. #148 now has
a real Rholang program taking the wrong `match` branch — a stronger case — but it changes verdicts,
where #144 only changed bound values. The direction is safe (it un-refuses matches that should have
fired)."* **CITED**.

#### Evidence

**★ The acceptance matrix — five builds, every non-HEAD cell MEASURED, not predicted.** The diagnostic
rows were built by disabling one restore at a time, measured, and discarded; none was staged.

| build | RED1 | RED2 | RED3 | RED4 | RED5 (verdict) | E2E | oracle 49 | ctrl |
|---|---|---|---|---|---|---|---|---|
| HEAD | red | red | red | red | **red** | red | ok | ok |
| **2′ outer-boundary snapshot** | red | red | ok | **RED** | ok | ok | ok | ok |
| 2 arms only, no `sub_pars` | ok | ok | ok | ok | ok | ok | ok | ok |
| 3 `sub_pars` only, no arms | red | red | ok | ok | ok | ok | ok | ok |
| **4 composite (this commit)** | ok | ok | ok | ok | ok | ok | ok | ok |

Per-file at HEAD: `matcher_connective_isolation` **5/11**, `matcher_negation_isolation` **4/9**,
`matcher_negation_reduction_witness` **3/6** (the 3 REDs fail, 3 controls pass). At the composite:
**11/11, 9/9, 6/6**, plus 4/4 disjunction, 7/7 state-isolation, 15/15 disposition, oracle **49/49**.

★ **Row 2′ is the discriminator.** The cheap wrong fix — an entry snapshot at the `Par`/`Par` boundary,
restored on refusal — makes the **verdict-level REDs and the whole end-to-end witness go green while
leaving both retry-loop REDs red**, because those escape on a *success* path interior to one
`spatial_match(Par, Par)` call. It is the row that separates *"the bindings were reverted where they were
made"* from *"the bindings were hidden at the exit."*

**★ WHAT THE DESIGN GOT WRONG — measured, and disclosed rather than quietly corrected.** The design
predicted row 2 (arms only, no `sub_pars` restore) would leave RED 4 red. **It does not: row 2 is
entirely green.** With every arm isolated, the `SpatialMatcher<Par, Connective>` impl has the property
*"refusal ⇒ `free_map` unchanged"* for all ten of its arms, so the loop has nothing left to revert.
Site 4 is therefore **not independently load-bearing today**; it is a **local guarantee** — the loop does
not have to trust a whole-program property of every present and future arm — and it is kept for that
reason and because it is free (below). ★ Row 3 is the sharper converse: site 4 **alone** fixes everything
except the two tests that enter `SpatialMatcher<Par, Connective>` *directly*, because every path from a
`Par` pattern to a connective goes through the retry loop. **MEASURED** (`b219e199`).

**★ The refusals counter — the direct measurement, not a proxy.** Whole `-p rholang -p models` suite,
debug, `--nocapture`, marker grepped:

| build | firings | breakdown |
|---|---|---|
| **at HEAD (pre-fix)** | **8** | ★ **SIX from PRODUCTION paths** — the three verdict-level REDs in `matcher_negation_isolation` and **all three** end-to-end witnesses — plus the two deliberate direct-call fixtures in `matcher_state_isolation.rs` (levels 0 and 5). |
| **after** | **2** | **only** those two deliberate fixtures. |

Identical in release. **MEASURED** (`b219e199`, `dc383ed1`). This is what makes §6.2's disclosure a
*record* rather than an argument.

**Cost — MEASURED, `n = 3`, release, `taskset -c 4`, governor `performance`, no pair of ranges
overlapping:**

| fixture | build | mean ns/match | sd | vs HEAD |
|---|---|---|---|---|
| A — backtracking bipartite `list_match`, 6×6, 20,000 matches/run | HEAD | 189,547.7 | 1,111.5 | — |
| A | arms-only | 194,874.7 | 302.3 | +2.81 % |
| A | **composite** | 192,099.3 | 772.6 | **+1.35 %** |
| B — wide `~`: `~{x} \| _` against 7 sends, all `$`2^7`$` splits rejected; **site 4's worst case**, 500 matches/run | HEAD | 12,043,466.7 | 13,390.5 | — |
| B | arms-only | 11,151,173.0 | 14,270.9 | −7.41 % |
| B | **composite** | 11,083,677.4 | 10,720.6 | **−7.97 %** |

Site 4's worst case is **7.97 % faster**, for the same reason **CBR-005** measured 7.96 %: under the
restore the map stops accumulating, so each clone is `$`O(\text{entry})`$` rather than
`$`O(\text{entry} + \text{everything every rejected candidate leaked})`$`. ★ The composite is **faster
than arms-only on both fixtures**, so site 4's isolation is not a cost at all — it is a small win.
⚠ These absolute numbers are **not** comparable with **CBR-005**'s 51,666 → 47,553: that fixture was
throwaway and was never committed. **The deltas are the quantity.**

**Tests — and one must not edit one's own oracle.** `matcher_connective_isolation.rs` (6 REDs, 5
controls, over the four sites; ★ C2 is the **anti-over-restore** control — a *successful* conjunction
must still bind the union of its conjuncts, at every level separately, so if the conjunction's restore
were written unconditional C2 flips green → red, *"and that would be a defect in the fix, not an
expectation to adjust"*); `matcher_negation_isolation.rs` (the site-3 minimal witness, the three
verdict-level REDs, and a test that **pins the premise** — the normalizer emits level 0 in *every*
negation body, `free_count == 0`, levels `[0, 0]`, so the collision is mechanical); and
`matcher_negation_reduction_witness.rs` (real programs through `RhoRuntimeImpl`, asserted on what reaches
the tuplespace). `models/src/rust/utils.rs` gains a `#[cfg(test)] mod isolation_tests` pinning the law
itself, including the one cell on which the two combinators differ.

★ **The `ConnOrBody` arm is the control for the combinator itself**: its hand-written clone/run/restore
was byte-for-byte what `attempt_opt` does, so if the combinator did anything else this arm moves and
`matcher_disjunction_isolation.rs` — byte-untouched — says so. And the two `ConnNotBody` branches
deleted were **byte-identical** (`has_or_body` was computed, matched on, and both arms did the same
thing), covered by `control_a_negation_over_a_disjunction_is_not_a_special_case` rather than trusted.

⚠ **One test moved green → red, and it was split rather than adjusted.** A test first written as a
control — comparing a negation over a disjunction-bearing body against one without, in **both** verdict
and state — turned out to discriminate at HEAD, because the two bodies differ in state there for exactly
the reason this commit fixes. Per the anti-fixup rule it was **split**: the verdict half stays a control
(green on both sides, and the verdict is all the deleted `has_or_body` branch could have moved) and the
state half is reclassified as the RED it actually is. **No previously-green test was weakened.**
**CITED**.

### CBR-008

**A relative path below the root built a key no `Par` can have.**

| | |
|---|---|
| Commit(s) | `a1feb437` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/pathmap_integration.rs`, `rholang/src/rust/interpreter/fused_pathmap_chain.rs`, `.../reduce.rs` |

#### (a) The issue

Every value in a `RholangPathMap` is filed under `encode_trie_path` **of itself**, and `decode_trie_path`
accepts exactly the encoder's image. A reader composing a *relative* path below the root built a key
outside that image, so the key **missed on every map, whatever the map held**. It was not answering
"absent"; it was asking an unanswerable question. The composition law existed in **three copies** and two
of them were wrong; it is now spelled once, at `cursor_entry_key`, the single point every entry key
passes through.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — `atPath` / `descendTo`+`getLeaf` below the root with a bare relative argument returned `Nil` and now return the composed entry. |
| 2 · verdict | **MOVES** — a program branching on `== Nil` takes the other branch. |
| 3 · bytes (Lane B) | **MOVES** — `descendTo` below the root by a bare argument writes `EZipper.cursor_kind` 1 (BARE) → 0 (SPLIT). |
| 3 · bytes (Lane P) | **MOVES** — `cursor_kind` is a **wire field**, so the change is visible to a program that stores the zipper without ever reading a leaf. |
| 4 · post-state hash | **MOVES** — these results reach programs, therefore the tuplespace and the event hash. |
| 5 · accepted programs | NO |
| 6 · metering | NO — *"No metering site changed"*; charge-trace neutrality asserted on the ground-list corpus. **CITED**. |

Four named result changes, quoted: *"`atPath` / `descendTo`+`getLeaf` below the root with a BARE
relative argument returned Nil and now return the composed entry … `descendTo` below the root by a bare
argument writes `EZipper.cursor_kind` 1 (BARE) → 0 (SPLIT) … `dropHead(n ≥ 1)` removes entries the codec
does not split instead of rewriting their interiors … `dropHead(0)` keeps the empty-path entry `[]`
instead of deleting it."* **CITED**.

**The disagreement.** A deploy navigating a path map by a bare (non-list) relative argument gets `Nil`
on an old node and the entry on a new one, then writes the difference into the tuplespace. Safety fork.

**Blast radius.** Programs using bare (non-list) path arguments below the root, and `dropHead`.
★ **Programs whose paths are all ground lists and whose cursors are all at the root move ZERO bytes** —
pinned by three named rows and by the whole pre-existing zipper corpus (63 tests across nine specs).
**CITED**. Reachable by an ordinary deploy: **yes**.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: scan historical deploy terms for a `EMethod` named `atPath`, `descendTo`,
`getLeaf` or `dropHead` whose argument is not a ground `EList`. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Today's answers are wrong. A path-map lookup that can never hit
is worse than an error, because the program *proceeds* on the false premise that the entry is absent.

**Why the invariant is now enforced rather than documented.** The law is checked at `cursor_entry_key`
under `#[cfg(debug_assertions)]`, so **consensus nodes compile it out and pay nothing** while the whole
test corpus runs with it live. Run against the pre-fix composition it names the defect **byte-exactly at
the first moment it is visible**. That is the shape that survives: a law with three copies is a law that
drifts, which is the direct lesson **CBR-007** cites back to this commit.

**Authority.** No owner ruling. The commit is explicit about the version obligation and declines to act:
*"f1r3node has no activation-height machinery … so a consensus change ships as a COORDINATED VERSION
BUMP. Whether to ship it as one is F1r3node's decision: NOTHING IS BUMPED HERE."* **CITED**.

#### Evidence

- `cargo test -p models --test pathmap_integration_tests` with the pre-fix composition restored:
  **54 passed, 2 failed** (the debug guard firing inside `cursor_entry_key`, naming the bytes);
  post-fix **56 passed, 0 failed**. **MEASURED** (`a1feb437`).

---

### CBR-009

**`setSubtrie` stopped dropping bare source entries.**

| | |
|---|---|
| Commit(s) | `ce7bb4fd` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/pathmap_crate_type_mapper.rs`, `rholang/src/rust/interpreter/reduce.rs` |

#### (a) The issue

`setSubtrie` derived its destination keys by a rule that disagreed with `encode_trie_path` for
**non-list** (bare) source entries, so those entries were **silently dropped**: the result could be short
by whole entries. The repair derives the value **from the key**, collapsing two routes into one.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — the resulting map holds entries it previously lost. |
| 2 · verdict | **MOVES** — a subsequent match or size test on that map answers differently. |
| 3 · bytes (Lane B) | **MOVES** — a different map serializes differently. |
| 3 · bytes (Lane P) | **MOVES** |
| 4 · post-state hash | **MOVES** — *"`setSubtrie` results reach the tuplespace and the event hash."* **CITED**. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

**The disagreement.** `m.setSubtrie(path, src)` where `src` holds any non-list entry: the old node's
result is missing those entries, the new node's is not. Safety fork.

**Blast radius.** *"any program whose source map holds a non-list entry — today those results are wrong,
and can be short by whole entries."* ★ *"Programs whose source entries are all ground lists move ZERO
bytes"* — pinned by `set_subtrie_with_split_source_entries_is_unchanged`, the one test in the new spec
that was **already green before the fix**. **CITED**. Reachable by an ordinary deploy: **yes**.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: scan historical deploys for `setSubtrie` calls, then check whether the source
map literal contains a non-`EList` element. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Silent data loss inside a collection operation. There is no
error, no diagnostic, and the resulting map is well-formed — so nothing downstream can detect it.

**Why the invariant is checked everywhere rather than at `setSubtrie`.**
`rholang_pathmap_to_e_pathmap` is *the single point every trie passes through on its way back to a
value*, and it now asserts `trie_entry_divergences(map).is_empty()` under `#[cfg(debug_assertions)]`.
The full suite was run with the guard live and **it did not fire once**: there is no third divergent
producer. That is the difference between fixing an instance and establishing there are no siblings —
which this campaign's own repeated failure mode makes the necessary standard. **CITED**.

**Authority.** No owner ruling; same version posture as **CBR-008**, quoted there.

#### Evidence

- ★ **Anti-vacuity, recorded**: the new spec was run against the pre-fix tree first. **7 of its 8 tests
  FAILED**, the guard naming the byte-exact divergence; the one that passed was the ground-list arm.
  **MEASURED**.
- ★ **No pre-existing test could have caught this**: `setsubtrie_spec.rs` had five tests, every one
  asserting only that evaluation raised no errors, and **every source entry in all of them is a ground
  list**. **MEASURED**.
- Post-fix: `zipper_path_management_spec` 8, `setsubtrie_spec` 5, `zipper_query_methods_spec` 18,
  `getsubtrie_spec` 4, `demo_verification` 1, `epathmap_replay_equivalence` 1,
  `zipper_enumeration_spec` 14, `zipper_advanced_navigation` 10, `epathmap_live_match_spec` 5 — all
  passing. **CITED**.

---

### CBR-010

**Path readers ask the codec instead of guessing; `next_value_path` becomes sound for every `from_key`.**

| | |
|---|---|
| Commit(s) | `0a6d2ce0` (readers ask the codec), `5aacebc3` (`next_value_path` totality) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/pathmap_integration.rs`, `models/src/rust/pathmap_zipper.rs`, `models/src/rust/pathmap_native_query.rs`, `rholang/src/rust/interpreter/fused_pathmap_chain.rs`, `.../reduce.rs` |

#### (a) The issue

`EZipper.current_path` (`RhoTypes.proto:368`, `repeated bytes`) stores per-element segments but **not the
split/bare discriminator**, so every reconstruction of the entry key had to *guess* — and always guessed
"split". Readers that **hold the path `Par`** can instead ask the codec directly. Separately,
`next_value_path` was unsound for a `from_key` dangling two or more bytes past the deepest existing node
(a root cause traced into `pathmap-0.2.2`'s `ReadZipperCore::to_next_get_val`).

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — on bare-arm paths and escape-arm carriers only. |
| 2 · verdict | **MOVES** — same population. |
| 3 · bytes (Lane B) | **MOVES** — same population; **zero** on the ground-list corpus. |
| 3 · bytes (Lane P) | **MOVES** — same. |
| 4 · post-state hash | **MOVES** — same. |
| 5 · accepted programs | NO |
| 6 · metering | NO — charge-trace neutrality asserted across nine specs. **CITED**. |

★ **The boundary is stated exactly, which is what makes this reviewable**: *"At the root, `entry_key_at`
is BYTE-IDENTICAL to the retired expression on the SPLIT arm … So the entire ground-LIST corpus moves
ZERO bytes. Asserted directly by `entry_key_at_the_root_moves_no_split_arm_bytes`. It differs exactly
where the retired expression was wrong: bare-arm paths … and escape-arm carriers."* **CITED**.

**Blast radius.** Programs whose path-map keys are not ground lists, or whose list carriers also carry a
send/receive/new/match/bundle/connective (the escape arm). Reachable by an ordinary deploy: **yes**.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: as **CBR-008**. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A cursor that guesses is a cursor that is wrong for one of the
two arms, always. The measured consequence: *"asking for the bare `1` returns the singleton list `[1]` —
a **wrong ANSWER, not a miss**"*, and *"whole-key prefix-freeness is broken, so bare entries sit at
interior trie positions and every `terminate=false` reader conflates entry with prefix."* **CITED**
(`caadf839`, the coverage-gap commit that made these witnesses executable).

**Why not fix the insert side instead.** Explicitly rejected, with the reason: *"The insert side is
correct and injective and is NOT the bug: appending the terminator there would make
`encode_trie_path(5) == encode_trie_path([5])` and merge two distinct entries."* **CITED**.

**Zero insert-side bytes.** `create_pathmap_from_elements`, `encode_trie_path`, `path_stream_of` and the
event-hash legs are untouched by both commits; `epathmap_wire_serialized_paths`,
`epathmap_canonical_fixtures` and `epathmap_spliced_event_bytes` green unchanged. **CITED**.

**Authority.** No owner ruling.

#### Evidence

- Injectivity kept green: `distinct_pars_have_distinct_keys`, `mixed_arms_produce_four_distinct_entries`.
  **CITED**.
- `5aacebc3` carries an **order oracle** — the specification checked against a brute-force scan of every
  key over random raw-key tries, with probe keys drawn from exactly the shapes a lossy cursor emits
  (every key, every proper prefix, every key extended by `00`/`01`/`7f`/`ff`) — and termination as a
  **bounded** property (`val_count()` steps visit every entry once; step `val_count()+1` is `None`), so
  a regression **fails rather than hangs**. **CITED**.

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
- Goldens unchanged: `prost_goldens_epathmap_fixtures`, `serde_bincode_goldens_epathmap_fixtures`,
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
| Files | `models/src/main/protobuf/RhoTypes.proto`, `models/src/rust/rhoapi_ext.rs`, `models/src/rust/rholang/wire.rs`, `wire_encode.rs`, `sorter/sort_combine.rs`, `models/src/rust/spliced_event_bytes.rs`, `models/src/rust/canonical_path.rs`, `rholang/src/rust/interpreter/reduce.rs` (and five more) |

#### (a) The issue

**CBR-011** made the comparators read the trie. This deletes the `Vec` itself, so there is no second
order left to read. With it go the canonicalisation fork (four consumers each carrying an "if this map is
ground, read the entries off a trie instead" branch), the `ps_make_mut` bypass and its `debug_assert`
policing, the cached-bytes validity checks, and a raw-pointer arena in `wire_encode` that existed only
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
- `wire_encode_space` measures the consequence of deleting the arena: a ground map's steady-state encode
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
key — which is why the grade is MECHANISM-ONLY rather than WITNESSED. Also: *"`decode ∘ encode` is the
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
| 3 · bytes (Lane B) | **MOVES** — `ezipper.bincode.bin` **833 → 837**: four zero bytes appended at the end. Nothing before offset 833 moved. `ezipper.json` **5090 → 5110**. **CITED**. |
| 3 · bytes (Lane P) | **NO** — ★ *"`ezipper.prost.bin` UNCHANGED. prost omits a default-valued scalar, so the CONSENSUS encoding of every pre-existing zipper is byte-identical, and every EZipper serialized before this field decodes to exactly the prior semantics."* **CITED**. |
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

- Golden-by-golden accounting, **MEASURED** and quoted: prost unchanged; bincode 833 → 837 with nothing
  before offset 833 moved; JSON 5090 → 5110 with nothing before it moved; **every other golden**
  (`e6a_index`, `locally_free`, `nested`, `remainder_connective`) unchanged across all three encodings;
  every event-hash golden unchanged; **zero insert-side bytes**.
- `spliced_event_bytes::emit_ezipper` mirrors the new field so the hand-rolled event-hash emitter stays
  byte-identical to `bincode::serialize`. **CITED**.
- `navigation_reaches_both_arms` asserts both halves. **CITED**.

---

### CBR-015

**`cursor_kind` reaches the pretty printer, whose output is replay-compared.**

| | |
|---|---|
| Commit(s) | `76de7d44` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | LATENT |
| Files | `models/src/rust/pathmap_integration.rs`, `rholang/src/rust/interpreter/pretty_printer.rs`, `pretty_printer_oracle.rs` |

#### (a) The issue

`EZipper.cursor_kind` participates in `PartialEq` and `Hash` (`models/src/lib.rs`) and is emitted into
the event hash (`models/src/rust/spliced_event_bytes.rs:576`) — but **neither copy of the hand-duplicated
`EZipper` printer read it**. Two zippers that compare unequal and hash differently **printed the same
string**.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | **MOVES** — for a `BARE`/`PREFIX` cursor only. The printer's output reaches `build_channel_string` → `cap` → `error_message` → `ProcessedSystemDeploy::Failed`, which is block-resident and replay-compared. |
| 3 · bytes (Lane P) | NO — the printer's output enters the block as a `string` field; its *content* changes on Lane B's terms but no protobuf layout changes. ⚠ See qualification. |
| 4 · post-state hash | **MOVES** — via the block-resident `error_msg`. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

⚠ **A qualification the register owes the reviewer.** The Lane-P cell is `NO` in the sense that no
protobuf *schema or field order* changes. The *string value* carried in `ProcessedSystemDeploy.error_msg`
does change for a `BARE`/`PREFIX` zipper, and that string is inside the block. A reviewer reading only
the axis grid should read this cell together with the Lane-B and hash cells. **DERIVED**.

★ **The `SPLIT` row is byte-identical to the previous rendering**, and `SPLIT` is proto value 0 — what
every `EZipper` serialized before `cursor_kind` existed decodes to. **No previously-representable
zipper's output moves.** **CITED**. That is why the grade is LATENT: producing a divergence requires a
`BARE` or `PREFIX` cursor to reach a failing system deploy's error message.

**Blast radius.** Programs whose failure path renders an `EZipper` with a non-`SPLIT` cursor. Reachable
by an ordinary deploy: **yes in principle** — the path is reachable from untrusted input via
`rho:io:stdout` — but no witness was exhibited.

**Could live chain state have been produced under the old behaviour?** No: the `SPLIT` rendering is
pinned byte-for-byte and `SPLIT` is what every pre-`cursor_kind` zipper decodes to. **DERIVED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A printer that cannot distinguish two values the system considers
distinct is a printer that lies, on a path a human reads during an incident.

**Why the renderer is TOTAL rather than asserting.** `cursor_kind` is a **peer-controlled `u32`**, so a
`Bare` cursor at a depth no bare entry inhabits, and an unrecognized wire value, are **marked**, not
asserted. *"A pretty printer that can fail cannot be used in an error path, and this one is in the error
path."* **CITED**.

**Authority.** No owner ruling.

#### Evidence

- The three-way conjunction — `!=`, hashes differ, **prints differ** — asserted on a pair differing in
  `cursor_kind` and nothing else. **The third conjunct was false** pre-fix. **MEASURED**.
- **CONTROL**: the `SPLIT` rendering pinned **byte-for-byte**, at depth 1 and at the root. It must not
  move, and it does not. **MEASURED**.
- All four wire values render and are pairwise distinct. **MEASURED**.

---

### CBR-016

**An operator-tunable environment variable leaves the consensus byte path.**

| | |
|---|---|
| Commit(s) | `ca84d535` (the split), `4290303c` (restore after a concurrent whole-file rewrite) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `shared/src/rust/shared/printer.rs`, `rholang/src/rust/interpreter/pretty_printer.rs`, `casper/src/rust/util/rholang/system_deploy_user_error.rs` |

#### (a) The issue

`PRETTY_PRINTER_OUTPUT_TRIM_AFTER` — an **environment variable** — sat inside a consensus byte path,
traced hop by hop:

```text
SystemDeployTrait::extract_result        (system_deploy.rs)
  → SystemDeployPlatformFailure::UnexpectedResult(Vec<Par>)
    → Display::fmt → show_seq_par → PrettyPrinter → cap
      → SystemDeployUserError::error_message
        → ProcessedSystemDeploy::Failed { error_msg }      ... into the block
          → replay_runtime.rs  expected_error == actual_error
```

Two validators with different settings of that variable computed **different `error_message` bytes for
the same failing system deploy**, and replay compares those bytes.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | **MOVES** — the rendered string changes for any operator not running the default. |
| 3 · bytes (Lane P) | **MOVES** — `ProcessedSystemDeploy.error_msg` is a block field. |
| 4 · post-state hash | **MOVES** — the block content changes. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

**The disagreement — and this one is a pre-existing live fault, not a new risk.** Before the fix, two
validators of the *same* build could disagree, purely by configuration:
`ReplayFailure::system_deploy_error_mismatch` **for a deploy that executed identically on both**. After
the fix, `Consensus` renders from the compile-time `Printer::CONSENSUS_TRIM_AFTER` and only the
`Operator` audience reads the environment.

**Blast radius.** Every failing system deploy on a node whose operator set the variable. Reachable by an
ordinary deploy: **yes** — the *deploy* need only fail; the divergence is supplied by the operator.

**Could live chain state have been produced under the old behaviour?** ★ **Partly settleable, and the
answer is important.** If every historical validator ran the default, no divergence occurred. That is an
**operational** fact, not a repository fact. Settling query: audit node deployment configurations for
`PRETTY_PRINTER_OUTPUT_TRIM_AFTER`; separately, scan history for any
`ProcessedSystemDeploy::Failed`. If the second count is zero the question is moot. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** *"An operator-tunable environment variable was a consensus input,
and nothing in its name, its type or its call site said so."* **CITED**. This is the sharpest kind of
consensus defect: correct code, correct tests, and a fork available to anyone who tunes a log setting.

**Why an `Audience` split rather than clamping the variable's range.** Clamping was **explicitly
rejected**: *"it would keep a local setting in the byte path and merely narrow the window in which an
operator can move it."* **CITED**. The split makes the *type* carry who a render is for: `Operator`
reads the environment and reaches logs, stdout, the REPL and `rnode eval`; `Consensus` reads the
compile-time constant and reaches a block.

**Authority.** No owner ruling. The commit states the version obligation: *"it ships as a coordinated
upgrade to a new genesis-anchored protocol version, which is a release decision, not a precondition for
the code."* **CITED**.

#### Evidence

- `system_deploy_error_message_determinism`: **one** sha256 for the consensus `error_message` across
  `TRIM = 4 / 40 / 200`, and **three** for the operator control. **MEASURED** (re-verified in
  `4290303c`).
- `the_capping_call_sites_are_reproduced` green and still straddling: `ok=1 panics=3` at every one of its
  five trims. **MEASURED**.
- ⚠ **A process failure worth disclosing**: `a4c23a58` rewrote `pretty_printer.rs` wholesale from a base
  predating `ca84d535`, removing the four hunks that gave the printer an `Audience`, so **HEAD did not
  compile** until `4290303c` restored them. Concurrent agents editing one file cost a consensus-relevant
  hunk; it was caught by the build, not by review. **CITED**.

---

### CBR-017

**The `New` bind bound becomes an interval — and the clamp it replaces had been moving consensus bytes.**

| | |
|---|---|
| Commit(s) | `a4c23a58` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `rholang/src/rust/interpreter/pretty_printer.rs` |

#### (a) The issue

The printer materialised one entry per name bound by a `New`, clamped to a display cap. `news_shift_indices`
is read back by `is_new_var`, which decides the `*` prefix on a bound variable — and **clamping the
marking un-marks every slot past the cap**. The replacement is a `BindRange` interval: `len() == 1`
regardless of `bind_count`.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | **MOVES** — the `*` prefix on variables past the old cap. |
| 3 · bytes (Lane P) | **MOVES** — same string, same block field as **CBR-016**. |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | ⚠ **MOVES**, in the liveness sense — see below. |
| 6 · metering | NO |

★ *"That clamp moved consensus-visible bytes."* **CITED** — measured by rendering a **151-point census**
under all three code states and diffing.

⚠ **The acceptance cell.** A `New` with `bind_count = i32::MAX` requested
$`(2^{31}-1) \times 4\ \mathrm{B} = 8{,}589{,}934{,}588\ \mathrm{B}`$ (7.999999999 GiB) from the
materialised form — **computed, never allocated**, because the test harness bounds the mutant. On the
real printer that is an allocation failure, i.e. a process abort on a term a peer can construct. The
interval form retains **32 B flat**. **MEASURED** at `bind_count` = 0, 1, 128, 1'000, 10'000, 100'000:
interval 32 B flat; materialised 32 / 36 / 548 / 4'548 / 404'548 B.

**Blast radius.** Any failing deploy whose rendered term contains a `New` binding more names than the
display cap. Reachable by an ordinary deploy: **yes**.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: scan historical `ProcessedSystemDeploy::Failed.error_msg` for a rendered
`new … in` whose bind count exceeds the display cap. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Two distinct faults in one expression: the rendered bytes are
wrong past the cap (a consensus fault), and the allocation is attacker-controlled (a liveness fault).

**★ Why `contains` is deliberately WRAPPING and not widened to `i64`.** This is the entry's subtlest
point and it is a *byte-preservation* argument, not a taste one: *"The node ships `release`, where the
materialised `i + bound_shift` wrapped; an `i64` reading would be the mathematically contiguous interval
and would therefore differ from the shipped bytes exactly where the two can be told apart."* **CITED**.
The interval reproduces the wrapping arithmetic so that the *only* byte movement is the one being
corrected.

**Authority.** No owner ruling.

#### Evidence

- The differential against a materialised `S(start, count)` is
  `a_bind_range_answers_membership_exactly_as_a_vector_did`, including **four wrapping cases** — and its
  anti-vacuity guard **caught the first corpus, which had only one**. **MEASURED**.
- `the_star_prefix_is_exact_across_nested_news` covers four slots the single-`New` fixture cannot see,
  *"found by census not by reasoning"*, and is the only fixture where `is_new_var` scans more than one
  interval. **MEASURED**.

---

### CBR-018

**The pretty printer renders its `match` target.**

| | |
|---|---|
| Commit(s) | `bd7cb45f` (the repair); `b98fa20a` and `739368a4` pinned the pre-repair bytes first |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `rholang/src/rust/interpreter/pretty_printer.rs` |

#### (a) The issue

Every `match` term printed `<unprintable>`. The path is block-resident and replay-compared
(`build_channel_string` → `SystemDeployPlatformFailure::UnexpectedResult` → `ProcessedSystemDeploy::Failed`,
compared byte-for-byte at `casper/src/rust/rholang/replay_runtime.rs:745-758`) and is reachable from
untrusted input through `rho:io:stdout`.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | **MOVES** — `<unprintable>` becomes the rendered term. |
| 3 · bytes (Lane P) | **MOVES** — same block field. |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | NO |
| 6 · metering | NO |

**Blast radius.** Any failing deploy whose rendered term contains a `match`. Reachable: **yes**.

**Could live chain state have been produced under the old behaviour?** ⚠ Not settleable from inside the
repository. Settling query: scan historical `error_msg` values for the literal `<unprintable>`.
★ This query is unusually cheap and unusually decisive — it is a **string search over block bodies** —
and it would settle the entry outright. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A consensus-visible diagnostic that says nothing about the term
it is diagnosing.

**★ Why it landed as its own commit rather than inside the printer's stack-depth conversion.** Stated
twice, in the two commits that deliberately deferred it: *"Changing what a `match` target prints changes
block-resident bytes, and that must not ride inside a stack-depth conversion."* **CITED** (`b98fa20a`,
`739368a4`). Those commits instead **pinned** the defective bytes — `a_match_target_renders_as_an_error_
string_and_that_is_pinned` — *"so they are a decision rather than an accident, and so the closed `PpNode`
dispatch that replaces `&dyn Any` in Stage D can be proven byte-neutral against them."* This is the
register's clearest example of correct sequencing: make the wrong bytes a *pinned* fact, prove the
refactor neutral against them, then move them in a reviewable commit of its own.

**Authority.** No owner ruling.

#### Evidence

- The byte movement is the assertion: the pinned golden had to be re-blessed, which is what makes the
  change visible in review rather than silent. **DERIVED**.

---

### CBR-019

**The channel leg of every event hash is routed through a new, single-walk encoder.**

| | |
|---|---|
| Commit(s) | `c28f4cf6` (the generated table and encoder), `903cefb3` (monomorphic emission), `00ff9187` (the wiring) |
| Status | LANDED |
| Direction | NEUTRAL |
| Evidence grade | NEUTRALITY-MEASURED |
| Files | `models/src/rust/rholang/wire.rs`, `wire_encode.rs`, `par_codec.rs`, `par_children.rs`, `models/build/wire_schema.rs`, `rspace++/src/rspace/hashing/stable_hash_provider.rs`, and every rspace++ event-hash call site |

#### (a) The issue

`hash_produce` / `hash_consume` hash the bincode encoding of the datum, each pattern, the continuation
**and the channel**. The channel leg went through `bincode::serialize`, which **traverses the term twice**
(`serialized_size` then `serialize_into`, bincode-1.3.3 `src/internal.rs:25-37`) and recurses. It is
replaced by a generated, single-walk, trampolined encoder.

⚠ **This entry exists precisely because it is claimed to move nothing.** It sits on the most
consensus-critical byte path in the system, so its neutrality is a *measured claim* that a reviewer must
be able to check, not an assumption they must accept.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | NO |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | **NO — and this is the claim under review.** Byte identity against the derived `Serialize`. |
| 3 · bytes (Lane P) | NO — untouched by these commits. |
| 4 · post-state hash | NO |
| 5 · accepted programs | NO |
| 6 · metering | NO |

**The disagreement that would occur if the claim is false.** Every produce and every consume in the
system would hash differently, so **every COMM event identity** would differ, so every block's deploy log
would differ. This is the largest blast radius in the register: **total**. There is no partial failure
mode.

**Could live chain state have been produced under the old behaviour?** All of it. Which is why the
neutrality obligation is absolute rather than statistical.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Nothing *correctness*-wise. The motivation is stack safety and
cost: `bincode::serialize` recurses, so a deep term on the event-hash path is a liveness hazard of the
kind **CBR-023** is about; and the double traversal is pure waste.

**Why the contract is stated at the trait rather than per call site.** `StableHashSerialize`'s **default
body IS `bincode::serialize`**, so every implementor is byte-identical *by definition* unless it
overrides, and the contract states that **an override may ONLY be a byte-identical faster
construction**. Only `Par` overrides. **DERIVED** (read at
`rspace++/src/rspace/hashing/stable_hash_provider.rs`).

**★ Why round-trip is explicitly not the property.** *"a codec that encodes differently but decodes its
own output round-trips and forks. The derived `Serialize` STAYS COMPILED as the encode oracle … it is
undriftable."* **CITED**.

**What is deliberately not touched.** The datum / pattern / continuation legs keep the intern-aware
spliced emitter (`spliced_event_bytes`), which reuses cached bytes at filled-cell `EPathMap` nodes.
*"That is a different and complementary optimization, and the ruling was not to rewrite it."* **CITED**.

**Authority.** No owner ruling.

#### Evidence — the neutrality claim, in full

- **Differential byte identity** against the derived `Serialize` over: every `ExprInstance` arm, every
  `ConnectiveInstance` arm, the full variant × arity × awkward-combination cross product, every awkward
  ground literal (`+0.0`, `-0.0`, NaN payloads, `GBigRat`, `GFixedPoint`, multibyte UTF-8), **both**
  `EPathMap` serialize arms, deep terms past every old ceiling, and proptest — *"with an executed proof
  that the differential can go RED"*. **CITED** (`models/tests/wire_encode_differential.rs`).
- Consensus-visible goldens re-verified after the wiring: `epathmap_canonical_fixtures` 13/13 (the 13
  bincode + prost + event-hash goldens), `par_codec_differential` 13/13, `serializer_par_byte_goldens`
  7/7, `epathmap_spliced_event_bytes` 8/8, `reduce_spec` 128/128, `interpreter_spec` 5/5,
  `stack_depth_gate` 8/8. **CITED**.
- **Anti-vacuity executed, not asserted**: `the_encode_differential_can_go_red` perturbs two field
  emissions and one variant index and requires the verdict to REJECT, **naming the clause**, with a
  control passing before and after. **CITED**.
- ★ **Three defects the suite found**, each recorded where it was made — and each is a defect that a
  round-trip test could not see:
  1. prost does not interleave oneofs with plain fields; `TaggedContinuation` produced *a 95-byte
     encoding with its halves exchanged*, same length, same byte multiset.
  2. `&'static` slices with identical contents are **merged by the linker**: `EPATHMAP_PROGRAM` is
     byte-for-byte `ELIST_PROGRAM`, so a downcast keyed on the program's *address* reinterpreted an
     `EList` as an `EPathMap` — **SIGSEGV**. Replaced by `WireNode::wire_as_pathmap`.
  3. A global allocation counter counted other test threads (green at `--test-threads=1`, red in the
     full suite).
- **Performance**, because the first factoring was measured and **rejected**: `WireNode::wire_field(i)`
  interpreted by a hand-written driver ran at **0.594×** the derived `Serialize` on a
  production-weighted mix (1,773 datums instrumented from five interpreter suites). After four root
  fixes: **1.19–1.58× faster**. **MEASURED** (`903cefb3`). Heap: **923× less allocation churn**, by
  massif heap profile. **CITED** (`decda6dd`).
- ⚠ **A build-correctness defect found in passing, and it is a consensus hazard in its own right**:
  emitting any `cargo:rerun-if-changed` switches cargo to watching only those paths, so editing
  `build/wire_schema.rs` left a **stale table in `OUT_DIR` while the build reported success** — *"a
  silent, byte-visible divergence between the generator in the tree and the table in the binary."*
  **CITED** (`903cefb3`).

---

### CBR-019b

**A trampolined prost encoder — built, gated, and dormant.**

| | |
|---|---|
| Commit(s) | `7c74260d` (the four-output generator), `56fb1fd0` (the prost encoder) |
| Status | LANDED, **DORMANT** |
| Direction | NEUTRAL |
| Evidence grade | DORMANT |
| Files | `models/src/rust/rholang/prost_wire.rs`, `prost_encode.rs`, `schema_meta.rs`, `models/build/wire_schema.rs` |

#### (a) The issue

The protobuf encoder is $`\Theta(\mathrm{depth})`$ in native stack at 302 B/level (see **CBR-028**).
A trampolined replacement was generated from the same single schema walk that produces the bincode table.
**It has no caller.**

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A — dormant. |
| 2 · verdict | N/A |
| 3 · bytes (Lane B) | N/A — `rhoapi_wire.rs` is **byte-identical**, md5 `0296fc17f2ef33897e7fd2ca9b68c524`, unchanged. **CITED**. |
| 3 · bytes (Lane P) | NO — *"converting the encoder changes zero bytes and zero accepted inputs"*; the encoder is not called. |
| 4 · post-state hash | N/A |
| 5 · accepted programs | NO — prost places **no** limit on the write side, asserted by `models/tests/par_prost_depth_ceiling.rs` stage 2, *"and fails loudly if one appears"*. **CITED**. |
| 6 · metering | N/A |

**Dormancy is established mechanically, not by intention**: *"`prost_encode::` appears in no `src/` tree
of models / rholang / rspace++ / casper / node / comm / shared — verified mechanically."* **CITED**.

#### (c) Why the change was necessary or correct

It is listed here **because it will stop being dormant**. When it is wired, it becomes an entry with the
same absolute neutrality obligation as **CBR-019**, on the *other* lane. Recording it now means the
wiring commit has a register entry to extend rather than one to invent.

★ **The generator-level mutation proof is the transferable methodology** and deserves a reviewer's
attention: *"Patch the generator, rebuild, and REFUSE TO REPORT unless the emitted
`OUT_DIR/rhoapi_prost_wire.rs` differs from the control. Two near-misses in this campaign were mutations
that reported green **because they had not applied**. A byte-level mutation proves the JUDGE can reject;
only a generator-level one proves the ENCODER would have been caught."* **CITED**.

| Mutation | Change | Verdict | What it proves |
|---|---|---|---|
| M1 | `sort_by_key(min_tag)` removed (declaration order) | REJECTED | first difference at byte 925; **both are 1031 bytes** — a pure permutation. No length check, no round-trip, and no protobuf decoder anywhere can see it. |
| M2 | sort key becomes `(is_oneof, min_tag)` | REJECTED | difference at byte 0; **both are 1140 bytes**, same byte multiset, halves exchanged. |
| M3 | skip-if-default guard → `if true` for `bool` | REJECTED | lengths 18 vs 14, 11 vs 9, 17 vs 11, 8 vs 6 across the corpus. |

**A named residual, disclosed.** `EPathMap` is an **opaque leaf** to any prost driver: its `encode_raw`
has three arms and which fires depends on a `OnceLock` another thread may fill. Both passes intercept it
at exact parity with `prost::encoding::message::encode`. *"CORRECT and NOT DEPTH-INDEPENDENT are two
separate statements."* **CITED**.

**Authority.** No owner ruling. No version bump.

#### Evidence

- $`\Theta(d^2) \to \Theta(n)`$ is checked **structurally, never by timing** (*"a timing assertion in a
  suite is a flake"*): the length table must grow linearly across $`d \in \{4,8,16,32\}`$. The same
  test pins $`\Theta(\mathrm{depth})`$ ops beside a $`\Theta(n)`$ table on one value: 4,096 siblings
  put fewer than 16 entries on either op stack while the table holds at least 4,096. **CITED**.
- `wire_schema_conformance.rs`, `serializer_par_byte_goldens.rs`, `wire_encode_differential.rs`,
  `par_codec_differential.rs` and `wire_encode_space.rs` are green **unmodified** (git reports no change
  to any of them). **CITED**.

---

### CBR-020

**The cold-store read path becomes fallible and heap-bounded.**

| | |
|---|---|
| Commit(s) | `9a5521a2` (the $`O(1)`$-native-stack decoder), `2bcfaf87` (the read path), `000b95d7` (teardown early-out) |
| Status | LANDED |
| Direction | PERMISSIVE |
| Evidence grade | WITNESSED |
| Files | `models/src/rust/rholang/par_codec.rs`, `rspace++/src/rspace/serializers/cold_store_decode.rs`, `serializers.rs`, `rspace++/src/rspace/errors.rs`, `history/*`, `merger/state_change.rs`, `casper/src/rust/merging/deploy_chain_index.rs` |

#### (a) The issue

The four `decode_*` functions in `serializers.rs` used bincode's **unbounded recursive descent** and
`.expect(..)` on failure. Two consequences: a too-deep record aborts the **process**, and there is **no
failure channel at all** for "the state we synced is not decodable".

★ The severity comes from *where* it sits: `rspace_importer` writes cold-store bytes to LMDB **without
ever deep-decoding them**, so a too-deep datum enters storage through a path that structurally cannot
observe its depth, then aborts the node **on every read-back, on every restart, on every peer that
synced the same state.** *"It is also the only member whose failure is PERMANENT AND REPLICATED. Every
other member is a transient worker fault."* **CITED**.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | NO — the decoder's obligation is **language identity** with the derived one. |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | NO — *"THE ENCODER IS NOT TOUCHED. `Serialize` stays derived … so byte identity is preserved BY CONSTRUCTION."* **CITED**. |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | NO |
| 5 · accepted programs | **MOVES** — a byte string that previously aborted the process is now read back (or refused with an `Err`). |
| 6 · metering | NO |

★ **The consensus obligation is stated exactly**, and it is the right one: *"The decoder's obligation is
LANGUAGE IDENTITY: for every byte string `b`, `cold_decode` and `bincode::deserialize` agree — same `Ok`
value, or both `Err`. **The `Err` half is the consensus-visible one: a node that accepts a byte string
another rejects FORKS.**"* **CITED**.

**The disagreement.** An old node aborts on a depth-4,096 datum in its own cold store; a new node reads
it. Since abort thresholds depend on stack size, the old behaviour was already **non-uniform across
nodes** — this repair removes a liveness split rather than creating one.

**Blast radius.** Every read of the cold store, i.e. every restart and every state sync. Reachable:
**yes** — *"That byte string is one `rspace_importer` could commit today."* **CITED**.

**Could live chain state have been produced under the old behaviour?** ★ **Yes, trivially and by
design** — the cold store *is* live chain state. The question that matters is the inverse: whether any
node has cold-store bytes it cannot read. Settling query: run `decode_datums` over every LMDB record on a
synced node and count `Err`s. This is **executable today** and is the cheapest high-value check in this
report. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A node that cannot start. Not a failed deploy, not a slashed
block — a process that aborts on every restart because a peer committed a legal byte string.

**Why a new error variant rather than reusing `ActionError`.** `HistoryError::DecodeError` is deliberately
distinct: *"The cold-store decode path is the one place the node reads bytes it did not necessarily
write … so 'the state we synced is not decodable' is a distinct condition from 'the action we just
performed was invalid', with a distinct operator response."* **CITED**.

**Why no blanket impl — a language limitation, not taste.** Rust has no specialization, so
`impl<T: DeserializeOwned> ColdStoreDecode for T` would catch `Par` (whose derive is retained as a test
oracle), make the machine impl a coherence error, and *"leave the recursive path in production while
every call site LOOKED converted."* The cost is four one-line delegations; the benefit is that "decoded
by the machine" is a **greppable list** rather than an inference outcome. **CITED**.

⚠ **The subtlest correctness obligation, disclosed.** `decode_datum` is no longer a whole-struct read:
`Datum<A>` is `{ a: A, persist: bool, source: Produce }` and `A` is a **prefix** — bincode is positional
and not self-delimiting, so the only way to know where `persist` starts is to have parsed `a`. The
machine reports that extent and the bounded tail decodes as a bincode tuple. *"The proviso is that the
extent be EXACT: off by one and `persist` is read out of the last byte of `a`, producing a **WRONG VALUE
rather than an error**. Hence the oracles."* **CITED**.

⚠ **A one-sided narrowing, disclosed with its argument.** `legacy_prefix` carries a byte limit, and it is
required rather than tidy: `IoReader::fill_buffer` resizes **before** reading while `SliceReader`
bounds-checks first, so a `String` prefixed with `[0xFF; 8]` would be a clean `Err` on the derived path
and an **OOM abort** here. The limit trips `ErrorKind::SizeLimit` before any allocation and *"cannot
reject anything the derived path accepts, since every charge equals the bytes consumed and the budget
starts at `bytes.len()`."* **CITED**.

**Authority.** No owner ruling.

#### Evidence

- Differential against the **retained derived oracle** (compiler-generated, so it cannot drift) over an
  exhaustive corpus: one representative of all 36 `ExprInstance` and all 9 `ConnectiveInstance` arms,
  every `Par` field including `conditionals` and `unforgeables`, both `EPathMap` serialize arms, every
  root type, plus `generate_par`. **CITED**.
- **1.88 M malformed inputs, all agreeing**: 117,601 truncations at every byte offset; 472,568 byte
  substitutions; 586,455 out-of-range variant indices; 701,898 hostile `u64` lengths (*"the family that
  would OOM-abort one side without serde's `size_hint::cautious` cap, reproduced verbatim"*); trailing
  bytes **accepted by both**, *"because tightening that narrows the language"*. **CITED**.
- A **falsification experiment** confirmed the differential fails on a single-field drift (an `ETuple`
  reading a `remainder` it does not have) — *"and that the three `generate_par` proptests stayed GREEN
  through it, which is precisely why the constructed corpus exists."* **CITED**.
- Depth 4,096 decoded on a **256 KiB stack**, and a *truncated* depth-4,096 term **rejected** on a
  256 KiB stack — the error path proved too. The derived path would have needed ~110 MiB. **CITED**.
- `models/tests/cold_store_records.rs`: the composition claim on the **production** instantiation
  `RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>`, with **26,793 record truncations**
  all agreeing. **CITED**.
- ⚠ A trap recorded for future readers: the serde oneof numbering is **declaration order, not proto
  tag** — `EPathmapBody` is proto tag 32 and serde index 25, so *"reading tags as indices mis-decodes 12
  of 36 arms."* **CITED**.

---

### CBR-021

**A malformed consume refuses instead of killing the process.**

| | |
|---|---|
| Commit(s) | `b961d7c4` |
| Status | LANDED |
| Direction | PERMISSIVE |
| Evidence grade | LATENT |
| Files | `rspace++/src/rspace/rspace.rs`, `rspace++/src/rspace/replay_rspace.rs` |

#### (a) The issue

Five `panic!`s in three functions answered an arity-or-emptiness fault **in their own arguments** by
spending the process, although `ISpace::consume` and `ISpace::install` have **always** returned
`Result<_, RSpaceError>`.

★ **The fourth site had already agreed.** `ReplayRSpace::locked_install_internal` was expected to be a
fourth panic and was not: it already returned `RSpaceError::BugFoundError` with this exact text. So
`RSpace::install` was **killing the node where `ReplayRSpace::install` refused** — *"A play/replay
asymmetry, in the two halves of the pair whose whole job is to agree."* **Converting play REMOVES an
existing divergence rather than creating one.** **CITED**.

| # | function | before | after |
|---|---|---|---|
| 1 | `RSpace::consume` | `panic!` ×2 | `Err` |
| 2 | `RSpace::locked_install_internal` | `panic!` | `Err` |
| 3 | `ReplayRSpace::consume` | `panic!` ×2 | `Err` |
| 4 | `ReplayRSpace::locked_install_internal` | **already `Err`** | `Err` |

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | NO |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | NO |
| 5 · accepted programs | **MOVES** — *"This turns 'every node dies' into 'this call fails' — a validator that today aborts would instead reject."* **CITED**. |
| 6 · metering | NO — both fire **before any state is touched and before any charge that differs between the two runs**, so `replay_cost_mismatch` cannot fire either. **CITED**. |

**The disagreement.** Mixed-version, an old node aborts and a new node returns a failed deploy. The
failure chain was traced hop by hop and is worth reproducing because it shows *why all four had to move
together*:

```text
RSpace::consume -> Err(RSpaceError::BugFoundError)
  -> reduce.rs:1026 `?` -> InterpreterError::RSpaceError   (errors.rs:431)
  -> interpreter.rs:172 handle_error -> catch-all arm
  -> EvaluateResult { errors: vec![e] }                    (interpreter.rs:247)
  -> casper/rholang/runtime.rs:662  is_failed = true
  -> replay: replay_runtime.rs:443 compares is_failed;
     DISAGREEMENT -> ReplayFailure::ReplayStatusMismatch
     -> InvalidBlock::InvalidTransaction -> is_slashable() == TRUE
```

*"So a refusal is a FAILED DEPLOY, not an invalid block — **PROVIDED play and replay agree**. They now do
by construction: identical guards, identical variant, identical deterministic text … Had they disagreed
it would have been slashable, which is exactly why all of them had to move together."* **CITED**.

**Blast radius — LATENT, and the enumeration is exhaustive.** Every in-tree caller was tabulated and
**none can express the fault**:

| caller | builds the two vectors as | can express it? |
|---|---|---|
| `reduce.rs::consume_inner` (**every deploy**) | `binds.unzip()` | NO |
| `eval_receive` → `Receive.binds` | normalizer rejects an empty receipt list | NO |
| `rho_runtime::introduce_system_process` | `vec![name]` / `vec![pattern]` | NO |
| `casper::consume_system_result` | `vec![channel]` / `vec![pattern]` | NO |
| `RSpace::restore_installs` | entries it recorded itself | NO |
| ★ the **FFI** (`consume`/`install`/`replay_consume`, and rholang's `consume_result`) | two independent repeated fields | **YES** |

*"No deploy can produce either fault, so no existing block's `is_failed` changes and the chain above is
not walked from consensus today. The guard is nevertheless live: the FFI is the open door, and it is
measured."* **CITED**.

**Could live chain state have been produced under the old behaviour?** **No** — derived from the
enumeration above: no deploy can reach either fault. **DERIVED** (the enumeration is the commit's;
this register reproduces it).

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A play/replay asymmetry sits in the pair whose whole job is to
agree, masked only because both in-tree callers turn the `Err` straight back into a panic. That mask is
one refactor from lifting.

**Why the variant and text are taken verbatim from row 4.** *"so all four now agree literally, not merely
in spirit."* **CITED**. Literal agreement is the property replay compares.

**Authority.** No owner ruling — but ⚠ the commit states a **hard prerequisite** more strongly than any
other in this register:

> *"⚠ CONSENSUS VERSION. This turns 'every node dies' into 'this call fails' — a validator that today
> aborts would instead reject. **IT REQUIRES A COORDINATED `Validate::version` BUMP.**
> `casper/src/rust/validate.rs:273-275` compares versions for EXACT equality with no activation-height
> machinery, so the bump cannot be made here: it is F1r3node's act. No version is bumped by this
> commit."* **CITED**.

#### Evidence

- The four-row table above is the evidence: row 4 was **measured** to be already `Err`, which is what
  turned the change from "introduce a divergence" into "remove one". **CITED**.

---

### CBR-022

**Deploy admission owns its discard — a 43,565-byte deploy stops aborting the node.**

| | |
|---|---|
| Commit(s) | `a09f1de2` (the term check that owns its discard), `3b265eb7` (both call sites) |
| Status | LANDED |
| Direction | PERMISSIVE |
| Evidence grade | WITNESSED |
| Files | `casper/src/rust/util/rholang/interpreter_util.rs`, `casper/src/rust/engine/multi_parent_casper/block_admission.rs` |

#### (a) The issue

`admit_deploy` and `admit_deploy_cosigned` validated a deploy by parsing its term and throwing the result
away — `Ok(_parsed_term)`, bound with a leading underscore, never read, released when the match arm ends.
`Par` is prost-generated, so that release is the derived
`drop_in_place::<Par>` ⇄ `drop_in_place::<ExprInstance>` cycle across the `EList.ps: Vec<Par>` edge:
**96 bytes of native stack per nesting level**, measured and confirmed against the disassembly (5 pushes,
no `sub rsp`, in each of the two frames).

★ **Where it sat is what made it urgent.** Every hop was read:
`DeployService/doDeploy` → `deploy_grpc_service_v1.rs:256` → `block_api.rs:477 deploy_cosigned`
(**synchronous, inline on the tokio worker, no `spawn_blocking`**) → `dispatch.rs:66` →
`block_admission.rs:105`. *"So it fired on **unauthenticated network input**, on the receiving node,
before the deploy was stored, before consensus, and with no `RuntimeBudget` in scope — cost accounting
could not bound it, not because the charge would be too small but because **no charge exists yet**. The
inbound cap is 16 MiB, **385× more headroom than the attack needed**, and the signature is trivially
generated with a fresh keypair."* **CITED**.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | NO |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | NO |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | NO |
| 5 · accepted programs | **MOVES** — a deploy at depth 21,782 previously killed the receiving node; now it is admitted or rejected on its merits. |
| 6 · metering | NO |

**The disagreement.** Measured: on a 2 MiB worker, **depth 21,781 exits 0 and 21,782 exits 134**
(SIGABRT). Nodes with different stack budgets therefore disagree about whether the deploy is processable
at all — a liveness split, available to anyone who can open a connection.

**Blast radius.** Any node accepting deploys. Reachable: **yes, by an unauthenticated peer.**

**Could live chain state have been produced under the old behaviour?** Not applicable in the usual sense:
the failure is a crash before storage, so it leaves no state — it leaves a **dead node**. The
observational question is whether any node crash in the historical record matches this signature.
Settling query: search node crash logs for SIGABRT with a stack-overflow signature during
`doDeploy`/`deploy_cosigned`. **UNVERIFIED** (and outside the repository).

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** An unauthenticated 43,565-byte message kills a node.

**★ Why this is provably byte-neutral, in three legs.** The register's confidence here rests on the fact
that the discarded `Par` is *never* part of anything signed or stored:

1. *"the signature is over the SOURCE"* — `Signed::create` / `Cosigned::from_signed_data` sign
   `data.to_message().encode_to_vec()`, and `DeployData::to_message`'s `term` field is the source
   string, **so the normalized `Par` is never in the signed payload**;
2. *"storage is the source"* — admission persists `Signed<DeployData>` plus the cosigner sidecar, and
   **no `Par` is written**;
3. *"the term is rebuilt later anyway"* — the proposer re-normalizes from source at `acceptance.rs:321`.

*"So this changes the order in which one discarded value's allocations are released, and nothing else."*
**CITED**. `acceptance.rs:321` also builds a `Par` from source but **consumes** it, and is deliberately
not touched.

**Authority.** No owner ruling.

#### Evidence

- `validate_deploy_term_releases_a_term_the_derived_destructor_could_not` releases a **depth-32,768**
  term on a **1 MiB** thread. The derived destructor needs ~3.0 MiB there in release and ~14 MiB in
  debug, so the tightest margin is **3×** and the asserted property is *a change of complexity class*.
  **CITED**.
- `validate_deploy_term_agrees_with_mk_term_on_both_arms` drives a 16-source corpus (8 accepted, 8
  rejected, **asserted**) and compares verdicts and rendered errors. **CITED**.
- ⚠ **A pre-existing defect found and deliberately NOT repaired here**, disclosed rather than buried:
  the comparison is **modulo word order**, because `Compiler::top_level_error` builds
  `TopLevelFreeVariablesNotAllowedError` by iterating a `HashMap`, so `mk_term` alone renders the
  variable list in different orders from one call to the next — **64 calls on one source produced 2
  distinct strings**. It is logged in the audit (§14.10.10, E102) *"with the open question of whether it
  can reach the persisted `ProcessedDeploy::system_deploy_error`"*. **CITED**. ★ If it can, that is an
  unregistered consensus fault of the **CBR-016** class; see [§6.3](#63-known-open-questions).

---

### CBR-023

**The $`\Theta(\mathrm{depth})`$ conversion programme — a liveness ceiling removed from seven traversal families.**

| | |
|---|---|
| Commit(s) | `f0894109` (audit + gate), `f11ffb54`, `b98fa20a`, `6714a128` (substitution), `2c32b173`, `6ce7c5b9` (the sorter and score tree), `07853de0`, `88ef41cd` (the normalizer), `6675fc06`, `739368a4` (the printer), `a3fd6fe4` (the guard evaluator), `d2591fa1`, `9082d12c`, `6c87b3f9`, `94dc983f`, `64a5d2bc` (de-cloning) — **sixteen commits** |
| Status | LANDED |
| Direction | PERMISSIVE |
| Evidence grade | WITNESSED |
| Files | `rholang/src/rust/interpreter/{substitute*,pretty_printer*,compiler/normalize*,reduce.rs,matcher/fold_match.rs}`, `models/src/rust/rholang/sorter/*` |

#### (a) The issue

Six families of traversal over `Par` recursed on the **native stack**, so a sufficiently deep term
aborted the process. Three properties make this a consensus concern rather than a performance one:

1. The normalizer *"fires in `inj_attempt`'s FIRST phase (`build-normalized-term`), before
   `set-initial-cost` establishes a budget — so cost accounting could not bound it, not because the
   charge was too small but **because no charge existed yet**."*
2. *"a stack overflow is a SIGSEGV on the guard page, not an `Err`, so the call site's
   `Err(e) => handle_error(ParserError(..))` arm — which exists to turn a bad deploy into a *failed
   deploy* — **never ran, and the node died instead**."*
3. *"`ReplayRuntimeOps::run_user_deploy → evaluate → inj_attempt` puts it on the **VALIDATOR path**, on
   source that arrived from the network, requiring no privilege and no stake."* **CITED** (`07853de0`).

The headline reproducer: **577 bytes** — `[` ×288, `0`, `]` ×288 — *"no `new`, no send, no user-defined
process, one nested list literal, fits in a single TCP segment"*, aborting a release node.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | NO — every conversion is gated against a recursive oracle twin. |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | NO — byte-identity differentials, per family. |
| 3 · bytes (Lane P) | NO — the normalizer differential compares `encode_to_vec()` **bytes**, the final `FreeMap`, and the final `BoundMapChain`. |
| 4 · post-state hash | NO |
| 5 · accepted programs | **MOVES** — a deploy that killed the node is now processed. |
| 6 · metering | NO — *"Charge count, charge order and charge value are unchanged."* **CITED** (`6c87b3f9`). |

**The disagreement.** Old node dies; new node returns a normal result. Because the death threshold is a
function of the thread's stack size, the old behaviour was **already non-uniform between nodes**.

**Blast radius.** Every deploy. Reachable by an unauthenticated peer: **yes**.

**Could live chain state have been produced under the old behaviour?** The *converted* traversals produce
identical results, so no historical state is invalidated. The open question is the inverse — whether any
historical node crash was this. Settling query: as **CBR-022**. **UNVERIFIED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A 577-byte network message kills validators.

**★ The proof standard, which is what makes fifteen commits reviewable at all.** Each family is converted
against a `#[cfg(test)]` **recursive oracle twin** — the pre-conversion bodies verbatim, with only their
cross-calls renamed so the twin is a *closed* recursive system — plus a **differential** asserting
equality of the consensus-relevant observable. Two examples of the standard being met rather than
claimed:

- **The normalizer.** *"'Both accepted' is deliberately not the claim: free-variable numbering is
  consensus-visible and a mis-threaded driver **fails SILENTLY**, emitting a plausible term with wrong
  indices."* The differential compares bytes, the `FreeMap` and the `BoundMapChain`. It caught **two live
  defects while being written**: a `~P` dispatch passing `proc.span` where the recursive form passed
  `arg.span`, and `format!("{:?}")` on a `HashMap`-backed `FreeMap` comparing **iteration order**.
  **CITED**.
- **The sorter.** *"`cost_accounting/sig.rs` computes `sort_match(&par).term.encode_to_vec()` — the bytes
  that get signed — so **a one-element reordering is a fork**."* `sort_by` is kept, never
  `sort_unstable_by`, because the comparator returns `Equal` for distinct terms with equal scores (the
  sorter is a normalizer, hence not injective — **directly measured**) and an unstable sort would be free
  to reorder those. **CITED**.

**★ Three things deliberately NOT converted, each with its reason** — this is where a reviewer should
look for over-claiming, and the campaign declined in all three:

1. **`encoded_len`.** *"the ONE member that is NOT covered by [the cost-neutrality] argument, because its
   return value IS the charge"* — an off-by-one there *"is a consensus fork, not a performance
   regression."* `6c87b3f9` moves a **call**, not the callee. **CITED**.
2. **Nested set/map sorting.** Left as a *tripwire*, not a conversion target, because restructuring the
   rounds *"would change how many `HashSet`s are built and in what order, and `RandomState` is seeded
   from a per-thread counter, so **container construction order is consensus-observable** whenever two
   distinct elements share a score."* Deep set nesting is infeasible in **time** long before the stack
   residual bites: $`3^n`$ sorts; depth 20 ($`3.5\times10^9`$ sorts) did not terminate.
   **MEASURED**. **CITED**.
3. **`ScoredTerm::sort_vec`.** Sorting on `(score, sibling_index)` was considered and **rejected as an
   ordering change**: it is `pub` and reaches `SortedParHashSet`/`SortedParMap`, *"whose iteration order
   is consensus-observable."* **CITED**.

**★ And `Env::get`'s clone is documented as un-removable, in both places, because the copy IS the meaning
of substitution.** Measured 15,850 B/level — `Par::clone` to within 0.2%. It sits in the tripwire as
`substitute_deep_binding`, *"never in the converted list, so the residual is visible instead of folded
into a claim of full depth-independence."* **CITED**.

**Authority.** No owner ruling. `d2591fa1` records the sharpest consensus caveat of the group:
*"Re-typing `split` touches consensus: it decides each parallel branch's `Blake2b512Random`, which
determines every unforgeable name the branch mints and therefore the COMM events it can take part in.
**One changed split id is a fork.**"* — verified by `cargo nextest run --workspace`, 3439 passed, with
the reducer's own differential and `cost_should_be_repeatable_when_generated` inside that run as the
end-to-end guard on COMM/rand order. **CITED**.

#### Evidence

| Subject | Before | After | Source |
|---|---|---|---|
| `plain_deploy` end-to-end ceiling | 286 levels | **6,831 levels (23.9×)** | `64a5d2bc` |
| `subst_and_charge` | 2,852 B/level | 0 (the slope **was** the clone, to the byte) | `6c87b3f9` |
| `inj_attempt` metering handshake | depth 729 | **≥ 1,048,576** | `9082d12c` |
| deploy ingress teardown | 96 B/level | **0 B/level, no ceiling** | `a09f1de2`, `3b265eb7` |
| the 577-byte reproducer | aborts a release node at depth 288 | normalizes at 288, 1,152 **and 100,000** | `88ef41cd` |
| `env_get_deploy` | 283 levels | **UNMOVED at 283, exactly as predicted** | `64a5d2bc` |

- ★ **A gate that lied, and the lesson written into its own documentation.** While `substitute` still had
  a 437 B/level residual, its tripwire reported **0 B/level**: both probe points sat inside the subject's
  ~136 KiB **intercept**, where bisection at 4 KiB resolution cannot see 48 levels × 437 B. *"A large
  intercept reads as a zero slope on a short ladder — a green number for the wrong reason, which is the
  **third occurrence of that failure mode** in this work."* The bar became `assert_no_slope` over
  4 → 4,096. **MEASURED** (`b98fa20a`).
- ★ **The admission rule for the converted list is stated and enforced**: *"a traversal enters
  `CONVERTED_DEPTH` only by being CONVERTED, never by having a ceiling lowered."* **CITED** (`64a5d2bc`).
- ★ **A lowered ceiling shown RED at the value it exists to refuse**: reverting the subject body to a
  copying call failed with `2852 B/level exceeds the 700 B/level ceiling` — *"2,852 is precisely the
  pre-repair reading, so the ceiling refuses the exact regression it was lowered to refuse — not merely
  something."* **MEASURED**.
- ⚠ **A width-axis finding that is a real defect, not a measurement artifact**: `compare_score_nodes`
  recursed on the list **tail**. At `-O2` LLVM turns that into a loop and the measured slope is 0; at
  `-O0` it is 201 B per sibling. *"Relying on a codegen accident for a consensus-liveness property is not
  acceptable."* **MEASURED** (`6ce7c5b9`).

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
| 6 · metering | **MOVES** — a new charge site exists (it did not before). |

**The disagreement.** `[1,2,3].last()` fails on an old node, evaluates to `3` on a new one. Because the
old behaviour is a deterministic error, this is a slashable-fault class rather than a silent fork.

**Blast radius.** Programs calling `.last()`. Reachable: **yes**, but only by programs written after the
feature exists — no *existing* program can call it.

**Could live chain state have been produced under the old behaviour?** **No** — a method that does not
exist cannot have succeeded. **DERIVED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Nothing, in the safety sense. This is a capability addition, and
it is in the register because *acceptance is an axis*.

**★ The pricing decision, which is the part with consensus content.** `nth`'s price is **deliberately
reused**: *"`last` does exactly `nth`'s work, so it reserves `nth_method_call_cost()`. **Minting a
`last_method_call_cost` would be inventing a price, and pricing is a consensus decision this change does
not make.** The cost `operation` label is surfaced in eval results and out-of-phlogiston errors, so
reusing the constant also keeps `last` from ever drifting away from `nth`'s price."* **CITED**.

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
| 6 · metering | **MOVES** — three new charge sites. |

★ *"THE CONSENSUS SURFACE CHANGE IS ADDITIVE ONLY — verifiable without reading the diff twice."*
**CITED**. `getPath` only **reads out** a field already on the message and already serialized
(`RhoTypes.proto:368`, `repeated bytes current_path`), decoding it with the same `decode_trie_path` codec
the file already uses.

**Blast radius.** Programs calling the three new methods. **Could live chain state have been produced
under the old behaviour?** **No.** **DERIVED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A trie you cannot enumerate is a trie you cannot use as one.

**Why nothing new was built.** All three surface capability the `pathmap` crate already had
(`ZipperMoving::path()`, `ZipperIteration::to_next_val()`, `ZipperMoving::val_count()`). The only
reachability change in `models` is that one previously-dead decoder (`unflatten_segments`, carrying
`#[allow(dead_code)]`) becomes live — it is the inverse of `flatten_segments`. **CITED**.

**★ The metering derivation, because it is the one place a mistake would have been a fault.**
*"Metering reuses existing cost primitives only — no new metering surface (budgets are f1r3node's).
`leafCount` is charged in two parts: the flat `union_cost(1)` base every sibling zipper method takes,
plus a **PROPORTIONAL** `size_method_cost(count)` because the query is $`O(\mathrm{subtrie})`$. The
proportional part **must** use `reserve_incremental_primitive`: `leafCount()` on a missing prefix is 0,
and `reserve_primitive` rejects a non-positive charge with `BugFoundError` — **measured, not assumed.**"*
**CITED**.

**Authority.** No owner ruling.

#### Evidence

- `subtrie_value_count` also replaces the `collect_subtrie_values(..).len()` shape, *"which cloned every
  `Par` in a subtrie only to discard them."* **CITED**.

---

### CBR-026

**`E(S)` — the enabled-rendezvous query, and firing a NAMED selection.**

| | |
|---|---|
| Commit(s) | `2087c043` |
| Status | LANDED, **dormant on the consensus path** |
| Direction | PERMISSIVE |
| Evidence grade | DORMANT |
| Files | `rspace++/src/rspace/space_matcher.rs`, `internal.rs`, `rspace.rs`, `replay_rspace.rs` |

#### (a) The issue

`consume` and `produce` answer *"is there an admissible selection, and if so what is the least one?"*.
A speculative evaluator must answer *"what are **all** the rendezvous this state admits?"* and then fire
a member it **names**, rather than whichever one a fresh search rediscovers. Two trait methods are added:
`enumerate_admissible_selections` and `enumerate_enabled_rendezvous`.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A — additive API; no existing path is rerouted through it. |
| 2 · verdict | N/A — but see the *determinism* obligation below. |
| 3 · bytes (Lane B) | N/A |
| 3 · bytes (Lane P) | N/A |
| 4 · post-state hash | N/A |
| 5 · accepted programs | **MOVES** — the capability exists where it did not; nothing in the deploy path calls it yet. |
| 6 · metering | N/A |

**Blast radius.** None today. It is in the register because **it will acquire one**: the moment a caller
appears, `fire_named_selection` becomes a way to reach a *non-least* selection, and the determinism
argument below becomes load-bearing.

**Could live chain state have been produced under the old behaviour?** N/A. **DERIVED**.

#### (c) Why the change was necessary or correct

**★ Why a sibling of the production selector rather than a mode of it.** *"that function short-circuits
on `Admissible` at EVERY level, so 'keep going' is not a flag that can be threaded through, and **giving
the consensus-critical selector a mode it never uses in production is the wrong trade.**"* **CITED**.
The enumerator is the production selector's depth-first descent with the early return replaced by a
record — same pool snapshot/restore, same residual construction, same single `check_commit` at the leaf
— which is why **`out[0]` IS the lexicographically least admissible selection**, the one a real `consume`
takes. That identity is the bridge between speculative enumeration and ordinary execution, and it is
asserted (t1).

**★ Why `enumerate_enabled_rendezvous` is on the TRAIT.** So `RSpace::enabled_rendezvous` and
`ReplayRSpace::enabled_rendezvous` are three lines each. *"That is the **CBR-001** lesson applied before
it can bite: play and replay run ONE enumeration, so they cannot drift, and t6 measures it rather than
assuming it."* **CITED**.

**★ Determinism is a consensus surface here, and three orderings compose.** Channel groups ascending by
`Vec<C>` — *"the group set is read out of a `HashMap`, whose iteration order is seed-dependent — without
the sort **two validators would agree on the SET and disagree on the SEQUENCE**"* — then continuations
within a group by `order_candidates_with_index`, then selections by the descent. **CITED**.

**Authority.** No owner ruling.

#### Evidence

- `rspace++/tests/enabled_rendezvous_spec.rs`, 9 tests: head identity against the production selector
  (t1), completeness with and without a guard (t2), read-only in store / event log / produce counter
  (t3), determinism under permuted insertion order over 10 repetitions (t4), **a named NON-least
  selection firing exactly itself** (t5), play/replay agreement (t6), four negative shapes (t7), a join's
  cross product with an assignment-sensitive guard (t8). **CITED**.
- `cargo test -p rspace_plus_plus` green end to end: 319 tests, 0 failures. **CITED**.

---

### CBR-027

**GInt `+` and `-` stop wrapping on overflow.**

| | |
|---|---|
| Commit(s) | **IN FLIGHT** — designed and ruled; **not present in the tree at the time of writing.** |
| Status | IN FLIGHT (describes *intended* behaviour) |
| Direction | REGRESSIVE |
| Evidence grade | WITNESSED |
| Files | `rholang/src/rust/interpreter/reduce.rs` — working tree lines **3503** (`wrapping_add`) and **3597** (`wrapping_sub`); at commit `8853f839` the same expressions are at **3397** and **3489**. **MEASURED** (`grep -n`, both revisions). |

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
| 6 · metering | NO — the charge (`sum_cost()` / `subtraction_cost()`) is reserved before the arithmetic, unchanged. **DERIVED**. |

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

- The inconsistency is **DERIVED** and re-verified for this report: `wrapping_add` at working-tree
  `reduce.rs:3503`, `wrapping_sub` at `:3597`, against checked division-by-zero and
  $`\mathrm{i64::MIN}/-1`$ guards in the same `match` family.
- ⚠ **What would change if the implementation diverges from this design**: if the error is raised
  *before* the cost reservation rather than after, the metering cell becomes **MOVES**; if the error
  message includes the operands (as the ruling's option text says it should), that message becomes a
  candidate for the block-resident `error_message` path and the entry acquires **CBR-016**'s
  determinism obligation — the message must be a pure function of the term. **This must be re-checked
  when the commit lands.**

---

### CBR-028

**OPEN, UNREPAIRED — a term can be built, reduced, serialised and committed, and not read back.**

| | |
|---|---|
| Commit(s) | **NOT REPAIRED.** Characterised by `80f5e5d3`, `d8081107`, `935704d6`, `9994a75b`, `f0894109`; the four consensus-class `Par::decode` call sites are consolidated into `dispatch::decode_non_deterministic_output` so that *"when the read ceiling is ruled on, there is one edit."* **CITED**. |
| Status | **OPEN** |
| Direction | — (no change has been made) |
| Evidence grade | WITNESSED |

#### (a) The issue

prost's **decoder** caps recursion at 100 levels (`DecodeContext`); prost's **encoder** caps nothing.
Three consequences, each independently established:

1. **The bare `Par` ceiling.** Term depth 33 round-trips; depth 34 answers `DecodeError(recursion limit
   reached)`. **MEASURED, bisected** (`f0894109`).
2. **`EPathMap` tag 8** — *"the only one of the four on a consensus WIRE FORMAT"*. A ground map encodes
   its entries with `encode_trie_path`, documented **total and unlimited**, and decodes them with
   `decode_trie_path`, which is **bounded**. **MEASURED**: entry depth 33 round-trips; depth 34 encodes
   to **75 bytes** and answers `DepthLimitExceeded`. *"Its reachability is NOT established here and must
   not be assumed by analogy."* **CITED** (`80f5e5d3`).
3. **The escape arm.** `EntryTrie`'s escape arm stores a non-`eval_stable` `Par` as its canonical prost
   bytes, so *"past that depth a `Par` encodes to a key that will not decode, while the trie holding it
   is perfectly well-formed and its keys are perfectly canonical."* **CITED** (`935704d6`).

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A — nothing has changed. |
| 2 · verdict | N/A |
| 3 · bytes (Lane B) | N/A — ★ the bincode wire is **symmetric** (both directions unbounded) and is therefore **not** in this class. **CITED**. |
| 3 · bytes (Lane P) | **MOVES** in the sense that matters: a byte string one node writes, another rejects. |
| 4 · post-state hash | **MOVES** — via the replay path below. |
| 5 · accepted programs | **MOVES** — asymmetrically between the writer and the reader. |
| 6 · metering | N/A |

★ **The leg that makes it consensus-class, measured and reproduced** (`d8081107`):

```text
play    Produce::create sets output_value: vec![]; RSpace::locked_produce returns THAT
        Produce ⇒ reduce.rs:1065 iterates ∅ ⇒ no decode — and then reduce.rs WRITES
        the bytes into the block.
replay  ReplayRSpace::locked_produce returns comm.produces.find(hash == produce_ref.hash)
        — the Produce FROM THE TRACE, whose output_value came from the block ⇒
        reduce.rs:1065 decodes real bytes ⇒ Err(DecodeError) ⇒ eval_successful = false
        (replay_runtime.rs:427) ⇒ the is_failed mismatch at :443 ⇒ CasperError::ReplayFailure.
```

**⇒ The proposer builds a block no validator can replay.**

| | depth 33 | depth 34 |
|---|---|---|
| play (`is_replay = false`) | green | green |
| replay (`is_replay = true`) | green | **RED — `DecodeError(… recursion limit reached)`** |

**MEASURED**, `current_thread` runtime, outcome identical over **25 consecutive runs**; red-teamed by
disabling the splice, which fails the test. `is_replay` is *read* from the two spaces, not assumed.

Why the bytes get that far: `ProduceEventProto.outputValue` is `repeated bytes`
(`CasperMessage.proto:393`), **not** a nested message, so the block body carries them **opaquely** — the
block decodes fine and only replay does not. And because the eventual `Par::decode` is a *fresh
top-level* decode it gets prost's full budget, *"which puts the consensus-class member at the BARE
ceiling, 33/34, not the wrapped 32/33."* **CITED**.

**Blast radius, and the honest severity.** `80f5e5d3` was filed as *"a proposer-controlled consensus
divergence"* and that claim was **measured and downgraded**: the deploy path cannot reach depth 34 —
margin **31 levels** — so the class is **LATENT for the bare-`Par` member**. But three facts keep it
open: (i) the `EPathMap` tag-8 member's reachability is **not** established; (ii) *"the five FFI reads
(`rholang/src/lib.rs`, `rspace_rhotypes`) are in the class and **PANIC rather than `Err`**, which is
strictly worse"*; (iii) ★ **today's own repairs made it more reachable** — `substitute` and the sorter
are now unbounded, where `substitute` previously capped the reducer at 75. **CITED** (`9a5521a2`).

**Could live chain state have been produced under the old behaviour?** The relevant question is whether
any historical block carries an `output_value` that fails `Par::decode`. Settling query — and this one is
**directly executable against the block store**: for each block, for each `ProduceEventProto` in each
`ProcessedDeploy`'s deploy log, attempt `Par::decode` on every `output_value` element and count failures.
**UNVERIFIED**.

#### (c) Why no change was made — and what the options are

**This entry exists to prevent the hazard being lost, not to record a repair.** No fix is possible that
is not itself a consensus decision, and the campaign declined to make it:

- **Cap the write side** — explicitly ruled out by the owner (*"no new protocol-level nesting caps"*),
  and it would make a currently-valid term invalid.
- **Raise the read limit** — changes which byte strings are accepted, i.e. Axis 5, and requires a
  coordinated bump.
- **Make the read fallible where it currently panics** (the five FFI reads) — the smaller, strictly-safe
  half; see **CBR-021** for the same posture applied to `consume`.

★ **What the campaign did instead, and why it is the right preparatory step**: the consensus-class decode
was written out **four times** (twice in `reduce.rs`, twice in `contract_call.rs`) and is now one
function, `dispatch::decode_non_deterministic_output`, next to the `dispatch_type` encoder that is its
inverse. The compiler confirmed the swap was **total rather than additive**: `prost::Message` became an
unused import in `contract_call.rs`. *"When the read ceiling is ruled on, there is one edit."* **CITED**.
That commit moved **no bytes, no hash, no validity predicate**, capped no write side, added no feature
gate and bumped no version.

**Authority.** The owner has ruled out write-side caps. The read-ceiling disposition is **awaiting a
ruling**.

#### Evidence

- The four call sites are documented in place: `reduce.rs:1065` (consensus-class), `reduce.rs:1195` (the
  consume twin — one caller, `consume_inner`, which passes a literal `Vec::new()`, so it never runs),
  `contract_call.rs:90` (the system-contract copy), `contract_call.rs:110` (the return leg). **CITED**.
- ⚠ **Anti-vacuity on the negative result**, which is what makes the "five clean cells" of `0e0f9719`'s
  reachability probe a measurement rather than a false zero: splicing this depth-34 payload through the
  **identical harness** turns the replay red with `recursion limit reached`. *"The harness is provably
  delivering the bytes."* **CITED**.

---

## 4.4 Surface L — MeTTaIL's Rholang

⚠ **Read this section against a different clock.** MeTTaIL's Rholang does not run consensus today. A
divergence here is a *future* fork — and the standard the campaign works to is that MeTTaIL's Rholang
must be a **superset** of upstream Rholang: it must accept everything upstream accepts and compute the
same value, with divergence permitted only where upstream has a bug, and only by explicit ruling. The
entries below are therefore classified against the *same* six axes, but the "disagreement" they describe
is between **the two implementations**, not between two nodes.

★ Two of these entries reach F1r3node's RSpace **today**: **CBR-L05** and **CBR-L06** publish into a
live deploy's tuplespace via `Publisher::publish` → `produce`, so their bytes are part of the
post-deploy state and therefore of the checkpoint root.

---

### CBR-L01

**Equal operator precedence becomes representable, and Rholang's ladder is corrected against the normative grammar.**

| | |
|---|---|
| Commit(s) | `3ff1c98b` (the `same` marker), `f586e138` (the corrections), `ce887d0b` (eight documents that asserted the defect as intended) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `macros/` binding-power assignment; `languages/src/*.rs` for all five bundled languages |

#### (a) The issue

`analyze_binding_powers` advanced its precedence counter in **both** associativity arms, so no branch
left it unchanged. Rule $`i`$ of a category received $`\mathrm{left\_bp} \in \{2+2i,\; 3+2i\}`$, and
two rules $`i < j`$ could share a `left_bp` only if

```math
3 + 2i \;=\; 2 + 2j
\qquad\Longleftrightarrow\qquad
2\,(j - i) \;=\; 1
```

which **no integers satisfy**. Every non-postfix operator in a category was therefore *provably* distinct
in precedence, and declaration order was a strict **total** order by construction. The grammar had no way
to say that `*` and `/` bind equally tightly, so `6 * 3 / 2` could only parse as `6 * (3 / 2) = 6`.

★ *"That is why the gap went unnoticed for so long: the assigner could not **emit** a table exhibiting the
missing relation, so no test ever saw one, and the two lints written to detect the shape tested a
fiction instead."* **CITED**.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — `6 * 3 / 2` was `6` and is `9`. |
| 2 · verdict | **MOVES** — a differently-parsed term matches differently. |
| 3 · bytes (Lane B) | **MOVES** — a different parse is a different term is different bytes. |
| 3 · bytes (Lane P) | **MOVES** — same. |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | NO — the same source text parses; it parses to something else. |
| 6 · metering | NO |

**The disagreement.** Between MeTTaIL's Rholang and upstream Rholang, on **arithmetic**. The corrected
ladder is checked against the normative `rholang-rs/rholang-tree-sitter/grammar.js` and now matches it
exactly. Eleven operators moved level; `matches` additionally became **right**-associative
(`prec.right(6, …)`), and `==` / `!=` joined it at level 6.

**Blast radius.** Every program mixing `*` `/` `%`, or `+` `-`, or mixing comparison with equality
without explicit parentheses. Reachable: **yes, in any arithmetic expression.**

**Could live chain state have been produced under the old behaviour?** N/A on Surface N — MeTTaIL does
not run consensus. ★ On Surface L the analogous question is whether any *shipped MeTTaIL artefact*
(demo, run sheet, test golden) encodes the old parse; the campaign surveyed and found **zero demos doing
arithmetic the precedence change would move**. **CITED** (campaign ledger, 2026-07-28).

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** `6 * 3 / 2` evaluates to `6`. *"`a + b - c` has the same shape but
is accidentally harmless (`a + (b - c) == (a + b) - c` over a ring), which is exactly why nobody noticed.
Division isn't so forgiving."* **CITED**.

**Why a `same` marker rather than absolute `@prec(n)`.** *"An absolute `@prec(n)` was rejected again: it
admits partial specifications, forces renumbering churn, and lets two rules disagree about which level a
number denotes. **`same` has no operand to get wrong.**"* Declaration order still supplies the **order**;
`same` supplies only the **ties**, giving a *total preorder* — a sequence of levels, each an unordered
set — *"which is exactly what Pratt binding powers encode."* **CITED**.

**Authority.** No owner ruling on the fix itself; the owner ruled the general standard
(*"upstream behavior should be duplicated in MeTTaIL's Rholang semantics, with MeTTaIL's Rholang being a
superset of upstream Rholang"*), and the tree-sitter grammar is the normative authority the fix is
checked against.

#### Evidence

- The full before/after ladder is tabulated in `f586e138` — 18 rows, each with its level.
- 30 precedence guards, **each RED before the fix and each with an anti-vacuity argument**. **CITED**
  (`2c9e3cc1`).
- ⚠ **A disclosed documentation failure**: *"eight documents asserted the defect as intended behaviour."*
  **CITED** (`ce887d0b`). A wrong specification is worse than no specification, because it stops the
  defect being re-derived.

---

### CBR-L02

**The substrate lane stops answering "false" for a guard it could not decide.**

| | |
|---|---|
| Commit(s) | `0f3d298c` |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `rholang-runtime/src/guard_par_substrate.rs` |

#### (a) The issue

The Surface-L twin of **CBR-002**, at a different layer. `guard_par_substrate.rs:754` mapped
`Sat3::DontKnow` through `dont_know_policy(ReceiveWhere) = FailClosedBlock` to **`false`** — with no
signal. `false` is what a guard that *was* evaluated and *was refuted* returns.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A |
| 2 · verdict | **MOVES** — a guard that could not be decided now refuses **loudly** instead of silently blocking. |
| 3 · bytes (Lane B) | NO |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | **MOVES** — a refused deploy leaves no state where a silently-blocked one left resting data. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

★ **The measurement, on the real runtime** — one datum (`5` on `@"c"`), one guarded receive, the guard
varied. **MEASURED** (`0f3d298c`):

| guard | fired | resting | error |
|---|---|---|---|
| `x > 0` | `[5]` | `[]` | — |
| `x > 100` | `[]` | `[5]` | — |
| `x + 1` | `[]` | `[5]` | — ← **SILENT** |
| `[1, 2]` | `[]` | `[5]` | — ← **SILENT** |
| `x` | `[]` | `[5]` | — ← **SILENT** |
| `"nope"` | `[]` | `[5]` | — ← **SILENT** |
| `6 / (x - 5) > 1` | `[]` | `[5]` | — ← **SILENT** |
| `x.toByteArray() == x.toByteArray()` | `[]` | `[]` | the **machine** lane refuses |

The last row is the control: **CBR-002**'s gate is live in this tree and pre-empts exactly the subset
`rho_pure_eval` calls `UnsupportedExpression`. *"Every row above it is the substrate's OWN `DontKnow`,
and every one of them was **indistinguishable from `where x > 100`**."*

**Blast radius.** Every MeTTaIL RSpace — `run`, `step`, `speculation`, `bench_support`. Reachable:
**yes**.

**Could live chain state have been produced under the old behaviour?** N/A — Surface L.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** The same defect class as **CBR-002**, at a lower layer, and
therefore *not* fixed by **CBR-002**: an abstention indistinguishable from a verdict. This campaign
recorded it as its single most repeated finding — **a disposition is a value, not an absence.**

**Authority.** No owner ruling; it follows the **CBR-002** disposition.

---

### CBR-L03

**A residual binder rests the COMM, whatever the guard formula collapsed to.**

| | |
|---|---|
| Commit(s) | `69c66cd1` |
| Status | LANDED |
| Direction | REGRESSIVE |
| Evidence grade | WITNESSED |
| Files | `rholang-runtime/src/` guard substrate |

#### (a) The issue

The surface lane **fired** a COMM whenever short-circuit evaluation made the guard formula constant-true,
**even when the guard still mentioned a binder the arrived payload never supplied**. The reducer, on the
same guard, produces an internal `Err` and the process **rests**. ★ *"Firing where the reducer rests is
unsoundness in the FIRING direction — the one direction a fail-closed policy cannot excuse."* **CITED**.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | N/A |
| 2 · verdict | **MOVES** — the entry. Four measured rows below. |
| 3 · bytes (Lane B) | NO |
| 3 · bytes (Lane P) | NO |
| 4 · post-state hash | **MOVES** — a COMM that no longer fires leaves the datum resting. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

**MEASURED RED, then closed** (`69c66cd1`):

| guard | was | now | reducer |
|---|---|---|---|
| `true or (y == 2)` | **Fires** | Declines | rests |
| `(1 == 1) or (y == 2)` | **Fires** | Declines | rests |
| `false implies (y == 2)` | **Fires** | Declines | rests |
| `false and (y == 2)` | Blocks | Declines | rests |

**Blast radius.** Guarded receives whose guard mentions a binder the payload does not supply, in a
short-circuiting position. Reachable: **yes**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** The surface lane and the reducer disagree, in the direction that
*fires*. If MeTTaIL becomes the node's language, the surface lane's answer would consume data the
reducer would have left resting.

**Why DECLINE and not an error.** *"The reducer raises nothing the author sees, so the surface lane
refuses — loudly, carrying `GuardRefusalCause::ResidualBinder` — rather than raising `UndecidableGuard`,
which would be a **THIRD behaviour**, turning a resting process into a failing one."* **CITED**. The
reducer is normative; the surface lane's job is to agree with it, not to improve on it.

**★ Why the check cannot live in the formula.** `GuardFormula::or` collapses `(True, _)` at
**construction** time, so `true or (y == 2)` has formula literally `True` with no residual to inspect.
The check must therefore be on the *term*, not the formula. **CITED**.

**Authority.** ★ Owner ruling, relayed **2026-07-28**: **"#113 reducer normative"** — *"surface lane
declines with `ResidualBinder`, GATE 2 to be **re-derived, not weakened**."* **CITED**.

---

### CBR-L04

**The `!?` query bind executes, and its lowering stops being hash-ordered.**

| | |
|---|---|
| Commit(s) | `ac7f71af` (the send leg), `6e6639ee` (the hash-ordered par) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `languages/src/rholang.rs`, `rholang-runtime/src/runtime.rs`, the WPDA lowering |

#### (a) The issue

Two defects, the second found only because the first was fixed.

**(i) The feature did nothing.** `for(p <- svc!?(a, b)){B}` is request/response sugar for
`new r in { svc!(*r, a, b) | for(p <- r){B} }`. The expansion was correct, tested, and **called** — by
`Proc::term_eq`, which builds a *comparison key*. The expanded program was computed, compared against,
and **dropped**; the program that RAN still carried the raw `InputBindQuery`, so the lowering read `svc`
as the receive channel, discarded `args`, and emitted **no request send at all**. The receive waited
forever — *"no error, no diagnostic, exit code 0."* **CITED**.

**(ii) The lowering was not a function of the term.** `desugar_for_rows` composed
`send₀ | send₁ | receive` into a `PPar`, which is a `HashBag` (`HashMap<Proc, usize, FxHasher>`), so it
serialized in **hash order**. Each send carries `*rₖ`, a return channel minted by `FreeVar::fresh_named`
from a **process-global counter**, so two lowerings of the same term mint different ids, hash
differently, and order their members differently.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — the feature computes where it computed nothing. |
| 2 · verdict | **MOVES** |
| 3 · bytes (Lane B) | **MOVES** — ★ and defect (ii) means the *old* bytes were **nondeterministic**. |
| 3 · bytes (Lane P) | **MOVES** — same. |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | NO — the source parsed before; it simply did nothing. |
| 6 · metering | NO |

**MEASURED, same source, before and after** (`ac7f71af`):

```text
before:  @"OUT" observations (1):  [0] ⟦11⟧            ← the control, alone
after:   @"OUT" observations (4):  [0] ⟦6⟧ [1] ⟦5⟧ [2] ⟦70⟧ [3] ⟦11⟧
```

And the nondeterminism, caught by the M-2 driver/oracle differential the moment `!?` rows existed
(`6e6639ee`):

```text
M-2 DIFFERENTIAL FAILED on `for(x <- @"a"!?(1) & y <- @"b"!?(2)) { @"OUT"!(*x) }`
  driver: [… 26, 1, 97 … 26, 1, 98 …]        ← service `a` first
  oracle: [… 26, 1, 98 … 26, 1, 97 …]        ← service `b` first
```

The two encodings differ by a **swap of the two request sends and in nothing else**.

**Blast radius.** Every program using `!?`. Reachable: **yes**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A declared language feature that silently does nothing, and — once
it does something — a lowering whose bytes depend on a **process-global counter** and therefore on
scheduling and on test order. ★ *"Under `cargo test` (one process, shared counter) the outcome depended
on how many variables earlier tests had minted, so it passed there and failed under nextest's per-test
process. **A gate whose verdict depends on the test schedule is not a gate.**"* **CITED**.

**Why the ids themselves never leaked.** `Scope::new` binds them, so they lower to de Bruijn indices.
*"That is exactly why only the ORDER moved, and why nothing caught it before — the expansion had never
run on the lowering path at all."* **CITED**.

**Authority.** No owner ruling.

---

### CBR-L05

**Published lookahead bytes stop carrying host-local order.**

| | |
|---|---|
| Commit(s) | `03ec33de` (the trace digest), `826bb96e` (across-channel), `11472763` (within-channel) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `rholang-runtime/src/lookahead.rs`, `speculation/` |

#### (a) The issue

Three instances of one class — **a local, host-assigned ordering promoted into published bytes**.

1. **`step_digest`** folded each selection's `continuation_index` and `datum_indices` alongside the
   content hashes. *"A store index is not content: `HotStore::put_datum` / `put_continuation` **PREPEND**,
   so a datum's position is 'how many data were staged after it on that channel', and arrival races by
   design — every branch of a `|` is a detached `tokio::spawn` whose own `DriveState` doc concedes 'Push
   order is non-deterministic.'"* **CITED**.
2. **`reify` across channels.** Its doc comment said `HotStoreState`'s maps are `BTreeMap`s. *"They are
   `HashMap`s"* (`rspace++/src/rspace/hot_store.rs:90-94`), whose iteration order is `RandomState`-seeded
   **per process**. *"The conclusion was right and the premise it rested on was not — the worse of the
   two failure modes, because a stated reason does not get re-checked."* **CITED**.
3. **`reify` within a channel.** `826bb96e` deliberately kept the within-channel order on the stated
   reason that *"two data on one channel are genuinely ordered"* — **and that reason is false**, as two
   things in the tree already said: `candidate_order.rs`'s own module header (*"insertion order … is an
   artifact of how a particular node interleaved its reductions rather than a property of the
   program"*), and `speculation::content_fingerprint`, which **sorts within a channel** and has since it
   was written. **CITED**.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | NO — *"Neither changes an answer: the same branches are found and the same terms delivered."* **CITED**. |
| 2 · verdict | NO — *"The selection was never affected: `order_candidates_with_index` sorts by `(content_hash, store_index)` and the content hash dominates."* **CITED**. |
| 3 · bytes (Lane B) | **MOVES** — the published `Par` bytes. |
| 3 · bytes (Lane P) | **MOVES** — same. |
| 4 · post-state hash | **MOVES** — ★ these bytes reach a **live deploy's RSpace** via `Publisher::publish` → `produce`, so they are part of the post-deploy state and therefore of the checkpoint root. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

★ **This is the register's clearest case of "the old behaviour was itself nondeterministic".** The
divergence is not old-versus-new; it is **run-versus-run on one node**. Two validators, or the same
validator twice, produced different published bytes for the same configuration.

**Blast radius.** `^spec-success`, `^spec-truncated`, all three `^spec-delivery` collections, and — for a
subject with no registered guest, via `LeafProjection::Configuration` — **the bare reply term published
on the caller's own channel**. **CITED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** Published, hashed bytes that vary between runs of the same
program. There is no version of consensus that survives that.

**★ The diagnostic method deserves a reviewer's attention, because the first discriminator was
unsound.** *"20 runs giving 20 distinct digests was read as an entropy signature; the trace is ~400
steps, so the space of index labelings is astronomically large and all-distinct is equally what a
scheduler race predicts. The measurement with actual discriminating power is a **dose-response curve over
scheduler width**"* — varying `TOKIO_WORKER_THREADS` and counting distinct digests. **CITED**
(`03ec33de`).

**Why it went unnoticed.** *"a reified configuration is never RENDERED: no transcript shows it."*
**CITED**.

**Authority.** No owner ruling.

---

### CBR-L06

**Published diagnostics stop being derived `Debug` dumps.**

| | |
|---|---|
| Commit(s) | `2d0ec9b1` (the class), `df57a828` (the two remaining fields) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `rholang-runtime/src/lookahead.rs`, `speculation/` |

#### (a) The issue

`guest_evaluator_failures` embedded `format!("{stuck:?}")` — **prost's derived `Debug`** for the
reflected redex — into the `message` of a FIPS `failure` leaf, and `SpeculationError::Display` rendered
the **derived `Debug`** of `InterpreterError` / `RSpaceError`. Those strings are published by
`Publisher::publish`, which calls `produce` into the **live deploy's RSpace**.

★ **Why a derived `Debug` in published bytes is a consensus fault in its own right**: *"A derived `Debug`
is generated code. A `thiserror` or `prost` bump that re-spells it silently changes those bytes, and **a
node built against the new derive can no longer replay a block produced by the old one.** That is
precisely the hazard `ErrorCode` writes its discriminants out longhand to prevent."* **CITED**.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | NO — *"the same branches are found and the same terms delivered."* **CITED**. |
| 2 · verdict | NO |
| 3 · bytes (Lane B) | **MOVES** — the spelling (D4) and contents (D5) of a published diagnostic. |
| 3 · bytes (Lane P) | **MOVES** — same. |
| 4 · post-state hash | **MOVES** — part of the checkpoint root. |
| 5 · accepted programs | NO |
| 6 · metering | NO |

★ **This entry is also the concrete instance of a false-negative class this report's own method would
miss** (§3.4, class 1): under this register's definition, *a dependency version bump alone* — `prost` or
`thiserror` — was a consensus change, and no path in $`\mathcal{P}`$ would have surfaced it.

**Blast radius.** Three instances, enumerated rather than patched one at a time:
(1) `guest_evaluator_failures` — the reported defect; (2) `parse_request`'s malformed `[n]` bound —
*"strictly worse, because `^spec-n` is an ordinary channel served by an installed system process, so the
operand is **program-controlled** and reaches it directly"*; (3) `unserved_requests` — host-side, not a
replay hazard, but the same ~15 KB dump. **CITED**.

**Measured**: **14 KB of nested struct noise for a fact that renders in 30 characters.** **CITED**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A dependency upgrade becomes a hard fork, silently, with no code
change in this repository at all.

**Why these two sites were missed by the first sweep.** *"these sites were missed because nothing renders
a `SpeculationError` in a transcript."* **CITED** — the same invisibility that hid **CBR-L05**.

**Authority.** No owner ruling.

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
| 6 · metering | **UNVERIFIED** — this register did not establish whether MeTTaIL's `last` reuses `nth`'s price as **CBR-024** does. |

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

### CBR-L09

**DELIBERATE DIVERGENT — float division by zero answers `error`, where upstream yields $`\pm\infty`$.**

| | |
|---|---|
| Commit(s) | **Pre-existing**; ruled **kept** on 2026-07-29. No commit implements it — the ruling is to *not* change it. |
| Status | LIVE DIVERGENCE, ruled |
| Direction | **DIVERGENT** |
| Evidence grade | WITNESSED |
| Files | `mettail-rust`: `languages/src/rholang.rs`, the `CastFloat`/`CastFloat` division arm. F1r3node: `rholang/src/rust/interpreter/reduce.rs`, the `(GDouble, GDouble)` division arm — `f64::from_bits(d1) / f64::from_bits(d2)`, **no zero guard**. **DERIVED** (read at both sites). |

#### (a) The issue

MeTTaIL's Rholang answers `Proc::Err` for `x / 0.0` on floats:

```rust
// languages/src/rholang.rs — the CastFloat ÷ CastFloat arm
(Float::FloatLit(x), Float::FloatLit(y)) => {
    if y.get() == 0.0 {
        Proc::Err
    } else {
        match <mettail_runtime::CanonicalFloat64 as mettail_runtime::SafeArith>::safe_div(*x, *y) {
            Some(v) => Proc::CastFloat(std::sync::Arc::new(Float::FloatLit(v))),
            None => Proc::Err,
        }
    }
}
```

F1r3node's reducer has **no such guard** on the `(GDouble, GDouble)` arm and therefore yields IEEE-754
$`\pm\infty`$ [IEEE754-2019]. Integer, rational and fixed-point division **do** refuse zero on both
sides; float is the sole asymmetry.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — `1.0 / 0.0` is $`+\infty`$ on the node and `error` in MeTTaIL. |
| 2 · verdict | **MOVES** — a subsequent comparison or `match` differs. |
| 3 · bytes (Lane B) | **MOVES** |
| 3 · bytes (Lane P) | **MOVES** |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | NO — the program parses and normalizes identically. |
| 6 · metering | NO |

**The disagreement.** ★ **A program upstream accepts does not compute the same value here.** This is on
the wrong side of the standard the owner set — *"diagnostics may exceed, semantics may not diverge"* —
and it is recorded as a divergence rather than folded in among the bug fixes.

**Blast radius.** Every program dividing floats by a possibly-zero divisor. Reachable: **yes**.

**Could live chain state have been produced under the old behaviour?** N/A on Surface N (the node is
unchanged). ★ The forward-looking question is the one that matters: **if MeTTaIL's Rholang becomes the
node's language, this divergence becomes a hard fork against every block containing a float division by
zero.** Settling query: scan history for `GDouble` division whose divisor evaluates to $`\pm 0.0`$.
**UNVERIFIED**.

#### (c) Why the divergence was kept

**Authority — the ruling, verbatim, with its date.**

> **2026-07-29T17:53:39Z** — asked *"That is a LIVE SEMANTICS divergence — in the stricter direction, but
> on the wrong side of the 'diagnostics may exceed, semantics may not diverge' line you set,"* the owner
> selected:
>
> **"Keep the strictness — it is the better semantics"**

The stated rationale in the option: $`\pm\infty`$ propagating silently through a computation is the
failure mode the strictness prevents.

**What a reviewer should take from this.** This is an *accepted* divergence, not an oversight. It is
listed at full weight, with all six axes answered, because a reviewer comparing the two implementations
must find the reasoning here rather than rediscover the difference in a test failure. ⚠ It is
**unresolved** in the sense that it will require either an upstream change or an explicit exception at
the point MeTTaIL's Rholang becomes normative.

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

**The literal-domain and canonical-surface repairs.**

| | |
|---|---|
| Commit(s) | `ab13aee0` (D1+D2, a literal category must spell every value it can hold), `4aa64cb6` (the `UInt32` acceptor), `9485d372` (mandatory literal tails and the sign rule), `f2f2351b` (the `@`-wrap asks the surface, not the template), `5a5cc9b0` (a sigil-led prefix application is not a primary), `7244058b` (X5 — one denotation, one surface: the canonical member is DECLARED) |
| Status | LANDED |
| Direction | CORRECTIVE |
| Evidence grade | WITNESSED |
| Files | `macros/src/gen/syntax/display.rs`, `languages/src/*.rs`, `ast/src/` |

#### (a) The issue

A cluster with one root: **the printed surface and the accepted surface were derived from different
things**, so a term could print in a form its own grammar would not accept, or accept a spelling its
`Display` never writes. Six instances, each with its own gate.

#### (b) How it (potentially) breaks consensus

| Axis | Verdict |
|---|---|
| 1 · computed value | **MOVES** — a differently-parsed term. |
| 2 · verdict | NO — no matching relation changes directly. |
| 3 · bytes (Lane B) | **MOVES** — display/parse round-tripping is how terms move between MeTTaIL surfaces. |
| 3 · bytes (Lane P) | **MOVES** |
| 4 · post-state hash | **MOVES** |
| 5 · accepted programs | **MOVES** — `4aa64cb6` *narrows* the `UInt32` acceptor to spellings its `Display` writes; `ab13aee0` *widens* a literal category to spell every value it can hold. |
| 6 · metering | NO |

**Blast radius.** Programs using literal spellings at the boundary of a category's domain, and programs
whose terms are round-tripped through the surface. Reachable: **yes**.

#### (c) Why the change was necessary or correct

**What breaks if we do not change it.** A language whose printer emits text its parser rejects has no
usable serialisation at the surface level, and one whose parser accepts text its printer never emits has
**two spellings for one denotation** — which is exactly the "canonical member" problem `7244058b`
closes by **declaring** the canonical member rather than electing it.

**Why gates rather than fixes alone.** Each commit adds the gate that fails *at the grammar* rather than
at the symptom — `f2f2351b`'s is explicit: *"a gate so the next one fails at the grammar"*.

**Authority.** No owner ruling on the individual repairs; they follow the superset standard.

⚠ **Grouped rather than split, and the reader should know why.** These six share one root and one gate
family; splitting them would produce six entries with near-identical axis profiles and no additional
review signal. If any one of them needs to be weighed separately, its commit is named above.

---

## 5. Risk analysis

### 5.1 Aggregate axis exposure

Counting **register entries**, not commits. `●` cells from the summary table in §4.1.

| Axis | Entries that move it | Share of the 40 |
|---|---|---|
| 1 · computed value | **17** | 43 % |
| 2 · verdict | **20** | 50 % |
| 3 · bytes — Lane B (bincode) | **19** | 48 % |
| 3 · bytes — Lane P (prost) | **18** | 45 % |
| 4 · post-state hash | **28** | 70 % |
| 5 · accepted programs | **12** | 30 % |
| 6 · metering | **2** | 5 % |

**Reading.** The post-state hash is the most-touched axis, which is expected: it is downstream of both
value and verdict. The **metering** axis is touched by exactly two entries (**CBR-024**, **CBR-025**),
both of which add a *new* charge site for a *new* method and neither of which re-prices anything
existing — which is the intended posture, since pricing is a consensus decision and budgets are
F1r3node's.

### 5.2 The three highest-risk entries, and why

| Rank | Entry | Why it ranks here |
|---|---|---|
| 1 | **CBR-019** | The **largest blast radius in the register: total.** If the byte-identity claim is false, every produce and every consume in the system hashes differently. There is no partial failure mode. The claim is extensively measured — but it is a claim of *neutrality*, and neutrality claims are the ones that fail silently. |
| 2 | **CBR-001** | The only entry that changes **when a COMM fires** for guard-free programs as well as guarded ones, and whose failure mode is a **silent post-state divergence with no detectable event** (permuted selections build the same COMM event and slip past the trace assertion). Its determinism argument is the load-bearing part and should be reviewed on its own. |
| 3 | **CBR-027** | The only change that alters **computed values by design** rather than by correcting an outright defect, and the only **REGRESSIVE** entry on Surface N whose old behaviour produced a committed *value* rather than an error. Its chain-history query should be run **before** it ships. |

### 5.3 Direction profile

| Direction | Count | Comment |
|---|---|---|
| CORRECTIVE | **23** | The bulk. A wrong answer becomes right; the program ran before and runs now. |
| PERMISSIVE | **9** | Mostly liveness (**CBR-020**, **CBR-022**, **CBR-023**) and additive surface (**CBR-024**, **CBR-025**). |
| **REGRESSIVE** | **4** | ★ **CBR-002**, **CBR-027**, **CBR-L03**, **CBR-L08**. These are what a reviewer weighs hardest. |
| NEUTRAL | **2** | **CBR-019**, **CBR-019b** — in the register because their neutrality is a measured claim. |
| DIVERGENT | **1** | **CBR-L09**, ruled and kept. |
| — | **1** | **CBR-028**, an open hazard with no change. |
| **Total** | **40** | |

**The four REGRESSIVE entries, stated plainly** — a previously-succeeding thing now fails:

- **CBR-002** — a deploy whose `where` guard is undecidable normalized, ran, and admitted nothing. It now
  **fails**. Deterministic across validators, hence a slashable-fault class rather than a silent fork.
- **CBR-027** — a deploy computing `i64::MAX + 1` produced `i64::MIN`. It now **fails**.
- **CBR-L03** — a guarded receive whose formula collapsed to constant-true **fired**. It now declines and
  the datum rests.
- **CBR-L08** — `{| @a : @b |}` **parses today** and yields an empty map. It will be **refused**.

### 5.4 What could have produced live chain state

Eleven entries carry an unanswerable chain-history question. They are consolidated here so a reviewer can
commission the queries as one piece of work rather than eleven.

| Entry | The query that would settle it | Cost |
|---|---|---|
| **CBR-020** | Run `decode_datums` over every LMDB cold-store record on a synced node; count `Err`s. | ★ **Cheapest and highest value — executable today with no instrumentation.** |
| **CBR-018** | String-search block bodies' `ProcessedSystemDeploy::Failed.error_msg` for the literal `<unprintable>`. | ★ Cheap; **decisive** on its own. |
| **CBR-028** | For every `ProduceEventProto` in every deploy log, attempt `Par::decode` on each `output_value` element; count failures. | Cheap; decisive. |
| **CBR-001** | Parse every historical `ProcessedDeploy.deploy.data.term`; report those with `ReceiveBind` count > 1 or a non-empty `Receive.condition`. | Moderate — needs a term walker. |
| **CBR-002** | Same walk; report guards containing any of the six refused `ExprInstance` arms. | Moderate; shares the walker. |
| **CBR-003** | Same walk; report guards containing `EMatchesBody`. | Moderate; shares the walker. |
| **CBR-004** | Same walk; report `ReceiveBind.pattern` / `MatchCase.pattern` containing any of the eleven newly-implemented arms. | Moderate; shares the walker. |
| **CBR-006** | Same walk; report `EMatches` whose `pattern` subtree contains a `VarRefBody`. | Moderate; shares the walker. |
| **CBR-008**, **CBR-009**, **CBR-010** | Same walk; report `atPath` / `descendTo` / `getLeaf` / `dropHead` / `setSubtrie` calls whose argument or source is not a ground `EList`. | Moderate; shares the walker. |
| **CBR-011**, **CBR-012**, **CBR-013** | Scan event-hash preimages for an `EPathMap` with `connective_used = true` or a non-empty `remainder`. | Moderate. |
| **CBR-005** | Replay history under an instrumented build; count matcher attempts whose winning snapshot exceeds its own writes. | ★ **Expensive** — needs a full instrumented replay. |
| **CBR-007** | Scan chain history **and the deploy corpus** for `MatchCase` patterns and `EMatches` right-hand sides whose `connectives` contain a `ConnNotBody` or `ConnOrBody` **whose body carries a `FreeVar` or has `connective_used == true`**. | Moderate — a static scan, no replay needed. ★ Sharper than a replay because the predicate is syntactic. |
| **CBR-027** | Replay under an instrumented build counting `GInt` `+`/`-` where `checked_*` would return `None`. | Expensive, but ★ **should be run before this change ships.** |
| **CBR-016** | Audit node deployment configurations for `PRETTY_PRINTER_OUTPUT_TRIM_AFTER`; separately count historical `ProcessedSystemDeploy::Failed`. | ⚠ **Operational, not a repository question.** |
| **CBR-022** | Search node crash logs for SIGABRT with a stack-overflow signature during `doDeploy` / `deploy_cosigned`. | ⚠ Operational. |

★ **Nine of these share one artefact**: a walker over historical deploy terms with a per-entry predicate.
Building it once discharges most of the table.

### 5.5 The conjunction risk

`Validate::version` is exact equality with no activation-height machinery, so **these entries cannot be
rolled out independently**. A reviewer accepting any one of them is accepting the version bump that
carries all of them. The practical consequence: the aggregate risk is not
$`\max_i r_i`$ but closer to $`1 - \prod_i (1 - r_i)`$ over the entries whose neutrality claims could
be wrong — which is why **CBR-019**'s neutrality is the single most consequential claim in the report,
and why §7's gate matters more than any individual entry.

---

## 6. Threats to validity

### 6.1 What this report did not do

1. **It did not re-run the campaign's measurements.** Every number quoted from a commit message is tagged
   **CITED**, not **MEASURED**. A commit that measured wrongly, or that measured a different thing from
   what its prose says, would pass this sweep.
2. **It did not read generated code.** The wire tables in `OUT_DIR` are produced by `models/build.rs`;
   this report read the generator, not its output.
3. **It did not verify the remaining in-flight entries' tests.** **CBR-027** and **CBR-L08** describe
   *intended* behaviour, and each states what would change if the implementation diverges from the
   design.
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
production path", and the linearity premise it rested on. The fix `f5fd6c34` made is correct and is
**CBR-005**.

**★ The remedy, and why history was not rewritten.** Per the project's standing rule that published
history is not rewritten, the correction is **not** an amendment to `f5fd6c34`. `dc383ed1` — a
documentation-only commit — records it **at the two addresses that carry the claim**: `aggregate_updates`'
doc comment and the refusals-counter comment in `metrics_constants.rs`. A reviewer reading either site
now finds the correction beside the claim rather than discovering the discrepancy themselves, which is
the worst way to find it.

**What a reviewer should take from this.** Not that the measurement was sloppy — it was exact for the
corpus it ran on — but that **a corpus measurement was quoted as a universal**. That is the same failure
shape as §6.6(2), and it is why every number in this report carries a **CITED** / **MEASURED** tag saying
who observed it.

### 6.3 Known open questions

| # | Question | Where it came from | Status |
|---|---|---|---|
| 1 | Can `Compiler::top_level_error`'s `HashMap`-ordered variable list reach the persisted `ProcessedDeploy::system_deploy_error`? **64 calls on one source produced 2 distinct strings.** | **CBR-022** evidence; audit §14.10.10, E102 | ⚠ **OPEN.** If yes, it is an unregistered consensus fault of the **CBR-016** class — an ordering nondeterminism inside block-resident bytes. |
| 2 | Is the `EPathMap` tag-8 read ceiling reachable by a deploy? | **CBR-028** | ⚠ **OPEN**, and explicitly *"must not be assumed by analogy"* with the bare-`Par` member. |
| 3 | What is the disposition of the five FFI reads that **panic** rather than `Err` on a too-deep `Par`? | **CBR-028** | ⚠ **OPEN.** Strictly worse than the `Err` case. |
| 4 | `#109 residual 3` — a peer-steerable `decode_trie_path(..).unwrap_or(..)`, reachable via an `EZipper.current_path` that is not a valid codec path, and `current_path` crosses the wire as `repeated bytes`. | campaign ledger | **RULED** *"stuck term"* (owner, 2026-07-29T14:04:34Z), **not started**. It is not in this register because no change has been made; when it lands it becomes an entry with Axis 1 and Axis 2 moving. |
| 5 | Does MeTTaIL's `last` reuse `nth`'s price, as F1r3node's does? | **CBR-L07** | **UNVERIFIED** — the one `?` cell in the summary table. |
| 6 | Two CI jobs lack the f1r3node sibling checkout, so the cross-repository agreement **CBR-L10** rests on is not gated. | campaign ledger | ⚠ **OPEN.** |

### 6.4 UNVERIFIED budget

**One** axis cell in the summary table is `UNVERIFIED` (**CBR-L07**, metering). Every other cell is
answered. Eleven entries carry an **UNVERIFIED** chain-history answer, consolidated in §5.4 — these are
questions about *history*, not about the code, and are unanswerable from inside the repository by
construction.

### 6.5 Coverage asymmetry between the two surfaces

⚠ **The Surface-N sweep is an exact partition; the Surface-L sweep is not.**

| | Surface N | Surface L |
|---|---|---|
| Commits in the campaign window | 111 | 236 |
| Commits touching the path set | **78** | 149 |
| Covered by register entries | **57 SHAs / 29 entries** | 16 SHAs / 11 entries |
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
   verdicts. Two entries live there (`eaa44c2f` in **CBR-002**, `a3fd6fe4` in **CBR-023**) and were found
   only because the entry set was assembled from commit *messages* as well as from paths. The path set in
   §3.1 has been corrected; the *general* risk stands, and **CBR-L06** shows its sharpest form: under
   this report's own definition, **a `prost` or `thiserror` version bump alone** is a consensus change,
   and no source path would show it.
2. **A false neutrality assertion** in a commit message — see §6.1(1).
3. **An emergent divergence with no single owning commit** — two individually byte-neutral changes
   composing into a non-neutral one. A per-commit sweep cannot see it. **No mitigation. Named as a gap.**
4. **Generated code** — the `cargo:rerun-if-changed` stale-`OUT_DIR` defect (**CBR-019** evidence) is the
   measured instance of this class.
5. **Uncommitted and concurrent work** — both trees had substantial uncommitted changes and several
   agents were editing concurrently while this was written. **CBR-016**'s evidence records a concrete
   instance of the harm: a concurrent whole-file rewrite silently removed a consensus-relevant hunk, and
   it was caught by the **build**, not by review.

### 6.7 Reconciliation with the coordinator's candidate list

The candidate list supplied to this report was treated as hypotheses. Its disposition, in full, including
where it was wrong:

**Confirmed as stated:**

- `f5fd6c34`, `eaa905fe` — #144: *"changes bound values, not verdicts."* **CONFIRMED** → **CBR-005**.
- `8853f839` — #126: *"substituted bytes are signed bytes."* **CONFIRMED** → **CBR-006**.
- #148 — *"changes verdicts (un-refuses matches that should have fired)."* **CONFIRMED**, and it
  **landed during authoring** as `b219e199` + `dc383ed1` → **CBR-007**. ★ Two of the coordinator's own
  framings were then refuted by measurement and are recorded in the entry: the acceptance matrix's
  **row-2 prediction was wrong** (arms-only goes entirely green, so site 4 is a local guarantee rather
  than independently load-bearing), and the design's row set had to be **extended** with an
  outer-boundary snapshot row that is the actual discriminator.
- The `wrapping_add`/`wrapping_sub` fix — *"changes computed values."* **CONFIRMED** → **CBR-027**;
  **not** in the tree.
- The kv repair — *"changes which programs are accepted."* **CONFIRMED** → **CBR-L08**.
- The float $`\div 0`$ divergence — **CONFIRMED** as a deliberate DIVERGENT → **CBR-L09**.

**Corrected:**

- ⚠ **Line numbers.** The candidate list cites `reduce.rs:3503, :3598` for the wrapping arithmetic. The
  working-tree lines are **3503** and **3597**; at `8853f839` the same expressions are at **3397** and
  **3489**. **MEASURED.** Minor, but a review document that cites a line must cite the right one and say
  which revision it is in.
- ⚠ **#109 residual 3** is listed as "RULED, NOT STARTED". It is **not** a register entry, because no
  change has been made — a register of *changes* must not contain a change that does not exist. It is
  recorded in §6.3 as an open question instead.
- ⚠ **#130** ("the value slot needs a PAIR") is a **measured finding**, not a change: `decode_trie_path`
  is not total on `encode_trie_path`'s image. It appears as evidence inside **CBR-012** and as a leg of
  **CBR-028**, not as an entry of its own.
- ⚠ **#116** ("EPathMap must BE a trie map") is not one change but **three landed stages** — S1
  (**CBR-011**), S2 (**CBR-012**), S3 (**CBR-013**) — with distinct and materially different axis
  profiles. S1 moves no bytes; S2 moves both wires for non-ground maps. Treating them as one entry would
  have hidden exactly the distinction a reviewer needs.
- ⚠ **#119 / #120 / #121** ("prost codec depth asymmetry") resolve into **two** entries plus an open
  hazard: the bincode codec (**CBR-019**), the dormant prost encoder (**CBR-019b**), and the read-side
  asymmetry which is **not repaired** (**CBR-028**). The candidate list's framing as a single
  "consensus-adjacent" item understates it: **CBR-028** is a live, witnessed, unrepaired hazard.
- ⚠ **#135** ("the only one on a consensus wire") is **one leg of three** in **CBR-028**, not a
  standalone item; and the campaign's own severity claim for the sibling `#129` was **measured and
  downgraded** from *"proposer-controlled consensus divergence"* to LATENT with a 31-level margin.

**Added by this sweep, and not on the candidate list:**

**CBR-001**, **CBR-002**, **CBR-003**, **CBR-004**, **CBR-008**, **CBR-009**, **CBR-010**, **CBR-014**,
**CBR-015**, **CBR-016**, **CBR-017**, **CBR-018**, **CBR-020**, **CBR-021**, **CBR-022**, **CBR-023**,
**CBR-024**, **CBR-025**, **CBR-026**, **CBR-L01** .. **CBR-L07**, **CBR-L10** and **CBR-L11** —
**28 of the 40 entries**. (The candidate list is traceable to the other 12: **CBR-005**, **CBR-006**,
**CBR-007**, **CBR-011**, **CBR-012**, **CBR-013**, **CBR-019**, **CBR-019b**, **CBR-027**, **CBR-028**,
**CBR-L08**, **CBR-L09**.)

★ Two of these are, on the analysis above, higher-risk than anything on the candidate list:
**CBR-019** (total blast radius) and **CBR-001** (silent post-state divergence with no detectable event).
This is the strongest available evidence that a register must be **derived** rather than assembled.

---

## 7. Maintenance — how an omission fails loudly

### 7.1 The problem, stated as an engineering requirement

This register is a **derived fact** maintained by hand. Derived facts maintained by hand drift. §1.2
records that this campaign hit that class **six times in one session**, and in each case the repair was
to *derive the set rather than list it*, or to *make the wrong form unspellable*.

> **Requirement.** A commit that lands on a consensus-critical path without a corresponding register
> entry or a typed exemption must cause a **build failure that names the commit**, not a discrepancy
> somebody may notice later.

### Figure 4 — the drift gate

![The drift gate](figures/drift-gate.svg)

*Source: [`figures/drift-gate.puml`](figures/drift-gate.puml).*

### 7.2 The design, and why this one

⚠ **Status: DESIGNED, NOT BUILT.** Of the three artefacts below, only the prose register (this document)
exists at the time of writing. `register.toml`, `REGISTER_BASE` and the gate are specified here — paths,
data shape, checks, failure messages and anti-vacuity cells — so that building them is an implementation
task with no remaining design decisions. Until they exist, **this register is exactly the
discipline-dependent artefact §1.2 argues against**, and a reviewer should treat that as the report's
largest maintenance risk.

**Three artefacts:**

| Artefact | Path | Role |
|---|---|---|
| The prose register | `docs/consensus/consensus-change-register.md` | This document. Human-written; section (c) is the reviewable content. |
| The machine index | `docs/consensus/register.toml` | One `[[entry]]` per register ID with its SHAs and its seven axis cells; one `[[exempt]]` per exempted SHA with a typed `reason` and a non-empty `evidence`. |
| The anchor | `docs/consensus/REGISTER_BASE` | A single SHA. Moves only by an explicit, reviewed edit — which is what makes "the range" a decision rather than an accident. |

**The gate:** `shared/tests/consensus_change_register_gate.rs`.

**Why `shared`.** It is in the CI crate matrix (`.github/workflows/ci.yml:177-188`), it is cheap to
build, and — the load-bearing reason — **it does not depend on `models`, `rholang` or `rspace++`**, so
the gate cannot be broken by the code it polices.

**Why a test that reads git is not a novelty here.** CI **already** checks out full history, deliberately,
for `rholang/tests/normalize_oracle_provenance.rs`, which re-derives an oracle twin from git and compares
it byte-for-byte. The workflow comment says so in as many words. This gate reuses an established
mechanism rather than introducing one.

**The seven checks**, in order, each with its own failure message (Figure 4):

1. **Non-vacuity floor** — $`\mathcal{O} \neq \varnothing`$. A gate that finds nothing to check is a
   gate that cannot fail; an empty obligation set means the path set or the anchor is wrong.
2. **Coverage** — $`\mathcal{O} \subseteq \mathcal{E} \uplus \mathcal{X}`$. Names every unregistered
   SHA and the consensus path it touched.
3. **Exactness** — $`\mathcal{E} \cap \mathcal{X} = \varnothing`$ **and**
   $`\mathcal{E} \uplus \mathcal{X} \subseteq \mathcal{O}`$. Catches a SHA listed twice, and a stale row
   naming a commit the range no longer contains (which is what a rebase produces).
4. **Typed exemptions** — every `reason` from the **closed** enum of §3.2; every `evidence` non-empty and
   naming a test or a measurement. A free-text reason is a shrug; adding a reason variant is a code
   change and therefore reviewed.
5. **Complete axis answers** — all seven cells present, from `{MOVES, NO, N/A, UNVERIFIED}`.
6. **UNVERIFIED budget** — the count of `UNVERIFIED` cells is asserted **exactly**, so `UNVERIFIED` stays
   a legitimate answer that cannot silently accumulate. Raising the budget is a visible diff.
7. **Prose ↔ index agreement** — every index `id` appears as a heading in the register, and every
   register heading appears in the index.

The algorithm, in literate form:

```text
ALGORITHM  ConsensusRegisterGate
INPUT      REGISTER_BASE : Sha
           P             : Set of consensus-critical path prefixes
           register.toml : (entries : Set of Entry, exempt : Set of Exemption)
           register.md   : the prose document
OUTPUT     Pass, or Fail carrying the offending SHAs and the clause violated

⟨Enumerate the obligation⟩ ≡
   O ← { c : c ∈ rev-list(REGISTER_BASE .. HEAD) ∧ touches(c, P) }

⟨Assert the floor⟩ ≡                                    -- check 1
   if O = ∅ then Fail("non-vacuity: the gate has nothing to check")

⟨Partition⟩ ≡                                           -- checks 2 and 3
   E ← ⋃ { e.commits : e ∈ entries }
   X ← { x.commit : x ∈ exempt }
   if O ⊄ E ⊎ X   then Fail("unregistered", O ∖ (E ∪ X))
   if E ∩ X ≠ ∅   then Fail("double-listed", E ∩ X)
   if E ∪ X ⊄ O   then Fail("stale row", (E ∪ X) ∖ O)

⟨Type-check the exemptions⟩ ≡                           -- check 4
   for x ∈ exempt:
      if x.reason ∉ CLOSED_REASONS  then Fail("untyped exemption", x)
      if x.evidence = ""            then Fail("undischarged exemption", x)

⟨Type-check the entries⟩ ≡                              -- checks 5 and 6
   for e ∈ entries:
      if |e.axes| ≠ 7                        then Fail("incomplete axes", e.id)
      if ∃ a ∈ e.axes . a ∉ AXIS_VOCABULARY  then Fail("bad axis value", e.id)
   if count(a = UNVERIFIED) > UNVERIFIED_BUDGET  then Fail("unverified creep")

⟨Cross-check the prose⟩ ≡                               -- check 7
   if headings(register.md) ≠ { e.id : e ∈ entries } then Fail("prose/index divergence")

Pass
```

### 7.3 Why this design and not the alternatives

| Alternative | Verdict |
|---|---|
| **A git pre-commit hook.** | ★ **REJECTED on measured grounds.** *"No git hook has ever run in the f1r3node worktree — every local gate is inert."* **CITED** (campaign ledger). A hook is a non-repair here: it would be shipped, believed, and dead. |
| **Generate the prose from the index.** | **REJECTED.** Section (c) — the justification — is the part a reviewer weighs, and generating it turns it into a template. The index checks the prose; it does not author it. |
| **Assert only that the register is non-empty.** | **REJECTED** as vacuous: it passes forever after the first entry. |
| **A reviewer checklist.** | **REJECTED.** That is discipline, and discipline demonstrably did not keep the other five derived sets honest. |
| **Derive the entries themselves.** | ★ **Not possible, and saying so is the point.** "Does this commit move an axis?" is not decidable from the diff. What *is* mechanisable is the obligation to **answer the question** for every commit, and that is exactly what the exception table forces. |

### 7.4 Anti-vacuity — the gate must be shown RED

The gate is subject to the same standard it enforces. Before it is trusted, three cells must be executed:

1. **Remove one SHA** from `register.toml`. The gate must fail **naming that SHA**.
2. **Add a SHA outside the range.** The gate must fail on the *stale row* clause — this is the check that
   catches a rebase, and it is the one a naive `⊆` test would miss.
3. **Blank one `evidence` field.** The gate must fail on the *undischarged exemption* clause.

A control run with none of the three mutations must pass.

### 7.5 First extensions

1. **Close the Surface-L gap** (§6.5): give `mettail-rust` its own anchor, path set and exemption table
   so that surface becomes an exact partition too.
2. **Add a dependency-version leg.** **CBR-L06** proves that a `prost` or `thiserror` bump alone can move
   published bytes. The natural mechanisation is to include `Cargo.lock` in the path set, with a typed
   reason `DEP_BUMP_BYTE_NEUTRAL` requiring a named differential as its evidence.
3. **Wire the chain-history walker** of §5.4 as a one-off tool and record its answers as entry fields, so
   that "could live chain state have been produced?" stops being `UNVERIFIED` for nine entries at once.

---

## 8. Conclusions

1. **40 consensus-visible changes** were derived from the campaign record: 29 on the F1r3node node, 11
   on MeTTaIL's Rholang. **Thirty-six are landed, three are in flight**, one is an open unrepaired
   hazard.
   **Twenty-eight of the 40 were not on the coordinator's candidate list**, including the two the
   analysis ranks highest-risk — which is the report's own strongest argument for deriving a register
   rather than assembling one.
2. **The axes are genuinely independent and must be reviewed separately.** `7dcff96f` moves four bytes on
   the bincode lane and **zero** on the protobuf lane, for the same field addition. `f5fd6c34` moves
   bound values and provably not verdicts; `#148` moves verdicts and not the values it then binds. A
   review that collapses these into "consensus-breaking" cannot weigh any of them.
3. **The dominant risk is a neutrality claim, not a behaviour change.** **CBR-019** rerouted the channel
   leg of every event hash through a new encoder. Its blast radius if the byte-identity claim is wrong is
   *total*, and neutrality claims are the ones that fail silently. Its evidence is correspondingly the
   most extensive in the report — an exhaustive structural differential against a retained,
   compiler-generated oracle, with an executed proof that the differential can go RED, and a
   **generator-level** mutation proof, because two earlier mutations in this campaign *reported green
   because they had not applied*.
4. **Four entries are REGRESSIVE**, and they are named plainly in §5.3. One of them (**CBR-L08**) refuses
   source that parses today.
5. **One entry is a deliberate, owner-ruled divergence** (**CBR-L09**) and is labelled as such rather
   than folded in among the repairs.
6. **A shipped commit contains a claim that is currently false** (§6.2), disclosed with its mechanism,
   its remedy, and the measurement a reviewer should require.
7. **Eleven entries carry a question that cannot be settled from inside the repository.** Nine of them
   share one artefact — a walker over historical deploy terms. Two of the queries (**CBR-020**,
   **CBR-018**) are cheap and individually decisive and should be run first.
8. **The register must not depend on discipline.** §7 specifies a SHA-keyed, typed exception table with a
   non-vacuity floor, sited where CI already provides what it needs, argued against four alternatives —
   one of which (a git hook) is rejected on the measured ground that **no git hook has ever run in this
   worktree**.

**Recommendation to the reviewer.** Weigh **CBR-019**'s neutrality evidence, **CBR-001**'s determinism
argument, and **CBR-027**'s chain-history query first; commission the §5.4 walker; and require §7.4's
three RED cells before the gate is trusted.

---

## References

- [Milner1992] R. Milner, J. Parrow, D. Walker. *A Calculus of Mobile Processes, I & II.* Information and
  Computation 100(1), 1992. DOI: [10.1016/0890-5401(92)90008-4](https://doi.org/10.1016/0890-5401(92)90008-4)
  and [10.1016/0890-5401(92)90009-5](https://doi.org/10.1016/0890-5401(92)90009-5).
- [Meredith2005] L. G. Meredith, M. Radestock. *A Reflective Higher-order Calculus.* Electronic Notes in
  Theoretical Computer Science 141(5), 2005, 49–67. DOI:
  [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
- [Hopcroft1973] J. E. Hopcroft, R. M. Karp. *An $`n^{5/2}`$ Algorithm for Maximum Matchings in
  Bipartite Graphs.* SIAM Journal on Computing 2(4), 1973, 225–231. DOI:
  [10.1137/0202019](https://doi.org/10.1137/0202019). — the algorithm family behind
  `maximum_bipartite_match.rs`, whose per-attempt state ownership is **CBR-005**.
- [Pratt1973] V. R. Pratt. *Top Down Operator Precedence.* POPL '73, 41–51. DOI:
  [10.1145/512927.512931](https://doi.org/10.1145/512927.512931). — binding powers, whose parity
  obstruction is **CBR-L01**.
- [Knuth1984] D. E. Knuth. *Literate Programming.* The Computer Journal 27(2), 1984, 97–111. DOI:
  [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97). — the form of §7.2's algorithm.
- [IEEE754-2019] IEEE Standard for Floating-Point Arithmetic, IEEE Std 754-2019. DOI:
  [10.1109/IEEESTD.2019.8766229](https://doi.org/10.1109/IEEESTD.2019.8766229). — the $`\pm\infty`$
  semantics **CBR-L09** diverges from.
- [Aumasson2013] J.-P. Aumasson, S. Neves, Z. Wilcox-O'Hearn, C. Winnerlein. *BLAKE2: Simpler, Smaller,
  Fast as MD5.* ACNS 2013, LNCS 7954, 119–135. DOI:
  [10.1007/978-3-642-38980-1_8](https://doi.org/10.1007/978-3-642-38980-1_8). — `Blake2b256` (event
  hashes) and `Blake2b512Random` (unforgeable-name derivation, **CBR-023**).
- [Merkle1988] R. C. Merkle. *A Digital Signature Based on a Conventional Encryption Function.*
  CRYPTO '87, LNCS 293, 369–378. DOI:
  [10.1007/3-540-48184-2_32](https://doi.org/10.1007/3-540-48184-2_32). — the post-state hash's tree
  structure.

**In-repository sources.** `casper/src/rust/validate.rs`, `casper/src/rust/block_status.rs`,
`casper/src/rust/rholang/replay_runtime.rs`, `rspace++/src/rspace/hashing/stable_hash_provider.rs`,
`models/src/main/protobuf/RhoTypes.proto`, `models/src/main/protobuf/CasperMessage.proto`,
`.github/workflows/ci.yml`. The normative Rholang grammar for Surface-L conformance is
`rholang-rs/rholang-tree-sitter/grammar.js`.

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

#### Evidence

Quote actual numbers. The RED, the measurement, the acceptance matrix. Tag each **DERIVED** /
**MEASURED** / **CITED** / **UNVERIFIED**.
````

**The matching index row** in `docs/consensus/register.toml`:

```toml
[[entry]]
id       = "CBR-0NN"
surface  = "N"                      # "N" = f1r3node · "L" = mettail-rust
commits  = ["sha1", "sha2"]
status   = "LANDED"                 # LANDED | IN_FLIGHT | OPEN
direction = "CORRECTIVE"            # REGRESSIVE | PERMISSIVE | CORRECTIVE | NEUTRAL | DIVERGENT
grade    = "WITNESSED"              # WITNESSED | MECHANISM_ONLY | LATENT | DORMANT
                                    # | NEUTRALITY_MEASURED | UNVERIFIED
axes     = { value = "MOVES", verdict = "MOVES", bytes_bincode = "NO",
             bytes_prost = "NO", post_state_hash = "MOVES",
             acceptance = "NO", metering = "NO" }
```

**An exemption row:**

```toml
[[exempt]]
commit   = "sha"
reason   = "BYTE_NEUTRAL_MEASURED"  # closed enum — see §3.2
evidence = "models/tests/wire_encode_differential.rs, 13/13, goldens unchanged"
```

---

## Appendix B — the exemption table

Every commit in `REGISTER_BASE..dc383ed1` touching the consensus-critical path set, that is **not** a
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
| 16 | `892b74e8` | a scratch directory that cleans up without a `Drop` that never runs | `INFRA` | Rust runs no destructors for statics. Measured: 20 tests → 20 directories → 72 MB of tmpfs; after, zero bytes. Test infrastructure only. **CITED**. |
| 17 | `779bf881` | the oracle's citations become checked, and two of them were wrong | `TESTS_ONLY` | "VERBATIM" overclaimed on 22 of 23 blocks; one block carried an undocumented hand edit. *"the block's meaning is unchanged and the differential's results stand."* **CITED**. |
| 18 | `b9aaa3d4` | the full bound-site enumeration, and a correction | `DOCS_ONLY` | Documentation in `cold_store_decode.rs`. |
| 19 | `96ca51a0` | leg-2 execution record — three falsification experiments | `DOCS_ONLY` | Audit record plus a test-corpus edit. Records that F2 (*"an owned `Env` per work item is acceptable"*) was **REFUTED** by measurement. **CITED**. |
| 20 | `550b967a` | leg-2 stage A — harness prerequisites and the canonical child-slot table | `TESTS_ONLY` | Adds `substitute_descends_into` as *"a checkable record"*; the record's content is reproduced verbatim from production, and *"Reproducing that verbatim is a requirement, not an oversight: descending would change substituted bytes, hence signed bytes."* **CITED**. |
| 21 | `caadf839` | stage 1 — close the coverage gap that let the bare-element key defect survive | `TESTS_ONLY` | Test module only. Produced the witnesses that made **CBR-010** reviewable, including *"asking for the bare `1` returns the singleton list `[1]` — a wrong ANSWER, not a miss."* **CITED**. |

⚠ **This table covers Surface N only.** Surface L is not yet an exact partition; see §6.5.
