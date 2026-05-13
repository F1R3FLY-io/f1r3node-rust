# 03 · Symbolic Rust with Kani

> *“The price of reliability is the pursuit of the utmost simplicity.”*
> — C.A.R. Hoare, *The Emperor's Old Clothes* [Hoa81].

This chapter explains the role of Kani in the slashing methodology.
Kani [VanH22] is a bounded model checker for Rust built on CBMC
[Kro03] (formally on top of MiniSAT [ES03] and Z3 [DMB08]). It
verifies **actual Rust source code** — not a Rocq translation, not
a TLA⁺ abstraction — against properties expressed as Rust
predicates. The contract is *unconditional* on the bounded input
domain, with zero counterexamples.

Organization:

- [§1 — When Kani is the right tool](#1--when-kani-is-the-right-tool)
- [§2 — The slashing Kani harnesses](#2--the-slashing-kani-harnesses)
- [§3 — Literate walkthrough of `checked_base_seq`](#3--literate-walkthrough-of-checked_base_seq)
- [§4 — Bounded vs. unbounded — the libFuzzer companion](#4--bounded-vs-unbounded--the-libfuzzer-companion)
- [§5 — Pitfalls](#5--pitfalls)
- [§6 — Related work](#6--related-work)

---

## 1 · When Kani is the right tool

Kani occupies a specific epistemic niche:

| Property class                                                           | Best tool   |
|--------------------------------------------------------------------------|-------------|
| Soundness of a single Rust function on a bounded input domain            | **Kani**    |
| Equivalence of a Rust function to a mathematical specification (bounded) | **Kani**    |
| Panic-freedom on a bounded input domain                                  | **Kani**    |
| Soundness under all schedules                                            | TLA⁺ / Loom |
| Soundness on an unbounded input domain                                   | Rocq        |
| Soundness on a structured byte-level input (parsers, protos)             | libFuzzer   |

Kani's strength is that the property is expressed as ordinary Rust
code (assertions inside a `#[kani::proof]` harness), and the
verifier proves the assertion holds **for every** input nondeterministically
chosen by `kani::any()`. The cost is bounded model-checking time
(typically seconds; sometimes minutes for complex bit-level
manipulation).

### 1.1 The slashing fit

The slashing port introduced one small but load-bearing module of
checked arithmetic and authorization predicates:
[`casper/src/rust/slashing_authorization.rs`](../../../../../casper/src/rust/slashing_authorization.rs).
The functions in this module:

- `checked_base_seq(seq_num: i32) -> Option<i32>` — predecessor of a
  validator's sequence number; the boundary `seq_num ≤ 0` is **the
  bug** fixed in commit `db0b979`. Returns `Option` (the `i32`
  invariants live in this signature unchanged).
- `checked_next_seq(max_seq: u64) -> Option<i32>` — successor with
  double-checked saturation (`u64::checked_add` then `i32::try_from`).
- `epoch_for_block_number(block_number: i64, epoch_length: i32) -> Result<Epoch, DomainError>`
  — floor-division with explicit `DomainError` routing
  (`InvalidEpochLength` for `epoch_length ≤ 0`,
  `NegativeBlockNumber` for `block_number < 0`). Returns the typed
  [`Epoch`](../../../../../casper/src/rust/epoch.rs) newtype.
- `slash_target_epoch_is_current(…)` and
  `slash_evidence_epoch_matches_target(…)` — return
  `Result<bool, DomainError>`. Propagate the `DomainError` from
  `epoch_for_block_number`.
- `received_slash_deploy_authorized(…)` — conjunctive predicate
  proven sufficient by Theorem T-9.8. Returns `Result<bool, DomainError>`.

These are pure functions of bounded-range inputs. They are exactly
the shape Kani exists for.

---

## 2 · The slashing Kani harnesses

The Kani harnesses live in a `#[cfg(kani)] mod kani_proofs` block
inside [`casper/src/rust/slashing_authorization.rs`](../../../../../casper/src/rust/slashing_authorization.rs).
The harness count and rationale:

| Harness                                                             | Property checked                                                                                                            |
|---------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------|
| `checked_base_seq_rejects_nonpositive`                              | `∀ s ≤ 0. checked_base_seq(s) = None`                                                                                       |
| `checked_base_seq_matches_positive_i32_predecessor`                 | `∀ s > 0. checked_base_seq(s) = Some(s − 1)`                                                                                |
| `checked_next_seq_matches_i32_successor`                            | `∀ m ≤ i32::MAX as u64. checked_next_seq(m) = Some(m + 1)`; `∀ m > i32::MAX as u64. = None`                                 |
| `epoch_for_block_number_rejects_invalid_domain`                     | `∀ n < 0 ∨ L ≤ 0. epoch_for_block_number(n, L) = Err(DomainError)`; also pins variant routing (`InvalidEpochLength` vs `NegativeBlockNumber`) |
| `epoch_for_block_number_matches_bounded_floor_division`             | `∀ n ≥ 0, L > 0. = Ok(Epoch::new(n / L))`                                                                                   |
| `slash_target_epoch_is_current_matches_epoch_projection`            | The epoch predicate matches the epoch projection on a bounded `(n, L)` domain                                               |
| `slash_evidence_epoch_matches_target_matches_epoch_projection`      | The evidence-epoch predicate matches the epoch projection on the same domain                                                |
| `received_slash_deploy_authorized_rejects_invalid_domain`           | Rejects inputs where any argument is out of its declared sub-range                                                          |
| `received_slash_deploy_authorized_is_conjunction_on_bounded_domain` | On the bounded domain, the predicate is exactly `(current_epoch ∧ matching_evidence_epoch ∧ positive_bond ∧ invalid_block)` |
| `slash_target_has_positive_bond_matches_positive`                   | `∀ b ∈ Bonds. slash_target_has_positive_bond(b) ⇔ b > 0`                                                                    |
| `received_authorization_requires_*_on_bounded_domain`               | Five harnesses, one per authorization clause, each proving the clause is *necessary*                                        |
| `slash_target_key_collides_matches_pair_equality`                   | Duplicate target rejection matches direct pair equality on a bounded validator domain                                       |

The full enumeration is in [`../slashing-search-horizon.md §4`](../../slashing-search-horizon.md);
the runner command is in
[`scripts/ci/slashing-search-horizon.sh`](../../../../../scripts/ci/slashing-search-horizon.sh).

### 2.1 Why one harness per property?

Each Kani harness exists to prove **one specific predicate**. A
harness that proves multiple predicates simultaneously has two
disadvantages:

1. **Counterexamples are confused** — when the proof fails, the
   engineer cannot tell which predicate failed without re-execution
   under a narrower harness.
2. **The cost of one slow check pollutes the others** — Kani must
   re-explore the full state space for every assertion.

A harness-per-property keeps each check ≤ 10 seconds in this
development, and aligns with the methodology's preference for
single-responsibility artifacts.

### 2.2 The discipline this enforces

The Kani harnesses act as **executable theorem specifications** for
the arithmetic and authorization helpers. They are not the *only*
defense against regression (the libFuzzer envelope and the integration
tests are additional layers), but they are the **highest-evidence**
layer — every input in the bounded domain is exhaustively considered.

A refactor that changes `checked_base_seq` to return `Some(0)` for
`s = 0` (a common off-by-one mistake) is caught at CI time by
`checked_base_seq_rejects_nonpositive`, with a concrete
counterexample (`s = 0`), in seconds. The Rust regression test that
would have caught the same bug is also present
([`pre_fix_bug_*.rs`](../../../../../casper/tests/slashing/pre_fix_bug_2.rs)),
but the Kani harness catches the *whole class* of inputs, not just
the specific one a developer happened to write.

---

## 3 · Literate walkthrough of `checked_base_seq`

The simplest harness illustrates the methodology end-to-end.

### 3.1 The function

```rust
/// Predecessor of a sequence number used as the *exclusive* lower bound for
/// self-justification walks. The boundary is `seq_num <= 0`, not `<= 1`:
/// sequence 1 is a valid genesis-child and must round-trip to `Some(0)`.
pub fn checked_base_seq(seq_num: i32) -> Option<i32> {
    if seq_num <= 0 {
        None
    } else {
        Some(seq_num - 1)
    }
}
```

The boundary `seq_num <= 0` is **the** load-bearing detail. A previous
version of this function used `seq_num <= 1` (off-by-one) and was
fixed in commit `db0b979`. The bug allowed an attacker to suppress
self-justification walks by manipulating sequence number 1.

### 3.2 The Kani harnesses (literate form)

```
harness checked_base_seq_rejects_nonpositive:
    let s ← kani::any() : i32
    kani::assume(s ≤ 0)
    assert checked_base_seq(s) = None

harness checked_base_seq_matches_positive_i32_predecessor:
    let s ← kani::any() : i32
    kani::assume(0 < s)
    kani::assume(s ≤ i32::MAX)        (* always true; explicit for clarity *)
    assert checked_base_seq(s) = Some(s − 1)
```

Read aloud:

> *“For every signed 32-bit integer `s`, if `s ≤ 0` then
> `checked_base_seq(s)` returns `None`; otherwise it returns
> `Some(s − 1)`.”*

### 3.3 Kani's algorithm

Kani translates the harness into a CBMC goto-program and asks the
SMT solver:

> *“Does there exist an `s : i32` such that the assertion fails?”*

The SMT solver explores all `2^32` candidate values symbolically.
If no value falsifies the assertion, Kani prints:

```
VERIFICATION:- SUCCESSFUL
```

If a value falsifies it, Kani prints the falsifying `s` and the
sequence of program steps that reaches the failed assertion. This is
a **proof** on the bounded `i32` domain — equivalent to running the
function with every possible 32-bit input and observing it never
violates the property, but completed in seconds because the SMT
solver prunes by structure.

### 3.4 The pseudocode pipeline from witness to action

```
algorithm verify_checked_base_seq:
    ▸ 1. write Kani harness covering ∀ s : i32
    ▸ 2. run `cargo kani --harness checked_base_seq_rejects_nonpositive`
    ▸ 3. match result:
         | VERIFICATION:- SUCCESSFUL     → property holds on bounded domain
         | VERIFICATION:- FAILED         → SMT model gives concrete s
                                          → reproduce in proptest
                                          → fix Rust source
                                          → re-run Kani
                                          → also write Loom test if concurrent
    ▸ 4. record verification result in scripts/ci/slashing-search-horizon.sh output
    ▸ 5. if a Rocq theorem exists for the same property, mark this Kani
         harness as the *executable witness* of that theorem
```

---

## 4 · Bounded vs. unbounded — the libFuzzer companion

Kani's exhaustion is **bounded** by the domain Rust types specify
(`i32`, `i64`, `u64`). The corresponding libFuzzer target,
[`fuzz/fuzz_targets/slashing_arithmetic.rs`](../../../../../fuzz/fuzz_targets/slashing_arithmetic.rs),
is technically narrower (libFuzzer is coverage-guided, not
exhaustive), but it extends the search to compositions:

| Layer       | What is covered                                                     | What is missed                                       |
|-------------|---------------------------------------------------------------------|------------------------------------------------------|
| **Kani**    | Every single value of the function's primitive-typed input          | Cross-function compositions, structural input shapes |
| **libFuzzer**| Coverage-guided exploration of compositions and structures          | Domain exhaustion (libFuzzer may miss the worst case)|
| **proptest**| Shrinking; small inputs first                                       | Domain exhaustion; deep cross-function chains        |

The methodology runs **all three** on the arithmetic helpers — Kani
for the per-function exhaustion, libFuzzer for the composition
coverage, proptest for the cross-system property-shrinking. The
costs are additive but each layer catches a class the others miss:

- Kani catches *“is the off-by-one fixed?”*
- libFuzzer catches *“does the composition of `checked_base_seq` and
  `checked_next_seq` ever produce an unexpected `None`?”*
- proptest catches *“does the harness's sequence-number plumbing ever
  pass an out-of-range value to either function?”*

Diagram 04 in [`../diagrams/`](../diagrams/04-tool-theorem-coverage.svg)
shows the coverage overlap.

---

## 5 · Pitfalls

### 5.1 Pitfall: implicit unbounded loop

Kani requires every loop to either have a bound annotation
(`#[kani::loop_invariant]`) or unwind to a small constant
(`#[kani::unwind(N)]`). A function that contains an unbounded loop
will silently fail to verify with a CBMC timeout.

**Mitigation**: the slashing arithmetic helpers contain no loops; the
authorization predicates are flat conjunctions. The methodology
restricts Kani harnesses to loop-free functions or functions with
explicit bounds. Functions with intrinsic loops (the detector's BFS,
the closure iteration) are left to TLA⁺ + Rocq.

### 5.2 Pitfall: over-trusting the bound

`i32::MAX = 2³¹ − 1`, so a Kani harness over `i32` exhausts
`2³² ≈ 4 × 10⁹` values. Real production traffic may exhibit
inputs that would not be `i32`-valued; if the upstream API silently
casts a `u64` to `i32`, the Kani harness covers the post-cast value
but not the pre-cast one.

**Mitigation**: every Kani harness in this development takes its
input type from the function's actual Rust signature, and every
upstream call site is audited to confirm the cast happens before, not
after, the function call. The libFuzzer envelope provides redundancy.

### 5.3 Pitfall: relying on the SMT solver's heuristics

The SMT solver inside Kani uses heuristics to prune. A property that
verifies in 10 seconds today may fail to verify in 10 minutes
tomorrow after a benign-looking refactor that changes the symbolic
structure.

**Mitigation**: the slashing development pins Kani to a specific
version (see [`fuzz/Cargo.toml`](../../../../../fuzz/Cargo.toml) for
version constraints) and re-runs all Kani harnesses on every CI run
so heuristic drift becomes a build failure rather than a silent
weakening.

### 5.4 Pitfall: confusing Kani with execution

Kani does not *execute* the function — it analyzes it symbolically.
A function with side effects (panics, prints, file I/O) is harder to
reason about than a pure function. The arithmetic helpers in this
development are deliberately pure for exactly this reason.

---

## 6 · Related work

- **Bounded model checking**: Biere *et al.* [BCC99].
- **CBMC**: Kroening *et al.* [Kro03].
- **Kani**: VanHattum *et al.* [VanH22] — Kani's design and case
  studies, including Amazon FireCracker.
- **Z3** (SMT solver under CBMC): de Moura & Bjørner [DMB08].
- **MiniSAT** (SAT solver under SMT): Eén & Sörensson [ES03].

DOIs in [`../references.md`](../references.md).

---

## 7 · Next chapter

[`04-finite-modeling-sage.md`](./04-finite-modeling-sage.md) — the
**generator** of witness candidates for the rest of the stack. Sage
is not a verifier; it is a structured search engine that emits
witnesses to be classified and either dismissed (model boundary) or
promoted (proof or regression).
