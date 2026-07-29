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
            // ⚠ LIVE at HEAD. `9082d12c` removed a CALL at `inj_attempt`'s
            // set-initial-cost phase and entered the composition in
            // `CONVERTED_DEPTH` as `inj_attempt_clone`; `<Par as Clone>::clone`
            // itself is untouched and still in `TRIPWIRE_DEPTH`. Re-measured at
            // HEAD by `four_quadrant_s0_baseline`: 16,493 / 3,254 B/level.
            Disposition::Remaining("D-clone"),
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

    let bincode = emit_bincode_source(&messages, &resolved, &oneofs, &extern_set);
    let prost = emit_prost_source(&messages, &resolved, &oneofs, &extern_set);
    let term_ops = emit_term_ops_source();
    let (schema_meta, derive_row_count) =
        emit_schema_meta_source(&messages, &oneofs, &extern_set, &graph);

    Generated {
        counts: Counts {
            empty_bytes_fields,
            message_count: resolved.len(),
            extern_count,
            oneof_count: oneofs.len(),
            scc_count: graph.scc.len(),
            recursive_type_count: graph.recursive.len(),
            derive_row_count,
        },
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
         use crate::rhoapi::*;\n\
         use crate::rust::rholang::prost_wire::{ProstField, ProstKind};\n\
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

/// Emit the term-op file.
///
/// ★ It is **empty of code**, on purpose, and it is emitted and included all the
/// same. The four-output pipeline is then exercised end-to-end from the stage
/// that built it: `models/build.rs` writes four files and the crate includes
/// four modules, so a later stage that fills this one changes only its
/// contents. A file emitted but not included would be a slot nobody had proved
/// reachable; a slot filled with a placeholder constant would be a stub.
///
/// The emitted text is comments only, which is a valid module body.
fn emit_term_ops_source() -> String {
    String::from(
        "// @generated by models/build/wire_schema.rs from the protobuf FileDescriptorSet.\n\
         // DO NOT EDIT. Regenerate by touching models/src/main/protobuf/RhoTypes.proto.\n\
         //\n\
         // ── EMPTY, DELIBERATELY ──\n\
         //\n\
         // This is the TERM-OP slot of the four-output generator pass. It carries no code\n\
         // yet: the term-op drivers — the `Clone::clone`, `Ord::cmp`, `Debug::fmt` and\n\
         // `Message::clear` walks that `rhoapi_schema_meta.rs`'s\n\
         // DERIVE_DISPOSITION_REGISTRY marks `Remaining`, plus the hand-written\n\
         // `PartialEq::eq` / `Hash::hash` that no derive scan can see — belong to a later\n\
         // stage of the four-quadrant design and are not in scope here.\n\
         //\n\
         // It is EMITTED AND INCLUDED all the same, so the pipeline a later stage fills is\n\
         // exercised from the stage that built it: four files written, four modules\n\
         // included, one pass. A file emitted but not included would be a slot nobody had\n\
         // proved reachable, and a placeholder constant would be a stub.\n",
    )
}

// ===========================================================================
// §8  EMITTER D — the SCHEMA META (`rhoapi_schema_meta.rs`)
// ===========================================================================

fn emit_schema_meta_source(
    messages: &[Message<'_>],
    oneofs: &[Oneof],
    extern_set: &BTreeSet<&str>,
    graph: &SchemaGraph,
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
        for derive in DERIVE_DISPOSITIONS {
            if derive.applies_to == Applies::Oneofs {
                continue;
            }
            for (surface, disposition) in derive.surfaces {
                writeln!(
                    src,
                    "    (\"{ty}\", \"{surface}\", {}),",
                    disposition.as_source()
                )
                .expect("write");
                rows += 1;
            }
        }
    }
    for oneof in oneofs {
        let ty = &oneof.rust_ident;
        for derive in DERIVE_DISPOSITIONS {
            if derive.applies_to == Applies::Messages {
                continue;
            }
            for (surface, disposition) in derive.surfaces {
                writeln!(
                    src,
                    "    (\"{ty}\", \"{surface}\", {}),",
                    disposition.as_source()
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
