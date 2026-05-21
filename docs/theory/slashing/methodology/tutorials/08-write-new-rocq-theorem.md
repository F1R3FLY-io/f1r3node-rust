# 08 · Write a new Rocq theorem

## 1 · Prerequisites

- Rocq (Coq) installed, version pinned to the slashing
  development's expectation (see
  [`formal/rocq/slashing/README.md`](../../../../../formal/rocq/slashing/README.md)).
- Familiarity with the existing 14 Rocq modules
  ([`formal/rocq/slashing/theories/`](../../../../../formal/rocq/slashing/theories/)).
- A mathematical statement of the theorem ready in advance — the
  theorem text is *not* something to discover while proving.
- Resource limits configured (the build uses ≤ 12 GB per module
  under `systemd-run --user --scope -p MemoryMax=96G -p CPUQuota=1800%`).

## 2 · Skeleton

Add to the appropriate module file (or create a new module). The
template:

```rocq
(* ===========================================================
   THEOREM <name>
   STATEMENT: <natural-language statement>
   REFERENCES:
     - docs/theory/slashing/slashing-verification.md §<N>
     - docs/theory/slashing/design/09-bug-fixes-and-rationale.md §<N>
   STACK ROLE:
     - Mechanized arm of property <P>
     - Companion: <TLA+ invariant>, <Rust regression>
   =========================================================== *)

Theorem <theorem_name> :
  forall (<params>),
    <preconditions> ->
    <conclusion>.
Proof.
  intros.
  (* <structured proof, top-down> *)
Qed.
```

For new modules, add to `_CoqProject`:

```
theories/<NewModule>.v
```

Then regenerate the `Makefile`:

```sh
coq_makefile -f _CoqProject -o Makefile
```

## 3 · Example from this repo

See [`formal/rocq/slashing/theories/EquivocationDetector.v`](../../../../../formal/rocq/slashing/theories/EquivocationDetector.v)
`t_1_detection_sound` — the canonical example of a theorem matching
the *“no honest validator is ever slashed”* property.

## 4 · Verification step

Build under resource limits (per project CLAUDE.md):

```sh
systemd-run --user --scope \
    -p MemoryMax=96G -p CPUQuota=1800% \
    -p IOWeight=30 -p TasksMax=200 \
    make -j1
```

Verify trust base:

```sh
coqtop -batch -load-vernac-source theories/<Module>.v \
       -e 'Print Assumptions <theorem_name>.'
```

Required output:

```
Closed under the global context
```

If the output mentions any `Axiom`, `Parameter`, or `Admitted`,
the trust base is broken; the theorem must be reproved without
the offending dependency.

## 5 · Common pitfalls

- **Proving the model, not the system** — the theorem must
  correspond to a Rust observable; without a `harness`/`oracle`
  correspondence, the theorem is a model statement only.
- **Hidden precondition in notation** — quantifiers over subtypes
  silently weaken the theorem; use base inductive types.
- **Tactic-driven proof structure** — proofs that read as a
  sequence of `apply lemma_X23` are unauditable; factor every
  lemma into a named mathematical proposition.
- **Depending on tactic automation** — `intuition`, `eauto`,
  `lia` are allowed only at the *leaves* of the proof tree.

See [`../formal-methods/01-mechanized-proof-rocq.md §6`](../formal-methods/01-mechanized-proof-rocq.md)
for the full pitfall catalog.

## 6 · Promotion checklist

Before marking the theorem complete, verify:

- [ ] `Print Assumptions <theorem_name>` returns *“Closed under the
      global context”*.
- [ ] The corresponding TLA⁺ invariant exists in
      `formal/tlaplus/slashing/`.
- [ ] The corresponding `prop_t_*.rs` proptest exists.
- [ ] The corresponding entry in
      [`../../slashing-verification.md`](../../slashing-verification.md)
      cites the theorem by name and `file:line`.
- [ ] The entry in
      [`../../slashing-traceability.md`](../../slashing-traceability.md)
      records the theorem's stack depth.
