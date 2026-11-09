# F1r3lang integration

[F1r3lang](glossary.md#f1r3lang) will replace the Tree-Sitter frontend of the node with the MeTTaIL Rholang 1.4 frontend, and it brings installed languages and theories.
The merge of F1r3lang into this branch is phase P12, and it waits for the readiness signal of the user ([decision 3](decision-register.md#decision-3)).
This document states how the Reified components meet F1r3lang at that merge.
P7 step 4 writes the part on the [theory seam](theory-seam.md), and P12 adds the conflict resolutions and the Rholang 1.4 productions.

## Contents

1. [Status](#1-status)
2. [What F1r3lang provides for theories](#2-what-f1r3lang-provides-for-theories)
3. [How the seam of this build maps onto F1r3lang](#3-how-the-seam-of-this-build-maps-onto-f1r3lang)
4. [What changes at the merge](#4-what-changes-at-the-merge)
5. [What does not change](#5-what-does-not-change)
6. [Open items for phase P12](#6-open-items-for-phase-p12)
7. [References](#references)

## 1. Status

*Table 1. The parts of this document.*

| Part | Status |
|---|---|
| The theory seam (sections 2 to 6) | Written in P7 step 4. The seam of this build has the shapes of the F1r3lang theorem channel, and its builtins stand in for installed theories. |
| The conflict resolutions of the merge | Planned in P12, after the readiness signal (`rrs-f1r3lang-merge`) |
| The Reified productions of Rholang 1.4 | Planned in P12 (`rrs-f1r3lang-rholang14-productions`) |
| `produce_guarded` and the `Match` trait | Planned in P12 (`rrs-f1r3lang-produce-guarded-match`) |

This campaign does not implement F1r3lang, its where-clauses, or its installed theories ([decision 37](decision-register.md#decision-37)).
The F1r3lang owner implements them, and this campaign adapts the Reified components at the merge.

## 2. What F1r3lang provides for theories

F1r3lang separates two judgments.
Grammar recognition proves that a reflected value belongs to a syntactic category.
Stateful admission is a second judgment: a channel is indexed by an installed language and a theorem, and a mutation needs a checked certificate [[F1R3LANG-THEOREM](#ref-f1r3lang-theorem)].

*Table 2. The parts of the F1r3lang theorem channel.*

| Part | Role |
|---|---|
| `AdmissionChecker` | The trait of a checker of one theorem |
| `AdmissionDecision` | The verdict of a check: `Proven`, `Refuted`, or `Undetermined`, each with an `AdmissionUsage` |
| `AdmissionBudget` | The limits of a check: work units and evidence bytes. `AdmissionBudget::structural()` gives the structural budget. |
| `STRUCTURAL_ADMISSION_WORK_UNITS` | The work of one structural check: one unit |
| `AdmissionCertificate`, `ProvenAdmission` | The evidence of a `Proven` verdict, which the commit uses |
| `TheoremChannelKernel` | The transaction kernel of one theorem-indexed channel. `prepare_produce` checks, and `commit_produce` and `commit_consume` run their callbacks while the installed-language epoch and the channel-authority epoch are read-locked. |

A failed check never invokes the commit callback, so the callback is the single mutation boundary of an adapter.
The kernel owns no tuple-space persistence, so a space adapter must keep its own records.

## 3. How the seam of this build maps onto F1r3lang

The seam of this build uses the shapes of Table 2, so the merge can map an installed theory onto the trait `SpaceTheory` ([theory seam](theory-seam.md#2-the-parts-of-the-seam)).

*Table 3. The theory seam of this build and its F1r3lang counterpart.*

| This build | F1r3lang | Note |
|---|---|---|
| `TheoryVerdict` (`spaces/theory/mod.rs:64-77`) | `AdmissionDecision` | The same three verdicts. An `Undetermined` verdict reports the usage of the check, as F1r3lang does. |
| `TheoryUsage` (`spaces/theory/mod.rs:35-40`) | `AdmissionUsage`, `AdmissionBudget` | One type holds the usage of a check and the limits of an identity. |
| `SpaceTheory::admit` and `check_match` | `AdmissionChecker` through the kernel | The space agent calls the checker itself, because a space is not an `ISpace` implementor ([I-1](decision-register.md#i-1)). |
| One reserved unit for each builtin check | `STRUCTURAL_ADMISSION_WORK_UNITS = 1` | The same work for a structural check |
| `TheoryCommitLease` and `NestedCommitGuard` | The revalidation at the commit boundary | Section [4](#4-what-changes-at-the-merge) states the composition |
| The builtins `Nat`, `Int`, `String`, `Bool`, `Any` | Installed theories | The builtins stand in for installed theories until the merge |
| The identity bytes in the config ([decision 74](decision-register.md#decision-74)) | The content of an installed theory | No handle, token, or epoch is persisted |

## 4. What changes at the merge

**Installed theories.** After the merge, the first argument of `free Th(args)` must be the capability token of an installed theory.
Authority comes only from the token, and the call fails closed without it.
The factory persists the content identity that the token resolves to, and never the token, which is local to one process.
The wire form of `free Th(args)` does not change.

**The home of theory content.** The namespace reserves the KIND `T` (byte `0x54`) for canonical theory content, as $`E_0 \Vert T \Vert h`$ with the content hash $`h`$ ([specification 1.1](specification.md#11-spaces)).
After the merge, the resolution of an installed theory reads its content from consensus state, and never from a registry of the node.
The builtins keep their resolution from the code of the build.

**Charges.** An installed theory can need more work than one structural unit.
So its admission needs a rate in the cost schedule, which belongs to the cost-accounting owner, before `rrs-fu-mettail-theories` activates it.
Each check must still reserve its work before it reads, the same on a hit and a miss and in play and replay.

**Guard composition.** The combination point of the guards of a space transaction is `SpaceTxn::commit`, not `produce_guarded`, because a space is not an `ISpace` implementor.
The commit acquires the authority of the installed language once, revalidates every lease, applies the transaction, and releases the authority.
The leases come from the admission of a send, the fire of a match, and an FLT where-permit.
Until F1r3lang offers a revalidation that a space can call, a transaction with two or more leases aborts with `NestedCommitGuard` (rule TH-9 of the [specification](specification.md#5-theories)).

**Where-clauses.** From the merge on, a receive on a space follows the selection rule of F1r3lang ([decision 37](decision-register.md#decision-37)).
A guard failure then counts as a pattern non-match, so a receive takes the first candidate that matches and passes the guard.
The check of the theory at a match stays part of the guard, so it joins that selection.

## 5. What does not change

- The config slot holds `Nil` or the canonical bytes of the identity, and its version field leaves room for a new format.
- A verdict other than `Proven` fails closed: it aborts a send, and it refuses a match as a guard refusal.
- A datum in flight is checked at its send and not again at a match.
- The `config` reply of a handle shows the identity as a map.
- The ordinary terms keep their prost bytes, because the syntax of a theory is the protobuf field `EFreeBody` ([decision 112](decision-register.md#decision-112)).

## 6. Open items for phase P12

1. **Limit profiles.** F1r3lang names a limit profile by a string, such as `mettail-structural-theorem-limits/1`, and keeps the budget beside it.
   The identity of this build holds the budget itself.
   P12 decides whether a new identity version adds the name of the profile.
2. **Checker ABIs.** The builtins of this build use the ABI `rrs:structural:v1`, and the structural checker of F1r3lang uses `mettail-structural-theorem-checker/1`.
   P12 maps each ABI of an installed theory to its checker, and no ABI may resolve to two checkers.
3. **Evidence.** The builtins give no evidence, and F1r3lang can attach a certificate to a `Proven` verdict.
   The seam persists no certificate, because a check is a pure function of the identity, the value, and the budget, so replay checks again.
4. **The meaning of `free`.** F1r3lang uses `free` in its data definitions, and the Lookahead FIPS uses `free LambdaCalc()` as a language binder.
   P12 decides the meaning of `free` at the level of a process jointly with the F1r3lang owner.

## References

- <a id="ref-f1r3lang-theorem"></a>[F1R3LANG-THEOREM] F1R3FLY.io. *Source-neutral theorem certificates and typed-channel transaction protocol*: `grammar-core/src/theorem.rs` of the mettail-rust repository.
