# S2 — the protobuf encoder: the RESULT record

**Stage S2 of the four-quadrant generator design.** Landed 2026-07-28 on
`feature/mettail`, on top of S0 (`44535d75`) and S1 (`7c74260d`).

> **This document records RESULTS.** The design lives in the code it describes —
> `models/src/rust/rholang/prost_encode.rs` (the machine, §1-4),
> `models/src/rust/rholang/prost_wire.rs` (the alphabet and the two asymmetries),
> and `models/build/wire_schema.rs` (the one walk, the two sort keys). Restating
> it here would create a second copy, and this campaign has watched four prose
> copies of one truth drift, two of them within the hour of being reconciled.

---

## 1. What landed, and what deliberately did not

| landed | not landed (later stages) |
|---|---|
| `prost_encode::{encode_to_vec, encode_into, encoded_len}` | any production call site |
| generated `impl ProstNode` / `impl ProstOneof` | the protobuf DECODER (`Message::merge_field`) |
| the differential + the mutation proof | the term ops (`Clone`, `Ord`, `Debug`, `clear`) |
| the space introspection | `impl Drop for Par`, `DepthBudget`, any version bump |

★ **The encoder is DORMANT.** Verified mechanically: `prost_encode::` appears in
no `src/` tree of `models`, `rholang`, `rspace++`, `casper`, `node`, `comm` or
`shared`. The only reference outside its own file is the `pub mod` declaration.

★★ **Acceptance-neutral, and that is a property of protobuf's write side rather
than a promise.** `models/tests/par_prost_depth_ceiling.rs` (stage 2) asserts
that `prost` places **no limit** on encoding and fails loudly if one appears. So
converting the encoder changes zero bytes and zero accepted inputs. The
consensus-visible surface is untouched: `rhoapi_wire.rs` remains byte-identical
(md5 `0296fc17f2ef33897e7fd2ca9b68c524`, unchanged across S1 and S2).

---

## 2. ★★ The mutation proof — at the GENERATOR, not at the verdict

A green differential between two functions that are secretly the same function
is indistinguishable from a green differential between two functions that agree.
So the differential was **shown red**, three times, and not by perturbing bytes
in a test: by **patching the generator, rebuilding, and requiring the emitted
table to differ from the control before any verdict was consulted.**

That escalation was deliberate. This campaign has already produced two
near-misses of exactly the weaker kind — *"M3 first reported green because its
patch did not apply"* and two mutations that *"stayed green because they didn't
change what was being compared."* A byte-level mutation fed to a verdict proves
the **judge** can reject; only a generator-level mutation proves the **encoder**
would have been caught.

### 2.1 Protocol

```text
   for each mutation:
     1. restore the control generator
     2. apply the patch          ─── refuse unless the SOURCE changed
     3. cargo build -p models    ─── regenerate OUT_DIR/rhoapi_prost_wire.rs
     4. diff emitted vs control  ─── ★ REFUSE TO REPORT unless they differ
     5. run the differential     ─── it MUST fail
     6. restore, always (trap)
```

Step 4 is the one that matters. A patch that compiles, changes the generator
source, and emits a byte-identical table is **inert**, and any red it produced
would be about something else. The harness prints the emitted diff, so "it
applied" is shown rather than asserted.

### 2.2 Results

| # | generator patch | emitted table changed by | differential verdict |
|---|---|---|---|
| **M1** | `prost_order`'s `sort_by_key(min_tag)` **removed** — declaration order | 34 lines | REJECTED |
| **M2** | sort key becomes `(is_oneof, min_tag)` — oneofs last | 10 lines | REJECTED |
| **M3** | skip-if-default guard replaced by `if true` for `bool` | 68 lines | REJECTED |

**M1 — `Par` in declaration order.** The emitted table moved `bundles` (tag 11)
and `conditionals` (tag 12) ahead of `connectives` (8), `locally_free` (9) and
`connective_used` (10), exactly as reusing the bincode order table would:

```text
   PROST WRITE DIFFERENTIAL FAILED for `Par::all_par_fields`:
     first difference at byte 925 (machine 0x5a, oracle 0x42); both are 1031 bytes.
```

★ **1031 bytes on both sides.** A pure permutation. A length check sees nothing,
a round-trip sees nothing, and every protobuf decoder in existence accepts the
mutated message — the fields are the same fields.

**M2 — `TaggedContinuation`'s `guard` before its oneof.**

```text
   PROST WRITE DIFFERENTIAL FAILED for `TaggedContinuation::par_body`:
     first difference at byte 0 (machine 0x1a, oracle 0x0a); both are 1140 bytes.
```

⚠⚠ **Same length, same byte multiset, halves exchanged.** This is the mirror
image of the defect the *serde* order of this same message already produced in
this campaign — "a 95-byte encoding with its halves exchanged" — and for protobuf
the correct order is **the opposite of that fix**. The in-test twin of M2
additionally asserts the multiset is preserved and that the LENGTH verdict
**accepts** it, which is the executable form of "a length check would not see
this."

**M3 — skip-if-default dropped on `bool`.** Unlike M1 and M2 this one is visible
to a length check, and the record shows it failing that way across the corpus:

```text
   `Connective::ConnAndBody`      length 18 vs oracle 14
   `Expr::ENotBody`               length 11 vs oracle 9
   `deep_par(1)`                  length 17 vs oracle 11
   `BindPattern.remainder[0]`     length  8 vs oracle 6
```

`prost-derive` emits `if #ident != #default` in **both** `encode` and
`encoded_len` (`src/field/scalar.rs:116-125, 172-189`). The generator renders
that guard **once**, in `prost_scalar_arms`, and interpolates it into both
bodies — so no edit can apply it to one pass and not the other. M3 removes it
from both at once, which is why it fails on length rather than producing the
subtler self-inconsistency; the *self*-inconsistency is caught separately, by
`assert_encodes_identically`'s third clause (the machine's reported length must
equal the number of bytes it wrote).

### 2.3 Reproducing

The harness is a throwaway (`/tmp`), by construction: a permanent test may not
patch the generator it tests. What is permanent is the differential it drives —
`models/tests/prost_encode_differential.rs` — whose own
`the_prost_differential_can_go_red` reproduces all three mutations at the byte
level, each asserting it applied, with a control that passes before and after.

---

## 3. Facts confirmed from source, not assumed

Three semantic questions had to be answered before a byte could be written. Each
was resolved by reading `prost`'s own source, and each is cited at the code that
depends on it.

### 3.1 The field order is ascending minimum tag

`prost-derive-0.14.3/src/lib.rs:87-92`, verbatim:

```text
   // Sort the fields by tag number so that fields will be encoded in tag order.
   // TODO: This encodes oneof fields in the position of their lowest tag,
   // regardless of the currently occupied variant, is that consequential?
   fields.sort_by_key(|(_, field)| field.tags().into_iter().min().unwrap());
```

`encode_raw` and `encoded_len` are built from the **sorted** list (`:103-109`);
`Debug` from `unsorted_fields` (`:85, :214`). Two orders inside one derive. The
generator reproduces the oneof quirk rather than "fixing" it.

### 3.2 A oneof arm is ALWAYS written, even at its default

`scalar::Field::new_oneof` rewrites `Kind::Plain` into `Kind::Required`
(`src/field/scalar.rs:92-106`), and the `Required` arm of `encode` /
`encoded_len` carries **no** `if #ident != #default` guard. That is protobuf's
presence semantics: a set-but-default oneof member must stay distinguishable
from an absent one.

⇒ A driver that inherited the plain-field skip rule would silently erase
`GBool(false)`, `GInt(0)`, `GString("")`, `GUri("")`, `GByteArray([])` and
`GDouble(±0.0)` from the wire. All six are in the corpus
(`the_protobuf_specific_awkward_shapes_encode_identically`), and each case
additionally asserts the **oracle's** encoding is non-empty, so the case cannot
pass by both sides skipping.

### 3.3 A protobuf map is not a repeated value, and BOTH halves skip at default

`prost-0.14.3/src/encoding.rs:1044-1059`:

```text
   let skip_key = key == &K::default();
   let skip_val = val == val_default;
   let len = (if skip_key { 0 } else { key_encoded_len(1, key) })
           + (if skip_val { 0 } else { val_encoded_len(2, val) });
   encode_key(tag, WireType::LengthDelimited, buf);
   encode_varint(len as u64, buf);
```

Each pair is a length-delimited **entry message** with the key at tag 1 and the
value at tag 2 — so an entry is a node with an id of its own, whose single child
is the value.

⚠★ `val == val_default` goes through the **hand-written** `PartialEq` in
`models/src/lib.rs`, and `<Par as PartialEq>::eq` **deliberately ignores
`locally_free`**. So a `Par` carrying only `locally_free` **is** skipped. The
driver therefore compares against one `Par::default()` with `==` rather than
restating the predicate as "is it structurally empty?", which would differ on
precisely that value — and the corpus carries it
(`New.injections::locally-free-only`).

---

## 4. The complexity claim, and how it is checked

`Message::encode_to_vec` (`prost-0.14.3/src/message.rs:61-69`) calls
`self.encoded_len()` — a complete recursive walk — and then `encode_raw`; and
`encoding::message::encode` (`encoding.rs:788-795`) calls `msg.encoded_len()`
**again for every nested message it writes**. A node's subtree is therefore
measured once per ancestor:

```math
\sum_{v} \big|\mathrm{subtree}(v)\big| \;=\; \Theta(d^2)
\quad\text{on a depth-}d\text{ chain},
\qquad \Theta(n \cdot d) \text{ in general.}
```

The memoized bottom-up pass measures each node **once**, giving
`` $\Theta(n)$ ``, at a cost of `` $\Theta(n)$ `` *space* (4 bytes per message
node) where prost's is `` $O(1)$ ``. That is the whole trade.

★ **The claim is checked structurally, not by timing** — a timing assertion in a
test suite is a flake. `the_length_table_holds_exactly_one_entry_per_message_node`
asserts that the length table grows **linearly** in depth across
`` $d \in \{4, 8, 16, 32\}$ ``, i.e. that the per-level entry count is constant.
A growing per-level cost is precisely the `` $\Theta(d^2)$ `` behaviour, so
linearity is the observable form of "each node is measured once".

The same test pins the other half of the space claim: a 4,096-sibling term must
put fewer than 16 entries on **either** op stack (the counted repeat re-pushes
*itself*, not its children) while the length table holds ≥ 4,096 — Θ(depth) ops
alongside Θ(n) table, exhibited on one value.

### 4.1 ★ Why one monotonic cursor suffices

Pass 1 allocates pre-order ids **at the moment of descent**. Pass 2 descends in
the same order, through the same generated field walk, the same counted repeat
and the same `BTreeMap` iterator. So the sequence of nodes at which pass 2 must
write a length prefix is exactly the sequence in which pass 1 allocated ids, and
a single increasing index resolves every one — no child table, no id on the op
stack.

⚠ That makes agreement between the passes **structural rather than asserted**,
and it is bounded on both sides:

* `EmitMachine::open_child` panics, naming the slot, if the cursor runs **past**
  the table;
* `EmitMachine::finish_emit` panics if the cursor stops **short** — which is what
  a pass-2 walk that skipped a child would do, and which would otherwise produce
  a perfectly well-formed protobuf message that is simply missing a field.

---

## 5. ⚠ The one NAMED RESIDUAL: `EPathMap`

`EPathMap::encode_raw` (`models/src/rust/rhoapi_ext.rs`) has **three** arms — a
`memcpy` of the interned canonical bytes, the ground field-8 `` $U(m)$ `` form,
and the ordinary field walk — of which only the last is a field walk at all, and
which one fires depends on a `OnceLock` another thread may fill.

⇒ It is an **opaque leaf**: both passes intercept it through
`ProstNode::prost_opaque` and treat it as one node, `opaque_encoded_len()` for
the prefix and `opaque_encode_raw()` for the body. That is exact parity with
`prost::encoding::message::encode` at that position, and it is the only
formulation that keeps `EPathMap` the sole authority on its own bytes: a driver
that measured under one arm and emitted under another would write a length
prefix that does not match its body.

★ **Correct, and not depth-independent, are two separate statements.** A term
nested through an `EPathMap` recurses inside `EPathMap::encode_raw` exactly as
the derived path does. The bytes are identical — `deep_mixed_par` cycles through
an `EPathMap` every eighth level and is in the differential out to depth 256 —
and the native stack is **not** bounded through that one shape. It is named here,
in `prost_wire.rs` §D and in `prost_encode.rs` §4, rather than left for a stack
trace to report.

---

## 6. Verification

| gate | result |
|---|---|
| `rhoapi_wire.rs` byte-identical | ★ md5 `0296fc17f2ef33897e7fd2ca9b68c524`, unchanged |
| `cargo test --release -p models` (what CI runs) | **485 passed, 0 failed** across 32 binaries |
| `models/tests/prost_encode_differential.rs` | 13 passed |
| `models/tests/schema_meta_conformance.rs` | 10 passed |
| `cargo nextest run -p models` (both new binaries) | 23 passed |
| production call sites of `prost_encode::` | **zero** |
| generator-level mutation proof | 3/3 applied and rejected |

### 6.1 Runner discipline

⚠ `cargo test` runs a body on a **spawned thread honouring `RUST_MIN_STACK`**;
`cargo nextest` runs it on the process **main thread, which `RUST_MIN_STACK`
does not affect**. f1r3node CI runs `cargo test --release -p models` only.

Every deep body in the new test therefore runs inside an explicit
`std::thread::Builder::new().stack_size(N)` — the precedent is
`par_codec_wire_shapes.rs:639-660` and `wire_encode_differential.rs:646-661` —
and each such test's doc comment states the stack it passes on under **both**
runners (`deep_terms_encode_identically`: 256 MiB;
`the_length_table_holds_exactly_one_entry_per_message_node`: 64 MiB). Both were
run under both runners; both pass.

### 6.2 ⚠ A PRE-EXISTING `-D warnings` break, not introduced here

`cargo test --release -p models` with CI's
`RUSTFLAGS="-C target-feature=+aes,+sse2 -D warnings"` fails to compile the
`models` lib-test target:

```text
   models/src/rust/casper/protocol/casper_message.rs:2037:9:
     error: unused import: `proptest::prelude::*`
```

That file is **not touched by S0-S2** (`git status` reports it unmodified) and
the line is present at `HEAD~2`, before this work began; its last commit is
`e55769dd` *"feat(cost-accounting): F-A"*. It is reported rather than fixed:
it belongs to another agent's change, and silently deleting an import in a file
this stage has no business in would hide whichever half of that change is
incomplete.

The suite result above was therefore obtained without `-D warnings`. **With** it,
`models` does not compile at HEAD — independently of this work.

---

## 7. References

* `prost` 0.14.3 — `src/message.rs:61-69`, `src/encoding.rs:106`, `:133-135`,
  `:788-795`, `:845-852`, `:1026-1060`, `:1105-1134`.
* `prost-derive` 0.14.3 — `src/lib.rs:85-109`, `:173-243`, `:440-518`;
  `src/field/scalar.rs:92-106`, `:108-135`, `:161-189`.
* `docs/design/audits/four-quadrant-s0-baseline-2026-07-28.md` — the S0 ladder
  table, the E0509 probe, and the containment graph.
* `docs/design/audits/theta-depth-traversals-2026-07-26.md` — the enumeration and
  the conversion pattern this encoder follows.
