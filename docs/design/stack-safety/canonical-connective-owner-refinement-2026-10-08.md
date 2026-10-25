# Canonical Connective cursor owner refinement

The byte-native matcher and count PDA suspend a cursor while they visit a
canonical `ConnectiveBody` child. The cursor must resume against the same body
that was validated before suspension. If a same-length replacement or copied
body could be attached to its saved offset, the next child could come from a
different term even though the cursor still appears in bounds.

The production contract lives in
[`CanonicalConnectiveChildren` and `CanonicalOwnedConnectiveChildren`](../../../models/src/rust/par_bag.rs).
Construction validates every field-1 child in the complete borrowed body. A
borrowed cursor retains that body, its next byte offset, and the number of
children left. `bind_owned` compares the owner's byte-slice pointer and length
with the retained body **before** transferring either cursor counter. An owned
cursor then reads only its stored owner and returns a range relative to that
owner. No child `Par` is decoded or collected during this transition.

| Stage | Source value | Required relationship |
|---|---|---|
| Validate | borrowed `message: &[u8]` | Canonical field-1 headers and minimally encoded lengths cover the entire body in order. |
| Suspend | `at`, `remaining` | `at` is the next validated header, or the body end after the last child; `remaining` is the validated suffix count. |
| Bind | `body.as_ref()` | Both pointer and length equal the borrowed body; mismatch returns `BodyMismatch` before a position is transferred. |
| Resume | owned `body`, `at`, `remaining` | Each `next` returns the next validated payload range, advances to its end, and decrements the count once. Exhaustion occurs only after the validated suffix is consumed. |

## Stable-owner boundary

The generic bound is
[`StableConnectiveBodyOwner`](../../../models/src/rust/par_bag.rs), an explicit
unsafe trait. Its implementor promises that every shared `as_ref()` call yields
the same pointer, length, and byte values from binding until the owned cursor
is dropped, including after the owner is moved. Plain `AsRef<[u8]>` does not
make this promise: a safe owner can use `Cell` to choose between two immutable
buffers on successive calls. The
[`SwitchingOwner` runtime control](../../../models/tests/par_bag.rs) and
[compile-fail trait example](../../../models/src/rust/par_bag.rs) exhibit that
gap; the switching owner cannot satisfy the new binding API without an
explicit unsafe implementation of the trait.

The audited implementations cover `&[u8]`, `Vec<u8>`,
[`ProtobufRegion`](../../../models/src/rust/rholang/protobuf_cursor.rs) (`Bytes` and
a fixed range), and the matcher's private
[`EncodedRegion`](../../../rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs)
(`Arc<Vec<u8>>` and fixed offsets). The owned cursor exposes only a shared
reference to its owner. Safe mutation of a different `Arc` clone uses
copy-on-write and does not change this cursor's view. A zero-length slice may
share a pointer value with another empty allocation; the validated empty body
has no child to read, so this does not grant an incorrect continuation. Pointer
equality is therefore an operational binding check, not a general claim of
allocation identity.

## Formal source model and extraction

The [Verus model](../../../formal/verus/canonical_connective_owner_cursor.rs)
takes its byte sequence from the exact borrowed `message`. Its child table is
constrained by the field-1 key byte, canonical varint length bytes, contiguous
child framing, and complete body coverage. `CursorPosition` represents the
Rust `at` and `remaining` fields. `checked_bind` models the production
pointer-and-length guard, while `modeled_next` returns the next validated
payload range and updated position. The model's `stable_owner_view` predicate
is the audited unsafe-trait premise; Verus does not derive byte immutability
from a raw address or verify the Rust borrow checker.

| Named invariant | Production transition | Executable extraction | Causal control |
|---|---|---|---|
| `mismatched_owner_refused_before_position_transfer` | `CanonicalConnectiveChildren::bind_owned` compares the retained slice pointer and length before copying `at` or `remaining` into the owned cursor. | The generated property refuses a different equal-length allocation, a copied body, truncation, and a wrong owner after exhaustion. | Length-only and pointer-only bind mutants violate `checked_bind`'s postcondition. |
| `exact_owner_preserves_validated_position` | The borrowed cursor stores its source slice and counters; `bind_owned` transfers both counters without advancing the parser, and `CanonicalOwnedConnectiveChildren::len` reports the retained suffix count. | A generated consumed-prefix split binds the original body after any number of borrowed child steps and checks the retained count. | The owner switch control shows why a merely safe `AsRef` bound is insufficient. |
| `next_returns_exact_validated_region` | `CanonicalConnectiveChildren::new` validates the complete field-1 sequence; `CanonicalOwnedConnectiveChildren::next` reads only its stored owner and returns the next payload range. | The generated property derives each owner-relative range from the child length and canonical header width, checks full body coverage, and compares the bytes at that range with the prebinding borrowed child. It repeats these checks after an arbitrary consumed prefix. A separate test compares independently encoded child bytes across the 127/128 and 16,383/16,384 length-header boundaries. | Skip-child and wrong-range mutants violate the exact-next postcondition. |
| `exhaustion_iff_all_validated_children_consumed` | The borrowed and owned `next` methods decrement `remaining` once per child and return `None` only when it is zero. | Both generated cursors return `None` only after their expected suffixes, including empty and fully consumed prefixes. | Early-exhaustion mutant violates next and exhaustion postconditions. |

The [SpatialMatcher obligation registry](../../../formal/spatial_matcher_obligations.generated.json)
links all four formal names to the exact assertions in
[`models/tests/par_bag.rs`](../../../models/tests/par_bag.rs), checks that the
production owner implementations and normal gate are registered, and pins
source hashes. The [normal stack-safe PDA runner](../../../scripts/check-stack-safe-pda-proofs.sh)
requires ten successful no-cheating Verus verifications, rejects all five
mutants generated by
[`generate-canonical-connective-owner-mutant.py`](../../../scripts/generate-canonical-connective-owner-mutant.py),
and runs the four concrete cursor tests plus the generated property in both
debug and release profiles.

This is a source-linked refinement, not a machine-checked compilation proof
of `read_canonical_varint`, pointer provenance, or the four unsafe trait
implementations. The byte-header-to-child-table extraction and stable-owner
premise are checked by the Rust source audit and executable properties. The
production [matcher PDA](../../../rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs)
binds an `EncodedRegion` in both its `And` and `Or` branches. The
[count PDA](../../../rholang/src/rust/interpreter/matcher/par_count.rs) binds a
`ProtobufRegion` for those branch bodies. Both call sites derive the owner as a
subrange of the region from which the borrowed cursor was validated; no
generic untrusted owner reaches them.

## Current-source evidence and final boundary

The focused, headless, resource-capped Verus run with `--no-cheating
--num-threads 1 --rlimit 30` passed **10 verified, 0 errors**. Five sequential
mutants failed at their intended postconditions: length-only and pointer-only
binding, skipped child, wrong returned range, and early exhaustion. The
current-source `models` cursor cohort passed **5/5** in debug and release;
the stable-owner compile-fail documentation test passed **1/1**. The latest
focused logs are under `target/verification/connective-owner-10879/` (Verus,
five mutants, concrete and generated debug/release properties, and compile-fail
documentation control). The formal source SHA-256 at this checkpoint is
`1883e5782f84bf33959ca15e1c3d28f79116a07bb707ae7ea4f751a7fc9ca9c7`.

These focused results are not yet commit-bound acceptance for this leaf. Its
gate is the no-cheating owner proof and intended-failure mutants, the concrete
and generated cursor properties in normal debug and release suites, and the
stable-owner compile-fail control, all checked against one frozen source
commit. Record that commit, source hashes, exact commands, results, and model
limitations through pgmcp. The campaign-wide admission gate is tracked
separately.
