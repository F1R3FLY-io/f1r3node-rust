//! # The wire-schema GENERATOR — one table, both directions
//!
//! A **build-script pass**, not a proc-macro. It reads the protobuf
//! `FileDescriptorSet` that `prost_build` was asked to dump (`build.rs`
//! `.file_descriptor_set_path(...)`) and emits, into `OUT_DIR/rhoapi_wire.rs`,
//! the single table that BOTH the serializer and the deserializer are driven
//! by.
//!
//! ## Why the descriptor and not the `#[prost(...)]` attributes
//!
//! The attributes describe the **protobuf** wire. The encoding this table
//! drives is **bincode over serde**, whose layout is *serde's declaration
//! order* — a fact serde's derive exposes nowhere at runtime. The descriptor
//! is the only artifact that carries declaration order, and prost emits its
//! Rust fields in exactly that order, so the descriptor IS the serde schema.
//!
//! ⚠ **The bincode variant index is serde DECLARATION ORDER, not the proto
//! tag.** `e_pathmap_body` is proto tag 32 and serde index 25. Reading tags as
//! indices silently mis-decodes 12 of the 36 `ExprInstance` arms. The oneof is
//! therefore **append-only**: inserting a member mid-list silently re-labels
//! every byte string already in the store.
//!
//! ## What is emitted
//!
//! ```text
//!   <TY>_PROGRAM : &'static [FieldKind]     one per message   (the SHAPE)
//!   impl WireNode for <TY>                  one per message   (the ACCESS)
//!   impl WireOneof for <ONEOF>              one per oneof     (exhaustive!)
//!   <ONEOF>_VARIANTS : &'static [VariantProgram]              (the INDEX)
//!   <ONEOF>_VARIANT_COUNT = <ONEOF>_VARIANTS.len()  ★ never a literal
//!   EX_<FIELD> : u32                        one per ExprInstance arm
//! ```
//!
//! The `impl WireOneof` matches carry **no wildcard arm**, so a 37th variant
//! added to the `.proto` is a *compile error* until the table regenerates —
//! which it does, in the same pass. That is what makes "coverage asserted
//! against the generated variant set" true by construction rather than by
//! a hand-maintained list that silently stays at 36.
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
//!    [`FieldKind::EmptyBytes`] by the same rule (field named `locallyFree`,
//!    type `bytes`) and `build.rs` **cross-checks the two counts**, so the
//!    textual pass and the table cannot drift apart.

use std::collections::BTreeSet;
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

/// Result of one generator run.
pub struct Generated {
    /// The Rust source to write into `OUT_DIR`.
    pub source: String,
    /// How many fields were classified [`FieldKind::EmptyBytes`]. `build.rs`
    /// asserts this equals the number of textual `serialize_with` injections
    /// it made — the two rules are then provably the same rule.
    pub empty_bytes_fields: usize,
    /// Message count, for the build-time log line.
    pub message_count: usize,
    /// Oneof count, for the build-time log line.
    pub oneof_count: usize,
}

/// One resolved field of one message, in **serde declaration order**.
struct Field {
    /// The Rust struct field identifier (already keyword-escaped).
    rust_name: String,
    /// The `FieldKind` variant name, as Rust source.
    kind: &'static str,
    /// The BODY of this field's `match` arm in the generated `wire_emit`, as
    /// Rust source. `self`, `out` and `i` are in scope; a descending arm
    /// `return`s a [`Descent`].
    emit: String,
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

/// Collect `msg` and every message nested inside it, skipping synthetic map
/// entries (which prost renders as `BTreeMap`, not as a type).
fn collect<'a>(msg: &'a DescriptorProto, parents: &[String], out: &mut Vec<Message<'a>>) {
    if is_map_entry(msg) {
        return;
    }
    out.push(Message {
        desc: msg,
        parents: parents.to_vec(),
    });
    let mut chain = parents.to_vec();
    chain.push(msg.name.clone().unwrap_or_default());
    for nested in &msg.nested_type {
        collect(nested, &chain, out);
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
    programs: &std::collections::BTreeMap<String, String>,
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

/// Generate the whole table.
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
    for file in &fds.file {
        if file.package.as_deref() != Some(PACKAGE) {
            continue;
        }
        for msg in &file.message_type {
            collect(msg, &[], &mut messages);
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
    let mut programs: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
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

    let mut src = String::with_capacity(96 * 1024);
    header(&mut src);

    let mut empty_bytes_fields = 0usize;
    let mut oneofs: Vec<Oneof> = Vec::new();
    let mut emitted_messages = 0usize;

    for msg in &messages {
        let rust_path = msg.rust_path();
        if extern_set.contains(msg.leaf_name()) {
            writeln!(
                src,
                "// `{rust_path}` is EXTERN (models/build.rs `.extern_path`). Its wire program\n\
                 // is HAND-WRITTEN in `models/src/rust/rholang/wire.rs` because its `Serialize`\n\
                 // deliberately re-orders `ps` into canonical trie order for ground maps — a\n\
                 // behaviour no descriptor can express. The obligation is enforced below.\n"
            )
            .expect("write");
            continue;
        }

        let (fields, mut msg_oneofs, empties) =
            resolve_message(msg, &known, &extern_set, &programs);
        empty_bytes_fields += empties;
        oneofs.append(&mut msg_oneofs);
        emit_message(&mut src, &rust_path, &msg.program_ident(), &fields);
        emitted_messages += 1;
    }

    for oneof in &oneofs {
        emit_oneof(&mut src, oneof);
    }

    emit_conformance_registry(&mut src, &messages, &extern_set);
    emit_extern_obligations(&mut src, &extern_set);

    Generated {
        source: src,
        empty_bytes_fields,
        message_count: emitted_messages,
        oneof_count: oneofs.len(),
    }
}

fn header(src: &mut String) {
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

/// Resolve one message into its serde field program.
fn resolve_message(
    message: &Message<'_>,
    known: &BTreeSet<String>,
    extern_set: &BTreeSet<&str>,
    programs: &std::collections::BTreeMap<String, String>,
) -> (Vec<Field>, Vec<Oneof>, usize) {
    let msg = message.desc;
    let msg_name = msg.name.clone().unwrap_or_default();
    let mut fields: Vec<Field> = Vec::new();
    let mut oneofs: Vec<Oneof> = Vec::new();
    let mut empty_bytes = 0usize;

    // ⚠★ THE ORDERING RULE, and it is NOT the intuitive one.
    //
    // prost emits **every plain field first, in declaration order, and then
    // every oneof field, in `oneof_decl` order** — it does *not* place a oneof
    // where its first member appears. `prost-build-0.14.3/src/code_generator.rs`
    // :270-291 is two separate loops over `fields` and `oneof_fields`.
    //
    // The difference is invisible in four of the five oneofs in this schema
    // (they are their message's only field) and decisive in the fifth:
    // `TaggedContinuation` declares `oneof tagged_cont { … }` BEFORE `guard`,
    // so the intuitive rule emits `tagged_cont` first while prost emits
    // `guard` first. That is a 95-byte encoding whose two halves are simply
    // exchanged — same length, same bytes, different order — which no length
    // check and no round-trip could ever see. The write differential caught it
    // on the first run; it is recorded here so it cannot be "simplified" back.
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
        let index = fields.len();
        let (kind, emit, is_empty_bytes) = classify(&msg_name, field, known, extern_set, index);
        if is_empty_bytes {
            empty_bytes += 1;
        }
        fields.push(Field {
            rust_name: rust_field_name(&field.name.clone().unwrap_or_default()),
            kind,
            emit,
        });
    }

    for (idx, decl) in msg.oneof_decl.iter().enumerate() {
        let proto_name = decl.name.clone().unwrap_or_default();
        let rust_ident = rust_type_name(&proto_name);
        let module = message.oneof_module();
        let rust_name = rust_field_name(&proto_name);
        let resume = fields.len() + 1;
        fields.push(Field {
            rust_name: rust_name.clone(),
            kind: "Oneof",
            // An `Option<oneof>` is a 1-byte tag, then (when present) the
            // oneof's own u32 declaration-order index and payload. The index
            // and any bounded payload are written by the generated
            // `WireOneof::wire_emit`, which is monomorphic and inlines; only a
            // MESSAGE payload suspends.
            emit: format!(
                "match &self.{rust_name} {{ \
                 Some(v) => {{ put_bool(out, true); \
                 if let Some(node) = WireOneof::wire_emit(v, out) {{ \
                 return Descent::Node {{ resume: {resume}, node }}; }} }} \
                 None => put_bool(out, false) }}"
            ),
        });
        let variants = resolve_oneof_variants(msg, idx, &rust_ident, known, extern_set, programs);
        assert!(
            !variants.is_empty(),
            "wire_schema: oneof `{msg_name}.{proto_name}` has no members; prost would emit no \
             enum and the table would name a type that does not exist."
        );
        oneofs.push(Oneof {
            proto_name,
            rust_ident,
            module,
            variants,
        });
    }

    // ★ THE TAIL-CALL PATCH. A descending field that is the program's LAST
    // needs no resume point, and the generator is the only place that knows
    // which one that is. `resume: {n}` — where `n` is the field count — can
    // only have been produced by the final field, so the rewrite is exact.
    //
    // Doing this at build time removes a virtual `wire_program().len()` call
    // from EVERY descent in the driver, which was the entire residual gap
    // against the derived encoder (see `wire.rs`'s `NO_RESUME`).
    assert!(
        fields.len() < u16::MAX as usize,
        "wire_schema: `{msg_name}` has {} fields; `Descent::resume` is a u16 and \
         `NO_RESUME` is its maximum.",
        fields.len()
    );
    let spent = format!("resume: {}", fields.len());
    for field in &mut fields {
        field.emit = field.emit.replace(&spent, "resume: NO_RESUME");
    }

    (fields, oneofs, empty_bytes)
}

/// Classify a non-oneof field into `(FieldKind, emission source, is_empty_bytes)`.
///
/// The emission source is the body of one `match` arm in the generated
/// `wire_emit`. Bounded fields call an emission primitive from
/// `crate::rust::rholang::wire` — the single place the layout is written — and
/// descending fields `return` a [`Descent`] naming the field to resume at.
fn classify(
    msg_name: &str,
    field: &FieldDescriptorProto,
    known: &BTreeSet<String>,
    extern_set: &BTreeSet<&str>,
    index: usize,
) -> (&'static str, String, bool) {
    let proto_name = field.name.clone().unwrap_or_default();
    let name = rust_field_name(&proto_name);
    let repeated = field.label == Some(Label::Repeated as i32);
    let resume = index + 1;
    let ty = Type::try_from(field.r#type.unwrap_or(0)).unwrap_or_else(|_| {
        panic!("wire_schema: `{msg_name}.{proto_name}` has an unrecognised protobuf type")
    });

    // A protobuf `map<K,V>` arrives as a REPEATED synthetic MapEntry message.
    if repeated && ty == Type::Message {
        let leaf = type_leaf(field.type_name.as_deref().unwrap_or(""));
        if leaf.ends_with("Entry") && !known.contains(leaf) {
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
                format!(
                    "{{ let m = &self.{name}; put_u64(out, m.len() as u64); \
                     if !m.is_empty() {{ return Descent::Map {{ resume: {resume}, map: m }}; }} }}"
                ),
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
                format!(
                    "{{ let s = &self.{name}; let n = s.len(); put_u64(out, n as u64); \
                     if n != 0 {{ return Descent::Seq {{ resume: {resume}, len: n, seq: s }}; }} }}"
                ),
                false,
            )
        }
        (true, Type::String) => ("StrSeq", format!("put_str_seq(out, &self.{name})"), false),
        (true, Type::Bytes) => ("BytesSeq", format!("put_bytes_seq(out, &self.{name})"), false),
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
                format!(
                    "match &self.{name} {{ \
                     Some(v) => {{ put_bool(out, true); \
                     return Descent::Node {{ resume: {resume}, node: v }}; }} \
                     None => put_bool(out, false) }}"
                ),
                false,
            )
        }
        (false, Type::Bool) => ("Bool", format!("put_bool(out, self.{name})"), false),
        (false, Type::String) => ("Str", format!("put_str(out, &self.{name})"), false),
        (false, Type::Bytes) => {
            // ⚠ THE SERIALIZE-ONLY ASYMMETRY. `models/build.rs` injects
            // `serialize_with = serialize_as_empty_bytes` on exactly the
            // `locally_free` bytes fields: written as eight zero bytes, read
            // back at the stream's REAL length. Same rule, same place.
            if proto_name.to_snake_case() == "locally_free" {
                ("EmptyBytes", "put_empty_bytes(out)".to_string(), true)
            } else {
                ("Bytes", format!("put_bytes(out, &self.{name})"), false)
            }
        }
        (false, Type::Int32 | Type::Sint32 | Type::Sfixed32) => {
            ("I32", format!("put_i32(out, self.{name})"), false)
        }
        (false, Type::Uint32 | Type::Fixed32) => {
            ("U32", format!("put_u32(out, self.{name})"), false)
        }
        (false, Type::Int64 | Type::Sint64 | Type::Sfixed64) => {
            ("I64", format!("put_i64(out, self.{name})"), false)
        }
        (false, Type::Uint64 | Type::Fixed64) => {
            ("U64", format!("put_u64(out, self.{name})"), false)
        }
        (false, other) => panic!(
            "wire_schema: `{msg_name}.{proto_name}` has type {other:?}, which has no `FieldKind`. \
             Add one deliberately — silently widening it to the nearest integer would change the \
             byte layout and fork consensus."
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
    programs: &std::collections::BTreeMap<String, String>,
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
        let (payload_expr, program_expr) = match ty {
            Type::Message => {
                let leaf = type_leaf(field.type_name.as_deref().unwrap_or(""));
                assert!(
                    known.contains(leaf) || extern_set.contains(leaf),
                    "wire_schema: oneof member `{msg_name}.{proto_name}` is a `{leaf}`, which is \
                     not a `{PACKAGE}` message."
                );
                let program = program_ident_of(leaf, extern_set, programs);
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

fn emit_message(src: &mut String, rust_ty: &str, program_ident: &str, fields: &[Field]) {
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
        for (i, f) in fields.iter().enumerate() {
            writeln!(
                src,
                "        if from < {} {{ {} }} // {}",
                i + 1,
                f.emit,
                f.rust_name
            )
            .expect("write");
        }
        src.push_str("        Descent::Done\n");
    }
    src.push_str("    }\n}\n\n");
}

fn emit_oneof(src: &mut String, oneof: &Oneof) {
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
        writeln!(
            src,
            "pub const {}: u32 = {};",
            v.const_name, v.index
        )
        .expect("write");
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
