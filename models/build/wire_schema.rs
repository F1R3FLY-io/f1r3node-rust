//! # The wire-schema GENERATOR — ONE pass, FOUR outputs
//!
//! A **build-script pass**, not a proc-macro. It reads the protobuf
//! `FileDescriptorSet` that `prost_build` was asked to dump (`build.rs`
//! `.file_descriptor_set_path(...)`), resolves every `rhoapi` message **once**,
//! and emits four files into `OUT_DIR`:
//!
//! ```text
//!   rhoapi_wire.rs         the BINCODE table  — serializer + deserializer
//!   rhoapi_prost_wire.rs   the PROST table    — protobuf field order + kinds
//!   rhoapi_term_ops.rs     the TERM-OP slot   — empty; filled by a later stage
//!   rhoapi_schema_meta.rs  the child relation, the SCC, and the DERIVE
//!                          DISPOSITION REGISTRY
//! ```
//!
//! ## ⚠★ The field-order table is PER FORMAT. It is a SORT KEY, not a walk.
//!
//! There are **two** orders in play and they are not the same order.
//!
//! **serde / bincode order** is the order prost-build writes the Rust struct's
//! fields into the source file: every plain field first, in declaration order,
//! then every oneof, in `oneof_decl` order (`prost-build-0.14.3/
//! src/code_generator.rs:270-291` is two separate loops). serde's derive is
//! positional over the struct, so that IS the bincode layout.
//!
//! **protobuf order** is ascending minimum tag. `prost-derive-0.14.3/
//! src/lib.rs:87-92`, verbatim:
//!
//! ```text
//!   // Sort the fields by tag number so that fields will be encoded in tag order.
//!   // TODO: This encodes oneof fields in the position of their lowest tag,
//!   // regardless of the currently occupied variant, is that consequential?
//!   fields.sort_by_key(|(_, field)| field.tags().into_iter().min().unwrap());
//! ```
//!
//! `encode_raw` and `encoded_len` are built from the **sorted** list (`:103-109`)
//! while `Debug` is built from `unsorted_fields` (`:85, :214`) — two orders
//! inside one derive.
//!
//! Over the 57 generated messages exactly **two** differ, and both are
//! load-bearing:
//!
//! | message | declaration order (serde / bincode / `Debug`) | prost encode order |
//! |---|---|---|
//! | `Par` | 1,2,4,5,6,7,**11**,8,**12**,9,10 | 1,2,4,5,6,7,8,9,10,11,12 |
//! | `TaggedContinuation` | **3** (`guard`), **1** (oneof) | 1 (oneof), 3 (`guard`) |
//!
//! ⚠⚠ `TaggedContinuation` is the exact message whose serde order already cost
//! this campaign *"a 95-byte encoding with its halves exchanged"* — and for prost
//! the correct order is **the opposite** of the fix applied for bincode. A
//! generator that reused one order table for both drivers would reproduce that
//! defect in mirror, on the hottest type in the schema.
//!
//! So [`resolve_message`] runs **once**, produces **one** `Vec<Field>` of
//! format-neutral facts, and the two emitters consume that same vector under two
//! different keys: [`emit_bincode_source`] under `identity`,
//! [`emit_prost_source`] under `sort_by_key(min_tag)`. Neither emitter walks the
//! descriptor. There is one walk and one classification, so the two tables cannot
//! disagree about *what* a field is — only, deliberately, about *where* it goes.
//!
//! ## Why the descriptor and not the `#[prost(...)]` attributes
//!
//! The attributes describe the protobuf wire but are not machine-readable from a
//! build script; serde's derive exposes nothing at run time. The descriptor
//! carries declaration order, tag numbers and types together, so it is the only
//! artifact that can supply *both* orders from one read.
//!
//! ⚠ **The bincode variant index is serde DECLARATION ORDER, not the proto
//! tag.** `e_pathmap_body` is proto tag 32 and serde index 25. Reading tags as
//! indices silently mis-decodes 12 of the 36 `ExprInstance` arms. The oneof is
//! therefore **append-only**: inserting a member mid-list silently re-labels
//! every byte string already in the store.
//!
//! ## What each output contains
//!
//! ```text
//!   rhoapi_wire.rs
//!     <TY>_PROGRAM : &'static [FieldKind]     one per message   (the SHAPE)
//!     impl WireNode for <TY>                  one per message   (the ACCESS)
//!     impl WireOneof for <ONEOF>              one per oneof     (exhaustive!)
//!     <ONEOF>_VARIANTS : &'static [VariantProgram]              (the INDEX)
//!     <ONEOF>_VARIANT_COUNT = <ONEOF>_VARIANTS.len()  ★ never a literal
//!     EX_<FIELD> : u32                        one per ExprInstance arm
//!
//!   rhoapi_prost_wire.rs
//!     <TY>_PROST_PROGRAM : &'static [ProstField]   ASCENDING MINIMUM TAG
//!     <ONEOF>_PROST_VARIANTS                       each arm's OWN tag
//!     PROST_CONFORMANCE_REGISTRY                   every message, for the probe
//!
//!   rhoapi_schema_meta.rs
//!     SCHEMA_CHILDREN     : the child relation, per type
//!     SCHEMA_SCC          : Tarjan's strongly connected components
//!     RECURSIVE_TYPES     : the types that can contain themselves
//!     DERIVE_DISPOSITION_REGISTRY : (type, trait surface, disposition)
//!     HAND_WRITTEN_TRAVERSALS     : the walks a derive scan CANNOT see
//! ```
//!
//! The `impl WireOneof` matches carry **no wildcard arm**, so a 37th variant
//! added to the `.proto` is a *compile error* until the table regenerates —
//! which it does, in the same pass. That is what makes "coverage asserted
//! against the generated variant set" true by construction rather than by
//! a hand-maintained list that silently stays at 36.
//!
//! ## ★★ The DERIVE DISPOSITION REGISTRY — the method fix
//!
//! Every driver this campaign writes replaces a recursive walk that a
//! `#[derive]` generated. The list of walks must therefore be **derived from
//! what is actually derived**, never hand-picked — a hand-picked list of four
//! missed `Hash` entirely, and the enumeration additionally found
//! `Ord`/`PartialOrd`, which nobody had named.
//!
//! [`DERIVE_DISPOSITIONS`] is the closed table: one row per `#[derive]` token
//! that reaches `OUT_DIR/rhoapi.rs`, expanded into the run-time SURFACES it
//! produces, each with a [`Disposition`]. The generator emits the cross product
//! (type × surface) as `DERIVE_DISPOSITION_REGISTRY`, and `models/build.rs`
//! cross-checks the table against a TEXTUAL scan of the generated file — exactly
//! as it already cross-checks the `locally_free` rewrite. A seventh trait fails
//! the build, naming itself, until somebody dispositions it.
//!
//! ⚠ The registry is a **lower bound** on the campaign's driver list, and says
//! so in its own doc comment: `models/build.rs` STRIPS `PartialEq`/`Eq`/`Hash`
//! from prost's output and `models/src/lib.rs` writes them by hand, so no
//! `#[derive]` scan can see them. `HAND_WRITTEN_TRAVERSALS` carries those.
//!
//! ## The three generator constraints
//!
//! 1. **`extern_path`'d types have no descriptor-driven program.** `EPathMap`
//!    is extern (`build.rs`) and its hand-written `Serialize` deliberately
//!    re-orders `ps` into canonical trie order for ground maps. The generator
//!    **fails loudly** on any extern type not listed in [`EXTERN_OVERRIDES`],
//!    and the emitted code *references* the type in `Opt`/`Node` positions, so
//!    a missing hand-written `impl WireNode` is a compile error too.
//! 2. **scalapb `field.type` overrides are ignored.** `ESet`/`EMap` carry
//!    `coop.rchain.models.ParSet` / `ParMap` annotations that Rust does not
//!    honour; no semantics are inferred from them.
//! 3. **`locally_free` is serialize-asymmetric.** `build.rs` textually injects
//!    `serialize_with = serialize_as_empty_bytes` on every `locally_free`
//!    bytes field: it is *written* as eight zero bytes and *read* as the
//!    stream's real length. The generator marks those fields
//!    `FieldKind::EmptyBytes` by the same rule (field named `locallyFree`,
//!    type `bytes`) and `build.rs` **cross-checks the two counts**, so the
//!    textual pass and the table cannot drift apart.
//!    ⚠ The asymmetry is **serde-only**. prost RETAINS `locally_free`, so the
//!    prost table classifies those fields as ordinary `bytes` — a table that
//!    inherited the blanking would drop a field from the protobuf wire, which is
//!    a fork.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use heck::{ToSnakeCase, ToUpperCamelCase};
use prost_types::field_descriptor_proto::{Label, Type};
use prost_types::{DescriptorProto, FieldDescriptorProto, FileDescriptorSet};

/// Types the descriptor declares but `build.rs` maps out with `.extern_path`.
///
/// Every entry MUST have a hand-written `impl WireNode` (and, where it is a
/// oneof payload, must behave byte-identically to its `Serialize`). Listing a
/// type here is a deliberate, reviewed act — the generator refuses to silently
/// skip anything.
pub const EXTERN_OVERRIDES: &[&str] = &["EPathMap"];

/// The proto package the table covers.
const PACKAGE: &str = "rhoapi";

/// The `OUT_DIR` file names this pass emits, in the order [`generate`] returns
/// them. `models/build.rs` writes each verbatim; naming them here keeps the
/// producer and the consumer from drifting over a string literal.
pub const OUTPUT_BINCODE: &str = "rhoapi_wire.rs";
/// See [`OUTPUT_BINCODE`].
pub const OUTPUT_PROST: &str = "rhoapi_prost_wire.rs";
/// See [`OUTPUT_BINCODE`].
pub const OUTPUT_TERM_OPS: &str = "rhoapi_term_ops.rs";
/// See [`OUTPUT_BINCODE`].
pub const OUTPUT_SCHEMA_META: &str = "rhoapi_schema_meta.rs";

// ===========================================================================
// §0  What one run produces
// ===========================================================================

/// Result of one generator run.
pub struct Generated {
    /// `(OUT_DIR file name, Rust source)`, one entry per output.
    pub sources: Vec<(&'static str, String)>,
    /// Everything `models/build.rs` cross-checks or logs.
    pub counts: Counts,
    /// ★★ The Rust item names whose `impl Clone` the term-op emitter wrote, in
    /// descriptor order.
    ///
    /// This is one half of a **two-rule cross-check**. `models/build.rs`'s
    /// textual pass strips the `Clone` token from the derive lines it decides are
    /// non-`Copy` and records the item names; this list is computed from the
    /// DESCRIPTOR by an independent reproduction of prost's `Copy` rule
    /// (`prost-build-0.14.3/src/context.rs:183-233`). `build.rs` requires the two
    /// to agree as SETS in both directions, naming the offending item — a type
    /// stripped but not emitted has no `Clone` at all, and one emitted but not
    /// stripped has two.
    pub clone_impls: Vec<String>,
}

/// The tallies one run produces, kept apart from the sources so a cross-check
/// reads as a statement about the schema rather than about a string.
pub struct Counts {
    /// How many fields were classified `FieldKind::EmptyBytes`. `build.rs`
    /// asserts this equals the number of textual `serialize_with` injections
    /// it made — the two rules are then provably the same rule.
    pub empty_bytes_fields: usize,
    /// Messages with a GENERATED program (i.e. excluding [`EXTERN_OVERRIDES`]).
    /// `build.rs` asserts this equals the number of `::prost::Message` derives
    /// in the post-processed `rhoapi.rs`.
    pub message_count: usize,
    /// Messages the descriptor declares that `build.rs` maps out with
    /// `.extern_path`, so `message_count + extern_count` is the whole package.
    pub extern_count: usize,
    /// Oneofs. `build.rs` asserts this equals the number of `::prost::Oneof`
    /// derives.
    pub oneof_count: usize,
    /// Strongly connected components of the child relation, of any size.
    pub scc_count: usize,
    /// Types that can contain themselves — the set every recursive walk in this
    /// schema is a walk over.
    pub recursive_type_count: usize,
    /// Rows in `DERIVE_DISPOSITION_REGISTRY` (type × trait surface).
    pub derive_row_count: usize,
    /// The CLONE CUT SET — the feedback vertex set of the child relation whose
    /// members get a DRIVEN `Clone`. Logged, so a `.proto` change that moves the
    /// cut is visible in the build output rather than only in a generated file.
    pub clone_cut_set: Vec<String>,
    /// How many items the clone driver ENTERS (messages + oneofs).
    pub clone_descend_count: usize,
    /// The residual (cut-removed) child relation's height — the constant that
    /// bounds a generated field-wise `clone`'s native recursion.
    pub clone_residual_height: usize,
}

// ===========================================================================
// §1  ★★ The closed DERIVE DISPOSITION table
// ===========================================================================

/// What this campaign has decided about one run-time surface of one `#[derive]`.
///
/// ⚠ There is no "unknown" variant, and that is the point. A trait token that
/// reaches `OUT_DIR/rhoapi.rs` without a row in [`DERIVE_DISPOSITIONS`] **fails
/// the build**; it cannot be carried as an unclassified entry that nobody
/// notices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Not a walk over term structure at all — a marker trait, a type-level
    /// artifact, or a constructor that fills defaults without descending.
    /// The string says WHY, because "it does not recurse" is a claim.
    NotATraversal(&'static str),
    /// A recursive walk that an explicit-worklist driver has already replaced.
    /// The string names the driver.
    Converted(&'static str),
    /// A recursive walk with no driver yet. The string names the stage that owns
    /// it in the four-quadrant design.
    Remaining(&'static str),
    /// A recursive walk that needs no driver of its own because a driver listed
    /// elsewhere subsumes it. The string names that driver.
    FollowsFrom(&'static str),
}

impl Disposition {
    /// The registry cell, as Rust source.
    fn as_source(self) -> String {
        match self {
            Disposition::NotATraversal(why) => format!("Disposition::NotATraversal({why:?})"),
            Disposition::Converted(driver) => format!("Disposition::Converted({driver:?})"),
            Disposition::Remaining(stage) => format!("Disposition::Remaining({stage:?})"),
            Disposition::FollowsFrom(driver) => format!("Disposition::FollowsFrom({driver:?})"),
        }
    }
}

/// The facts about ONE generated item that a per-item disposition depends on.
///
/// See [`refine_disposition`].
pub struct ItemFacts<'a> {
    /// The Rust type / enum name, as it appears in `OUT_DIR/rhoapi.rs`.
    pub rust_name: &'a str,
    /// Does prost derive `Copy` for this item? Reproduced from prost's own rule
    /// and cross-checked against the generated file — see §4b.
    pub is_copy: bool,
    /// Is this item in the CLONE CUT SET, i.e. does it get a DRIVEN `Clone`?
    pub in_clone_cut_set: bool,
}

/// ★★ **PER-ITEM REFINEMENT of [`DERIVE_DISPOSITIONS`].**
///
/// Most surfaces have the same disposition for every item that carries the
/// derive, and the table states those once. `Clone::clone` does **not**, and the
/// reason is a fact about the code rather than a nicety:
///
/// | item | after stage F-4 | disposition |
/// |---|---|---|
/// | a `Copy` message or oneof | keeps its derive; `Clone` is `*self` | `NotATraversal` |
/// | a CUT-SET member | `impl Clone` over `drive_with` | `Converted` |
/// | any other non-`Copy` item | generated FIELD-WISE `Clone`, flat because every cycle passes through the cut set | `FollowsFrom` |
///
/// ⚠ A uniform `Converted("term_ops::clone")` would be a false claim about 61 of
/// the 62 generated items, and `FollowsFrom` exists precisely so that "no driver
/// of its own" is an argument rather than an omission.
///
/// ⚠⚠ **This refinement is NOT forced by `check_derive_dispositions`, and that is
/// a REFUTED PREMISE worth recording.** The plan expected stripping `Clone` to
/// make the `Clone` row *stale* and fail the build, forcing the re-home. It does
/// not: the seven `Copy` items keep their `Clone` derive, so the token still
/// appears in `rhoapi.rs` and the staleness check stays green. What actually joins
/// the strip to the registry is the per-item set cross-check in `models/build.rs`
/// — the strip pass's names against [`Generated::clone_impls`] — which fails
/// naming the item. The mechanism had to be BUILT, not merely triggered.
fn refine_disposition(surface: &str, uniform: Disposition, facts: &ItemFacts<'_>) -> Disposition {
    match surface {
        "Clone::clone" => {
            assert!(
                !(facts.is_copy && facts.in_clone_cut_set),
                "wire_schema: `{}` is both `Copy` and a member of the CLONE CUT SET. A `Copy` \
                 type's `Clone` must be `*self`, and prost only derives `Copy` where every \
                 field is a non-repeated scalar — such a type contains no term and cannot be \
                 on a cycle of the child relation. One of the two derivations is wrong: either \
                 the `Copy` reproduction in §4b or the feedback-vertex-set search.",
                facts.rust_name
            );
            match (facts.is_copy, facts.in_clone_cut_set) {
            (true, _) => Disposition::NotATraversal(
                "prost derives `Copy` for this item (every field is a non-repeated scalar), so \
                 rustc's `Clone` is `*self` — a bitwise copy with no descent. `models/build.rs` \
                 therefore LEAVES the `Clone` token on its derive line; there is no walk to \
                 convert, and an emitted `impl Clone` would conflict with the derive",
            ),
            (false, true) => Disposition::Converted(
                "term_ops::clone — `impl Clone` over `drive_with` on pooled stacks. This item is \
                 in the CLONE CUT SET (a feedback vertex set of the child relation), so it is \
                 where the recursion is actually broken. Gate subject `clone`, in \
                 `CONVERTED_DEPTH`",
            ),
            (false, false) => Disposition::FollowsFrom(
                "term_ops::clone — this item's `Clone` is GENERATED FIELD-WISE (byte-for-byte \
                 what the stripped derive emitted) and is FLAT: every cycle in the child \
                 relation passes through the cut set, so it reaches a driven clone within \
                 CLONE_RESIDUAL_HEIGHT native frames. It needs no driver of its own",
            ),
            }
        }
        _ => uniform,
    }
}

/// One `#[derive]` token and every run-time surface it expands to.
pub struct DeriveTrait {
    /// The token exactly as it appears inside `#[derive(...)]` in
    /// `OUT_DIR/rhoapi.rs`, after `models/build.rs`'s textual post-processing.
    pub token: &'static str,
    /// Which kind of item carries it.
    pub applies_to: Applies,
    /// `(surface, disposition)`, one row per method or `impl` the derive
    /// produces. The surfaces are read off the derive's own expansion, cited
    /// per row, rather than recalled.
    pub surfaces: &'static [(&'static str, Disposition)],
}

/// Which generated items a derive token lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applies {
    /// Every message struct and every oneof enum (the blanket
    /// `.message_attribute` / `.enum_attribute` in `models/build.rs`).
    Both,
    /// Message structs only.
    Messages,
    /// Oneof enums only.
    Oneofs,
}

/// ★★ **THE CLOSED TABLE.** Every `#[derive]` token that reaches
/// `OUT_DIR/rhoapi.rs`, expanded into the surfaces it actually produces.
///
/// The expansions are cited, not recalled:
///
/// * `::prost::Message` → `prost-derive-0.14.3/src/lib.rs:173-203` (the
///   `Message` impl: `encode_raw`, `merge_field`, `encoded_len`, `clear`),
///   `:205-212` (`Default`), `:236-243` (`Debug`).
/// * `::prost::Oneof` → `:462-506` (the inherent `encode` / `merge` /
///   `encoded_len`), `:511-518` (`Debug`).
/// * `Clone`, `Copy`, `Eq`, `Ord`, `PartialOrd` → `rustc`'s built-in derives.
/// * `serde::Serialize` / `serde::Deserialize` / `utoipa::ToSchema` → those
///   crates' derives.
///
/// ⚠★ `PartialEq` and `Hash` are deliberately NOT here. `models/build.rs`
/// STRIPS them from prost's output (`line.replace("PartialEq, Eq, Hash,", "")`)
/// and `models/src/lib.rs` writes them BY HAND — `<Par as PartialEq>::eq`
/// ignores `locally_free`, which no derive would do. They are recursive walks
/// all the same, so this table is a LOWER BOUND on the driver list and
/// `HAND_WRITTEN_TRAVERSALS` carries the remainder. A campaign that read its
/// driver list off a `#[derive]` scan alone would miss exactly the two surfaces
/// a hand-picked list of four already missed once.
pub const DERIVE_DISPOSITIONS: &[DeriveTrait] = &[
    DeriveTrait {
        token: "serde::Serialize",
        applies_to: Applies::Both,
        surfaces: &[(
            "Serialize::serialize",
            // Stage H. `models/src/rust/rholang/wire_encode.rs` is the
            // single-walk trampolined encoder driven by `rhoapi_wire.rs`; the
            // derive stays compiled as the differential's oracle.
            Disposition::Converted("wire_encode::encode"),
        )],
    },
    DeriveTrait {
        token: "serde::Deserialize",
        applies_to: Applies::Both,
        surfaces: &[(
            "Deserialize::deserialize",
            // Stage F. `models/src/rust/rholang/par_codec.rs`, the explicit
            // obligation-stack decoder.
            Disposition::Converted("par_codec::cold_decode"),
        )],
    },
    DeriveTrait {
        token: "utoipa::ToSchema",
        applies_to: Applies::Both,
        surfaces: &[(
            "ToSchema::schema",
            Disposition::NotATraversal(
                "walks the SCHEMA once at OpenAPI-document build time, never a term value; \
                 its recursion is bounded by the type graph, not by a deploy",
            ),
        )],
    },
    DeriveTrait {
        token: "Eq",
        applies_to: Applies::Both,
        surfaces: &[(
            "Eq",
            Disposition::NotATraversal(
                "a marker trait with no method; the comparison it certifies is the \
                 HAND-WRITTEN `PartialEq` in models/src/lib.rs, which is not a derive surface",
            ),
        )],
    },
    DeriveTrait {
        token: "Ord",
        applies_to: Applies::Both,
        surfaces: &[(
            "Ord::cmp",
            // Measured for the FIRST time by `four_quadrant_s0_baseline`
            // (rholang/tests/stack_depth_gate.rs): 1,974 B/level debug, 438
            // release. It had never appeared in that gate at all.
            Disposition::Remaining("D-ord"),
        )],
    },
    DeriveTrait {
        token: "PartialOrd",
        applies_to: Applies::Both,
        surfaces: &[(
            "PartialOrd::partial_cmp",
            // rustc's derived `partial_cmp` is a second recursive walk with the
            // same shape as `cmp`, so one driver serves both surfaces.
            Disposition::FollowsFrom("D-ord"),
        )],
    },
    DeriveTrait {
        token: "Clone",
        applies_to: Applies::Both,
        surfaces: &[(
            "Clone::clone",
            // ★★ CONVERTED by stage F-4. The pre-repair readings, re-measured at
            // HEAD by `four_quadrant_s0_baseline` before the conversion, were
            // 16,493 B/level debug and 3,254 release. `9082d12c` had earlier
            // removed a CALL at `inj_attempt`'s set-initial-cost phase and
            // entered that COMPOSITION in `CONVERTED_DEPTH` as
            // `inj_attempt_clone`; `<Par as Clone>::clone` itself is what F-4
            // converts, and the `clone` subject moves to `CONVERTED_DEPTH` with
            // it.
            //
            // ⚠ This value is the CUT-SET answer and it is REFINED PER ITEM — see
            // `refine_disposition`. A `Copy` item keeps its derive
            // (`NotATraversal`) and a non-cut item gets a flat field-wise
            // `Clone` (`FollowsFrom`). Stating only this row would be a false
            // claim about 61 of the 62 generated items.
            Disposition::Converted("term_ops::clone"),
        )],
    },
    DeriveTrait {
        token: "Copy",
        applies_to: Applies::Both,
        surfaces: &[(
            "Copy",
            Disposition::NotATraversal(
                "a marker trait with no method; it lands only on messages whose fields are \
                 all scalars, which by construction contain no term",
            ),
        )],
    },
    DeriveTrait {
        token: "::prost::Message",
        applies_to: Applies::Messages,
        surfaces: &[
            (
                "Message::encode_raw",
                // The S2 deliverable, and the one place a conversion changes
                // NOTHING that is accepted: prost places no limit on the write
                // side (`models/tests/par_prost_depth_ceiling.rs` stage 2).
                Disposition::Remaining("D3 / prost_encode"),
            ),
            (
                "Message::encoded_len",
                // The same driver's first pass, and separately valuable:
                // `Message::encode_to_vec` calls `encoded_len()`
                // (`prost-0.14.3/src/message.rs:61-69`) and then
                // `encoding::message::encode` calls it AGAIN for every nested
                // message (`encoding.rs:788-795`), so the derived path is
                // Theta(d^2) on a depth-d chain and a memoized bottom-up pass is
                // Theta(n).
                Disposition::Remaining("D3 / prost_encode"),
            ),
            (
                "Message::merge_field",
                // Measured for the first time by `four_quadrant_s0_baseline`:
                // 27,794 B/level debug, 4,096 release — the most expensive per
                // level of the eight, and the only one prost itself caps
                // (`models/tests/par_prost_depth_ceiling.rs`).
                Disposition::Remaining("D4 / prost_decode"),
            ),
            (
                "Message::clear",
                // `clear` walks the same child set the encoder does; whichever
                // driver owns the child relation owns this too.
                Disposition::FollowsFrom("D3 / prost_encode"),
            ),
            (
                "Debug::fmt",
                // ⚠ Built from prost-derive's UNSORTED field list (`:85, :214`),
                // not the tag-sorted one `encode_raw` uses. Two orders inside
                // one derive; a driver for this surface must use the
                // DECLARATION order, i.e. the bincode table's.
                Disposition::Remaining("D-debug"),
            ),
            (
                "Default::default",
                Disposition::NotATraversal(
                    "fills every field with its own `Default`, and a message field's default \
                     is `None` / an empty `Vec` — so it constructs one node and descends \
                     into nothing",
                ),
            ),
        ],
    },
    DeriveTrait {
        token: "::prost::Oneof",
        applies_to: Applies::Oneofs,
        surfaces: &[
            ("Oneof::encode", Disposition::Remaining("D3 / prost_encode")),
            (
                "Oneof::encoded_len",
                Disposition::Remaining("D3 / prost_encode"),
            ),
            ("Oneof::merge", Disposition::Remaining("D4 / prost_decode")),
            ("Debug::fmt", Disposition::Remaining("D-debug")),
        ],
    },
];

/// Look one derive token up in the closed table.
///
/// `models/build.rs` calls this on every token its textual scan finds, and
/// **fails the build** on `None`. That is the seventh-trait tripwire.
pub fn disposition_of(token: &str) -> Option<&'static DeriveTrait> {
    DERIVE_DISPOSITIONS.iter().find(|d| d.token == token)
}

// ===========================================================================
// §2  The resolved facts — format-neutral, produced once
// ===========================================================================

/// What a field IS, independent of which format is about to write it.
///
/// ★ This is the type that makes "one walk, two emitters" real. The bincode
/// renderer and the prost renderer both `match` on it; neither re-reads the
/// descriptor, so they cannot form two opinions about what a field is.
#[derive(Clone, Debug)]
enum Shape {
    /// A singular protobuf scalar — `bool`, `string`, `bytes`, any integer
    /// family, `double`. The descriptor's own `Type` selects the bincode
    /// primitive and the prost encoding module alike.
    Scalar(Type),
    /// A `bytes` field named `locally_free`.
    ///
    /// ⚠ The blanking is **serde-only**. The prost renderer treats this exactly
    /// as `Scalar(Type::Bytes)`; a prost table that inherited the blanking would
    /// drop a field from the protobuf wire.
    EmptyBytes,
    /// `Option<Message>`.
    Message { leaf: String },
    /// `Vec<Message>`.
    RepeatedMessage { leaf: String },
    /// `Vec<String>`.
    RepeatedString,
    /// `Vec<Vec<u8>>`.
    RepeatedBytes,
    /// A protobuf `map<K,V>`, which `build.rs`'s `.btree_map(".")` renders as a
    /// `BTreeMap`. The key type and the value's leaf name come from the
    /// synthetic entry message, resolved rather than assumed.
    Map { key: Type, value_leaf: String },
    /// The oneof itself, occupying one struct field.
    Oneof,
}

/// One resolved field of one message, in **serde declaration order**.
///
/// The vector these live in is produced once by [`resolve_message`]; the two
/// emitters read it under two different sort keys. See the module header.
struct Field {
    /// The Rust struct field identifier (already keyword-escaped).
    rust_name: String,
    /// The `FieldKind` variant name, as Rust source — the BINCODE alphabet.
    kind: &'static str,
    /// What this field is, format-neutrally.
    shape: Shape,
    /// Every proto tag this field occupies: one for a plain field, **one per
    /// member** for a oneof.
    ///
    /// ★ It is a vector and not a `u32` because prost sorts a oneof to the
    /// position of its MINIMUM tag (`prost-derive-0.14.3/src/lib.rs:88-90`),
    /// while the ARM writes the tag it actually declares (`:465-471`). Those are
    /// different numbers for every member but the first, and collapsing them
    /// here would decide, silently, which one the table means.
    tags: Vec<u32>,
    /// The message leaf names this field can directly contain, for the child
    /// relation. A oneof's are its message-payload arms, which the `Field`
    /// cannot see and [`resolve_message`] fills in.
    children: Vec<String>,
}

impl Field {
    /// prost's sort key: the smallest tag this field occupies.
    fn min_tag(&self) -> u32 {
        self.tags
            .iter()
            .copied()
            .min()
            .expect("wire_schema: every field occupies at least one proto tag")
    }
}

/// One resolved oneof.
struct Oneof {
    /// Proto oneof name (e.g. `expr_instance`).
    proto_name: String,
    /// Rust enum identifier (e.g. `ExprInstance`).
    rust_ident: String,
    /// Rust module path of the enum (e.g. `expr`).
    module: String,
    /// `(serde_index, rust_variant_ident, payload)` in declaration order.
    variants: Vec<Variant>,
}

struct Variant {
    index: u32,
    rust_ident: String,
    /// The BODY of this arm in the generated `WireOneof::wire_emit`, as Rust
    /// source. `v` binds the payload and `out` is in scope; the arm evaluates
    /// to `Option<&dyn WireNode>`.
    payload_expr: String,
    /// The `&'static [FieldKind]` expression naming this arm's program.
    program_expr: String,
    /// `EX_*`-style constant name.
    const_name: String,
    /// This member's OWN proto tag — what the arm writes on the protobuf wire.
    tag: u32,
    /// The member's protobuf type, so the prost table can name its encoding.
    ty: Type,
    /// For a message payload, the leaf type name; `None` otherwise.
    message_leaf: Option<String>,
}

/// Rust keywords prost escapes with `r#`.
const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof",
    "unsized", "virtual", "yield", "async", "await", "try",
];

fn escape_ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// prost's Rust field name: snake_case, keyword-escaped.
fn rust_field_name(proto: &str) -> String {
    escape_ident(&proto.to_snake_case())
}

/// prost's Rust type/variant name: UpperCamelCase.
fn rust_type_name(proto: &str) -> String {
    proto.to_upper_camel_case()
}

/// The last path segment of a fully-qualified proto type name.
fn type_leaf(fq: &str) -> &str {
    fq.rsplit('.').next().unwrap_or(fq)
}

/// A message together with the chain of messages it is nested inside.
///
/// prost renders a nested message inside a module named after its parent
/// (`Var.WildcardMsg` → `crate::rhoapi::var::WildcardMsg`), so the parent chain
/// is needed to name the type at all — and a generator that assumed every
/// message sits at the package root would fail on exactly one type in this
/// schema, which is the kind of near-miss that reaches production.
struct Message<'a> {
    desc: &'a DescriptorProto,
    /// Outermost first; empty for a top-level message.
    parents: Vec<String>,
}

impl<'a> Message<'a> {
    fn leaf_name(&self) -> &str {
        self.desc.name.as_deref().unwrap_or("")
    }

    /// Rust path relative to `crate::rhoapi`, which the generated file `use`s.
    fn rust_path(&self) -> String {
        let mut path = String::new();
        for parent in &self.parents {
            path.push_str(&parent.to_snake_case());
            path.push_str("::");
        }
        path.push_str(&rust_type_name(self.leaf_name()));
        path
    }

    /// The `&'static [FieldKind]` identifier for this message's program.
    fn program_ident(&self) -> String {
        let mut ident = String::new();
        for parent in &self.parents {
            ident.push_str(&parent.to_uppercase());
            ident.push('_');
        }
        ident.push_str(&rust_type_name(self.leaf_name()).to_uppercase());
        ident.push_str("_PROGRAM");
        ident
    }

    /// The `&'static [ProstField]` identifier for this message's prost program.
    fn prost_program_ident(&self) -> String {
        self.program_ident().replace("_PROGRAM", "_PROST_PROGRAM")
    }

    /// The module path a oneof declared in this message lives in.
    fn oneof_module(&self) -> String {
        let mut path = String::new();
        for parent in &self.parents {
            path.push_str(&parent.to_snake_case());
            path.push_str("::");
        }
        path.push_str(&self.leaf_name().to_snake_case());
        path
    }
}

/// A synthetic protobuf map entry, resolved rather than assumed.
///
/// [`collect`] skips map entries (prost renders them as a `BTreeMap`, not a
/// type), so their key/value types would otherwise be unavailable to
/// [`classify`] — and the prost table needs both, because a map entry is a
/// length-delimited nested message with the key at tag 1 and the value at tag 2
/// (`prost-0.14.3/src/encoding.rs:1044-1059`).
struct MapEntry {
    key: Type,
    value_leaf: String,
}

/// Collect `msg` and every message nested inside it, skipping synthetic map
/// entries — but RECORDING each entry's key and value types, which the prost
/// table needs.
fn collect<'a>(
    msg: &'a DescriptorProto,
    parents: &[String],
    out: &mut Vec<Message<'a>>,
    entries: &mut BTreeMap<String, MapEntry>,
) {
    if is_map_entry(msg) {
        let name = msg.name.clone().unwrap_or_default();
        let key = msg
            .field
            .iter()
            .find(|f| f.number == Some(1))
            .and_then(|f| f.r#type)
            .and_then(|t| Type::try_from(t).ok())
            .unwrap_or_else(|| {
                panic!("wire_schema: map entry `{name}` has no resolvable key field at tag 1")
            });
        let value = msg
            .field
            .iter()
            .find(|f| f.number == Some(2))
            .unwrap_or_else(|| panic!("wire_schema: map entry `{name}` has no value field at tag 2"));
        let value_ty = value
            .r#type
            .and_then(|t| Type::try_from(t).ok())
            .unwrap_or_else(|| panic!("wire_schema: map entry `{name}`'s value has no type"));
        assert_eq!(
            value_ty,
            Type::Message,
            "wire_schema: map entry `{name}` has a {value_ty:?} value. Both drivers descend \
             into a map's VALUES; a scalar-valued map needs a deliberate widening, not a \
             silent reinterpretation."
        );
        entries.insert(
            name,
            MapEntry {
                key,
                value_leaf: type_leaf(value.type_name.as_deref().unwrap_or("")).to_string(),
            },
        );
        return;
    }
    out.push(Message {
        desc: msg,
        parents: parents.to_vec(),
    });
    let mut chain = parents.to_vec();
    chain.push(msg.name.clone().unwrap_or_default());
    for nested in &msg.nested_type {
        collect(nested, &chain, out, entries);
    }
}

/// Where a message's `&'static [FieldKind]` program lives.
///
/// Generated types get theirs in this file; `extern_path`'d types get theirs
/// from the hand-written module, because their layout is not the descriptor's
/// (see [`EXTERN_OVERRIDES`]).
fn program_ident_of(
    leaf: &str,
    extern_set: &BTreeSet<&str>,
    programs: &BTreeMap<String, String>,
) -> String {
    if extern_set.contains(leaf) {
        return format!(
            "crate::rust::rholang::wire::{}_PROGRAM",
            rust_type_name(leaf).to_uppercase()
        );
    }
    programs
        .get(leaf)
        .cloned()
        .unwrap_or_else(|| panic!("wire_schema: no program emitted for `{leaf}`"))
}

/// Is this message a synthetic protobuf map entry (`map<K,V>` desugaring)?
fn is_map_entry(msg: &DescriptorProto) -> bool {
    msg.options
        .as_ref()
        .and_then(|o| o.map_entry)
        .unwrap_or(false)
}

// ===========================================================================
// §3  The one pass
// ===========================================================================

/// Generate all four outputs from one walk of the descriptor.
///
/// # Panics
///
/// Loudly, on every situation where guessing would be a silent consensus
/// hazard: an unsupported field type, an `extern_path`'d type with no
/// registered override, an `optional` scalar (proto3 presence, which prost
/// renders as `Option<scalar>` and this table has no kind for), or a map whose
/// value is not a message.
pub fn generate(fds: &FileDescriptorSet) -> Generated {
    let mut messages: Vec<Message<'_>> = Vec::new();
    let mut map_entries: BTreeMap<String, MapEntry> = BTreeMap::new();
    for file in &fds.file {
        if file.package.as_deref() != Some(PACKAGE) {
            continue;
        }
        for msg in &file.message_type {
            collect(msg, &[], &mut messages, &mut map_entries);
        }
    }
    // Deterministic order: the descriptor's, which is the `.proto`'s.
    let extern_set: BTreeSet<&str> = EXTERN_OVERRIDES.iter().copied().collect();

    // Every message name in the package, so field classification can tell a
    // message-typed field from a scalar without resolving imports. Nested
    // messages are keyed by their LEAF name because that is what a field's
    // `type_name` resolves to once the package prefix is stripped.
    let known: BTreeSet<String> = messages.iter().map(|m| m.leaf_name().to_string()).collect();

    // Leaf name → program identifier, so a field can reference a nested type's
    // program correctly (`WildcardMsg` lives in `crate::rhoapi::var`, not at
    // the package root). Program identifiers embed the full parent chain, so
    // two messages whose leaf names collide cannot share one program.
    let mut programs: BTreeMap<String, String> = BTreeMap::new();
    for msg in &messages {
        if extern_set.contains(msg.leaf_name()) {
            continue;
        }
        let previous = programs.insert(msg.leaf_name().to_string(), msg.program_ident());
        assert!(
            previous.is_none(),
            "wire_schema: two `{PACKAGE}` messages share the leaf name `{}`. A field's \
             `type_name` resolves to the leaf, so one of them would silently adopt the other's \
             field order. Disambiguate before generating.",
            msg.leaf_name()
        );
    }

    // ── ★ THE ONE WALK. Every emitter below reads THIS. ──
    let mut empty_bytes_fields = 0usize;
    let mut oneofs: Vec<Oneof> = Vec::new();
    // Parallel to `messages`, minus the extern ones: `(index into messages,
    // resolved fields)`. Preallocated: the length is known.
    let mut resolved: Vec<(usize, Vec<Field>)> = Vec::with_capacity(messages.len());
    for (i, msg) in messages.iter().enumerate() {
        if extern_set.contains(msg.leaf_name()) {
            continue;
        }
        let (fields, mut msg_oneofs, empties) =
            resolve_message(msg, &known, &extern_set, &programs, &map_entries);
        empty_bytes_fields += empties;
        oneofs.append(&mut msg_oneofs);
        resolved.push((i, fields));
    }

    let extern_count = messages.len() - resolved.len();

    // ── the child relation and its SCC, computed once, read by every emitter ──
    let graph = SchemaGraph::build(&messages, &resolved, &extern_set);

    // ── ★ the CLONE PLAN: Copy-ness, the cut set, and what follows (§4b) ──
    let plan = ClonePlan::build(&messages, &resolved, &oneofs, &extern_set, &graph);

    let bincode = emit_bincode_source(&messages, &resolved, &oneofs, &extern_set);
    let prost = emit_prost_source(&messages, &resolved, &oneofs, &extern_set);
    let (term_ops, clone_impls) = emit_term_ops_source(
        &messages,
        &resolved,
        &oneofs,
        &extern_set,
        &graph,
        &plan,
    );
    let (schema_meta, derive_row_count) =
        emit_schema_meta_source(&messages, &oneofs, &extern_set, &graph, &plan);

    // ★ THE NON-VACUITY FLOOR, at the generator rather than only at the consumer.
    //
    // An emitter that returned `String::new()` would satisfy every count in this
    // struct and the build would pass in silence. `models/build.rs` already
    // refuses a zero-byte output; these refuse an output that is present but
    // says nothing. The bound is the SCHEMA's own arithmetic — every message and
    // oneof is either `Copy` or gets an impl — so it cannot be satisfied by a
    // placeholder and does not need updating when the `.proto` grows.
    let copy_items = messages
        .iter()
        .filter(|m| !extern_set.contains(m.leaf_name()) && plan.message_is_copy[m.leaf_name()])
        .count()
        + oneofs
            .iter()
            .filter(|o| plan.oneof_is_copy[&o.rust_ident])
            .count();
    assert_eq!(
        clone_impls.len(),
        resolved.len() + oneofs.len() - copy_items,
        "wire_schema: the term-op emitter wrote {} `impl Clone`s, but the schema has {} \
         generated messages + {} oneofs of which {} are `Copy`, i.e. {} non-`Copy` items that \
         MUST each get one. An item with neither a derive nor an emitted impl does not compile; \
         an item with both does not compile either. This is the arithmetic that makes \
         `EMITTED_TRAVERSALS` non-vacuous.",
        clone_impls.len(),
        resolved.len(),
        oneofs.len(),
        copy_items,
        resolved.len() + oneofs.len() - copy_items
    );
    assert!(
        term_ops.len() > 32 * 1024,
        "wire_schema: the term-op source is only {} bytes. The `Clone` emission for {} items \
         over a {}-type descend set cannot fit in that, so the emitter has silently stopped \
         emitting bodies — the exact failure a `String::new()` return would produce, and the \
         reason this floor is here rather than only a `!is_empty()` check in `models/build.rs`.",
        term_ops.len(),
        clone_impls.len(),
        plan.entered.len() + plan.oneof_entered.len()
    );

    Generated {
        counts: Counts {
            empty_bytes_fields,
            message_count: resolved.len(),
            extern_count,
            oneof_count: oneofs.len(),
            scc_count: graph.scc.len(),
            recursive_type_count: graph.recursive.len(),
            derive_row_count,
            clone_cut_set: plan.cut.iter().map(|c| rust_type_name(c)).collect(),
            clone_descend_count: plan.entered.len() + plan.oneof_entered.len(),
            clone_residual_height: plan.residual_height,
        },
        clone_impls,
        sources: vec![
            (OUTPUT_BINCODE, bincode),
            (OUTPUT_PROST, prost),
            (OUTPUT_TERM_OPS, term_ops),
            (OUTPUT_SCHEMA_META, schema_meta),
        ],
    }
}

/// Resolve one message into its field program, in **serde declaration order**.
fn resolve_message(
    message: &Message<'_>,
    known: &BTreeSet<String>,
    extern_set: &BTreeSet<&str>,
    programs: &BTreeMap<String, String>,
    map_entries: &BTreeMap<String, MapEntry>,
) -> (Vec<Field>, Vec<Oneof>, usize) {
    let msg = message.desc;
    let msg_name = msg.name.clone().unwrap_or_default();
    let mut fields: Vec<Field> = Vec::with_capacity(msg.field.len());
    let mut oneofs: Vec<Oneof> = Vec::with_capacity(msg.oneof_decl.len());
    let mut empty_bytes = 0usize;

    // ⚠★ THE DECLARATION-ORDER RULE, which is the SERDE order and not the
    // protobuf one.
    //
    // prost-build writes **every plain field first, in declaration order, and
    // then every oneof field, in `oneof_decl` order** into the Rust struct —
    // `prost-build-0.14.3/src/code_generator.rs:270-291` is two separate loops
    // over `fields` and `oneof_fields`. serde's derive is positional over the
    // struct, so this IS the bincode layout.
    //
    // The difference against a "oneof sits where its first member appears" rule
    // is invisible in four of the five oneofs in this schema (they are their
    // message's only field) and decisive in the fifth: `TaggedContinuation`
    // declares `oneof tagged_cont { … }` BEFORE `guard`, so the intuitive rule
    // emits `tagged_cont` first while prost-build emits `guard` first. That is a
    // 95-byte encoding whose two halves are simply exchanged — same length, same
    // bytes, different order — which no length check and no round-trip could
    // ever see. The write differential caught it on the first run; it is
    // recorded here so it cannot be "simplified" back.
    //
    // ⚠⚠ And for the PROTOBUF wire the correct order for that same message is
    // the OPPOSITE one (`tagged_cont` occupies tags 1-2, `guard` tag 3), which
    // is why the vector this function returns is sorted by the *consumer*. See
    // the module header.
    for field in &msg.field {
        // proto3 `optional` is modelled as a synthetic one-member oneof. prost
        // renders it `Option<scalar>`, which this table has no kind for; refuse
        // rather than guess.
        if field.proto3_optional.unwrap_or(false) {
            panic!(
                "wire_schema: `{msg_name}.{}` uses proto3 `optional` presence, which prost \
                 renders as `Option<scalar>`. No `FieldKind` models that. Add one deliberately \
                 (and a decoder arm) rather than letting the table guess.",
                field.name.clone().unwrap_or_default()
            );
        }
        if field.oneof_index.is_some() {
            continue; // emitted below, after every plain field
        }
        let (kind, shape, is_empty_bytes) =
            classify(&msg_name, field, known, extern_set, map_entries);
        if is_empty_bytes {
            empty_bytes += 1;
        }
        let proto_name = field.name.clone().unwrap_or_default();
        let tag = field
            .number
            .unwrap_or_else(|| panic!("wire_schema: `{msg_name}.{proto_name}` has no proto tag"));
        assert!(
            tag > 0,
            "wire_schema: `{msg_name}.{proto_name}` has proto tag {tag}; protobuf tags start \
             at 1 and prost's sort key would place a zero ahead of everything."
        );
        let children = shape_children(&shape);
        fields.push(Field {
            rust_name: rust_field_name(&proto_name),
            kind,
            shape,
            tags: vec![tag as u32],
            children,
        });
    }

    for (idx, decl) in msg.oneof_decl.iter().enumerate() {
        let proto_name = decl.name.clone().unwrap_or_default();
        let rust_ident = rust_type_name(&proto_name);
        let module = message.oneof_module();
        let rust_name = rust_field_name(&proto_name);
        let variants = resolve_oneof_variants(msg, idx, &rust_ident, known, extern_set, programs);
        assert!(
            !variants.is_empty(),
            "wire_schema: oneof `{msg_name}.{proto_name}` has no members; prost would emit no \
             enum and the table would name a type that does not exist."
        );
        fields.push(Field {
            rust_name,
            kind: "Oneof",
            shape: Shape::Oneof,
            // ★ EVERY member's tag. prost sorts the oneof to the position of the
            // MINIMUM of these (`prost-derive-0.14.3/src/lib.rs:88-90`), which is
            // a documented quirk this generator REPRODUCES rather than fixes.
            tags: variants.iter().map(|v| v.tag).collect(),
            children: variants.iter().filter_map(|v| v.message_leaf.clone()).collect(),
        });
        oneofs.push(Oneof {
            proto_name,
            rust_ident,
            module,
            variants,
        });
    }

    (fields, oneofs, empty_bytes)
}

/// The message leaf names a non-oneof shape can directly contain.
fn shape_children(shape: &Shape) -> Vec<String> {
    match shape {
        Shape::Scalar(_) | Shape::EmptyBytes | Shape::RepeatedString | Shape::RepeatedBytes => {
            Vec::new()
        }
        Shape::Message { leaf } | Shape::RepeatedMessage { leaf } => vec![leaf.clone()],
        Shape::Map { value_leaf, .. } => vec![value_leaf.clone()],
        // A oneof's children are its message-payload members, which
        // `resolve_message` fills in from the resolved variants.
        Shape::Oneof => Vec::new(),
    }
}

/// Classify a non-oneof field into `(FieldKind, Shape, is_empty_bytes)`.
///
/// ★ Format-neutral by construction: it returns FACTS. The bincode emission and
/// the prost emission are both rendered from the returned [`Shape`], so the two
/// tables cannot form two opinions about what a field is.
fn classify(
    msg_name: &str,
    field: &FieldDescriptorProto,
    known: &BTreeSet<String>,
    extern_set: &BTreeSet<&str>,
    map_entries: &BTreeMap<String, MapEntry>,
) -> (&'static str, Shape, bool) {
    let proto_name = field.name.clone().unwrap_or_default();
    let repeated = field.label == Some(Label::Repeated as i32);
    let ty = Type::try_from(field.r#type.unwrap_or(0)).unwrap_or_else(|_| {
        panic!("wire_schema: `{msg_name}.{proto_name}` has an unrecognised protobuf type")
    });

    // A protobuf `map<K,V>` arrives as a REPEATED synthetic MapEntry message.
    if repeated && ty == Type::Message {
        let leaf = type_leaf(field.type_name.as_deref().unwrap_or(""));
        if let Some(entry) = map_entries.get(leaf) {
            // `build.rs` sets `.btree_map(".")`, so prost renders it
            // `BTreeMap<K, V>`; serde emits a MAP (u64 count, then key/value
            // PAIRS), not a seq. The only such field is `New.injections`.
            assert_eq!(
                leaf, "InjectionsEntry",
                "wire_schema: `{msg_name}.{proto_name}` is a map whose entry type is `{leaf}`. \
                 The driver's `Descent::Map` is typed `BTreeMap<String, Par>`; a second map \
                 shape needs a deliberate widening, not a silent reinterpretation."
            );
            return (
                "Map",
                Shape::Map {
                    key: entry.key,
                    value_leaf: entry.value_leaf.clone(),
                },
                false,
            );
        }
    }

    match (repeated, ty) {
        (true, Type::Message) => {
            let leaf = type_leaf(field.type_name.as_deref().unwrap_or(""));
            assert!(
                known.contains(leaf) || extern_set.contains(leaf),
                "wire_schema: `{msg_name}.{proto_name}` is a repeated `{leaf}`, which is not a \
                 `{PACKAGE}` message. Cross-package descent is not modelled."
            );
            (
                "Seq",
                Shape::RepeatedMessage {
                    leaf: leaf.to_string(),
                },
                false,
            )
        }
        (true, Type::String) => ("StrSeq", Shape::RepeatedString, false),
        (true, Type::Bytes) => ("BytesSeq", Shape::RepeatedBytes, false),
        (true, other) => panic!(
            "wire_schema: `{msg_name}.{proto_name}` is a repeated {other:?}. Only repeated \
             message / string / bytes appear in this schema; a new one needs a `FieldKind`."
        ),
        (false, Type::Message) => {
            let leaf = type_leaf(field.type_name.as_deref().unwrap_or(""));
            assert!(
                known.contains(leaf) || extern_set.contains(leaf),
                "wire_schema: `{msg_name}.{proto_name}` is a `{leaf}`, which is not a \
                 `{PACKAGE}` message. Cross-package descent is not modelled."
            );
            (
                "Opt",
                Shape::Message {
                    leaf: leaf.to_string(),
                },
                false,
            )
        }
        (false, Type::Bool) => ("Bool", Shape::Scalar(Type::Bool), false),
        (false, Type::String) => ("Str", Shape::Scalar(Type::String), false),
        (false, Type::Bytes) => {
            // ⚠ THE SERIALIZE-ONLY ASYMMETRY. `models/build.rs` injects
            // `serialize_with = serialize_as_empty_bytes` on exactly the
            // `locally_free` bytes fields: written as eight zero bytes, read
            // back at the stream's REAL length. Same rule, same place.
            //
            // ⚠ It is a SERDE asymmetry. The prost renderer treats `EmptyBytes`
            // as ordinary `bytes` — see [`Shape::EmptyBytes`].
            if proto_name.to_snake_case() == "locally_free" {
                ("EmptyBytes", Shape::EmptyBytes, true)
            } else {
                ("Bytes", Shape::Scalar(Type::Bytes), false)
            }
        }
        (false, t @ (Type::Int32 | Type::Sint32 | Type::Sfixed32)) => {
            ("I32", Shape::Scalar(t), false)
        }
        (false, t @ (Type::Uint32 | Type::Fixed32)) => ("U32", Shape::Scalar(t), false),
        (false, t @ (Type::Int64 | Type::Sint64 | Type::Sfixed64)) => {
            ("I64", Shape::Scalar(t), false)
        }
        (false, t @ (Type::Uint64 | Type::Fixed64)) => ("U64", Shape::Scalar(t), false),
        (false, other) => panic!(
            "wire_schema: `{msg_name}.{proto_name}` has type {other:?}, which has no \
             `FieldKind`. Add one deliberately — silently widening it to the nearest integer \
             would change the byte layout and fork consensus."
        ),
    }
}

/// Resolve the members of oneof `idx` into declaration-ordered variants.
fn resolve_oneof_variants(
    msg: &DescriptorProto,
    idx: usize,
    oneof_ident: &str,
    known: &BTreeSet<String>,
    extern_set: &BTreeSet<&str>,
    programs: &BTreeMap<String, String>,
) -> Vec<Variant> {
    let msg_name = msg.name.clone().unwrap_or_default();
    let mut out = Vec::new();
    let mut index = 0u32;
    for field in &msg.field {
        if field.oneof_index != Some(idx as i32) {
            continue;
        }
        let proto_name = field.name.clone().unwrap_or_default();
        let rust_ident = rust_type_name(&proto_name);
        let const_name = format!(
            "{}{}",
            oneof_const_prefix(oneof_ident),
            proto_name.to_snake_case().to_uppercase()
        );
        let ty = Type::try_from(field.r#type.unwrap_or(0)).unwrap_or_else(|_| {
            panic!("wire_schema: `{msg_name}.{proto_name}` has an unrecognised protobuf type")
        });
        assert!(
            field.label != Some(Label::Repeated as i32),
            "wire_schema: `{msg_name}.{proto_name}` is a repeated oneof member, which protobuf \
             does not allow and this table cannot model."
        );
        let tag = field.number.unwrap_or_else(|| {
            panic!("wire_schema: oneof member `{msg_name}.{proto_name}` has no proto tag")
        });
        assert!(
            tag > 0,
            "wire_schema: oneof member `{msg_name}.{proto_name}` has proto tag {tag}; protobuf \
             tags start at 1 and prost's sort key would place a zero ahead of everything."
        );
        let mut message_leaf = None;
        let (payload_expr, program_expr) = match ty {
            Type::Message => {
                let leaf = type_leaf(field.type_name.as_deref().unwrap_or(""));
                assert!(
                    known.contains(leaf) || extern_set.contains(leaf),
                    "wire_schema: oneof member `{msg_name}.{proto_name}` is a `{leaf}`, which \
                     is not a `{PACKAGE}` message."
                );
                let program = program_ident_of(leaf, extern_set, programs);
                message_leaf = Some(leaf.to_string());
                (format!("{{ put_u32(out, {index}); Some(v) }}"), program)
            }
            Type::Bool => (
                format!("{{ put_u32(out, {index}); put_bool(out, *v); None }}"),
                "&[FieldKind::Bool]".to_string(),
            ),
            Type::String => (
                format!("{{ put_u32(out, {index}); put_str(out, v); None }}"),
                "&[FieldKind::Str]".to_string(),
            ),
            Type::Bytes => (
                format!("{{ put_u32(out, {index}); put_bytes(out, v); None }}"),
                "&[FieldKind::Bytes]".to_string(),
            ),
            Type::Int32 | Type::Sint32 | Type::Sfixed32 => (
                format!("{{ put_u32(out, {index}); put_i32(out, *v); None }}"),
                "&[FieldKind::I32]".to_string(),
            ),
            Type::Uint32 | Type::Fixed32 => (
                format!("{{ put_u32(out, {index}); put_u32(out, *v); None }}"),
                "&[FieldKind::U32]".to_string(),
            ),
            Type::Int64 | Type::Sint64 | Type::Sfixed64 => (
                format!("{{ put_u32(out, {index}); put_i64(out, *v); None }}"),
                "&[FieldKind::I64]".to_string(),
            ),
            Type::Uint64 | Type::Fixed64 => (
                format!("{{ put_u32(out, {index}); put_u64(out, *v); None }}"),
                "&[FieldKind::U64]".to_string(),
            ),
            other => panic!(
                "wire_schema: oneof member `{msg_name}.{proto_name}` has type {other:?}, which \
                 has no `Payload` shape."
            ),
        };
        out.push(Variant {
            index,
            rust_ident,
            payload_expr,
            program_expr,
            const_name,
            tag: tag as u32,
            ty,
            message_leaf,
        });
        index += 1;
    }
    out
}

/// The short constant prefix each oneof's indices are named with.
///
/// This is the one place a *naming* judgement is encoded, so it is an explicit
/// registry rather than an abbreviation rule that would silently produce
/// something plausible for a oneof nobody reviewed. An unregistered oneof is a
/// build failure, not a guess.
fn oneof_const_prefix(oneof_ident: &str) -> &'static str {
    match oneof_ident {
        "ExprInstance" => "EX_",
        "ConnectiveInstance" => "CN_",
        "VarInstance" => "VI_",
        "UnfInstance" => "UF_",
        "TaggedCont" => "TC_",
        other => panic!(
            "wire_schema: oneof `{other}` has no registered constant prefix. Add one to \
             `oneof_const_prefix` deliberately — an auto-abbreviation could collide with an \
             existing prefix and silently re-point a decoder arm."
        ),
    }
}

// ===========================================================================
// §4  The child relation and its SCC
// ===========================================================================

/// The message containment graph, and Tarjan's decomposition of it.
///
/// ★ Computed **once**, from the same resolved fields the emitters read, and
/// consumed by every driver that needs to know "which types can contain
/// themselves". Every recursive walk in this schema is a walk over
/// [`SchemaGraph::recursive`]; a driver that bounded only the types somebody
/// remembered would leave the rest recursive, which is how a Θ(depth) traversal
/// survives an audit.
struct SchemaGraph {
    /// Node names, in descriptor order (leaf names, including extern types).
    names: Vec<String>,
    /// `children[i]` = the leaf names `names[i]` can directly contain, deduped,
    /// in field order.
    children: Vec<Vec<String>>,
    /// Strongly connected components, as index lists, in Tarjan's completion
    /// order (which is a reverse topological order of the condensation).
    scc: Vec<Vec<usize>>,
    /// Names that can contain themselves: every member of an SCC of size > 1,
    /// plus every singleton with a self-edge.
    recursive: BTreeSet<String>,
}

impl SchemaGraph {
    fn build(
        messages: &[Message<'_>],
        resolved: &[(usize, Vec<Field>)],
        extern_set: &BTreeSet<&str>,
    ) -> SchemaGraph {
        let names: Vec<String> = messages.iter().map(|m| m.leaf_name().to_string()).collect();
        let index_of: BTreeMap<&str, usize> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();

        let mut children: Vec<Vec<String>> = vec![Vec::new(); names.len()];
        for (message_index, fields) in resolved {
            let slot = &mut children[*message_index];
            for field in fields {
                for child in &field.children {
                    if !slot.contains(child) {
                        slot.push(child.clone());
                    }
                }
            }
        }

        // ⚠ An EXTERN type contributes no resolved fields, so its row is empty —
        // and that is a statement, not an omission: `EPathMap`'s containment is
        // hand-written (`models/src/rust/rholang/wire.rs`), so the descriptor
        // cannot supply its children and the graph must not pretend it can. The
        // emitted table says so per row rather than leaving a reader to infer
        // that a leaf is a leaf.
        for (i, name) in names.iter().enumerate() {
            if extern_set.contains(name.as_str()) {
                assert!(
                    children[i].is_empty(),
                    "wire_schema: extern type `{name}` acquired descriptor-derived children, \
                     which means it stopped being extern without this table noticing"
                );
            }
        }

        let adjacency: Vec<Vec<usize>> = children
            .iter()
            .map(|row| {
                row.iter()
                    .filter_map(|c| index_of.get(c.as_str()).copied())
                    .collect()
            })
            .collect();

        let scc = tarjan_scc(&adjacency);

        let mut recursive: BTreeSet<String> = BTreeSet::new();
        for component in &scc {
            let self_looping =
                component.len() == 1 && adjacency[component[0]].contains(&component[0]);
            if component.len() > 1 || self_looping {
                for &i in component {
                    recursive.insert(names[i].clone());
                }
            }
        }

        SchemaGraph {
            names,
            children,
            scc,
            recursive,
        }
    }

    /// The transitive closure of the child relation: `out[i]` is every index
    /// reachable from `i` by one or more edges.
    ///
    /// ★ Needed by [`ClonePlan`] for two distinct questions — prost's
    /// `is_nested(field_type, owner)` guard on `Copy`, and "can this type reach
    /// the cut set" — so it is computed once here, over the same adjacency the
    /// SCC was computed from, rather than twice from two readings of `children`.
    ///
    /// Iterative worklist rather than Warshall: the graph is 58 nodes with ~90
    /// edges, so the sparse form is both faster and, more to the point, uses no
    /// native stack — this is the build script of a campaign about recursive
    /// walks (the same reasoning [`tarjan_scc`] carries).
    fn reachability(&self) -> Vec<BTreeSet<usize>> {
        let index_of: BTreeMap<&str, usize> = self
            .names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();
        let adjacency: Vec<Vec<usize>> = self
            .children
            .iter()
            .map(|row| {
                row.iter()
                    .filter_map(|c| index_of.get(c.as_str()).copied())
                    .collect()
            })
            .collect();

        let mut out: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); self.names.len()];
        for root in 0..self.names.len() {
            let mut work: Vec<usize> = adjacency[root].clone();
            while let Some(v) = work.pop() {
                if out[root].insert(v) {
                    work.extend(adjacency[v].iter().copied());
                }
            }
        }
        out
    }
}

/// Tarjan's strongly-connected-components algorithm, ITERATIVE.
///
/// ⚠ Iterative deliberately. This is the build script for a campaign whose
/// entire subject is recursive walks that consume native stack proportional to
/// their input; a recursive Tarjan here would be the same defect, one level up,
/// over a graph an author of `RhoTypes.proto` controls.
///
/// Returns components in completion order, which is a reverse topological order
/// of the condensation — the order a bottom-up driver wants.
///
/// R. Tarjan, *Depth-First Search and Linear Graph Algorithms*, SIAM J. Comput.
/// 1(2):146-160, 1972. <https://doi.org/10.1137/0201010>
fn tarjan_scc(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNVISITED: usize = usize::MAX;
    let n = adjacency.len();

    let mut index = vec![UNVISITED; n];
    let mut lowlink = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut component_stack: Vec<usize> = Vec::with_capacity(n);
    let mut next_index = 0usize;
    let mut components: Vec<Vec<usize>> = Vec::new();

    // (node, next child slot to explore) — the explicit call stack.
    let mut work: Vec<(usize, usize)> = Vec::with_capacity(n);

    for root in 0..n {
        if index[root] != UNVISITED {
            continue;
        }
        index[root] = next_index;
        lowlink[root] = next_index;
        next_index += 1;
        component_stack.push(root);
        on_stack[root] = true;
        work.push((root, 0));

        while let Some(&mut (v, ref mut child_slot)) = work.last_mut() {
            if *child_slot < adjacency[v].len() {
                let w = adjacency[v][*child_slot];
                *child_slot += 1;
                if index[w] == UNVISITED {
                    index[w] = next_index;
                    lowlink[w] = next_index;
                    next_index += 1;
                    component_stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    lowlink[v] = lowlink[v].min(index[w]);
                }
                continue;
            }

            work.pop();
            if lowlink[v] == index[v] {
                let mut component = Vec::new();
                loop {
                    let w = component_stack
                        .pop()
                        .expect("tarjan: the component stack cannot empty before its root");
                    on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                components.push(component);
            }
            if let Some(&mut (parent, _)) = work.last_mut() {
                lowlink[parent] = lowlink[parent].min(lowlink[v]);
            }
        }
    }

    components
}

// ===========================================================================
// §4b  ★★ THE CLONE PLAN — the CUT SET, and what follows from it
// ===========================================================================
//
// The term-op emitter (§7) needs three facts that §4's graph does not yet state,
// and all three are DERIVED here rather than named by hand.
//
// ## 1. Which items are `Copy`
//
// prost derives `Copy` for a message when every field can
// (`prost-build-0.14.3/src/context.rs:183-233`), and for a oneof enum when every
// MEMBER field can (`code_generator.rs:636-646`). A `Copy` type's `Clone` is
// `*self` — a bitwise copy, not a walk — so `models/build.rs` must LEAVE the
// `Clone` token on those derive lines, and this pass must not emit an `impl
// Clone` that would collide with it.
//
// ⚠ The rule is reproduced from prost's source, not guessed, and the reproduction
// is CROSS-CHECKED: `models/build.rs` compares the type names its textual strip
// pass actually stripped against the names this pass says are non-`Copy`, as
// SETS, in both directions. A divergence fails the build naming the type. Two
// independent rules, checked at the one point where both are visible — the same
// shape as the `locally_free` cross-check.
//
// ## 2. The CUT SET — a feedback vertex set of the child relation
//
// ★★ This is the load-bearing derivation, and it is what makes ONE driver serve
// the whole family. `<Send as Clone>::clone` is Θ(depth) only because it reaches
// `<Par as Clone>::clone`, which is Θ(depth). Make `Par::clone` iterative and
// `Send::clone` becomes FLAT for free: one native frame, then a driven clone.
//
// So the emitter does not need a driver per recursive type. It needs a driver for
// a set of types whose removal makes the child relation ACYCLIC — a *feedback
// vertex set* — because a field-wise `clone` over a DAG has native depth bounded
// by the DAG's height, which is a constant of the schema and not a function of
// the term.
//
// Finding a MINIMUM feedback vertex set is NP-hard (R. M. Karp, *Reducibility
// Among Combinatorial Problems*, 1972, <https://doi.org/10.1007/978-1-4684-2001-2_9>),
// so [`ClonePlan::build`] does not claim minimality. It takes the greedy
// max-degree heuristic and then **verifies the result**: after the cut, every SCC
// of the residual graph must be trivial. Correctness is proved, minimality is a
// heuristic, and the difference is stated rather than blurred. On this schema the
// answer is the singleton `{Par}`, which the emitted table publishes.
//
// ## 3. Which types the driver ENTERS
//
// A field whose type cannot reach the cut set is **bounded**: its whole value is
// cloned in one call, exactly as the derive did, because that call is flat. A
// field whose type CAN reach the cut set is **entered**: the driver walks its
// shell inline and suspends at the cut-set nodes below it. `reaches_cut` is that
// predicate, and `entered` is the sub-part of it the driver can actually arrive
// at from a cut-set member — emitting families for the rest would be dead code.
//
// ⚠★ An EXTERN type is bounded, and for `EPathMap` that is a FACT ABOUT ITS
// `Clone` rather than a consequence of externness: `EPathMap::clone` is O(1) AT
// THE NODE (`models/src/rust/rhoapi_ext.rs`) — `ps` is an `EntryTrie` whose clone
// is a refcount bump on the trie root plus an `Arc` bump on the memoized
// projection, and the shadow cell is an `OnceLock<Arc<_>>` clone. It never
// re-enters `Par::clone` at all. The obligation is stated in the emitted file and
// MEASURED by `rholang/tests/stack_depth_gate.rs`'s `clone_pathmap_chain`
// subject, which nests through `EPathmapBody` and is gated flat.

/// Cut-set members for which an ITERATIVE teardown exists.
///
/// ★ A **typed exception table**, not a list somebody keeps in step. If a panic
/// unwinds out of the driver, the pooled value stack still owns cloned terms, and
/// releasing them with `Vec::clear` would run `drop_in_place::<Par>` — itself
/// Θ(depth), the one destructor this campaign measured and deliberately did NOT
/// convert (`rholang/tests/stack_depth_gate.rs`, subject `par_drop`; `impl Drop
/// for Par` produces 353 diagnostics across 61 unique lines in `models` alone).
/// So every cut-set member must name a teardown that is iterative.
///
/// [`ClonePlan::build`] **panics** if the derived cut set acquires a member with
/// no entry here, naming the member and what it needs. A `_ =>` arm in the
/// generated teardown would have been the silent alternative.
const ITERATIVE_TEARDOWN: &[(&str, &str)] =
    &[("Par", "crate::rust::rholang::par_children::dismantle")];

/// The protobuf scalar types whose Rust rendering is `Copy`.
///
/// Verbatim `prost-build-0.14.3/src/context.rs:220-233`'s `matches!` arm: every
/// numeric family, `bool` and `enum`. `String` and `Bytes` are absent because
/// prost renders them as `String` / `Vec<u8>`, which own a heap allocation.
fn scalar_is_copy(ty: Type) -> bool {
    matches!(
        ty,
        Type::Float
            | Type::Double
            | Type::Int32
            | Type::Int64
            | Type::Uint32
            | Type::Uint64
            | Type::Sint32
            | Type::Sint64
            | Type::Fixed32
            | Type::Fixed64
            | Type::Sfixed32
            | Type::Sfixed64
            | Type::Bool
            | Type::Enum
    )
}

/// Everything §7 needs to know about the schema that §4's graph does not state.
struct ClonePlan {
    /// Leaf name → does prost derive `Copy` for this message?
    message_is_copy: BTreeMap<String, bool>,
    /// Oneof `rust_ident` → does prost derive `Copy` for this enum?
    oneof_is_copy: BTreeMap<String, bool>,
    /// The feedback vertex set, in descriptor order. Every emitted driver has one
    /// of these as its `Node`.
    cut: Vec<String>,
    /// Leaf names that can reach a cut-set member (cut members themselves
    /// included when they can reach one, which in a cyclic schema they do).
    reaches_cut: BTreeSet<String>,
    /// Oneof `rust_ident`s whose members can reach a cut-set member.
    oneof_reaches_cut: BTreeSet<String>,
    /// Leaf names the driver actually ENTERS: reachable from a cut-set member
    /// through types that reach the cut set. The families in §7 cover exactly
    /// this set, so nothing emitted is dead.
    entered: BTreeSet<String>,
    /// Oneof `rust_ident`s the driver enters.
    oneof_entered: BTreeSet<String>,
    /// The height of the residual (cut-removed) child relation — the constant
    /// that bounds a field-wise `clone`'s native recursion. Published in the
    /// emitted file so "bounded by a constant" is a number.
    residual_height: usize,
}

impl ClonePlan {
    fn build(
        messages: &[Message<'_>],
        resolved: &[(usize, Vec<Field>)],
        oneofs: &[Oneof],
        extern_set: &BTreeSet<&str>,
        graph: &SchemaGraph,
    ) -> ClonePlan {
        // ── 1. Copy-ness, as a monotone fixed point ──
        //
        // prost's rule is recursive over field types and guarded by
        // `is_nested(field_type, message)` — "would this field make the message
        // recursive?" — which returns `false` (not Copy) and thereby cuts every
        // cycle. So the fixed point below, started at all-true and only ever
        // falsifying, is exact rather than optimistic: a cyclic dependency is
        // resolved by the reachability guard before the recursion can spin.
        let reachable = graph.reachability();
        let index_of: BTreeMap<&str, usize> = graph
            .names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();
        let reaches = |from: &str, to: &str| -> bool {
            match (index_of.get(from), index_of.get(to)) {
                (Some(&f), Some(&t)) => reachable[f].contains(&t),
                _ => false,
            }
        };

        let mut message_is_copy: BTreeMap<String, bool> = BTreeMap::new();
        for msg in messages {
            // ⚠ An EXTERN type is NOT `Copy`: prost does not generate it, and the
            // hand-written `EPathMap` owns an `EntryTrie` and a `Vec<u8>`.
            // Answering `true` here would let a containing message claim `Copy`.
            let is_extern = extern_set.contains(msg.leaf_name());
            message_is_copy.insert(msg.leaf_name().to_string(), !is_extern);
        }
        // The raw descriptor fields, per message — prost's rule reads THESE
        // (oneof members included, because `DescriptorProto::field` carries them).
        let raw: BTreeMap<&str, &DescriptorProto> =
            messages.iter().map(|m| (m.leaf_name(), m.desc)).collect();

        let mut changed = true;
        while changed {
            changed = false;
            for msg in messages {
                let leaf = msg.leaf_name();
                if extern_set.contains(leaf) {
                    continue;
                }
                let all_copy = msg
                    .desc
                    .field
                    .iter()
                    .all(|f| field_is_copy(leaf, f, &message_is_copy, &reaches));
                let slot = message_is_copy
                    .get_mut(leaf)
                    .expect("wire_schema: every message seeded a Copy slot");
                if *slot != all_copy {
                    *slot = all_copy;
                    changed = true;
                }
            }
        }

        // ── the oneof enums: `Copy` iff every MEMBER field is ──
        let mut oneof_is_copy: BTreeMap<String, bool> = BTreeMap::new();
        for oneof in oneofs {
            let owner = owner_of_oneof(messages, oneof);
            let desc = raw
                .get(owner.as_str())
                .unwrap_or_else(|| panic!("wire_schema: oneof `{}` has no owner message", oneof.rust_ident));
            let idx = desc
                .oneof_decl
                .iter()
                .position(|d| d.name.as_deref() == Some(oneof.proto_name.as_str()))
                .unwrap_or_else(|| {
                    panic!(
                        "wire_schema: `{owner}` does not declare a oneof named `{}`",
                        oneof.proto_name
                    )
                });
            let all_copy = desc
                .field
                .iter()
                .filter(|f| f.oneof_index == Some(idx as i32))
                .all(|f| field_is_copy(&owner, f, &message_is_copy, &reaches));
            oneof_is_copy.insert(oneof.rust_ident.clone(), all_copy);
        }

        // ── 2. the CUT SET: greedy max-degree, then VERIFIED acyclic ──
        let adjacency: Vec<Vec<usize>> = graph
            .children
            .iter()
            .map(|row| {
                row.iter()
                    .filter_map(|c| index_of.get(c.as_str()).copied())
                    .collect()
            })
            .collect();

        let mut in_degree = vec![0usize; graph.names.len()];
        for row in &adjacency {
            for &t in row {
                in_degree[t] += 1;
            }
        }

        let mut cut_indices: BTreeSet<usize> = BTreeSet::new();
        loop {
            // The still-cyclic part of the residual graph.
            let residual: Vec<Vec<usize>> = adjacency
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    if cut_indices.contains(&i) {
                        Vec::new()
                    } else {
                        row.iter()
                            .copied()
                            .filter(|t| !cut_indices.contains(t))
                            .collect()
                    }
                })
                .collect();
            let cyclic: BTreeSet<usize> = tarjan_scc(&residual)
                .into_iter()
                .filter(|component| {
                    component.len() > 1
                        || (component.len() == 1 && residual[component[0]].contains(&component[0]))
                })
                .flatten()
                .collect();
            if cyclic.is_empty() {
                break;
            }
            // ★ Deterministic: highest total degree first, then DESCRIPTOR ORDER.
            // A tie broken by hash iteration order would make the generated file
            // depend on the allocator, and the byte-identity golden would catch
            // it as a mystery.
            let pick = cyclic
                .iter()
                .copied()
                .max_by_key(|&i| (adjacency[i].len() + in_degree[i], usize::MAX - i))
                .expect("wire_schema: a non-empty cyclic set has a maximum");
            cut_indices.insert(pick);
        }

        let cut: Vec<String> = cut_indices
            .iter()
            .map(|&i| graph.names[i].clone())
            .collect();
        assert!(
            !cut.is_empty(),
            "wire_schema: the CLONE CUT SET is EMPTY. The `rhoapi` child relation is cyclic by \
             construction — `Par` contains `Send` which contains `Par` — so an empty cut set \
             means the graph this pass read is not the schema's. A vacuous cut set would emit no \
             driver at all and every `Clone` would silently stay Θ(depth)."
        );
        for name in &cut {
            let rust = rust_type_name(name);
            assert!(
                ITERATIVE_TEARDOWN.iter().any(|(ty, _)| *ty == rust),
                "wire_schema: `{rust}` joined the CLONE CUT SET but has no entry in \
                 `ITERATIVE_TEARDOWN`. A panic unwinding out of the driver leaves cloned \
                 `{rust}`s on the pooled value stack, and releasing them with `Vec::clear` runs \
                 the DERIVED recursive destructor, which is itself Θ(depth) (gate subject \
                 `par_drop`). Add `(\"{rust}\", \"<path to an iterative teardown>\")` to \
                 `ITERATIVE_TEARDOWN` — and write the teardown — rather than letting the \
                 generated match fall through."
            );
        }

        let cut_set: BTreeSet<&str> = cut.iter().map(|s| s.as_str()).collect();

        // ── 3. `reaches_cut`, over the residual where cut members are SINKS ──
        let mut reaches_cut: BTreeSet<String> = BTreeSet::new();
        let mut changed = true;
        while changed {
            changed = false;
            for (i, name) in graph.names.iter().enumerate() {
                if reaches_cut.contains(name) {
                    continue;
                }
                let hit = adjacency[i].iter().any(|&t| {
                    let child = &graph.names[t];
                    cut_set.contains(child.as_str()) || reaches_cut.contains(child)
                });
                if hit {
                    reaches_cut.insert(name.clone());
                    changed = true;
                }
            }
        }

        let mut oneof_reaches_cut: BTreeSet<String> = BTreeSet::new();
        for oneof in oneofs {
            let hit = oneof.variants.iter().any(|v| match &v.message_leaf {
                Some(leaf) => cut_set.contains(leaf.as_str()) || reaches_cut.contains(leaf),
                None => false,
            });
            if hit {
                oneof_reaches_cut.insert(oneof.rust_ident.clone());
            }
        }

        // ── the ENTERED sets: what the driver can actually arrive at ──
        let fields_of: BTreeMap<&str, &[Field]> = resolved
            .iter()
            .map(|(i, fields)| (messages[*i].leaf_name(), fields.as_slice()))
            .collect();
        let oneof_by_field: BTreeMap<String, &Oneof> = oneofs
            .iter()
            .map(|o| (oneof_key(messages, o), o))
            .collect();

        let mut entered: BTreeSet<String> = cut.iter().cloned().collect();
        let mut oneof_entered: BTreeSet<String> = BTreeSet::new();
        let mut frontier: Vec<String> = cut.clone();
        while let Some(current) = frontier.pop() {
            let Some(fields) = fields_of.get(current.as_str()) else {
                continue;
            };
            for field in fields.iter() {
                match &field.shape {
                    Shape::Oneof => {
                        let key = format!("{current}::{}", field.rust_name);
                        let Some(oneof) = oneof_by_field.get(&key) else {
                            continue;
                        };
                        if !oneof_reaches_cut.contains(&oneof.rust_ident) {
                            continue;
                        }
                        if oneof_entered.insert(oneof.rust_ident.clone()) {
                            for variant in &oneof.variants {
                                if let Some(leaf) = &variant.message_leaf {
                                    if !cut_set.contains(leaf.as_str())
                                        && reaches_cut.contains(leaf)
                                        && entered.insert(leaf.clone())
                                    {
                                        frontier.push(leaf.clone());
                                    }
                                }
                            }
                        }
                    }
                    other => {
                        for child in shape_children(other) {
                            if !cut_set.contains(child.as_str())
                                && reaches_cut.contains(&child)
                                && entered.insert(child.clone())
                            {
                                frontier.push(child);
                            }
                        }
                    }
                }
            }
        }

        // ── the residual height, so "a constant" is a NUMBER ──
        let residual_height = residual_height(&graph.names, &adjacency, &cut_set);

        ClonePlan {
            message_is_copy,
            oneof_is_copy,
            cut,
            reaches_cut,
            oneof_reaches_cut,
            entered,
            oneof_entered,
            residual_height,
        }
    }

    /// Is `leaf` a cut-set member — i.e. does the driver SUSPEND at it?
    fn in_cut(&self, leaf: &str) -> bool {
        self.cut.iter().any(|c| c == leaf)
    }

    /// Every item whose `Clone` §7 emits: the non-`Copy` messages and oneofs, in
    /// descriptor order. This is the join point with `models/build.rs`'s strip.
    fn clone_items(&self, messages: &[Message<'_>], oneofs: &[Oneof], extern_set: &BTreeSet<&str>) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(messages.len() + oneofs.len());
        for msg in messages {
            if extern_set.contains(msg.leaf_name()) {
                continue;
            }
            if !self.message_is_copy[msg.leaf_name()] {
                out.push(rust_type_name(msg.leaf_name()));
            }
        }
        for oneof in oneofs {
            if !self.oneof_is_copy[&oneof.rust_ident] {
                out.push(oneof.rust_ident.clone());
            }
        }
        out
    }
}

/// prost's `can_field_derive_copy`, reproduced.
///
/// `prost-build-0.14.3/src/context.rs:194-233`: repeated ⇒ no; a message field
/// ⇒ no if it would make the owner recursive (`is_nested`) or is `.boxed()`
/// (this build sets no `boxed` paths), else the field message's own answer;
/// otherwise a `Copy` scalar.
fn field_is_copy(
    owner: &str,
    field: &FieldDescriptorProto,
    message_is_copy: &BTreeMap<String, bool>,
    reaches: &impl Fn(&str, &str) -> bool,
) -> bool {
    if field.label == Some(Label::Repeated as i32) {
        return false;
    }
    let ty = match field.r#type.and_then(|t| Type::try_from(t).ok()) {
        Some(t) => t,
        None => return false,
    };
    if ty != Type::Message {
        return scalar_is_copy(ty);
    }
    let leaf = type_leaf(field.type_name.as_deref().unwrap_or(""));
    // prost's `is_nested(field_type, owner)`: would this field make the owner
    // recursive? If so, no `Copy` — and this is also what cuts the fixed point's
    // cycles.
    if reaches(leaf, owner) {
        return false;
    }
    message_is_copy.get(leaf).copied().unwrap_or(false)
}

/// The leaf name of the message that declares `oneof`.
///
/// [`Oneof::module`] is the owner's module path (`var`, `expr`, `tagged_continuation`),
/// which is the owner's leaf name in snake case — so the owner is recovered by
/// matching that rather than by assuming the oneof's position in a flat list.
fn owner_of_oneof(messages: &[Message<'_>], oneof: &Oneof) -> String {
    messages
        .iter()
        .find(|m| m.oneof_module() == oneof.module)
        .map(|m| m.leaf_name().to_string())
        .unwrap_or_else(|| {
            panic!(
                "wire_schema: no message has oneof module `{}`; the oneof `{}` cannot be \
                 attributed to an owner, and Copy-ness / cut reachability are per-owner facts",
                oneof.module, oneof.rust_ident
            )
        })
}

/// `"<owner leaf>::<rust field name>"` — the key that identifies WHICH oneof a
/// `Shape::Oneof` field is, without relying on `rust_type_name` round-tripping
/// the field name back to the enum name.
fn oneof_key(messages: &[Message<'_>], oneof: &Oneof) -> String {
    format!(
        "{}::{}",
        owner_of_oneof(messages, oneof),
        rust_field_name(&oneof.proto_name)
    )
}

/// The height of the child relation once the cut set is removed — the constant
/// that bounds a generated field-wise `clone`'s native recursion.
///
/// The residual is acyclic (verified by [`ClonePlan::build`]'s loop), so the
/// longest path is well defined and computed by memoized descent over an
/// explicit stack. Iterative deliberately: this is the build script of a
/// campaign about recursive walks.
fn residual_height(names: &[String], adjacency: &[Vec<usize>], cut: &BTreeSet<&str>) -> usize {
    const UNKNOWN: usize = usize::MAX;
    let mut height = vec![UNKNOWN; names.len()];
    for root in 0..names.len() {
        if height[root] != UNKNOWN {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&mut (v, ref mut slot)) = work.last_mut() {
            // A cut member is a SINK in the residual: the driver suspends there.
            let edges: &[usize] = if cut.contains(names[v].as_str()) {
                &[]
            } else {
                &adjacency[v]
            };
            if *slot < edges.len() {
                let w = edges[*slot];
                *slot += 1;
                if height[w] == UNKNOWN && !work.iter().any(|(u, _)| *u == w) {
                    work.push((w, 0));
                }
                continue;
            }
            work.pop();
            let best = edges
                .iter()
                .map(|&w| {
                    if height[w] == UNKNOWN {
                        0
                    } else {
                        height[w] + 1
                    }
                })
                .max()
                .unwrap_or(0);
            height[v] = best;
        }
    }
    height.iter().copied().filter(|h| *h != UNKNOWN).max().unwrap_or(0)
}

// ===========================================================================
// §5  EMITTER A — the BINCODE table (`rhoapi_wire.rs`)
// ===========================================================================
//
// ⚠★ THE ORDER IS `identity`: the resolved vector, as `resolve_message`
// produced it, which is prost-build's struct order and therefore serde's.
//
// ⚠⚠ This file's bytes are a FIXED POINT. `models/tests/
// wire_schema_conformance.rs` and `models/tests/serializer_par_byte_goldens.rs`
// are gated on the encoding it drives, and the cold store already holds byte
// strings written by it. Any change here that is not accompanied by a
// consensus-visible migration is a fork.

/// Render one bincode field emission — the BODY of one arm in the generated
/// `wire_emit`. `self`, `out` and `i` are in scope; a descending arm `return`s a
/// `Descent`.
///
/// `index` is the field's position in the SERDE program, which is what
/// `Descent::resume` names.
fn bincode_emit(field: &Field, index: usize) -> String {
    let name = &field.rust_name;
    let resume = index + 1;
    match &field.shape {
        Shape::Map { .. } => format!(
            "{{ let m = &self.{name}; put_u64(out, m.len() as u64); \
             if !m.is_empty() {{ return Descent::Map {{ resume: {resume}, map: m }}; }} }}"
        ),
        Shape::RepeatedMessage { .. } => format!(
            "{{ let s = &self.{name}; let n = s.len(); put_u64(out, n as u64); \
             if n != 0 {{ return Descent::Seq {{ resume: {resume}, len: n, seq: s }}; }} }}"
        ),
        Shape::RepeatedString => format!("put_str_seq(out, &self.{name})"),
        Shape::RepeatedBytes => format!("put_bytes_seq(out, &self.{name})"),
        Shape::Message { .. } => format!(
            "match &self.{name} {{ \
             Some(v) => {{ put_bool(out, true); \
             return Descent::Node {{ resume: {resume}, node: v }}; }} \
             None => put_bool(out, false) }}"
        ),
        Shape::EmptyBytes => "put_empty_bytes(out)".to_string(),
        // An `Option<oneof>` is a 1-byte tag, then (when present) the oneof's
        // own u32 declaration-order index and payload. The index and any bounded
        // payload are written by the generated `WireOneof::wire_emit`, which is
        // monomorphic and inlines; only a MESSAGE payload suspends.
        Shape::Oneof => format!(
            "match &self.{name} {{ \
             Some(v) => {{ put_bool(out, true); \
             if let Some(node) = WireOneof::wire_emit(v, out) {{ \
             return Descent::Node {{ resume: {resume}, node }}; }} }} \
             None => put_bool(out, false) }}"
        ),
        Shape::Scalar(ty) => match ty {
            Type::Bool => format!("put_bool(out, self.{name})"),
            Type::String => format!("put_str(out, &self.{name})"),
            Type::Bytes => format!("put_bytes(out, &self.{name})"),
            Type::Int32 | Type::Sint32 | Type::Sfixed32 => format!("put_i32(out, self.{name})"),
            Type::Uint32 | Type::Fixed32 => format!("put_u32(out, self.{name})"),
            Type::Int64 | Type::Sint64 | Type::Sfixed64 => format!("put_i64(out, self.{name})"),
            Type::Uint64 | Type::Fixed64 => format!("put_u64(out, self.{name})"),
            other => panic!(
                "wire_schema: scalar {other:?} reached the bincode renderer with no primitive. \
                 `classify` refuses unclassifiable types, so this is unreachable unless the two \
                 have drifted apart."
            ),
        },
    }
}

/// Render a message's bincode emissions, with the tail-call patch applied.
///
/// ★ THE TAIL-CALL PATCH. A descending field that is the program's LAST needs no
/// resume point, and the generator is the only place that knows which one that
/// is. `resume: {n}` — where `n` is the field count — can only have been
/// produced by the final field, so the rewrite is exact.
///
/// Doing this at build time removes a virtual `wire_program().len()` call from
/// EVERY descent in the driver, which was the entire residual gap against the
/// derived encoder (see `wire.rs`'s `NO_RESUME`).
///
/// ⚠ It is applied HERE, per emitter, and not in [`resolve_message`]: "which
/// field is last" is a property of the ORDER, and the two emitters order the
/// same fields differently. A patch applied once, before the sort, would name
/// the wrong field for one of them.
fn bincode_program(msg_name: &str, fields: &[Field]) -> Vec<String> {
    assert!(
        fields.len() < u16::MAX as usize,
        "wire_schema: `{msg_name}` has {} fields; `Descent::resume` is a u16 and \
         `NO_RESUME` is its maximum.",
        fields.len()
    );
    let spent = format!("resume: {}", fields.len());
    fields
        .iter()
        .enumerate()
        .map(|(i, f)| bincode_emit(f, i).replace(&spent, "resume: NO_RESUME"))
        .collect()
}

fn emit_bincode_source(
    messages: &[Message<'_>],
    resolved: &[(usize, Vec<Field>)],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
) -> String {
    let mut src = String::with_capacity(96 * 1024);
    bincode_header(&mut src);

    for (msg, fields) in walk_messages(messages, resolved, extern_set) {
        let rust_path = msg.rust_path();
        let Some(fields) = fields else {
            writeln!(
                src,
                "// `{rust_path}` is EXTERN (models/build.rs `.extern_path`). Its wire program\n\
                 // is HAND-WRITTEN in `models/src/rust/rholang/wire.rs` because its `Serialize`\n\
                 // deliberately re-orders `ps` into canonical trie order for ground maps — a\n\
                 // behaviour no descriptor can express. The obligation is enforced below.\n"
            )
            .expect("write");
            continue;
        };
        emit_bincode_message(
            &mut src,
            &rust_path,
            &msg.program_ident(),
            msg.leaf_name(),
            fields,
        );
    }

    for oneof in oneofs {
        emit_bincode_oneof(&mut src, oneof);
    }

    emit_conformance_registry(&mut src, messages, extern_set);
    emit_extern_obligations(&mut src, extern_set);
    src
}

/// Pair every message with its resolved fields — or with `None` when it is
/// `extern_path`'d and has no descriptor-driven program — **in descriptor
/// order**.
///
/// ★ One place where "the resolved vector is parallel to the message vector" is
/// checked, rather than two emitters each keeping their own cursor. A mismatch
/// would attach a table to the wrong type, and it would do so silently.
///
/// ⚠ It returns the extern entries **in position** rather than handling them
/// itself. An earlier form took an `on_extern` callback and ran it during
/// collection, which emitted every extern comment at the TOP of the file instead
/// of where its type sits — a five-line diff against the byte-identity golden,
/// caught by that golden, and a reminder that "iterate and emit" is one loop and
/// not two.
fn walk_messages<'m, 'f>(
    messages: &'m [Message<'m>],
    resolved: &'f [(usize, Vec<Field>)],
    extern_set: &BTreeSet<&str>,
) -> Vec<(&'m Message<'m>, Option<&'f [Field]>)> {
    let mut out: Vec<(&Message<'_>, Option<&[Field]>)> = Vec::with_capacity(messages.len());
    let mut cursor = 0usize;
    for (i, msg) in messages.iter().enumerate() {
        if extern_set.contains(msg.leaf_name()) {
            out.push((msg, None));
            continue;
        }
        let (index, fields) = resolved
            .get(cursor)
            .expect("wire_schema: every non-extern message was resolved");
        assert_eq!(
            *index, i,
            "wire_schema: the resolved-field vector is out of step with the message vector; \
             a generated table would be attached to the wrong type."
        );
        out.push((msg, Some(fields.as_slice())));
        cursor += 1;
    }
    assert_eq!(
        cursor,
        resolved.len(),
        "wire_schema: resolved fields remained after every message was visited"
    );
    out
}

fn bincode_header(src: &mut String) {
    src.push_str(
        "// @generated by models/build/wire_schema.rs from the protobuf FileDescriptorSet.\n\
         // DO NOT EDIT. Regenerate by touching models/src/main/protobuf/RhoTypes.proto.\n\
         //\n\
         // ONE table, BOTH directions: `models::rust::rholang::wire_encode` (serializer)\n\
         // and `models::rust::rholang::par_codec` (deserializer) are driven by exactly the\n\
         // programs below. The indices are serde DECLARATION ORDER, never the proto tag.\n\
         \n\
         use crate::rhoapi::*;\n\
         use crate::rhoapi::connective::ConnectiveInstance;\n\
         use crate::rhoapi::expr::ExprInstance;\n\
         use crate::rhoapi::g_unforgeable::UnfInstance;\n\
         use crate::rhoapi::tagged_continuation::TaggedCont;\n\
         use crate::rhoapi::var::VarInstance;\n\
         use crate::rust::rhoapi_ext::EPathMap;\n\
         use crate::rust::rholang::wire::{\n\
         \x20   put_bool, put_bytes, put_bytes_seq, put_empty_bytes, put_i32, put_i64, put_str,\n\
         \x20   put_str_seq, put_u32, put_u64, Descent, FieldKind, VariantProgram, WireNode,\n\
         \x20   WireOneof, NO_RESUME,\n\
         };\n\
         \n",
    );
}

fn emit_bincode_message(
    src: &mut String,
    rust_ty: &str,
    program_ident: &str,
    leaf_name: &str,
    fields: &[Field],
) {
    writeln!(
        src,
        "/// The serde field program of [`{rust_ty}`], in DECLARATION ORDER.\n\
         pub static {program_ident}: &[FieldKind] = &["
    )
    .expect("write");
    for f in fields {
        writeln!(src, "    FieldKind::{}, // {}", f.kind, f.rust_name).expect("write");
    }
    src.push_str("];\n\n");

    // ★ The field NAMES, so the conformance probe can compare this table
    // against what serde's own derive emits — per (type, field) rather than
    // "byte 4711 differs". prost's field order is *not* the intuitive one
    // (plain fields first, then oneofs), and the probe is what turns that from
    // a belief into a checked fact for every type at once.
    let names_ident = program_ident.replace("_PROGRAM", "_FIELD_NAMES");
    writeln!(
        src,
        "/// The serde field NAMES of [`{rust_ty}`], in DECLARATION ORDER.\n\
         pub static {names_ident}: &[&str] = &["
    )
    .expect("write");
    for f in fields {
        // The probe sees the name serde emits, which is the field identifier
        // WITHOUT prost's `r#` keyword escape.
        writeln!(
            src,
            "    \"{}\",",
            f.rust_name.strip_prefix("r#").unwrap_or(&f.rust_name)
        )
        .expect("write");
    }
    src.push_str("];\n\n");

    writeln!(src, "impl WireNode for {rust_ty} {{").expect("write");
    writeln!(
        src,
        "    #[inline]\n    fn wire_program(&self) -> &'static [FieldKind] {{ {program_ident} }}"
    )
    .expect("write");
    // ★ ONE virtual call per node per suspension, not one per field. The body
    // is monomorphic, so every bounded field inlines exactly as serde's derive
    // does — see `wire.rs` §A2 for the measurement that forced this shape.
    src.push_str("    fn wire_emit(&self, from: usize, out: &mut Vec<u8>) -> Descent<'_> {\n");
    if fields.is_empty() {
        // An empty protobuf message (`WildcardMsg`, `GSysAuthToken`)
        // serializes as `serialize_struct(name, 0)` — ZERO bytes.
        src.push_str("        let _ = (from, out);\n        Descent::Done\n");
    } else {
        // ★ A FALLTHROUGH CHAIN, not `loop { match i { … } i += 1 }`.
        //
        // The `match` form re-enters a jump table for every field, so a `Par`
        // paid eleven indirect jumps per node. `if from < k` is a compare
        // against a value that does not change inside the body: the branches
        // are perfectly predicted, the `from == 0` entry — which every node
        // takes once — runs straight-line, and LLVM can fold the chain.
        // Descending arms `return`, so the chain exits naturally.
        let emits = bincode_program(leaf_name, fields);
        for (i, (f, emit)) in fields.iter().zip(&emits).enumerate() {
            writeln!(
                src,
                "        if from < {} {{ {} }} // {}",
                i + 1,
                emit,
                f.rust_name
            )
            .expect("write");
        }
        src.push_str("        Descent::Done\n");
    }
    src.push_str("    }\n}\n\n");
}

fn emit_bincode_oneof(src: &mut String, oneof: &Oneof) {
    let Oneof {
        proto_name,
        rust_ident,
        module,
        variants,
    } = oneof;
    let table_ident = format!("{}_VARIANTS", proto_name.to_uppercase());
    let count_ident = format!("{}_VARIANT_COUNT", proto_name.to_uppercase());

    writeln!(
        src,
        "// ---------------------------------------------------------------------------\n\
         // oneof `{proto_name}` → `crate::rhoapi::{module}::{rust_ident}`\n\
         // ---------------------------------------------------------------------------\n"
    )
    .expect("write");

    writeln!(
        src,
        "/// The serde variant indices of [`{rust_ident}`] — DECLARATION ORDER, not proto tags.\n\
         ///\n\
         /// ⚠ APPEND-ONLY. Inserting a member mid-list re-labels every byte string already\n\
         /// written to the cold store.\n\
         pub static {table_ident}: &[VariantProgram] = &["
    )
    .expect("write");
    for v in variants {
        writeln!(
            src,
            "    VariantProgram {{ serde_index: {}, name: \"{}\", payload: {} }},",
            v.index, v.rust_ident, v.program_expr
        )
        .expect("write");
    }
    src.push_str("];\n\n");

    writeln!(
        src,
        "/// ★ Derived from the table, never a literal: a new arm moves this by construction.\n\
         pub const {count_ident}: usize = {table_ident}.len();\n"
    )
    .expect("write");

    for v in variants {
        writeln!(src, "pub const {}: u32 = {};", v.const_name, v.index).expect("write");
    }
    src.push('\n');

    // ★ The exhaustive match: no wildcard arm, so a 37th variant fails to
    // compile until this table regenerates.
    writeln!(
        src,
        "impl WireOneof for {rust_ident} {{\n\
         \x20   /// ★ EXHAUSTIVE — no wildcard arm. A variant added to the `.proto` cannot\n\
         \x20   /// reach production untested: the table regenerates and this match with it.\n\
         \x20   ///\n\
         \x20   /// Writes the `u32` DECLARATION-ORDER index (never the proto tag) and any\n\
         \x20   /// bounded payload; returns `Some(node)` only for a message payload, which\n\
         \x20   /// is the one case the driver has to suspend at.\n\
         \x20   #[inline]\n\
         \x20   fn wire_emit(&self, out: &mut Vec<u8>) -> Option<&dyn WireNode> {{\n\
         \x20       match self {{"
    )
    .expect("write");
    for v in variants {
        writeln!(
            src,
            "            {rust_ident}::{}(v) => {},",
            v.rust_ident, v.payload_expr
        )
        .expect("write");
    }
    src.push_str("        }\n    }\n}\n\n");
}

/// Emit the registry the conformance probe iterates.
///
/// ★ This is what makes "every type is checked" true rather than hoped: the
/// list is generated from the descriptor in the same pass as the table, so a
/// new message joins it automatically. A hand-written list would omit exactly
/// the type nobody remembered — which is how a 37th variant goes untested
/// while every assertion passes.
fn emit_conformance_registry(
    src: &mut String,
    messages: &[Message<'_>],
    extern_set: &BTreeSet<&str>,
) {
    src.push_str(
        "// ---------------------------------------------------------------------------\n\
         // The CONFORMANCE REGISTRY — every generated type, for the probe\n\
         // ---------------------------------------------------------------------------\n\
         \n\
         /// One row per generated message: `(rust type name, field kinds, field names)`.\n\
         ///\n\
         /// `models/tests/wire_schema_conformance.rs` runs serde's OWN derived\n\
         /// `Serialize` against each row and requires the names and their order to\n\
         /// match. That is the gate that localizes a generator mistake to a\n\
         /// (type, field) instead of to a byte offset — and prost's field order is\n\
         /// **not** the intuitive one (all plain fields first, then all oneofs), so\n\
         /// the belief needs to be a checked fact.\n\
         pub static CONFORMANCE_REGISTRY: &[(&str, &[FieldKind], &[&str])] = &[\n",
    );
    for msg in messages {
        if extern_set.contains(msg.leaf_name()) {
            continue;
        }
        let program = msg.program_ident();
        let names = program.replace("_PROGRAM", "_FIELD_NAMES");
        writeln!(
            src,
            "    (\"{}\", {program}, {names}),",
            rust_type_name(msg.leaf_name())
        )
        .expect("write");
    }
    src.push_str("];\n\n");
}

fn emit_extern_obligations(src: &mut String, extern_set: &BTreeSet<&str>) {
    src.push_str(
        "// ---------------------------------------------------------------------------\n\
         // EXTERN obligations — the generator refuses to skip a type silently\n\
         // ---------------------------------------------------------------------------\n\
         //\n\
         // Each `extern_path`'d type below has NO descriptor-driven program. This\n\
         // makes the missing hand-written `impl WireNode` a COMPILE ERROR rather than\n\
         // a byte-level surprise at the first ground `EPathMap`.\n\
         #[allow(dead_code)]\n\
         fn wire_extern_obligations() {\n\
         \x20   fn assert_hand_written<T: WireNode + ?Sized>() {}\n",
    );
    for name in extern_set {
        let ty = rust_type_name(name);
        writeln!(src, "    assert_hand_written::<{ty}>();").expect("write");
    }
    src.push_str("}\n");
}

// ===========================================================================
// §6  EMITTER B — the PROST table (`rhoapi_prost_wire.rs`)
// ===========================================================================
//
// ⚠★ THE ORDER IS `sort_by_key(min_tag)`. Same vector, different key. See the
// module header for the two messages where it differs and why both matter.

/// The `ProstKind` variant naming this field's protobuf encoding.
///
/// ★ It names a prost ENCODING MODULE, not a re-implementation. Every bounded
/// field is written by `prost::encoding::<module>::{encode, encoded_len}` — the
/// same functions `prost-derive` emits calls to — so protobuf's layout stays
/// written in exactly one place and only the RECURSION is replaced. A generator
/// that restated varint or zigzag here would be a second opinion about a byte
/// format, which is the failure mode this whole module exists to prevent.
fn prost_kind(shape: &Shape) -> String {
    match shape {
        // ⚠ `locally_free` reaches the protobuf wire UNBLANKED. The serde-only
        // `serialize_as_empty_bytes` normalization has no protobuf counterpart:
        // `models/tests/wire_encode_differential.rs` pins that prost RETAINS it.
        Shape::EmptyBytes => "ProstKind::Bytes".to_string(),
        Shape::Scalar(ty) => format!("ProstKind::{}", prost_scalar_variant(*ty)),
        Shape::Message { .. } => "ProstKind::Message".to_string(),
        Shape::RepeatedMessage { .. } => "ProstKind::RepeatedMessage".to_string(),
        Shape::RepeatedString => "ProstKind::RepeatedString".to_string(),
        Shape::RepeatedBytes => "ProstKind::RepeatedBytes".to_string(),
        Shape::Map { key, .. } => {
            assert_eq!(
                *key,
                Type::String,
                "wire_schema: a map with a {key:?} key reached the prost table. The driver's \
                 map arm writes the key with `prost::encoding::string::encode`; another key \
                 type needs a deliberate widening, not a silent reinterpretation."
            );
            "ProstKind::MapStringMessage".to_string()
        }
        Shape::Oneof => "ProstKind::Oneof".to_string(),
    }
}

/// The `ProstKind` variant for one protobuf scalar type.
///
/// ⚠ `sint32`/`sint64` are ZIGZAG on the protobuf wire and plain `i32`/`i64` in
/// bincode — the bincode table folds them into `I32`/`I64` (serde never sees a
/// protobuf encoding), and this table must NOT. Five `sint32` fields and one
/// `sint64` are live in `RhoTypes.proto`, so the fold is not hypothetical.
fn prost_scalar_variant(ty: Type) -> &'static str {
    match ty {
        Type::Bool => "Bool",
        Type::String => "String",
        Type::Bytes => "Bytes",
        Type::Int32 => "Int32",
        Type::Sint32 => "Sint32",
        Type::Sfixed32 => "Sfixed32",
        Type::Uint32 => "Uint32",
        Type::Fixed32 => "Fixed32",
        Type::Int64 => "Int64",
        Type::Sint64 => "Sint64",
        Type::Sfixed64 => "Sfixed64",
        Type::Uint64 => "Uint64",
        Type::Fixed64 => "Fixed64",
        Type::Double => "Double",
        Type::Float => "Float",
        other => panic!(
            "wire_schema: protobuf type {other:?} has no `ProstKind`. Add one deliberately — \
             widening it to the nearest neighbour would change the protobuf bytes and fork \
             consensus."
        ),
    }
}

/// The `prost::encoding` module that writes one protobuf scalar type, and the
/// literal `prost-derive` compares against for skip-at-default.
///
/// ★ Both halves are read off `prost-derive`'s own expansion rather than
/// restated: the module comes from `Ty::module()` and the default literal from
/// `DefaultValue::typed()` (`prost-derive-0.14.3/src/field/scalar.rs`). The
/// generated code CALLS `prost::encoding::<module>::{encode, encoded_len}`; it
/// never reimplements varint, zigzag or length delimiting.
///
/// ⚠ `String` and `Bytes` compare against `""` and `b"" as &[u8]` — the exact
/// forms `prost-derive` emits — because `String != ""` and `Vec<u8> != b""`
/// resolve through `PartialEq<str>` / `PartialEq<[u8]>` impls that
/// `String::new() != String::default()` would not exercise identically.
fn prost_scalar_module_and_default(ty: Type) -> (&'static str, &'static str) {
    match ty {
        Type::Bool => ("bool", "false"),
        Type::String => ("string", "\"\""),
        Type::Bytes => ("bytes", "b\"\" as &[u8]"),
        Type::Int32 => ("int32", "0i32"),
        Type::Sint32 => ("sint32", "0i32"),
        Type::Sfixed32 => ("sfixed32", "0i32"),
        Type::Uint32 => ("uint32", "0u32"),
        Type::Fixed32 => ("fixed32", "0u32"),
        Type::Int64 => ("int64", "0i64"),
        Type::Sint64 => ("sint64", "0i64"),
        Type::Sfixed64 => ("sfixed64", "0i64"),
        Type::Uint64 => ("uint64", "0u64"),
        Type::Fixed64 => ("fixed64", "0u64"),
        Type::Float => ("float", "0f32"),
        Type::Double => ("double", "0f64"),
        other => panic!(
            "wire_schema: protobuf type {other:?} has no `prost::encoding` module. Add one \
             deliberately — the emitted code CALLS prost's encoders, so an unmapped type has \
             no bytes rather than the wrong ones, and this refusal is what keeps it that way."
        ),
    }
}

/// The prost program of one message: the same resolved fields, ASCENDING
/// MINIMUM TAG.
///
/// ⚠ It returns REFERENCES into the one resolved vector, so there is no second
/// classification and no second walk — only a second order.
fn prost_order(fields: &[Field]) -> Vec<&Field> {
    let mut sorted: Vec<&Field> = fields.iter().collect();
    // `sort_by_key` is stable, and so is prost's, so two fields sharing a
    // minimum tag would keep declaration order in both — but they cannot:
    // `prost-derive` bails on a duplicate tag (`src/lib.rs:94-101`) and so does
    // `emit_prost_message`.
    sorted.sort_by_key(|f| f.min_tag());
    sorted
}

fn emit_prost_source(
    messages: &[Message<'_>],
    resolved: &[(usize, Vec<Field>)],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
) -> String {
    let mut src = String::with_capacity(64 * 1024);
    src.push_str(
        "// @generated by models/build/wire_schema.rs from the protobuf FileDescriptorSet.\n\
         // DO NOT EDIT. Regenerate by touching models/src/main/protobuf/RhoTypes.proto.\n\
         //\n\
         // ⚠★ THE ORDER HERE IS ASCENDING MINIMUM TAG, and it is NOT the order in\n\
         // `rhoapi_wire.rs`. `prost-derive-0.14.3/src/lib.rs:87-92` sorts a message's\n\
         // fields by `field.tags().into_iter().min()` before building `encode_raw` and\n\
         // `encoded_len`, and places a ONEOF at the position of its LOWEST tag whatever\n\
         // variant is occupied. `Debug` is built from the UNSORTED list (`:85, :214`) —\n\
         // two orders inside one derive.\n\
         //\n\
         // Over the 57 generated messages exactly TWO differ from declaration order, and\n\
         // both are load-bearing: `Par` (tags 11 and 12 are declared before 8, 9, 10) and\n\
         // `TaggedContinuation` (the oneof holds tags 1-2, `guard` holds tag 3, and they\n\
         // are declared the other way round). `TaggedContinuation` is the message whose\n\
         // SERDE order already cost this campaign a 95-byte encoding with its halves\n\
         // exchanged — and here the correct order is the OPPOSITE of that fix.\n\
         \n\
         use prost::encoding;\n\
         \n\
         use crate::rhoapi::*;\n\
         use crate::rhoapi::connective::ConnectiveInstance;\n\
         use crate::rhoapi::expr::ExprInstance;\n\
         use crate::rhoapi::g_unforgeable::UnfInstance;\n\
         use crate::rhoapi::tagged_continuation::TaggedCont;\n\
         use crate::rhoapi::var::VarInstance;\n\
         use crate::rust::rhoapi_ext::EPathMap;\n\
         use crate::rust::rholang::prost_wire::{\n\
         \x20   ProstDescent, ProstField, ProstKind, ProstNode, ProstOneof, NO_RESUME,\n\
         };\n\
         \n",
    );

    for (msg, fields) in walk_messages(messages, resolved, extern_set) {
        let rust_path = msg.rust_path();
        let Some(fields) = fields else {
            writeln!(
                src,
                "// `{rust_path}` is EXTERN (models/build.rs `.extern_path`). It has NO\n\
                 // descriptor-driven prost program, and — unlike the bincode side — it does not\n\
                 // get a hand-written one either. `EPathMap::encode_raw` has THREE arms (a memcpy\n\
                 // of the interned canonical bytes, the ground field-8 `U(m)` form, and the\n\
                 // ordinary field walk) of which only the last is a field walk at all, and which\n\
                 // one fires depends on a shadow cell another thread may fill. It is therefore an\n\
                 // OPAQUE LEAF to the prost driver, at exact parity with what\n\
                 // `prost::encoding::message::encode` does at that position.\n"
            )
            .expect("write");
            continue;
        };
        emit_prost_message(
            &mut src,
            &rust_path,
            &msg.prost_program_ident(),
            msg.leaf_name(),
            fields,
        );
    }

    for oneof in oneofs {
        emit_prost_oneof(&mut src, oneof);
    }

    emit_prost_conformance_registry(&mut src, messages, extern_set);
    src
}

fn emit_prost_message(
    src: &mut String,
    rust_ty: &str,
    program_ident: &str,
    leaf_name: &str,
    fields: &[Field],
) {
    let sorted = prost_order(fields);

    // A duplicate tag would make the sorted order ambiguous and is a
    // `prost-derive` hard error; refuse here for the same reason.
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for f in &sorted {
        for &tag in &f.tags {
            assert!(
                seen.insert(tag),
                "wire_schema: `{leaf_name}` has two fields at proto tag {tag}. \
                 `prost-derive` refuses this outright (`src/lib.rs:94-101`) and the sorted \
                 order would be ambiguous."
            );
        }
    }

    writeln!(
        src,
        "/// The protobuf field program of [`{rust_ty}`], in ASCENDING MINIMUM TAG order.\n\
         ///\n\
         /// ⚠ NOT the order of `{}` — see this file's header.\n\
         pub static {program_ident}: &[ProstField] = &[",
        program_ident.replace("_PROST_PROGRAM", "_PROGRAM")
    )
    .expect("write");
    for f in &sorted {
        let note = if f.tags.len() == 1 {
            String::new()
        } else {
            format!(
                " (oneof over tags {}; each ARM writes its own)",
                f.tags
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        writeln!(
            src,
            "    ProstField {{ tag: {}, kind: {}, name: \"{}\" }}, // {}{}",
            f.min_tag(),
            prost_kind(&f.shape),
            f.rust_name.strip_prefix("r#").unwrap_or(&f.rust_name),
            f.rust_name,
            note
        )
        .expect("write");
    }
    src.push_str("];\n\n");

    // ── the two passes, generated from ONE field list by ONE renderer ──
    //
    // ⚠★ That is the whole guarantee that they agree about skip-at-default.
    // `prost-derive` applies `if #ident != #default` in both `encode` and
    // `encoded_len`; two hand-written bodies would be two places for that rule
    // to live, and a driver whose passes disagreed about one `bool` writes a
    // length prefix that does not match the bytes after it.
    let (len_arms, emit_arms) = prost_bodies(leaf_name, &sorted);

    writeln!(src, "impl ProstNode for {rust_ty} {{").expect("write");
    writeln!(
        src,
        "    #[inline]\n\
         \x20   fn prost_program(&self) -> &'static [ProstField] {{ {program_ident} }}"
    )
    .expect("write");

    // ⚠ Whether ANY field of this message writes bytes in place, as opposed to
    // only descending. `Par`, `Expr` and `Var` are all-descent, so a
    // `let mut n` / a used `out` would be dead in exactly those bodies — and the
    // workspace builds with `-D warnings`, so "harmless dead binding" is a build
    // failure. The predicate is the SAME for both passes because it is the same
    // question, which is why it is computed once here.
    let writes_bounded = sorted.iter().any(|f| {
        matches!(
            f.shape,
            Shape::Scalar(_)
                | Shape::EmptyBytes
                | Shape::RepeatedString
                | Shape::RepeatedBytes
                | Shape::Oneof
        )
    });

    src.push_str("    fn prost_len_step(&self, from: usize) -> (u64, ProstDescent<'_>) {\n");
    if sorted.is_empty() {
        // An empty protobuf message (`WildcardMsg`, `GSysAuthToken`) encodes to
        // ZERO bytes — no key, no length, nothing.
        src.push_str("        let _ = from;\n        (0, ProstDescent::Done)\n");
    } else {
        if writes_bounded {
            src.push_str("        let mut n = 0u64;\n");
        } else {
            src.push_str("        // Every field of this message DESCENDS; nothing is written\n\
                          \x20       // in place, so the whole of its length comes from its\n\
                          \x20       // children's `Op::Close` contributions.\n\
                          \x20       let n = 0u64;\n");
        }
        for (i, (f, arm)) in sorted.iter().zip(&len_arms).enumerate() {
            writeln!(src, "        if from < {} {{ {} }} // {}", i + 1, arm, f.rust_name)
                .expect("write");
        }
        src.push_str("        (n, ProstDescent::Done)\n");
    }
    src.push_str("    }\n\n");

    src.push_str("    fn prost_emit(&self, from: usize, out: &mut Vec<u8>) -> ProstDescent<'_> {\n");
    if sorted.is_empty() {
        src.push_str("        let _ = (from, out);\n        ProstDescent::Done\n");
    } else {
        if !writes_bounded {
            src.push_str("        // Every field DESCENDS; the driver writes each child's key\n\
                          \x20       // and length prefix, so this body emits nothing itself.\n\
                          \x20       let _ = out;\n");
        }
        for (i, (f, arm)) in sorted.iter().zip(&emit_arms).enumerate() {
            writeln!(src, "        if from < {} {{ {} }} // {}", i + 1, arm, f.rust_name)
                .expect("write");
        }
        src.push_str("        ProstDescent::Done\n");
    }
    src.push_str("    }\n}\n\n");
}

/// Render a message's two protobuf pass bodies, with the tail-call patch
/// applied — `(len arms, emit arms)`, parallel to `sorted`.
///
/// ★ ONE renderer, TWO outputs. Every rule that must hold in both passes —
/// skip-at-default above all — is written once here, so the passes cannot
/// disagree. `prost-derive` has the same property for the same reason
/// (`src/lib.rs:103-109` maps one sorted field list through `field.encode` and
/// `field.encoded_len`).
///
/// The tail-call patch is applied to the SORTED length, which is a different
/// field from the bincode table's last: see [`bincode_program`].
fn prost_bodies(msg_name: &str, sorted: &[&Field]) -> (Vec<String>, Vec<String>) {
    assert!(
        sorted.len() < u16::MAX as usize,
        "wire_schema: `{msg_name}` has {} fields; `ProstDescent::resume` is a u16 and \
         `NO_RESUME` is its maximum.",
        sorted.len()
    );
    let spent = format!("resume: {}", sorted.len());
    let mut lens = Vec::with_capacity(sorted.len());
    let mut emits = Vec::with_capacity(sorted.len());
    for (i, f) in sorted.iter().enumerate() {
        let (len, emit) = prost_arms(f, i);
        lens.push(len.replace(&spent, "resume: NO_RESUME"));
        emits.push(emit.replace(&spent, "resume: NO_RESUME"));
    }
    (lens, emits)
}

/// The `(measure, emit)` pair for ONE protobuf field at position `index` of the
/// sorted program.
///
/// `n` (a `u64` accumulator) and `out` are in scope in the respective bodies; a
/// descending arm `return`s.
fn prost_arms(field: &Field, index: usize) -> (String, String) {
    let name = &field.rust_name;
    let resume = index + 1;
    let tag = field.min_tag();
    match &field.shape {
        // ⚠ `locally_free` is ORDINARY BYTES here. The eight-zero-bytes rule is
        // serde-only; prost retains the field.
        Shape::EmptyBytes => prost_scalar_arms(name, Type::Bytes, tag),
        Shape::Scalar(ty) => prost_scalar_arms(name, *ty, tag),
        Shape::RepeatedString => (
            format!("n += encoding::string::encoded_len_repeated({tag}u32, &self.{name}) as u64;"),
            format!("encoding::string::encode_repeated({tag}u32, &self.{name}, out);"),
        ),
        Shape::RepeatedBytes => (
            format!("n += encoding::bytes::encoded_len_repeated({tag}u32, &self.{name}) as u64;"),
            format!("encoding::bytes::encode_repeated({tag}u32, &self.{name}, out);"),
        ),
        // A singular message is OMITTED entirely when `None`
        // (`prost-derive-0.14.3/src/field/message.rs`, the `Optional` arm), so
        // there is no tag and no zero-length body to write.
        Shape::Message { .. } => {
            let descent =
                format!("ProstDescent::Node {{ resume: {resume}, tag: {tag}u32, node: v }}");
            (
                format!("if let Some(v) = &self.{name} {{ return (n, {descent}); }}"),
                format!("if let Some(v) = &self.{name} {{ return {descent}; }}"),
            )
        }
        // Each element carries its OWN key and length prefix, all under `tag`.
        Shape::RepeatedMessage { .. } => {
            let descent = format!(
                "ProstDescent::Seq {{ resume: {resume}, tag: {tag}u32, len: k, seq: s }}"
            );
            (
                format!(
                    "{{ let s = &self.{name}; let k = s.len(); if k != 0 {{ return (n, {descent}); }} }}"
                ),
                format!(
                    "{{ let s = &self.{name}; let k = s.len(); if k != 0 {{ return {descent}; }} }}"
                ),
            )
        }
        Shape::Map { .. } => {
            let descent =
                format!("ProstDescent::Map {{ resume: {resume}, tag: {tag}u32, map: m }}");
            (
                format!(
                    "{{ let m = &self.{name}; if !m.is_empty() {{ return (n, {descent}); }} }}"
                ),
                format!("{{ let m = &self.{name}; if !m.is_empty() {{ return {descent}; }} }}"),
            )
        }
        // ⚠★ The ARM supplies its own tag — prost places the oneof FIELD at its
        // lowest tag but each variant encodes under the tag it declares
        // (`prost-derive-0.14.3/src/lib.rs:462-471`). `{tag}` above is the SORT
        // KEY and is deliberately not used here.
        Shape::Oneof => (
            format!(
                "if let Some(v) = &self.{name} {{ \
                 let (b, child) = ProstOneof::prost_len_step(v); n += b; \
                 if let Some((t, node)) = child {{ \
                 return (n, ProstDescent::Node {{ resume: {resume}, tag: t, node }}); }} }}"
            ),
            format!(
                "if let Some(v) = &self.{name} {{ \
                 if let Some((t, node)) = ProstOneof::prost_emit(v, out) {{ \
                 return ProstDescent::Node {{ resume: {resume}, tag: t, node }}; }} }}"
            ),
        ),
    }
}

/// The `(measure, emit)` pair for one singular protobuf scalar.
///
/// ⚠★ **Skip-at-default, in both, from one place.** `prost-derive`'s
/// `Kind::Plain` arm wraps both the encode and the length in
/// `if #ident != #default` (`src/field/scalar.rs:116-125, 172-189`). The guard
/// is rendered here once and interpolated into both bodies, so no edit can
/// apply it to one pass and not the other.
fn prost_scalar_arms(name: &str, ty: Type, tag: u32) -> (String, String) {
    let (module, default) = prost_scalar_module_and_default(ty);
    let guard = format!("if self.{name} != {default}");
    (
        format!("{guard} {{ n += encoding::{module}::encoded_len({tag}u32, &self.{name}) as u64; }}"),
        format!("{guard} {{ encoding::{module}::encode({tag}u32, &self.{name}, out); }}"),
    )
}

fn emit_prost_oneof(src: &mut String, oneof: &Oneof) {
    let Oneof {
        proto_name,
        rust_ident,
        module,
        variants,
    } = oneof;
    let table_ident = format!("{}_PROST_VARIANTS", proto_name.to_uppercase());

    writeln!(
        src,
        "// oneof `{proto_name}` → `crate::rhoapi::{module}::{rust_ident}`\n\
         //\n\
         // ⚠ The ARM writes its own tag. prost places the oneof FIELD at the position of\n\
         // its lowest tag but each variant encodes under the tag it actually declares —\n\
         // `prost-derive-0.14.3/src/lib.rs:462-471`. The two are different numbers for\n\
         // every arm but the first.\n\
         /// Each arm of [`{rust_ident}`], carrying the tag the ARM writes.\n\
         pub static {table_ident}: &[ProstField] = &["
    )
    .expect("write");
    for v in variants {
        let kind = match &v.message_leaf {
            Some(_) => "ProstKind::Message".to_string(),
            None => format!("ProstKind::{}", prost_scalar_variant(v.ty)),
        };
        writeln!(
            src,
            "    ProstField {{ tag: {}, kind: {}, name: \"{}\" }}, // serde index {}",
            v.tag, kind, v.rust_ident, v.index
        )
        .expect("write");
    }
    src.push_str("];\n\n");

    // ── the exhaustive impl: no wildcard arm, so a new variant fails to compile ──
    writeln!(
        src,
        "impl ProstOneof for {rust_ident} {{\n\
         \x20   /// ★ EXHAUSTIVE — no wildcard arm. A variant added to the `.proto` cannot\n\
         \x20   /// reach production untested: the table regenerates and this match with it.\n\
         \x20   ///\n\
         \x20   /// ⚠★ NO SKIP-AT-DEFAULT. `prost-derive`'s `scalar::Field::new_oneof`\n\
         \x20   /// rewrites `Kind::Plain` into `Kind::Required` (`src/field/scalar.rs:92-106`)\n\
         \x20   /// and the `Required` arm carries no `if #ident != #default` guard, because a\n\
         \x20   /// set-but-default oneof member must stay distinguishable from an absent one.\n\
         \x20   /// Inheriting the plain-field rule here would erase `GBool(false)`, `GInt(0)`\n\
         \x20   /// and `GString(\"\")` from the wire.\n\
         \x20   #[inline]\n\
         \x20   fn prost_len_step(&self) -> (u64, Option<(u32, &dyn ProstNode)>) {{\n\
         \x20       match self {{"
    )
    .expect("write");
    for v in variants {
        let arm = match &v.message_leaf {
            // The DRIVER writes a message arm's key and length prefix; it is the
            // only place that knows the child's length.
            Some(_) => format!("(0, Some(({}u32, v)))", v.tag),
            None => {
                let (module, _) = prost_scalar_module_and_default(v.ty);
                format!(
                    "(encoding::{module}::encoded_len({}u32, v) as u64, None)",
                    v.tag
                )
            }
        };
        writeln!(src, "            {rust_ident}::{}(v) => {arm},", v.rust_ident).expect("write");
    }
    src.push_str("        }\n    }\n\n");

    // ⚠ A oneof whose every arm is a MESSAGE writes nothing itself — the driver
    // writes each arm's key and length prefix. `UnfInstance` is exactly that, so
    // an unconditional `out` binding would be dead in its body, and the
    // workspace builds with `-D warnings`.
    let has_scalar_arm = variants.iter().any(|v| v.message_leaf.is_none());
    writeln!(
        src,
        "\x20   /// Writes the bounded arm and reports a message arm. See\n\
         \x20   /// [`Self::prost_len_step`] for why no arm is skipped at its default.\n\
         \x20   #[inline]\n\
         \x20   fn prost_emit(&self, out: &mut Vec<u8>) -> Option<(u32, &dyn ProstNode)> {{{}\n\
         \x20       match self {{",
        if has_scalar_arm {
            String::new()
        } else {
            "\n        // Every arm of this oneof is a MESSAGE, so the driver writes\n\
             \x20       // each one's key and length prefix and this body emits nothing.\n\
             \x20       let _ = out;"
                .to_string()
        }
    )
    .expect("write");
    for v in variants {
        let arm = match &v.message_leaf {
            Some(_) => format!("Some(({}u32, v))", v.tag),
            None => {
                let (module, _) = prost_scalar_module_and_default(v.ty);
                format!("{{ encoding::{module}::encode({}u32, v, out); None }}", v.tag)
            }
        };
        writeln!(src, "            {rust_ident}::{}(v) => {arm},", v.rust_ident).expect("write");
    }
    src.push_str("        }\n    }\n}\n\n");
}

fn emit_prost_conformance_registry(
    src: &mut String,
    messages: &[Message<'_>],
    extern_set: &BTreeSet<&str>,
) {
    src.push_str(
        "/// One row per generated message: `(rust type name, protobuf field program)`.\n\
         ///\n\
         /// ★ Generated in the same pass as the bincode table, so a new message joins the\n\
         /// probe automatically. A hand-written list would omit exactly the type nobody\n\
         /// remembered.\n\
         pub static PROST_CONFORMANCE_REGISTRY: &[(&str, &[ProstField])] = &[\n",
    );
    for msg in messages {
        if extern_set.contains(msg.leaf_name()) {
            continue;
        }
        writeln!(
            src,
            "    (\"{}\", {}),",
            rust_type_name(msg.leaf_name()),
            msg.prost_program_ident()
        )
        .expect("write");
    }
    src.push_str("];\n");
}

// ===========================================================================
// §7  EMITTER C — the TERM-OP slot (`rhoapi_term_ops.rs`)
// ===========================================================================

/// How the driver treats one field.
///
/// ★ Derived, never named: the three arms are a function of [`ClonePlan`]'s cut
/// set and `reaches_cut` predicate, so a `.proto` change that puts a `Par` under
/// a type that used to be a leaf reclassifies that field automatically.
enum FieldDescent {
    /// Cloned whole, in one call, because that call is FLAT — either the type
    /// contains no cut-set member at all, or (for `EPathMap`) its hand-written
    /// `Clone` is O(1) at the node.
    Bounded,
    /// **Suspend here.** The field's type is a cut-set member; the driver pushes
    /// a `Descend` and the parent's `Combine` takes the finished value back.
    /// Carries the member's Rust type name.
    Cut(String),
    /// **Enter here.** The field's type can reach the cut set, so the driver
    /// walks its shell inline. Carries the entered item's function stem.
    Enter(String),
}

/// How many values one field contributes.
enum FieldArity {
    /// `Option<T>` — a singular message, or the oneof field.
    Optional,
    /// `Vec<T>`.
    Repeated,
    /// `BTreeMap<K, T>` — the driver descends into the VALUES.
    Map,
}

/// Classify one field for the clone driver: `(arity, descent)`.
fn clone_field_plan(
    owner_leaf: &str,
    field: &Field,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
) -> (FieldArity, FieldDescent) {
    match &field.shape {
        Shape::Scalar(_) | Shape::EmptyBytes | Shape::RepeatedString | Shape::RepeatedBytes => {
            (FieldArity::Repeated, FieldDescent::Bounded)
        }
        Shape::Message { leaf } => (FieldArity::Optional, message_descent(leaf, plan)),
        Shape::RepeatedMessage { leaf } => (FieldArity::Repeated, message_descent(leaf, plan)),
        Shape::Map { value_leaf, .. } => (FieldArity::Map, message_descent(value_leaf, plan)),
        Shape::Oneof => {
            let key = format!("{owner_leaf}::{}", field.rust_name);
            let oneof = oneof_by_field.get(&key).unwrap_or_else(|| {
                panic!(
                    "wire_schema: `{owner_leaf}.{}` is a oneof field but no resolved oneof \
                     answers to `{key}`. The clone emitter names a oneof's generated walk after \
                     its OWNER and field, so an unattributable oneof would silently become a \
                     bounded field — i.e. a whole-value clone of a type that contains `Par`s, \
                     which is exactly the Θ(depth) delegation this stage exists to remove.",
                    field.rust_name
                )
            });
            let descent = if plan.oneof_reaches_cut.contains(&oneof.rust_ident) {
                FieldDescent::Enter(oneof.rust_ident.to_snake_case())
            } else {
                FieldDescent::Bounded
            };
            (FieldArity::Optional, descent)
        }
    }
}

/// The descent for a message-typed field of leaf type `leaf`.
fn message_descent(leaf: &str, plan: &ClonePlan) -> FieldDescent {
    if plan.in_cut(leaf) {
        FieldDescent::Cut(rust_type_name(leaf))
    } else if plan.reaches_cut.contains(leaf) {
        FieldDescent::Enter(rust_type_name(leaf).to_snake_case())
    } else {
        FieldDescent::Bounded
    }
}

/// Emit the term-op file: the GENERATED `Clone` for the whole `rhoapi` surface.
///
/// Returns `(source, the item names whose `Clone` was emitted)`. The second value
/// is the join point with `models/build.rs`'s textual derive-strip pass; see
/// there for the set cross-check in both directions.
///
/// ## What this emits, and why each piece is generated rather than written
///
/// ```text
///   §A  the alphabet     CloneNode / CloneVal / CloneKont / CloneChildren
///   §B  the pooled stacks   thread-local, so a shallow clone allocates NOTHING
///   §C  the traversal    one `impl Traversal`, dispatching on the CUT SET
///   §D  the families     push-children / count-children / rebuild, per ENTERED type
///   §E  the impls        `impl Clone` × the non-`Copy` items
///   §F  the ORACLE       the derive's own Θ(depth) body, retained as a free fn
///   §G  the join tables  what `models/build.rs` and the tests check against
/// ```
///
/// ## ⚠★ Why the emission is MONOMORPHIC and the trampoline is not
///
/// [`crate::rust::rholang::wire`] §A2 records the obvious factoring — a generated
/// table exposing `fn wire_field(i) -> FieldVal` interpreted by a hand-written
/// driver — and what it cost: 21 indirect calls per `Par` node, 31.8% of the
/// profile in the driver loop, **1.7× slower than the derive it replaced**. The
/// rule that came out of it is *"bounded-field EMISSION is generated and
/// monomorphic; the TRAMPOLINE is hand-written and generic"*, and every family in
/// §D obeys it: one straight-line function per type, no per-field indirection, no
/// `&dyn` anything.
///
/// ## ⚠★★ THROUGHPUT — and the five figures this section used to LEAD with are
/// ## RETRACTED, so they are quoted here as retracted rather than deleted
///
/// `models/benches/term_ops_bench.rs` measures this emission against the retained
/// derive oracle on the production-weighted mix (2,001 datums, **12,286 `Par`
/// nodes**, 95.43% at depth 2, nothing deeper than 6).
///
/// ⚠ The paragraph that stood here said: *"the driven form is **0.674×** the
/// derived form's throughput — a 48% slowdown — against a stated acceptance
/// threshold of 0.98×, **Welch t = −307**, intervals disjoint at α = 0.01. Per
/// depth: **0.648×** at 1, **0.665×** at 2."* **Not one of those numbers is a
/// measurement**, and the two paragraphs below are the two independent reasons —
/// the instrument was blocked, *and* the instrument's own output contained the
/// refutation of the replication claim it was being used to support.
///
/// ## ⚠⚠ RETRACTED 2026-07-30 (i) — THE INSTRUMENT WAS BLOCKED
///
/// `models/benches/term_ops_bench.rs`'s `measure()` ran **every** repetition of one
/// arm and was then called again for the other, while its own module header
/// claimed per-repetition interleaving. The arms were timed in different windows on
/// a host that runs six concurrent build jobs. Same binary, minutes apart:
/// **1.0748× (PASS) then 0.9461× (FAIL)**. `wire_encode_bench` carried the
/// identical defect behind the identical sentence and scattered **27%** over three
/// consecutive runs.
///
/// ⇒ 0.674×, 0.648× and 0.665× are **not measurements**. Paired — with genuine
/// per-repetition interleaving, order rotation and a paired t, in the shared
/// `models/benches/paired.rs` — the production-weighted mix reads
/// **0.948×–0.962×**: a **5% deficit, not 32%.**
///
/// ★ The durable part is not the corrected number, it is why the old one was
/// wrong. A ratio from unpaired arms on a loaded host is a draw from a
/// distribution 13–27% wide, and quoting four significant figures of it states a
/// precision that does not exist.
///
/// ## ⚠⚠★★★ RETRACTED 2026-07-30 (ii) — THE TWO `t` VALUES REFUTE THE
/// ## REPLICATION CLAIM ARITHMETICALLY, AND THEY WERE READ AS AGREEMENT
///
/// `b228545f` wrote **0.674× / `t` = −307** into *this file* and **0.678× /
/// `t` = −65** into *its own commit message*, in the same act, and described them
/// as replicates: *"Reproduced across three runs (0.674x, 0.678x)."*
///
/// Inverting Welch at the fixed `n = 60` for the pooled relative standard
/// deviation each pair implies:
///
/// | citation | ratio | `` $\lvert t \rvert$ `` | implied `` $s$ `` (equal-rel) | (equal-abs) |
/// |---|---|---|---|---|
/// | this file | 0.674 | 307 | **0.68%** | **0.86%** |
/// | register row 36 | 0.678 | 65 | **3.18%** | **4.00%** |
/// | | | **ratio** | **4.66×** | **4.64×** |
///
/// `b228545f` itself states *"a 0.5–0.9% standard deviation"*. The **−307** run
/// sits inside that band; the **−65** run is **3.5–4.4× above** it — at the stated
/// band it should have produced `` $\lvert t \rvert \approx 229\text{–}413$ ``.
///
/// ⇒ ★★ **The instrument's own output contained the refutation of the claim it was
/// supporting, and it was read as agreement.** Two readings 4.7× apart in implied
/// variance were called replicates because their *ratios* agreed to 0.6%.
/// **Agreement in the point estimate is not agreement in the measurement**, and
/// that is the lesson to carry to the next figure written into this file — which is
/// why it is written *at* the site rather than in an audit.
///
/// ⚠ Neither figure was **transcribed**: both were *derived*, from two different
/// runs, and `docs/consensus/consensus-change-register.md` faithfully copied the
/// commit message. So this is not a transcription drift — the defect is upstream of
/// any copying, in calling two runs one experiment.
///
/// ⚠ And keep the three statistics apart: `t = −307` and `t = −65` are both
/// **weighted-mix** figures from `b228545f`; the direct comparator for the later
/// `t = 219.55` is **−103**, a **depth-2** figure from a re-run. Three statistics,
/// two subjects. The register correction belongs to
/// `docs/consensus/consensus-change-register.md`, which this file cannot edit; it
/// has been reported for filing.
///
/// ### ★★ THE ACCEPTANCE CRITERION, RESTATED
///
/// The 0.98× threshold demanded 2% of an instrument that scatters by 13–27%. It is
/// replaced, in rank order:
///
/// | rank | instrument | criterion |
/// |---|---|---|
/// | **primary** | `TERM_OPS_ARM` under `valgrind --tool=cachegrind --cache-sim=yes`, `fixture`-subtracted, per `Par` node | `Ir(driven) / Ir(derived)` ≤ **1.20** (measured **1.1079** at `CLONE_DESCEND_BUDGET = 3`) |
/// | corroboration | the bench's paired median-of-repetition ratio | ≥ **0.90×**, a band the host can resolve |
/// | corroboration | `perf stat -e instructions,cycles`, normalised on the **derived** arm | agrees with the primary to within 0.5% |
///
/// ⚠ The wall-clock floor is deliberately **looser** than 0.98×. That is not a
/// relaxation of standards; it is the refusal to state a precision the instrument
/// does not have.
///
/// ★ And the restated criterion is **falsifiable and has been seen to RESPOND**,
/// which the old one had not — twice now, and the second time it responded while
/// the clock could not resolve the change at all:
///
/// | change | primary `Ir` ratio | paired wall clock |
/// |---|---|---|
/// | pre-form-B | 1.1740 | — |
/// | form-B (walk elimination + leaf fast path) | **1.1519** | 0.954× → 0.885× (the WRONG way) |
/// | re-measured at HEAD before the budget | **1.1510** | — |
/// | `CLONE_DESCEND_BUDGET = 3` | **1.1079** | see below |
///
/// ⚠ The 1.1519 / 1.1510 pair is worth noting: the same mechanism, re-measured on
/// a later toolchain and a later tree, moved **0.08%**. That is the reproducibility
/// of the primary instrument across a rebuild, and it is the reason the ceiling has
/// 2.2% of headroom rather than 0.2%.
///
/// ### The optimization that was tried, and REFUTED
///
/// `perf` put **41.77%** of `drive_with`'s samples on one instruction —
/// `cmp $0x24, %eax`, the 36-arm `ExprInstance` jump-table bounds check — and
/// 23.61% on a single stack spill. The obvious reading is that the *three*
/// structural walks per node (`clone_push_children_*`, `clone_child_count_*`,
/// `clone_rebuild_*`) cost three dispatches where the derive costs one, so the
/// count walk was removed: `descend` recorded `base = vals.len()` in the `Kont`,
/// the recount became a `debug_assertions`-only cross-check, and a LEAF FAST PATH
/// skipped the `Combine` round-trip for the ~4-of-6 `Par` nodes in a depth-2 datum
/// that have no cut-set child.
///
/// **It measured 0.650× — WORSE than the 0.674× it was meant to improve** (both
/// arms re-measured in the same run; the derived arm moved +0.8% while the driven
/// arm moved −2.8%, against a 0.5–0.9% standard deviation). The experiment is not
/// in the tree.
///
/// ⇒ The 41.77% was a **mis-read**: without a precise (PEBS) event, samples land on
/// the instruction after the retiring one, and `cmp $0x24` immediately precedes an
/// indirect jump. The cost is the branch MISPREDICTION at the jump table, which
/// removing one of three dispatch *sites* does not remove — while the wider `Kont`
/// (8 B → 16 B, so `Step` 16 B → 24 B) and the extra leaf branch cost more than the
/// walk saved.
///
/// ### What the gap actually is, quantified
///
/// ```text
///   measured gap, weighted pass        0.713 ms
///   `Par` nodes per pass              12,286   (~6 per depth-2 datum; 2,001 datums)
///   size_of::<Par>()                     248 B (11 fields, `#[repr(C)]`)
///   extra 248-B moves per node             3   stack slot -> CloneVal -> vals
///                                              -> drain -> parent's slot
///   implied bandwidth              12.53 GB/s  <-- plausible L1/L2 memcpy
///   gap per node                       59.4 ns
/// ```
///
/// A post-order fold whose `Val` is the OWNED node moves each node's 248 bytes
/// three extra times; the derive constructs it once, directly into the parent's
/// `Vec` slot via `SpecFromIterNested`.
///
/// ## ★★ MEASURED 2026-07-29 — the moves are CONFIRMED, and every instrument
/// ## named above was wrong about how to see them
///
/// ⚠ The paragraph that used to stand here called a **PEBS-precise `mem-stores`
/// profile** "the decisive experiment". **PEBS is an Intel mechanism and does not
/// exist on this ISA** — this host is a Threadripper PRO 5975WX (Zen 3, family
/// 0x19 model 0x8), and `perf` answers `mem-stores` with *"Unable to find event on
/// a PMU"*. What actually works, characterised rather than assumed (at
/// `kernel.perf_event_paranoid = 0`):
///
/// ```text
///   valgrind --tool=cachegrind --cache-sim=yes    works, DETERMINISTIC, no skid
///   perf stat -e ls_dispatch.store_dispatch       works  <-- the AMD store counter
///   perf stat -e cycles / cycles:P                both work
///   perf record --call-graph dwarf -e cycles:P    works
///   perf stat -e mem-stores                       DOES NOT EXIST (Intel PEBS)
///   perf ... -e ibs_op/swfilt=1/ (per-thread)     REFUSES: "enable system wide with '-a'"
///   perf record --call-graph lbr                  REFUSES on this part
/// ```
///
/// `models/benches/term_ops_bench.rs`'s `TERM_OPS_ARM` mode runs one arm over the
/// weighted mix and exits, with a `fixture` arm that does everything except the
/// clone, so `arm − fixture` isolates the mechanism. Per `Par` node, over 245,720
/// nodes:
///
/// ```text
///                 Ir        Dr        Dw    D1 miss   LLd miss
///   derived    2456.1     662.8     555.6      33.92    0.7521
///   driven     2883.4     835.4     698.4      33.88    0.7548
///   ratio      1.1740    1.2604    1.2571     0.9988    1.0035
/// ```
///
/// ★ **Cache behaviour is at PARITY** — the driven arm misses very slightly less.
/// The gap is +427 instructions and +143 write references per node: **retired
/// work, not stalls.** And `cg_annotate` attributes it:
///
/// ```text
///   ΔDw/node   ΔIr/node   function
///     +93.6      +365.8   drive::drive_with::<CloneTraversal>
///     +68.3      +179.8   Map<Iter<Expr>, clone_rebuild_par::{closure#3}>::fold
///     −37.0      −112.0   term_ops::oracle_clone_par        (absent in driven)
///     −30.7      −160.9   __memcpy_avx_unaligned_erms       (driven does LESS)
/// ```
///
/// ★★ `drive_with` **itself** — the trampoline, not the visitor — costs **+93.6
/// write references per node**, and `93.6 × 8 B = 749 B` against
/// `3 × size_of::<Par>() = 744 B`: **agreement to 0.7%.** The three moves are
/// exactly where this comment said they were. The prediction of ~288,000 extra
/// `Dw` per pass came in **6.09× low** for one reason: it priced the moves at
/// 32-byte vector stores, and `objdump` on `drive_with` finds 280 `mov` and 30
/// `movq` and **zero** `vmov*`/`movdq*` — the compiler emits **scalar 8-byte
/// stores.**
///
/// Hardware corroborates and cross-validates: `ls_dispatch.store_dispatch` gives
/// 606.7 → 902.5 store µops per node (**1.487×**), and `instructions` gives
/// **1.1742×** against cachegrind's **1.1740×** — two instruments, four
/// significant figures. IPC is *higher* on the driven arm (1.278 vs 1.148), so
/// `1.1742 / (1.278/1.148) = 1.055` = the measured cycles ratio. **The gap is an
/// instruction-count gap with no stall term.**
///
/// ⚠ And the gap is footprint-dependent, which is why the two ratios in this file
/// disagree: 1.055× cycles on the **weighted mix** is 0.948× throughput, matching
/// the paired wall-clock reading exactly, while the uniform depth-2 leg reads
/// ~0.64×. Production's 3 MB mix makes both arms memory-bound and the driven arm's
/// instruction surplus overlaps shared stalls.
///
/// ## ★★★ MEASURED 2026-07-30 — THE DESCEND BUDGET, and 28.5% of the gap is gone
///
/// [`DESCEND_BUDGET`] amortizes the per-node trampoline tax: one `descend` walks
/// `k + 1` cut-set levels natively, so a term costs `⌈D/(k+1)⌉` suspensions rather
/// than `D`. Same instrument, same recipe, same 245,720 nodes, `fixture`-subtracted:
///
/// ```text
///                       Ir        Dr        Dw
///   derived         2458.07    663.49    556.24    <-- the CONTROL, see below
///   driven, k=0     2829.12    818.86    698.51
///   driven, k=3     2723.29    787.99    675.43
///   ratio, k=0       1.1510    1.2342    1.2558
///   ratio, k=3       1.1079    1.1876    1.2143
///   driven Δ         -3.74%    -3.77%    -3.30%
/// ```
///
/// ★★ **The `derived` arm moved by +0.0000% on all three counters** — 2458.07 →
/// 2458.07, 663.49 → 663.49, 556.24 → 556.24. It is an **invariant control that
/// costs nothing to run**: the oracle family is untouched by the budget, so a
/// non-zero reading on it would mean the measurement, the build or the fixture had
/// moved rather than the mechanism. This is the deterministic answer to a demand
/// that a wall-clock harness can only meet with a third arm.
///
/// ### Where the change went, per `cg_annotate`
///
/// ```text
///   Ir/node          k=0      k=3        Δ
///   drive_with     159.9     32.4   -127.5   <-- the trampoline, 4.93x less
///   memcpy         379.4    329.5    -49.8   <-- the 248-B moves, going away
///   push_children      —     60.1    +60.1   <-- ★ THE PRICE, and it is visible
///   rebuild_par    179.8    192.8    +13.0
///   Vec<Par>::…     25.6     29.4     +3.8
/// ```
///
/// ★ `drive_with`'s own cost falls **4.93×**, which independently corroborates the
/// **5.91×** drop in `descend` count that `models/tests/clone_descend_budget.rs`
/// measures by counting them (12,286 → 2,078 over one weighted pass). Two
/// instruments, two mechanisms, one ratio to within 17%.
///
/// ⚠★ **And the price is real and is not hidden.** `clone_push_children_par`
/// appears in the k=3 profile as a genuine function at **+60.1 Ir/node** where it
/// was previously inlined away entirely: threading a budget makes the family
/// *cyclic* (`par → send → par`), and LLVM cannot fully inline a cycle. That cost
/// is why the net is −3.74% and not the −82% a naive "82% of nodes are now native"
/// argument predicts. The naive argument prices only what the trampoline stops
/// doing and nothing that the native walk starts doing.
///
/// ### ⇒ A WORK REDUCTION WITH NO THROUGHPUT CLAIM, and the clock was ASKED
///
/// `Ir` −3.74%, `Dr` −3.77%, `Dw` −3.30%, all deterministic, `derived` invariant to
/// +0.0000%. The paired wall clock was then run in **four alternating runs, two per
/// configuration**, and it **reversed**:
///
/// ```text
///   rep   k   median-of-rep   control drift
///     1   0          0.6580           0.88%
///     1   3          0.7454           0.30%     <-- k=3 BETTER by 13.3%
///     2   0          0.8204           0.76%
///     2   3          0.7143           1.90%     <-- k=0 BETTER by 12.9%
/// ```
///
/// ⚠★★ Note what the invariant control did NOT catch. Every one of those four runs
/// had a control drift under 2%, so each was individually "usable" — and the
/// configurations still swapped places. **A control measures within-run resolution;
/// it says nothing about between-run reproducibility.** The `k = 0` span alone is
/// 24.7%. ⇒ The clock is consistent with the deterministic reading and does not
/// resolve it, which is the same disposition form-B landed under, for the same
/// reason, on the same host.
///
/// ## ⚠⚠ CORRECTED: `Outcome::Tail` IS NOT THIS GAP'S FIX
///
/// This comment used to say that closing the gap "requires … TOP-DOWN allocation
/// with a resumable continuation — `drive.rs`'s documented but unimplemented
/// `Outcome::Tail` extension." **Those are two different mechanisms and only one
/// of them is `Tail`.**
///
/// `Outcome::Tail` (now LANDED, `drive.rs` §A) is a **control-flow** primitive:
/// *"this `Combine` produces no value; re-enter `descend` on this node instead."*
/// It exists because a **codec's** parent discovers its next obligation only after
/// the previous child completes — bytes sit *between* the children — and `combine`
/// is deliberately not handed the work stack, so it cannot express a resume any
/// other way. **`Tail` moves no data.** Worse for this gap: under `Tail` each
/// resumption *pops* the child value and must park it somewhere until the node
/// completes — either back on `vals` (a **fourth** 248-byte move) or in `State`.
/// A tail call changes *when* a value is produced; this gap is about *where* it
/// lands.
///
/// ★ The correct name for what this gap needs is **destination-passing descent**
/// (out-parameter / `sret`-threading), and the reason is that **Rust's `sret` ABI
/// already is destination-passing**: `<Vec<Par> as Clone>::clone` hands
/// `Par::clone` the final slot address as its return pointer, so the derive
/// constructs each node exactly once. The driven form breaks that chain by routing
/// through `Vec<CloneVal>` — `size_of::<CloneVal>() == 248` — which LLVM cannot
/// see through. Restoring it means giving `descend` a destination, i.e. widening
/// `Step::Descend`. It requires **nothing** from `Outcome`.
///
/// ★★ **The depth-`k` hybrid was the cheaper candidate and it has LANDED**, as
/// [`DESCEND_BUDGET`] — with no `unsafe`, no change to `Node<'t>: Copy`, nothing at
/// all from `drive.rs` and no widening of `Step` (still 16 B, pinned by
/// `models/tests/drive_step_width_gate.rs`). And it turns out to restore *part* of
/// the `sret` chain for free, which is why it works: above budget 0 a cut-set slot
/// is filled by `clone_rebuild_par(v, children, budget - 1)`, whose return value
/// the ABI constructs **directly in the parent's `Vec` slot**. No `Vec<CloneVal>`
/// round trip, and therefore no 248-byte move, for the `⌈D/(k+1)⌉ − 1` of every
/// `D` levels that the budget covers. `memcpy` falls 379.4 → 329.5 Ir/node.
///
/// ⇒ Destination-passing descent — widening `Step::Descend` with a destination —
/// remains the way to remove the *remaining* moves, i.e. the ones at the budget
/// frontier. It is a separate stage with its own measurement, and it is now
/// strictly smaller than it was: the budget already removed the moves for 5 of
/// every 6 nodes on the production mix.
///
/// ⚠ **The conversion is correct and the trade is stated rather than hidden**: the
/// derived form aborts a release node at depth ~640 on a 2 MiB tokio worker
/// (3,254 B/level as `<Par as Clone>::clone`; the oracle FAMILY measures 7,021, and
/// the two are not interchangeable — see [`DESCEND_BUDGET`]), on a term a deploy
/// controls, before any budget exists to bound it. This form is flat to depth 4,096
/// and beyond **at every value of the descend budget**. A throughput cost on a
/// clone is a different KIND of quantity from an uncatchable `SIGSEGV` on the
/// validator path, and choosing between them is the reviewer's call, not this
/// file's — but the cost is now **`Ir` +10.8% per node against the derive**, down
/// from +15.1%, and it is measured with a deterministic instrument rather than
/// asserted from a blocked one.
///
/// ## ⚠⚠ Why collection ELEMENTS are pushed one at a time
///
/// The sibling `mettail-rust` generator emits nine "iterative" drivers and
/// **eight of nine measured Θ(depth)**, at 254–10,592 B/level. The cause was
/// **collection-element delegation**: a `Vec<T>` field routed to a whole-value
/// call (`a.cmp(b)`, `Hash::hash(v, state)`) which re-entered the element type's
/// trait method and recursed. The cross-*category* hop was fine; the escape was
/// `Category → Vec<Elem> → Elem`.
///
/// `Par` has exactly that shape nine times over — `Vec<Send>`, `Vec<Receive>`,
/// `Vec<New>`, `Vec<Expr>`, `Vec<Match>`, `Vec<Bundle>`, `Vec<Connective>`,
/// `Vec<If>`, `Vec<GUnforgeable>` — so §D **never** emits
/// `<Vec<T> as Clone>::clone` for an element type that reaches the cut set. It
/// emits a `for` loop that pushes each element's cut-set children individually.
/// `rholang/tests/stack_depth_gate.rs`'s `clone` and `clone_send_chain` subjects
/// are the executed proof, and both ladders nest THROUGH a `Vec` field — a pure
/// `Par → Par` chain could not exhibit the defect and would prove nothing.
fn emit_term_ops_source(
    messages: &[Message<'_>],
    resolved: &[(usize, Vec<Field>)],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    graph: &SchemaGraph,
    plan: &ClonePlan,
) -> (String, Vec<String>) {
    let oneof_by_field: BTreeMap<String, &Oneof> =
        oneofs.iter().map(|o| (oneof_key(messages, o), o)).collect();
    let fields_of: BTreeMap<&str, &[Field]> = resolved
        .iter()
        .map(|(i, fields)| (messages[*i].leaf_name(), fields.as_slice()))
        .collect();
    let path_of: BTreeMap<&str, String> = messages
        .iter()
        .map(|m| (m.leaf_name(), m.rust_path()))
        .collect();

    let mut src = String::with_capacity(160 * 1024);
    term_ops_header(&mut src, plan, graph);
    emit_clone_alphabet(&mut src, plan);
    emit_clone_pool(&mut src, plan);
    emit_clone_traversal(&mut src, plan);

    // ── §D  the families, over the ENTERED set, in DESCRIPTOR order ──
    src.push_str(
        "// ===========================================================================\n\
         // §D  The FAMILIES — one straight-line function per ENTERED item\n\
         // ===========================================================================\n\
         //\n\
         // Three mutually recursive families over the RESIDUAL child relation, which the\n\
         // generator VERIFIED acyclic, so their native recursion is bounded by the\n\
         // residual height (published as `CLONE_RESIDUAL_HEIGHT`) and not by the term:\n\
         //\n\
         //   clone_push_children_*   pushes each cut-set child, in DECLARATION order\n\
         //   clone_child_count_*     the SECOND, INDEPENDENT statement of that count,\n\
         //                           which `drive`'s deficit invariant cross-checks\n\
         //   clone_rebuild_*         rebuilds the shell, pulling one child per slot in\n\
         //                           the SAME order `push_children` pushed them\n\
         //\n\
         // ★★ ALL THREE TAKE A `budget`, and it is the SAME number in all three or the\n\
         // three walks are looking at different frontiers. It counts CUT-SET levels the\n\
         // walk may still enter NATIVELY; at 0 a cut-set child is suspended as a\n\
         // `Step::Descend` and the trampoline resumes it. So the native recursion is\n\
         // bounded by `CLONE_DESCEND_BUDGET * CLONE_RESIDUAL_HEIGHT + O(1)` frames — a\n\
         // CONSTANT of the schema and the budget, still not a function of the term. The\n\
         // surviving depth is therefore unbounded at every budget; see\n\
         // `CLONE_DESCEND_BUDGET` for why no value of it can imply a maximum depth.\n\
         //\n\
         // ⚠★ THE ORDER IS THE IDENTITY (DECLARATION) ORDER, never `min_tag`. The two\n\
         // genuinely differ for `Par` (…7, 11, 8, 12, 9, 10) and `TaggedContinuation`,\n\
         // and the three families must agree with EACH OTHER — a `push` in declaration\n\
         // order paired with a `rebuild` in tag order would hand `Par`'s cloned bundles\n\
         // to its connectives, on the hottest type in the schema, with no length change\n\
         // to give it away.\n\n",
    );
    let mut families = 0usize;
    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !plan.entered.contains(leaf) {
            continue;
        }
        let fields = fields_of
            .get(leaf)
            .unwrap_or_else(|| panic!("wire_schema: entered type `{leaf}` has no resolved fields"));
        emit_clone_family_message(
            &mut src,
            &path_of[leaf],
            leaf,
            fields,
            plan,
            &oneof_by_field,
            extern_set,
        );
        families += 1;
    }
    for oneof in oneofs {
        if !plan.oneof_entered.contains(&oneof.rust_ident) {
            continue;
        }
        emit_clone_family_oneof(&mut src, oneof, plan);
        families += 1;
    }
    assert!(
        families > 0,
        "wire_schema: the clone emitter produced NO family. The driver would then have nothing \
         to walk, every `Clone` would fall back to a whole-value copy, and the file would \
         compile — which is precisely the silent-vacuity failure the non-vacuity floors in \
         `models/build.rs` exist to refuse. Check `ClonePlan::entered`."
    );

    // ── §E  the `impl Clone`s ──
    let clone_items = plan.clone_items(messages, oneofs, extern_set);
    emit_clone_impls(
        &mut src,
        messages,
        oneofs,
        extern_set,
        plan,
        &fields_of,
        &path_of,
    );

    // ── §F  the retained oracle ──
    emit_clone_oracle(
        &mut src,
        messages,
        oneofs,
        extern_set,
        plan,
        &oneof_by_field,
        &fields_of,
        &path_of,
    );

    // ── §G  the join tables ──
    emit_clone_join_tables(&mut src, messages, oneofs, extern_set, plan, &clone_items);

    (src, clone_items)
}

fn term_ops_header(src: &mut String, plan: &ClonePlan, graph: &SchemaGraph) {
    let cut = plan
        .cut
        .iter()
        .map(|c| rust_type_name(c))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        src,
        "// @generated by models/build/wire_schema.rs from the protobuf FileDescriptorSet.\n\
         // DO NOT EDIT. Regenerate by touching models/src/main/protobuf/RhoTypes.proto.\n\
         //\n\
         // ── THE TERM-OP DRIVERS — stage F-4 fills the slot stage S2 built ──\n\
         //\n\
         // This file was EMITTED EMPTY by the pass that created it, deliberately, so that\n\
         // \"four files written, four modules compiled\" was a property the build already\n\
         // had rather than one this stage would have to establish. It now carries the\n\
         // GENERATED `Clone` for the whole `rhoapi` surface.\n\
         //\n\
         // ## The CUT SET: {cut}\n\
         //\n\
         // `Clone` is not converted type by type. It is converted at a FEEDBACK VERTEX SET\n\
         // of the child relation — the smallest set of types whose removal leaves the\n\
         // relation acyclic — because a field-wise `clone` over a DAG has native depth\n\
         // bounded by the DAG's height. So `<Send as Clone>::clone` needs no driver: it\n\
         // costs one native frame and then reaches a DRIVEN `<Par as Clone>::clone`.\n\
         //\n\
         // The residual (cut-removed) child relation has height {height}, and the\n\
         // generator VERIFIED it acyclic (every residual SCC trivial) before emitting.\n\
         // {recursive} types in this schema can contain themselves; {cut_len} of them carry a\n\
         // driver.\n\
         //\n\
         // ## What is NOT here\n\
         //\n\
         // `Ord::cmp` (F-5) and `Debug::fmt` (F-6) are the same three families over the\n\
         // same cut set with a different `Val` and a different `combine`; they are not in\n\
         // this commit. `Drop` is REFUSED, by measurement rather than by preference:\n\
         // `impl Drop for Par` produces 353 diagnostics across 61 unique lines in `models`\n\
         // alone (38 functional-record-update, 23 partial move) and the build aborts before\n\
         // reaching `rholang`. The by-move teardown stays\n\
         // `par_children::dismantle_all`.\n",
        cut = cut,
        height = plan.residual_height,
        recursive = graph.recursive.len(),
        cut_len = plan.cut.len()
    )
    .expect("write");
    src.push_str(
        "\n\
         use std::cell::RefCell;\n\
         use std::collections::BTreeMap;\n\
         use std::convert::Infallible;\n\
         \n\
         use crate::rhoapi::*;\n\
         use crate::rhoapi::connective::ConnectiveInstance;\n\
         use crate::rhoapi::expr::ExprInstance;\n\
         use crate::rhoapi::g_unforgeable::UnfInstance;\n\
         use crate::rhoapi::tagged_continuation::TaggedCont;\n\
         use crate::rhoapi::var::VarInstance;\n\
         use crate::rust::rhoapi_ext::EPathMap;\n\
         use crate::rust::rholang::drive::{drive_with, Outcome, Step, Traversal};\n\
         \n",
    );
}

/// §A — the alphabet: one arm per cut-set member, in three enums.
fn emit_clone_alphabet(src: &mut String, plan: &ClonePlan) {
    src.push_str(
        "// ===========================================================================\n\
         // §A  The ALPHABET — one arm per CUT-SET member\n\
         // ===========================================================================\n\n\
         /// One borrowed cut-set node awaiting a clone.\n\
         ///\n\
         /// ★ An enum with one arm per cut-set member, even when there is exactly one.\n\
         /// rustc lays a single-variant enum out as its payload with no discriminant, so\n\
         /// the generality is free — and a `.proto` change that introduces a cycle not\n\
         /// through the present cut set adds an arm here rather than needing a new shape.\n\
         #[derive(Clone, Copy)]\n\
         pub enum CloneNode<'t> {\n",
    );
    for name in &plan.cut {
        let ty = rust_type_name(name);
        writeln!(src, "    /// A borrowed [`{ty}`] to clone.\n    {ty}(&'t {ty}),").expect("write");
    }
    src.push_str(
        "}\n\n\
         /// One completed clone.\n\
         ///\n\
         /// ⚠ This is the `Val` that OWNS TERMS which `drive.rs` warns about: abandoning\n\
         /// the value stack releases these through the DERIVED recursive destructor.\n\
         /// [`CloneVal::dismantle`] is the iterative alternative, and\n\
         /// [`CloneStacks::drop`] is the one place it is needed (a panic unwinding out of\n\
         /// the driver).\n\
         pub enum CloneVal {\n",
    );
    for name in &plan.cut {
        let ty = rust_type_name(name);
        writeln!(src, "    /// A finished [`{ty}`].\n    {ty}({ty}),").expect("write");
    }
    src.push_str("}\n\nimpl CloneVal {\n");
    for name in &plan.cut {
        let ty = rust_type_name(name);
        let stem = ty.to_snake_case();
        let teardown = ITERATIVE_TEARDOWN
            .iter()
            .find(|(t, _)| *t == ty)
            .map(|(_, path)| *path)
            .expect("wire_schema: ClonePlan::build proved every cut member has a teardown");
        writeln!(
            src,
            "    /// Take the [`{ty}`] this value must be.\n    \
             #[inline]\n    \
             fn into_{stem}(self) -> {ty} {{\n        \
             match self {{"
        )
        .expect("write");
        for other in &plan.cut {
            let other_ty = rust_type_name(other);
            if other_ty == ty {
                writeln!(src, "            CloneVal::{ty}(v) => v,").expect("write");
                continue;
            }
            let other_teardown = ITERATIVE_TEARDOWN
                .iter()
                .find(|(t, _)| *t == other_ty)
                .map(|(_, path)| *path)
                .expect("wire_schema: every cut member has a teardown");
            writeln!(
                src,
                "            CloneVal::{other_ty}(v) => {{\n                \
                 // Released ITERATIVELY before the panic: unwinding with a deep term\n                \
                 // still on the stack would run the Theta(depth) derived destructor.\n                \
                 {other_teardown}(v);\n                \
                 clone_value_type_mismatch(\"{ty}\", \"{other_ty}\")\n            \
                 }}"
            )
            .expect("write");
        }
        src.push_str("        }\n    }\n\n");
        let _ = teardown;
    }
    // ⚠ ONE `dismantle` over the whole enum, after the per-member accessors — a
    // copy per member would be N copies of one match.
    src.push_str(
        "    /// Release this value with an ITERATIVE teardown, per\n\
         \x20   /// `wire_schema.rs`'s `ITERATIVE_TEARDOWN` table.\n\
         \x20   ///\n\
         \x20   /// ⚠ Needed on exactly one path: a PANIC unwinding out of the driver, which\n\
         \x20   /// leaves cloned terms on the pooled value stack. `Vec::clear` there would run\n\
         \x20   /// the DERIVED recursive destructor — itself Theta(depth) (gate subject\n\
         \x20   /// `par_drop`) — on a stack that is already unwinding.\n\
         \x20   fn dismantle(self) {\n\
         \x20       match self {\n",
    );
    for name in &plan.cut {
        let ty = rust_type_name(name);
        let teardown = ITERATIVE_TEARDOWN
            .iter()
            .find(|(t, _)| *t == ty)
            .map(|(_, path)| *path)
            .expect("wire_schema: every cut member has a teardown");
        writeln!(src, "            CloneVal::{ty}(v) => {teardown}(v),").expect("write");
    }
    src.push_str("        }\n    }\n}\n\n");

    src.push_str(
        "/// The defunctionalized continuation: the borrowed ORIGINAL whose shell is\n\
         /// rebuilt once its children are done, plus the value-stack index its children\n\
         /// start at.\n\
         ///\n\
         /// ★ It carries the node itself rather than a copy of the shell, so\n\
         /// `clone_rebuild_*` reads the bounded fields straight from the source.\n\
         ///\n\
         /// ⚠★ **`base` is a MEASURED optimization, not a shortcut.** `combine` used to\n\
         /// recover its children by RECOUNTING them (`clone_child_count_*`) and slicing\n\
         /// `vals.len() - n ..`. `perf` put **41.77%** of `drive_with`'s samples on a single\n\
         /// instruction — `cmp $0x24, %eax`, the 36-arm `ExprInstance` jump-table bounds\n\
         /// check — because that dispatch was paid THREE times per node (push, count,\n\
         /// rebuild) where the derive pays it once. `descend` already knows where its\n\
         /// children will start; recording it removes the third walk entirely.\n\
         ///\n\
         /// ⚠ The cross-check `clone_child_count_*` exists for is NOT lost. It becomes\n\
         /// `debug_assertions`-only — exactly like `Traversal::arity`, which still calls it —\n\
         /// and `CloneChildren::finish` still compares the count against what the REBUILD\n\
         /// consumed, unconditionally, in every profile.\n\
         #[derive(Clone, Copy)]\n\
         pub enum CloneKont<'t> {\n",
    );
    for name in &plan.cut {
        let ty = rust_type_name(name);
        writeln!(
            src,
            "    /// Rebuild this [`{ty}`]'s shell; its children start at `base` on the value\n    \
             /// stack.\n    {ty} {{ src: &'t {ty}, base: usize }},"
        )
        .expect("write");
    }
    src.push_str(
        "}\n\n\
         /// The finished children of one node, in the order `clone_push_children_*`\n\
         /// pushed them.\n\
         ///\n\
         /// ★ A `Drain` and not a `split_off`: `split_off` would allocate a `Vec` per\n\
         /// node, which on the measured production distribution (95.43% of terms at depth\n\
         /// 2) is the whole cost of the node. `Drain` removes the tail in place.\n\
         struct CloneChildren<'d> {\n\
         \x20   inner: std::vec::Drain<'d, CloneVal>,\n\
         }\n\n\
         impl<'d> CloneChildren<'d> {\n",
    );
    for name in &plan.cut {
        let ty = rust_type_name(name);
        let stem = ty.to_snake_case();
        writeln!(
            src,
            "    /// The next child, which must be a [`{ty}`].\n    \
             #[inline]\n    \
             fn {stem}(&mut self) -> {ty} {{\n        \
             match self.inner.next() {{\n            \
             Some(v) => v.into_{stem}(),\n            \
             None => clone_children_exhausted(\"{ty}\"),\n        \
             }}\n    }}\n"
        )
        .expect("write");
    }
    src.push_str(
        "    /// ★ The UNCONDITIONAL other half of the arity cross-check.\n\
         \x20   ///\n\
         \x20   /// `drive`'s deficit invariant is `debug_assertions`-only for the running\n\
         \x20   /// case, and it compares `arity()` against the number of values `combine`\n\
         \x20   /// POPPED. This compares `arity()` against the number the REBUILD asked for,\n\
         \x20   /// in every profile: a rebuild that forgot a field consumes fewer children\n\
         \x20   /// than were counted, and the leftovers are found here rather than being\n\
         \x20   /// dropped silently by `Drain`.\n\
         \x20   fn finish(mut self, ty: &'static str) {\n\
         \x20       let leftover = self.inner.by_ref().count();\n\
         \x20       if leftover != 0 {\n\
         \x20           clone_children_leftover(ty, leftover);\n\
         \x20       }\n\
         \x20   }\n\
         }\n\n\
         #[cold]\n\
         #[inline(never)]\n\
         fn clone_children_exhausted(want: &'static str) -> ! {\n\
         \x20   panic!(\n\
         \x20       \"term_ops::clone: the value stack ran out while rebuilding a `{want}` \\\n\
         \x20        child. `clone_child_count_*` and `clone_rebuild_*` are two independent \\\n\
         \x20        walks of one node and they have disagreed: the rebuild asked for more \\\n\
         \x20        children than the count reported, so the count is missing a field the \\\n\
         \x20        rebuild has (or `clone_push_children_*` never pushed it). All three \\\n\
         \x20        families are generated from ONE resolved-field vector in DECLARATION \\\n\
         \x20        order, so a disagreement means the emitter's three renderers have \\\n\
         \x20        drifted — see models/build/wire_schema.rs section 7.\"\n\
         \x20   )\n\
         }\n\n\
         #[cold]\n\
         #[inline(never)]\n\
         fn clone_children_leftover(ty: &'static str, leftover: usize) -> ! {\n\
         \x20   panic!(\n\
         \x20       \"term_ops::clone: rebuilding a `{ty}` left {leftover} cloned child value(s) \\\n\
         \x20        UNCONSUMED. `clone_child_count_{{ty}}` counted more children than \\\n\
         \x20        `clone_rebuild_{{ty}}` placed, i.e. the rebuild is missing a field the \\\n\
         \x20        count has. Those children were CLONED and are about to be dropped, so \\\n\
         \x20        the result would be a term with a silently missing subtree — which no \\\n\
         \x20        length check and no round-trip could see. This is checked in EVERY \\\n\
         \x20        profile, unlike `drive`'s running deficit invariant.\"\n\
         \x20   )\n\
         }\n\n\
         #[cold]\n\
         #[inline(never)]\n\
         fn clone_children_underflow(ty: &'static str, wanted: usize, have: usize) -> ! {\n\
         \x20   panic!(\n\
         \x20       \"term_ops::clone: rebuilding a `{ty}` needs {wanted} child value(s) but the \\\n\
         \x20        value stack holds only {have}. The children of this node were never \\\n\
         \x20        produced, which means `clone_push_children_{{ty}}` pushed fewer \\\n\
         \x20        `Descend`s than `clone_child_count_{{ty}}` counted.\"\n\
         \x20   )\n\
         }\n\n\
",
    );
    // ⚠ Emitted only when it can be CALLED. With a single-member cut set every
    // `into_*` match is total, so this would be dead code — and `models` builds
    // with `-D warnings`.
    if plan.cut.len() > 1 {
        src.push_str(
            "#[cold]\n\
             #[inline(never)]\n\
             fn clone_value_type_mismatch(want: &'static str, got: &'static str) -> ! {\n\
             \x20   panic!(\n\
             \x20       \"term_ops::clone: the value stack yielded a `{got}` where a `{want}` was \\\n\
             \x20        required. The cut set has more than one member and a `Kont`'s rebuild \\\n\
             \x20        disagrees with its `push_children` about a child's TYPE.\"\n\
             \x20   )\n\
             }\n\n",
        );
    }
}

/// ★★ **The DESCEND BUDGET `k`** — how many cut-set levels one `descend` walks
/// NATIVELY before it suspends to the heap trampoline.
///
/// ## What it is
///
/// `k = 0` is the pre-`k` machine exactly: every cut-set child becomes a
/// `Step::Descend` and the trampoline is re-entered once per node. `k = 1` walks
/// one further cut-set level in native frames, `k = 3` walks three, and a node at
/// cut-set depth `` $D$ `` below a driven root costs `` $\lceil D/k \rceil$ ``
/// heap suspensions instead of `` $D$ ``.
///
/// ## ★ Why 3, DERIVED — from the measured distribution, not from the stack
///
/// The benefit saturates at the modal depth of the *measured* produce
/// distribution (`models/benches/term_ops_bench.rs`: 1,773 instrumented datums,
/// **95.43% at depth 2**, nothing deeper than 6). A depth-2 datum is a chain of
/// **three** cut-set levels — root, its `EList`/`ETuple` elements, and the
/// `Send`'s channel and data — so `k = 3` covers **96.11%** of datums (depth 1
/// and 2 together) *entirely* inside one `descend`, and `k = 4` buys only the
/// further 2.03% at depth 3. Three is the knee.
///
/// ⇒ On the weighted mix this takes the trampoline from **12,286** re-entries per
/// pass to **2,213** — measured by `models/tests/clone_descend_budget.rs`, which
/// counts them rather than predicting them.
///
/// ## ⚠★★ Why no value of `k` can cap the representable depth
///
/// The native prefix is bounded by `k`, **not by the term**: `k` cut-set levels ×
/// at most [`CLONE_RESIDUAL_HEIGHT`] residual frames each, and then the walk
/// *suspends*. So the flat stack rises by a CONSTANT and the SLOPE stays zero,
/// which is the property `rholang/tests/stack_depth_gate.rs` actually checks
/// (`zero_slope_verdict` compares the bisected minimum stack at depth 4 against
/// depth 4,096). Surviving depth remains unbounded for every `k`; a larger `k`
/// spends a larger constant, never a maximum depth.
///
/// ⚠ **A `12288 / 3254 = 3.78` style inequality — "the native prefix must fit
/// under the driven form's 12,288 B flat cost" — is NOT the constraint, and
/// reading it as one gets both the bound and its units wrong.** The 12,288 B is
/// the flat floor the bisection reports for a traversal that does not grow; it is
/// not a depth budget, and a prefix that exceeds it costs a bigger constant, not
/// less depth. The real ceiling is the share of the smallest production stack
/// (a 2 MiB tokio worker) one is willing to spend on a constant.
///
/// Nor is 3,254 B/level the slope to price the prefix at. That figure was
/// bisected from `<Par as Clone>::clone` while it *was* the derive — a single
/// monomorphic function. The prefix is a **family of free functions**, which is
/// what `clone_oracle` is, and that measures **7,021 B/level** (re-measured at
/// HEAD, release, 124 KiB @ 16 → 892 KiB @ 128) — **2.16× more.** Priced
/// conservatively at the family figure, `k = 3` spends at most
/// `` $3 \times 7{,}021 = 21{,}063$ `` B, i.e. **1.0% of a 2 MiB worker**, on top
/// of a 12,288 B floor. `models/tests/clone_descend_budget.rs` publishes the
/// arithmetic and the gate measures what it actually costs.
const DESCEND_BUDGET: usize = 3;

/// §B — the thread-local stack pool. The reason a shallow clone allocates nothing.
fn emit_clone_pool(src: &mut String, plan: &ClonePlan) {
    let teardowns: String = plan
        .cut
        .iter()
        .map(|name| format!("`{}`", rust_type_name(name)))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        src,
        "// ===========================================================================\n\
         // §B  The POOLED STACKS — why a shallow clone allocates NOTHING\n\
         // ===========================================================================\n\
         //\n\
         // ★ `drive` allocates a work stack and a value stack per call. On the measured\n\
         // production distribution — 1,773 instrumented datums, 95.43% at depth 2, nothing\n\
         // deeper than 6 (`models/benches/wire_encode_bench.rs`) — a `Par` clone touches a\n\
         // handful of nodes, and two extra mallocs against the derive's two or three is a\n\
         // double-digit regression on the case that decides the verdict. So both stacks\n\
         // are parked per thread and handed to `drive_with`.\n\
         //\n\
         // This is `wire_encode`'s `take_ops` / `give_ops`, verbatim in structure,\n\
         // including its soundness argument and its two policies: a NESTED clone gets a\n\
         // private buffer rather than aliasing, and a one-off DEEP clone is not parked, so\n\
         // one pathological term cannot pin its high-water mark for the life of the\n\
         // thread.\n\
         //\n\
         // ⚠ The pooled value stack owns cloned {teardowns} on a panic path, and releasing\n\
         // those with `Vec::clear` would run the derived Theta(depth) destructor. `Drop`\n\
         // routes them through the ITERATIVE teardown instead."
    )
    .expect("write");
    writeln!(
        src,
        "\n\
         /// ★★ **The DESCEND BUDGET**: how many cut-set levels ONE `descend` walks in\n\
         /// NATIVE frames before it suspends to the heap trampoline.\n\
         ///\n\
         /// A node at cut-set depth `D` below a driven root costs `ceil(D / {DESCEND_BUDGET})`\n\
         /// heap suspensions instead of `D`. `0` would be the pre-budget machine exactly —\n\
         /// every cut-set child a `Step::Descend` — and is REFUSED by\n\
         /// `models/tests/clone_descend_budget.rs`, which would otherwise pass vacuously.\n\
         ///\n\
         /// ## ⚠★★ It cannot cap the representable depth, and that is STRUCTURAL\n\
         ///\n\
         /// The native prefix is bounded by this constant times [`CLONE_RESIDUAL_HEIGHT`],\n\
         /// **not by the term**: after that many levels the walk SUSPENDS. So the flat\n\
         /// native stack rises by a CONSTANT and the per-level SLOPE stays zero — which is\n\
         /// the property `rholang/tests/stack_depth_gate.rs` checks, by comparing the\n\
         /// bisected minimum stack at depth 4 against depth 4,096. Surviving depth stays\n\
         /// UNBOUNDED for every value of this constant; a larger value spends a larger\n\
         /// constant, never a maximum depth.\n\
         ///\n\
         /// ## Why {DESCEND_BUDGET}\n\
         ///\n\
         /// The measured produce distribution is 95.43% at depth 2, which is a chain of\n\
         /// THREE cut-set levels, so {DESCEND_BUDGET} covers 96.11% of datums entirely in\n\
         /// one `descend` and 4 would buy a further 2.03%. See\n\
         /// `models/build/wire_schema.rs`'s `DESCEND_BUDGET` for the full derivation,\n\
         /// including why the `12288 / 3254` inequality is NOT the constraint and why the\n\
         /// prefix is priced at the family-of-free-functions slope (7,021 B/level) rather\n\
         /// than at the single-derive one (3,254).\n\
         pub const CLONE_DESCEND_BUDGET: usize = {DESCEND_BUDGET};\n"
    )
    .expect("write");
    src.push_str(
        "\n\
         /// Work-stack preallocation. One `Par` level costs one `Combine` plus one\n\
         /// `Descend` per child, so 64 covers a 32-deep chain before the first regrowth.\n\
         const CLONE_WORK_CAPACITY: usize = 64;\n\
         /// Value-stack preallocation. A post-order fold holds only the current\n\
         /// FRONTIER, which for a chain is one value; 16 covers a wide shallow node.\n\
         const CLONE_VAL_CAPACITY: usize = 16;\n\
         /// The largest work allocation worth keeping (entries, not bytes).\n\
         const CLONE_MAX_POOLED_WORK: usize = 8192;\n\
         /// The largest value allocation worth keeping.\n\
         const CLONE_MAX_POOLED_VALS: usize = 1024;\n\
         \n\
         thread_local! {\n\
         \x20   /// The pooled work-stack ALLOCATION.\n\
         \x20   ///\n\
         \x20   /// ⚠ The parameter is `'static` because a thread-local cannot be generic over\n\
         \x20   /// a caller's lifetime. It is ALWAYS EMPTY while parked, so no\n\
         \x20   /// `Step<'static, _>` value ever exists — see [`CloneStacks::take`].\n\
         \x20   static CLONE_WORK: RefCell<Vec<Step<'static, CloneTraversal>>> =\n\
         \x20       RefCell::new(Vec::with_capacity(CLONE_WORK_CAPACITY));\n\
         \x20   /// The pooled value-stack allocation. `CloneVal` carries no lifetime, so this\n\
         \x20   /// one needs no transmute.\n\
         \x20   static CLONE_VALS: RefCell<Vec<CloneVal>> =\n\
         \x20       RefCell::new(Vec::with_capacity(CLONE_VAL_CAPACITY));\n\
         }\n\
         \n\
         /// The two stacks one driven clone runs on, borrowed with the caller's lifetime.\n\
         struct CloneStacks<'t> {\n\
         \x20   work: Vec<Step<'t, CloneTraversal>>,\n\
         \x20   vals: Vec<CloneVal>,\n\
         }\n\
         \n\
         impl<'t> CloneStacks<'t> {\n\
         \x20   /// Borrow the pooled allocations.\n\
         \x20   ///\n\
         \x20   /// # Soundness\n\
         \x20   ///\n\
         \x20   /// The parked work vector is EMPTY (the match guard), so the transmute\n\
         \x20   /// re-types ZERO live values — only the heap allocation is carried across,\n\
         \x20   /// which is the standard buffer-recycling idiom. `Step<'t, T>` and\n\
         \x20   /// `Step<'static, T>` are layout-identical: lifetimes are erased before\n\
         \x20   /// codegen and appear in no discriminant, size or alignment. [`Drop`] clears\n\
         \x20   /// before parking, so the emptiness invariant is restored on every path\n\
         \x20   /// including a panic.\n\
         \x20   ///\n\
         \x20   /// If the slot is already taken — a NESTED clone — a private vector is used\n\
         \x20   /// instead of aliasing.\n\
         \x20   #[inline]\n\
         \x20   fn take() -> CloneStacks<'t> {\n\
         \x20       let work = CLONE_WORK.with(|cell| match cell.try_borrow_mut() {\n\
         \x20           Ok(mut parked) if parked.is_empty() && parked.capacity() > 0 => {\n\
         \x20               let recycled = std::mem::take(&mut *parked);\n\
         \x20               debug_assert!(\n\
         \x20                   recycled.is_empty(),\n\
         \x20                   \"the pooled clone work stack must be parked EMPTY\"\n\
         \x20               );\n\
         \x20               // SAFETY: emptied above; see this function's soundness note.\n\
         \x20               unsafe {\n\
         \x20                   std::mem::transmute::<\n\
         \x20                       Vec<Step<'static, CloneTraversal>>,\n\
         \x20                       Vec<Step<'t, CloneTraversal>>,\n\
         \x20                   >(recycled)\n\
         \x20               }\n\
         \x20           }\n\
         \x20           _ => Vec::with_capacity(CLONE_WORK_CAPACITY),\n\
         \x20       });\n\
         \x20       let vals = CLONE_VALS.with(|cell| match cell.try_borrow_mut() {\n\
         \x20           Ok(mut parked) if parked.is_empty() && parked.capacity() > 0 => {\n\
         \x20               std::mem::take(&mut *parked)\n\
         \x20           }\n\
         \x20           _ => Vec::with_capacity(CLONE_VAL_CAPACITY),\n\
         \x20       });\n\
         \x20       CloneStacks { work, vals }\n\
         \x20   }\n\
         }\n\
         \n\
         impl<'t> Drop for CloneStacks<'t> {\n\
         \x20   fn drop(&mut self) {\n\
         \x20       let mut work = std::mem::take(&mut self.work);\n\
         \x20       work.clear();\n\
         \x20       if work.capacity() > 0 && work.capacity() <= CLONE_MAX_POOLED_WORK {\n\
         \x20           // SAFETY: emptied immediately above; see `CloneStacks::take`.\n\
         \x20           let parked: Vec<Step<'static, CloneTraversal>> =\n\
         \x20               unsafe { std::mem::transmute(work) };\n\
         \x20           CLONE_WORK.with(|cell| {\n\
         \x20               if let Ok(mut slot) = cell.try_borrow_mut() {\n\
         \x20                   // Keep the LARGER, so the pool converges upward to the working\n\
         \x20                   // set instead of oscillating.\n\
         \x20                   if slot.capacity() < parked.capacity() {\n\
         \x20                       *slot = parked;\n\
         \x20                   }\n\
         \x20               }\n\
         \x20           });\n\
         \x20       }\n\
         \n\
         \x20       let mut vals = std::mem::take(&mut self.vals);\n\
         \x20       if !vals.is_empty() {\n\
         \x20           // ⚠ A PANIC unwound out of the driver, so the value stack still owns\n\
         \x20           // CLONED TERMS. `Vec::clear` would run `drop_in_place`, itself\n\
         \x20           // Theta(depth) (gate subject `par_drop`) — on a stack that is already\n\
         \x20           // unwinding. Release them iteratively instead.\n\
         \x20           for value in vals.drain(..) {\n\
         \x20               value.dismantle();\n\
         \x20           }\n\
         \x20       }\n\
         \x20       if vals.capacity() > 0 && vals.capacity() <= CLONE_MAX_POOLED_VALS {\n\
         \x20           CLONE_VALS.with(|cell| {\n\
         \x20               if let Ok(mut slot) = cell.try_borrow_mut() {\n\
         \x20                   if slot.capacity() < vals.capacity() {\n\
         \x20                       *slot = vals;\n\
         \x20                   }\n\
         \x20               }\n\
         \x20           });\n\
         \x20       }\n\
         \x20   }\n\
         }\n\n",
    );
}

/// §C — the one `impl Traversal`.
fn emit_clone_traversal(src: &mut String, plan: &ClonePlan) {
    src.push_str(
        "// ===========================================================================\n\
         // §C  The TRAVERSAL — one `impl Traversal`, dispatching on the CUT SET\n\
         // ===========================================================================\n\n\
         /// The clone traversal: a borrowing, post-order fold that rebuilds each node's\n\
         /// shell from the finished clones of its cut-set children.\n\
         ///\n\
         /// A unit struct because the visitor is the *configuration* and there is none:\n\
         /// `Clone` needs no environment, no oracle and no output buffer, so `State` is\n\
         /// `()` too. The one piece of mutable working memory a naive form would want — a\n\
         /// scratch buffer to reverse the children through — is not needed either: the\n\
         /// region is reversed IN PLACE on the work stack.\n\
         pub struct CloneTraversal;\n\n\
         impl Traversal for CloneTraversal {\n\
         \x20   type Node<'t> = CloneNode<'t>;\n\
         \x20   type Val = CloneVal;\n\
         \x20   type Kont<'t> = CloneKont<'t>;\n\
         \x20   type State = ();\n\
         \x20   /// ★ UNINHABITED. A structural copy cannot fail, and saying so with\n\
         \x20   /// `Infallible` rather than a placeholder error type is what makes the two\n\
         \x20   /// abort paths in `drive_with` provably dead here — which is in turn why\n\
         \x20   /// this instance needs no `dismantle_all` on the error path.\n\
         \x20   type Err = Infallible;\n\
         \x20   const WORK_CAPACITY: usize = CLONE_WORK_CAPACITY;\n\
         \x20   const VAL_CAPACITY: usize = CLONE_VAL_CAPACITY;\n\n\
         \x20   /// Push the continuation, then every cut-set child.\n\
         \x20   ///\n\
         \x20   /// ★ Bounded fields are NOT touched here — they are cloned in `combine`, by\n\
         \x20   /// the same straight-line `clone_rebuild_*` that places the children. A\n\
         \x20   /// two-phase form that copied the shell on the way down would have to park\n\
         \x20   /// it somewhere, and the only place is the work stack, which is the one\n\
         \x20   /// thing that must stay small.\n\
         \x20   #[inline]\n\
         \x20   fn descend<'t>(\n\
         \x20       &mut self,\n\
         \x20       _state: &mut (),\n\
         \x20       node: CloneNode<'t>,\n\
         \x20       work: &mut Vec<Step<'t, Self>>,\n\
         \x20       vals: &mut Vec<CloneVal>,\n\
         \x20   ) -> Result<(), Infallible> {\n\
         \x20       match node {\n",
    );
    for name in &plan.cut {
        let ty = rust_type_name(name);
        let stem = ty.to_snake_case();
        writeln!(
            src,
            "            CloneNode::{ty}(src) => {{\n                \
             let base = vals.len();\n                \
             work.push(Step::Combine(CloneKont::{ty} {{ src, base }}));\n                \
             let first = work.len();\n                \
             clone_push_children_{stem}(src, work, CLONE_DESCEND_BUDGET);\n                \
             if work.len() == first {{\n                    \
             // ★★ THE WHOLLY-NATIVE FAST PATH, and it is measured. Nothing was\n                    \
             // suspended, so this node's ENTIRE subtree fits inside the descend\n                    \
             // budget and the continuation just pushed has nothing to wait for:\n                    \
             // drop it and produce the value here. It saves a `Combine` push, a\n                    \
             // loop iteration, a pop and a `Drain` setup.\n                    \
             //\n                    \
             // ★ At `CLONE_DESCEND_BUDGET == 0` this is the LEAF fast path, and on\n                    \
             // the measured production distribution (95.43% of terms at depth 2)\n                    \
             // four of a datum's six `Par` nodes are leaves. At the budget in force\n                    \
             // it is the WHOLE-DATUM path: a depth-2 datum is three cut-set levels,\n                    \
             // so all six nodes are cloned in this one call.\n                    \
             //\n                    \
             // ⚠ Invariant 1 holds in BOTH branches, and they are the two halves\n                    \
             // of it: a `descend` that pushes no work must push exactly ONE value\n                    \
             // (this branch), and a `descend` that pushes work must push NONE (the\n                    \
             // other).\n                    \
             work.truncate(first - 1);\n                    \
             let leaf = {{\n                        \
             let mut children = CloneChildren {{ inner: vals.drain(base..) }};\n                        \
             let rebuilt = clone_rebuild_{stem}(src, &mut children, CLONE_DESCEND_BUDGET);\n                        \
             children.finish(\"{ty}\");\n                        \
             rebuilt\n                    \
             }};\n                    \
             vals.push(CloneVal::{ty}(leaf));\n                \
             }} else {{\n                    \
             // ★ The children went on in DECLARATION order, so the region is\n                    \
             // reversed IN PLACE to make them POP in declaration order. No scratch\n                    \
             // buffer and no allocation.\n                    \
             //\n                    \
             // ⚠★ With a non-zero budget the region is the frontier of a DFS over\n                    \
             // several cut-set levels, not one node's child list — and one reverse\n                    \
             // is still exactly right. `drive_with` pops from the end, so the\n                    \
             // DFS-first frontier node runs first and completes (its own region is\n                    \
             // pushed above the rest and drains before control returns to the\n                    \
             // earlier ones). Values therefore land on `vals` in DFS order, which\n                    \
             // is the order `clone_rebuild_{stem}` re-enters them in.\n                    \
             work[first..].reverse();\n                \
             }}\n            \
             }}"
        )
        .expect("write");
    }
    src.push_str(
        "        }\n\
         \x20       Ok(())\n\
         \x20   }\n\n\
         \x20   /// Rebuild one node's shell around its finished children.\n\
         \x20   #[inline]\n\
         \x20   fn combine<'t>(\n\
         \x20       &mut self,\n\
         \x20       _state: &mut (),\n\
         \x20       kont: CloneKont<'t>,\n\
         \x20       vals: &mut Vec<CloneVal>,\n\
         \x20   ) -> Result<Outcome<CloneVal, CloneNode<'t>>, Infallible>\n\
         \x20   where\n\
         \x20       Self: 't,\n\
         \x20   {\n\
         \x20       match kont {\n",
    );
    for name in &plan.cut {
        let ty = rust_type_name(name);
        let stem = ty.to_snake_case();
        writeln!(
            src,
            "            CloneKont::{ty} {{ src, base }} => {{\n                \
             if base > vals.len() {{\n                    \
             clone_children_underflow(\"{ty}\", base, vals.len());\n                \
             }}\n                \
             // ⚠ The recount that used to COMPUTE `base` is now a DEBUG-ONLY\n                \
             // cross-check against the index `descend` recorded — see `CloneKont`\n                \
             // for the profile that moved it. `CloneChildren::finish` below still\n                \
             // checks the rebuild against the count in EVERY profile.\n                \
             debug_assert_eq!(\n                    \
             vals.len() - base,\n                    \
             clone_child_count_{stem}(src, CLONE_DESCEND_BUDGET),\n                    \
             \"term_ops::clone: `clone_push_children_{stem}` produced {{}} value(s) but \
             `clone_child_count_{stem}` counts {{}} at budget {{}}; two independent walks of one \
             node have drifted\",\n                    \
             vals.len() - base,\n                    \
             clone_child_count_{stem}(src, CLONE_DESCEND_BUDGET),\n                    \
             CLONE_DESCEND_BUDGET\n                \
             );\n                \
             let mut children = CloneChildren {{ inner: vals.drain(base..) }};\n                \
             let rebuilt = clone_rebuild_{stem}(src, &mut children, CLONE_DESCEND_BUDGET);\n                \
             children.finish(\"{ty}\");\n                \
             Ok(Outcome::Value(CloneVal::{ty}(rebuilt)))\n            \
             }}"
        )
        .expect("write");
    }
    src.push_str(
        "        }\n\
         \x20   }\n\n\
         \x20   /// ★ The SECOND, INDEPENDENT statement of `combine`'s pop count. It recounts\n\
         \x20   /// from the borrowed original by its own walk; `combine` consumes one child\n\
         \x20   /// per structural slot. `drive`'s deficit invariant compares them.\n\
         \x20   #[inline]\n\
         \x20   fn arity(kont: &CloneKont<'_>) -> usize {\n\
         \x20       match kont {\n",
    );
    for name in &plan.cut {
        let ty = rust_type_name(name);
        let stem = ty.to_snake_case();
        writeln!(
            src,
            "            CloneKont::{ty} {{ src, .. }} => \
             clone_child_count_{stem}(src, CLONE_DESCEND_BUDGET),"
        )
        .expect("write");
    }
    src.push_str("        }\n    }\n}\n\n");
}

/// Why a whole-value clone of a message-shaped field is FLAT — stated per field,
/// because this is the exact shape the sibling repo's eight Θ(depth) drivers
/// escaped through.
fn bounded_note(leaf: &str, plan: &ClonePlan, extern_note: bool) -> String {
    if extern_note {
        format!(
            "// `{leaf}` is EXTERN: its hand-written `Clone` is O(1) AT THE NODE (`ps` is an\n    \
             // `EntryTrie`, a refcount bump; the shadow cell is an `Arc` bump), so it never\n    \
             // re-enters a driven clone. Measured by the gate subject `clone_pathmap_chain`."
        )
    } else {
        let _ = plan;
        format!(
            "// BOUNDED: `{leaf}` cannot reach the clone cut set, so its `Clone` is flat and\n    \
             // one whole-value call is correct. ⚠ This is the ONLY shape in which a\n    \
             // `<Vec<T> as Clone>::clone` may be emitted for a message element type."
        )
    }
}

/// §D — the three families for one message.
#[allow(clippy::too_many_arguments)]
fn emit_clone_family_message(
    src: &mut String,
    rust_path: &str,
    leaf: &str,
    fields: &[Field],
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
) {
    let stem = rust_type_name(leaf).to_snake_case();
    let mut push: Vec<String> = Vec::with_capacity(fields.len());
    let mut count: Vec<String> = Vec::with_capacity(fields.len());
    let mut binds: Vec<String> = Vec::with_capacity(fields.len());
    let mut names: Vec<String> = Vec::with_capacity(fields.len());

    for f in fields {
        let name = f.rust_name.clone();
        names.push(name.clone());
        let (arity, descent) = clone_field_plan(leaf, f, plan, oneof_by_field);
        match descent {
            FieldDescent::Bounded => {
                let note = match &f.shape {
                    Shape::Message { leaf: l } | Shape::RepeatedMessage { leaf: l } => Some(
                        bounded_note(&rust_type_name(l), plan, extern_set.contains(l.as_str())),
                    ),
                    Shape::Map { value_leaf, .. } => Some(bounded_note(
                        &rust_type_name(value_leaf),
                        plan,
                        extern_set.contains(value_leaf.as_str()),
                    )),
                    // ⚠ `to_string`, NOT `format!`: a `format!` with no interpolation is
                    // `clippy::useless_format`, and this workspace's clippy job runs
                    // `-D warnings`, so one of them fails the lint for EVERY crate rather
                    // than for `models` alone. The other arms interpolate; this one does not.
                    Shape::Oneof => {
                        Some("// BOUNDED: no member of this oneof reaches the clone cut set.".to_string())
                    }
                    _ => None,
                };
                if let Some(note) = note {
                    binds.push(format!("    {note}\n    let {name} = src.{name}.clone();"));
                } else {
                    binds.push(format!("    let {name} = src.{name}.clone();"));
                }
            }
            FieldDescent::Cut(cut_ty) => {
                let cut_stem = cut_ty.to_snake_case();
                // ★★ THE BUDGET FORK, and it is the same three-line shape in all three
                // families so they cannot take different branches: at `budget == 0` this
                // is the pre-budget machine verbatim (suspend to the trampoline), and
                // above 0 the walk RE-ENTERS the cut-set type's own family natively with
                // one less level of budget. The `rebuild` side is where the win is: the
                // recursive call returns `{cut_ty}` by `sret` straight into the parent's
                // slot, which is exactly the chain the derive had and the `Vec<CloneVal>`
                // round-trip broke.
                let descend_par = format!("clone_push_children_{cut_stem}(v, work, budget - 1)");
                let count_par = format!("clone_child_count_{cut_stem}(v, budget - 1)");
                let rebuild_par = format!("clone_rebuild_{cut_stem}(v, children, budget - 1)");
                match arity {
                    FieldArity::Optional => {
                        push.push(format!(
                            "    if let Some(v) = &src.{name} {{\n        \
                             if budget == 0 {{\n            \
                             work.push(Step::Descend(CloneNode::{cut_ty}(v)));\n        \
                             }} else {{\n            \
                             {descend_par};\n        \
                             }}\n    }}"
                        ));
                        count.push(format!(
                            "    if let Some(v) = &src.{name} {{\n        \
                             n += if budget == 0 {{ 1 }} else {{ {count_par} }};\n    }}"
                        ));
                        binds.push(format!(
                            "    let {name} = match &src.{name} {{\n        \
                             Some(v) => Some(if budget == 0 {{\n            \
                             let _ = v;\n            \
                             children.{cut_stem}()\n        \
                             }} else {{\n            \
                             {rebuild_par}\n        \
                             }}),\n        \
                             None => None,\n    }};"
                        ));
                    }
                    FieldArity::Repeated => {
                        push.push(format!(
                            "    // ★ ELEMENT BY ELEMENT. `self.{name}.clone()` here would be a\n    \
                             // `<Vec<{cut_ty}> as Clone>::clone`, which re-enters the driven\n    \
                             // `{cut_ty}::clone` and is Theta(depth) — the sibling repo's defect.\n    \
                             for v in &src.{name} {{\n        \
                             if budget == 0 {{\n            \
                             work.push(Step::Descend(CloneNode::{cut_ty}(v)));\n        \
                             }} else {{\n            \
                             {descend_par};\n        \
                             }}\n    }}"
                        ));
                        count.push(format!(
                            "    if budget == 0 {{\n        \
                             n += src.{name}.len();\n    \
                             }} else {{\n        \
                             for v in &src.{name} {{\n            \
                             n += {count_par};\n        \
                             }}\n    }}"
                        ));
                        binds.push(format!(
                            "    let {name} = if budget == 0 {{\n        \
                             (0..src.{name}.len()).map(|_| children.{cut_stem}()).collect()\n    \
                             }} else {{\n        \
                             src.{name}.iter().map(|v| {rebuild_par}).collect()\n    \
                             }};"
                        ));
                    }
                    FieldArity::Map => {
                        push.push(format!(
                            "    // The map's VALUES, in `BTreeMap` (sorted-key) order — the same\n    \
                             // order the rebuild re-associates them in.\n    \
                             for v in src.{name}.values() {{\n        \
                             if budget == 0 {{\n            \
                             work.push(Step::Descend(CloneNode::{cut_ty}(v)));\n        \
                             }} else {{\n            \
                             {descend_par};\n        \
                             }}\n    }}"
                        ));
                        count.push(format!(
                            "    if budget == 0 {{\n        \
                             n += src.{name}.len();\n    \
                             }} else {{\n        \
                             for v in src.{name}.values() {{\n            \
                             n += {count_par};\n        \
                             }}\n    }}"
                        ));
                        binds.push(format!(
                            "    let {name} = if budget == 0 {{\n        \
                             src.{name}.keys().map(|k| (k.clone(), children.{cut_stem}())).collect()\n    \
                             }} else {{\n        \
                             src.{name}.iter().map(|(k, v)| (k.clone(), {rebuild_par})).collect()\n    \
                             }};"
                        ));
                    }
                }
            }
            // ⚠ The budget is threaded UNCHANGED through an `Enter`. It counts CUT-SET
            // levels, not frames: a residual hop (`Par → Expr → ExprInstance → EList`)
            // does not spend it, because the residual relation is acyclic and its height
            // is already bounded by `CLONE_RESIDUAL_HEIGHT`. Decrementing here would make
            // the budget mean "residual frames", and the same `k` would then cover a
            // fraction of a cut-set level on some paths and several on others.
            FieldDescent::Enter(child_stem) => match arity {
                FieldArity::Optional => {
                    push.push(format!(
                        "    if let Some(v) = &src.{name} {{\n        \
                         clone_push_children_{child_stem}(v, work, budget);\n    }}"
                    ));
                    count.push(format!(
                        "    if let Some(v) = &src.{name} {{\n        \
                         n += clone_child_count_{child_stem}(v, budget);\n    }}"
                    ));
                    binds.push(format!(
                        "    let {name} = match &src.{name} {{\n        \
                         Some(v) => Some(clone_rebuild_{child_stem}(v, children, budget)),\n        \
                         None => None,\n    }};"
                    ));
                }
                FieldArity::Repeated => {
                    push.push(format!(
                        "    // ★ ELEMENT BY ELEMENT — see the note on the cut-set case.\n    \
                         for v in &src.{name} {{\n        \
                         clone_push_children_{child_stem}(v, work, budget);\n    }}"
                    ));
                    count.push(format!(
                        "    for v in &src.{name} {{\n        \
                         n += clone_child_count_{child_stem}(v, budget);\n    }}"
                    ));
                    binds.push(format!(
                        "    let {name} = src.{name}.iter().map(|v| clone_rebuild_{child_stem}(v, children, budget)).collect();"
                    ));
                }
                FieldArity::Map => {
                    push.push(format!(
                        "    for v in src.{name}.values() {{\n        \
                         clone_push_children_{child_stem}(v, work, budget);\n    }}"
                    ));
                    count.push(format!(
                        "    for v in src.{name}.values() {{\n        \
                         n += clone_child_count_{child_stem}(v, budget);\n    }}"
                    ));
                    binds.push(format!(
                        "    let {name} = src.{name}.iter().map(|(k, v)| (k.clone(), clone_rebuild_{child_stem}(v, children, budget))).collect();"
                    ));
                }
            },
        }
    }

    // ── clone_push_children_<stem> ──
    writeln!(
        src,
        "/// The cut-set children of [`{rust_path}`] AT THE BUDGET FRONTIER, pushed in\n\
         /// DECLARATION order.\n\
         ///\n\
         /// `budget` is the number of further CUT-SET levels this walk may enter\n\
         /// natively; at `0` a cut-set child becomes a `Step::Descend` and the trampoline\n\
         /// resumes it. See [`CLONE_DESCEND_BUDGET`].\n\
         #[inline]\n\
         fn clone_push_children_{stem}<'t>(src: &'t {rust_path}, work: &mut Vec<Step<'t, CloneTraversal>>, budget: usize) {{"
    )
    .expect("write");
    if push.is_empty() {
        src.push_str(
            "    // No field of this type reaches the clone cut set; it is entered only\n    \
             // because it is a cut-set member itself.\n    let _ = (src, work, budget);\n",
        );
    } else {
        for line in &push {
            src.push_str(line);
            src.push('\n');
        }
    }
    src.push_str("}\n\n");

    // ── clone_child_count_<stem> ──
    writeln!(
        src,
        "/// How many cut-set children [`{rust_path}`] has AT THE BUDGET FRONTIER — i.e.\n\
         /// how many values the matching `clone_rebuild_{stem}` will pull from the value\n\
         /// stack, at the same `budget`.\n\
         ///\n\
         /// ★ A SECOND walk, written independently of the rebuild below, so\n\
         /// `drive`'s deficit invariant and `CloneChildren::finish` have something to\n\
         /// cross-check the rebuild AGAINST. ⚠ It must be called at the SAME `budget` the\n\
         /// push and the rebuild used, or the two walks are counting different frontiers.\n\
         #[inline]\n\
         fn clone_child_count_{stem}(src: &{rust_path}, budget: usize) -> usize {{"
    )
    .expect("write");
    if count.is_empty() {
        src.push_str("    let _ = (src, budget);\n    0\n");
    } else {
        src.push_str("    let mut n = 0usize;\n");
        for line in &count {
            src.push_str(line);
            src.push('\n');
        }
        src.push_str("    n\n");
    }
    src.push_str("}\n\n");

    // ── clone_rebuild_<stem> ──
    writeln!(
        src,
        "/// Rebuild a [`{rust_path}`] shell, taking one finished child per structural\n\
         /// slot in DECLARATION order — the order `clone_push_children_{stem}` pushed them.\n\
         ///\n\
         /// ⚠ The fields are bound with explicit `let`s, in declaration order, rather than\n\
         /// written straight into the struct literal. A struct literal does evaluate its\n\
         /// fields in the order WRITTEN, but the child order is load-bearing and a `let`\n\
         /// sequence states it instead of relying on that rule.\n\
         ///\n\
         /// ★ `budget` must be the SAME value `clone_push_children_{stem}` walked with:\n\
         /// above `0` a cut-set slot is rebuilt NATIVELY (returned by `sret` straight into\n\
         /// this shell's slot), at `0` it is pulled from the value stack. The two walks\n\
         /// visit the same frontier in the same order only if they agree on `budget`.\n\
         #[inline]\n\
         fn clone_rebuild_{stem}(src: &{rust_path}, children: &mut CloneChildren<'_>, budget: usize) -> {rust_path} {{"
    )
    .expect("write");
    if binds.is_empty() {
        src.push_str("    let _ = (children, budget);\n");
    } else {
        for line in &binds {
            src.push_str(line);
            src.push('\n');
        }
        if push.is_empty() {
            src.push_str("    let _ = (children, budget);\n");
        }
    }
    writeln!(src, "    {rust_path} {{ {} }}\n}}\n", names.join(", ")).expect("write");
}

/// §D — the three families for one oneof enum. Exhaustive matches, no wildcard:
/// a new member is a compile error until this file regenerates.
fn emit_clone_family_oneof(src: &mut String, oneof: &Oneof, plan: &ClonePlan) {
    let enum_ty = &oneof.rust_ident;
    let stem = enum_ty.to_snake_case();

    let arm_plan = |v: &Variant| -> FieldDescent {
        match &v.message_leaf {
            Some(leaf) => message_descent(leaf, plan),
            None => FieldDescent::Bounded,
        }
    };

    // ⚠★ A oneof whose every arm is BOUNDED still takes `budget` — the signature is
    // uniform across the family so a call site cannot be written that omits it — so
    // the parameter is discarded where no arm spends it. Same for `work`.
    let any_recursive_arm = oneof
        .variants
        .iter()
        .any(|v| !matches!(arm_plan(v), FieldDescent::Bounded));
    let discard_push = if any_recursive_arm {
        String::new()
    } else {
        "    let _ = (work, budget);\n".to_string()
    };
    let discard_count = if any_recursive_arm {
        String::new()
    } else {
        "    let _ = budget;\n".to_string()
    };
    let discard_rebuild = if any_recursive_arm {
        String::new()
    } else {
        "    let _ = (children, budget);\n".to_string()
    };

    writeln!(
        src,
        "/// The cut-set children of [`{enum_ty}`] AT THE BUDGET FRONTIER.\n\
         ///\n\
         /// ⚠ EXHAUSTIVE, with no wildcard arm: a member added to the `.proto` is a\n\
         /// COMPILE ERROR until this file regenerates — which it does, in the same pass.\n\
         #[inline]\n\
         fn clone_push_children_{stem}<'t>(src: &'t {enum_ty}, work: &mut Vec<Step<'t, CloneTraversal>>, budget: usize) {{\n\
         {discard_push}    match src {{"
    )
    .expect("write");
    for v in &oneof.variants {
        let arm = &v.rust_ident;
        match arm_plan(v) {
            FieldDescent::Bounded => {
                writeln!(src, "        {enum_ty}::{arm}(_) => {{}}").expect("write")
            }
            FieldDescent::Cut(cut_ty) => {
                let cut_stem = cut_ty.to_snake_case();
                writeln!(
                    src,
                    "        {enum_ty}::{arm}(v) => {{\n            \
                     if budget == 0 {{\n                \
                     work.push(Step::Descend(CloneNode::{cut_ty}(v)));\n            \
                     }} else {{\n                \
                     clone_push_children_{cut_stem}(v, work, budget - 1);\n            \
                     }}\n        \
                     }}"
                )
                .expect("write")
            }
            FieldDescent::Enter(child) => writeln!(
                src,
                "        {enum_ty}::{arm}(v) => clone_push_children_{child}(v, work, budget),"
            )
            .expect("write"),
        }
    }
    src.push_str("    }\n}\n\n");

    writeln!(
        src,
        "/// How many cut-set children [`{enum_ty}`]'s current arm has AT THE BUDGET\n\
         /// FRONTIER.\n\
         #[inline]\n\
         fn clone_child_count_{stem}(src: &{enum_ty}, budget: usize) -> usize {{\n\
         {discard_count}    match src {{"
    )
    .expect("write");
    for v in &oneof.variants {
        let arm = &v.rust_ident;
        match arm_plan(v) {
            FieldDescent::Bounded => {
                writeln!(src, "        {enum_ty}::{arm}(_) => 0,").expect("write")
            }
            FieldDescent::Cut(cut_ty) => {
                let cut_stem = cut_ty.to_snake_case();
                writeln!(
                    src,
                    "        {enum_ty}::{arm}(v) => {{\n            \
                     if budget == 0 {{\n                \
                     let _ = v;\n                \
                     1\n            \
                     }} else {{\n                \
                     clone_child_count_{cut_stem}(v, budget - 1)\n            \
                     }}\n        \
                     }}"
                )
                .expect("write")
            }
            FieldDescent::Enter(child) => writeln!(
                src,
                "        {enum_ty}::{arm}(v) => clone_child_count_{child}(v, budget),"
            )
            .expect("write"),
        }
    }
    src.push_str("    }\n}\n\n");

    writeln!(
        src,
        "/// Rebuild a [`{enum_ty}`] arm around its finished children.\n\
         #[inline]\n\
         fn clone_rebuild_{stem}(src: &{enum_ty}, children: &mut CloneChildren<'_>, budget: usize) -> {enum_ty} {{\n\
         {discard_rebuild}    match src {{"
    )
    .expect("write");
    for v in &oneof.variants {
        let arm = &v.rust_ident;
        match arm_plan(v) {
            FieldDescent::Bounded => writeln!(
                src,
                "        {enum_ty}::{arm}(v) => {enum_ty}::{arm}(v.clone()),"
            )
            .expect("write"),
            FieldDescent::Cut(cut_ty) => {
                let cut_stem = cut_ty.to_snake_case();
                writeln!(
                    src,
                    "        {enum_ty}::{arm}(v) => {enum_ty}::{arm}(if budget == 0 {{\n            \
                     let _ = v;\n            \
                     children.{cut_stem}()\n        \
                     }} else {{\n            \
                     clone_rebuild_{cut_stem}(v, children, budget - 1)\n        \
                     }}),"
                )
                .expect("write")
            }
            FieldDescent::Enter(child) => writeln!(
                src,
                "        {enum_ty}::{arm}(v) => {enum_ty}::{arm}(clone_rebuild_{child}(v, children, budget)),"
            )
            .expect("write"),
        }
    }
    src.push_str("    }\n}\n\n");
}

/// §E — one `impl Clone` per non-`Copy` item.
///
/// Two body shapes, selected by a DERIVED predicate rather than by a list:
///
/// * a **cut-set member** delegates to `drive_with` on the pooled stacks;
/// * everything else is **field-wise** — byte-for-byte what rustc's derive
///   emitted, generated from the same resolved-field vector the wire tables come
///   from, so it cannot drift from the schema. It is FLAT because every cycle in
///   the child relation passes through the cut set, so it reaches a driven clone
///   after at most `CLONE_RESIDUAL_HEIGHT` frames.
#[allow(clippy::too_many_arguments)]
fn emit_clone_impls(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    fields_of: &BTreeMap<&str, &[Field]>,
    path_of: &BTreeMap<&str, String>,
) {
    src.push_str(
        "// ===========================================================================\n\
         // §E  The `impl Clone` FAMILY — one per non-`Copy` item\n\
         // ===========================================================================\n\
         //\n\
         // ⚠ `models/build.rs` STRIPS `Clone` from exactly these items' `#[derive(...)]`\n\
         // lines, and the two sets are cross-checked AS SETS, in both directions, naming\n\
         // the offending type. A `Copy` item keeps its derive: `Copy`'s `Clone` must be a\n\
         // bitwise copy, and prost only derives `Copy` where every field is a scalar.\n\n",
    );
    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || plan.message_is_copy[leaf] {
            continue;
        }
        let rust_path = &path_of[leaf];
        let fields = fields_of
            .get(leaf)
            .unwrap_or_else(|| panic!("wire_schema: `{leaf}` has no resolved fields"));
        if plan.in_cut(leaf) {
            let stem = rust_type_name(leaf).to_snake_case();
            writeln!(
                src,
                "impl Clone for {rust_path} {{\n    \
                 /// ★ **DRIVEN.** [`{rust_path}`] is in the CLONE CUT SET, so its `Clone` is\n    \
                 /// the explicit-worklist traversal and native stack is O(1) in both nesting\n    \
                 /// depth and sibling width. The derived form measured 16,493 B/level debug\n    \
                 /// and 3,254 release (`rholang/tests/stack_depth_gate.rs`,\n    \
                 /// `four_quadrant_s0_baseline` at HEAD).\n    \
                 #[inline]\n    \
                 fn clone(&self) -> {rust_path} {{\n        \
                 let mut stacks = CloneStacks::take();\n        \
                 let value = match drive_with(\n            \
                 &mut CloneTraversal,\n            \
                 &mut (),\n            \
                 Step::Descend(CloneNode::{ty}(self)),\n            \
                 &mut stacks.work,\n            \
                 &mut stacks.vals,\n        \
                 ) {{\n            \
                 Ok(value) => value,\n            \
                 // `CloneTraversal::Err` is `Infallible` — UNINHABITED, so this arm\n            \
                 // is unreachable by TYPE rather than by argument. `match never {{}}`\n            \
                 // is the construct that says so.\n            \
                 Err(never) => match never {{}},\n        \
                 }};\n        \
                 value.into_{stem}()\n    \
                 }}\n}}\n",
                ty = rust_type_name(leaf)
            )
            .expect("write");
            continue;
        }
        let inits = fields
            .iter()
            .map(|f| format!("{}: self.{}.clone()", f.rust_name, f.rust_name))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            src,
            "impl Clone for {rust_path} {{\n    \
             /// FIELD-WISE, and FLAT: every cycle in the child relation passes through the\n    \
             /// cut set, so this reaches a driven clone within `CLONE_RESIDUAL_HEIGHT`\n    \
             /// frames. Byte-for-byte what the stripped derive emitted.\n    \
             #[inline]\n    \
             fn clone(&self) -> {rust_path} {{\n        \
             {rust_path} {{ {inits} }}\n    \
             }}\n}}\n"
        )
        .expect("write");
    }
    for oneof in oneofs {
        if plan.oneof_is_copy[&oneof.rust_ident] {
            continue;
        }
        let enum_ty = &oneof.rust_ident;
        writeln!(
            src,
            "impl Clone for {enum_ty} {{\n    \
             /// FIELD-WISE, and FLAT — see the message impls above. Exhaustive, with no\n    \
             /// wildcard arm: a new member is a compile error until this file regenerates.\n    \
             #[inline]\n    \
             fn clone(&self) -> {enum_ty} {{\n        \
             match self {{"
        )
        .expect("write");
        for v in &oneof.variants {
            writeln!(
                src,
                "            {enum_ty}::{arm}(v) => {enum_ty}::{arm}(v.clone()),",
                arm = v.rust_ident
            )
            .expect("write");
        }
        src.push_str("        }\n    }\n}\n\n");
    }
}

/// The oracle function name for a message leaf, if it has one.
///
/// `Copy` items have none (their `Clone` is `*self`, which the oracle can just
/// call) and neither does an EXTERN item (its `Clone` is hand-written and O(1) at
/// the node — the derive called it, so the oracle must too).
fn oracle_of_message(leaf: &str, plan: &ClonePlan, extern_set: &BTreeSet<&str>) -> Option<String> {
    if extern_set.contains(leaf) || plan.message_is_copy.get(leaf).copied().unwrap_or(false) {
        return None;
    }
    Some(format!(
        "oracle_clone_{}",
        rust_type_name(leaf).to_snake_case()
    ))
}

/// §F — the retained ORACLE: the derive's own Θ(depth) body, as free functions.
///
/// ★ **Why an oracle has to be GENERATED rather than retained in place.** Every
/// other conversion in this campaign could keep the derive compiled beside its
/// replacement, because the replacement was a new function (`wire_encode::encode`
/// beside the derived `Serialize`, and the gate carries both as
/// `bincode_ser` / `bincode_ser_derived`). `Clone` is a TRAIT IMPL: converting it
/// means the derive is gone, and with it the differential's reference.
///
/// So the derive's body is re-emitted, from the same resolved fields, as a family
/// of free functions that recurse into EACH OTHER rather than through
/// `<Par as Clone>::clone`. That makes `oracle_clone_par` a faithful Θ(depth)
/// reference — the `derived` arm of `models/benches/term_ops_bench.rs`, and the
/// oracle `models/tests/clone_equivalence_corpus.rs` compares every generated
/// clone against on seven axes.
///
/// ⚠ It is Θ(depth) BY DESIGN and must not be used on a deep term outside a
/// sized thread. That is a property of the thing it reproduces.
#[allow(clippy::too_many_arguments)]
fn emit_clone_oracle(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    fields_of: &BTreeMap<&str, &[Field]>,
    path_of: &BTreeMap<&str, String>,
) {
    src.push_str(
        "// ===========================================================================\n\
         // §F  The retained ORACLE — the DERIVE's own body, as free functions\n\
         // ===========================================================================\n\
         //\n\
         // ★ Converting a TRAIT IMPL destroys the differential's reference: unlike\n\
         // `wire_encode` (which sits beside a still-derived `Serialize`, and the gate\n\
         // carries both as `bincode_ser` / `bincode_ser_derived`), a converted `Clone`\n\
         // leaves nothing to compare against. So the derive's body is re-emitted here,\n\
         // from the same resolved fields, as functions that recurse into EACH OTHER rather\n\
         // than through `<Par as Clone>::clone`.\n\
         //\n\
         // ⚠ Theta(depth) BY DESIGN — that is what it reproduces. `models/tests/\n\
         // clone_equivalence_corpus.rs` runs it on an exhaustive SHALLOW corpus and\n\
         // `models/benches/term_ops_bench.rs` uses it as the `derived` arm; neither runs it\n\
         // deep except on an explicitly sized thread.\n\n",
    );
    for msg in messages {
        let leaf = msg.leaf_name();
        let Some(fn_name) = oracle_of_message(leaf, plan, extern_set) else {
            continue;
        };
        let rust_path = &path_of[leaf];
        let fields = fields_of
            .get(leaf)
            .unwrap_or_else(|| panic!("wire_schema: `{leaf}` has no resolved fields"));
        let inits = fields
            .iter()
            .map(|f| {
                let name = &f.rust_name;
                let expr = match &f.shape {
                    Shape::Message { leaf: l } => match oracle_of_message(l, plan, extern_set) {
                        Some(inner) => format!("src.{name}.as_ref().map({inner})"),
                        None => format!("src.{name}.clone()"),
                    },
                    Shape::RepeatedMessage { leaf: l } => {
                        match oracle_of_message(l, plan, extern_set) {
                            Some(inner) => format!("src.{name}.iter().map({inner}).collect()"),
                            None => format!("src.{name}.clone()"),
                        }
                    }
                    Shape::Map { value_leaf, .. } => {
                        match oracle_of_message(value_leaf, plan, extern_set) {
                            Some(inner) => format!(
                                "src.{name}.iter().map(|(k, v)| (k.clone(), {inner}(v))).collect()"
                            ),
                            None => format!("src.{name}.clone()"),
                        }
                    }
                    Shape::Oneof => {
                        let key = format!("{leaf}::{name}");
                        let oneof = oneof_by_field.get(&key).unwrap_or_else(|| {
                            panic!("wire_schema: no oneof answers to `{key}` for the oracle")
                        });
                        if plan.oneof_is_copy[&oneof.rust_ident] {
                            format!("src.{name}.clone()")
                        } else {
                            format!(
                                "src.{name}.as_ref().map(oracle_clone_{})",
                                oneof.rust_ident.to_snake_case()
                            )
                        }
                    }
                    _ => format!("src.{name}.clone()"),
                };
                format!("{name}: {expr}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            src,
            "/// The DERIVE's body for [`{rust_path}`].\n\
             pub fn {fn_name}(src: &{rust_path}) -> {rust_path} {{\n    \
             {rust_path} {{ {inits} }}\n}}\n"
        )
        .expect("write");
    }
    for oneof in oneofs {
        if plan.oneof_is_copy[&oneof.rust_ident] {
            continue;
        }
        let enum_ty = &oneof.rust_ident;
        writeln!(
            src,
            "/// The DERIVE's body for [`{enum_ty}`].\n\
             pub fn oracle_clone_{}(src: &{enum_ty}) -> {enum_ty} {{\n    match src {{",
            enum_ty.to_snake_case()
        )
        .expect("write");
        for v in &oneof.variants {
            let arm = &v.rust_ident;
            let expr = match &v.message_leaf {
                Some(leaf) => match oracle_of_message(leaf, plan, extern_set) {
                    Some(inner) => format!("{inner}(v)"),
                    None => "v.clone()".to_string(),
                },
                None => "v.clone()".to_string(),
            };
            writeln!(src, "        {enum_ty}::{arm}(v) => {enum_ty}::{arm}({expr}),").expect("write");
        }
        src.push_str("    }\n}\n\n");
    }
}

/// §G — the tables `models/build.rs` and the tests are checked against.
fn emit_clone_join_tables(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    clone_items: &[String],
) {
    src.push_str(
        "// ===========================================================================\n\
         // §G  The JOIN TABLES — what the build and the tests check against\n\
         // ===========================================================================\n\n\
         /// ★★ **Every item whose `Clone` this file emits**, with the BODY SHAPE chosen for\n\
         /// it: `(type, \"driven\" | \"field-wise\")`.\n\
         ///\n\
         /// This is the join point with `models/build.rs`'s textual derive-strip pass. The\n\
         /// strip records the type names it actually removed `Clone` from; this table is\n\
         /// computed from the DESCRIPTOR by an independent reproduction of prost's `Copy`\n\
         /// rule; and `models/build.rs` requires the two to agree AS SETS, in both\n\
         /// directions, naming the offending type. Stripped-but-not-emitted is a missing\n\
         /// `Clone` impl (a compile error, but one whose message names `rhoapi.rs` rather\n\
         /// than this pass); emitted-but-not-stripped is a conflicting impl.\n\
         pub static EMITTED_TRAVERSALS: &[(&str, &str)] = &[\n",
    );
    for item in clone_items {
        let shape = if plan.in_cut(item) || plan.cut.iter().any(|c| rust_type_name(c) == *item) {
            "driven"
        } else {
            "field-wise"
        };
        writeln!(src, "    (\"{item}\", \"{shape}\"),").expect("write");
    }
    src.push_str("];\n\n");

    writeln!(
        src,
        "/// The CLONE CUT SET: a feedback vertex set of the child relation, derived by the\n\
         /// greedy max-degree heuristic and then VERIFIED (every residual SCC trivial).\n\
         ///\n\
         /// ★ Every member has a DRIVEN `Clone`; no other type needs one, because a\n\
         /// field-wise clone over the residual DAG has native depth bounded by\n\
         /// [`CLONE_RESIDUAL_HEIGHT`].\n\
         ///\n\
         /// Minimality is NOT claimed — a minimum feedback vertex set is NP-hard (Karp,\n\
         /// 1972, <https://doi.org/10.1007/978-1-4684-2001-2_9>). CORRECTNESS is: the\n\
         /// generator refuses to emit unless the residual is acyclic.\n\
         pub static CLONE_CUT_SET: &[&str] = &[{}];\n",
        plan.cut
            .iter()
            .map(|c| format!("\"{}\"", rust_type_name(c)))
            .collect::<Vec<_>>()
            .join(", ")
    )
    .expect("write");

    let mut entered: Vec<String> = plan
        .entered
        .iter()
        .map(|e| rust_type_name(e))
        .chain(plan.oneof_entered.iter().cloned())
        .collect();
    entered.sort();
    writeln!(
        src,
        "\n/// The items the driver ENTERS: it walks their shells inline and suspends at the\n\
         /// cut-set nodes below them. Every other type is BOUNDED — cloned whole, in one\n\
         /// flat call.\n\
         ///\n\
         /// ★ This set is why no `<Vec<T> as Clone>::clone` is emitted for a recursive\n\
         /// element type: a `Vec<T>` whose `T` is in this set is walked element by element.\n\
         pub static CLONE_DESCEND_SET: &[&str] = &[\n{}\n];\n",
        entered
            .iter()
            .map(|e| format!("    \"{e}\","))
            .collect::<Vec<_>>()
            .join("\n")
    )
    .expect("write");

    writeln!(
        src,
        "\n/// The height of the residual (cut-removed) child relation — the constant that\n\
         /// bounds a generated field-wise `clone`'s native recursion. \"Bounded by a\n\
         /// constant\" as a NUMBER rather than as a claim.\n\
         pub const CLONE_RESIDUAL_HEIGHT: usize = {};\n",
        plan.residual_height
    )
    .expect("write");

    let bounded_externs: Vec<String> = messages
        .iter()
        .filter(|m| extern_set.contains(m.leaf_name()))
        .map(|m| rust_type_name(m.leaf_name()))
        .collect();
    writeln!(
        src,
        "\n/// ⚠★ **The EXTERN CLONE OBLIGATION.** An extern type contributes no\n\
         /// descriptor-derived children, so this pass treats it as BOUNDED — one\n\
         /// whole-value `Clone` call. That is only correct if the call is FLAT, and for\n\
         /// `EPathMap` it is a fact about its hand-written impl rather than about\n\
         /// externness: `ps` is an `EntryTrie` whose clone is a refcount bump on the trie\n\
         /// root plus an `Arc` bump on the memoized projection, and the shadow cell is an\n\
         /// `OnceLock<Arc<_>>` clone. `EPathMap::clone` never re-enters a driven clone.\n\
         ///\n\
         /// The obligation is MEASURED, not asserted: `rholang/tests/stack_depth_gate.rs`'s\n\
         /// `clone_pathmap_chain` subject nests through `ExprInstance::EPathmapBody` and is\n\
         /// gated depth-independent.\n\
         pub static CLONE_EXTERN_BOUNDED: &[&str] = &[{}];\n\
         \n\
         /// The bounded treatment must at least TYPE-CHECK: every extern type named above\n\
         /// really does implement `Clone`.\n\
         const _: fn() = || {{\n    \
         fn assert_clone<T: Clone>() {{}}\n{}\
         }};\n",
        bounded_externs
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<_>>()
            .join(", "),
        bounded_externs
            .iter()
            .map(|e| format!("    assert_clone::<{e}>();\n"))
            .collect::<String>()
    )
    .expect("write");

    let _ = oneofs;
}

// ===========================================================================
// §8  EMITTER D — the SCHEMA META (`rhoapi_schema_meta.rs`)
// ===========================================================================

fn emit_schema_meta_source(
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    graph: &SchemaGraph,
    plan: &ClonePlan,
) -> (String, usize) {
    let mut src = String::with_capacity(48 * 1024);
    src.push_str(
        "// @generated by models/build/wire_schema.rs from the protobuf FileDescriptorSet.\n\
         // DO NOT EDIT. Regenerate by touching models/src/main/protobuf/RhoTypes.proto.\n\
         //\n\
         // The SCHEMA META: the child relation, its strongly connected components, and the\n\
         // DERIVE DISPOSITION REGISTRY. Computed in the SAME pass as the two field tables,\n\
         // from the same resolved fields, so \"which types can contain themselves\" is a\n\
         // fact about the table the drivers are generated from rather than a second\n\
         // reading of the `.proto`.\n\
         \n\
         use crate::rust::rholang::schema_meta::Disposition;\n\
         \n",
    );

    // ── the child relation ──
    src.push_str(
        "/// The CHILD RELATION: `(type, the types it can directly contain)`.\n\
         ///\n\
         /// Derived from the resolved fields: a singular or repeated message field\n\
         /// contributes its type, a map contributes its VALUE type, and a oneof\n\
         /// contributes every message-payload arm. Scalars contribute nothing.\n\
         ///\n\
         /// ⚠ An EXTERN type's row is EMPTY, and that is a statement rather than an\n\
         /// omission: `EPathMap`'s containment is hand-written, so the descriptor cannot\n\
         /// supply it and this table must not pretend otherwise.\n\
         pub static SCHEMA_CHILDREN: &[(&str, &[&str])] = &[\n",
    );
    for (i, name) in graph.names.iter().enumerate() {
        let children = graph.children[i]
            .iter()
            .map(|c| format!("\"{}\"", rust_type_name(c)))
            .collect::<Vec<_>>()
            .join(", ");
        let note = if extern_set.contains(name.as_str()) {
            " // EXTERN: containment is hand-written, not descriptor-derived"
        } else {
            ""
        };
        writeln!(
            src,
            "    (\"{}\", &[{}]),{}",
            rust_type_name(name),
            children,
            note
        )
        .expect("write");
    }
    src.push_str("];\n\n");

    // ── the SCC ──
    writeln!(
        src,
        "/// Tarjan's strongly connected components of [`SCHEMA_CHILDREN`], in completion\n\
         /// order — a reverse topological order of the condensation, which is the order a\n\
         /// bottom-up driver wants.\n\
         ///\n\
         /// R. Tarjan, *Depth-First Search and Linear Graph Algorithms*, SIAM J. Comput.\n\
         /// 1(2):146-160, 1972. <https://doi.org/10.1137/0201010>\n\
         pub static SCHEMA_SCC: &[&[&str]] = &["
    )
    .expect("write");
    for component in &graph.scc {
        let members = component
            .iter()
            .map(|&i| format!("\"{}\"", rust_type_name(&graph.names[i])))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(src, "    &[{members}],").expect("write");
    }
    src.push_str("];\n\n");

    writeln!(
        src,
        "/// The types that can CONTAIN THEMSELVES: every member of an SCC of size > 1,\n\
         /// plus every singleton with a self-edge.\n\
         ///\n\
         /// ★ Every recursive walk over this schema is a walk over exactly this set. A\n\
         /// driver that bounded only the types somebody remembered would leave the rest\n\
         /// recursive, which is how a Θ(depth) traversal survives an audit.\n\
         pub static RECURSIVE_TYPES: &[&str] = &["
    )
    .expect("write");
    for name in &graph.recursive {
        writeln!(src, "    \"{}\",", rust_type_name(name)).expect("write");
    }
    src.push_str("];\n\n");

    // ── the derive disposition registry ──
    src.push_str(
        "/// ★★ **THE DERIVE DISPOSITION REGISTRY**: `(type, trait surface, disposition)`.\n\
         ///\n\
         /// One row per (generated item × run-time surface of every `#[derive]` it\n\
         /// carries). The rows are the CROSS PRODUCT of the descriptor's items with\n\
         /// `models/build/wire_schema.rs`'s closed `DERIVE_DISPOSITIONS` table, so the\n\
         /// list of recursive walks this campaign owes a driver is DERIVED from what is\n\
         /// actually derived — never hand-picked. A hand-picked list of four missed `Hash`\n\
         /// entirely, and the enumeration additionally found `Ord`/`PartialOrd`, which\n\
         /// nobody had named.\n\
         ///\n\
         /// ⚠ It is a LOWER BOUND. `PartialEq` and `Hash` are absent because\n\
         /// `models/build.rs` STRIPS them from prost's output and `models/src/lib.rs`\n\
         /// writes them by hand, so no `#[derive]` scan can see them — see\n\
         /// [`HAND_WRITTEN_TRAVERSALS`].\n\
         ///\n\
         /// `models/build.rs` cross-checks the closed table against a TEXTUAL scan of\n\
         /// `OUT_DIR/rhoapi.rs`, so a seventh trait fails the build naming itself.\n\
         pub static DERIVE_DISPOSITION_REGISTRY: &[(&str, &str, Disposition)] = &[\n",
    );
    let mut rows = 0usize;
    for msg in messages.iter() {
        if extern_set.contains(msg.leaf_name()) {
            // ⚠ An extern type carries no prost derives at all — prost does not
            // generate it. Its traits are hand-written, so it belongs in
            // `HAND_WRITTEN_TRAVERSALS`, not here.
            continue;
        }
        let ty = rust_type_name(msg.leaf_name());
        // ★ Per-item facts, so a surface whose disposition genuinely varies by
        // item can say so. See `refine_disposition`.
        let facts = ItemFacts {
            rust_name: &ty,
            is_copy: plan.message_is_copy[msg.leaf_name()],
            in_clone_cut_set: plan.in_cut(msg.leaf_name()),
        };
        for derive in DERIVE_DISPOSITIONS {
            if derive.applies_to == Applies::Oneofs {
                continue;
            }
            for (surface, disposition) in derive.surfaces {
                writeln!(
                    src,
                    "    (\"{ty}\", \"{surface}\", {}),",
                    refine_disposition(surface, *disposition, &facts).as_source()
                )
                .expect("write");
                rows += 1;
            }
        }
    }
    for oneof in oneofs {
        let ty = &oneof.rust_ident;
        let facts = ItemFacts {
            rust_name: ty,
            is_copy: plan.oneof_is_copy[ty],
            // A oneof is an ENUM, never a message, so it is never a cut-set
            // member: `CloneNode` carries borrowed MESSAGES. Its `Clone` is
            // field-wise and flat.
            in_clone_cut_set: false,
        };
        for derive in DERIVE_DISPOSITIONS {
            if derive.applies_to == Applies::Messages {
                continue;
            }
            for (surface, disposition) in derive.surfaces {
                writeln!(
                    src,
                    "    (\"{ty}\", \"{surface}\", {}),",
                    refine_disposition(surface, *disposition, &facts).as_source()
                )
                .expect("write");
                rows += 1;
            }
        }
    }
    src.push_str("];\n\n");

    // ── the traits that are NOT derives ──
    src.push_str(
        "/// The recursive walks over this schema that are NOT derive surfaces.\n\
         ///\n\
         /// ⚠★ `models/build.rs` strips `PartialEq`, `Eq` and `Hash` from prost's output\n\
         /// (`line.replace(\"PartialEq, Eq, Hash,\", \"\")`) and `models/src/lib.rs` writes\n\
         /// them BY HAND — `<Par as PartialEq>::eq` deliberately ignores `locally_free`,\n\
         /// which no derive would do. They are therefore invisible to a textual\n\
         /// `#[derive]` scan, and a driver list read off that scan alone would miss them.\n\
         ///\n\
         /// That is not hypothetical: a hand-picked list of four missed `Hash`, and `Hash`\n\
         /// is here. First measured by `rholang/tests/stack_depth_gate.rs`'s\n\
         /// `four_quadrant_s0_baseline`.\n\
         pub static HAND_WRITTEN_TRAVERSALS: &[(&str, &str)] = &[\n\
         \x20   (\"PartialEq::eq\", \"models/src/lib.rs — ignores `locally_free`\"),\n\
         \x20   (\"Hash::hash\", \"models/src/lib.rs — mirrors `PartialEq`'s field set\"),\n\
         \x20   (\"Drop::drop\", \"rustc's implicit recursive destructor; no `impl Drop` exists\"),\n\
         ];\n\n",
    );

    let disposition_traits = DERIVE_DISPOSITIONS
        .iter()
        .map(|d| format!("    \"{}\",", d.token))
        .collect::<Vec<_>>()
        .join("\n");
    writeln!(
        src,
        "/// The closed set of `#[derive]` tokens `wire_schema.rs` dispositions.\n\
         ///\n\
         /// `models/build.rs` requires the textual scan of `OUT_DIR/rhoapi.rs` to produce\n\
         /// exactly this set. A token that appears in the generated file and not here\n\
         /// fails the build naming itself; a token here that appears nowhere fails it too,\n\
         /// because a stale disposition is a claim about code that no longer exists.\n\
         pub static DISPOSITIONED_DERIVES: &[&str] = &[\n{disposition_traits}\n];"
    )
    .expect("write");

    (src, rows)
}
