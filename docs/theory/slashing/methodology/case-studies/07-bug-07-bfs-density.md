# Case study #7 — Off-by-one seq-number density

## 1 · Summary

Pre-fix, the equivocation detector's BFS-style walk of a validator's
sequence-number chain used `base_seq + 1` as the inclusive lower
bound, missing one block at the boundary. Under a contrived
sequence shape (the *canonical self-chain child above base*), an
equivocation at the boundary escaped detection. Post-fix, the
boundary is `base_seq` inclusive; the BFS density check is
closed under `seq ↦ seq + 1`.

## 2 · Discovery technique

**Primary**: Sage `closure_certificate_model.sage` emitted a
fixed-point depth + shortest-path certificate that revealed the
detector's BFS terminated *one block early* on a particular self-
chain shape.

**Corroborating**: Hypothesis `arithmetic_projection_stress` enumerated
sequence-number boundaries and surfaced the same off-by-one as a
witness with seq numbers `[1, 2, 3]` and a contrived view at
`base_seq = 1`.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_7
```

The fixture
[`casper/tests/slashing/pre_fix_bug_7.rs`](../../../../../casper/tests/slashing/pre_fix_bug_7.rs)
encodes the canonical self-chain shape; pre-fix the boundary block
is missed, post-fix it is detected.

## 4 · Classification trace

```
threat_class       = permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_7.rs + post-fix anchor
                     prop_t_9_7_seqnum_density.rs
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                        |
|------------------|-------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.7 (`BugFixSeqnumDensity.v` — implicit in detector totality)                                 |
| Sage witness     | `closure_certificate_model.sage` finding                                                        |
| Hypothesis       | `hypothesis_arithmetic_projection_stress.rs`                                                    |
| Rust regression  | `pre_fix_bug_7.rs`, `prop_t_9_7_seqnum_density.rs`                                              |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.8`](../../design/09-bug-fixes-and-rationale.md) |

**Stack depth: 4** (Rocq + Sage + Rust + design).

## 6 · Lessons for the methodology

1. **Off-by-one bugs hide in inclusive/exclusive boundaries**. The
   bug was *one line*: `base_seq + 1` vs. `base_seq`. The
   methodology requires every boundary expression to have an
   explicit comment naming the inclusive/exclusive choice and the
   reason.
2. **Closure certificates surface BFS bugs**. The
   `closure_certificate_model.sage` emits the shortest neglect path
   alongside the closure depth; comparing the model's path against
   the detector's traversal exposes off-by-ones immediately.
3. **Symmetric boundary tests matter**. A test that only exercises
   `seq = 0, 5, 10` will miss boundary issues; tests that exercise
   `seq = 0, 1, 2, 3, 4, 5, …` for small ranges catch off-by-ones.
