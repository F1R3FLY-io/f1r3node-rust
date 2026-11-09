# The theory seam

A [theory](glossary.md#theory) gives a native space its data rules ([decision 9](decision-register.md#decision-9), item 1).
A factory call names it with `free Th()`, and the space then admits only the values of the theory.
This document explains the design of the seam in phase P7, step 4.
It tells what a theory is, how the node checks it, what it costs, and why each part has its form.
The [specification](specification.md#5-theories) states the normative rules TH-1 to TH-10, and this document cites them.
The [F1r3lang integration](f1r3lang-integration.md) document states how installed theories replace the builtins after the F1r3lang merge.

## Contents

1. [What a theory gives a space](#1-what-a-theory-gives-a-space)
2. [The parts of the seam](#2-the-parts-of-the-seam)
3. [The data flow](#3-the-data-flow)
4. [The builtin theories](#4-the-builtin-theories)
5. [The identity and its bytes](#5-the-identity-and-its-bytes)
6. [Resolution](#6-resolution)
7. [The admission of a send](#7-the-admission-of-a-send)
8. [The check at a match](#8-the-check-at-a-match)
9. [Charges](#9-charges)
10. [Leases and test checkers](#10-leases-and-test-checkers)
11. [Security properties](#11-security-properties)
12. [Evidence](#12-evidence)
13. [References](#references)

## 1. What a theory gives a space

The FIPS describes a theory as the set of data that processes may send on the channels of a space [[FIPS-RRS](#ref-fips-rrs)].
Without a theory, a space accepts every value, as the default space does.
With a theory, a send of a value outside the theory aborts the deploy.
A receive never takes a stored datum that the theory does not prove.

Table 1 shows the effect of the builtin theory `Nat` on a bag space.
The examples come from the native suite `rholang/tests/reified_theory.rs`.

*Table 1. Sends and receives on a space with the theory `Nat`.*

| Program text | Result |
|---|---|
| `a!(0)`, `a!(9223372036854775807)`, `a!(18446744073709551616n)` | Stored. A receive takes each value. |
| `a!(-1)`, `a!(-1n)` | The deploy aborts with `TheoryRefuted`, and the error names `Nat` and the index 0. |
| `a!("Nat(42)")`, `a!(1 \| 2)`, `a!(Nil)`, `a!([1])` | The deploy aborts with `TheoryRefuted`. |
| `a!(1, -1)` | The deploy aborts with `TheoryRefuted` at the index 1, and the space stores nothing. |
| `space!?("produce", *a, [-1])` | The handle call aborts the deploy with `TheoryRefuted`, as a routed send does. |

A theory changes nothing for the data that it admits.
On each racing runtime, the family programs of the native suites run with `free Nat()` and without a theory (test `space_theory_admission`).
Each play delivers an allowed label set ([decision 93](decision-register.md#decision-93)).

*Changed by [decision 93](decision-register.md#decision-93): before, the family programs delivered the same values with `free Nat()` as without a theory. Racing play varies the deliveries of one program.*

## 2. The parts of the seam

The seam has five parts.
Each part has one job, and the next sections explain them.

*Table 2. The parts of the theory seam.*

| Part | Job | Code |
|---|---|---|
| [Theory identity](glossary.md#theory-identity) | The content that names a checker: no handle, token, or epoch | `spaces/theory/identity.rs:59-76` |
| Checker | The code of this build that decides the values of a theory: the trait `SpaceTheory` | `spaces/theory/mod.rs:101-118` |
| [Admission verdict](glossary.md#admission-verdict) | The result of one check: `Proven`, `Refuted`, or `Undetermined`, with its usage | `spaces/theory/mod.rs:64-77` |
| Admission | The check of each value of a send, before any staging | `spaces/theory/mod.rs:140-163` |
| Check at a match | The check of each stored datum of a match, after the `where` guard | `spaces/generic.rs:2274-2293` |

The shapes follow the neutral theorem channel of F1r3lang, so the merge of phase P12 maps an installed theory onto the same trait [[F1R3LANG-THEOREM](#ref-f1r3lang-theorem)].
F1r3lang calls the verdict `AdmissionDecision`, the usage `AdmissionUsage`, and the limit profile `AdmissionBudget`.
Its structural admission also costs one work unit.

## 3. The data flow

Figure 1 shows the three paths through the seam: the factory gate, the admission of a send, and the check at a match.
A failure on any path aborts the deploy before its commit, except a refusal at a match, which acts as a guard refusal.

![The theory seam as an activity. A factory call with free Th(args) passes gate 4 when Th is a builtin without arguments, and the config slot "theory" then holds one GByteArray with the bytes of the builtin identity. A builtin with arguments fails with BadConstructorArgument, and another name fails with UnknownTheory and the nearest builtin. A send reads the config, decodes the identity bytes and resolves them, or fails with TheoryUnavailable. The checker then admits each value in order before any staging, and the first refuted or undetermined value aborts with TheoryRefuted or TheoryUndetermined and its index. A receive reads and resolves the config, finds the first candidate that matches spatially, and checks the where guard. After the guard, the checker checks each stored datum of the match. A refusal of either is a guard refusal: a scan receive parks, a head blocks, and a join keeps its search. A commit with two or more leases fails with NestedCommitGuard.](diagrams/10-theory-admission.svg)

*Figure 1. The theory seam: the gate of a factory call, the admission of a send, and the check at a match (`diagrams/10-theory-admission.puml`).*

## 4. The builtin theories

A builtin decides a value by its shape alone (rule TH-1).
The test $`\mathrm{sole}(v)`$ holds when the value $`v`$ has exactly one expression, every other field of $`v`$ is empty, and $`v`$ uses no connective (`spaces/theory/builtin.rs:84-118`).
Then the kind of the one expression decides:

```math
\begin{aligned}
\mathrm{Nat}(v) &\iff \mathrm{sole}(v) \wedge \big(v = \mathrm{GInt}(n),\ n \ge 0 \;\vee\; v = \mathrm{GBigInt}(b),\ b \ge 0\big) \\
\mathrm{Int}(v) &\iff \mathrm{sole}(v) \wedge v \in \{\mathrm{GInt}, \mathrm{GBigInt}\} \\
\mathrm{String}(v) &\iff \mathrm{sole}(v) \wedge v = \mathrm{GString}(s) \\
\mathrm{Bool}(v) &\iff \mathrm{sole}(v) \wedge v = \mathrm{GBool}(x) \\
\mathrm{Any}(v) &\iff \mathrm{true}
\end{aligned}
```

The bytes of a `GBigInt` are two's complement in big-endian order.
So its sign is the top bit of its first byte, and the empty encoding is 0.
The legacy branch checked a theory with a prefix of the text of a value, so it accepted the string `"Nat(42)"` as a natural number.
The shape test refuses that string, and the test `theory_nat_rejects_minus1_string_par` pins the refusal.

A builtin answers in one structural step, so it never needs more work than its limit profile gives.
Its verdict is `Proven` or `Refuted`, never `Undetermined`, unless the limit profile gives less than one work unit (section [9](#9-charges)).

## 5. The identity and its bytes

The config of a space holds the identity of its theory, and never a reference to process-local state (rule TH-2).
A handle, a token, or an epoch exists only in one process.
A restarted node, a validator in replay, or a node that attaches later could not use it.
The identity holds content only, and every node computes the same checker from it.

*Table 3. The fields of a theory identity, and the values of a builtin.*

| Field | Meaning | Builtin value |
|---|---|---|
| `version` | The version of the identity format | 1 |
| `languageFingerprint` | The language of the theory | `"rrs:builtin"` |
| `category` | The category of the checked terms | `"Proc"` |
| `categoryName` | The name of that category | `"Proc"` |
| `theoremId` | The theorem that the checker decides | `"Nat"`, `"Int"`, `"String"`, `"Bool"`, or `"Any"` |
| `checkerAbi` | The calling contract of the checker | `"rrs:structural:v1"` |
| `limitProfile` | The work units and the evidence bytes of one check | 1 unit and 0 bytes |
| `args` | The arguments of the theory | None |

**The bytes (decision 74).** The config slot `"theory"` holds `Nil` or one `GByteArray` with the canonical bytes of the identity ([decision 74](decision-register.md#decision-74)).
The bytes hold the fields in the order of Table 3 (`spaces/theory/identity.rs:128-163`).
The version is an `i64` and each limit is a `u64`, in big-endian form.
Each string and each argument comes after its length, which is a big-endian `u32`, and the arguments come after their count.

For the identity of `Nat`, the encoding has 87 bytes.
The version takes 8 bytes, the five strings take 15, 8, 8, 7, and 21 bytes, the limits take 16, and the count takes 4.

The decoder reads the fields in order, rejects a short or a long input, and decodes each argument with prost (`spaces/theory/identity.rs:169-202`).
Then it encodes the decoded identity again and compares the result with its input.
So the decoder accepts exactly the image of the encoder, and one identity has exactly one form in the config.

```text
FROM_BYTES(b):                                     ▷ identity.rs, TheoryIdentity::from_bytes
  r ← a reader over b
  version ← i64 of r.take(8)                       ▷ any short input fails here or below
  language, category, categoryName, theoremId, checkerAbi ← five r.string()
  work, evidence ← u64 of r.take(8), u64 of r.take(8)
  count ← r.length()
  args ← count times: Par::decode(r.take(r.length()))
  identity ← (version, …, args)
  if r has bytes left or TO_BYTES(identity) ≠ b: reject   ▷ no trailing byte, no other form
  return identity
```

**Why bytes, and not the map of design v3.** Design v3 put the identity in the config as a map with eight string keys and a nested limits map.
At that time, the strict metered decoder read the config of every space operation, and the native cost model charged each byte of that read.
The map made each read of a space with a theory 169 % more expensive, so many family programs exceeded the replay limit of $`2^{28}`$ `VerificationBytes`.
One `GByteArray` cost about 5 % more, and the user chose it on 2026-10-09.
[Decision 89](decision-register.md#decision-89) retired that charge, so a config read now charges nothing (rule NE-2), and the bytes stay as a state format.

The `config` reply of a handle still shows the identity as the map of Table 3, so a program sees the same value as before.

## 6. Resolution

Each config read decodes the identity and resolves it to a checker of this build (rules TH-3 and TH-7).
This build resolves only the structural builtins: the builtin values of Table 3 for every field except `limitProfile`, which may have any value (`spaces/theory/builtin.rs:139-153`).
Every other identity fails with `TheoryUnavailable`, which names its theorem id (`spaces/theory/mod.rs:123-132`).

The resolution is a function of the identity alone.
It reads no registry, cache, or other state of the node.
So a restarted node, a validator in replay, and a node that attaches later all resolve one identity to one checker.
A test build also resolves the test checkers of section [10](#10-leases-and-test-checkers), and a production build cannot reach them.

## 7. The admission of a send

On a space with a theory, the checker admits each value of a send in order (rule TH-5).
The admission runs after the config read and the gates of the send: the key, the payload, and the params.
It runs before any staging or counter bump, so a refused send changes no state and emits no event (`spaces/generic.rs:4972-5065`).

```text
ADMIT(theory, values):                             ▷ theory/mod.rs, admit_values
  leases ← []
  for i, v in values, in order:
    verdict ← theory.admit(v)                      ▷ the check charges nothing (TH-8)
    if verdict = Proven:     leases ← leases ++ theory.commit_lease()
    if verdict = Refuted:    abort TheoryRefuted(theory, i)
    if verdict = Undetermined: abort TheoryUndetermined(theory, i)
  return leases
```

The admission covers a routed send, the handle `produce`, and each round of a re-issued send.
A second send of Rule E, a re-store, and a re-offer carry data that a send admitted, so they are not admitted again.

**The place of the admission is tested.** A test can observe a refused send only by its error, because an aborted transaction discards its staged work.
So the test `theory_admission_precedes_any_staging` checks the order of the gates.
A send that fails at the params gate fails there, whatever its values, and a refuted send leaves no record and no event.
Before [decision 89](decision-register.md#decision-89), the test also bounded the host work of a refused send.
The dry run of an admission after the counter bump failed that bound (section [12](#12-evidence)).

## 8. The check at a match

After the `where` guard admits a match, the checker checks each stored datum of the match with the values that its bind captures (rule TH-6).
A verdict other than `Proven` is a guard refusal, so each discipline treats it as it treats a refusing guard under [decision 37](decision-register.md#decision-37).
A scan receive parks, the head of an ordered collection blocks, and a join keeps its search.

**Stored data and data in flight.** The checker checks only stored data.
The datum of a send is in flight, and so is a record that a cascade, a lift, or a re-offer carries.
Its send admitted it, so a check would add only work.
The enum `Held` names the two cases at each site that matches one datum (`spaces/generic.rs:2246-2264`).

*Table 4. The sites of the check at a match.*

| Site | Stored data that the checker checks | Code |
|---|---|---|
| Head driver: queue, cell | The head record of a receive | `Cx::matches` with `Held::Stored` |
| Scan: bag, set | The first record that binds the pattern. A set class is one value, so its first member decides the class. | `spaces/disciplines/scan.rs:409-413` |
| Stack settle | The top record, unless the settle carries it in flight | `Held` from `BoundRecord` |
| Priority queue | The stored level head of a receive or of a settle | `stored_match` in `priority_queue.rs` |
| VectorDB | Each stored member of a list, and the record of a single-mode receive | `Cx::admits_records` |
| Join engine | Each stored entry of the bound entries of a join | `Engine::admits` with `JoinSource::stored_payload` |

On a PathMap space, the checker checks the own payload of the datum, not its re-rooted value.
The theory admitted the payload as the send carried it, and the re-rooted value of a prefix receive is a list that `Nat` refutes.
The test `space_guard_match` pins this case with a prefix receive on a space with `free Nat()`.

The internal COMMs of the plans, such as the counter bumps, the retires, and the lifts, use `Cx::bind` and never reach the checker.
A dry run that moved the check into `bind` failed three tests, because the checker then refused the datum of a send in flight.

**Leases.** A check takes no lease.
The fire of a match takes one lease for each stored datum that the check proved (`SpaceTheory::commit_lease`, `Cx::lease_stored`).
The commit must revalidate each lease under the authority of its language (rule TH-9).

## 9. Charges

A theory check adds no [phlo](glossary.md#phlo) (rule TH-8).
The introduction bytes of a send already pay for its values.

Each check of a builtin reports a usage of one work unit and no evidence bytes (`BuiltinTheory::check`, `spaces/theory/builtin.rs`).
A check at a match reports one unit for each stored datum, whatever the number of its values.
The usage is not metered ([decision 89](decision-register.md#decision-89)), so no check reserves or charges anything.
A limit profile of zero units gives `Undetermined` before the read, so a starved check fails closed.

The usage depends only on the identity and the value.
So it is the same on a hit and a miss, and in play and replay.
The cache-free runtime holds no cache, and the headline `usage_cache_transparent` proves that a later cache must keep this property.

**The cost of the identity.** The identity adds no read charge (rule NE-2), and the config slot holds one compact `GByteArray` (test `theory_config_slot_is_one_compact_par`).
For the measurement before [decision 89](decision-register.md#decision-89), see [decision 74](decision-register.md#decision-74).

## 10. Leases and test checkers

A theory may give a commit lease (`SpaceTheory::commit_lease`).
The fire of a match takes one lease for each stored datum that the check proved.
The commit must revalidate each lease under the authority of its language (rule TH-9).
The acquire, the revalidation, and the release of a lease belong to the guard composition of phase P12 ([F1r3lang integration](f1r3lang-integration.md)).
Until then, a transaction with two or more leases aborts with `NestedCommitGuard` before its commit (`spaces/generic.rs:4504-4509`).
The builtins attach no lease, so no program of this build reaches that abort.

The checkers that test the lease, the refusal at a match, and the starved limit profile exist only in test builds (rule TH-10).
`TestBehavior::Leasing` proves every datum and attaches a lease, and `TestBehavior::RefuteMatch` refutes every stored datum at a match.
`TestBehavior::RefuteValue` refutes every value of a send.
The library tests of `spaces/theory` and `spaces/generic.rs` use them.

## 11. Security properties

**Fail closed.** Every verdict other than `Proven` refuses: at a send it aborts the deploy, and at a match it acts as a guard refusal.
An identity that names no checker of this build fails every operation on its space with `TheoryUnavailable`.

**No process-local state in consensus.** The config holds only the content of the identity, so no node-local handle, token, or epoch enters a state root.
A replayed block resolves the same checker from the same bytes.

**No forged identity.** Only gate 4 of a factory writes an identity, and it writes only the builtin identities.
The config record lives on an unforgeable channel of the space, so a program cannot write another identity there.

**Bounded work.** The limit profile of an identity bounds the work units of each check, and a check adds no phlo (rule TH-8).
A refused value stops the admission at once.

## 12. Evidence

*Table 5. The formal and test evidence of the theory seam.*

| Claim | Formal evidence | Tests |
|---|---|---|
| TH-1: the builtins decide by shape, for one unit | `builtin_decidable_structural` (`TheoryAdmission.v`) | `prop_theory_builtin_decidable`, `theory_nat_accepts_bigint_nonneg`, `theory_nat_rejects_minus1_string_par` |
| TH-2a: the identity bytes | `identity_bytes_roundtrip` (`TheoryAdmission.v`) | `prop_theory_identity_bytes_roundtrip`, `theory_config_slot_is_one_compact_par` |
| TH-3: resolution by content | `resolve_identity_deterministic` (`TheoryAdmission.v`) | `prop_resolve_identity_deterministic`, `theory_unavailable_fails_each_config_read` |
| TH-4: gate 4 | `theory_gate_validated` (`SpaceFactory.v`) | `prop_theory_gate`, `theory_unknown_name_suggestion`, `theory_builtin_args_rejected` |
| TH-5, TH-9: fail closed | `fail_closed` (`TheoryAdmission.v`) | `prop_theory_fail_closed`, `theory_nested_commit_guard`, `theory_admission_precedes_any_staging` |
| TH-6: a refusal at a match is a guard refusal | `theory_refusal_is_guard_refusal` (`Match.v`) | `prop_theory_refusal_is_guard_refusal`, `space_guard_match`, `space_guard_match_refusal_is_a_guard_refusal` |
| TH-7: replay | `replay_preserves_theory_admission` (`TheoryAdmission.v`) | `prop_replay_preserves_theory_admission` |
| TH-8: charges | `usage_cache_transparent` (`TheoryAdmission.v`) | `prop_theory_usage_cache_transparent`, `theory_check_takes_no_lease_and_the_fire_takes_one_for_each_stored_datum` |

The [verification dossier](verification.md) gives the commands, the results, and the fourteen negative controls of the Rocq models.

## References

- <a id="ref-fips-rrs"></a>[FIPS-RRS] F1R3FLY.io. *Reifying RSpaces*. FIPS, approved 2025-09-26.
- <a id="ref-f1r3lang-theorem"></a>[F1R3LANG-THEOREM] F1R3FLY.io. The neutral theorem channel of F1r3lang: `AdmissionDecision`, `AdmissionUsage`, `AdmissionBudget`, and `STRUCTURAL_ADMISSION_WORK_UNITS` in `grammar-core/src/theorem.rs` of the mettail-rust repository.
