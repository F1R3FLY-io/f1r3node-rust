# Case study #1 — IgnorableEquivocation non-slashable (DOS vector)

## 1 · Summary

Pre-fix, `IgnorableEquivocation` (an equivocation observed but not
requested as a dependency by any other block) was **not** in the
`is_slashable()` taxonomy. A Byzantine validator could flood the
network with such blocks without economic cost — a denial-of-service
campaign against honest validators' verification CPU. Post-fix,
`IgnorableEquivocation` is slashable and follows the standard
record-and-slash path.

## 2 · Discovery technique

**Primary**: code-walking review of the `is_slashable()` taxonomy
(`block_status.rs:191`). The Scala counterpart at
`BlockStatus.scala:62-65` carried an explicit TODO comment naming
the DOS vector: *“Make IgnorableEquivocation slashable again ...
will become a DOS vector if not fixed.”*

**Corroborating**: Sage `differential_bisimilarity_model` produced
a Rust/Scala/oracle divergence witness for the canonical
ignorable-equivocation scenario; classified `permitted_bug_fix` and
cited in [`../sage-models/04-differential-and-bisimilarity.md`](../sage-models/04-differential-and-bisimilarity.md).

The Sage witness in
[`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md)
finding #16 corroborated the audit-found defect with a deterministic
3-validator scenario.

## 3 · Witness reproduction

Deterministic reproduction:

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_1
```

The fixture file
[`casper/tests/slashing/pre_fix_bug_1.rs`](../../../../../casper/tests/slashing/pre_fix_bug_1.rs)
encodes the minimum scenario: validator `v0` signs two distinct blocks
at sequence number 0; no other block cites the offending block (so
the equivocation is `Ignorable`). The pre-fix Rust path returns
`Status::Valid` with a log line; the post-fix path returns
`Status::IgnorableEquivocation` and creates an `EquivocationRecord`.

## 4 · Classification trace

```
threat_class       = permitted_bug_fix
                     (Rust deliberately diverges from Scala to fix
                      a documented DOS vector)
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_1.rs regression + post-fix
                     uc_03_ignorable_unrequested.rs anchor
```

Both the pre-fix regression and the post-fix anchor are in the
test suite; CI runs both.

## 5 · Evidence stack

| Layer            | Artifact                                                                                                          |
|------------------|-------------------------------------------------------------------------------------------------------------------|
| Rocq theorem     | `bug_fix_ignorable_safety`, `post_fix_ignorable_implies_equivocation` (`BugFixIgnorable.v`)                       |
| TLA⁺ invariant   | `Inv_TaxonomyCorrect` (`EquivocationDetector.tla`)                                                                |
| Rust regression  | `pre_fix_bug_1.rs`, `prop_t_9_1_ignorable_safety.rs`, `uc_03_ignorable_unrequested.rs`                            |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.2`](../../design/09-bug-fixes-and-rationale.md)                  |
| Diagram          | [Diagram 03 — Ignorable-equivocation slash flow (post-fix)](../../diagrams/03-seq-ignorable-equivocation-fixed.svg) |

**Stack depth: 5** (Rocq + TLA⁺ + Rust + design + visual).

## 6 · Lessons for the methodology

1. **Audit-driven discovery is cheap and effective for "known
   gotchas"**. An explicit TODO in the upstream code is the
   strongest possible signal that the audit's eyes should land on
   that function. The methodology mandates a TODO/FIXME audit pass
   as the first step of any port.
2. **The Sage differential model is the *confirmation* of the
   audit, not the *discovery***. The differential model would have
   eventually surfaced the same divergence, but the code-walking
   review found it cheaply and the Sage run confirmed it. This is
   the methodology's preferred ordering — *cheap-first, expensive-
   second*.
3. **Both pre-fix and post-fix tests are required**. The pre-fix
   `pre_fix_bug_1.rs` proves the bug existed; the post-fix
   `uc_03_ignorable_unrequested.rs` proves the fix works. A single
   test cannot do both.
