# 03 · Coverage-guided fuzzing with cargo-fuzz / libFuzzer

> *“If you didn't fuzz it, it's broken.”* —  Modern adage in
> systems security, generally attributed to Charlie Miller.

This chapter explains the role of coverage-guided fuzzing
(`cargo-fuzz` driving libFuzzer [Ser16]) in the slashing
methodology. Coverage-guided fuzzing is the **adaptive byte-level**
search arm: where proptest and Hypothesis sample inputs according to
a hand-written strategy, libFuzzer **observes which edges of the
compiled binary are exercised** and biases future mutations toward
inputs that reach new edges.

Organization:

- [§1 — When fuzzing is the right tool](#1--when-fuzzing-is-the-right-tool)
- [§2 — The six slashing fuzz targets](#2--the-six-slashing-fuzz-targets)
- [§3 — The structure-aware contract](#3--the-structure-aware-contract)
- [§4 — Literate walkthrough of `slash_authorization_paths`](#4--literate-walkthrough-of-slash_authorization_paths)
- [§5 — From crash to source fix](#5--from-crash-to-source-fix)
- [§6 — Pitfalls](#6--pitfalls)
- [§7 — Related work](#7--related-work)

---

## 1 · When fuzzing is the right tool

Coverage-guided fuzzing dominates the alternatives when:

| Property                                           | Why fuzzing wins                                                                     |
|----------------------------------------------------|--------------------------------------------------------------------------------------|
| Byte-level boundaries (protos, hashes, signatures) | The fuzzer mutates bytes directly; structure-aware mutators preserve well-formedness |
| Code paths gated by complex predicates             | Edge-coverage feedback drives the fuzzer toward gates the random sampler would miss  |
| Continuous CI integration                          | A fuzz target runs indefinitely with `-runs=N` budgeting                             |
| Regression on a real crash                         | Crashes are minimized to bytes; replay is one command                                |

The methodology uses libFuzzer specifically for:

1. **Arithmetic envelope** — exhausting the `i32` / `i64` / `u64`
   boundary behavior beyond Kani's `i32` bound.
2. **Proto round-trip** — `from_proto ∘ to_proto = id` on
   `SystemDeployData::Slash` and on full `BlockMessage`.
3. **Structure-aware authorization** — driving the
   `validate_received_slash_deploys` path with structured `Block` +
   `SlashDeploy` pairs.
4. **Detector classification** — driving
   `EquivocationDetector::dispatch` with structured DAGs.
5. **Lifecycle traces** — driving end-to-end
   detect → record → propose → SlashDeploy with structured action
   sequences.

It is **not** used for:

- Properties about unbounded validator counts — use Rocq.
- Concurrency interleavings — use Loom or TLA⁺.
- Multi-step *semantic* properties — use Hypothesis (the fuzzer can
  generate the bytes, but Hypothesis's shrinking is better at action
  sequences).

---

## 2 · The six slashing fuzz targets

The targets live in [`fuzz/fuzz_targets/`](../../../../../fuzz/fuzz_targets/):

| Target                           | What it drives                                                            | Why it exists                                                                                                                                |
|----------------------------------|---------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------|
| `slashing_arithmetic.rs`         | `checked_base_seq`, `checked_next_seq`, `epoch_for_block_number`          | Extends the Kani harnesses to compositional paths libFuzzer can find that Kani's bound misses                                                |
| `slash_deploy_roundtrip.rs`      | `SystemDeployData::Slash` proto round-trip                                | The narrow surface lets the fuzzer isolate hash-length, public-key-length, and epoch-`i64` edges that `block_message_roundtrip` would buffer |
| `block_message_roundtrip.rs`     | Full `BlockMessage` proto round-trip                                      | Catch-all for proto field-encoding regressions                                                                                               |
| `slash_authorization_paths.rs`   | `validate_received_slash_deploys` with structured `Block` + `SlashDeploy` | Drives the authorization predicate Theorem T-9.8 covers; finds adversarial inputs that exercise each authorization clause                    |
| `equivocation_detector_paths.rs` | `EquivocationDetector::dispatch` with structured DAG                      | Drives the detector classification logic Bug #11 changed; the structured input exercises latest-message projection edge cases                |
| `slash_lifecycle_trace.rs`       | End-to-end candidate → SlashDeploy → effect                               | Drives the full pipeline with structured action sequences; catches integration regressions the per-component targets miss                    |

Plus `support.rs` for shared input-decoding helpers (parsing the
`Arbitrary`-derived structures into well-formed slashing inputs).

Two design rules govern this set:

1. **One target per *path*, not per *function***. A target like
   `slash_authorization_paths` is not "one function under test";
   it's the entire authorization path with the receive side driven
   by structured fuzz input.
2. **Maximum 60-second smoke**. Every CI smoke run executes each
   target for ≤ 60 seconds; longer campaigns are nightly only. This
   keeps PRs fast while still surfacing the most common crashes.

---

## 3 · The structure-aware contract

Naïve byte-level fuzzing of a slashing input would spend
99%+ of its time generating inputs the proto decoder rejects (bad
varint, wrong field tag, etc.). The fuzzer would never reach the
*interesting* code — the authorization predicate, the detector, the
lifecycle.

**Structure-aware fuzzing** [Rust Fuzz Book — SA] solves this by
constraining the fuzzer's mutations to the shape of the input.
libFuzzer's `Arbitrary` trait does this:

```rust
#[derive(arbitrary::Arbitrary, Debug)]
struct SlashAuthorizationInput {
    block_number: i64,
    epoch_length: i32,
    offender: ValidatorChoice,            // a small enum
    invalid_block_hash: BlockHashChoice,  // a small enum
    proposer: ValidatorChoice,
    bond_map: SmallBondMap,               // a constrained map
}
```

The fuzzer mutates each field within its type's valid range, then
calls the function under test. The validity of the *encoding* is
guaranteed by `Arbitrary`; the validity of the *semantics* is what
the fuzzer is searching for.

### 3.1 Why this matters for slashing

The slashing authorization predicate is a four-clause conjunction:

```
authorized(deploy) ≜
    current_epoch(deploy) ∧
    matching_evidence_epoch(deploy) ∧
    positive_bond(deploy.offender) ∧
    invalid_block(deploy.invalid_block_hash)
```

Each clause has a different failure mode. Naïve fuzzing might never
reach the third clause because the first or second already rejected
the input. Structure-aware fuzzing biases toward inputs where the
first three clauses hold but the fourth fails — exactly the
adversarial shape an attacker would use.

### 3.2 The cost

Structure-aware fuzzing pays for itself in coverage but adds
maintenance overhead: every change to the proto or to the
authorization signature requires updating the `Arbitrary`
derivation. The methodology mitigates this by:

1. Keeping `support.rs` thin and well-documented.
2. Requiring every public slashing function under fuzz to have a
   short comment block citing the corresponding `Arbitrary` type.
3. Re-running the fuzz smoke on every PR.

---

## 4 · Literate walkthrough of `slash_authorization_paths`

The most intricate target is
[`fuzz/fuzz_targets/slash_authorization_paths.rs`](../../../../../fuzz/fuzz_targets/slash_authorization_paths.rs).
It exercises every clause of Theorem T-9.8's authorization predicate
under adversarial structured input.

### 4.1 The target in literate form

```
target slash_authorization_paths:
    fuzz_target(input : SlashAuthorizationInput):
        let snapshot     ← build_snapshot_from(input)
        let block        ← build_block_from(input)
        let result       ← validate_received_slash_deploys(snapshot, block)

        (* property 1: no panic — every adversarial input is handled cleanly *)
        (* property 2: result matches predicate evaluated independently *)

        let predicate_eval ← independently_evaluate(
            current_epoch(input.block_number, input.epoch_length),
            evidence_epoch(input.invalid_block_block_number, input.epoch_length),
            input.bond_map[input.offender] > 0,
            is_invalid(input.invalid_block_hash)
        )

        assert result.is_authorized ⇔ all_four_clauses(predicate_eval)
```

### 4.2 The shape of the property

The property is a **double-implication** between the actual
authorization decision and the four-clause conjunction. The fuzzer
searches for inputs where the two disagree.

This is the *executable companion* of Theorem T-9.8 (see
[`../../slashing-verification.md §9.14`](../../slashing-verification.md)):

> *T-9.8: `received_slash_deploy_authorized(d)` ⇔
> `(current_epoch(d) ∧ matching_evidence_epoch(d) ∧
> positive_bond(d.offender) ∧ invalid_block(d.invalid_block_hash))`.*

The Rocq theorem proves the equivalence; the fuzz target re-validates
it on every CI run with a coverage-guided search over the structured
input space.

### 4.3 Why this is not subsumed by Kani

Kani proves the *predicate* over the bounded primitive domain. The
fuzz target validates the *path* — the actual `validate_received_slash_deploys`
function that consumes a `BlockMessage`, looks up the snapshot,
walks the slash candidates, and rejects unauthorized ones. The two
artifacts cover complementary layers of the same theorem.

---

## 5 · From crash to source fix

A crash discovered by libFuzzer follows a deterministic pipeline:

```
algorithm process_fuzz_crash(input : FuzzInput, crash_log : CrashLog):
    ▸ 1. minimize input via `cargo fuzz tmin <target>`
            → minimized.bin
    ▸ 2. reproduce deterministically:
            `cargo fuzz run <target> minimized.bin`
    ▸ 3. classify under threat-model vocabulary (§4 of threat model)
    ▸ 4. trace into Rust production path (witness rule)
    ▸ 5. write a deterministic Rust regression test that ingests
         minimized.bin verbatim:
            casper/tests/slashing/fuzz_crash_NN_<name>.rs
    ▸ 6. fix Rust source if classification = confirmed_current_bug
    ▸ 7. re-run target to confirm fix and to flush adjacent crashes
    ▸ 8. record in slashing-traceability.md
    ▸ 9. add minimized.bin to fuzz/corpus/<target>/ to seed future runs
```

Step 5 is the **bridge** between the fuzz artifact and the
deterministic regression. The methodology treats the fuzz crash as
the *witness* and the Rust regression as the *promotion*; the witness
itself does not run in CI — the regression does.

### 5.1 The corpus discipline

Every minimized crash is committed to `fuzz/corpus/<target>/`.
libFuzzer reads this corpus on startup and uses it as seed inputs.
This **carries the institutional memory** across CI runs: a crash
found in week 1 keeps being explored in subsequent weeks, often
finding adjacent crashes the original search missed.

---

## 6 · Pitfalls

### 6.1 Pitfall: the target panics on infeasible input

A target that calls `unwrap()` on a fuzzer-generated input panics
on infeasible inputs; libFuzzer treats every panic as a crash, so
the fuzzer spends most of its time minimizing irrelevant panics.

**Mitigation**: every fuzz target in this development uses
`.expect("…")` with a descriptive message **inside the `support`
helpers** that decode fuzzer input into typed values, and uses
`if let Some(x) = …` (or `?`) inside the target body. Real panics
in the function under test are kept; cosmetic panics in input
decoding are eliminated.

### 6.2 Pitfall: ASAN noise on `Arc` / `Rc`

AddressSanitizer flags some patterns of `Arc<Mutex<…>>` reuse that
are not bugs but are flagged as data races. The fuzz smoke runs with
`ASAN_OPTIONS=detect_leaks=0` for this reason — leak detection on
shared-immutable structures is too noisy in this codebase.

**Mitigation**: documented in
[`../../slashing-search-horizon.md §4`](../../slashing-search-horizon.md);
the command line is identical for every fuzz target so noise is
consistent.

### 6.3 Pitfall: target does not actually exercise the path

A target that does setup work but then drops the relevant `Block`
without dispatching it exercises nothing interesting. The fuzzer's
coverage feedback will plateau and no new edges will be found.

**Mitigation**: every fuzz target in this development has a sanity
test (`cargo fuzz cmin <target>`) that compares the corpus
edge-coverage before and after a one-minute run; a target with zero
coverage gain is flagged for review.

### 6.4 Pitfall: target compiles different code than production

A target that builds against a `#[cfg(fuzzing)]`-altered version of
the function exercises a different code path than production. A
crash found in the altered path may not reproduce in production.

**Mitigation**: the slashing fuzz targets do **not** use
`#[cfg(fuzzing)]` in the function under test. They use it only in
the input-decoding helpers (`support.rs`).

---

## 7 · Related work

- **libFuzzer**: Serebryany [Ser16].
- **AFL**: Zalewski [Zal18] — the spiritual ancestor of all
  coverage-guided fuzzing.
- **Structure-aware fuzzing**: Padhye *et al.* [PLPS19] (JQF / Zest)
  and the Rust Fuzz Book [RustFuzz].
- **Minimization (`tmin` / delta debugging)**: Zeller [Zel02].
- **Coverage-guided fuzzing of consensus protocols**: Yakdan *et al.*
  [Yak18] — survey of fuzz programs against blockchain stacks.

DOIs in [`../references.md`](../references.md).

---

## 8 · Next chapter

[`04-concurrency-interleaving-loom.md`](./04-concurrency-interleaving-loom.md)
— the concurrency-permutation arm of the methodology. Loom
exhaustively explores the permitted memory-model orderings of a
concurrent program, the only feasible way to catch the lost-update
race the lock-free tracker exhibited.
