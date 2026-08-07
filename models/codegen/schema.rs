//! # Schema code generation — one descriptor pass, all generated traversals
//!
//! A **build-script pass**, not a proc-macro. It reads the protobuf
//! `FileDescriptorSet` that `prost_build` was asked to dump (`build.rs`
//! `.file_descriptor_set_path(...)`), resolves every `rhoapi` message **once**,
//! and emits five files into `OUT_DIR`:
//!
//! ```text
//!   rhoapi_bincode_schema.rs    the bincode table — serializer + deserializer
//!   rhoapi_protobuf_schema.rs   the protobuf table — field order + kinds
//!   rhoapi_term_ops.rs          generated term-operation PDAs
//!   rhoapi_schema_meta.rs       child relation, SCC, and derive dispositions
//!   rhoapi_protobuf_decoder.rs
//!                               generated protobuf deserializer PDA (§8)
//! ```
//!
//! Repository-owned names identify the serialized **format**: `bincode_*` or
//! `protobuf_*`. Lowercase `prost::...`, `prost_build`, and `prost_types` names
//! below are calls into the upstream implementation API, not owned format names.
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
//! [`emit_protobuf_source`] under `sort_by_key(min_tag)`. Neither emitter walks the
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
//!   rhoapi_bincode_schema.rs
//!     <TY>_PROGRAM : &'static [FieldKind]     one per message   (the SHAPE)
//!     impl BincodeNode for <TY>                  one per message   (the ACCESS)
//!     impl BincodeOneof for <ONEOF>              one per oneof     (exhaustive!)
//!     <ONEOF>_VARIANTS : &'static [VariantProgram]              (the INDEX)
//!     <ONEOF>_VARIANT_COUNT = <ONEOF>_VARIANTS.len()  ★ never a literal
//!     EX_<FIELD> : u32                        one per ExprInstance arm
//!
//!   rhoapi_protobuf_schema.rs
//!     <TY>_PROTOBUF_PROGRAM : &'static [ProtobufField]   ASCENDING MINIMUM TAG
//!     <ONEOF>_PROTOBUF_VARIANTS                       each arm's OWN tag
//!     PROTOBUF_CONFORMANCE_REGISTRY                   every message, for the probe
//!
//!   rhoapi_schema_meta.rs
//!     SCHEMA_CHILDREN     : the child relation, per type
//!     SCHEMA_SCC          : Tarjan's strongly connected components
//!     RECURSIVE_TYPES     : the types that can contain themselves
//!     DERIVE_DISPOSITION_REGISTRY : (type, trait surface, disposition)
//!     HAND_WRITTEN_TRAVERSALS     : the walks a derive scan CANNOT see
//! ```
//!
//! The `impl BincodeOneof` matches carry **no wildcard arm**, so a 37th variant
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
//!    a missing hand-written `impl BincodeNode` is a compile error too.
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
/// Every entry MUST have a hand-written `impl BincodeNode` (and, where it is a
/// oneof payload, must behave byte-identically to its `Serialize`). Listing a
/// type here is a deliberate, reviewed act — the generator refuses to silently
/// skip anything.
pub const EXTERN_OVERRIDES: &[&str] = &["EPathMap"];

/// The proto package the table covers.
const PACKAGE: &str = "rhoapi";

/// The `OUT_DIR` file names this pass emits, in the order [`generate`] returns
/// them. `models/build.rs` writes each verbatim; naming them here keeps the
/// producer and the consumer from drifting over a string literal.
pub const OUTPUT_BINCODE_SCHEMA: &str = "rhoapi_bincode_schema.rs";
/// See [`OUTPUT_BINCODE_SCHEMA`].
pub const OUTPUT_PROTOBUF_SCHEMA: &str = "rhoapi_protobuf_schema.rs";
/// See [`OUTPUT_BINCODE_SCHEMA`].
pub const OUTPUT_TERM_OPS: &str = "rhoapi_term_ops.rs";
/// See [`OUTPUT_BINCODE_SCHEMA`].
pub const OUTPUT_SCHEMA_META: &str = "rhoapi_schema_meta.rs";
/// See [`OUTPUT_BINCODE_SCHEMA`].
///
/// The repository-owned name identifies the stable protobuf format. Calls into
/// `prost::...` inside the generated source name the current implementation API.
pub const OUTPUT_PROTOBUF_DECODER: &str = "rhoapi_protobuf_decoder.rs";

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
    pub reaches_clone_cut_set: bool,
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
                "schema: `{}` is both `Copy` and a member of the CLONE CUT SET. A `Copy` \
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
        "Ord::cmp" => match (facts.is_copy, facts.in_clone_cut_set) {
            (true, _) => Disposition::NotATraversal(
                "prost derives `Copy` for this scalar-only item, so its derived ordering has no \
                 recursive child relation",
            ),
            (false, true) => Disposition::Converted(
                "term_ops::ord_compare — descriptor-generated explicit-worklist lexicographic \
                 comparator at the schema feedback vertex set; gate subject `ord`",
            ),
            (false, false) => Disposition::FollowsFrom(
                "term_ops::ord_compare — this item retains field-wise derived ordering, whose \
                 native depth is bounded by CLONE_RESIDUAL_HEIGHT before it reaches the driven \
                 schema cut set",
            ),
        },
        "PartialOrd::partial_cmp" => match (facts.is_copy, facts.in_clone_cut_set) {
            (true, _) => Disposition::NotATraversal(
                "prost derives `Copy` for this scalar-only item, so partial ordering has no \
                 recursive child relation",
            ),
            (false, true) => Disposition::FollowsFrom(
                "term_ops::ord_compare — the generated cut-set `PartialOrd` returns \
                 `Some(Ord::cmp(self, other))`",
            ),
            (false, false) => Disposition::FollowsFrom(
                "term_ops::ord_compare — the residual derived walk reaches the driven ordering \
                 cut set within CLONE_RESIDUAL_HEIGHT frames",
            ),
        },
        "Debug::fmt" => match (facts.is_copy, facts.in_clone_cut_set) {
            (true, _) => Disposition::NotATraversal(
                "prost derives `Copy` only for scalar-only items, so this formatter has no \
                 recursive child relation",
            ),
            (false, true) => Disposition::Converted(
                "term_ops::debug_format — descriptor-generated explicit-worklist formatter at \
                 the schema feedback vertex set; gate subject `debug`",
            ),
            (false, false) => Disposition::FollowsFrom(
                "term_ops::debug_format — the residual prost-derived formatter reaches the \
                 driven schema cut set within CLONE_RESIDUAL_HEIGHT frames",
            ),
        },
        "Message::encode_raw" => match (
            facts.in_clone_cut_set,
            facts.reaches_clone_cut_set,
        ) {
            (true, _) => Disposition::Converted(
                "term_ops generated `impl prost::Message` at the schema feedback vertex; \
                 `encode_raw` writes directly through `protobuf_encoder::encode_into`",
            ),
            (false, true) => Disposition::FollowsFrom(
                "the retained Prost-derived field walk reaches the generated stack-safe Message \
                 feedback vertex within CLONE_RESIDUAL_HEIGHT frames",
            ),
            (false, false) => Disposition::NotATraversal(
                "this message cannot reach the recursive schema feedback vertex, so its generated \
                 field walk is bounded by the acyclic schema graph rather than term depth",
            ),
        },
        "Message::encoded_len" => match (
            facts.in_clone_cut_set,
            facts.reaches_clone_cut_set,
        ) {
            (true, _) => Disposition::Converted(
                "term_ops generated `impl prost::Message`; `encoded_len` uses the memoized \
                 bottom-up `protobuf_encoder::encoded_len` PDA",
            ),
            (false, true) => Disposition::FollowsFrom(
                "the retained Prost-derived length walk reaches the generated stack-safe Message \
                 feedback vertex within CLONE_RESIDUAL_HEIGHT frames",
            ),
            (false, false) => Disposition::NotATraversal(
                "this message cannot reach the recursive schema feedback vertex, so its length \
                 walk is bounded by the acyclic schema graph",
            ),
        },
        "Message::merge_field" => match (
            facts.in_clone_cut_set,
            facts.reaches_clone_cut_set,
        ) {
            (true, _) => Disposition::Converted(
                "term_ops generated `impl prost::Message`; `merge_field` delegates to the \
                 descriptor-generated `protobuf_decoder::merge_par_field` and iterative unknown-field skipper",
            ),
            (false, true) => Disposition::FollowsFrom(
                "the retained Prost-derived decoder reaches the generated stack-safe Message \
                 feedback vertex within CLONE_RESIDUAL_HEIGHT frames; DecodeContext consumption \
                 is therefore schema-bounded, not term-depth-bounded",
            ),
            (false, false) => Disposition::NotATraversal(
                "this message cannot reach the recursive schema feedback vertex, so its merge \
                 walk is bounded by the acyclic schema graph",
            ),
        },
        "Message::clear" => match (
            facts.in_clone_cut_set,
            facts.reaches_clone_cut_set,
        ) {
            (true, _) => Disposition::Converted(
                "term_ops generated `impl prost::Message`; `clear` detaches all recursive Par \
                 children through `par_children::dismantle_in_place`",
            ),
            (false, true) => Disposition::FollowsFrom(
                "the retained Prost-derived clear walk reaches the generated stack-safe Message \
                 feedback vertex within CLONE_RESIDUAL_HEIGHT frames",
            ),
            (false, false) => Disposition::NotATraversal(
                "this message cannot reach the recursive schema feedback vertex, so clearing is \
                 bounded by the acyclic schema graph",
            ),
        },
        "Oneof::encode" | "Oneof::encoded_len" | "Oneof::merge" => {
            if facts.reaches_clone_cut_set {
                Disposition::FollowsFrom(
                    "the oneof has at most one active arm and any recursive message arm reaches \
                     the generated stack-safe Message feedback vertex within CLONE_RESIDUAL_HEIGHT frames",
                )
            } else {
                Disposition::NotATraversal(
                    "no arm can reach the recursive schema feedback vertex, so the active-arm \
                     operation is bounded by the acyclic schema graph",
                )
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
            // Stage H. `models/src/rust/rholang/bincode_encoder.rs` is the
            // single-walk trampolined encoder driven by `rhoapi_bincode_schema.rs`; the
            // derive stays compiled as the differential's oracle.
            Disposition::Converted("bincode_encoder::encode"),
        )],
    },
    DeriveTrait {
        token: "serde::Deserialize",
        applies_to: Applies::Both,
        surfaces: &[(
            "Deserialize::deserialize",
            // Stage F. `models/src/rust/rholang/bincode_decoder.rs`, the explicit
            // obligation-stack decoder.
            Disposition::Converted("bincode_decoder::cold_decode"),
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
        surfaces: &[("Ord::cmp", Disposition::Converted("term_ops::ord_compare"))],
    },
    DeriveTrait {
        token: "PartialOrd",
        applies_to: Applies::Both,
        surfaces: &[(
            "PartialOrd::partial_cmp",
            Disposition::FollowsFrom("term_ops::ord_compare"),
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
                // side (`models/tests/par_protobuf_depth_ceiling.rs` stage 2).
                Disposition::Converted("protobuf_encoder via the generated Message cut-set impl"),
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
                Disposition::Converted("protobuf_encoder via the generated Message cut-set impl"),
            ),
            (
                "Message::merge_field",
                // Measured for the first time by `four_quadrant_s0_baseline`:
                // 27,794 B/level debug, 4,096 release — the most expensive per
                // level of the eight, and the only one prost itself caps
                // (`models/tests/par_protobuf_depth_ceiling.rs`).
                Disposition::Converted("protobuf_decoder via the generated Message cut-set impl"),
            ),
            (
                "Message::clear",
                // `clear` walks the same child set the encoder does; whichever
                // driver owns the child relation owns this too.
                Disposition::Converted("generated Message cut-set clear implementation"),
            ),
            (
                "Debug::fmt",
                // ⚠ Built from prost-derive's UNSORTED field list (`:85, :214`),
                // not the tag-sorted one `encode_raw` uses. Two orders inside
                // one derive; a driver for this surface must use the
                // DECLARATION order, i.e. the bincode table's.
                Disposition::Converted("term_ops::debug_format"),
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
            (
                "Oneof::encode",
                Disposition::FollowsFrom("generated Message cut-set implementation"),
            ),
            (
                "Oneof::encoded_len",
                Disposition::FollowsFrom("generated Message cut-set implementation"),
            ),
            (
                "Oneof::merge",
                Disposition::FollowsFrom("generated Message cut-set implementation"),
            ),
            (
                "Debug::fmt",
                Disposition::FollowsFrom("term_ops::debug_format"),
            ),
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
            .expect("schema: every field occupies at least one proto tag")
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
    /// The BODY of this arm in the generated `BincodeOneof::bincode_emit`, as Rust
    /// source. `v` binds the payload and `out` is in scope; the arm evaluates
    /// to `Option<&dyn BincodeNode>`.
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
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "abstract", "become", "box", "do", "final", "macro", "override",
    "priv", "typeof", "unsized", "virtual", "yield", "async", "await", "try",
];

fn escape_ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// prost's Rust field name: snake_case, keyword-escaped.
fn rust_field_name(proto: &str) -> String { escape_ident(&proto.to_snake_case()) }

/// prost's Rust type/variant name: UpperCamelCase.
fn rust_type_name(proto: &str) -> String { proto.to_upper_camel_case() }

/// The last path segment of a fully-qualified proto type name.
fn type_leaf(fq: &str) -> &str { fq.rsplit('.').next().unwrap_or(fq) }

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
    fn leaf_name(&self) -> &str { self.desc.name.as_deref().unwrap_or("") }

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

    /// The `&'static [ProtobufField]` identifier for this message's prost program.
    fn protobuf_program_ident(&self) -> String {
        self.program_ident()
            .replace("_PROGRAM", "_PROTOBUF_PROGRAM")
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
                panic!("schema: map entry `{name}` has no resolvable key field at tag 1")
            });
        let value = msg
            .field
            .iter()
            .find(|f| f.number == Some(2))
            .unwrap_or_else(|| panic!("schema: map entry `{name}` has no value field at tag 2"));
        let value_ty = value
            .r#type
            .and_then(|t| Type::try_from(t).ok())
            .unwrap_or_else(|| panic!("schema: map entry `{name}`'s value has no type"));
        assert_eq!(
            value_ty,
            Type::Message,
            "schema: map entry `{name}` has a {value_ty:?} value. Both drivers descend \
             into a map's VALUES; a scalar-valued map needs a deliberate widening, not a \
             silent reinterpretation."
        );
        entries.insert(name, MapEntry {
            key,
            value_leaf: type_leaf(value.type_name.as_deref().unwrap_or("")).to_string(),
        });
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
            "crate::rust::rholang::bincode_schema::{}_PROGRAM",
            rust_type_name(leaf).to_uppercase()
        );
    }
    programs
        .get(leaf)
        .cloned()
        .unwrap_or_else(|| panic!("schema: no program emitted for `{leaf}`"))
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
            "schema: two `{PACKAGE}` messages share the leaf name `{}`. A field's \
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
    let protobuf = emit_protobuf_source(&messages, &resolved, &oneofs, &extern_set);
    let (term_ops, clone_impls) =
        emit_term_ops_source(&messages, &resolved, &oneofs, &extern_set, &graph, &plan);
    let (schema_meta, derive_row_count) =
        emit_schema_meta_source(&messages, &oneofs, &extern_set, &graph, &plan);
    // ★ §8, the protobuf DESERIALIZER. The unknown-field skipper is
    // schema-independent; the heterogeneous PDA is generated from the same
    // resolved descriptor walk as every other recursive driver. There is no
    // hand-maintained message/oneof list to drift from prost-build's derive.
    let protobuf_decoder = emit_protobuf_decoder_source(&messages, &resolved, &oneofs, &extern_set);

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
        "schema: the term-op emitter wrote {} `impl Clone`s, but the schema has {} \
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
        "schema: the term-op source is only {} bytes. The `Clone` emission for {} items \
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
            (OUTPUT_BINCODE_SCHEMA, bincode),
            (OUTPUT_PROTOBUF_SCHEMA, protobuf),
            (OUTPUT_TERM_OPS, term_ops),
            (OUTPUT_SCHEMA_META, schema_meta),
            (OUTPUT_PROTOBUF_DECODER, protobuf_decoder),
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
                "schema: `{msg_name}.{}` uses proto3 `optional` presence, which prost \
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
            .unwrap_or_else(|| panic!("schema: `{msg_name}.{proto_name}` has no proto tag"));
        assert!(
            tag > 0,
            "schema: `{msg_name}.{proto_name}` has proto tag {tag}; protobuf tags start \
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
            "schema: oneof `{msg_name}.{proto_name}` has no members; prost would emit no \
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
            children: variants
                .iter()
                .filter_map(|v| v.message_leaf.clone())
                .collect(),
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
        panic!("schema: `{msg_name}.{proto_name}` has an unrecognised protobuf type")
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
                "schema: `{msg_name}.{proto_name}` is a map whose entry type is `{leaf}`. \
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
                "schema: `{msg_name}.{proto_name}` is a repeated `{leaf}`, which is not a \
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
            "schema: `{msg_name}.{proto_name}` is a repeated {other:?}. Only repeated \
             message / string / bytes appear in this schema; a new one needs a `FieldKind`."
        ),
        (false, Type::Message) => {
            let leaf = type_leaf(field.type_name.as_deref().unwrap_or(""));
            assert!(
                known.contains(leaf) || extern_set.contains(leaf),
                "schema: `{msg_name}.{proto_name}` is a `{leaf}`, which is not a \
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
            "schema: `{msg_name}.{proto_name}` has type {other:?}, which has no \
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
            panic!("schema: `{msg_name}.{proto_name}` has an unrecognised protobuf type")
        });
        assert!(
            field.label != Some(Label::Repeated as i32),
            "schema: `{msg_name}.{proto_name}` is a repeated oneof member, which protobuf \
             does not allow and this table cannot model."
        );
        let tag = field.number.unwrap_or_else(|| {
            panic!("schema: oneof member `{msg_name}.{proto_name}` has no proto tag")
        });
        assert!(
            tag > 0,
            "schema: oneof member `{msg_name}.{proto_name}` has proto tag {tag}; protobuf \
             tags start at 1 and prost's sort key would place a zero ahead of everything."
        );
        let mut message_leaf = None;
        let (payload_expr, program_expr) = match ty {
            Type::Message => {
                let leaf = type_leaf(field.type_name.as_deref().unwrap_or(""));
                assert!(
                    known.contains(leaf) || extern_set.contains(leaf),
                    "schema: oneof member `{msg_name}.{proto_name}` is a `{leaf}`, which \
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
                "schema: oneof member `{msg_name}.{proto_name}` has type {other:?}, which \
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
            "schema: oneof `{other}` has no registered constant prefix. Add one to \
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
        // hand-written (`models/src/rust/rholang/bincode_schema.rs`), so the descriptor
        // cannot supply its children and the graph must not pretend it can. The
        // emitted table says so per row rather than leaving a reader to infer
        // that a leaf is a leaf.
        for (i, name) in names.iter().enumerate() {
            if extern_set.contains(name.as_str()) {
                assert!(
                    children[i].is_empty(),
                    "schema: extern type `{name}` acquired descriptor-derived children, \
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
// is a refcount bump on the trie root plus `Arc` bumps on the canonical EPM1
// snapshot and layout caches. It never
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
                    .expect("schema: every message seeded a Copy slot");
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
            let desc = raw.get(owner.as_str()).unwrap_or_else(|| {
                panic!("schema: oneof `{}` has no owner message", oneof.rust_ident)
            });
            let idx = desc
                .oneof_decl
                .iter()
                .position(|d| d.name.as_deref() == Some(oneof.proto_name.as_str()))
                .unwrap_or_else(|| {
                    panic!(
                        "schema: `{owner}` does not declare a oneof named `{}`",
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
                .expect("schema: a non-empty cyclic set has a maximum");
            cut_indices.insert(pick);
        }

        let cut: Vec<String> = cut_indices
            .iter()
            .map(|&i| graph.names[i].clone())
            .collect();
        assert!(
            !cut.is_empty(),
            "schema: the CLONE CUT SET is EMPTY. The `rhoapi` child relation is cyclic by \
             construction — `Par` contains `Send` which contains `Par` — so an empty cut set \
             means the graph this pass read is not the schema's. A vacuous cut set would emit no \
             driver at all and every `Clone` would silently stay Θ(depth)."
        );
        for name in &cut {
            let rust = rust_type_name(name);
            assert!(
                ITERATIVE_TEARDOWN.iter().any(|(ty, _)| *ty == rust),
                "schema: `{rust}` joined the CLONE CUT SET but has no entry in \
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
        let oneof_by_field: BTreeMap<String, &Oneof> =
            oneofs.iter().map(|o| (oneof_key(messages, o), o)).collect();

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
    fn in_cut(&self, leaf: &str) -> bool { self.cut.iter().any(|c| c == leaf) }

    /// Every item whose `Clone` §7 emits: the non-`Copy` messages and oneofs, in
    /// descriptor order. This is the join point with `models/build.rs`'s strip.
    fn clone_items(
        &self,
        messages: &[Message<'_>],
        oneofs: &[Oneof],
        extern_set: &BTreeSet<&str>,
    ) -> Vec<String> {
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
                "schema: no message has oneof module `{}`; the oneof `{}` cannot be \
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
    height
        .iter()
        .copied()
        .filter(|h| *h != UNKNOWN)
        .max()
        .unwrap_or(0)
}

// ===========================================================================
// §5  EMITTER A — the BINCODE table (`rhoapi_bincode_schema.rs`)
// ===========================================================================
//
// ⚠★ THE ORDER IS `identity`: the resolved vector, as `resolve_message`
// produced it, which is prost-build's struct order and therefore serde's.
//
// ⚠⚠ This file's bytes are a FIXED POINT. `models/tests/
// bincode_schema_tables_conformance.rs` and `models/tests/serializer_par_byte_goldens.rs`
// are gated on the encoding it drives, and the cold store already holds byte
// strings written by it. Any change here that is not accompanied by a
// consensus-visible migration is a fork.

/// Render one bincode field emission — the BODY of one arm in the generated
/// `bincode_emit`. `self`, `out` and `i` are in scope; a descending arm `return`s a
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
        // payload are written by the generated `BincodeOneof::bincode_emit`, which is
        // monomorphic and inlines; only a MESSAGE payload suspends.
        Shape::Oneof => format!(
            "match &self.{name} {{ \
             Some(v) => {{ put_bool(out, true); \
             if let Some(node) = BincodeOneof::bincode_emit(v, out) {{ \
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
                "schema: scalar {other:?} reached the bincode renderer with no primitive. \
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
/// Doing this at build time removes a virtual `bincode_program().len()` call from
/// EVERY descent in the driver, which was the entire residual gap against the
/// derived encoder (see `bincode_schema.rs`'s `NO_RESUME`).
///
/// ⚠ It is applied HERE, per emitter, and not in [`resolve_message`]: "which
/// field is last" is a property of the ORDER, and the two emitters order the
/// same fields differently. A patch applied once, before the sort, would name
/// the wrong field for one of them.
fn bincode_program(msg_name: &str, fields: &[Field]) -> Vec<String> {
    assert!(
        fields.len() < u16::MAX as usize,
        "schema: `{msg_name}` has {} fields; `Descent::resume` is a u16 and \
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
                 // is HAND-WRITTEN in `models/src/rust/rholang/bincode_schema.rs` because its `Serialize`\n\
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
            .expect("schema: every non-extern message was resolved");
        assert_eq!(
            *index, i,
            "schema: the resolved-field vector is out of step with the message vector; \
             a generated table would be attached to the wrong type."
        );
        out.push((msg, Some(fields.as_slice())));
        cursor += 1;
    }
    assert_eq!(
        cursor,
        resolved.len(),
        "schema: resolved fields remained after every message was visited"
    );
    out
}

fn bincode_header(src: &mut String) {
    src.push_str(
        "// @generated by models/codegen/schema.rs from the protobuf FileDescriptorSet.\n\
         // DO NOT EDIT. Regenerate by touching models/src/main/protobuf/RhoTypes.proto.\n\
         //\n\
         // ONE table, BOTH directions: `models::rust::rholang::bincode_encoder` (serializer)\n\
         // and `models::rust::rholang::bincode_decoder` (deserializer) are driven by exactly the\n\
         // programs below. The indices are serde DECLARATION ORDER, never the proto tag.\n\
         \n\
         use crate::rhoapi::*;\n\
         use crate::rhoapi::connective::ConnectiveInstance;\n\
         use crate::rhoapi::expr::ExprInstance;\n\
         use crate::rhoapi::g_unforgeable::UnfInstance;\n\
         use crate::rhoapi::tagged_continuation::TaggedCont;\n\
         use crate::rhoapi::var::VarInstance;\n\
         use crate::rust::rhoapi_ext::EPathMap;\n\
         use crate::rust::rholang::bincode_schema::{\n\
         \x20   put_bool, put_bytes, put_bytes_seq, put_empty_bytes, put_i32, put_i64, put_str,\n\
         \x20   put_str_seq, put_u32, put_u64, Descent, FieldKind, VariantProgram, BincodeNode,\n\
         \x20   BincodeOneof, NO_RESUME,\n\
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

    writeln!(src, "impl BincodeNode for {rust_ty} {{").expect("write");
    writeln!(
        src,
        "    #[inline]\n    fn bincode_program(&self) -> &'static [FieldKind] {{ {program_ident} }}"
    )
    .expect("write");
    // ★ ONE virtual call per node per suspension, not one per field. The body
    // is monomorphic, so every bounded field inlines exactly as serde's derive
    // does — see `bincode_schema.rs` §A2 for the measurement that forced this shape.
    src.push_str("    fn bincode_emit(&self, from: usize, out: &mut Vec<u8>) -> Descent<'_> {\n");
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
        "impl BincodeOneof for {rust_ident} {{\n\
         \x20   /// ★ EXHAUSTIVE — no wildcard arm. A variant added to the `.proto` cannot\n\
         \x20   /// reach production untested: the table regenerates and this match with it.\n\
         \x20   ///\n\
         \x20   /// Writes the `u32` DECLARATION-ORDER index (never the proto tag) and any\n\
         \x20   /// bounded payload; returns `Some(node)` only for a message payload, which\n\
         \x20   /// is the one case the driver has to suspend at.\n\
         \x20   #[inline]\n\
         \x20   fn bincode_emit(&self, out: &mut Vec<u8>) -> Option<&dyn BincodeNode> {{\n\
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
         /// `models/tests/bincode_schema_tables_conformance.rs` runs serde's OWN derived\n\
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
         // makes the missing hand-written `impl BincodeNode` a COMPILE ERROR rather than\n\
         // a byte-level surprise at the first ground `EPathMap`.\n\
         #[allow(dead_code)]\n\
         fn bincode_extern_obligations() {\n\
         \x20   fn assert_hand_written<T: BincodeNode + ?Sized>() {}\n",
    );
    for name in extern_set {
        let ty = rust_type_name(name);
        writeln!(src, "    assert_hand_written::<{ty}>();").expect("write");
    }
    src.push_str("}\n");
}

// ===========================================================================
// §6  EMITTER B — the PROTOBUF table (`rhoapi_protobuf_schema.rs`)
// ===========================================================================
//
// ⚠★ THE ORDER IS `sort_by_key(min_tag)`. Same vector, different key. See the
// module header for the two messages where it differs and why both matter.

/// The `ProtobufKind` variant naming this field's protobuf encoding.
///
/// ★ It names a prost ENCODING MODULE, not a re-implementation. Every bounded
/// field is written by `prost::encoding::<module>::{encode, encoded_len}` — the
/// same functions `prost-derive` emits calls to — so protobuf's layout stays
/// written in exactly one place and only the RECURSION is replaced. A generator
/// that restated varint or zigzag here would be a second opinion about a byte
/// format, which is the failure mode this whole module exists to prevent.
fn protobuf_kind(shape: &Shape) -> String {
    match shape {
        // ⚠ `locally_free` reaches the protobuf wire UNBLANKED. The serde-only
        // `serialize_as_empty_bytes` normalization has no protobuf counterpart:
        // `models/tests/bincode_encoder_differential.rs` pins that prost RETAINS it.
        Shape::EmptyBytes => "ProtobufKind::Bytes".to_string(),
        Shape::Scalar(ty) => format!("ProtobufKind::{}", protobuf_scalar_variant(*ty)),
        Shape::Message { .. } => "ProtobufKind::Message".to_string(),
        Shape::RepeatedMessage { .. } => "ProtobufKind::RepeatedMessage".to_string(),
        Shape::RepeatedString => "ProtobufKind::RepeatedString".to_string(),
        Shape::RepeatedBytes => "ProtobufKind::RepeatedBytes".to_string(),
        Shape::Map { key, .. } => {
            assert_eq!(
                *key,
                Type::String,
                "schema: a map with a {key:?} key reached the prost table. The driver's \
                 map arm writes the key with `prost::encoding::string::encode`; another key \
                 type needs a deliberate widening, not a silent reinterpretation."
            );
            "ProtobufKind::MapStringMessage".to_string()
        }
        Shape::Oneof => "ProtobufKind::Oneof".to_string(),
    }
}

/// The `ProtobufKind` variant for one protobuf scalar type.
///
/// ⚠ `sint32`/`sint64` are ZIGZAG on the protobuf wire and plain `i32`/`i64` in
/// bincode — the bincode table folds them into `I32`/`I64` (serde never sees a
/// protobuf encoding), and this table must NOT. Five `sint32` fields and one
/// `sint64` are live in `RhoTypes.proto`, so the fold is not hypothetical.
fn protobuf_scalar_variant(ty: Type) -> &'static str {
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
            "schema: protobuf type {other:?} has no `ProtobufKind`. Add one deliberately — \
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
fn protobuf_scalar_module_and_default(ty: Type) -> (&'static str, &'static str) {
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
            "schema: protobuf type {other:?} has no `prost::encoding` module. Add one \
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
fn protobuf_order(fields: &[Field]) -> Vec<&Field> {
    let mut sorted: Vec<&Field> = fields.iter().collect();
    // `sort_by_key` is stable, and so is prost's, so two fields sharing a
    // minimum tag would keep declaration order in both — but they cannot:
    // `prost-derive` bails on a duplicate tag (`src/lib.rs:94-101`) and so does
    // `emit_protobuf_message`.
    sorted.sort_by_key(|f| f.min_tag());
    sorted
}

fn emit_protobuf_source(
    messages: &[Message<'_>],
    resolved: &[(usize, Vec<Field>)],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
) -> String {
    let mut src = String::with_capacity(64 * 1024);
    src.push_str(
        "// @generated by models/codegen/schema.rs from the protobuf FileDescriptorSet.\n\
         // DO NOT EDIT. Regenerate by touching models/src/main/protobuf/RhoTypes.proto.\n\
         //\n\
         // ⚠★ THE ORDER HERE IS ASCENDING MINIMUM TAG, and it is NOT the order in\n\
         // `rhoapi_bincode_schema.rs`. `prost-derive-0.14.3/src/lib.rs:87-92` sorts a message's\n\
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
         use prost::bytes::BufMut;\n\
         \n\
         use crate::rhoapi::*;\n\
         use crate::rhoapi::connective::ConnectiveInstance;\n\
         use crate::rhoapi::expr::ExprInstance;\n\
         use crate::rhoapi::g_unforgeable::UnfInstance;\n\
         use crate::rhoapi::tagged_continuation::TaggedCont;\n\
         use crate::rhoapi::var::VarInstance;\n\
         use crate::rust::rhoapi_ext::EPathMap;\n\
         use crate::rust::rholang::protobuf_schema::{\n\
         \x20   ProtobufDescent, ProtobufField, ProtobufKind, ProtobufNode, ProtobufOneof, NO_RESUME,\n\
         };\n\
         \n",
    );

    for (msg, fields) in walk_messages(messages, resolved, extern_set) {
        let rust_path = msg.rust_path();
        let Some(fields) = fields else {
            writeln!(
                src,
                "// `{rust_path}` is EXTERN (models/build.rs `.extern_path`). It has NO\n\
                 // descriptor-driven protobuf program. Its hand-written `Message` implementation\n\
                 // emits canonical EPM1 field 9 through the generated stack-safe encoder, with no\n\
                 // entry-list fallback or representation-dependent arm. It is therefore an\n\
                 // OPAQUE LEAF to the generic protobuf driver, at exact parity with what\n\
                 // `prost::encoding::message::encode` does at that position.\n"
            )
            .expect("write");
            continue;
        };
        emit_protobuf_message(
            &mut src,
            &rust_path,
            &msg.protobuf_program_ident(),
            msg.leaf_name(),
            fields,
        );
    }

    for oneof in oneofs {
        emit_protobuf_oneof(&mut src, oneof);
    }

    emit_protobuf_conformance_registry(&mut src, messages, extern_set);
    src
}

fn emit_protobuf_message(
    src: &mut String,
    rust_ty: &str,
    program_ident: &str,
    leaf_name: &str,
    fields: &[Field],
) {
    let sorted = protobuf_order(fields);

    // A duplicate tag would make the sorted order ambiguous and is a
    // `prost-derive` hard error; refuse here for the same reason.
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for f in &sorted {
        for &tag in &f.tags {
            assert!(
                seen.insert(tag),
                "schema: `{leaf_name}` has two fields at proto tag {tag}. \
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
         pub static {program_ident}: &[ProtobufField] = &[",
        program_ident.replace("_PROTOBUF_PROGRAM", "_PROGRAM")
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
            "    ProtobufField {{ tag: {}, kind: {}, name: \"{}\" }}, // {}{}",
            f.min_tag(),
            protobuf_kind(&f.shape),
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
    let (len_arms, emit_arms) = protobuf_bodies(leaf_name, &sorted);

    writeln!(src, "impl ProtobufNode for {rust_ty} {{").expect("write");
    writeln!(
        src,
        "    #[inline]\n\
         \x20   fn protobuf_program(&self) -> &'static [ProtobufField] {{ {program_ident} }}"
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

    src.push_str("    fn protobuf_len_step(&self, from: usize) -> (u64, ProtobufDescent<'_>) {\n");
    if sorted.is_empty() {
        // An empty protobuf message (`WildcardMsg`, `GSysAuthToken`) encodes to
        // ZERO bytes — no key, no length, nothing.
        src.push_str("        let _ = from;\n        (0, ProtobufDescent::Done)\n");
    } else {
        if writes_bounded {
            src.push_str("        let mut n = 0u64;\n");
        } else {
            src.push_str(
                "        // Every field of this message DESCENDS; nothing is written\n\
                          \x20       // in place, so the whole of its length comes from its\n\
                          \x20       // children's `Op::Close` contributions.\n\
                          \x20       let n = 0u64;\n",
            );
        }
        for (i, (f, arm)) in sorted.iter().zip(&len_arms).enumerate() {
            writeln!(
                src,
                "        if from < {} {{ {} }} // {}",
                i + 1,
                arm,
                f.rust_name
            )
            .expect("write");
        }
        src.push_str("        (n, ProtobufDescent::Done)\n");
    }
    src.push_str("    }\n\n");

    src.push_str(
        "    fn protobuf_emit(&self, from: usize, out: &mut dyn BufMut) -> ProtobufDescent<'_> {\n",
    );
    if sorted.is_empty() {
        src.push_str("        let _ = (from, out);\n        ProtobufDescent::Done\n");
    } else {
        if !writes_bounded {
            src.push_str(
                "        // Every field DESCENDS; the driver writes each child's key\n\
                          \x20       // and length prefix, so this body emits nothing itself.\n\
                          \x20       let _ = out;\n",
            );
        }
        for (i, (f, arm)) in sorted.iter().zip(&emit_arms).enumerate() {
            writeln!(
                src,
                "        if from < {} {{ {} }} // {}",
                i + 1,
                arm,
                f.rust_name
            )
            .expect("write");
        }
        src.push_str("        ProtobufDescent::Done\n");
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
fn protobuf_bodies(msg_name: &str, sorted: &[&Field]) -> (Vec<String>, Vec<String>) {
    assert!(
        sorted.len() < u16::MAX as usize,
        "schema: `{msg_name}` has {} fields; `ProtobufDescent::resume` is a u16 and \
         `NO_RESUME` is its maximum.",
        sorted.len()
    );
    let spent = format!("resume: {}", sorted.len());
    let mut lens = Vec::with_capacity(sorted.len());
    let mut emits = Vec::with_capacity(sorted.len());
    for (i, f) in sorted.iter().enumerate() {
        let (len, emit) = protobuf_arms(f, i);
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
fn protobuf_arms(field: &Field, index: usize) -> (String, String) {
    let name = &field.rust_name;
    let resume = index + 1;
    let tag = field.min_tag();
    match &field.shape {
        // ⚠ `locally_free` is ORDINARY BYTES here. The eight-zero-bytes rule is
        // serde-only; prost retains the field.
        Shape::EmptyBytes => protobuf_scalar_arms(name, Type::Bytes, tag),
        Shape::Scalar(ty) => protobuf_scalar_arms(name, *ty, tag),
        Shape::RepeatedString => (
            format!("n += encoding::string::encoded_len_repeated({tag}u32, &self.{name}) as u64;"),
            format!("encoding::string::encode_repeated({tag}u32, &self.{name}, &mut &mut *out);"),
        ),
        Shape::RepeatedBytes => (
            format!("n += encoding::bytes::encoded_len_repeated({tag}u32, &self.{name}) as u64;"),
            format!("encoding::bytes::encode_repeated({tag}u32, &self.{name}, &mut &mut *out);"),
        ),
        // A singular message is OMITTED entirely when `None`
        // (`prost-derive-0.14.3/src/field/message.rs`, the `Optional` arm), so
        // there is no tag and no zero-length body to write.
        Shape::Message { .. } => {
            let descent =
                format!("ProtobufDescent::Node {{ resume: {resume}, tag: {tag}u32, node: v }}");
            (
                format!("if let Some(v) = &self.{name} {{ return (n, {descent}); }}"),
                format!("if let Some(v) = &self.{name} {{ return {descent}; }}"),
            )
        }
        // Each element carries its OWN key and length prefix, all under `tag`.
        Shape::RepeatedMessage { .. } => {
            let descent = format!(
                "ProtobufDescent::Seq {{ resume: {resume}, tag: {tag}u32, len: k, seq: s }}"
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
                format!("ProtobufDescent::Map {{ resume: {resume}, tag: {tag}u32, map: m }}");
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
                 let (b, child) = ProtobufOneof::protobuf_len_step(v); n += b; \
                 if let Some((t, node)) = child {{ \
                 return (n, ProtobufDescent::Node {{ resume: {resume}, tag: t, node }}); }} }}"
            ),
            format!(
                "if let Some(v) = &self.{name} {{ \
                 if let Some((t, node)) = ProtobufOneof::protobuf_emit(v, out) {{ \
                 return ProtobufDescent::Node {{ resume: {resume}, tag: t, node }}; }} }}"
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
fn protobuf_scalar_arms(name: &str, ty: Type, tag: u32) -> (String, String) {
    let (module, default) = protobuf_scalar_module_and_default(ty);
    let guard = format!("if self.{name} != {default}");
    (
        format!(
            "{guard} {{ n += encoding::{module}::encoded_len({tag}u32, &self.{name}) as u64; }}"
        ),
        format!(
            "{guard} {{ encoding::{module}::encode({tag}u32, &self.{name}, &mut &mut *out); }}"
        ),
    )
}

fn emit_protobuf_oneof(src: &mut String, oneof: &Oneof) {
    let Oneof {
        proto_name,
        rust_ident,
        module,
        variants,
    } = oneof;
    let table_ident = format!("{}_PROTOBUF_VARIANTS", proto_name.to_uppercase());

    writeln!(
        src,
        "// oneof `{proto_name}` → `crate::rhoapi::{module}::{rust_ident}`\n\
         //\n\
         // ⚠ The ARM writes its own tag. prost places the oneof FIELD at the position of\n\
         // its lowest tag but each variant encodes under the tag it actually declares —\n\
         // `prost-derive-0.14.3/src/lib.rs:462-471`. The two are different numbers for\n\
         // every arm but the first.\n\
         /// Each arm of [`{rust_ident}`], carrying the tag the ARM writes.\n\
         pub static {table_ident}: &[ProtobufField] = &["
    )
    .expect("write");
    for v in variants {
        let kind = match &v.message_leaf {
            Some(_) => "ProtobufKind::Message".to_string(),
            None => format!("ProtobufKind::{}", protobuf_scalar_variant(v.ty)),
        };
        writeln!(
            src,
            "    ProtobufField {{ tag: {}, kind: {}, name: \"{}\" }}, // serde index {}",
            v.tag, kind, v.rust_ident, v.index
        )
        .expect("write");
    }
    src.push_str("];\n\n");

    // ── the exhaustive impl: no wildcard arm, so a new variant fails to compile ──
    writeln!(
        src,
        "impl ProtobufOneof for {rust_ident} {{\n\
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
         \x20   fn protobuf_len_step(&self) -> (u64, Option<(u32, &dyn ProtobufNode)>) {{\n\
         \x20       match self {{"
    )
    .expect("write");
    for v in variants {
        let arm = match &v.message_leaf {
            // The DRIVER writes a message arm's key and length prefix; it is the
            // only place that knows the child's length.
            Some(_) => format!("(0, Some(({}u32, v)))", v.tag),
            None => {
                let (module, _) = protobuf_scalar_module_and_default(v.ty);
                format!(
                    "(encoding::{module}::encoded_len({}u32, v) as u64, None)",
                    v.tag
                )
            }
        };
        writeln!(
            src,
            "            {rust_ident}::{}(v) => {arm},",
            v.rust_ident
        )
        .expect("write");
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
         \x20   /// [`Self::protobuf_len_step`] for why no arm is skipped at its default.\n\
         \x20   #[inline]\n\
         \x20   fn protobuf_emit(&self, out: &mut dyn BufMut) -> Option<(u32, &dyn ProtobufNode)> {{{}\n\
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
                let (module, _) = protobuf_scalar_module_and_default(v.ty);
                format!(
                    "{{ encoding::{module}::encode({}u32, v, &mut &mut *out); None }}",
                    v.tag
                )
            }
        };
        writeln!(
            src,
            "            {rust_ident}::{}(v) => {arm},",
            v.rust_ident
        )
        .expect("write");
    }
    src.push_str("        }\n    }\n}\n\n");
}

fn emit_protobuf_conformance_registry(
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
         pub static PROTOBUF_CONFORMANCE_REGISTRY: &[(&str, &[ProtobufField])] = &[\n",
    );
    for msg in messages {
        if extern_set.contains(msg.leaf_name()) {
            continue;
        }
        writeln!(
            src,
            "    (\"{}\", {}),",
            rust_type_name(msg.leaf_name()),
            msg.protobuf_program_ident()
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
                    "schema: `{owner_leaf}.{}` is a oneof field but no resolved oneof \
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
/// [`crate::rust::rholang::bincode_schema`] §A2 records the obvious factoring — a generated
/// table exposing `fn bincode_field(i) -> FieldVal` interpreted by a hand-written
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
/// **1.0748× (PASS) then 0.9461× (FAIL)**. `bincode_encoder_bench` carried the
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
            .unwrap_or_else(|| panic!("schema: entered type `{leaf}` has no resolved fields"));
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
        "schema: the clone emitter produced NO family. The driver would then have nothing \
         to walk, every `Clone` would fall back to a whole-value copy, and the file would \
         compile — which is precisely the silent-vacuity failure the non-vacuity floors in \
         `models/build.rs` exist to refuse. Check `ClonePlan::entered`."
    );

    // ── §E  the `impl Clone`s ──
    let clone_items = plan.clone_items(messages, oneofs, extern_set);
    emit_clone_impls(
        &mut src, messages, oneofs, extern_set, plan, &fields_of, &path_of,
    );

    // `Ord` and `PartialOrd` use the same descriptor-derived feedback vertex
    // set as `Clone`: only cut-set impls are stripped from prost's output and
    // replaced, while the residual derived call graph is statically bounded.
    emit_ord_driver(
        &mut src,
        messages,
        resolved,
        oneofs,
        extern_set,
        plan,
        &oneof_by_field,
        &path_of,
    );

    // Prost's generated `Debug` follows declaration order, just like the
    // bincode schema.  Break the same descriptor-derived feedback vertex set
    // with a formatter PDA; residual generated impls remain bounded.
    emit_debug_driver(
        &mut src,
        messages,
        resolved,
        oneofs,
        extern_set,
        plan,
        &oneof_by_field,
        &path_of,
    );

    emit_par_message_impl(
        &mut src,
        fields_of
            .get("Par")
            .copied()
            .expect("schema: Par must have resolved fields"),
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

fn emit_par_message_impl(src: &mut String, fields: &[Field]) {
    src.push_str("impl Default for Par {\n    fn default() -> Self {\n        Self {\n");
    for field in fields {
        writeln!(src, "            {}: Default::default(),", field.rust_name)
            .expect("write Par default field");
    }
    src.push_str(
        "        }\n\
         \x20   }\n\
         }\n\n\
         impl prost::Message for Par {\n\
         \x20   fn encode_raw(&self, buf: &mut impl prost::bytes::BufMut) {\n\
         \x20       crate::rust::rholang::protobuf_encoder::encode_into(self, buf);\n\
         \x20   }\n\n\
         \x20   #[cfg(feature = \"phase7-depth-histograms\")]\n\
         \x20   fn merge(&mut self, mut buf: impl prost::bytes::Buf) -> Result<(), prost::DecodeError> {\n\
         \x20       let ctx = prost::encoding::DecodeContext::default();\n\
         \x20       while buf.has_remaining() {\n\
         \x20           let (tag, protobuf_wire_type) = prost::encoding::decode_key(&mut buf)?;\n\
         \x20           self.merge_field(tag, protobuf_wire_type, &mut buf, ctx.clone())?;\n\
         \x20       }\n\
         \x20       crate::rust::rholang::phase7_depth_histogram::record_par(\"protobuf_decoder\", self);\n\
         \x20       Ok(())\n\
         \x20   }\n\n\
         \x20   fn merge_field(\n\
         \x20       &mut self,\n\
         \x20       tag: u32,\n\
         \x20       protobuf_wire_type: prost::encoding::WireType,\n\
         \x20       buf: &mut impl prost::bytes::Buf,\n\
         \x20       ctx: prost::encoding::DecodeContext,\n\
         \x20   ) -> Result<(), prost::DecodeError> {\n\
         \x20       crate::rust::rholang::protobuf_decoder::merge_par_field(\n\
         \x20           self, tag, protobuf_wire_type, buf, ctx,\n\
         \x20       )\n\
         \x20   }\n\n\
         \x20   fn encoded_len(&self) -> usize {\n\
         \x20       crate::rust::rholang::protobuf_encoder::encoded_len(self)\n\
         \x20   }\n\n\
         \x20   fn clear(&mut self) {\n\
         \x20       crate::rust::rholang::par_children::dismantle_in_place(self);\n",
    );
    for field in fields {
        writeln!(
            src,
            "        self.{} = Default::default();",
            field.rust_name
        )
        .expect("write Par clear field");
    }
    src.push_str("    }\n}\n\n");
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
        "// @generated by models/codegen/schema.rs from the protobuf FileDescriptorSet.\n\
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
         // ## Generated destructor\n\
         //\n\
         // `Drop for Par` delegates to the same schema-exhaustive, by-move child table as\n\
         // `par_children::dismantle_all`, but starts from `&mut self` because Rust forbids\n\
         // moving a value that implements `Drop`. Recursive children live on one heap\n\
         // worklist; every shell reaches ordinary field destruction only after its `Par`\n\
         // children have been detached.\n\
         //\n\
         // `Ord::cmp` (F-5) and `Debug::fmt` (F-6) use the same cut set with different\n\
         // value and continuation alphabets; they remain separate generated families.\n",
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
         use std::hash::Hash;\n\
         \n\
         use pathmap::zipper::{ZipperMoving, ZipperReadOnlyIteration};\n\
         \n\
         use crate::rhoapi::*;\n\
         use crate::rhoapi::connective::ConnectiveInstance;\n\
         use crate::rhoapi::expr::ExprInstance;\n\
         use crate::rhoapi::g_unforgeable::UnfInstance;\n\
         use crate::rhoapi::tagged_continuation::TaggedCont;\n\
         use crate::rhoapi::var::VarInstance;\n\
         use crate::rust::rhoapi_ext::EPathMap;\n\
         use crate::rust::rholang::drive::{drive_with, Outcome, Step, Traversal};\n\
         \n\
         /// Stack-safe destructor for the recursive schema cut set.\n\
         impl Drop for Par {\n\
             #[inline]\n\
             fn drop(&mut self) {\n\
                 crate::rust::rholang::par_children::dismantle_in_place(self);\n\
             }\n\
         }\n\
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
        writeln!(
            src,
            "    /// A borrowed [`{ty}`] to clone.\n    {ty}(&'t {ty}),"
        )
        .expect("write");
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
            .expect("schema: ClonePlan::build proved every cut member has a teardown");
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
                .expect("schema: every cut member has a teardown");
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
         \x20   /// `codegen/schema.rs`'s `ITERATIVE_TEARDOWN` table.\n\
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
            .expect("schema: every cut member has a teardown");
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
         \x20        drifted — see models/codegen/schema.rs section 7.\"\n\
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
         // deeper than 6 (`models/benches/bincode_encoder_bench.rs`) — a `Par` clone touches a\n\
         // handful of nodes, and two extra mallocs against the derive's two or three is a\n\
         // double-digit regression on the case that decides the verdict. So both stacks\n\
         // are parked per thread and handed to `drive_with`.\n\
         //\n\
         // This is `bincode_encoder`'s `take_ops` / `give_ops`, verbatim in structure,\n\
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
         /// `models/codegen/schema.rs`'s `DESCEND_BUDGET` for the full derivation,\n\
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
             // `EntryTrie`; trie-root and EPM1 cache clones are refcount bumps), so it never\n    \
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
                    Shape::Oneof => Some(
                        "// BOUNDED: no member of this oneof reaches the clone cut set."
                            .to_string(),
                    ),
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
            .unwrap_or_else(|| panic!("schema: `{leaf}` has no resolved fields"));
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

fn ord_message_arm(leaf: &str) -> String { format!("M{}", rust_type_name(leaf)) }

fn ord_oneof_arm(oneof: &Oneof) -> String { format!("O{}", oneof.rust_ident) }

fn ord_node_expr(leaf: &str, value: &str) -> String {
    format!("TermRef::{}({value})", ord_message_arm(leaf))
}

fn ord_oneof_expr(oneof: &Oneof, value: &str) -> String {
    format!("TermRef::{}({value})", ord_oneof_arm(oneof))
}

fn emit_debug_driver(
    src: &mut String,
    messages: &[Message<'_>],
    resolved: &[(usize, Vec<Field>)],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    path_of: &BTreeMap<&str, String>,
) {
    let fields_of: BTreeMap<&str, &[Field]> = resolved
        .iter()
        .map(|(i, fields)| (messages[*i].leaf_name(), fields.as_slice()))
        .collect();
    let mut reachable_messages: BTreeSet<&str> = plan.cut.iter().map(String::as_str).collect();
    let mut reachable_oneofs: BTreeSet<&str> = BTreeSet::new();
    loop {
        let previous = reachable_messages.len() + reachable_oneofs.len();
        for leaf in reachable_messages.clone() {
            let fields = fields_of.get(leaf).unwrap_or_else(|| {
                panic!("schema: Debug-reachable message `{leaf}` has no fields")
            });
            for field in *fields {
                match &field.shape {
                    Shape::Message { leaf } | Shape::RepeatedMessage { leaf } => {
                        if !extern_set.contains(leaf.as_str()) {
                            reachable_messages.insert(leaf);
                        }
                    }
                    Shape::Map { value_leaf, .. } => {
                        if !extern_set.contains(value_leaf.as_str()) {
                            reachable_messages.insert(value_leaf);
                        }
                    }
                    Shape::Oneof => {
                        let key = format!("{leaf}::{}", field.rust_name);
                        let oneof = oneof_by_field.get(&key).unwrap_or_else(|| {
                            panic!("schema: Debug-reachable field `{key}` has no oneof")
                        });
                        reachable_oneofs.insert(oneof.rust_ident.as_str());
                        for variant in &oneof.variants {
                            if let Some(child) = &variant.message_leaf {
                                if !extern_set.contains(child.as_str()) {
                                    reachable_messages.insert(child);
                                }
                            }
                        }
                    }
                    Shape::Scalar(_)
                    | Shape::EmptyBytes
                    | Shape::RepeatedString
                    | Shape::RepeatedBytes => {}
                }
            }
        }
        if reachable_messages.len() + reachable_oneofs.len() == previous {
            break;
        }
    }

    let mut sequence_leaves = BTreeSet::new();
    for (index, fields) in resolved {
        if !reachable_messages.contains(messages[*index].leaf_name()) {
            continue;
        }
        for field in fields {
            if let Shape::RepeatedMessage { leaf } = &field.shape {
                if !extern_set.contains(leaf.as_str()) {
                    sequence_leaves.insert(leaf.as_str());
                }
            }
        }
    }

    src.push_str(
        "// ===========================================================================\n\
         // §F  The `Debug` PDA\n\
         // ===========================================================================\n\
         \n\
         #[derive(Clone, Copy)]\n\
         enum DebugNode<'a> {\n",
    );
    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        writeln!(src, "    {}(&'a {}),", ord_message_arm(leaf), path_of[leaf]).expect("write");
    }
    for oneof in oneofs {
        if reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            writeln!(
                src,
                "    {}(&'a crate::rhoapi::{}::{}),",
                ord_oneof_arm(oneof),
                oneof.module,
                oneof.rust_ident
            )
            .expect("write");
        }
    }
    src.push_str("}\n\n#[derive(Clone, Copy)]\nenum DebugSeq<'a> {\n");
    for leaf in &sequence_leaves {
        writeln!(
            src,
            "    {}(&'a [{}]),",
            rust_type_name(leaf),
            path_of[leaf]
        )
        .expect("write");
    }
    src.push_str(
        "}\n\n\
         impl<'a> DebugSeq<'a> {\n\
             fn len(self) -> usize {\n\
                 match self {\n",
    );
    for leaf in &sequence_leaves {
        writeln!(
            src,
            "            DebugSeq::{}(values) => values.len(),",
            rust_type_name(leaf)
        )
        .expect("write");
    }
    src.push_str("        }\n    }\n\n    fn get(self, index: usize) -> DebugNode<'a> {\n        match self {\n");
    for leaf in &sequence_leaves {
        writeln!(
            src,
            "            DebugSeq::{ty}(values) => DebugNode::{arm}(&values[index]),",
            ty = rust_type_name(leaf),
            arm = ord_message_arm(leaf)
        )
        .expect("write");
    }
    src.push_str(
        "        }\n    }\n}\n\n\
         enum DebugOp<'a> {\n\
             Node(DebugNode<'a>, usize),\n\
             Seq(DebugSeq<'a>, usize),\n\
             External(&'a crate::rust::rhoapi_ext::EPathMap),\n\
             // Schema-complete support for a future `repeated EPathMap`; the\n\
             // current descriptor has no instance of that shape.\n\
             #[allow(dead_code)]\n\
             ExternalSeq(&'a [crate::rust::rhoapi_ext::EPathMap], usize),\n\
             Map(&'a std::collections::BTreeMap<String, Par>, usize),\n\
             Bytes(&'a [u8], usize),\n\
             ByteSeq(&'a [Vec<u8>], usize),\n\
             Strings(&'a [String], usize),\n\
             Scalar(&'a dyn std::fmt::Debug),\n\
             Text(&'static str),\n\
             Indent(usize),\n\
         }\n\n\
         fn debug_format(root: DebugNode<'_>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n\
             use std::fmt::Write as _;\n\
             let pretty = f.alternate();\n\
             let mut ops = Vec::with_capacity(128);\n\
             ops.push(DebugOp::Node(root, 0));\n\
             while let Some(op) = ops.pop() {\n\
                 match op {\n\
                     DebugOp::Node(node, depth) => debug_push_node(node, depth, pretty, &mut ops),\n\
                     DebugOp::Seq(values, depth) => debug_push_seq(values, depth, pretty, &mut ops),\n\
                     DebugOp::External(value) => std::fmt::Debug::fmt(value, f)?,\n\
                     DebugOp::ExternalSeq(values, depth) => debug_push_external_seq(values, depth, pretty, &mut ops),\n\
                     DebugOp::Map(values, depth) => debug_push_map(values, depth, pretty, &mut ops),\n\
                     DebugOp::Bytes(values, depth) => debug_push_bytes(values, depth, pretty, &mut ops),\n\
                     DebugOp::ByteSeq(values, depth) => debug_push_byte_seq(values, depth, pretty, &mut ops),\n\
                     DebugOp::Strings(values, depth) => debug_push_strings(values, depth, pretty, &mut ops),\n\
                     DebugOp::Scalar(value) => std::fmt::Debug::fmt(value, f)?,\n\
                     DebugOp::Text(value) => f.write_str(value)?,\n\
                     DebugOp::Indent(depth) => { for _ in 0..depth { f.write_str(\"    \")?; } }\n\
                 }\n\
             }\n\
             Ok(())\n\
         }\n\n\
         fn debug_push_node<'a>(node: DebugNode<'a>, depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {\n\
             match node {\n",
    );
    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        writeln!(
            src,
            "        DebugNode::{arm}(value) => debug_push_{stem}(value, depth, pretty, ops),",
            arm = ord_message_arm(leaf),
            stem = rust_type_name(leaf).to_snake_case()
        )
        .expect("write");
    }
    for oneof in oneofs {
        if reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            writeln!(
                src,
                "        DebugNode::{arm}(value) => debug_push_oneof_{stem}(value, depth, pretty, ops),",
                arm = ord_oneof_arm(oneof),
                stem = oneof.rust_ident.to_snake_case()
            )
            .expect("write");
        }
    }
    src.push_str(
        "    }\n}\n\n\
         fn debug_push_option_node<'a>(value: DebugNode<'a>, depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {\n\
             if pretty {\n\
                 ops.push(DebugOp::Text(\")\")); ops.push(DebugOp::Indent(depth));\n\
                 ops.push(DebugOp::Text(\",\\n\")); ops.push(DebugOp::Node(value, depth + 1));\n\
                 ops.push(DebugOp::Indent(depth + 1)); ops.push(DebugOp::Text(\"Some(\\n\"));\n\
             } else {\n\
                 ops.push(DebugOp::Text(\")\")); ops.push(DebugOp::Node(value, depth));\n\
                 ops.push(DebugOp::Text(\"Some(\"));\n\
             }\n\
         }\n\n\
         fn debug_push_option_external<'a>(value: &'a crate::rust::rhoapi_ext::EPathMap, depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {\n\
             if pretty {\n\
                 ops.push(DebugOp::Text(\")\")); ops.push(DebugOp::Indent(depth));\n\
                 ops.push(DebugOp::Text(\",\\n\")); ops.push(DebugOp::External(value));\n\
                 ops.push(DebugOp::Indent(depth + 1)); ops.push(DebugOp::Text(\"Some(\\n\"));\n\
             } else {\n\
                 ops.push(DebugOp::Text(\")\")); ops.push(DebugOp::External(value));\n\
                 ops.push(DebugOp::Text(\"Some(\"));\n\
             }\n\
         }\n\n",
    );
    emit_debug_container_helpers(src);

    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        emit_debug_message_push(
            src,
            leaf,
            path_of[leaf].as_str(),
            fields_of[leaf],
            oneof_by_field,
            extern_set,
        );
    }
    for oneof in oneofs {
        if reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            emit_debug_oneof_push(src, oneof, extern_set);
        }
    }
    for leaf in &plan.cut {
        let rust_path = &path_of[leaf.as_str()];
        writeln!(
            src,
            "impl std::fmt::Debug for {rust_path} {{\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n        debug_format(DebugNode::{arm}(self), f)\n    }}\n}}\n",
            arm = ord_message_arm(leaf)
        )
        .expect("write");
    }

    emit_debug_oracle(
        src,
        messages,
        oneofs,
        extern_set,
        plan,
        oneof_by_field,
        &fields_of,
        &reachable_messages,
        &reachable_oneofs,
        path_of,
    );
}

fn emit_debug_container_helpers(src: &mut String) {
    src.push_str(
        "fn debug_push_seq<'a>(values: DebugSeq<'a>, depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {\n\
             let len = values.len();\n\
             if len == 0 { ops.push(DebugOp::Text(\"[]\")); return; }\n\
             ops.push(DebugOp::Text(\"]\"));\n\
             if pretty {\n\
                 ops.push(DebugOp::Indent(depth));\n\
                 for index in (0..len).rev() {\n\
                     ops.push(DebugOp::Text(\",\\n\")); ops.push(DebugOp::Node(values.get(index), depth + 1)); ops.push(DebugOp::Indent(depth + 1));\n\
                 }\n\
                 ops.push(DebugOp::Text(\"[\\n\"));\n\
             } else {\n\
                 for index in (0..len).rev() {\n\
                     ops.push(DebugOp::Node(values.get(index), depth)); ops.push(DebugOp::Text(if index == 0 { \"[\" } else { \", \" }));\n\
                 }\n\
             }\n\
         }\n\n\
         fn debug_push_bytes<'a>(values: &'a [u8], depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {\n\
             if values.is_empty() { ops.push(DebugOp::Text(\"[]\")); return; }\n\
             ops.push(DebugOp::Text(\"]\"));\n\
             if pretty {\n\
                 ops.push(DebugOp::Indent(depth));\n\
                 for value in values.iter().rev() { ops.push(DebugOp::Text(\",\\n\")); ops.push(DebugOp::Scalar(value)); ops.push(DebugOp::Indent(depth + 1)); }\n\
                 ops.push(DebugOp::Text(\"[\\n\"));\n\
             } else {\n\
                 for (index, value) in values.iter().enumerate().rev() { ops.push(DebugOp::Scalar(value)); ops.push(DebugOp::Text(if index == 0 { \"[\" } else { \", \" })); }\n\
             }\n\
         }\n\n\
         fn debug_push_strings<'a>(values: &'a [String], depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {\n\
             if values.is_empty() { ops.push(DebugOp::Text(\"[]\")); return; }\n\
             ops.push(DebugOp::Text(\"]\"));\n\
             if pretty { ops.push(DebugOp::Indent(depth)); for value in values.iter().rev() { ops.push(DebugOp::Text(\",\\n\")); ops.push(DebugOp::Scalar(value)); ops.push(DebugOp::Indent(depth + 1)); } ops.push(DebugOp::Text(\"[\\n\")); }\n\
             else { for (index, value) in values.iter().enumerate().rev() { ops.push(DebugOp::Scalar(value)); ops.push(DebugOp::Text(if index == 0 { \"[\" } else { \", \" })); } }\n\
         }\n\n\
         fn debug_push_byte_seq<'a>(values: &'a [Vec<u8>], depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {\n\
             if values.is_empty() { ops.push(DebugOp::Text(\"[]\")); return; }\n\
             ops.push(DebugOp::Text(\"]\"));\n\
             if pretty { ops.push(DebugOp::Indent(depth)); for value in values.iter().rev() { ops.push(DebugOp::Text(\",\\n\")); ops.push(DebugOp::Bytes(value, depth + 1)); ops.push(DebugOp::Indent(depth + 1)); } ops.push(DebugOp::Text(\"[\\n\")); }\n\
             else { for (index, value) in values.iter().enumerate().rev() { ops.push(DebugOp::Bytes(value, depth)); ops.push(DebugOp::Text(if index == 0 { \"[\" } else { \", \" })); } }\n\
         }\n\n\
         fn debug_push_external_seq<'a>(values: &'a [crate::rust::rhoapi_ext::EPathMap], depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {\n\
             if values.is_empty() { ops.push(DebugOp::Text(\"[]\")); return; }\n\
             ops.push(DebugOp::Text(\"]\"));\n\
             if pretty { ops.push(DebugOp::Indent(depth)); for value in values.iter().rev() { ops.push(DebugOp::Text(\",\\n\")); ops.push(DebugOp::External(value)); ops.push(DebugOp::Indent(depth + 1)); } ops.push(DebugOp::Text(\"[\\n\")); }\n\
             else { for (index, value) in values.iter().enumerate().rev() { ops.push(DebugOp::External(value)); ops.push(DebugOp::Text(if index == 0 { \"[\" } else { \", \" })); } }\n\
         }\n\n\
         fn debug_push_map<'a>(values: &'a std::collections::BTreeMap<String, Par>, depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {\n\
             if values.is_empty() { ops.push(DebugOp::Text(\"{}\")); return; }\n\
             ops.push(DebugOp::Text(\"}\"));\n\
             if pretty { ops.push(DebugOp::Indent(depth)); for (key, value) in values.iter().rev() { ops.push(DebugOp::Text(\",\\n\")); ops.push(DebugOp::Node(DebugNode::MPar(value), depth + 1)); ops.push(DebugOp::Text(\": \")); ops.push(DebugOp::Scalar(key)); ops.push(DebugOp::Indent(depth + 1)); } ops.push(DebugOp::Text(\"{\\n\")); }\n\
             else { for (index, (key, value)) in values.iter().enumerate().rev() { ops.push(DebugOp::Node(DebugNode::MPar(value), depth)); ops.push(DebugOp::Text(\": \")); ops.push(DebugOp::Scalar(key)); ops.push(DebugOp::Text(if index == 0 { \"{\" } else { \", \" })); } }\n\
         }\n\n",
    );
}

fn emit_debug_message_push(
    src: &mut String,
    leaf: &str,
    rust_path: &str,
    fields: &[Field],
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
) {
    let stem = rust_type_name(leaf).to_snake_case();
    let value_name = if fields.is_empty() { "_value" } else { "value" };
    let depth_name = if fields.is_empty() { "_depth" } else { "depth" };
    let pretty_name = if fields.is_empty() {
        "_pretty"
    } else {
        "pretty"
    };
    writeln!(src, "fn debug_push_{stem}<'a>({value_name}: &'a {rust_path}, {depth_name}: usize, {pretty_name}: bool, ops: &mut Vec<DebugOp<'a>>) {{").expect("write");
    if fields.is_empty() {
        writeln!(
            src,
            "    ops.push(DebugOp::Text({:?}));\n}}\n",
            rust_type_name(leaf)
        )
        .expect("write");
        return;
    }
    src.push_str("    if pretty {\n        ops.push(DebugOp::Text(\"}\"));\n        ops.push(DebugOp::Indent(depth));\n");
    for field in fields.iter().rev() {
        src.push_str("        ops.push(DebugOp::Text(\",\\n\"));\n");
        emit_debug_field_value(src, leaf, field, "depth + 1", oneof_by_field, extern_set, 8);
        writeln!(src, "        ops.push(DebugOp::Text(\": \"));\n        ops.push(DebugOp::Text({:?}));\n        ops.push(DebugOp::Indent(depth + 1));", field.rust_name).expect("write");
    }
    writeln!(src, "        ops.push(DebugOp::Text(\" {{\\n\"));\n        ops.push(DebugOp::Text({:?}));\n    }} else {{\n        ops.push(DebugOp::Text(\" }}\"));", rust_type_name(leaf)).expect("write");
    for (index, field) in fields.iter().enumerate().rev() {
        emit_debug_field_value(src, leaf, field, "depth", oneof_by_field, extern_set, 8);
        writeln!(src, "        ops.push(DebugOp::Text(\": \"));\n        ops.push(DebugOp::Text({:?}));\n        ops.push(DebugOp::Text({:?}));", field.rust_name, if index == 0 { " { " } else { ", " }).expect("write");
    }
    writeln!(
        src,
        "        ops.push(DebugOp::Text({:?}));\n    }}\n}}\n",
        rust_type_name(leaf)
    )
    .expect("write");
}

fn emit_debug_field_value(
    src: &mut String,
    owner: &str,
    field: &Field,
    depth: &str,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
    indent: usize,
) {
    let pad = " ".repeat(indent);
    let name = &field.rust_name;
    match &field.shape {
        Shape::Scalar(Type::Bytes) | Shape::EmptyBytes => writeln!(src, "{pad}ops.push(DebugOp::Bytes(&value.{name}, {depth}));").expect("write"),
        Shape::Scalar(_) => writeln!(src, "{pad}ops.push(DebugOp::Scalar(&value.{name}));").expect("write"),
        Shape::RepeatedString => writeln!(src, "{pad}ops.push(DebugOp::Strings(&value.{name}, {depth}));").expect("write"),
        Shape::RepeatedBytes => writeln!(src, "{pad}ops.push(DebugOp::ByteSeq(&value.{name}, {depth}));").expect("write"),
        Shape::Message { leaf } if extern_set.contains(leaf.as_str()) => {
            assert_eq!(leaf, "EPathMap");
            writeln!(src, "{pad}match &value.{name} {{ Some(child) => debug_push_option_external(child, {depth}, pretty, ops), None => ops.push(DebugOp::Text(\"None\")), }}").expect("write");
        }
        Shape::Message { leaf } => writeln!(src, "{pad}match &value.{name} {{ Some(child) => debug_push_option_node(DebugNode::{}(child), {depth}, pretty, ops), None => ops.push(DebugOp::Text(\"None\")), }}", ord_message_arm(leaf)).expect("write"),
        Shape::RepeatedMessage { leaf } if extern_set.contains(leaf.as_str()) => {
            assert_eq!(leaf, "EPathMap");
            writeln!(src, "{pad}ops.push(DebugOp::ExternalSeq(&value.{name}, {depth}));").expect("write");
        }
        Shape::RepeatedMessage { leaf } => writeln!(src, "{pad}ops.push(DebugOp::Seq(DebugSeq::{}(&value.{name}), {depth}));", rust_type_name(leaf)).expect("write"),
        Shape::Map { value_leaf, .. } => {
            assert_eq!(value_leaf, "Par", "schema: Debug map driver only models map<string, Par>");
            writeln!(src, "{pad}ops.push(DebugOp::Map(&value.{name}, {depth}));").expect("write");
        }
        Shape::Oneof => {
            let key = format!("{owner}::{name}");
            let oneof = oneof_by_field.get(&key).unwrap_or_else(|| panic!("schema: Debug field `{key}` has no oneof"));
            writeln!(src, "{pad}match &value.{name} {{ Some(child) => debug_push_option_node(DebugNode::{}(child), {depth}, pretty, ops), None => ops.push(DebugOp::Text(\"None\")), }}", ord_oneof_arm(oneof)).expect("write");
        }
    }
}

fn emit_debug_oneof_push(src: &mut String, oneof: &Oneof, extern_set: &BTreeSet<&str>) {
    let ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
    let stem = oneof.rust_ident.to_snake_case();
    writeln!(src, "fn debug_push_oneof_{stem}<'a>(value: &'a {ty}, depth: usize, pretty: bool, ops: &mut Vec<DebugOp<'a>>) {{\n    match value {{").expect("write");
    for variant in &oneof.variants {
        let arm = &variant.rust_ident;
        writeln!(src, "        {ty}::{arm}(payload) => {{\n            ops.push(DebugOp::Text(\")\"));\n            if pretty {{\n                ops.push(DebugOp::Indent(depth));\n                ops.push(DebugOp::Text(\",\\n\"));").expect("write");
        emit_debug_oneof_payload(src, variant, extern_set, "depth + 1", 16);
        writeln!(src, "                ops.push(DebugOp::Indent(depth + 1));\n                ops.push(DebugOp::Text({:?}));\n            }} else {{", format!("{arm}(\n")).expect("write");
        emit_debug_oneof_payload(src, variant, extern_set, "depth", 16);
        writeln!(
            src,
            "                ops.push(DebugOp::Text({:?}));\n            }}\n        }}",
            format!("{arm}(")
        )
        .expect("write");
    }
    src.push_str("    }\n}\n\n");
}

fn emit_debug_oneof_payload(
    src: &mut String,
    variant: &Variant,
    extern_set: &BTreeSet<&str>,
    depth: &str,
    indent: usize,
) {
    let pad = " ".repeat(indent);
    match &variant.message_leaf {
        Some(leaf) if extern_set.contains(leaf.as_str()) => {
            assert_eq!(leaf, "EPathMap");
            writeln!(src, "{pad}ops.push(DebugOp::External(payload));").expect("write");
        }
        Some(leaf) => writeln!(
            src,
            "{pad}ops.push(DebugOp::Node(DebugNode::{}(payload), {depth}));",
            ord_message_arm(leaf)
        )
        .expect("write"),
        None if variant.ty == Type::Bytes => {
            writeln!(src, "{pad}ops.push(DebugOp::Bytes(payload, {depth}));").expect("write")
        }
        None => writeln!(src, "{pad}ops.push(DebugOp::Scalar(payload));").expect("write"),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_debug_oracle(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    fields_of: &BTreeMap<&str, &[Field]>,
    reachable_messages: &BTreeSet<&str>,
    reachable_oneofs: &BTreeSet<&str>,
    path_of: &BTreeMap<&str, String>,
) {
    src.push_str(
        "#[cfg(test)]\n\
         struct OracleDebug<'a>(DebugNode<'a>);\n\n\
         #[cfg(test)]\n\
         impl std::fmt::Debug for OracleDebug<'_> {\n\
             fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n\
                 match self.0 {\n",
    );
    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        writeln!(
            src,
            "            DebugNode::{arm}(value) => oracle_debug_{stem}(value, f),",
            arm = ord_message_arm(leaf),
            stem = rust_type_name(leaf).to_snake_case()
        )
        .expect("write");
    }
    for oneof in oneofs {
        if reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            writeln!(
                src,
                "            DebugNode::{arm}(value) => oracle_debug_oneof_{stem}(value, f),",
                arm = ord_oneof_arm(oneof),
                stem = oneof.rust_ident.to_snake_case()
            )
            .expect("write");
        }
    }
    src.push_str(
        "        }\n    }\n}\n\n\
         #[cfg(test)]\n\
         struct OracleOption<'a>(Option<DebugNode<'a>>);\n\n\
         #[cfg(test)]\n\
         impl std::fmt::Debug for OracleOption<'_> {\n\
             fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n\
                 match self.0 {\n\
                     None => f.write_str(\"None\"),\n\
                     Some(value) => f.debug_tuple(\"Some\").field(&OracleDebug(value)).finish(),\n\
                 }\n\
             }\n\
         }\n\n\
         #[cfg(test)]\n\
         struct OracleSeq<'a>(DebugSeq<'a>);\n\n\
         #[cfg(test)]\n\
         impl std::fmt::Debug for OracleSeq<'_> {\n\
             fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n\
                 let mut list = f.debug_list();\n\
                 for index in 0..self.0.len() { list.entry(&OracleDebug(self.0.get(index))); }\n\
                 list.finish()\n\
             }\n\
         }\n\n\
         #[cfg(test)]\n\
         struct OracleMap<'a>(&'a std::collections::BTreeMap<String, Par>);\n\n\
         #[cfg(test)]\n\
         impl std::fmt::Debug for OracleMap<'_> {\n\
             fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n\
                 let mut map = f.debug_map();\n\
                 for (key, value) in self.0 { map.entry(key, &OracleDebug(DebugNode::MPar(value))); }\n\
                 map.finish()\n\
             }\n\
         }\n\n",
    );

    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        let fields = fields_of[leaf];
        let stem = rust_type_name(leaf).to_snake_case();
        let value_name = if fields.is_empty() { "_value" } else { "value" };
        writeln!(
            src,
            "#[cfg(test)]\nfn oracle_debug_{stem}({value_name}: &{}, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{",
            path_of[leaf]
        )
        .expect("write");
        if fields.is_empty() {
            writeln!(
                src,
                "    f.debug_struct({:?}).finish()\n}}\n",
                rust_type_name(leaf)
            )
            .expect("write");
            continue;
        }
        writeln!(
            src,
            "    let mut builder = f.debug_struct({:?});",
            rust_type_name(leaf)
        )
        .expect("write");
        for field in fields {
            emit_debug_oracle_field(src, leaf, field, oneof_by_field, extern_set);
        }
        src.push_str("    builder.finish()\n}\n\n");
    }

    for oneof in oneofs {
        if !reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            continue;
        }
        let ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
        let stem = oneof.rust_ident.to_snake_case();
        writeln!(
            src,
            "#[cfg(test)]\nfn oracle_debug_oneof_{stem}(value: &{ty}, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n    match value {{"
        )
        .expect("write");
        for variant in &oneof.variants {
            let arm = &variant.rust_ident;
            writeln!(src, "        {ty}::{arm}(payload) => {{").expect("write");
            match &variant.message_leaf {
                Some(leaf) if extern_set.contains(leaf.as_str()) => {
                    assert_eq!(leaf, "EPathMap");
                    writeln!(
                        src,
                        "            f.debug_tuple({arm:?}).field(payload).finish()"
                    )
                    .expect("write");
                }
                Some(leaf) => {
                    writeln!(src, "            f.debug_tuple({arm:?}).field(&OracleDebug(DebugNode::{}(payload))).finish()", ord_message_arm(leaf)).expect("write");
                }
                None => {
                    writeln!(
                        src,
                        "            f.debug_tuple({arm:?}).field(payload).finish()"
                    )
                    .expect("write");
                }
            }
            src.push_str("        }\n");
        }
        src.push_str("    }\n}\n\n");
    }

    assert_eq!(
        plan.cut.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["Par"],
        "schema: the generated Debug differential currently names Par as its root; \
         widen the generated test roots when the descriptor-derived cut set changes"
    );
    src.push_str(
        "#[cfg(test)]\n\
         mod debug_pda_differential {\n\
             use super::*;\n\
             use proptest::strategy::{Strategy, ValueTree};\n\
             use proptest::test_runner::TestRunner;\n\n\
             #[test]\n\
             fn generated_debug_matches_the_recursive_builder_oracle() {\n\
                 let strategy = crate::rust::test_utils::test_utils::generate_par(4);\n\
                 let mut runner = TestRunner::deterministic();\n\
                 for _ in 0..48 {\n\
                     let value = strategy.new_tree(&mut runner).expect(\"Debug differential strategy produces a value\").current();\n\
                     let oracle = OracleDebug(DebugNode::MPar(&value));\n\
                     let actual = format!(\"{:?}\", value); let expected = format!(\"{:?}\", oracle);\n\
                     if actual != expected {\n\
                         let at = actual.bytes().zip(expected.bytes()).position(|(a, b)| a != b).unwrap_or(actual.len().min(expected.len()));\n\
                         let actual_tail: String = actual[at..].chars().take(120).collect();\n\
                         let expected_tail: String = expected[at..].chars().take(120).collect();\n\
                         panic!(\"compact Debug differs at byte {at}; actual={actual_tail:?}; expected={expected_tail:?}\");\n\
                     }\n\
                     let actual = format!(\"{:#?}\", value); let expected = format!(\"{:#?}\", oracle);\n\
                     if actual != expected {\n\
                         let at = actual.bytes().zip(expected.bytes()).position(|(a, b)| a != b).unwrap_or(actual.len().min(expected.len()));\n\
                         let actual_tail: String = actual[at..].chars().take(120).collect();\n\
                         let expected_tail: String = expected[at..].chars().take(120).collect();\n\
                         panic!(\"alternate Debug differs at byte {at}; actual={actual_tail:?}; expected={expected_tail:?}\");\n\
                     }\n\
                 }\n\
             }\n\n\
             #[test]\n\
             fn generated_debug_is_stack_safe_at_depth_4096() {\n\
                 std::thread::Builder::new().stack_size(256 * 1024).spawn(|| {\n\
                     let mut value = Par::default();\n\
                     for _ in 0..4096 {\n\
                         let mut list = EList::default(); list.ps.push(value);\n\
                         let mut parent = Par::default(); parent.exprs.push(Expr { expr_instance: Some(ExprInstance::EListBody(list)) });\n\
                         value = parent;\n\
                     }\n\
                     let rendered = format!(\"{:?}\", value);\n\
                     assert!(rendered.starts_with(\"Par { sends: []\"));\n\
                     crate::rust::rholang::par_children::dismantle(value);\n\
                 }).expect(\"spawn Debug depth test\").join().expect(\"Debug depth test panicked\");\n\
             }\n\
         }\n\n",
    );
}

fn emit_debug_oracle_field(
    src: &mut String,
    owner: &str,
    field: &Field,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
) {
    let name = &field.rust_name;
    let binding = format!("oracle_field_{}", name.trim_start_matches("r#"));
    match &field.shape {
        Shape::Message { leaf } if !extern_set.contains(leaf.as_str()) => {
            writeln!(src, "    let {binding} = OracleOption(value.{name}.as_ref().map(|child| DebugNode::{}(child)));", ord_message_arm(leaf)).expect("write");
            writeln!(src, "    builder.field({name:?}, &{binding});").expect("write");
        }
        Shape::RepeatedMessage { leaf } if !extern_set.contains(leaf.as_str()) => {
            writeln!(
                src,
                "    let {binding} = OracleSeq(DebugSeq::{}(&value.{name}));",
                rust_type_name(leaf)
            )
            .expect("write");
            writeln!(src, "    builder.field({name:?}, &{binding});").expect("write");
        }
        Shape::Map { value_leaf, .. } => {
            assert_eq!(value_leaf, "Par");
            writeln!(src, "    let {binding} = OracleMap(&value.{name});\n    builder.field({name:?}, &{binding});").expect("write");
        }
        Shape::Oneof => {
            let key = format!("{owner}::{name}");
            let oneof = oneof_by_field
                .get(&key)
                .unwrap_or_else(|| panic!("schema: Debug oracle field `{key}` has no oneof"));
            writeln!(src, "    let {binding} = OracleOption(value.{name}.as_ref().map(|child| DebugNode::{}(child)));", ord_oneof_arm(oneof)).expect("write");
            writeln!(src, "    builder.field({name:?}, &{binding});").expect("write");
        }
        Shape::Scalar(_)
        | Shape::EmptyBytes
        | Shape::RepeatedString
        | Shape::RepeatedBytes
        | Shape::Message { .. }
        | Shape::RepeatedMessage { .. } => {
            writeln!(src, "    builder.field({name:?}, &value.{name});").expect("write");
        }
    }
}

fn emit_ord_driver(
    src: &mut String,
    messages: &[Message<'_>],
    resolved: &[(usize, Vec<Field>)],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    path_of: &BTreeMap<&str, String>,
) {
    let fields_of: BTreeMap<&str, &[Field]> = resolved
        .iter()
        .map(|(i, fields)| (messages[*i].leaf_name(), fields.as_slice()))
        .collect();
    let mut reachable_messages: BTreeSet<&str> = plan.cut.iter().map(String::as_str).collect();
    let mut reachable_oneofs: BTreeSet<&str> = BTreeSet::new();
    loop {
        let previous = reachable_messages.len() + reachable_oneofs.len();
        for leaf in reachable_messages.clone() {
            let fields = fields_of
                .get(leaf)
                .unwrap_or_else(|| panic!("schema: Ord-reachable message `{leaf}` has no fields"));
            for field in *fields {
                match &field.shape {
                    Shape::Message { leaf } | Shape::RepeatedMessage { leaf } => {
                        if !extern_set.contains(leaf.as_str()) {
                            reachable_messages.insert(leaf);
                        }
                    }
                    Shape::Map { value_leaf, .. } => {
                        if !extern_set.contains(value_leaf.as_str()) {
                            reachable_messages.insert(value_leaf);
                        }
                    }
                    Shape::Oneof => {
                        let key = format!("{leaf}::{}", field.rust_name);
                        let oneof = oneof_by_field.get(&key).unwrap_or_else(|| {
                            panic!("schema: Ord-reachable field `{key}` has no oneof")
                        });
                        reachable_oneofs.insert(oneof.rust_ident.as_str());
                        for variant in &oneof.variants {
                            if let Some(child) = &variant.message_leaf {
                                if !extern_set.contains(child.as_str()) {
                                    reachable_messages.insert(child);
                                }
                            }
                        }
                    }
                    Shape::Scalar(_)
                    | Shape::EmptyBytes
                    | Shape::RepeatedString
                    | Shape::RepeatedBytes => {}
                }
            }
        }
        if reachable_messages.len() + reachable_oneofs.len() == previous {
            break;
        }
    }
    let mut sequence_leaves = BTreeSet::new();
    let mut has_external_sequence = false;
    for (index, fields) in resolved {
        if !reachable_messages.contains(messages[*index].leaf_name()) {
            continue;
        }
        for field in fields {
            if let Shape::RepeatedMessage { leaf } = &field.shape {
                if extern_set.contains(leaf.as_str()) {
                    has_external_sequence = true;
                } else {
                    sequence_leaves.insert(leaf.as_str());
                }
            }
        }
    }

    src.push_str(
        "// ===========================================================================\n\
         // §F  The `Ord` / `PartialOrd` PDA\n\
         // ===========================================================================\n\
         \n\
         #[derive(Clone, Copy)]\n\
         enum TermRef<'a> {\n",
    );
    let mut node_arms = BTreeSet::new();
    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        let arm = ord_message_arm(leaf);
        assert!(
            node_arms.insert(arm.clone()),
            "schema: duplicate TermRef arm `{arm}`"
        );
        writeln!(src, "    {arm}(&'a {}),", path_of[leaf]).expect("write");
    }
    for oneof in oneofs {
        if !reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            continue;
        }
        let arm = ord_oneof_arm(oneof);
        assert!(
            node_arms.insert(arm.clone()),
            "schema: duplicate TermRef arm `{arm}`"
        );
        writeln!(
            src,
            "    {arm}(&'a crate::rhoapi::{}::{}),",
            oneof.module, oneof.rust_ident
        )
        .expect("write");
    }
    src.push_str(
        "    EPathMap(&'a crate::rust::rhoapi_ext::EPathMap),\n\
         \x20   EPathMapRepr(&'a crate::rust::epathmap_trie_codec::EPathMapRepr<Par>),\n\
         }\n\n\
         #[derive(Clone, Copy)]\n\
         enum TermSlice<'a> {\n",
    );
    for leaf in &sequence_leaves {
        writeln!(
            src,
            "    {}(&'a [{}]),",
            rust_type_name(leaf),
            path_of[leaf]
        )
        .expect("write");
    }
    if has_external_sequence {
        src.push_str("    EPathMap(&'a [crate::rust::rhoapi_ext::EPathMap]),\n");
    }
    src.push_str(
        "}\n\n\
         impl<'a> TermSlice<'a> {\n\
             #[inline]\n\
             fn len(self) -> usize {\n\
                 match self {\n",
    );
    for leaf in &sequence_leaves {
        writeln!(
            src,
            "            TermSlice::{}(values) => values.len(),",
            rust_type_name(leaf)
        )
        .expect("write");
    }
    if has_external_sequence {
        src.push_str("            TermSlice::EPathMap(values) => values.len(),\n");
    }
    src.push_str(
        "        }\n\
             }\n\n\
             #[inline]\n\
             fn get(self, index: usize) -> TermRef<'a> {\n\
                 match self {\n",
    );
    for leaf in &sequence_leaves {
        writeln!(
            src,
            "            TermSlice::{ty}(values) => TermRef::{arm}(&values[index]),",
            ty = rust_type_name(leaf),
            arm = ord_message_arm(leaf)
        )
        .expect("write");
    }
    if has_external_sequence {
        src.push_str(
            "            TermSlice::EPathMap(values) => TermRef::EPathMap(&values[index]),\n",
        );
    }
    src.push_str(
        "        }\n\
             }\n\
         }\n\n\
         enum OrdOp<'a> {\n\
             Node { left: TermRef<'a>, right: TermRef<'a>, from: u16 },\n\
             Seq { left: TermSlice<'a>, right: TermSlice<'a>, index: usize },\n\
             Map,\n\
             EPathMapEntries,\n\
         }\n\n\
         type OrdMapIter<'a> = std::collections::btree_map::Iter<'a, String, Par>;\n\n\
         enum OrdEPathMapIter<'a> {\n\
             Set {\n\
                 left_paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, ()>,\n\
                 right_paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, ()>,\n\
                 left: pathmap::zipper::ReadZipperUntracked<'a, 'static, ()>,\n\
                 right: pathmap::zipper::ReadZipperUntracked<'a, 'static, ()>,\n\
                 paths_done: bool,\n\
             },\n\
             Map {\n\
                 left_paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, Par>,\n\
                 right_paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, Par>,\n\
                 left: pathmap::zipper::ReadZipperUntracked<'a, 'static, Par>,\n\
                 right: pathmap::zipper::ReadZipperUntracked<'a, 'static, Par>,\n\
                 paths_done: bool,\n\
             },\n\
         }\n\n\
         fn ord_compare<'a>(left: TermRef<'a>, right: TermRef<'a>) -> std::cmp::Ordering {\n\
             let mut ops = Vec::with_capacity(64);\n\
             let mut maps: Vec<(OrdMapIter<'a>, OrdMapIter<'a>)> = Vec::new();\n\
             let mut epathmaps: Vec<OrdEPathMapIter<'a>> = Vec::new();\n\
             ops.push(OrdOp::Node { left, right, from: 0 });\n\
             while let Some(op) = ops.pop() {\n\
                 let unequal = match op {\n\
                     OrdOp::Node { left, right, from } => {\n\
                         ord_step(left, right, from, &mut ops, &mut maps, &mut epathmaps)\n\
                     }\n\
                     OrdOp::Seq { left, right, index } => {\n\
                         let common = left.len().min(right.len());\n\
                         if index < common {\n\
                             ops.push(OrdOp::Seq { left, right, index: index + 1 });\n\
                             ops.push(OrdOp::Node {\n\
                                 left: left.get(index),\n\
                                 right: right.get(index),\n\
                                 from: 0,\n\
                             });\n\
                             None\n\
                         } else {\n\
                             let ordering = left.len().cmp(&right.len());\n\
                             (ordering != std::cmp::Ordering::Equal).then_some(ordering)\n\
                         }\n\
                     }\n\
                     OrdOp::Map => {\n\
                         let (left_iter, right_iter) = maps\n\
                             .last_mut()\n\
                             .expect(\"ord PDA: Map without a live iterator pair\");\n\
                         match (left_iter.next(), right_iter.next()) {\n\
                             (Some((left_key, left_value)), Some((right_key, right_value))) => {\n\
                                 let key_order = left_key.cmp(right_key);\n\
                                 if key_order != std::cmp::Ordering::Equal {\n\
                                     Some(key_order)\n\
                                 } else {\n\
                                     ops.push(OrdOp::Map);\n\
                                     ops.push(OrdOp::Node {\n\
                                         left: TermRef::MPar(left_value),\n\
                                         right: TermRef::MPar(right_value),\n\
                                         from: 0,\n\
                                     });\n\
                                     None\n\
                                 }\n\
                             }\n\
                             (None, None) => {\n\
                                 maps.pop();\n\
                                 None\n\
                             }\n\
                             (None, Some(_)) => Some(std::cmp::Ordering::Less),\n\
                             (Some(_), None) => Some(std::cmp::Ordering::Greater),\n\
                         }\n\
                     }\n\
                     OrdOp::EPathMapEntries => {\n\
                         ord_step_epath_map_entries(&mut ops, &mut epathmaps)\n\
                     }\n\
                 };\n\
                 if let Some(ordering) = unequal {\n\
                     return ordering;\n\
                 }\n\
             }\n\
             debug_assert!(maps.is_empty(), \"ord PDA left map iterators live\");\n\
             debug_assert!(epathmaps.is_empty(), \"ord PDA left EPathMap iterators live\");\n\
             std::cmp::Ordering::Equal\n\
         }\n\n\
         fn ord_step<'a>(\n\
             left: TermRef<'a>,\n\
             right: TermRef<'a>,\n\
             from: u16,\n\
             ops: &mut Vec<OrdOp<'a>>,\n\
             maps: &mut Vec<(OrdMapIter<'a>, OrdMapIter<'a>)>,\n\
             epathmaps: &mut Vec<OrdEPathMapIter<'a>>,\n\
         ) -> Option<std::cmp::Ordering> {\n\
             match (left, right) {\n",
    );
    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        let arm = ord_message_arm(leaf);
        let stem = rust_type_name(leaf).to_snake_case();
        writeln!(
            src,
            "        (TermRef::{arm}(left), TermRef::{arm}(right)) => \
             ord_step_{stem}(left, right, from, ops, maps),"
        )
        .expect("write");
    }
    for oneof in oneofs {
        if !reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            continue;
        }
        let arm = ord_oneof_arm(oneof);
        let stem = oneof.rust_ident.to_snake_case();
        writeln!(
            src,
            "        (TermRef::{arm}(left), TermRef::{arm}(right)) => \
             ord_step_oneof_{stem}(left, right, ops),"
        )
        .expect("write");
    }
    src.push_str(
        "        (TermRef::EPathMap(left), TermRef::EPathMap(right)) =>\n\
         \x20           ord_step_epath_map(left, right, from, ops),\n\
         \x20       (TermRef::EPathMapRepr(left), TermRef::EPathMapRepr(right)) =>\n\
         \x20           ord_open_epath_map_repr(left, right, ops, epathmaps),\n\
         \x20       _ => unreachable!(\"ord PDA compared different schema node types\"),\n\
             }\n\
         }\n\n",
    );

    emit_ord_epath_map_steps(src);

    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        emit_ord_message_step(
            src,
            leaf,
            path_of[leaf].as_str(),
            fields_of[leaf],
            oneof_by_field,
            extern_set,
        );
    }
    for oneof in oneofs {
        if !reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            continue;
        }
        emit_ord_oneof_step(src, oneof, extern_set);
    }

    for leaf in &plan.cut {
        assert!(
            !extern_set.contains(leaf.as_str()),
            "schema: Ord cut-set member `{leaf}` is extern and cannot receive a generated impl"
        );
        let rust_path = &path_of[leaf.as_str()];
        let arm = ord_message_arm(leaf);
        writeln!(
            src,
            "impl Ord for {rust_path} {{\n    \
             #[inline]\n    \
             fn cmp(&self, other: &Self) -> std::cmp::Ordering {{\n        \
             ord_compare(TermRef::{arm}(self), TermRef::{arm}(other))\n    \
             }}\n\
             }}\n\n\
             impl PartialOrd for {rust_path} {{\n    \
             #[inline]\n    \
             fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {{\n        \
             Some(self.cmp(other))\n    \
             }}\n\
             }}\n"
        )
        .expect("write");
    }

    src.push_str(
        "impl Ord for EPathMap {\n\
         \x20   #[inline]\n\
         \x20   fn cmp(&self, other: &Self) -> std::cmp::Ordering {\n\
         \x20       ord_compare(TermRef::EPathMap(self), TermRef::EPathMap(other))\n\
         \x20   }\n\
         }\n\n\
         impl PartialOrd for EPathMap {\n\
         \x20   #[inline]\n\
         \x20   fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }\n\
         }\n\n\
         impl Ord for crate::rust::rhoapi_ext::EntryTrie {\n\
         \x20   #[inline]\n\
         \x20   fn cmp(&self, other: &Self) -> std::cmp::Ordering {\n\
         \x20       ord_compare(TermRef::EPathMapRepr(self.representation()), TermRef::EPathMapRepr(other.representation()))\n\
         \x20   }\n\
         }\n\n\
         impl PartialOrd for crate::rust::rhoapi_ext::EntryTrie {\n\
         \x20   #[inline]\n\
         \x20   fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }\n\
         }\n\n",
    );

    emit_eq_hash_driver(
        src,
        messages,
        oneofs,
        extern_set,
        plan,
        oneof_by_field,
        &fields_of,
        &reachable_messages,
        &reachable_oneofs,
        path_of,
    );

    emit_ord_oracle(
        src,
        messages,
        oneofs,
        extern_set,
        plan,
        oneof_by_field,
        &fields_of,
        &reachable_messages,
        &reachable_oneofs,
        path_of,
    );
}

fn emit_ord_epath_map_steps(src: &mut String) {
    src.push_str(
        "fn ord_step_epath_map<'a>(\n\
         \x20   left: &'a EPathMap,\n\
         \x20   right: &'a EPathMap,\n\
         \x20   from: u16,\n\
         \x20   ops: &mut Vec<OrdOp<'a>>,\n\
         ) -> Option<std::cmp::Ordering> {\n\
         \x20   if from < 1 {\n\
         \x20       ops.push(OrdOp::Node { left: TermRef::EPathMap(left), right: TermRef::EPathMap(right), from: 1 });\n\
         \x20       ops.push(OrdOp::Node { left: TermRef::EPathMapRepr(left.representation()), right: TermRef::EPathMapRepr(right.representation()), from: 0 });\n\
         \x20       return None;\n\
         \x20   }\n\
         \x20   if from < 2 {\n\
         \x20       let ordering = left.locally_free.cmp(&right.locally_free);\n\
         \x20       if ordering != std::cmp::Ordering::Equal { return Some(ordering); }\n\
         \x20   }\n\
         \x20   if from < 3 {\n\
         \x20       let ordering = left.connective_used.cmp(&right.connective_used);\n\
         \x20       if ordering != std::cmp::Ordering::Equal { return Some(ordering); }\n\
         \x20   }\n\
         \x20   if from < 4 {\n\
         \x20       let ordering = left.remainder.cmp(&right.remainder);\n\
         \x20       if ordering != std::cmp::Ordering::Equal { return Some(ordering); }\n\
         \x20   }\n\
         \x20   None\n\
         }\n\n\
         fn ord_open_epath_map_repr<'a>(\n\
         \x20   left: &'a crate::rust::epathmap_trie_codec::EPathMapRepr<Par>,\n\
         \x20   right: &'a crate::rust::epathmap_trie_codec::EPathMapRepr<Par>,\n\
         \x20   ops: &mut Vec<OrdOp<'a>>,\n\
         \x20   epathmaps: &mut Vec<OrdEPathMapIter<'a>>,\n\
         ) -> Option<std::cmp::Ordering> {\n\
         \x20   use crate::rust::epathmap_trie_codec::EPathMapRepr;\n\
         \x20   let mode_order = (left.mode() as u8).cmp(&(right.mode() as u8));\n\
         \x20   if mode_order != std::cmp::Ordering::Equal { return Some(mode_order); }\n\
         \x20   match (left, right) {\n\
         \x20       (EPathMapRepr::Empty, EPathMapRepr::Empty) => None,\n\
         \x20       (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => {\n\
         \x20           epathmaps.push(OrdEPathMapIter::Set {\n\
         \x20               left_paths: left.read_zipper().into_path_iter(),\n\
         \x20               right_paths: right.read_zipper().into_path_iter(),\n\
         \x20               left: left.read_zipper(), right: right.read_zipper(), paths_done: false,\n\
         \x20           });\n\
         \x20           ops.push(OrdOp::EPathMapEntries);\n\
         \x20           None\n\
         \x20       }\n\
         \x20       (EPathMapRepr::Map(left), EPathMapRepr::Map(right)) => {\n\
         \x20           epathmaps.push(OrdEPathMapIter::Map {\n\
         \x20               left_paths: left.read_zipper().into_path_iter(),\n\
         \x20               right_paths: right.read_zipper().into_path_iter(),\n\
         \x20               left: left.read_zipper(), right: right.read_zipper(), paths_done: false,\n\
         \x20           });\n\
         \x20           ops.push(OrdOp::EPathMapEntries);\n\
         \x20           None\n\
         \x20       }\n\
         \x20       _ => unreachable!(\"equal EPathMap modes named different representations\"),\n\
         \x20   }\n\
         }\n\n\
         fn ord_step_epath_map_entries<'a>(\n\
         \x20   ops: &mut Vec<OrdOp<'a>>,\n\
         \x20   epathmaps: &mut Vec<OrdEPathMapIter<'a>>,\n\
         ) -> Option<std::cmp::Ordering> {\n\
         \x20   let current = epathmaps.last_mut().expect(\"ord PDA: EPathMapEntries without live zippers\");\n\
         \x20   match current {\n\
         \x20       OrdEPathMapIter::Set { left_paths, right_paths, left, right, paths_done } => {\n\
         \x20           if !*paths_done {\n\
         \x20               return match (left_paths.next(), right_paths.next()) {\n\
         \x20                   (None, None) => { *paths_done = true; ops.push(OrdOp::EPathMapEntries); None }\n\
         \x20                   (None, Some(_)) => Some(std::cmp::Ordering::Less),\n\
         \x20                   (Some(_), None) => Some(std::cmp::Ordering::Greater),\n\
         \x20                   (Some(left_path), Some(right_path)) => {\n\
         \x20                       let ordering = left_path.cmp(&right_path);\n\
         \x20                       if ordering == std::cmp::Ordering::Equal { ops.push(OrdOp::EPathMapEntries); None } else { Some(ordering) }\n\
         \x20                   }\n\
         \x20               };\n\
         \x20           }\n\
         \x20           match (left.to_next_get_val(), right.to_next_get_val()) {\n\
         \x20               (None, None) => { epathmaps.pop(); None }\n\
         \x20               (None, Some(_)) => Some(std::cmp::Ordering::Less),\n\
         \x20               (Some(_), None) => Some(std::cmp::Ordering::Greater),\n\
         \x20               (Some(_), Some(_)) => {\n\
         \x20                   let ordering = left.path().cmp(right.path());\n\
         \x20                   if ordering == std::cmp::Ordering::Equal { ops.push(OrdOp::EPathMapEntries); None } else { Some(ordering) }\n\
         \x20               }\n\
         \x20           }\n\
         \x20       }\n\
         \x20       OrdEPathMapIter::Map { left_paths, right_paths, left, right, paths_done } => {\n\
         \x20           if !*paths_done {\n\
         \x20               return match (left_paths.next(), right_paths.next()) {\n\
         \x20                   (None, None) => { *paths_done = true; ops.push(OrdOp::EPathMapEntries); None }\n\
         \x20                   (None, Some(_)) => Some(std::cmp::Ordering::Less),\n\
         \x20                   (Some(_), None) => Some(std::cmp::Ordering::Greater),\n\
         \x20                   (Some(left_path), Some(right_path)) => {\n\
         \x20                       let ordering = left_path.cmp(&right_path);\n\
         \x20                       if ordering == std::cmp::Ordering::Equal { ops.push(OrdOp::EPathMapEntries); None } else { Some(ordering) }\n\
         \x20                   }\n\
         \x20               };\n\
         \x20           }\n\
         \x20           match (left.to_next_get_val(), right.to_next_get_val()) {\n\
         \x20               (None, None) => { epathmaps.pop(); None }\n\
         \x20               (None, Some(_)) => Some(std::cmp::Ordering::Less),\n\
         \x20               (Some(_), None) => Some(std::cmp::Ordering::Greater),\n\
         \x20               (Some(left_value), Some(right_value)) => {\n\
         \x20                   let ordering = left.path().cmp(right.path());\n\
         \x20                   if ordering != std::cmp::Ordering::Equal { return Some(ordering); }\n\
         \x20                   ops.push(OrdOp::EPathMapEntries);\n\
         \x20                   ops.push(OrdOp::Node { left: TermRef::MPar(left_value), right: TermRef::MPar(right_value), from: 0 });\n\
         \x20                   None\n\
         \x20               }\n\
         \x20           }\n\
         \x20       }\n\
         \x20   }\n\
         }\n\n",
    );
}

fn emit_ord_message_step(
    src: &mut String,
    leaf: &str,
    rust_path: &str,
    fields: &[Field],
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
) {
    let stem = rust_type_name(leaf).to_snake_case();
    let has_fields = !fields.is_empty();
    let uses_ops = fields.iter().any(|field| match &field.shape {
        Shape::Message { .. } | Shape::RepeatedMessage { .. } => true,
        Shape::Map { .. } | Shape::Oneof => true,
        Shape::Scalar(_) | Shape::EmptyBytes | Shape::RepeatedString | Shape::RepeatedBytes => {
            false
        }
    });
    let uses_maps = fields
        .iter()
        .any(|field| matches!(field.shape, Shape::Map { .. }));
    let left_name = if has_fields { "left" } else { "_left" };
    let right_name = if has_fields { "right" } else { "_right" };
    let from_name = if has_fields { "from" } else { "_from" };
    let ops_name = if uses_ops { "ops" } else { "_ops" };
    let maps_name = if uses_maps { "maps" } else { "_maps" };
    writeln!(
        src,
        "fn ord_step_{stem}<'a>(\n    \
         {left_name}: &'a {rust_path},\n    \
         {right_name}: &'a {rust_path},\n    \
         {from_name}: u16,\n    \
         {ops_name}: &mut Vec<OrdOp<'a>>,\n    \
         {maps_name}: &mut Vec<(OrdMapIter<'a>, OrdMapIter<'a>)>,\n\
         ) -> Option<std::cmp::Ordering> {{"
    )
    .expect("write");
    for (index, field) in fields.iter().enumerate() {
        let slot = index + 1;
        let name = &field.rust_name;
        writeln!(src, "    if from < {slot} {{").expect("write");
        match &field.shape {
            Shape::Scalar(_) | Shape::EmptyBytes | Shape::RepeatedString | Shape::RepeatedBytes => {
                writeln!(
                    src,
                    "        let ordering = left.{name}.cmp(&right.{name});\n        \
                     if ordering != std::cmp::Ordering::Equal {{ return Some(ordering); }}"
                )
                .expect("write");
            }
            Shape::Message { leaf: child } if extern_set.contains(child.as_str()) => {
                assert_eq!(child, "EPathMap");
                writeln!(
                    src,
                    "        match (&left.{name}, &right.{name}) {{\n            \
                     (None, None) => {{}},\n            \
                     (None, Some(_)) => return Some(std::cmp::Ordering::Less),\n            \
                     (Some(_), None) => return Some(std::cmp::Ordering::Greater),\n            \
                     (Some(left_child), Some(right_child)) => {{\n                \
                     ops.push(OrdOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n                \
                     ops.push(OrdOp::Node {{ left: TermRef::EPathMap(left_child), right: TermRef::EPathMap(right_child), from: 0 }});\n                \
                     return None;\n            \
                     }}\n        \
                     }}",
                    owner = ord_message_arm(leaf)
                )
                .expect("write");
            }
            Shape::Message { leaf: child } => {
                writeln!(
                    src,
                    "        match (&left.{name}, &right.{name}) {{\n            \
                     (None, None) => {{}},\n            \
                     (None, Some(_)) => return Some(std::cmp::Ordering::Less),\n            \
                     (Some(_), None) => return Some(std::cmp::Ordering::Greater),\n            \
                     (Some(left_child), Some(right_child)) => {{\n                \
                     ops.push(OrdOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n                \
                     ops.push(OrdOp::Node {{ left: {left_child}, right: {right_child}, from: 0 }});\n                \
                     return None;\n            \
                     }}\n        \
                     }}",
                    owner = ord_message_arm(leaf),
                    left_child = ord_node_expr(child, "left_child"),
                    right_child = ord_node_expr(child, "right_child")
                )
                .expect("write");
            }
            Shape::RepeatedMessage { leaf: child } if extern_set.contains(child.as_str()) => {
                assert_eq!(child, "EPathMap");
                writeln!(
                    src,
                    "        ops.push(OrdOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n        \
                     ops.push(OrdOp::Seq {{ left: TermSlice::EPathMap(&left.{name}), right: TermSlice::EPathMap(&right.{name}), index: 0 }});\n        \
                     return None;",
                    owner = ord_message_arm(leaf)
                )
                .expect("write");
            }
            Shape::RepeatedMessage { leaf: child } => {
                writeln!(
                    src,
                    "        ops.push(OrdOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n        \
                     ops.push(OrdOp::Seq {{\n            \
                     left: TermSlice::{seq}(&left.{name}),\n            \
                     right: TermSlice::{seq}(&right.{name}),\n            \
                     index: 0,\n        \
                     }});\n        \
                     return None;",
                    owner = ord_message_arm(leaf),
                    seq = rust_type_name(child)
                )
                .expect("write");
            }
            Shape::Map { value_leaf, .. } => {
                assert_eq!(
                    value_leaf, "Par",
                    "schema: Ord map driver only models map<string, Par>"
                );
                writeln!(
                    src,
                    "        ops.push(OrdOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n        \
                     maps.push((left.{name}.iter(), right.{name}.iter()));\n        \
                     ops.push(OrdOp::Map);\n        \
                     return None;",
                    owner = ord_message_arm(leaf)
                )
                .expect("write");
            }
            Shape::Oneof => {
                let key = format!("{leaf}::{name}");
                let oneof = oneof_by_field
                    .get(&key)
                    .unwrap_or_else(|| panic!("schema: Ord field `{key}` has no resolved oneof"));
                writeln!(
                    src,
                    "        match (&left.{name}, &right.{name}) {{\n            \
                     (None, None) => {{}},\n            \
                     (None, Some(_)) => return Some(std::cmp::Ordering::Less),\n            \
                     (Some(_), None) => return Some(std::cmp::Ordering::Greater),\n            \
                     (Some(left_child), Some(right_child)) => {{\n                \
                     ops.push(OrdOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n                \
                     ops.push(OrdOp::Node {{ left: {left_child}, right: {right_child}, from: 0 }});\n                \
                     return None;\n            \
                     }}\n        \
                     }}",
                    owner = ord_message_arm(leaf),
                    left_child = ord_oneof_expr(oneof, "left_child"),
                    right_child = ord_oneof_expr(oneof, "right_child")
                )
                .expect("write");
            }
        }
        src.push_str("    }\n");
    }
    src.push_str("    None\n}\n\n");
}

fn emit_ord_oneof_step(src: &mut String, oneof: &Oneof, extern_set: &BTreeSet<&str>) {
    let ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
    let stem = oneof.rust_ident.to_snake_case();
    let uses_ops = oneof
        .variants
        .iter()
        .any(|variant| variant.message_leaf.as_ref().is_some());
    let ops_name = if uses_ops { "ops" } else { "_ops" };
    writeln!(
        src,
        "fn ord_rank_oneof_{stem}(value: &{ty}) -> u32 {{\n    match value {{"
    )
    .expect("write");
    for variant in &oneof.variants {
        writeln!(
            src,
            "        {ty}::{}(_) => {}u32,",
            variant.rust_ident, variant.index
        )
        .expect("write");
    }
    src.push_str("    }\n}\n\n");
    writeln!(
        src,
        "fn ord_step_oneof_{stem}<'a>(\n    \
         left: &'a {ty},\n    \
         right: &'a {ty},\n    \
         {ops_name}: &mut Vec<OrdOp<'a>>,\n\
         ) -> Option<std::cmp::Ordering> {{\n    \
         let rank_order = ord_rank_oneof_{stem}(left).cmp(&ord_rank_oneof_{stem}(right));\n    \
         if rank_order != std::cmp::Ordering::Equal {{ return Some(rank_order); }}\n    \
         match (left, right) {{"
    )
    .expect("write");
    for variant in &oneof.variants {
        let arm = &variant.rust_ident;
        match &variant.message_leaf {
            Some(leaf) if !extern_set.contains(leaf.as_str()) => {
                writeln!(
                    src,
                    "        ({ty}::{arm}(left_child), {ty}::{arm}(right_child)) => {{\n            \
                     ops.push(OrdOp::Node {{\n                \
                     left: {left},\n                \
                     right: {right},\n                \
                     from: 0,\n            \
                     }});\n            \
                     None\n        \
                     }},",
                    left = ord_node_expr(leaf, "left_child"),
                    right = ord_node_expr(leaf, "right_child")
                )
                .expect("write");
            }
            Some(leaf) if extern_set.contains(leaf.as_str()) => {
                assert_eq!(leaf, "EPathMap");
                writeln!(
                    src,
                    "        ({ty}::{arm}(left_child), {ty}::{arm}(right_child)) => {{\n            \
                     ops.push(OrdOp::Node {{ left: TermRef::EPathMap(left_child), right: TermRef::EPathMap(right_child), from: 0 }});\n            \
                     None\n        \
                     }},"
                )
                .expect("write");
            }
            _ => {
                writeln!(
                    src,
                    "        ({ty}::{arm}(left_value), {ty}::{arm}(right_value)) => {{\n            \
                     let ordering = left_value.cmp(right_value);\n            \
                     (ordering != std::cmp::Ordering::Equal).then_some(ordering)\n        \
                     }},"
                )
                .expect("write");
            }
        }
    }
    src.push_str(
        "        _ => unreachable!(\"equal oneof ranks named different variants\"),\n\
         \x20   }\n\
         }\n\n",
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_eq_hash_driver(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    fields_of: &BTreeMap<&str, &[Field]>,
    reachable_messages: &BTreeSet<&str>,
    reachable_oneofs: &BTreeSet<&str>,
    path_of: &BTreeMap<&str, String>,
) {
    emit_eq_driver(
        src,
        messages,
        oneofs,
        extern_set,
        plan,
        oneof_by_field,
        fields_of,
        reachable_messages,
        reachable_oneofs,
        path_of,
    );
    emit_hash_driver(
        src,
        messages,
        oneofs,
        extern_set,
        plan,
        oneof_by_field,
        fields_of,
        reachable_messages,
        reachable_oneofs,
        path_of,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_eq_driver(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    fields_of: &BTreeMap<&str, &[Field]>,
    reachable_messages: &BTreeSet<&str>,
    reachable_oneofs: &BTreeSet<&str>,
    path_of: &BTreeMap<&str, String>,
) {
    src.push_str(
        "// ===========================================================================\n\
         // §G  The `PartialEq` PDA\n\
         // ===========================================================================\n\
         enum EqOp<'a> {\n\
         \x20   Node { left: TermRef<'a>, right: TermRef<'a>, from: u16 },\n\
         \x20   Seq { left: TermSlice<'a>, right: TermSlice<'a>, index: usize },\n\
         \x20   Map,\n\
         \x20   EPathMapEntries,\n\
         }\n\n\
         type EqMapIter<'a> = std::collections::btree_map::Iter<'a, String, Par>;\n\n\
         enum EqEPathMapIter<'a> {\n\
         \x20   Set {\n\
         \x20       left_paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, ()>,\n\
         \x20       right_paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, ()>,\n\
         \x20       left: pathmap::zipper::ReadZipperUntracked<'a, 'static, ()>,\n\
         \x20       right: pathmap::zipper::ReadZipperUntracked<'a, 'static, ()>,\n\
         \x20       paths_done: bool,\n\
         \x20   },\n\
         \x20   Map {\n\
         \x20       left_paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, Par>,\n\
         \x20       right_paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, Par>,\n\
         \x20       left: pathmap::zipper::ReadZipperUntracked<'a, 'static, Par>,\n\
         \x20       right: pathmap::zipper::ReadZipperUntracked<'a, 'static, Par>,\n\
         \x20       paths_done: bool,\n\
         \x20   },\n\
         }\n\n\
         fn term_eq<'a>(left: TermRef<'a>, right: TermRef<'a>) -> bool {\n\
         \x20   let mut ops = Vec::with_capacity(64);\n\
         \x20   let mut maps: Vec<(EqMapIter<'a>, EqMapIter<'a>)> = Vec::new();\n\
         \x20   let mut epathmaps: Vec<EqEPathMapIter<'a>> = Vec::new();\n\
         \x20   ops.push(EqOp::Node { left, right, from: 0 });\n\
         \x20   while let Some(op) = ops.pop() {\n\
         \x20       let equal = match op {\n\
         \x20           EqOp::Node { left, right, from } => eq_step(left, right, from, &mut ops, &mut maps, &mut epathmaps),\n\
         \x20           EqOp::Seq { left, right, index } => {\n\
         \x20               if left.len() != right.len() { false } else if index == left.len() { true } else {\n\
         \x20                   ops.push(EqOp::Seq { left, right, index: index + 1 });\n\
         \x20                   ops.push(EqOp::Node { left: left.get(index), right: right.get(index), from: 0 });\n\
         \x20                   true\n\
         \x20               }\n\
         \x20           }\n\
         \x20           EqOp::Map => {\n\
         \x20               let (left, right) = maps.last_mut().expect(\"eq PDA: Map without live iterators\");\n\
         \x20               match (left.next(), right.next()) {\n\
         \x20                   (None, None) => { maps.pop(); true }\n\
         \x20                   (Some((left_key, left_value)), Some((right_key, right_value))) if left_key == right_key => {\n\
         \x20                       ops.push(EqOp::Map);\n\
         \x20                       ops.push(EqOp::Node { left: TermRef::MPar(left_value), right: TermRef::MPar(right_value), from: 0 });\n\
         \x20                       true\n\
         \x20                   }\n\
         \x20                   (None, Some(_)) | (Some(_), None) | (Some(_), Some(_)) => false,\n\
         \x20               }\n\
         \x20           }\n\
         \x20           EqOp::EPathMapEntries => eq_step_epath_map_entries(&mut ops, &mut epathmaps),\n\
         \x20       };\n\
         \x20       if !equal { return false; }\n\
         \x20   }\n\
         \x20   debug_assert!(maps.is_empty(), \"eq PDA left map iterators live\");\n\
         \x20   debug_assert!(epathmaps.is_empty(), \"eq PDA left EPathMap iterators live\");\n\
         \x20   true\n\
         }\n\n\
         fn eq_step<'a>(\n\
         \x20   left: TermRef<'a>, right: TermRef<'a>, from: u16,\n\
         \x20   ops: &mut Vec<EqOp<'a>>,\n\
         \x20   maps: &mut Vec<(EqMapIter<'a>, EqMapIter<'a>)>,\n\
         \x20   epathmaps: &mut Vec<EqEPathMapIter<'a>>,\n\
         ) -> bool {\n\
         \x20   match (left, right) {\n",
    );
    for message in messages {
        let leaf = message.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        writeln!(
            src,
            "        (TermRef::{arm}(left), TermRef::{arm}(right)) => eq_step_{stem}(left, right, from, ops, maps),",
            arm = ord_message_arm(leaf),
            stem = rust_type_name(leaf).to_snake_case()
        )
        .expect("write");
    }
    for oneof in oneofs {
        if !reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            continue;
        }
        writeln!(
            src,
            "        (TermRef::{arm}(left), TermRef::{arm}(right)) => eq_step_oneof_{stem}(left, right, ops),",
            arm = ord_oneof_arm(oneof),
            stem = oneof.rust_ident.to_snake_case()
        )
        .expect("write");
    }
    src.push_str(
        "        (TermRef::EPathMap(left), TermRef::EPathMap(right)) => eq_step_epath_map(left, right, from, ops),\n\
         \x20       (TermRef::EPathMapRepr(left), TermRef::EPathMapRepr(right)) => eq_open_epath_map_repr(left, right, ops, epathmaps),\n\
         \x20       _ => unreachable!(\"eq PDA compared different schema node types\"),\n\
         \x20   }\n\
         }\n\n",
    );
    emit_eq_epath_map_steps(src);
    for message in messages {
        let leaf = message.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        emit_eq_message_step(
            src,
            leaf,
            path_of[leaf].as_str(),
            fields_of[leaf],
            oneof_by_field,
            extern_set,
        );
    }
    for oneof in oneofs {
        if reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            emit_eq_oneof_step(src, oneof, extern_set);
        }
    }
    for leaf in &plan.cut {
        let rust_path = &path_of[leaf.as_str()];
        writeln!(
            src,
            "impl PartialEq for {rust_path} {{\n    fn eq(&self, other: &Self) -> bool {{ term_eq(TermRef::{arm}(self), TermRef::{arm}(other)) }}\n}}\n",
            arm = ord_message_arm(leaf)
        )
        .expect("write");
    }
    src.push_str(
        "impl PartialEq for EPathMap {\n\
         \x20   fn eq(&self, other: &Self) -> bool { term_eq(TermRef::EPathMap(self), TermRef::EPathMap(other)) }\n\
         }\n\
         impl Eq for EPathMap {}\n\n\
         impl PartialEq for crate::rust::rhoapi_ext::EntryTrie {\n\
         \x20   fn eq(&self, other: &Self) -> bool {\n\
         \x20       self.len() == other.len() && term_eq(TermRef::EPathMapRepr(self.representation()), TermRef::EPathMapRepr(other.representation()))\n\
         \x20   }\n\
         }\n\
         impl Eq for crate::rust::rhoapi_ext::EntryTrie {}\n\n",
    );

    emit_eq_oracle(
        src,
        messages,
        oneofs,
        extern_set,
        plan,
        oneof_by_field,
        fields_of,
        reachable_messages,
        reachable_oneofs,
        path_of,
    );
}

fn emit_eq_epath_map_steps(src: &mut String) {
    src.push_str(
        "fn eq_step_epath_map<'a>(left: &'a EPathMap, right: &'a EPathMap, from: u16, ops: &mut Vec<EqOp<'a>>) -> bool {\n\
         \x20   if from < 1 && left.connective_used != right.connective_used { return false; }\n\
         \x20   if from < 2 && left.remainder != right.remainder { return false; }\n\
         \x20   if from < 3 {\n\
         \x20       ops.push(EqOp::Node { left: TermRef::EPathMap(left), right: TermRef::EPathMap(right), from: 3 });\n\
         \x20       ops.push(EqOp::Node { left: TermRef::EPathMapRepr(left.representation()), right: TermRef::EPathMapRepr(right.representation()), from: 0 });\n\
         \x20   }\n\
         \x20   true\n\
         }\n\n\
         fn eq_open_epath_map_repr<'a>(\n\
         \x20   left: &'a crate::rust::epathmap_trie_codec::EPathMapRepr<Par>,\n\
         \x20   right: &'a crate::rust::epathmap_trie_codec::EPathMapRepr<Par>,\n\
         \x20   ops: &mut Vec<EqOp<'a>>,\n\
         \x20   epathmaps: &mut Vec<EqEPathMapIter<'a>>,\n\
         ) -> bool {\n\
         \x20   use crate::rust::epathmap_trie_codec::EPathMapRepr;\n\
         \x20   if left.mode() != right.mode() { return false; }\n\
         \x20   match (left, right) {\n\
         \x20       (EPathMapRepr::Empty, EPathMapRepr::Empty) => true,\n\
         \x20       (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => {\n\
         \x20           epathmaps.push(EqEPathMapIter::Set {\n\
         \x20               left_paths: left.read_zipper().into_path_iter(),\n\
         \x20               right_paths: right.read_zipper().into_path_iter(),\n\
         \x20               left: left.read_zipper(), right: right.read_zipper(), paths_done: false,\n\
         \x20           });\n\
         \x20           ops.push(EqOp::EPathMapEntries);\n\
         \x20           true\n\
         \x20       }\n\
         \x20       (EPathMapRepr::Map(left), EPathMapRepr::Map(right)) => {\n\
         \x20           epathmaps.push(EqEPathMapIter::Map {\n\
         \x20               left_paths: left.read_zipper().into_path_iter(),\n\
         \x20               right_paths: right.read_zipper().into_path_iter(),\n\
         \x20               left: left.read_zipper(), right: right.read_zipper(), paths_done: false,\n\
         \x20           });\n\
         \x20           ops.push(EqOp::EPathMapEntries);\n\
         \x20           true\n\
         \x20       }\n\
         \x20       _ => unreachable!(\"equal EPathMap modes named different representations\"),\n\
         \x20   }\n\
         }\n\n\
         fn eq_step_epath_map_entries<'a>(ops: &mut Vec<EqOp<'a>>, epathmaps: &mut Vec<EqEPathMapIter<'a>>) -> bool {\n\
         \x20   let current = epathmaps.last_mut().expect(\"eq PDA: EPathMapEntries without live zippers\");\n\
         \x20   match current {\n\
         \x20       EqEPathMapIter::Set { left_paths, right_paths, left, right, paths_done } => {\n\
         \x20           if !*paths_done {\n\
         \x20               return match (left_paths.next(), right_paths.next()) {\n\
         \x20                   (None, None) => { *paths_done = true; ops.push(EqOp::EPathMapEntries); true }\n\
         \x20                   (Some(left_path), Some(right_path)) if left_path == right_path => { ops.push(EqOp::EPathMapEntries); true }\n\
         \x20                   (None, Some(_)) | (Some(_), None) | (Some(_), Some(_)) => false,\n\
         \x20               };\n\
         \x20           }\n\
         \x20           match (left.to_next_get_val(), right.to_next_get_val()) {\n\
         \x20           (None, None) => { epathmaps.pop(); true }\n\
         \x20           (Some(_), Some(_)) if left.path() == right.path() => { ops.push(EqOp::EPathMapEntries); true }\n\
         \x20           (None, Some(_)) | (Some(_), None) | (Some(_), Some(_)) => false,\n\
         \x20           }\n\
         \x20       }\n\
         \x20       EqEPathMapIter::Map { left_paths, right_paths, left, right, paths_done } => {\n\
         \x20           if !*paths_done {\n\
         \x20               return match (left_paths.next(), right_paths.next()) {\n\
         \x20                   (None, None) => { *paths_done = true; ops.push(EqOp::EPathMapEntries); true }\n\
         \x20                   (Some(left_path), Some(right_path)) if left_path == right_path => { ops.push(EqOp::EPathMapEntries); true }\n\
         \x20                   (None, Some(_)) | (Some(_), None) | (Some(_), Some(_)) => false,\n\
         \x20               };\n\
         \x20           }\n\
         \x20           match (left.to_next_get_val(), right.to_next_get_val()) {\n\
         \x20           (None, None) => { epathmaps.pop(); true }\n\
         \x20           (Some(left_value), Some(right_value)) if left.path() == right.path() => {\n\
         \x20               ops.push(EqOp::EPathMapEntries);\n\
         \x20               ops.push(EqOp::Node { left: TermRef::MPar(left_value), right: TermRef::MPar(right_value), from: 0 });\n\
         \x20               true\n\
         \x20           }\n\
         \x20           (None, Some(_)) | (Some(_), None) | (Some(_), Some(_)) => false,\n\
         \x20           }\n\
         \x20       }\n\
         \x20   }\n\
         }\n\n",
    );
}

fn emit_eq_message_step(
    src: &mut String,
    leaf: &str,
    rust_path: &str,
    fields: &[Field],
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
) {
    let stem = rust_type_name(leaf).to_snake_case();
    writeln!(
        src,
        "#[allow(unused_variables)]\nfn eq_step_{stem}<'a>(left: &'a {rust_path}, right: &'a {rust_path}, from: u16, ops: &mut Vec<EqOp<'a>>, maps: &mut Vec<(EqMapIter<'a>, EqMapIter<'a>)>) -> bool {{"
    )
    .expect("write");
    for (index, field) in fields.iter().enumerate() {
        if field.rust_name == "locally_free" {
            continue;
        }
        let slot = index + 1;
        let name = &field.rust_name;
        writeln!(src, "    if from < {slot} {{").expect("write");
        match &field.shape {
            Shape::Scalar(_) | Shape::EmptyBytes | Shape::RepeatedString | Shape::RepeatedBytes => {
                writeln!(
                    src,
                    "        if left.{name} != right.{name} {{ return false; }}"
                )
                .expect("write");
            }
            Shape::Message { leaf: child } => {
                let (left_child, right_child) = if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    (
                        "TermRef::EPathMap(left_child)".to_string(),
                        "TermRef::EPathMap(right_child)".to_string(),
                    )
                } else {
                    (
                        ord_node_expr(child, "left_child"),
                        ord_node_expr(child, "right_child"),
                    )
                };
                writeln!(
                    src,
                    "        match (&left.{name}, &right.{name}) {{\n            \
                     (None, None) => {{}},\n            \
                     (Some(left_child), Some(right_child)) => {{\n                \
                     ops.push(EqOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n                \
                     ops.push(EqOp::Node {{ left: {left_child}, right: {right_child}, from: 0 }});\n                \
                     return true;\n            \
                     }}\n            \
                     (None, Some(_)) | (Some(_), None) => return false,\n        \
                     }}",
                    owner = ord_message_arm(leaf)
                )
                .expect("write");
            }
            Shape::RepeatedMessage { leaf: child } => {
                let (left_seq, right_seq) = if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    (
                        format!("TermSlice::EPathMap(&left.{name})"),
                        format!("TermSlice::EPathMap(&right.{name})"),
                    )
                } else {
                    let seq = rust_type_name(child);
                    (
                        format!("TermSlice::{seq}(&left.{name})"),
                        format!("TermSlice::{seq}(&right.{name})"),
                    )
                };
                writeln!(
                    src,
                    "        ops.push(EqOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n        \
                     ops.push(EqOp::Seq {{ left: {left_seq}, right: {right_seq}, index: 0 }});\n        \
                     return true;",
                    owner = ord_message_arm(leaf)
                )
                .expect("write");
            }
            Shape::Map { value_leaf, .. } => {
                assert_eq!(value_leaf, "Par");
                writeln!(
                    src,
                    "        ops.push(EqOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n        \
                     maps.push((left.{name}.iter(), right.{name}.iter()));\n        \
                     ops.push(EqOp::Map);\n        \
                     return true;",
                    owner = ord_message_arm(leaf)
                )
                .expect("write");
            }
            Shape::Oneof => {
                let key = format!("{leaf}::{name}");
                let oneof = oneof_by_field
                    .get(&key)
                    .unwrap_or_else(|| panic!("schema: Eq field `{key}` has no oneof"));
                writeln!(
                    src,
                    "        match (&left.{name}, &right.{name}) {{\n            \
                     (None, None) => {{}},\n            \
                     (Some(left_child), Some(right_child)) => {{\n                \
                     ops.push(EqOp::Node {{ left: TermRef::{owner}(left), right: TermRef::{owner}(right), from: {slot} }});\n                \
                     ops.push(EqOp::Node {{ left: {left_child}, right: {right_child}, from: 0 }});\n                \
                     return true;\n            \
                     }}\n            \
                     (None, Some(_)) | (Some(_), None) => return false,\n        \
                     }}",
                    owner = ord_message_arm(leaf),
                    left_child = ord_oneof_expr(oneof, "left_child"),
                    right_child = ord_oneof_expr(oneof, "right_child")
                )
                .expect("write");
            }
        }
        src.push_str("    }\n");
    }
    src.push_str("    true\n}\n\n");
}

fn emit_eq_oneof_step(src: &mut String, oneof: &Oneof, extern_set: &BTreeSet<&str>) {
    let ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
    let stem = oneof.rust_ident.to_snake_case();
    writeln!(
        src,
        "#[allow(unused_variables)]\nfn eq_step_oneof_{stem}<'a>(left: &'a {ty}, right: &'a {ty}, ops: &mut Vec<EqOp<'a>>) -> bool {{\n    if ord_rank_oneof_{stem}(left) != ord_rank_oneof_{stem}(right) {{ return false; }}\n    match (left, right) {{"
    )
    .expect("write");
    for variant in &oneof.variants {
        let arm = &variant.rust_ident;
        match &variant.message_leaf {
            Some(leaf) if extern_set.contains(leaf.as_str()) => {
                assert_eq!(leaf, "EPathMap");
                writeln!(
                    src,
                    "        ({ty}::{arm}(left), {ty}::{arm}(right)) => {{ ops.push(EqOp::Node {{ left: TermRef::EPathMap(left), right: TermRef::EPathMap(right), from: 0 }}); true }},"
                )
                .expect("write");
            }
            Some(leaf) => {
                writeln!(
                    src,
                    "        ({ty}::{arm}(left), {ty}::{arm}(right)) => {{ ops.push(EqOp::Node {{ left: {left}, right: {right}, from: 0 }}); true }},",
                    left = ord_node_expr(leaf, "left"),
                    right = ord_node_expr(leaf, "right")
                )
                .expect("write");
            }
            None => {
                writeln!(
                    src,
                    "        ({ty}::{arm}(left), {ty}::{arm}(right)) => left == right,"
                )
                .expect("write");
            }
        }
    }
    src.push_str(
        "        _ => unreachable!(\"equal oneof ranks named different variants\"),\n    }\n}\n\n",
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_eq_oracle(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    fields_of: &BTreeMap<&str, &[Field]>,
    reachable_messages: &BTreeSet<&str>,
    reachable_oneofs: &BTreeSet<&str>,
    path_of: &BTreeMap<&str, String>,
) {
    src.push_str(
        "#[cfg(test)]\nfn oracle_eq_epath_map(left: &EPathMap, right: &EPathMap) -> bool {\n\
         \x20   use crate::rust::epathmap_trie_codec::EPathMapRepr;\n\
         \x20   if left.connective_used != right.connective_used || left.remainder != right.remainder { return false; }\n\
         \x20   match (left.representation(), right.representation()) {\n\
         \x20       (EPathMapRepr::Empty, EPathMapRepr::Empty) => true,\n\
         \x20       (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => {\n\
         \x20           if !left.read_zipper().into_path_iter().eq(right.read_zipper().into_path_iter()) { return false; }\n\
         \x20           let mut left = left.iter(); let mut right = right.iter();\n\
         \x20           loop { match (left.next(), right.next()) {\n\
         \x20               (None, None) => break true,\n\
         \x20               (Some((left_key, ())), Some((right_key, ()))) if left_key == right_key => {}\n\
         \x20               (None, Some(_)) | (Some(_), None) | (Some(_), Some(_)) => break false,\n\
         \x20           }}\n\
         \x20       }\n\
         \x20       (EPathMapRepr::Map(left), EPathMapRepr::Map(right)) => {\n\
         \x20           if !left.read_zipper().into_path_iter().eq(right.read_zipper().into_path_iter()) { return false; }\n\
         \x20           let mut left = left.iter(); let mut right = right.iter();\n\
         \x20           loop { match (left.next(), right.next()) {\n\
         \x20               (None, None) => break true,\n\
         \x20               (Some((left_key, left_value)), Some((right_key, right_value))) if left_key == right_key && oracle_eq_par(left_value, right_value) => {}\n\
         \x20               (None, Some(_)) | (Some(_), None) | (Some(_), Some(_)) => break false,\n\
         \x20           }}\n\
         \x20       }\n\
         \x20       _ => false,\n\
         \x20   }\n\
         }\n\n",
    );
    for message in messages {
        let leaf = message.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        emit_eq_oracle_message(
            src,
            leaf,
            path_of[leaf].as_str(),
            fields_of[leaf],
            oneof_by_field,
            extern_set,
        );
    }
    for oneof in oneofs {
        if reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            emit_eq_oracle_oneof(src, oneof, extern_set);
        }
    }
    assert_eq!(
        plan.cut.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["Par"],
        "schema: Eq differential root must track the descriptor-derived cut set"
    );
    src.push_str(
        "#[cfg(test)]\nmod eq_pda_differential {\n\
         \x20   use super::*;\n\
         \x20   use proptest::strategy::{Strategy, ValueTree};\n\
         \x20   use proptest::test_runner::TestRunner;\n\n\
         \x20   #[test]\n\
         \x20   fn generated_eq_matches_the_recursive_oracle() {\n\
         \x20       let strategy = crate::rust::test_utils::test_utils::generate_par(4);\n\
         \x20       let mut runner = TestRunner::deterministic();\n\
         \x20       for _ in 0..64 {\n\
         \x20           let left = strategy.new_tree(&mut runner).expect(\"Eq strategy left\").current();\n\
         \x20           let right = strategy.new_tree(&mut runner).expect(\"Eq strategy right\").current();\n\
         \x20           assert_eq!(left == right, oracle_eq_par(&left, &right));\n\
         \x20           let equal = left.clone();\n\
         \x20           assert!(left == equal && oracle_eq_par(&left, &equal));\n\
         \x20           let mut ignored_metadata = left.clone();\n\
         \x20           ignored_metadata.locally_free.push(0xA5);\n\
         \x20           assert!(left == ignored_metadata && oracle_eq_par(&left, &ignored_metadata));\n\
         \x20       }\n\
         \x20   }\n\
         }\n\n",
    );
}

fn emit_eq_oracle_message(
    src: &mut String,
    leaf: &str,
    rust_path: &str,
    fields: &[Field],
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
) {
    let stem = rust_type_name(leaf).to_snake_case();
    let has_semantic_fields = fields.iter().any(|field| field.rust_name != "locally_free");
    let left_name = if has_semantic_fields { "left" } else { "_left" };
    let right_name = if has_semantic_fields {
        "right"
    } else {
        "_right"
    };
    writeln!(
        src,
        "#[cfg(test)]\nfn oracle_eq_{stem}({left_name}: &{rust_path}, {right_name}: &{rust_path}) -> bool {{"
    )
    .expect("write");
    for field in fields {
        if field.rust_name == "locally_free" {
            continue;
        }
        let name = &field.rust_name;
        match &field.shape {
            Shape::Scalar(_) | Shape::EmptyBytes | Shape::RepeatedString | Shape::RepeatedBytes => {
                writeln!(
                    src,
                    "    if left.{name} != right.{name} {{ return false; }}"
                )
                .expect("write");
            }
            Shape::Message { leaf: child } => {
                let compare = if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    "oracle_eq_epath_map".to_string()
                } else {
                    format!("oracle_eq_{}", rust_type_name(child).to_snake_case())
                };
                writeln!(
                    src,
                    "    let equal = match (&left.{name}, &right.{name}) {{ (None, None) => true, (Some(left), Some(right)) => {compare}(left, right), (None, Some(_)) | (Some(_), None) => false }};\n    if !equal {{ return false; }}"
                )
                .expect("write");
            }
            Shape::RepeatedMessage { leaf: child } => {
                let compare = if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    "oracle_eq_epath_map".to_string()
                } else {
                    format!("oracle_eq_{}", rust_type_name(child).to_snake_case())
                };
                writeln!(
                    src,
                    "    if left.{name}.len() != right.{name}.len() {{ return false; }}\n    if !left.{name}.iter().zip(&right.{name}).all(|(left, right)| {compare}(left, right)) {{ return false; }}"
                )
                .expect("write");
            }
            Shape::Map { value_leaf, .. } => {
                assert_eq!(value_leaf, "Par");
                writeln!(
                    src,
                    "    if left.{name}.len() != right.{name}.len() {{ return false; }}\n    if !left.{name}.iter().zip(&right.{name}).all(|((left_key, left_value), (right_key, right_value))| left_key == right_key && oracle_eq_par(left_value, right_value)) {{ return false; }}"
                )
                .expect("write");
            }
            Shape::Oneof => {
                let key = format!("{leaf}::{name}");
                let oneof = oneof_by_field
                    .get(&key)
                    .unwrap_or_else(|| panic!("schema: Eq oracle field `{key}` has no oneof"));
                let compare = format!("oracle_eq_oneof_{}", oneof.rust_ident.to_snake_case());
                writeln!(
                    src,
                    "    let equal = match (&left.{name}, &right.{name}) {{ (None, None) => true, (Some(left), Some(right)) => {compare}(left, right), (None, Some(_)) | (Some(_), None) => false }};\n    if !equal {{ return false; }}"
                )
                .expect("write");
            }
        }
    }
    src.push_str("    true\n}\n\n");
}

fn emit_eq_oracle_oneof(src: &mut String, oneof: &Oneof, extern_set: &BTreeSet<&str>) {
    let ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
    let stem = oneof.rust_ident.to_snake_case();
    writeln!(
        src,
        "#[cfg(test)]\nfn oracle_eq_oneof_{stem}(left: &{ty}, right: &{ty}) -> bool {{\n    if ord_rank_oneof_{stem}(left) != ord_rank_oneof_{stem}(right) {{ return false; }}\n    match (left, right) {{"
    )
    .expect("write");
    for variant in &oneof.variants {
        let arm = &variant.rust_ident;
        let expression = match &variant.message_leaf {
            Some(leaf) if extern_set.contains(leaf.as_str()) => {
                assert_eq!(leaf, "EPathMap");
                "oracle_eq_epath_map(left, right)".to_string()
            }
            Some(leaf) => format!(
                "oracle_eq_{}(left, right)",
                rust_type_name(leaf).to_snake_case()
            ),
            None => "left == right".to_string(),
        };
        writeln!(
            src,
            "        ({ty}::{arm}(left), {ty}::{arm}(right)) => {expression},"
        )
        .expect("write");
    }
    src.push_str(
        "        _ => unreachable!(\"equal oneof ranks named different variants\"),\n    }\n}\n\n",
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_hash_driver(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    fields_of: &BTreeMap<&str, &[Field]>,
    reachable_messages: &BTreeSet<&str>,
    reachable_oneofs: &BTreeSet<&str>,
    path_of: &BTreeMap<&str, String>,
) {
    src.push_str(
        "// ===========================================================================\n\
         // §H  The `Hash` PDA\n\
         // ===========================================================================\n\
         enum HashOp<'a> {\n\
         \x20   Node { node: TermRef<'a>, from: u16 },\n\
         \x20   Seq { values: TermSlice<'a>, index: usize },\n\
         \x20   Map,\n\
         \x20   EPathMapEntries,\n\
         }\n\n\
         type HashMapIter<'a> = std::collections::btree_map::Iter<'a, String, Par>;\n\n\
         enum HashEPathMapIter<'a> {\n\
         \x20   Set {\n\
         \x20       paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, ()>,\n\
         \x20       values: pathmap::zipper::ReadZipperUntracked<'a, 'static, ()>,\n\
         \x20       paths_done: bool,\n\
         \x20   },\n\
         \x20   Map {\n\
         \x20       paths: pathmap::zipper::ReadZipperPathIter<'a, 'static, Par>,\n\
         \x20       values: pathmap::zipper::ReadZipperUntracked<'a, 'static, Par>,\n\
         \x20       paths_done: bool,\n\
         \x20   },\n\
         }\n\n\
         fn term_hash<'a, H: std::hash::Hasher>(root: TermRef<'a>, state: &mut H) {\n\
         \x20   let mut ops = Vec::with_capacity(64);\n\
         \x20   let mut maps: Vec<HashMapIter<'a>> = Vec::new();\n\
         \x20   let mut epathmaps: Vec<HashEPathMapIter<'a>> = Vec::new();\n\
         \x20   ops.push(HashOp::Node { node: root, from: 0 });\n\
         \x20   while let Some(op) = ops.pop() {\n\
         \x20       match op {\n\
         \x20           HashOp::Node { node, from } => hash_step(node, from, state, &mut ops, &mut maps, &mut epathmaps),\n\
         \x20           HashOp::Seq { values, index } => {\n\
         \x20               if index < values.len() {\n\
         \x20                   ops.push(HashOp::Seq { values, index: index + 1 });\n\
         \x20                   ops.push(HashOp::Node { node: values.get(index), from: 0 });\n\
         \x20               }\n\
         \x20           }\n\
         \x20           HashOp::Map => {\n\
         \x20               let iter = maps.last_mut().expect(\"hash PDA: Map without live iterator\");\n\
         \x20               if let Some((key, value)) = iter.next() {\n\
         \x20                   key.hash(state);\n\
         \x20                   ops.push(HashOp::Map);\n\
         \x20                   ops.push(HashOp::Node { node: TermRef::MPar(value), from: 0 });\n\
         \x20               } else { maps.pop(); }\n\
         \x20           }\n\
         \x20           HashOp::EPathMapEntries => hash_step_epath_map_entries(state, &mut ops, &mut epathmaps),\n\
         \x20       }\n\
         \x20   }\n\
         \x20   debug_assert!(maps.is_empty(), \"hash PDA left map iterators live\");\n\
         \x20   debug_assert!(epathmaps.is_empty(), \"hash PDA left EPathMap iterators live\");\n\
         }\n\n\
         fn hash_step<'a, H: std::hash::Hasher>(\n\
         \x20   node: TermRef<'a>, from: u16, state: &mut H,\n\
         \x20   ops: &mut Vec<HashOp<'a>>, maps: &mut Vec<HashMapIter<'a>>,\n\
         \x20   epathmaps: &mut Vec<HashEPathMapIter<'a>>,\n\
         ) {\n\
         \x20   match node {\n",
    );
    for message in messages {
        let leaf = message.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        writeln!(
            src,
            "        TermRef::{arm}(value) => hash_step_{stem}(value, from, state, ops, maps),",
            arm = ord_message_arm(leaf),
            stem = rust_type_name(leaf).to_snake_case()
        )
        .expect("write");
    }
    for oneof in oneofs {
        if !reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            continue;
        }
        writeln!(
            src,
            "        TermRef::{arm}(value) => hash_step_oneof_{stem}(value, state, ops),",
            arm = ord_oneof_arm(oneof),
            stem = oneof.rust_ident.to_snake_case()
        )
        .expect("write");
    }
    src.push_str(
        "        TermRef::EPathMap(value) => hash_step_epath_map(value, from, state, ops),\n\
         \x20       TermRef::EPathMapRepr(value) => hash_open_epath_map_repr(value, ops, epathmaps),\n\
         \x20   }\n\
         }\n\n",
    );
    emit_hash_epath_map_steps(src);
    for message in messages {
        let leaf = message.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        emit_hash_message_step(
            src,
            leaf,
            path_of[leaf].as_str(),
            fields_of[leaf],
            oneof_by_field,
            extern_set,
        );
    }
    for oneof in oneofs {
        if reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            emit_hash_oneof_step(src, oneof, extern_set);
        }
    }
    for leaf in &plan.cut {
        let rust_path = &path_of[leaf.as_str()];
        writeln!(
            src,
            "impl std::hash::Hash for {rust_path} {{\n    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {{ term_hash(TermRef::{arm}(self), state); }}\n}}\n",
            arm = ord_message_arm(leaf)
        )
        .expect("write");
    }
    src.push_str(
        "impl std::hash::Hash for EPathMap {\n\
         \x20   fn hash<H: std::hash::Hasher>(&self, state: &mut H) { term_hash(TermRef::EPathMap(self), state); }\n\
         }\n\n\
         impl std::hash::Hash for crate::rust::rhoapi_ext::EntryTrie {\n\
         \x20   fn hash<H: std::hash::Hasher>(&self, state: &mut H) { term_hash(TermRef::EPathMapRepr(self.representation()), state); }\n\
         }\n\n",
    );

    emit_hash_oracle(
        src,
        messages,
        oneofs,
        extern_set,
        plan,
        oneof_by_field,
        fields_of,
        reachable_messages,
        reachable_oneofs,
        path_of,
    );
}

fn emit_hash_epath_map_steps(src: &mut String) {
    src.push_str(
        "fn hash_step_epath_map<'a, H: std::hash::Hasher>(value: &'a EPathMap, from: u16, state: &mut H, ops: &mut Vec<HashOp<'a>>) {\n\
         \x20   if from < 1 {\n\
         \x20       ops.push(HashOp::Node { node: TermRef::EPathMap(value), from: 1 });\n\
         \x20       ops.push(HashOp::Node { node: TermRef::EPathMapRepr(value.representation()), from: 0 });\n\
         \x20       return;\n\
         \x20   }\n\
         \x20   if from < 2 { value.connective_used.hash(state); }\n\
         \x20   if from < 3 { value.remainder.hash(state); }\n\
         }\n\n\
         fn hash_open_epath_map_repr<'a>(\n\
         \x20   value: &'a crate::rust::epathmap_trie_codec::EPathMapRepr<Par>,\n\
         \x20   ops: &mut Vec<HashOp<'a>>,\n\
         \x20   epathmaps: &mut Vec<HashEPathMapIter<'a>>,\n\
         ) {\n\
         \x20   use crate::rust::epathmap_trie_codec::EPathMapRepr;\n\
         \x20   match value {\n\
         \x20       EPathMapRepr::Empty => {}\n\
         \x20       EPathMapRepr::Set(map) => {\n\
         \x20           epathmaps.push(HashEPathMapIter::Set { paths: map.read_zipper().into_path_iter(), values: map.read_zipper(), paths_done: false });\n\
         \x20           ops.push(HashOp::EPathMapEntries);\n\
         \x20       }\n\
         \x20       EPathMapRepr::Map(map) => {\n\
         \x20           epathmaps.push(HashEPathMapIter::Map { paths: map.read_zipper().into_path_iter(), values: map.read_zipper(), paths_done: false });\n\
         \x20           ops.push(HashOp::EPathMapEntries);\n\
         \x20       }\n\
         \x20   }\n\
         }\n\n\
         fn hash_step_epath_map_entries<'a, H: std::hash::Hasher>(state: &mut H, ops: &mut Vec<HashOp<'a>>, epathmaps: &mut Vec<HashEPathMapIter<'a>>) {\n\
         \x20   let current = epathmaps.last_mut().expect(\"hash PDA: EPathMapEntries without live zipper\");\n\
         \x20   match current {\n\
         \x20       HashEPathMapIter::Set { paths, values, paths_done } => {\n\
         \x20           if !*paths_done {\n\
         \x20               if let Some(path) = paths.next() { path.hash(state); ops.push(HashOp::EPathMapEntries); } else { *paths_done = true; ops.push(HashOp::EPathMapEntries); }\n\
         \x20           } else if values.to_next_get_val().is_some() { values.path().hash(state); ops.push(HashOp::EPathMapEntries); } else { epathmaps.pop(); }\n\
         \x20       }\n\
         \x20       HashEPathMapIter::Map { paths, values, paths_done } => {\n\
         \x20           if !*paths_done {\n\
         \x20               if let Some(path) = paths.next() { path.hash(state); ops.push(HashOp::EPathMapEntries); } else { *paths_done = true; ops.push(HashOp::EPathMapEntries); }\n\
         \x20           } else if let Some(value) = values.to_next_get_val() {\n\
         \x20               values.path().hash(state);\n\
         \x20               ops.push(HashOp::EPathMapEntries);\n\
         \x20               ops.push(HashOp::Node { node: TermRef::MPar(value), from: 0 });\n\
         \x20           } else { epathmaps.pop(); }\n\
         \x20       }\n\
         \x20   }\n\
         }\n\n",
    );
}

fn emit_hash_message_step(
    src: &mut String,
    leaf: &str,
    rust_path: &str,
    fields: &[Field],
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
) {
    let stem = rust_type_name(leaf).to_snake_case();
    writeln!(
        src,
        "#[allow(unused_variables)]\nfn hash_step_{stem}<'a, H: std::hash::Hasher>(value: &'a {rust_path}, from: u16, state: &mut H, ops: &mut Vec<HashOp<'a>>, maps: &mut Vec<HashMapIter<'a>>) {{"
    )
    .expect("write");
    for (index, field) in fields.iter().enumerate() {
        if field.rust_name == "locally_free" {
            continue;
        }
        let slot = index + 1;
        let name = &field.rust_name;
        writeln!(src, "    if from < {slot} {{").expect("write");
        match &field.shape {
            Shape::Scalar(_) | Shape::EmptyBytes | Shape::RepeatedString | Shape::RepeatedBytes => {
                writeln!(src, "        value.{name}.hash(state);").expect("write");
            }
            Shape::Message { leaf: child } => {
                let child_expr = if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    "TermRef::EPathMap(child)".to_string()
                } else {
                    ord_node_expr(child, "child")
                };
                writeln!(
                    src,
                    "        std::mem::discriminant(&value.{name}).hash(state);\n        \
                     if let Some(child) = &value.{name} {{\n            \
                     ops.push(HashOp::Node {{ node: TermRef::{owner}(value), from: {slot} }});\n            \
                     ops.push(HashOp::Node {{ node: {child_expr}, from: 0 }});\n            \
                     return;\n        \
                     }}",
                    owner = ord_message_arm(leaf)
                )
                .expect("write");
            }
            Shape::RepeatedMessage { leaf: child } => {
                let seq = if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    format!("TermSlice::EPathMap(&value.{name})")
                } else {
                    format!("TermSlice::{}(&value.{name})", rust_type_name(child))
                };
                writeln!(
                    src,
                    "        state.write_length_prefix(value.{name}.len());\n        \
                     ops.push(HashOp::Node {{ node: TermRef::{owner}(value), from: {slot} }});\n        \
                     ops.push(HashOp::Seq {{ values: {seq}, index: 0 }});\n        \
                     return;",
                    owner = ord_message_arm(leaf)
                )
                .expect("write");
            }
            Shape::Map { value_leaf, .. } => {
                assert_eq!(value_leaf, "Par");
                writeln!(
                    src,
                    "        state.write_length_prefix(value.{name}.len());\n        \
                     ops.push(HashOp::Node {{ node: TermRef::{owner}(value), from: {slot} }});\n        \
                     maps.push(value.{name}.iter());\n        \
                     ops.push(HashOp::Map);\n        \
                     return;",
                    owner = ord_message_arm(leaf)
                )
                .expect("write");
            }
            Shape::Oneof => {
                let key = format!("{leaf}::{name}");
                let oneof = oneof_by_field
                    .get(&key)
                    .unwrap_or_else(|| panic!("schema: Hash field `{key}` has no oneof"));
                writeln!(
                    src,
                    "        std::mem::discriminant(&value.{name}).hash(state);\n        \
                     if let Some(child) = &value.{name} {{\n            \
                     ops.push(HashOp::Node {{ node: TermRef::{owner}(value), from: {slot} }});\n            \
                     ops.push(HashOp::Node {{ node: {child}, from: 0 }});\n            \
                     return;\n        \
                     }}",
                    owner = ord_message_arm(leaf),
                    child = ord_oneof_expr(oneof, "child")
                )
                .expect("write");
            }
        }
        src.push_str("    }\n");
    }
    src.push_str("}\n\n");
}

fn emit_hash_oneof_step(src: &mut String, oneof: &Oneof, extern_set: &BTreeSet<&str>) {
    let ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
    let stem = oneof.rust_ident.to_snake_case();
    writeln!(
        src,
        "#[allow(unused_variables)]\nfn hash_step_oneof_{stem}<'a, H: std::hash::Hasher>(value: &'a {ty}, state: &mut H, ops: &mut Vec<HashOp<'a>>) {{\n    match value {{"
    )
    .expect("write");
    for variant in &oneof.variants {
        let arm = &variant.rust_ident;
        match &variant.message_leaf {
            Some(leaf) if extern_set.contains(leaf.as_str()) => {
                assert_eq!(leaf, "EPathMap");
                writeln!(
                    src,
                    "        {ty}::{arm}(child) => ops.push(HashOp::Node {{ node: TermRef::EPathMap(child), from: 0 }}),"
                )
                .expect("write");
            }
            Some(leaf) => {
                writeln!(
                    src,
                    "        {ty}::{arm}(child) => ops.push(HashOp::Node {{ node: {child}, from: 0 }}),",
                    child = ord_node_expr(leaf, "child")
                )
                .expect("write");
            }
            None => {
                writeln!(src, "        {ty}::{arm}(child) => child.hash(state),").expect("write");
            }
        }
    }
    src.push_str("    }\n}\n\n");
}

#[allow(clippy::too_many_arguments)]
fn emit_hash_oracle(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    fields_of: &BTreeMap<&str, &[Field]>,
    reachable_messages: &BTreeSet<&str>,
    reachable_oneofs: &BTreeSet<&str>,
    path_of: &BTreeMap<&str, String>,
) {
    src.push_str(
        "#[cfg(test)]\nfn oracle_hash_epath_map<H: std::hash::Hasher>(value: &EPathMap, state: &mut H) {\n\
         \x20   use crate::rust::epathmap_trie_codec::EPathMapRepr;\n\
         \x20   match value.representation() {\n\
         \x20       EPathMapRepr::Empty => {}\n\
         \x20       EPathMapRepr::Set(map) => {\n\
         \x20           for path in map.read_zipper().into_path_iter() { path.hash(state); }\n\
         \x20           for (key, ()) in map.iter() { key.hash(state); }\n\
         \x20       }\n\
         \x20       EPathMapRepr::Map(map) => {\n\
         \x20           for path in map.read_zipper().into_path_iter() { path.hash(state); }\n\
         \x20           for (key, child) in map.iter() { key.hash(state); oracle_hash_par(child, state); }\n\
         \x20       }\n\
         \x20   }\n\
         \x20   value.connective_used.hash(state);\n\
         \x20   value.remainder.hash(state);\n\
         }\n\n",
    );
    for message in messages {
        let leaf = message.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        emit_hash_oracle_message(
            src,
            leaf,
            path_of[leaf].as_str(),
            fields_of[leaf],
            oneof_by_field,
            extern_set,
        );
    }
    for oneof in oneofs {
        if reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            emit_hash_oracle_oneof(src, oneof, extern_set);
        }
    }
    assert_eq!(
        plan.cut.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["Par"],
        "schema: Hash differential root must track the descriptor-derived cut set"
    );
    src.push_str(
        "#[cfg(test)]\nmod hash_pda_differential {\n\
         \x20   use super::*;\n\
         \x20   use proptest::strategy::{Strategy, ValueTree};\n\
         \x20   use proptest::test_runner::TestRunner;\n\
         \x20   use std::collections::hash_map::DefaultHasher;\n\
         \x20   use std::hash::{Hash, Hasher};\n\n\
         \x20   fn generated(value: &Par) -> u64 { let mut state = DefaultHasher::new(); value.hash(&mut state); state.finish() }\n\
         \x20   fn oracle(value: &Par) -> u64 { let mut state = DefaultHasher::new(); oracle_hash_par(value, &mut state); state.finish() }\n\n\
         \x20   #[test]\n\
         \x20   fn generated_hash_matches_the_recursive_oracle() {\n\
         \x20       let strategy = crate::rust::test_utils::test_utils::generate_par(4);\n\
         \x20       let mut runner = TestRunner::deterministic();\n\
         \x20       for _ in 0..64 {\n\
         \x20           let value = strategy.new_tree(&mut runner).expect(\"Hash strategy\").current();\n\
         \x20           assert_eq!(generated(&value), oracle(&value));\n\
         \x20           let mut ignored_metadata = value.clone();\n\
         \x20           ignored_metadata.locally_free.push(0x5A);\n\
         \x20           assert_eq!(generated(&value), generated(&ignored_metadata));\n\
         \x20           assert_eq!(oracle(&value), oracle(&ignored_metadata));\n\
         \x20       }\n\
         \x20   }\n\
         }\n\n",
    );
}

fn emit_hash_oracle_message(
    src: &mut String,
    leaf: &str,
    rust_path: &str,
    fields: &[Field],
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
) {
    let stem = rust_type_name(leaf).to_snake_case();
    let has_semantic_fields = fields.iter().any(|field| field.rust_name != "locally_free");
    let value_name = if has_semantic_fields {
        "value"
    } else {
        "_value"
    };
    let state_name = if has_semantic_fields {
        "state"
    } else {
        "_state"
    };
    writeln!(
        src,
        "#[cfg(test)]\nfn oracle_hash_{stem}<H: std::hash::Hasher>({value_name}: &{rust_path}, {state_name}: &mut H) {{"
    )
    .expect("write");
    for field in fields {
        if field.rust_name == "locally_free" {
            continue;
        }
        let name = &field.rust_name;
        match &field.shape {
            Shape::Scalar(_) | Shape::EmptyBytes | Shape::RepeatedString | Shape::RepeatedBytes => {
                writeln!(src, "    value.{name}.hash(state);").expect("write");
            }
            Shape::Message { leaf: child } => {
                let hash = if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    "oracle_hash_epath_map".to_string()
                } else {
                    format!("oracle_hash_{}", rust_type_name(child).to_snake_case())
                };
                writeln!(
                    src,
                    "    std::mem::discriminant(&value.{name}).hash(state);\n    if let Some(child) = &value.{name} {{ {hash}(child, state); }}"
                )
                .expect("write");
            }
            Shape::RepeatedMessage { leaf: child } => {
                let hash = if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    "oracle_hash_epath_map".to_string()
                } else {
                    format!("oracle_hash_{}", rust_type_name(child).to_snake_case())
                };
                writeln!(
                    src,
                    "    state.write_length_prefix(value.{name}.len());\n    for child in &value.{name} {{ {hash}(child, state); }}"
                )
                .expect("write");
            }
            Shape::Map { value_leaf, .. } => {
                assert_eq!(value_leaf, "Par");
                writeln!(
                    src,
                    "    state.write_length_prefix(value.{name}.len());\n    for (key, child) in &value.{name} {{ key.hash(state); oracle_hash_par(child, state); }}"
                )
                .expect("write");
            }
            Shape::Oneof => {
                let key = format!("{leaf}::{name}");
                let oneof = oneof_by_field
                    .get(&key)
                    .unwrap_or_else(|| panic!("schema: Hash oracle field `{key}` has no oneof"));
                let hash = format!("oracle_hash_oneof_{}", oneof.rust_ident.to_snake_case());
                writeln!(
                    src,
                    "    std::mem::discriminant(&value.{name}).hash(state);\n    if let Some(child) = &value.{name} {{ {hash}(child, state); }}"
                )
                .expect("write");
            }
        }
    }
    src.push_str("}\n\n");
}

fn emit_hash_oracle_oneof(src: &mut String, oneof: &Oneof, extern_set: &BTreeSet<&str>) {
    let ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
    let stem = oneof.rust_ident.to_snake_case();
    writeln!(
        src,
        "#[cfg(test)]\nfn oracle_hash_oneof_{stem}<H: std::hash::Hasher>(value: &{ty}, state: &mut H) {{\n    match value {{"
    )
    .expect("write");
    for variant in &oneof.variants {
        let arm = &variant.rust_ident;
        let statement = match &variant.message_leaf {
            Some(leaf) if extern_set.contains(leaf.as_str()) => {
                assert_eq!(leaf, "EPathMap");
                "oracle_hash_epath_map(child, state)".to_string()
            }
            Some(leaf) => format!(
                "oracle_hash_{}(child, state)",
                rust_type_name(leaf).to_snake_case()
            ),
            None => "child.hash(state)".to_string(),
        };
        writeln!(src, "        {ty}::{arm}(child) => {statement},").expect("write");
    }
    src.push_str("    }\n}\n\n");
}

#[allow(clippy::too_many_arguments)]
fn emit_ord_oracle(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    plan: &ClonePlan,
    oneof_by_field: &BTreeMap<String, &Oneof>,
    fields_of: &BTreeMap<&str, &[Field]>,
    reachable_messages: &BTreeSet<&str>,
    reachable_oneofs: &BTreeSet<&str>,
    path_of: &BTreeMap<&str, String>,
) {
    src.push_str(
        "#[cfg(test)]\n\
         fn oracle_ord_epath_map(left: &EPathMap, right: &EPathMap) -> std::cmp::Ordering {\n\
         use crate::rust::epathmap_trie_codec::EPathMapRepr;\n\
             let entries = match (left.representation(), right.representation()) {\n\
                 (EPathMapRepr::Empty, EPathMapRepr::Empty) => std::cmp::Ordering::Equal,\n\
                 (EPathMapRepr::Set(left), EPathMapRepr::Set(right)) => {\n\
                     let topology = left.read_zipper().into_path_iter().cmp(right.read_zipper().into_path_iter());\n\
                     if topology != std::cmp::Ordering::Equal { return topology; }\n\
                     let mut left = left.iter();\n\
                     let mut right = right.iter();\n\
                     loop {\n\
                         match (left.next(), right.next()) {\n\
                             (None, None) => break std::cmp::Ordering::Equal,\n\
                             (None, Some(_)) => break std::cmp::Ordering::Less,\n\
                             (Some(_), None) => break std::cmp::Ordering::Greater,\n\
                             (Some((left_key, ())), Some((right_key, ()))) => {\n\
                                 let ordering = left_key.cmp(&right_key);\n\
                                 if ordering != std::cmp::Ordering::Equal { break ordering; }\n\
                             }\n\
                         }\n\
                     }\n\
                 }\n\
                 (EPathMapRepr::Map(left), EPathMapRepr::Map(right)) => {\n\
                     let topology = left.read_zipper().into_path_iter().cmp(right.read_zipper().into_path_iter());\n\
                     if topology != std::cmp::Ordering::Equal { return topology; }\n\
                     let mut left = left.iter();\n\
                     let mut right = right.iter();\n\
                     loop {\n\
                         match (left.next(), right.next()) {\n\
                             (None, None) => break std::cmp::Ordering::Equal,\n\
                             (None, Some(_)) => break std::cmp::Ordering::Less,\n\
                             (Some(_), None) => break std::cmp::Ordering::Greater,\n\
                             (Some((left_key, left_value)), Some((right_key, right_value))) => {\n\
                                 let key_order = left_key.cmp(&right_key);\n\
                                 if key_order != std::cmp::Ordering::Equal { break key_order; }\n\
                                 let value_order = oracle_ord_par(left_value, right_value);\n\
                                 if value_order != std::cmp::Ordering::Equal { break value_order; }\n\
                             }\n\
                         }\n\
                     }\n\
                 }\n\
                 _ => (left.mode() as u8).cmp(&(right.mode() as u8)),\n\
             };\n\
             entries\n\
                 .then_with(|| left.locally_free.cmp(&right.locally_free))\n\
                 .then_with(|| left.connective_used.cmp(&right.connective_used))\n\
                 .then_with(|| left.remainder.cmp(&right.remainder))\n\
         }\n\n",
    );

    for msg in messages {
        let leaf = msg.leaf_name();
        if extern_set.contains(leaf) || !reachable_messages.contains(leaf) {
            continue;
        }
        emit_ord_oracle_message(
            src,
            leaf,
            path_of[leaf].as_str(),
            fields_of[leaf],
            oneof_by_field,
            extern_set,
        );
    }
    for oneof in oneofs {
        if reachable_oneofs.contains(oneof.rust_ident.as_str()) {
            emit_ord_oracle_oneof(src, oneof, extern_set);
        }
    }

    assert_eq!(
        plan.cut.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["Par"],
        "schema: the generated Ord differential currently names Par as its root; \
         widen the generated test roots when the descriptor-derived cut set changes"
    );
    src.push_str(
        "#[cfg(test)]\n\
         mod ord_pda_differential {\n\
             use super::*;\n\
             use proptest::strategy::{Strategy, ValueTree};\n\
             use proptest::test_runner::TestRunner;\n\n\
             #[test]\n\
             fn generated_ord_matches_the_recursive_oracle() {\n\
                 let strategy = crate::rust::test_utils::test_utils::generate_par(4);\n\
                 let mut runner = TestRunner::deterministic();\n\
                 let mut values = Vec::with_capacity(48);\n\
                 for _ in 0..48 {\n\
                     values.push(\n\
                         strategy\n\
                             .new_tree(&mut runner)\n\
                             .expect(\"Ord differential strategy produces a value\")\n\
                             .current(),\n\
                     );\n\
                 }\n\
                 assert!(values.iter().any(|value| !value.exprs.is_empty()));\n\
                 assert!(values.iter().any(|value| {\n\
                     !value.sends.is_empty()\n\
                         || !value.receives.is_empty()\n\
                         || !value.news.is_empty()\n\
                         || !value.matches.is_empty()\n\
                         || !value.bundles.is_empty()\n\
                         || !value.connectives.is_empty()\n\
                 }));\n\
                 for left in &values {\n\
                     for right in &values {\n\
                         assert_eq!(\n\
                             left.cmp(right),\n\
                             oracle_ord_par(left, right),\n\
                             \"generated Ord diverged from the recursive descriptor oracle\",\n\
                         );\n\
                     }\n\
                 }\n\
             }\n\n\
             #[test]\n\
             fn generated_ord_is_stack_safe_at_depth_4096() {\n\
                 std::thread::Builder::new()\n\
                     .stack_size(256 * 1024)\n\
                     .spawn(|| {\n\
                         fn nested(depth: usize, leaf: i64) -> Par {\n\
                             let mut value = Par::default();\n\
                             value.exprs.push(Expr {\n\
                                 expr_instance: Some(ExprInstance::GInt(leaf)),\n\
                             });\n\
                             for _ in 0..depth {\n\
                                 let mut list = EList::default();\n\
                                 list.ps.push(value);\n\
                                 let mut parent = Par::default();\n\
                                 parent.exprs.push(Expr {\n\
                                     expr_instance: Some(ExprInstance::EListBody(list)),\n\
                                 });\n\
                                 value = parent;\n\
                             }\n\
                             value\n\
                         }\n\
                         let left = nested(4096, 0);\n\
                         let right = nested(4096, 1);\n\
                         assert_eq!(left.cmp(&right), std::cmp::Ordering::Less);\n\
                         crate::rust::rholang::par_children::dismantle(left);\n\
                         crate::rust::rholang::par_children::dismantle(right);\n\
                     })\n\
                     .expect(\"spawn Ord depth test\")\n\
                     .join()\n\
                     .expect(\"Ord depth test panicked\");\n\
             }\n\
         }\n\n",
    );
}

fn emit_ord_oracle_message(
    src: &mut String,
    leaf: &str,
    rust_path: &str,
    fields: &[Field],
    oneof_by_field: &BTreeMap<String, &Oneof>,
    extern_set: &BTreeSet<&str>,
) {
    let stem = rust_type_name(leaf).to_snake_case();
    let left_name = if fields.is_empty() { "_left" } else { "left" };
    let right_name = if fields.is_empty() { "_right" } else { "right" };
    writeln!(
        src,
        "#[cfg(test)]\nfn oracle_ord_{stem}({left_name}: &{rust_path}, {right_name}: &{rust_path}) -> std::cmp::Ordering {{"
    )
    .expect("write");
    for field in fields {
        let name = &field.rust_name;
        match &field.shape {
            Shape::Scalar(_) | Shape::EmptyBytes | Shape::RepeatedString | Shape::RepeatedBytes => {
                writeln!(
                    src,
                    "    let ordering = left.{name}.cmp(&right.{name});\n    \
                     if ordering != std::cmp::Ordering::Equal {{ return ordering; }}"
                )
                .expect("write");
            }
            Shape::Message { leaf: child } => {
                let compare = if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    "oracle_ord_epath_map".to_string()
                } else {
                    format!("oracle_ord_{}", rust_type_name(child).to_snake_case())
                };
                writeln!(
                    src,
                    "    let ordering = match (&left.{name}, &right.{name}) {{\n        \
                     (None, None) => std::cmp::Ordering::Equal,\n        \
                     (None, Some(_)) => std::cmp::Ordering::Less,\n        \
                     (Some(_), None) => std::cmp::Ordering::Greater,\n        \
                     (Some(left_child), Some(right_child)) => {compare}(left_child, right_child),\n    \
                     }};\n    \
                     if ordering != std::cmp::Ordering::Equal {{ return ordering; }}"
                )
                .expect("write");
            }
            Shape::RepeatedMessage { leaf: child } => {
                if extern_set.contains(child.as_str()) {
                    assert_eq!(child, "EPathMap");
                    writeln!(
                        src,
                        "    for (left_child, right_child) in left.{name}.iter().zip(&right.{name}) {{\n        \
                         let ordering = oracle_ord_epath_map(left_child, right_child);\n        \
                         if ordering != std::cmp::Ordering::Equal {{ return ordering; }}\n    \
                         }}"
                    )
                    .expect("write");
                } else {
                    let child_stem = rust_type_name(child).to_snake_case();
                    writeln!(
                        src,
                        "    for (left_child, right_child) in left.{name}.iter().zip(&right.{name}) {{\n        \
                         let ordering = oracle_ord_{child_stem}(left_child, right_child);\n        \
                         if ordering != std::cmp::Ordering::Equal {{ return ordering; }}\n    \
                         }}"
                    )
                    .expect("write");
                }
                writeln!(
                    src,
                    "    let ordering = left.{name}.len().cmp(&right.{name}.len());\n    \
                     if ordering != std::cmp::Ordering::Equal {{ return ordering; }}"
                )
                .expect("write");
            }
            Shape::Map { value_leaf, .. } => {
                assert_eq!(value_leaf, "Par");
                writeln!(
                    src,
                    "    let mut left_iter = left.{name}.iter();\n    \
                     let mut right_iter = right.{name}.iter();\n    \
                     loop {{\n        \
                     match (left_iter.next(), right_iter.next()) {{\n            \
                     (None, None) => break,\n            \
                     (None, Some(_)) => return std::cmp::Ordering::Less,\n            \
                     (Some(_), None) => return std::cmp::Ordering::Greater,\n            \
                     (Some((left_key, left_value)), Some((right_key, right_value))) => {{\n                \
                     let ordering = left_key.cmp(right_key);\n                \
                     if ordering != std::cmp::Ordering::Equal {{ return ordering; }}\n                \
                     let ordering = oracle_ord_par(left_value, right_value);\n                \
                     if ordering != std::cmp::Ordering::Equal {{ return ordering; }}\n            \
                     }}\n        \
                     }}\n    \
                     }}"
                )
                .expect("write");
            }
            Shape::Oneof => {
                let key = format!("{leaf}::{name}");
                let oneof = oneof_by_field
                    .get(&key)
                    .unwrap_or_else(|| panic!("schema: Ord oracle field `{key}` has no oneof"));
                let oneof_stem = oneof.rust_ident.to_snake_case();
                writeln!(
                    src,
                    "    let ordering = match (&left.{name}, &right.{name}) {{\n        \
                     (None, None) => std::cmp::Ordering::Equal,\n        \
                     (None, Some(_)) => std::cmp::Ordering::Less,\n        \
                     (Some(_), None) => std::cmp::Ordering::Greater,\n        \
                     (Some(left_child), Some(right_child)) => oracle_ord_oneof_{oneof_stem}(left_child, right_child),\n    \
                     }};\n    \
                     if ordering != std::cmp::Ordering::Equal {{ return ordering; }}"
                )
                .expect("write");
            }
        }
    }
    src.push_str("    std::cmp::Ordering::Equal\n}\n\n");
}

fn emit_ord_oracle_oneof(src: &mut String, oneof: &Oneof, extern_set: &BTreeSet<&str>) {
    let ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
    let stem = oneof.rust_ident.to_snake_case();
    writeln!(
        src,
        "#[cfg(test)]\nfn oracle_ord_oneof_{stem}(left: &{ty}, right: &{ty}) -> std::cmp::Ordering {{\n    \
         let rank_order = ord_rank_oneof_{stem}(left).cmp(&ord_rank_oneof_{stem}(right));\n    \
         if rank_order != std::cmp::Ordering::Equal {{ return rank_order; }}\n    \
         match (left, right) {{"
    )
    .expect("write");
    for variant in &oneof.variants {
        let arm = &variant.rust_ident;
        match &variant.message_leaf {
            Some(leaf) if extern_set.contains(leaf.as_str()) => {
                assert_eq!(leaf, "EPathMap");
                writeln!(
                    src,
                    "        ({ty}::{arm}(left), {ty}::{arm}(right)) => oracle_ord_epath_map(left, right),"
                )
                .expect("write");
            }
            Some(leaf) => {
                let child_stem = rust_type_name(leaf).to_snake_case();
                writeln!(
                    src,
                    "        ({ty}::{arm}(left), {ty}::{arm}(right)) => oracle_ord_{child_stem}(left, right),"
                )
                .expect("write");
            }
            None => {
                writeln!(
                    src,
                    "        ({ty}::{arm}(left), {ty}::{arm}(right)) => left.cmp(right),"
                )
                .expect("write");
            }
        }
    }
    src.push_str(
        "        _ => unreachable!(\"equal oneof ranks named different variants\"),\n\
         \x20   }\n\
         }\n\n",
    );
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
/// replacement, because the replacement was a new function (`bincode_encoder::encode`
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
         // `bincode_encoder` (which sits beside a still-derived `Serialize`, and the gate\n\
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
            .unwrap_or_else(|| panic!("schema: `{leaf}` has no resolved fields"));
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
                            panic!("schema: no oneof answers to `{key}` for the oracle")
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
            writeln!(
                src,
                "        {enum_ty}::{arm}(v) => {enum_ty}::{arm}({expr}),"
            )
            .expect("write");
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
         /// root plus `Arc` bumps on the canonical EPM1 snapshot and layout caches.\n\
         /// `EPathMap::clone` never re-enters a driven clone.\n\
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
        "// @generated by models/codegen/schema.rs from the protobuf FileDescriptorSet.\n\
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
         /// `models/codegen/schema.rs`'s closed `DERIVE_DISPOSITIONS` table, so the\n\
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
            reaches_clone_cut_set: plan.entered.contains(msg.leaf_name()),
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
            reaches_clone_cut_set: plan.oneof_entered.contains(ty),
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
         \x20   (\"Drop::drop\", \"generated `impl Drop for Par` delegates to `par_children::dismantle_in_place`\"),\n\
         ];\n\n",
    );

    let disposition_traits = DERIVE_DISPOSITIONS
        .iter()
        .map(|d| format!("    \"{}\",", d.token))
        .collect::<Vec<_>>()
        .join("\n");
    writeln!(
        src,
        "/// The closed set of `#[derive]` tokens `codegen/schema.rs` dispositions.\n\
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

// ===========================================================================
// §8  ★★ THE PROTOBUF DESERIALIZER — iterative skipper + schema PDA
// ===========================================================================
//
// ## Why this output is GENERATED and not a checked-in source file
//
// Fifty-seven `rhoapi` messages are regenerated from the descriptor on every
// build. A hand-written decoder table drifts from the derive silently — the
// derive gains a field, the hand table does not, and the divergence surfaces as
// a mis-decode rather than as a compile error. The iterative unknown-field
// skipper and the schema-dependent heterogeneous decoder PDA are therefore
// emitted together from the same descriptor walk as the other generated
// drivers. Production `Message::merge_field` implementations delegate to this
// machine; this module is not a dormant staging artefact.

/// Emit the protobuf deserializer's runtime support.
///
/// The skipper itself is schema-independent. The same emission pass appends
/// per-message arms from `(messages, resolved, oneofs, extern_set)`, so the
/// unknown-field and known-field walks cannot drift into separate generated
/// modules or lifecycle stages.
fn emit_protobuf_decoder_source(
    messages: &[Message<'_>],
    resolved: &[(usize, Vec<Field>)],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
) -> String {
    let mut src = String::from(
        r##"// @generated by models/codegen/schema.rs (§8). DO NOT EDIT.
// Regenerate by touching models/codegen/schema.rs — models/build.rs emits a
// `cargo:rerun-if-changed` for it, so an edit here cannot leave a stale copy in
// `OUT_DIR` while the build reports success.
//
// ===========================================================================
// THE GENERATED PROTOBUF DESERIALIZER — ITERATIVE SKIPPER + MESSAGE PDA
// ===========================================================================
//
// This is production code. Generated `Message::merge_field` implementations
// enter the heterogeneous decoder PDA below, whose unknown-tag arm calls
// `skip_unknown_field`. Exactness against prost is retained as an independent
// differential even though the recursive prost walk is no longer production.
//
// ## What it is
//
// An ITERATIVE re-implementation of `prost::encoding::skip_field`
// (`prost-0.14.3/src/encoding.rs:166-199`). prost's version is self-recursive:
// its `StartGroup` arm (`:187`) calls itself once per nesting level, and the
// nesting level is chosen by whoever sent the bytes. This version keeps the
// same walk and moves the frames onto an explicit `Vec<u32>` of open group
// tags, so its stack consumption is O(1) in the input.
//
// ## Why groups matter when the schema has none
//
// proto3 has no group syntax, so no `rhoapi` message can ever legitimately
// carry wire type 3. That is a statement about what this node WRITES, not
// about what it READS. Skipping an *unknown* field is a walk over bytes that a
// peer chose, and a peer is free to send `StartGroup` keys forever. The
// unknown-field path is therefore a recursive descent whose depth is entirely
// attacker-controlled and entirely independent of the known-field walk that
// the schema bounds.
//
// ## The exactness contract
//
// `skip_unknown_field` accepts exactly what `prost::encoding::skip_field`
// accepts and rejects exactly what it rejects, returning `DecodeError` values
// that compare EQUAL to prost's, and leaving the buffer at the same position
// in both the accepting and the rejecting case. It differs in exactly two
// ways, both deliberate:
//
//   1. it does not recurse; and
//   2. it consults no depth budget, so it has no `RecursionLimitReached`.
//
// (2) is a CONSEQUENCE of (1). prost's `ctx.limit_reached()?` at `:172` exists
// to stop the recursion at `:187` before the machine stack runs out; with the
// recursion gone the budget is guarding nothing, and this workspace's standing
// ruling is that artificial limits are removed outright rather than retained
// as decoration.
//
// ⚠ prost's own recursive `skip_field` STAYS in the tree, and stays
// recursion-limited. It is safe precisely because that limit still exists. The
// limit and the recursion come out together or not at all — never in opposite
// orders.
//
// ## Termination, and why the bound is not an artificial limit
//
// `|depth_stack|` is bounded by `|input|`: a tag is pushed only when a
// `StartGroup` key has been read, and a key costs at least one byte, so a
// buffer of `n` bytes can open at most `n` groups. The loop makes progress on
// every iteration because every arm either consumes at least one byte or pops
// a tag that a previously consumed byte pushed. There is no constant ceiling
// anywhere in this function, and adding one would re-introduce exactly the
// artificial limit that removing the recursion exists to retire.
//
// ## Space
//
// `Vec::new()`, not `Vec::with_capacity(_)`. The workspace preallocates when
// the size is known — and here it IS known, and it is ZERO: proto3 emits no
// groups, so no message this node produces can make the vector allocate. The
// allocation happens only for a peer that sent a group, which is the case this
// function exists to survive rather than the case it is tuned for. The cost is
// then 4 bytes per open group against at least 1 input byte per open group,
// i.e. O(|input|) heap in place of O(|input|) machine stack — heap being the
// one of the two that can fail without aborting the process.
//
// ## Why the two error values are MINTED by calling prost
//
// `prost::error::DecodeErrorKind` is `pub(crate)` (`prost-0.14.3/src/error.rs:105`)
// and `DecodeError`'s only public constructors produce `Other { description }`.
// So a `BufferUnderflow` written out here by hand would be a DIFFERENT VALUE
// from prost's under `PartialEq` — the exactness contract above would be false
// and no test could tell, because the two render to similar text.
//
// The two values are therefore obtained from prost itself, by calling
// `skip_field` on a constant input chosen to reach the arm that produces each:
//
// | wanted | call | prost's path |
// |---|---|---|
// | `BufferUnderflow` | `skip_field(ThirtyTwoBit, MIN_TAG, &mut &[][..], _)` | `len = 4`, `4 > 0` at `:193` |
// | `UnexpectedEndGroupTag` | `skip_field(EndGroup, MIN_TAG, &mut &[][..], _)` | the `EndGroup` arm at `:190` |
//
// Both are non-recursive, allocation-free apart from the error itself, and
// reached in constant time; both are `#[cold]` and live only on the error path.
// Making the value BY CONSTRUCTION rather than by transcription is what lets
// the gate assert equality instead of asserting a rendering.
//
// ⚠ The remaining error values need no minting: `decode_key` and
// `decode_varint` are prost's own public functions and their errors
// (`InvalidVarint`, `InvalidKey`, `InvalidTag`, `InvalidWireType`) propagate
// through `?` unaltered, exactly as they do inside `skip_field`.

use prost::bytes::{Buf, Bytes};
use prost::encoding::{decode_key, decode_varint, DecodeContext, WireType, MIN_TAG};
use prost::DecodeError;

/// Skip one unknown protobuf field, iteratively.
///
/// Reads and discards the field whose key was `(tag, protobuf_wire_type)`, advancing
/// `buf` past its payload. A `StartGroup` field is skipped through to its
/// matching `EndGroup`, however deeply the groups nest, WITHOUT recursion and
/// WITHOUT a depth budget.
///
/// # Errors
///
/// Exactly `prost::encoding::skip_field`'s errors, as EQUAL values, minus
/// `RecursionLimitReached` which this function cannot produce:
///
/// * `UnexpectedEndGroupTag` — `protobuf_wire_type` is `EndGroup`, or a group closed
///   with a tag other than the one that opened it;
/// * `BufferUnderflow` — the field's payload runs past the end of `buf`;
/// * whatever `decode_varint` / `decode_key` return for a malformed key or
///   length (`InvalidVarint`, `InvalidKey`, `InvalidTag`, `InvalidWireType`).
///
/// # Panics
///
/// Does not panic. `Buf::advance` is called only after the same
/// `len > buf.remaining()` guard prost applies at `encoding.rs:193`.
pub fn skip_unknown_field(
    protobuf_wire_type: WireType,
    tag: u32,
    buf: &mut impl Buf,
) -> Result<(), DecodeError> {
    // ★ THE EXPLICIT STACK — the tags of the groups currently open, outermost
    // first. This IS prost's recursion, with the frames named: prost's
    // `skip_field(_, tag, _, ctx.enter_recursion())` carries `tag` down the
    // machine stack so the matching `EndGroup` can be checked against it, and
    // `tag` is the only thing a frame carries. So one `u32` per frame is not a
    // summary of the recursion — it is the whole of it.
    //
    // Empty for every message this node writes (proto3 emits no groups), so
    // the legitimate path never allocates.
    let mut depth_stack: Vec<u32> = Vec::new();

    // The field currently being skipped. Rebound rather than shadowed by a
    // recursive call.
    let mut protobuf_wire_type = protobuf_wire_type;
    let mut tag = tag;

    loop {
        // ── ONE field, exactly prost's `match` at `encoding.rs:173-191` ──
        let len: u64 = match protobuf_wire_type {
            WireType::Varint => {
                decode_varint(buf)?;
                0
            }
            WireType::ThirtyTwoBit => 4,
            WireType::SixtyFourBit => 8,
            WireType::LengthDelimited => decode_varint(buf)?,
            // prost `:190`. Reached only for the field this call was ENTERED
            // on: an inner `EndGroup` is handled by the key loop below and
            // never reaches here, exactly as prost's inner `match` at `:181`
            // intercepts it before the recursive call at `:187`.
            WireType::EndGroup => return Err(unexpected_end_group_tag()),
            // prost `:178`. Where prost recurses, this pushes.
            //
            // `len` is 0 because prost's `StartGroup` arm `break 0`s: the group
            // consumes its bytes through the key loop, not through `advance`.
            WireType::StartGroup => {
                depth_stack.push(tag);
                0
            }
        };

        // prost `:193-197`, applied to every arm including the two that reach
        // it with `len == 0` — `advance(0)` is a no-op and the comparison is
        // false, so keeping the shape identical costs nothing and leaves one
        // fewer difference to argue about.
        if len > buf.remaining() as u64 {
            return Err(buffer_underflow());
        }
        buf.advance(len as usize);

        // ── the enclosing group's key loop, prost `:178-189` ──
        //
        // Reached when the field above is finished. In prost this is the point
        // where the recursive call RETURNS and the caller's `loop` reads the
        // next key; here it is the same loop, entered from the same place.
        loop {
            // No open group ⇒ the field this call was entered on is complete.
            // In prost this is the outermost `skip_field` returning `Ok(())`.
            let Some(&open) = depth_stack.last() else {
                return Ok(());
            };

            let (inner_tag, inner_wire_type) = decode_key(buf)?;
            if inner_wire_type == WireType::EndGroup {
                // prost `:181-186`: the tag must match the one that opened
                // THIS group. Popping first would lose the tag the check needs.
                if inner_tag != open {
                    return Err(unexpected_end_group_tag());
                }
                depth_stack
                    .pop()
                    .expect("skip_unknown_field: `last()` returned Some, so `pop()` cannot be None");
                // The group is closed. In prost, that `skip_field` invocation
                // now returns `Ok(())` into ITS caller's key loop — which is
                // this same loop, one open group shallower.
                continue;
            }

            // prost `:187`, the recursive call, as a rebinding.
            protobuf_wire_type = inner_wire_type;
            tag = inner_tag;
            break;
        }
    }
}

/// prost's own `BufferUnderflow`, obtained from prost.
///
/// See this module's header for why the value is minted rather than written:
/// `DecodeErrorKind` is `pub(crate)`, so a hand-built error would not compare
/// equal to the one `skip_field` produces.
///
/// `ThirtyTwoBit` on an empty buffer takes prost's `len = 4` arm and fails the
/// `len > buf.remaining()` check at `encoding.rs:193` with `remaining() == 0`.
/// No recursion, no group, no allocation beyond the error itself.
#[cold]
#[inline(never)]
fn buffer_underflow() -> DecodeError {
    let mut empty: &[u8] = &[];
    prost::encoding::skip_field(
        WireType::ThirtyTwoBit,
        MIN_TAG,
        &mut empty,
        DecodeContext::default(),
    )
    .expect_err(
        "skip_unknown_field: `skip_field(ThirtyTwoBit, .., <empty>)` must fail with \
         BufferUnderflow — a 4-byte fixed field cannot be read from a 0-byte buffer. \
         If this succeeded, prost's `encoding.rs:193` guard has changed shape and the \
         error values this module mints are no longer the ones it produces.",
    )
}

/// prost's own `UnexpectedEndGroupTag`, obtained from prost. See
/// [`buffer_underflow`] for the rationale.
///
/// prost's `EndGroup` arm at `encoding.rs:190` returns this unconditionally,
/// before reading anything, so the empty buffer is never touched.
#[cold]
#[inline(never)]
fn unexpected_end_group_tag() -> DecodeError {
    let mut empty: &[u8] = &[];
    prost::encoding::skip_field(
        WireType::EndGroup,
        MIN_TAG,
        &mut empty,
        DecodeContext::default(),
    )
    .expect_err(
        "skip_unknown_field: `skip_field(EndGroup, ..)` must fail with UnexpectedEndGroupTag \
         — prost's `encoding.rs:190` returns it unconditionally. If this succeeded, the \
         error values this module mints are no longer the ones prost produces.",
    )
}
"##,
    );

    emit_protobuf_decoder_pda(&mut src, messages, resolved, oneofs, extern_set);

    // ★ THE NON-VACUITY FLOOR, at the emitter. `models/build.rs` refuses a
    // zero-byte output; this refuses an output that is present but does not
    // carry the item `models/src/rust/rholang/mod.rs` includes it FOR. The
    // `include!` would fail to compile, but its message names `OUT_DIR` and a
    // missing function rather than the emitter that stopped emitting it.
    assert!(
        src.contains("pub fn skip_unknown_field("),
        "schema §8: the protobuf-decoder emitter produced {} bytes without \
         `pub fn skip_unknown_field(`. That is the only item this output exists to \
         carry at S0.",
        src.len()
    );

    src
}

/// Emit the schema-dependent half of the protobuf reader.
///
/// The generated machine owns a heterogeneous frame stack. A nested message is
/// represented by a Node frame followed by an Attach continuation, so native
/// call-stack use is constant even when the protobuf term is arbitrarily deep.
/// The same resolved Field and Oneof values that drive the encoders drive every
/// arm below; adding a schema member therefore regenerates the reader instead
/// of silently falling through a hand-maintained table.
fn emit_protobuf_decoder_pda(
    src: &mut String,
    messages: &[Message<'_>],
    resolved: &[(usize, Vec<Field>)],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
) {
    let rows = walk_messages(messages, resolved, extern_set);
    assert_eq!(
        extern_set.iter().copied().collect::<Vec<_>>(),
        vec!["EPathMap"],
        "protobuf decoder: every extern message needs an explicit iterative node and teardown"
    );

    src.push_str(
        r#"

// ===========================================================================
// THE GENERATED HETEROGENEOUS PROTOBUF PDA
// ===========================================================================
//
// A frame is either a message currently consuming keys, or the continuation
// which attaches a completed child to its parent. Length-delimited messages use
// an ABSOLUTE remaining-byte boundary on the shared Buf, exactly like prost
// merge_loop. A child is intentionally allowed to cross its boundary; the
// child's next boundary check then returns prost's DelimitedLengthExceeded and
// the pending Attach frames reconstruct prost's error-location stack.

use std::mem;
use prost::encoding;
use crate::rhoapi::Par;
use crate::rust::epathmap_trie_codec::OwnedPendingEpmDecode;
use crate::rust::rhoapi_ext::EPathMap;
use crate::rust::rholang::par_children::dismantle_all;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DecodeStats {
    pub max_frames: usize,
    pub max_values: usize,
}

enum Node {
"#,
    );
    for (msg, fields) in &rows {
        if fields.is_none() {
            continue;
        }
        let variant = rust_type_name(msg.leaf_name());
        let ty = msg.rust_path();
        writeln!(
            src,
            "    {variant} {{ value: crate::rhoapi::{ty}, limit: usize }},"
        )
        .expect("write protobuf decoder Node");
    }
    src.push_str(
        "    EPathMap { value: EPathMap, limit: usize },\n\
             MapStringPar { key: String, value: Par, limit: usize },\n\
         }\n\n\
         enum Value {\n",
    );
    for (msg, fields) in &rows {
        if fields.is_none() {
            continue;
        }
        let variant = rust_type_name(msg.leaf_name());
        let ty = msg.rust_path();
        writeln!(src, "    {variant}(crate::rhoapi::{ty}),").expect("write protobuf decoder Value");
    }
    src.push_str(
        "    EPathMap(EPathMap),\n\
             MapStringPar(String, Par),\n\
         }\n\n\
         enum Attach {\n",
    );

    for (msg, fields) in &rows {
        let Some(fields) = fields else { continue };
        let owner = rust_type_name(msg.leaf_name());
        let owner_ty = msg.rust_path();
        for field in fields.iter() {
            match &field.shape {
                Shape::Message { .. } | Shape::RepeatedMessage { .. } | Shape::Map { .. } => {
                    let name = decoder_attach_name(&owner, &field.rust_name, None);
                    writeln!(
                        src,
                        "    {name} {{ parent: crate::rhoapi::{owner_ty}, limit: usize }},"
                    )
                    .expect("write protobuf decoder Attach");
                }
                Shape::Oneof => {
                    let oneof = decoder_oneof(messages, oneofs, msg.leaf_name(), &field.rust_name);
                    for variant in oneof
                        .variants
                        .iter()
                        .filter(|variant| variant.message_leaf.is_some())
                    {
                        let name = decoder_attach_name(
                            &owner,
                            &field.rust_name,
                            Some(&variant.rust_ident),
                        );
                        writeln!(
                            src,
                            "    {name} {{ parent: crate::rhoapi::{owner_ty}, limit: usize }},"
                        )
                        .expect("write protobuf decoder oneof Attach");
                    }
                }
                Shape::Scalar(_)
                | Shape::EmptyBytes
                | Shape::RepeatedString
                | Shape::RepeatedBytes => {}
            }
        }
    }
    src.push_str(
        "    EPathMapPs { parent: EPathMap, limit: usize },\n\
             EPathMapRemainder { parent: EPathMap, limit: usize },\n\
             EPathMapSnapshotValue { frame: EpmFrame },\n\
             MapStringParValue { key: String, limit: usize },\n\
         }\n\n\
         struct EpmFrame {\n\
             parent: EPathMap,\n\
             limit: usize,\n\
             pending: OwnedPendingEpmDecode,\n\
             validate_canonical: bool,\n\
         }\n\n\
         enum Frame {\n\
             Node(Node),\n\
             Attach(Attach),\n\
             Epm(EpmFrame),\n\
         }\n\n",
    );

    emit_protobuf_decoder_node_impl(src, &rows);
    emit_protobuf_decoder_attach_impl(src, messages, oneofs, &rows);
    emit_protobuf_decoder_machine(src, messages, oneofs, &rows);
    emit_protobuf_decoder_salvage(src, messages, oneofs, &rows);
    emit_protobuf_decoder_entries(src, &rows);

    let generated_count = rows.iter().filter(|(_, fields)| fields.is_some()).count();
    writeln!(
        src,
        "\npub const GENERATED_PROTOBUF_MESSAGE_COUNT: usize = {generated_count};\n\
         pub const GENERATED_PROTOBUF_ONEOF_COUNT: usize = {};\n\
         pub const GENERATED_PROTOBUF_NODE_SIZE: usize = mem::size_of::<Node>();\n\
         pub const GENERATED_PROTOBUF_VALUE_SIZE: usize = mem::size_of::<Value>();\n\
         pub const GENERATED_PROTOBUF_ATTACH_SIZE: usize = mem::size_of::<Attach>();\n\
         pub const GENERATED_PROTOBUF_FRAME_SIZE: usize = mem::size_of::<Frame>();",
        oneofs.len()
    )
    .expect("write protobuf decoder counts");
    assert_eq!(
        generated_count + extern_set.len(),
        messages.len(),
        "protobuf decoder: every descriptor message is generated or explicitly extern"
    );
}

fn decoder_attach_name(owner: &str, field: &str, variant: Option<&str>) -> String {
    let field = field.strip_prefix("r#").unwrap_or(field);
    match variant {
        Some(variant) => format!("{owner}{}{}", rust_type_name(field), variant),
        None => format!("{owner}{}", rust_type_name(field)),
    }
}

fn decoder_oneof<'a>(
    messages: &[Message<'_>],
    oneofs: &'a [Oneof],
    owner: &str,
    field: &str,
) -> &'a Oneof {
    oneofs
        .iter()
        .find(|oneof| {
            owner_of_oneof(messages, oneof) == owner && rust_field_name(&oneof.proto_name) == field
        })
        .unwrap_or_else(|| {
            panic!("protobuf decoder: {owner}.{field} is Shape::Oneof but has no resolved Oneof")
        })
}

fn emit_protobuf_decoder_node_impl(src: &mut String, rows: &[(&Message<'_>, Option<&[Field]>)]) {
    src.push_str(
        "impl Node {\n\
             fn into_value(self) -> Value {\n\
                 match self {\n",
    );
    for (msg, fields) in rows {
        if fields.is_none() {
            continue;
        }
        let variant = rust_type_name(msg.leaf_name());
        writeln!(
            src,
            "            Node::{variant} {{ value, limit: _ }} => Value::{variant}(value),"
        )
        .expect("write Node::into_value");
    }
    src.push_str(
        "            Node::EPathMap { value, limit: _ } => Value::EPathMap(value),\n\
                     Node::MapStringPar { key, value, limit: _ } => \
                            Value::MapStringPar(key, value),\n\
                 }\n\
             }\n\
         }\n\n\
         impl Value {\n\
             fn kind_name(&self) -> &'static str {\n\
                 match self {\n",
    );
    for (msg, fields) in rows {
        if fields.is_none() {
            continue;
        }
        let variant = rust_type_name(msg.leaf_name());
        writeln!(src, "            Value::{variant}(_) => \"{variant}\",")
            .expect("write Value::kind_name");
    }
    src.push_str(
        "            Value::EPathMap(_) => \"EPathMap\",\n\
                     Value::MapStringPar(_, _) => \"MapStringPar\",\n\
                 }\n\
             }\n\
         }\n\n",
    );
}

fn emit_protobuf_decoder_attach_impl(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    rows: &[(&Message<'_>, Option<&[Field]>)],
) {
    src.push_str(
        "impl Attach {\n\
             fn location(&self) -> Option<(&'static str, &'static str)> {\n\
                 match self {\n",
    );
    for (msg, fields) in rows {
        let Some(fields) = fields else { continue };
        let owner = rust_type_name(msg.leaf_name());
        for field in fields.iter() {
            let field_name = field
                .rust_name
                .strip_prefix("r#")
                .unwrap_or(&field.rust_name);
            match &field.shape {
                Shape::Message { .. } | Shape::RepeatedMessage { .. } | Shape::Map { .. } => {
                    let attach = decoder_attach_name(&owner, &field.rust_name, None);
                    writeln!(
                        src,
                        "            Attach::{attach} {{ .. }} => Some((\"{}\", \"{field_name}\")),",
                        msg.leaf_name()
                    )
                    .expect("write Attach::location");
                }
                Shape::Oneof => {
                    let oneof = decoder_oneof(messages, oneofs, msg.leaf_name(), &field.rust_name);
                    for variant in oneof
                        .variants
                        .iter()
                        .filter(|variant| variant.message_leaf.is_some())
                    {
                        let attach = decoder_attach_name(
                            &owner,
                            &field.rust_name,
                            Some(&variant.rust_ident),
                        );
                        writeln!(
                            src,
                            "            Attach::{attach} {{ .. }} => Some((\"{}\", \"{field_name}\")),",
                            msg.leaf_name()
                        )
                        .expect("write oneof Attach::location");
                    }
                }
                Shape::Scalar(_)
                | Shape::EmptyBytes
                | Shape::RepeatedString
                | Shape::RepeatedBytes => {}
            }
        }
    }
    src.push_str(
        "            Attach::EPathMapPs { .. } => Some((\"EPathMap\", \"ps\")),\n\
                     Attach::EPathMapRemainder { .. } => \
                            Some((\"EPathMap\", \"remainder\")),\n\
                     Attach::EPathMapSnapshotValue { .. } => \
                            Some((\"EPathMap\", \"trie_snapshot\")),\n\
                     Attach::MapStringParValue { .. } => None,\n\
                 }\n\
             }\n\n\
             fn into_parent_value(self) -> Option<Value> {\n\
                 match self {\n",
    );
    for (msg, fields) in rows {
        let Some(fields) = fields else { continue };
        let owner = rust_type_name(msg.leaf_name());
        for field in fields.iter() {
            match &field.shape {
                Shape::Message { .. } | Shape::RepeatedMessage { .. } | Shape::Map { .. } => {
                    let attach = decoder_attach_name(&owner, &field.rust_name, None);
                    writeln!(
                        src,
                        "            Attach::{attach} {{ parent, limit: _ }} => Some(Value::{owner}(parent)),"
                    )
                    .expect("write Attach::into_parent_value");
                }
                Shape::Oneof => {
                    let oneof = decoder_oneof(messages, oneofs, msg.leaf_name(), &field.rust_name);
                    for variant in oneof
                        .variants
                        .iter()
                        .filter(|variant| variant.message_leaf.is_some())
                    {
                        let attach = decoder_attach_name(
                            &owner,
                            &field.rust_name,
                            Some(&variant.rust_ident),
                        );
                        writeln!(
                            src,
                            "            Attach::{attach} {{ parent, limit: _ }} => Some(Value::{owner}(parent)),"
                        )
                        .expect("write oneof Attach::into_parent_value");
                    }
                }
                Shape::Scalar(_)
                | Shape::EmptyBytes
                | Shape::RepeatedString
                | Shape::RepeatedBytes => {}
            }
        }
    }
    src.push_str(
        "            Attach::EPathMapPs { parent, limit: _ }\n\
                     | Attach::EPathMapRemainder { parent, limit: _ } => \
                            Some(Value::EPathMap(parent)),\n\
                     Attach::EPathMapSnapshotValue { frame } => \
                            Some(Value::EPathMap(frame.parent)),\n\
                     Attach::MapStringParValue { key, limit: _ } => \
                            {{ drop(key); None }},\n\
                 }\n\
             }\n\
         }\n\n",
    );
}

fn emit_protobuf_decoder_machine(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    rows: &[(&Message<'_>, Option<&[Field]>)],
) {
    src.push_str(
        r#"enum DecoderInput<B> {
    Root(B),
    Nested(Bytes),
}

impl<B: Buf> Buf for DecoderInput<B> {
    fn remaining(&self) -> usize {
        match self {
            DecoderInput::Root(buf) => buf.remaining(),
            DecoderInput::Nested(buf) => buf.remaining(),
        }
    }

    fn chunk(&self) -> &[u8] {
        match self {
            DecoderInput::Root(buf) => buf.chunk(),
            DecoderInput::Nested(buf) => buf.chunk(),
        }
    }

    fn advance(&mut self, count: usize) {
        match self {
            DecoderInput::Root(buf) => buf.advance(count),
            DecoderInput::Nested(buf) => buf.advance(count),
        }
    }
}

struct Machine<B> {
    buf: DecoderInput<B>,
    input_stack: Vec<DecoderInput<B>>,
    epm_depth: usize,
    frames: Vec<Frame>,
    values: Vec<Value>,
    garbage: Vec<Value>,
    stats: DecodeStats,
}

impl<B: Buf> Machine<B> {
    fn new(buf: B, root: Node) -> Self {
        Machine {
            buf: DecoderInput::Root(buf),
            input_stack: Vec::new(),
            epm_depth: 0,
            frames: vec![Frame::Node(root)],
            values: Vec::new(),
            garbage: Vec::new(),
            stats: DecodeStats {
                max_frames: 1,
                max_values: 0,
            },
        }
    }

    fn message_limit(&mut self, protobuf_wire_type: WireType) -> Result<usize, DecodeError> {
        encoding::check_wire_type(WireType::LengthDelimited, protobuf_wire_type)?;
        self.unchecked_delimited_limit()
    }

    fn bytes_region(&mut self, protobuf_wire_type: WireType) -> Result<Bytes, DecodeError> {
        encoding::check_wire_type(WireType::LengthDelimited, protobuf_wire_type)?;
        let len = decode_varint(&mut self.buf)?;
        if len > self.buf.remaining() as u64 {
            return Err(buffer_underflow());
        }
        let len = len as usize;
        Ok(match &mut self.buf {
            DecoderInput::Root(buf) => buf.copy_to_bytes(len),
            DecoderInput::Nested(buf) => buf.split_to(len),
        })
    }

    fn enter_input(&mut self, input: Bytes) {
        let outer = mem::replace(&mut self.buf, DecoderInput::Nested(input));
        self.input_stack.push(outer);
    }

    fn leave_input(&mut self) {
        assert_eq!(
            self.buf.remaining(),
            0,
            "protobuf decoder: an EPM1 map value did not consume its complete protobuf body",
        );
        self.buf = self
            .input_stack
            .pop()
            .expect("protobuf decoder: EPM1 value input has no saved parent buffer");
    }

    // Prost's generated map-field arm calls `btree_map::merge`/`hash_map::merge`,
    // whose API has no WireType parameter. It therefore reads the entry length
    // without checking the field's wire type. Error precedence is observable
    // on malformed input, so the PDA preserves that transition exactly.
    fn unchecked_delimited_limit(&mut self) -> Result<usize, DecodeError> {
        let len = decode_varint(&mut self.buf)?;
        let remaining = self.buf.remaining();
        if len > remaining as u64 {
            return Err(buffer_underflow());
        }
        Ok(remaining - len as usize)
    }

    fn run(&mut self) -> Result<(), DecodeError> {
        while let Some(frame) = self.frames.pop() {
            let result = match frame {
                Frame::Node(node) => self.step_node(node),
                Frame::Attach(attach) => self.attach(attach),
                Frame::Epm(frame) => self.step_epm_snapshot(frame),
            };
            if let Err(error) = result {
                return Err(self.decorate_pending(error));
            }
            self.stats.max_frames = self.stats.max_frames.max(self.frames.len());
            self.stats.max_values = self.stats.max_values.max(self.values.len());
        }
        assert!(self.input_stack.is_empty(), "protobuf decoder: nested input outlived its EPM1 value");
        assert_eq!(self.epm_depth, 0, "protobuf decoder: EPM1 validation depth did not return to zero");
        Ok(())
    }

    fn decorate_pending(&self, mut error: DecodeError) -> DecodeError {
        // Nearest continuation first, matching nested map_err calls while the
        // recursive prost implementation unwinds.
        for frame in self.frames.iter().rev() {
            if let Frame::Attach(attach) = frame {
                if let Some((message, field)) = attach.location() {
                    error.push(message, field);
                }
            }
        }
        error
    }

    fn step_node(&mut self, node: Node) -> Result<(), DecodeError> {
        match node {
"#,
    );

    for (msg, fields) in rows {
        if fields.is_none() {
            continue;
        }
        let owner = rust_type_name(msg.leaf_name());
        let method = msg.leaf_name().to_snake_case();
        writeln!(
            src,
            "            Node::{owner} {{ value, limit }} => self.step_{method}(value, limit),"
        )
        .expect("write thin protobuf node dispatch");
    }
    src.push_str(
        "            Node::EPathMap { value, limit } => self.step_epath_map(value, limit),\n\
                     Node::MapStringPar { key, value, limit } => \
                         self.step_map_string_par(key, value, limit),\n\
                 }\n\
             }\n\
             \n",
    );

    for (msg, fields) in rows {
        let Some(fields) = fields else { continue };
        emit_protobuf_decoder_message_step(src, messages, oneofs, msg, fields);
    }
    emit_protobuf_decoder_epathmap_step(src);
    emit_protobuf_decoder_map_step(src);
    emit_protobuf_decoder_attach_arms(src, messages, oneofs, rows);
    src.push_str("}\n\n");
}

fn emit_protobuf_decoder_message_step(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    msg: &Message<'_>,
    fields: &[Field],
) {
    let owner = rust_type_name(msg.leaf_name());
    let method = msg.leaf_name().to_snake_case();
    let ty = msg.rust_path();
    let mut routes = String::new();
    let mut methods = String::new();
    for field in fields {
        emit_protobuf_decoder_field_step(&mut routes, &mut methods, messages, oneofs, msg, field);
    }
    writeln!(
        src,
        "    #[inline(never)]\n\
         fn step_{method}(\n\
             &mut self,\n\
             mut value: crate::rhoapi::{ty},\n\
             limit: usize,\n\
         ) -> Result<(), DecodeError> {{\n\
             match self.buf.remaining().cmp(&limit) {{\n\
                             std::cmp::Ordering::Equal => {{\n\
                                 self.values.push(Value::{owner}(value));\n\
                                 return Ok(());\n\
                             }}\n\
                             std::cmp::Ordering::Less => return Err(delimited_length_exceeded()),\n\
                             std::cmp::Ordering::Greater => {{}}\n\
                         }}\n\
             let (tag, protobuf_wire_type) = decode_key(&mut self.buf)?;\n\
             match tag {{\n\
         {routes}\n\
                 _ => {{\n\
                     skip_unknown_field(protobuf_wire_type, tag, &mut self.buf)?;\n\
                     self.frames.push(Frame::Node(Node::{owner} {{\n\
                         value: mem::take(&mut value), limit,\n\
                     }}));\n\
                     Ok(())\n\
                 }}\n\
             }}\n\
         }}\n"
    )
    .expect("write message step header");
    src.push_str(&methods);
}

fn decoder_location_map(owner: &str, field: &str) -> String {
    let field = field.strip_prefix("r#").unwrap_or(field);
    format!(".map_err(|mut error| {{ error.push(\"{owner}\", \"{field}\"); error }})?")
}

fn emit_protobuf_decoder_field_step(
    routes: &mut String,
    methods: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    msg: &Message<'_>,
    field: &Field,
) {
    let owner = rust_type_name(msg.leaf_name());
    let owner_ty = format!("crate::rhoapi::{}", msg.rust_path());
    let name = &field.rust_name;
    let location = decoder_location_map(msg.leaf_name(), name);
    let method = format!(
        "step_{}_tag_{}",
        msg.leaf_name().to_snake_case(),
        field.min_tag()
    );
    writeln!(
        routes,
        "                {}u32 => self.{method}(&mut value, limit, protobuf_wire_type),",
        field.min_tag()
    )
    .expect("write protobuf field route");
    match &field.shape {
        Shape::Scalar(_) | Shape::EmptyBytes => {
            let ty = match &field.shape {
                Shape::Scalar(ty) => *ty,
                Shape::EmptyBytes => Type::Bytes,
                _ => unreachable!("guarded scalar shape"),
            };
            let (module, _) = protobuf_scalar_module_and_default(ty);
            writeln!(
                methods,
                "    #[inline(never)]\n\
                 fn {method}(&mut self, value: &mut {owner_ty}, limit: usize, protobuf_wire_type: WireType) -> Result<(), DecodeError> {{\n\
                                         encoding::{module}::merge(\n\
                                             protobuf_wire_type, &mut value.{name}, &mut self.buf,\n\
                                             DecodeContext::default(),\n\
                                         ){location};\n\
                                         self.frames.push(Frame::Node(Node::{owner} {{ value: mem::take(value), limit }}));\n\
                                         Ok(())\n\
                                     }}"
            )
            .expect("write scalar decode arm");
        }
        Shape::RepeatedString => {
            writeln!(
                methods,
                "    #[inline(never)]\n\
                 fn {method}(&mut self, value: &mut {owner_ty}, limit: usize, protobuf_wire_type: WireType) -> Result<(), DecodeError> {{\n\
                                         encoding::string::merge_repeated(\n\
                                             protobuf_wire_type, &mut value.{name}, &mut self.buf,\n\
                                             DecodeContext::default(),\n\
                                         ){location};\n\
                                         self.frames.push(Frame::Node(Node::{owner} {{ value: mem::take(value), limit }}));\n\
                                         Ok(())\n\
                                     }}"
            )
            .expect("write repeated string arm");
        }
        Shape::RepeatedBytes => {
            writeln!(
                methods,
                "    #[inline(never)]\n\
                 fn {method}(&mut self, value: &mut {owner_ty}, limit: usize, protobuf_wire_type: WireType) -> Result<(), DecodeError> {{\n\
                                         encoding::bytes::merge_repeated(\n\
                                             protobuf_wire_type, &mut value.{name}, &mut self.buf,\n\
                                             DecodeContext::default(),\n\
                                         ){location};\n\
                                         self.frames.push(Frame::Node(Node::{owner} {{ value: mem::take(value), limit }}));\n\
                                         Ok(())\n\
                                     }}"
            )
            .expect("write repeated bytes arm");
        }
        Shape::Message { leaf } => {
            let child = rust_type_name(leaf);
            let attach = decoder_attach_name(&owner, name, None);
            writeln!(
                methods,
                "    #[inline(never)]\n\
                 fn {method}(&mut self, value: &mut {owner_ty}, limit: usize, protobuf_wire_type: WireType) -> Result<(), DecodeError> {{\n\
                                         let child_limit = self.message_limit(protobuf_wire_type){location};\n\
                                         let child = value.{name}.take().unwrap_or_default();\n\
                                         self.frames.push(Frame::Attach(Attach::{attach} {{ parent: mem::take(value), limit }}));\n\
                                         self.frames.push(Frame::Node(Node::{child} {{ value: child, limit: child_limit }}));\n\
                                         Ok(())\n\
                                     }}"
            )
            .expect("write singular message arm");
        }
        Shape::RepeatedMessage { leaf } => {
            let child = rust_type_name(leaf);
            let attach = decoder_attach_name(&owner, name, None);
            writeln!(
                methods,
                "    #[inline(never)]\n\
                 fn {method}(&mut self, value: &mut {owner_ty}, limit: usize, protobuf_wire_type: WireType) -> Result<(), DecodeError> {{\n\
                                         let child_limit = self.message_limit(protobuf_wire_type){location};\n\
                                         self.frames.push(Frame::Attach(Attach::{attach} {{ parent: mem::take(value), limit }}));\n\
                                         self.frames.push(Frame::Node(Node::{child} {{ value: Default::default(), limit: child_limit }}));\n\
                                         Ok(())\n\
                                     }}"
            )
            .expect("write repeated message arm");
        }
        Shape::Map { key, value_leaf } => {
            assert_eq!(
                *key,
                Type::String,
                "protobuf decoder supports the resolved rhoapi string-key map"
            );
            assert_eq!(
                value_leaf, "Par",
                "protobuf decoder synthetic map node is map<string, Par>"
            );
            let attach = decoder_attach_name(&owner, name, None);
            writeln!(
                methods,
                "    #[inline(never)]\n\
                 fn {method}(&mut self, value: &mut {owner_ty}, limit: usize, _protobuf_wire_type: WireType) -> Result<(), DecodeError> {{\n\
                                         let child_limit = self.unchecked_delimited_limit(){location};\n\
                                         self.frames.push(Frame::Attach(Attach::{attach} {{ parent: mem::take(value), limit }}));\n\
                                         self.frames.push(Frame::Node(Node::MapStringPar {{\n\
                                             key: String::new(), value: Par::default(), limit: child_limit,\n\
                                         }}));\n\
                                         Ok(())\n\
                                     }}"
            )
            .expect("write map arm");
        }
        Shape::Oneof => {
            let oneof = decoder_oneof(messages, oneofs, msg.leaf_name(), name);
            // A oneof owns one protobuf tag per variant rather than one tag for
            // the containing Rust field. Replace the placeholder route above
            // with one generated route and method for each variant.
            let placeholder = format!(
                "                {}u32 => self.{method}(&mut value, limit, protobuf_wire_type),\n",
                field.min_tag()
            );
            debug_assert!(routes.ends_with(&placeholder));
            routes.truncate(routes.len() - placeholder.len());
            emit_protobuf_decoder_oneof_steps(routes, methods, msg, field, oneof);
        }
    }
}

fn decoder_oneof_salvage_fn(oneof: &Oneof) -> String {
    format!(
        "salvage_{}_{}",
        oneof.module.replace("::", "_"),
        oneof.rust_ident.to_snake_case()
    )
}

fn emit_protobuf_decoder_oneof_steps(
    routes: &mut String,
    methods: &mut String,
    msg: &Message<'_>,
    field: &Field,
    oneof: &Oneof,
) {
    let owner = rust_type_name(msg.leaf_name());
    let name = &field.rust_name;
    let field_name = name.strip_prefix("r#").unwrap_or(name);
    let oneof_ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
    let salvage = decoder_oneof_salvage_fn(oneof);
    for variant in &oneof.variants {
        let location = decoder_location_map(msg.leaf_name(), field_name);
        let method = format!(
            "step_{}_tag_{}",
            msg.leaf_name().to_snake_case(),
            variant.tag
        );
        writeln!(
            routes,
            "                {}u32 => self.{method}(&mut value, limit, protobuf_wire_type),",
            variant.tag
        )
        .expect("write protobuf oneof route");
        if let Some(leaf) = &variant.message_leaf {
            let child = rust_type_name(leaf);
            let attach = decoder_attach_name(&owner, name, Some(&variant.rust_ident));
            writeln!(
                methods,
                "    #[inline(never)]\n\
                 fn {method}(&mut self, value: &mut crate::rhoapi::{}, limit: usize, protobuf_wire_type: WireType) -> Result<(), DecodeError> {{\n\
                                         let child_limit = self.message_limit(protobuf_wire_type){location};\n\
                                         let child = match value.{name}.take() {{\n\
                                             Some({oneof_ty}::{}(child)) => child,\n\
                                             old => {{ {salvage}(old, &mut self.garbage); Default::default() }}\n\
                                         }};\n\
                                         self.frames.push(Frame::Attach(Attach::{attach} {{ parent: mem::take(value), limit }}));\n\
                                         self.frames.push(Frame::Node(Node::{child} {{ value: child, limit: child_limit }}));\n\
                                         Ok(())\n\
                                     }}",
                msg.rust_path(),
                variant.rust_ident
            )
            .expect("write message oneof arm");
        } else {
            let (module, _) = protobuf_scalar_module_and_default(variant.ty);
            writeln!(
                methods,
                "    #[inline(never)]\n\
                 fn {method}(&mut self, value: &mut crate::rhoapi::{}, limit: usize, protobuf_wire_type: WireType) -> Result<(), DecodeError> {{\n\
                                         let mut payload = Default::default();\n\
                                         encoding::{module}::merge(\n\
                                             protobuf_wire_type, &mut payload, &mut self.buf,\n\
                                             DecodeContext::default(),\n\
                                         ){location};\n\
                                         let old = value.{name}.replace({oneof_ty}::{}(payload));\n\
                                         {salvage}(old, &mut self.garbage);\n\
                                         self.frames.push(Frame::Node(Node::{owner} {{ value: mem::take(value), limit }}));\n\
                                         Ok(())\n\
                                     }}",
                msg.rust_path(),
                variant.rust_ident
            )
            .expect("write scalar oneof arm");
        }
    }
}

fn emit_protobuf_decoder_epathmap_step(src: &mut String) {
    src.push_str(
        r#"    #[inline(never)]
    fn step_epath_map(
        &mut self,
        mut value: EPathMap,
        limit: usize,
    ) -> Result<(), DecodeError> {
                match self.buf.remaining().cmp(&limit) {
                    std::cmp::Ordering::Equal => {
                        self.values.push(Value::EPathMap(value));
                        return Ok(());
                    }
                    std::cmp::Ordering::Less => return Err(delimited_length_exceeded()),
                    std::cmp::Ordering::Greater => {}
                }
                let (tag, protobuf_wire_type) = decode_key(&mut self.buf)?;
                match tag {
                    1u32 => {
                        let child_limit = self.message_limit(protobuf_wire_type)
                            .map_err(|mut error| {
                                error.push("EPathMap", "ps");
                                error
                            })?;
                        self.frames.push(Frame::Attach(Attach::EPathMapPs {
                            parent: value,
                            limit,
                        }));
                        self.frames.push(Frame::Node(Node::Par {
                            value: Par::default(),
                            limit: child_limit,
                        }));
                    }
                    3u32 => {
                        encoding::bytes::merge(
                            protobuf_wire_type,
                            &mut value.locally_free,
                            &mut self.buf,
                            DecodeContext::default(),
                        )
                        .map_err(|mut error| {
                            error.push("EPathMap", "locally_free");
                            error
                        })?;
                        self.frames.push(Frame::Node(Node::EPathMap { value, limit }));
                    }
                    4u32 => {
                        encoding::bool::merge(
                            protobuf_wire_type,
                            &mut value.connective_used,
                            &mut self.buf,
                            DecodeContext::default(),
                        )
                        .map_err(|mut error| {
                            error.push("EPathMap", "connective_used");
                            error
                        })?;
                        self.frames.push(Frame::Node(Node::EPathMap { value, limit }));
                    }
                    5u32 => {
                        let child_limit = self.message_limit(protobuf_wire_type)
                            .map_err(|mut error| {
                                error.push("EPathMap", "remainder");
                                error
                            })?;
                        let child = value.remainder.take().unwrap_or_default();
                        self.frames.push(Frame::Attach(Attach::EPathMapRemainder {
                            parent: value,
                            limit,
                        }));
                        self.frames.push(Frame::Node(Node::Var {
                            value: child,
                            limit: child_limit,
                        }));
                    }
                    8u32 => {
                        let mut region = Vec::new();
                        encoding::bytes::merge(
                            protobuf_wire_type,
                            &mut region,
                            &mut self.buf,
                            DecodeContext::default(),
                        )
                        .map_err(|mut error| {
                            error.push("EPathMap", "serialized_paths");
                            error
                        })?;
                        value.merge_serialized_paths(&region)?;
                        self.frames.push(Frame::Node(Node::EPathMap { value, limit }));
                    }
                    9u32 => {
                        let region = self.bytes_region(protobuf_wire_type).map_err(|mut error| {
                            error.push("EPathMap", "trie_snapshot");
                            error
                        })?;
                        let pending = OwnedPendingEpmDecode::new(region).map_err(|error| {
                            let mut error: DecodeError = error.into();
                            error.push("EPathMap", "trie_snapshot");
                            error
                        })?;
                        let validate_canonical = self.epm_depth == 0;
                        self.epm_depth += 1;
                        self.frames.push(Frame::Epm(EpmFrame {
                            parent: value,
                            limit,
                            pending,
                            validate_canonical,
                        }));
                    }
                    _ => {
                        skip_unknown_field(protobuf_wire_type, tag, &mut self.buf)?;
                        self.frames.push(Frame::Node(Node::EPathMap { value, limit }));
                    }
                }
                Ok(())
            }

    #[inline(never)]
    fn step_epm_snapshot(&mut self, mut frame: EpmFrame) -> Result<(), DecodeError> {
        let next = frame.pending.next_value_bytes().map_err(|error| {
            let mut error: DecodeError = error.into();
            error.push("EPathMap", "trie_snapshot");
            error
        })?;
        if let Some(body) = next {
            self.enter_input(body);
            self.frames.push(Frame::Attach(Attach::EPathMapSnapshotValue { frame }));
            self.frames.push(Frame::Node(Node::Par {
                value: Par::default(),
                limit: 0,
            }));
            return Ok(());
        }

        let repr = frame
            .pending
            .finish(frame.validate_canonical)
            .map_err(|error| {
                let mut error: DecodeError = error.into();
                error.push("EPathMap", "trie_snapshot");
                error
            })?;
        frame.parent.replace_decoded_representation(repr)?;
        self.epm_depth = self
            .epm_depth
            .checked_sub(1)
            .expect("protobuf decoder: EPM1 completion without an active validation frame");
        self.frames.push(Frame::Node(Node::EPathMap {
            value: frame.parent,
            limit: frame.limit,
        }));
        Ok(())
    }
"#,
    );
}

fn emit_protobuf_decoder_map_step(src: &mut String) {
    src.push_str(
        r#"    #[inline(never)]
    fn step_map_string_par(
        &mut self,
        mut key: String,
        value: Par,
        limit: usize,
    ) -> Result<(), DecodeError> {
                match self.buf.remaining().cmp(&limit) {
                    std::cmp::Ordering::Equal => {
                        self.values.push(Value::MapStringPar(key, value));
                        return Ok(());
                    }
                    std::cmp::Ordering::Less => return Err(delimited_length_exceeded()),
                    std::cmp::Ordering::Greater => {}
                }
                let (tag, protobuf_wire_type) = decode_key(&mut self.buf)?;
                match tag {
                    1u32 => {
                        encoding::string::merge(
                            protobuf_wire_type,
                            &mut key,
                            &mut self.buf,
                            DecodeContext::default(),
                        )?;
                        self.frames.push(Frame::Node(Node::MapStringPar {
                            key,
                            value,
                            limit,
                        }));
                    }
                    2u32 => {
                        let child_limit = self.message_limit(protobuf_wire_type)?;
                        self.frames.push(Frame::Attach(Attach::MapStringParValue {
                            key,
                            limit,
                        }));
                        self.frames.push(Frame::Node(Node::Par {
                            value,
                            limit: child_limit,
                        }));
                    }
                    _ => {
                        skip_unknown_field(protobuf_wire_type, tag, &mut self.buf)?;
                        self.frames.push(Frame::Node(Node::MapStringPar {
                            key,
                            value,
                            limit,
                        }));
                    }
                }
                Ok(())
            }
"#,
    );
}

fn emit_protobuf_decoder_attach_arms(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    rows: &[(&Message<'_>, Option<&[Field]>)],
) {
    let mut routes = String::new();
    let mut methods = String::new();

    for (msg, fields) in rows {
        let Some(fields) = fields else { continue };
        let owner = rust_type_name(msg.leaf_name());
        let owner_ty = format!("crate::rhoapi::{}", msg.rust_path());
        for field in fields.iter() {
            let name = &field.rust_name;
            let display_name = name.strip_prefix("r#").unwrap_or(name);
            let location = format!("{}.{display_name}", msg.leaf_name());
            match &field.shape {
                Shape::Message { leaf } => {
                    let child = rust_type_name(leaf);
                    let attach = decoder_attach_name(&owner, name, None);
                    let method = format!("attach_{}", attach.to_snake_case());
                    writeln!(
                        routes,
                        "            Attach::{attach} {{ parent, limit }} => self.{method}(parent, limit),"
                    )
                    .expect("write singular attach route");
                    let success = format!(
                        "                parent.{name} = Some(child);\n\
                                         self.frames.push(Frame::Node(Node::{owner} {{ value: parent, limit }}));"
                    );
                    emit_protobuf_decoder_attach_method(
                        &mut methods,
                        &method,
                        &format!("mut parent: {owner_ty}, limit: usize"),
                        &format!("Value::{child}(child)"),
                        &success,
                        &location,
                        &format!("Some(Value::{owner}(parent))"),
                    );
                }
                Shape::RepeatedMessage { leaf } => {
                    let child = rust_type_name(leaf);
                    let attach = decoder_attach_name(&owner, name, None);
                    let method = format!("attach_{}", attach.to_snake_case());
                    writeln!(
                        routes,
                        "            Attach::{attach} {{ parent, limit }} => self.{method}(parent, limit),"
                    )
                    .expect("write repeated attach route");
                    let success = format!(
                        "                parent.{name}.push(child);\n\
                                         self.frames.push(Frame::Node(Node::{owner} {{ value: parent, limit }}));"
                    );
                    emit_protobuf_decoder_attach_method(
                        &mut methods,
                        &method,
                        &format!("mut parent: {owner_ty}, limit: usize"),
                        &format!("Value::{child}(child)"),
                        &success,
                        &location,
                        &format!("Some(Value::{owner}(parent))"),
                    );
                }
                Shape::Map { .. } => {
                    let attach = decoder_attach_name(&owner, name, None);
                    let method = format!("attach_{}", attach.to_snake_case());
                    writeln!(
                        routes,
                        "            Attach::{attach} {{ parent, limit }} => self.{method}(parent, limit),"
                    )
                    .expect("write map attach route");
                    let success = format!(
                        "                if let Some(old) = parent.{name}.insert(key, child) {{\n\
                                             self.garbage.push(Value::Par(old));\n\
                                         }}\n\
                                         self.frames.push(Frame::Node(Node::{owner} {{ value: parent, limit }}));"
                    );
                    emit_protobuf_decoder_attach_method(
                        &mut methods,
                        &method,
                        &format!("mut parent: {owner_ty}, limit: usize"),
                        "Value::MapStringPar(key, child)",
                        &success,
                        &location,
                        &format!("Some(Value::{owner}(parent))"),
                    );
                }
                Shape::Oneof => {
                    let oneof = decoder_oneof(messages, oneofs, msg.leaf_name(), name);
                    let oneof_ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
                    for variant in oneof
                        .variants
                        .iter()
                        .filter(|variant| variant.message_leaf.is_some())
                    {
                        let child =
                            rust_type_name(variant.message_leaf.as_deref().expect("filtered Some"));
                        let attach = decoder_attach_name(&owner, name, Some(&variant.rust_ident));
                        let method = format!("attach_{}", attach.to_snake_case());
                        writeln!(
                            routes,
                            "            Attach::{attach} {{ parent, limit }} => self.{method}(parent, limit),"
                        )
                        .expect("write oneof attach route");
                        let success = format!(
                            "                parent.{name} = Some({oneof_ty}::{}(child));\n\
                                             self.frames.push(Frame::Node(Node::{owner} {{ value: parent, limit }}));",
                            variant.rust_ident
                        );
                        emit_protobuf_decoder_attach_method(
                            &mut methods,
                            &method,
                            &format!("mut parent: {owner_ty}, limit: usize"),
                            &format!("Value::{child}(child)"),
                            &success,
                            &location,
                            &format!("Some(Value::{owner}(parent))"),
                        );
                    }
                }
                Shape::Scalar(_)
                | Shape::EmptyBytes
                | Shape::RepeatedString
                | Shape::RepeatedBytes => {}
            }
        }
    }

    routes.push_str(
        "            Attach::EPathMapPs { parent, limit } => \
                         self.attach_epath_map_ps(parent, limit),\n\
                     Attach::EPathMapRemainder { parent, limit } => \
                         self.attach_epath_map_remainder(parent, limit),\n\
                     Attach::EPathMapSnapshotValue { frame } => \
                         self.attach_epath_map_snapshot_value(frame),\n\
                     Attach::MapStringParValue { key, limit } => \
                         self.attach_map_string_par_value(key, limit),\n",
    );
    emit_protobuf_decoder_attach_method(
        &mut methods,
        "attach_epath_map_ps",
        "mut parent: EPathMap, limit: usize",
        "Value::Par(child)",
        "                let consumed = parent.try_insert_entry_replacing(child)?;\n\
                         self.garbage.push(Value::Par(consumed));\n\
                         self.frames.push(Frame::Node(Node::EPathMap { value: parent, limit }));",
        "EPathMap.ps",
        "Some(Value::EPathMap(parent))",
    );
    emit_protobuf_decoder_attach_method(
        &mut methods,
        "attach_epath_map_remainder",
        "mut parent: EPathMap, limit: usize",
        "Value::Var(child)",
        "                parent.remainder = Some(child);\n\
                         self.frames.push(Frame::Node(Node::EPathMap { value: parent, limit }));",
        "EPathMap.remainder",
        "Some(Value::EPathMap(parent))",
    );
    methods.push_str(
        r#"    #[inline(never)]
    fn attach_epath_map_snapshot_value(
        &mut self,
        mut frame: EpmFrame,
    ) -> Result<(), DecodeError> {
        let child = self.pop_attached_value();
        self.leave_input();
        match child {
            Value::Par(child) => {
                frame.pending.push_value(child).map_err(|error| {
                    let mut error: DecodeError = error.into();
                    error.push("EPathMap", "trie_snapshot");
                    error
                })?;
                self.frames.push(Frame::Epm(frame));
                Ok(())
            }
            child => self.attach_mismatch(
                "EPathMap.trie_snapshot Par value",
                Some(Value::EPathMap(frame.parent)),
                child,
            ),
        }
    }

"#,
    );
    emit_protobuf_decoder_attach_method(
        &mut methods,
        "attach_map_string_par_value",
        "key: String, limit: usize",
        "Value::Par(child)",
        "                self.frames.push(Frame::Node(Node::MapStringPar {\n\
                             key, value: child, limit,\n\
                         }));",
        "map<string, Par>.value",
        "None",
    );

    src.push_str(
        "    #[inline(never)]\n\
         fn attach(&mut self, attach: Attach) -> Result<(), DecodeError> {\n\
             match attach {\n",
    );
    src.push_str(&routes);
    src.push_str("        }\n    }\n\n");
    src.push_str(
        "    #[inline]\n\
         fn pop_attached_value(&mut self) -> Value {\n\
             self.values.pop().expect(\n\
                 \"protobuf decoder: an Attach frame must follow one completed child\",\n\
             )\n\
         }\n\n",
    );
    src.push_str(&methods);
    src.push_str(
        "    #[cold]\n\
         #[inline(never)]\n\
         fn attach_mismatch(\n\
             &mut self,\n\
             expected: &'static str,\n\
             parent: Option<Value>,\n\
             child: Value,\n\
         ) -> ! {\n\
             let found = child.kind_name();\n\
             if let Some(parent) = parent {\n\
                 self.garbage.push(parent);\n\
             }\n\
             self.garbage.push(child);\n\
             panic!(\"protobuf decoder invariant: {expected} received {found}\");\n\
         }\n\n",
    );
}

fn emit_protobuf_decoder_attach_method(
    methods: &mut String,
    method: &str,
    parameters: &str,
    expected_pattern: &str,
    success: &str,
    location: &str,
    parent_on_error: &str,
) {
    writeln!(
        methods,
        "    #[inline(never)]\n\
         fn {method}(&mut self, {parameters}) -> Result<(), DecodeError> {{\n\
             let child = self.pop_attached_value();\n\
             match child {{\n\
                 {expected_pattern} => {{\n\
         {success}\n\
                     Ok(())\n\
                 }}\n\
                 child => self.attach_mismatch({location:?}, {parent_on_error}, child),\n\
             }}\n\
         }}\n"
    )
    .expect("write split protobuf attach method");
}

fn emit_protobuf_decoder_salvage(
    src: &mut String,
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    rows: &[(&Message<'_>, Option<&[Field]>)],
) {
    for oneof in oneofs {
        let function = decoder_oneof_salvage_fn(oneof);
        let oneof_ty = format!("crate::rhoapi::{}::{}", oneof.module, oneof.rust_ident);
        writeln!(
            src,
            "fn {function}(value: Option<{oneof_ty}>, work: &mut Vec<Value>) {{\n\
                 let Some(value) = value else {{ return }};\n\
                 match value {{"
        )
        .expect("write oneof salvage header");
        for variant in &oneof.variants {
            if let Some(leaf) = &variant.message_leaf {
                let child = rust_type_name(leaf);
                writeln!(
                    src,
                    "        {oneof_ty}::{}(child) => work.push(Value::{child}(child)),",
                    variant.rust_ident
                )
                .expect("write oneof salvage message");
            } else {
                writeln!(
                    src,
                    "        {oneof_ty}::{}(_bounded) => {{}},",
                    variant.rust_ident
                )
                .expect("write oneof salvage scalar");
            }
        }
        src.push_str("    }\n}\n\n");
    }

    src.push_str(
        r#"impl<B> Drop for Machine<B> {
    fn drop(&mut self) {
        self.dismantle();
    }
}

impl<B> Machine<B> {
    fn is_fully_drained(&self) -> bool {
        self.frames.is_empty() && self.values.is_empty() && self.garbage.is_empty()
    }

    fn dismantle(&mut self) {
        if self.is_fully_drained() {
            return;
        }

        let mut work = mem::take(&mut self.values);
        work.append(&mut self.garbage);
        for frame in mem::take(&mut self.frames) {
            match frame {
                Frame::Node(node) => work.push(node.into_value()),
                Frame::Attach(attach) => {
                    if let Some(parent) = attach.into_parent_value() {
                        work.push(parent);
                    }
                }
                Frame::Epm(frame) => work.push(Value::EPathMap(frame.parent)),
            }
        }

        let mut pars = Vec::new();
        while let Some(value) = work.pop() {
            match value {
"#,
    );
    for (msg, fields) in rows {
        let Some(fields) = fields else { continue };
        let owner = rust_type_name(msg.leaf_name());
        if msg.leaf_name() == "Par" {
            src.push_str("                Value::Par(value) => pars.push(value),\n");
            continue;
        }
        let ty = msg.rust_path();
        writeln!(
            src,
            "                Value::{owner}(value) => {{\n\
                                 let crate::rhoapi::{ty} {{"
        )
        .expect("write salvage destructure header");
        for field in fields.iter() {
            let name = &field.rust_name;
            match field.shape {
                Shape::Message { .. }
                | Shape::RepeatedMessage { .. }
                | Shape::Map { .. }
                | Shape::Oneof => {
                    writeln!(src, "                        {name},")
                        .expect("write salvage owned field");
                }
                Shape::Scalar(_)
                | Shape::EmptyBytes
                | Shape::RepeatedString
                | Shape::RepeatedBytes => {
                    writeln!(src, "                        {name}: _,")
                        .expect("write salvage bounded field");
                }
            }
        }
        src.push_str("                    } = value;\n");
        for field in fields.iter() {
            let name = &field.rust_name;
            match &field.shape {
                Shape::Message { leaf } => {
                    let child = rust_type_name(leaf);
                    writeln!(
                        src,
                        "                    if let Some(child) = {name} {{ work.push(Value::{child}(child)); }}"
                    )
                    .expect("write salvage optional child");
                }
                Shape::RepeatedMessage { leaf } => {
                    let child = rust_type_name(leaf);
                    writeln!(
                        src,
                        "                    work.extend({name}.into_iter().map(Value::{child}));"
                    )
                    .expect("write salvage repeated child");
                }
                Shape::Map { value_leaf, .. } => {
                    let child = rust_type_name(value_leaf);
                    writeln!(
                        src,
                        "                    work.extend({name}.into_values().map(Value::{child}));"
                    )
                    .expect("write salvage map values");
                }
                Shape::Oneof => {
                    let oneof = decoder_oneof(messages, oneofs, msg.leaf_name(), &field.rust_name);
                    let function = decoder_oneof_salvage_fn(oneof);
                    writeln!(src, "                    {function}({name}, &mut work);")
                        .expect("write salvage oneof");
                }
                Shape::Scalar(_)
                | Shape::EmptyBytes
                | Shape::RepeatedString
                | Shape::RepeatedBytes => {}
            }
        }
        src.push_str("                }\n");
    }
    src.push_str(
        r#"                Value::EPathMap(mut value) => {
                    if let Some(remainder) = value.remainder.take() {
                        work.push(Value::Var(remainder));
                    }
                    value.drain_owned_pars(&mut pars);
                }
                Value::MapStringPar(_key, value) => pars.push(value),
            }
        }
        if !pars.is_empty() {
            dismantle_all(pars);
        }
    }
}

#[cold]
#[inline(never)]
fn delimited_length_exceeded() -> DecodeError {
    // Ask prost to construct its private DelimitedLengthExceeded kind. The
    // one-byte region has two available bytes; consuming both in one merge
    // iteration crosses the absolute boundary and reaches precisely that arm.
    let mut bytes: &[u8] = &[1u8, 0u8, 0u8];
    let mut unit = ();
    encoding::merge_loop(
        &mut unit,
        &mut bytes,
        DecodeContext::default(),
        |_unit, buf, _ctx| {
            buf.advance(2);
            Ok(())
        },
    )
    .expect_err(
        "protobuf decoder: the prost merge_loop mint must cross its declared boundary",
    )
}

"#,
    );
}

fn emit_protobuf_decoder_entries(src: &mut String, rows: &[(&Message<'_>, Option<&[Field]>)]) {
    for (msg, fields) in rows {
        if fields.is_none() || msg.leaf_name() == "Par" {
            continue;
        }
        let variant = rust_type_name(msg.leaf_name());
        let function = format!("decode_{}", msg.leaf_name().to_snake_case());
        let ty = msg.rust_path();
        writeln!(
            src,
            "pub fn {function}<B: Buf>(buf: B) -> Result<crate::rhoapi::{ty}, DecodeError> {{\n\
                 let mut machine = Machine::new(\n\
                     buf,\n\
                     Node::{variant} {{ value: Default::default(), limit: 0 }},\n\
                 );\n\
                 machine.run()?;\n\
                 if machine.values.len() != 1 {{\n\
                     panic!(\"protobuf decoder invariant: {variant} left {{}} root values\", machine.values.len());\n\
                 }}\n\
                 match machine.values.pop().expect(\"the single checked root value exists\") {{\n\
                     Value::{variant}(value) => Ok(value),\n\
                     other => {{\n\
                         let found = other.kind_name();\n\
                         machine.values.push(other);\n\
                         panic!(\"protobuf decoder invariant: expected {variant}, found {{found}}\");\n\
                     }}\n\
                 }}\n\
             }}\n"
        )
        .expect("write protobuf decoder entry");
    }

    src.push_str(
        r#"pub fn decode_epath_map<B: Buf>(buf: B) -> Result<EPathMap, DecodeError> {
    let mut machine = Machine::new(
        buf,
        Node::EPathMap {
            value: EPathMap::default(),
            limit: 0,
        },
    );
    machine.run()?;
    if machine.values.len() != 1 {
        panic!(
            "protobuf decoder invariant: EPathMap left {} root values",
            machine.values.len()
        );
    }
    match machine
        .values
        .pop()
        .expect("the single checked EPathMap root exists")
    {
        Value::EPathMap(value) => Ok(value),
        other => {
            let found = other.kind_name();
            machine.values.push(other);
            panic!("protobuf decoder invariant: expected EPathMap, found {found}");
        }
    }
}

pub fn decode_par<B: Buf>(buf: B) -> Result<Par, DecodeError> {
    decode_par_with_stats(buf).map(|(value, _stats)| value)
}

pub fn decode_par_with_stats<B: Buf>(
    buf: B,
) -> Result<(Par, DecodeStats), DecodeError> {
    let mut machine = Machine::new(
        buf,
        Node::Par {
            value: Par::default(),
            limit: 0,
        },
    );
    machine.run()?;
    if machine.values.len() != 1 {
        panic!(
            "protobuf decoder invariant: Par left {} root values",
            machine.values.len()
        );
    }
    let stats = machine.stats;
    match machine
        .values
        .pop()
        .expect("the single checked Par root exists")
    {
        Value::Par(value) => {
            #[cfg(feature = "phase7-depth-histograms")]
            crate::rust::rholang::phase7_depth_histogram::record_par("protobuf_decoder", &value);
            Ok((value, stats))
        }
        other => {
            let found = other.kind_name();
            machine.values.push(other);
            panic!("protobuf decoder invariant: expected Par, found {found}");
        }
    }
}

"#,
    );

    let par_fields = rows
        .iter()
        .find_map(|(message, fields)| (message.leaf_name() == "Par").then_some(*fields))
        .flatten()
        .expect("schema: the generated Par message must have resolved fields");
    emit_protobuf_decoder_merge_par_field(src, par_fields);
}

fn emit_protobuf_decoder_merge_par_field(src: &mut String, fields: &[Field]) {
    src.push_str(
        "pub fn merge_par_field<B: Buf>(\n\
         \x20   value: &mut Par,\n\
         \x20   tag: u32,\n\
         \x20   protobuf_wire_type: WireType,\n\
         \x20   buf: &mut B,\n\
         \x20   ctx: DecodeContext,\n\
         ) -> Result<(), DecodeError> {\n\
         \x20   match tag {\n",
    );
    for field in protobuf_order(fields) {
        let tag = field.min_tag();
        let name = &field.rust_name;
        let location = name.strip_prefix("r#").unwrap_or(name);
        match &field.shape {
            Shape::RepeatedMessage { leaf } => {
                let decode = format!("decode_{}", leaf.to_snake_case());
                writeln!(
                    src,
                    "        {tag}u32 => {{\n\
                     \x20           encoding::check_wire_type(WireType::LengthDelimited, protobuf_wire_type)\n\
                     \x20               .map_err(|mut error| {{ error.push(\"Par\", {location:?}); error }})?;\n\
                     \x20           let len = decode_varint(buf)\n\
                     \x20               .map_err(|mut error| {{ error.push(\"Par\", {location:?}); error }})?;\n\
                     \x20           if len > buf.remaining() as u64 {{\n\
                     \x20               let mut error = buffer_underflow();\n\
                     \x20               error.push(\"Par\", {location:?});\n\
                     \x20               return Err(error);\n\
                     \x20           }}\n\
                     \x20           let mut child_buf = (&mut *buf).take(len as usize);\n\
                     \x20           let child = {decode}(&mut child_buf)\n\
                     \x20               .map_err(|mut error| {{ error.push(\"Par\", {location:?}); error }})?;\n\
                     \x20           debug_assert_eq!(child_buf.remaining(), 0);\n\
                     \x20           value.{name}.push(child);\n\
                     \x20           Ok(())\n\
                     \x20       }}"
                )
                .expect("write Par repeated-message merge arm");
            }
            Shape::Scalar(_) | Shape::EmptyBytes => {
                let ty = match &field.shape {
                    Shape::Scalar(ty) => *ty,
                    Shape::EmptyBytes => Type::Bytes,
                    _ => unreachable!(),
                };
                let (module, _) = protobuf_scalar_module_and_default(ty);
                writeln!(
                    src,
                    "        {tag}u32 => encoding::{module}::merge(\n\
                     \x20           protobuf_wire_type, &mut value.{name}, buf, ctx,\n\
                     \x20       )\n\
                     \x20       .map_err(|mut error| {{ error.push(\"Par\", {location:?}); error }}),"
                )
                .expect("write Par scalar merge arm");
            }
            other => panic!(
                "schema: Par field `{name}` has unsupported manual Message merge shape \
                 {other:?}; widen `merge_par_field` before changing the recursive cut-set type"
            ),
        }
    }
    src.push_str(
        "        _ => skip_unknown_field(protobuf_wire_type, tag, buf),\n\
         \x20   }\n\
         }\n",
    );
}
