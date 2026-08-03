extern crate tonic_prost_build;

// https://docs.rs/prost-build/latest/prost_build/struct.Config.html
// https://docs.rs/tonic-build/latest/tonic_build/struct.Builder.html#

use std::path::Path;
use std::{env, fs};

use prost::Message as _;

/// The schema-code generator: one descriptor pass emits the bincode and
/// protobuf tables plus the generated term-operation and decode PDAs. See its
/// module docs for why this is a build-script pass rather than a proc-macro.
#[path = "codegen/schema_codegen.rs"]
mod schema_codegen;

fn main() {
    let manifest_dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).to_path_buf();
    let proto_src_dir = manifest_dir.join("src/main/protobuf");
    let scala_proto_base_dir = manifest_dir.join("src");

    let proto_files = [
        "CasperMessage.proto",
        "DeployServiceCommon.proto",
        "DeployServiceV1.proto",
        "ProposeServiceCommon.proto",
        "ProposeServiceV1.proto",
        "RholangScalaRustTypes.proto",
        "RhoTypes.proto",
        "RSpacePlusPlusTypes.proto",
        "ServiceError.proto",
        "ExternalCommunicationServiceCommon.proto",
        "ExternalCommunicationServiceV1.proto",
        "routing.proto",
    ];

    let absolute_proto_files: Vec<_> = proto_files.iter().map(|f| proto_src_dir.join(f)).collect();

    // Tell Cargo to only rerun this build script if proto files change
    for proto_file in &absolute_proto_files {
        println!("cargo:rerun-if-changed={}", proto_file.display());
    }
    // Also watch the scalapb proto used for imports
    println!(
        "cargo:rerun-if-changed={}",
        scala_proto_base_dir.join("scalapb/scalapb.proto").display()
    );
    // ⚠ AND the generator's own source. Emitting ANY `cargo:rerun-if-changed`
    // switches cargo from "rerun when anything in the package changed" to
    // "rerun only for these paths" — so without this line, editing
    // `codegen/schema_codegen.rs` leaves a STALE generated table in `OUT_DIR` while
    // the build reports success. That is a silent, byte-visible divergence
    // between the generator in the tree and the table in the binary; it was
    // observed once, during this module's development, and cost a confusing
    // benchmark result.
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("codegen/schema_codegen.rs").display()
    );

    // The descriptor set is what the schema-code generator reads. It carries
    // FIELD DECLARATION ORDER, which is the bincode/serde layout — a fact the
    // `#[prost(...)]` attributes (which describe the *protobuf* wire) and
    // serde's derive (which exposes nothing at run time) both fail to provide.
    let out_dir_path = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).to_path_buf();
    let descriptor_path = out_dir_path.join("rhoapi_descriptor.bin");

    tonic_prost_build::configure()
        .file_descriptor_set_path(&descriptor_path)
        .build_client(true)
        .build_server(true)
        .btree_map(".")
        // `.rhoapi.EPathMap` is external: generated references resolve to the
        // PathMap-native implementation in `rhoapi_ext.rs`. It specializes
        // empty/set/map storage and supplies stack-safe generated term and
        // protobuf operations while preserving the public schema path.
        // `crate::rhoapi` re-exports it (models/src/lib.rs), so downstream
        // import paths are unchanged. The textual post-processing below
        // continues to apply to the REMAINING generated types.
        .extern_path(".rhoapi.EPathMap", "crate::rust::rhoapi_ext::EPathMap")
        .message_attribute(
            ".rhoapi",
            "#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]",
        )
        .message_attribute(".rhoapi", "#[derive(Eq, Ord, PartialOrd)]")
        .message_attribute(".rhoapi", "#[repr(C)]")
        .enum_attribute(
            ".rhoapi",
            "#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]",
        )
        .enum_attribute(".rhoapi", "#[derive(Eq, Ord, PartialOrd)]")
        .enum_attribute(".rhoapi", "#[repr(C)]")
        .bytes(".casper")
        .bytes(".routing")
        // needed for grpc services from deploy_grpc_service_v1.rs to avoid upper camel case warnings
        .server_mod_attribute(".", "#[allow(non_camel_case_types)]")
        .client_mod_attribute(".", "#[allow(non_camel_case_types)]")
        .compile_protos(
            &absolute_proto_files,
            &[proto_src_dir, manifest_dir, scala_proto_base_dir],
        )
        .expect("Failed to compile proto files");

    let descriptor_bytes = fs::read(&descriptor_path).expect("read the protobuf descriptor set");
    let descriptor_set = prost_types::FileDescriptorSet::decode(&descriptor_bytes[..])
        .expect("decode the protobuf descriptor set");
    let generated = schema_codegen::generate(&descriptor_set);
    let counts = &generated.counts;

    // Remove PartialEq from specific generated structs from rhoapi.rs
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let file_path = format!("{}/rhoapi.rs", out_dir);
    let content = fs::read_to_string(&file_path).expect("Unable to read file");

    // Preserve prost-build's untouched recursive implementation as an
    // independent, test-only semantic oracle.  The production module below is
    // rewritten to replace every feedback-vertex-set traversal with a generated
    // PDA; differential tests must not compare that PDA with `Par::decode`,
    // because `Par::decode` is one of the very surfaces the PDA implements.
    // Keeping the pre-rewrite source makes the two implementations originate
    // from independent code paths while retaining the exact schema and Prost
    // error semantics used before the conversion.
    fs::write(
        out_dir_path.join("rhoapi_recursive_oracle.rs"),
        prepare_recursive_oracle(&content),
    )
    .expect("write the recursive protobuf oracle source");

    // ★★ THE DERIVE-STRIP PASS, and the RECORD of what it stripped.
    //
    // prost-build hard-codes `"#[derive(Clone, {}PartialEq, {}{}::Message)]"`
    // (`prost-build-0.14.3/src/code_generator.rs:248`) and `message_attribute`
    // can only APPEND, so removing a derive prost inserts has to be textual.
    // This pass removes three things and records one of them:
    //
    //   PartialEq / Eq / Hash   hand-written in models/src/lib.rs
    //                           (`<Par as PartialEq>::eq` ignores `locally_free`,
    //                           which no derive would do)
    //   ★ Clone, on the NON-`Copy` items only
    //                           GENERATED by the term-op pass as an explicit
    //                           worklist traversal (stage F-4)
    //
    // ⚠ The `Copy` items keep `Clone`: a `Copy` type's `Clone` must be a bitwise
    // copy, prost only derives `Copy` where every field is a non-repeated scalar,
    // and an emitted impl would conflict with the derive.
    //
    // ⚠★ The stripped ITEM NAMES are recorded, not just counted. A count
    // cross-check names numbers; the campaign's rule is that a failure names
    // ITSELF, so the generator's independently-computed list is compared against
    // this one AS SETS in both directions.
    let lines: Vec<&str> = content.lines().collect();
    let mut rewritten: Vec<String> = Vec::with_capacity(lines.len());
    let mut clone_stripped: Vec<String> = Vec::with_capacity(64);
    let mut ord_stripped: Vec<String> = Vec::with_capacity(counts.clone_cut_set.len());
    let mut debug_stripped: Vec<String> = Vec::with_capacity(counts.clone_cut_set.len());
    let mut message_stripped: Vec<String> = Vec::with_capacity(counts.clone_cut_set.len());
    for (i, line) in lines.iter().enumerate() {
        let mut out = if line.contains("#[derive(Clone, PartialEq, ::prost::Message)]")
            || line.contains("#[derive(Clone, PartialEq, ::prost::Oneof)]")
            || line.contains("#[derive(Clone, Copy, PartialEq, ::prost::Message)]")
            || line.contains("#[derive(Clone, Copy, PartialEq, ::prost::Oneof)]")
        {
            line.replace("PartialEq,", "")
        } else if line.contains("#[derive(Clone, Copy, PartialEq, Eq, Hash, ::prost::Message)]")
            || line.contains("#[derive(Clone, Copy, PartialEq, Eq, Hash, ::prost::Oneof)]")
            || line.contains("#[derive(Clone, PartialEq, Eq, Hash, ::prost::Message)]")
            || line.contains("#[derive(Clone, PartialEq, Eq, Hash, ::prost::Oneof)]")
        {
            line.replace("PartialEq, Eq, Hash,", "")
        } else {
            line.to_string()
        };
        let protobuf_item = is_protobuf_derive_line(line).then(|| item_declared_after(&lines, i));
        let manual_message = protobuf_item.as_ref().is_some_and(|item| {
            line.contains("::prost::Message") && counts.clone_cut_set.contains(item)
        });
        if let Some(item) = protobuf_item.as_ref().filter(|_| !line.contains("Copy")) {
            out = out.replace("Clone,", "");
            clone_stripped.push(item.clone());
        }
        if manual_message {
            message_stripped.push(
                protobuf_item
                    .as_ref()
                    .expect("manual Message derive has an attributed item")
                    .clone(),
            );
            out.clear();
        }
        if line.trim() == "#[derive(Eq, Ord, PartialOrd)]" {
            let item = item_declared_after(&lines, i);
            if counts.clone_cut_set.contains(&item) {
                out = "#[derive(Eq)]".to_string();
                ord_stripped.push(item);
            }
        }
        rewritten.push(out);
        // `Debug` is emitted *inside* prost's Message/Oneof derive rather than
        // as a token we can remove.  Prost's own `#[prost(skip_debug)]` switch
        // suppresses it.  Inject the switch at the descriptor-derived feedback
        // vertex set only; every residual generated formatter reaches this cut
        // within `clone_residual_height` frames.
        if let Some(item) = protobuf_item.filter(|item| counts.clone_cut_set.contains(item)) {
            if !manual_message {
                rewritten.push("#[prost(skip_debug)]".to_string());
            }
            debug_stripped.push(item);
        }
    }
    let modified_content = strip_prost_field_attributes(&rewritten.join("\n"), &message_stripped);

    // Normalize locally_free in serde serialization — always serialize as empty vec.
    // This matches Scala's AlwaysEqual semantics where locally_free is a transient
    // analysis field that must not affect Blake2b256 channel hashes in RSpace.
    const LOCALLY_FREE_DECL: &str = "pub locally_free: ::prost::alloc::vec::Vec<u8>,";
    let locally_free_sites = modified_content.matches(LOCALLY_FREE_DECL).count();
    let modified_content = modified_content.replace(
        LOCALLY_FREE_DECL,
        "#[serde(serialize_with = \"crate::rust::serde_helpers::serialize_as_empty_bytes\")]\n    pub locally_free: ::prost::alloc::vec::Vec<u8>,",
    );

    fs::write(&file_path, &modified_content).expect("Unable to write file");

    // -----------------------------------------------------------------------
    // Stage 2: the schema-codegen pass — ONE walk, FIVE outputs
    // -----------------------------------------------------------------------
    //
    // This extends an existing two-stage pipeline: the textual pass above
    // already post-processes prost's output, and this pass reads the same
    // compilation's descriptor set.
    // ★ THE ANTI-DRIFT CROSS-CHECK. The textual pass above and the generator
    // must be applying the SAME rule to the SAME fields. The textual pass
    // counts the `locally_free` declarations it rewrote; the generator counts
    // the fields it classified `FieldKind::EmptyBytes`. If they ever disagree,
    // one of the two has silently stopped covering a field — and since the
    // rewrite is what makes `locally_free` serialize as eight zero bytes, a
    // divergence is a byte-level consensus hazard, not a cosmetic one.
    assert_eq!(
        locally_free_sites, counts.empty_bytes_fields,
        "models/build.rs: the serialize_as_empty_bytes TEXTUAL pass rewrote {} \
         `locally_free` declarations, but schema codegen classified {} fields as \
         EmptyBytes. The two rules have drifted apart; one of them no longer covers every \
         `locally_free` field, and the serialize-only blanking is a consensus-visible \
         normalization.",
        locally_free_sites, counts.empty_bytes_fields
    );

    // ★★ THE DERIVE CROSS-CHECK — the same shape, applied to the trait surface.
    //
    // The campaign's driver list must be DERIVED from what is actually
    // `#[derive]`d, never hand-picked: a hand-picked list of four missed `Hash`
    // entirely, and the enumeration that replaced it additionally found
    // `Ord`/`PartialOrd`, which nobody had named. `models/codegen/schema_codegen.rs` holds the
    // closed `DERIVE_DISPOSITIONS` table; this scans the post-processed
    // `rhoapi.rs` for the tokens actually present and requires the two to agree
    // as SETS, in both directions.
    check_derive_dispositions(&modified_content, counts);

    // ★★ THE CLONE JOIN — the textual strip against the descriptor-derived list.
    //
    // Two INDEPENDENT rules for "which items are non-`Copy`":
    //
    //   the strip pass  a `#[derive(...)]` line that names `Clone` and does NOT
    //                   name `Copy`, attributed to the item declared after it
    //   the generator   `ClonePlan`'s reproduction of prost's own rule
    //                   (`prost-build-0.14.3/src/context.rs:183-233`), computed
    //                   from the descriptor
    //
    // They must agree as SETS, in both directions. An item stripped but not
    // emitted has NO `Clone` at all; an item emitted but not stripped has TWO.
    // Both are compile errors — but the compiler's message names `OUT_DIR/rhoapi.rs`
    // and a conflicting impl, not the rule that diverged, so the failure is made
    // to land here and to name the item.
    check_clone_join(&clone_stripped, &generated.clone_impls);
    check_ord_join(&ord_stripped, &counts.clone_cut_set);
    check_debug_join(&debug_stripped, &counts.clone_cut_set);
    check_message_join(&message_stripped, &counts.clone_cut_set);

    // ── the five outputs of the one pass ──
    assert_eq!(
        generated.sources.len(),
        5,
        "models/build.rs: schema codegen must produce exactly five outputs \
         (bincode table, protobuf table, term-op slot, schema meta, protobuf deserializer); \
         it produced {}. `models/src/rust/rholang/mod.rs` includes five modules and a \
         missing file is a compile error whose message names `OUT_DIR`, not this pass.",
        generated.sources.len()
    );
    for (name, source) in &generated.sources {
        assert!(
            !source.is_empty(),
            "models/build.rs: schema codegen produced an EMPTY `{name}`. Even the \
             term-op slot carries its own explanation of why it has no code; a zero-byte \
             file means the emitter returned nothing."
        );
        fs::write(out_dir_path.join(name), source)
            .unwrap_or_else(|e| panic!("write the generated `{name}`: {e}"));
    }

    // Recorded in the build script's captured stdout (`target/*/build/models-*/output`)
    // rather than as a `cargo:warning`, so the evidence is retained without
    // decorating every build of every dependent crate.
    println!(
        "schema_codegen: {} generated messages + {} extern, {} oneofs, {} serialize-only \
         `locally_free` fields (cross-checked against the textual pass); {} SCCs, {} \
         self-containing types, {} derive-disposition rows (cross-checked against the \
         `#[derive]` scan); {} outputs written",
        counts.message_count,
        counts.extern_count,
        counts.oneof_count,
        counts.empty_bytes_fields,
        counts.scc_count,
        counts.recursive_type_count,
        counts.derive_row_count,
        generated.sources.len()
    );
    println!(
        "schema_codegen: clone cut set [{}] (residual height {}), {} items entered by the driver, \
         {} `impl Clone`s emitted (cross-checked against {} textually stripped `Clone` derives)",
        counts.clone_cut_set.join(", "),
        counts.clone_residual_height,
        counts.clone_descend_count,
        generated.clone_impls.len(),
        clone_stripped.len()
    );
}

/// Make prost-build's untouched recursive output independently compilable.
///
/// `message_attribute` adds `Eq`/`Ord` to every generated item, while Prost
/// also adds `Eq`/`Hash` to bytewise-equatable leaf messages.  Production has
/// hand-written equality and therefore removes Prost's entire equality group;
/// the oracle must retain Prost's `PartialEq` but remove only the duplicate
/// `Eq` (and the unneeded `Hash`).  No traversal implementation is changed.
fn prepare_recursive_oracle(source: &str) -> String {
    source
        .replace(
            "PartialEq, Eq, Hash, ::prost::Message",
            "PartialEq, ::prost::Message",
        )
        .replace(
            "PartialEq, Eq, Hash, ::prost::Oneof",
            "PartialEq, ::prost::Oneof",
        )
}

/// Is `line` one of the `#[derive(...)]` lines prost-build writes for a generated
/// message or oneof?
///
/// ⚠ Matched on `::prost::Message` / `::prost::Oneof` rather than on the whole
/// literal, because the `PartialEq` / `Eq` / `Hash` strip above has already run
/// on the same line in the same pass and leaves DOUBLE SPACES behind.
fn is_protobuf_derive_line(line: &str) -> bool {
    line.contains("#[derive(Clone,")
        && (line.contains("::prost::Message") || line.contains("::prost::Oneof"))
}

/// The name of the `struct` / `enum` declared after `lines[i]`.
///
/// prost-build writes a derive attribute, then any number of further attributes
/// (`#[repr(C)]`, `#[serde(...)]`, doc comments), then the item. Scanning forward
/// for the first declaration is therefore exact, and it is the only way to
/// attribute a stripped derive to an ITEM — which is what lets the cross-check
/// name the offender instead of reporting a count.
///
/// # Panics
///
/// If no declaration follows. A silent `None` here would drop an item from the
/// stripped set and the cross-check would then report a spurious
/// "emitted-but-not-stripped" against a type that was in fact stripped.
fn item_declared_after(lines: &[&str], i: usize) -> String {
    for line in &lines[i + 1..] {
        let trimmed = line.trim_start();
        for keyword in ["pub struct ", "pub enum ", "struct ", "enum "] {
            if let Some(rest) = trimmed.strip_prefix(keyword) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    panic!(
        "models/build.rs: the `#[derive(...)]` at line {} of the generated `rhoapi.rs` is \
         followed by no `struct` or `enum` declaration. The clone cross-check attributes each \
         stripped derive to the ITEM it belongs to so that a divergence names the type; a \
         line it cannot attribute would silently leave that type out of the stripped set and \
         turn the cross-check into a false accusation against a different one.\n\nThe line was: \
         {:?}",
        i + 1,
        lines[i]
    )
}

fn strip_prost_field_attributes(source: &str, items: &[String]) -> String {
    use std::collections::BTreeSet;

    let wanted: BTreeSet<&str> = items.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut active = false;
    let mut out = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !active {
            if let Some(rest) = trimmed.strip_prefix("pub struct ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if wanted.contains(name.as_str()) {
                    active = true;
                    seen.insert(name);
                }
            }
            out.push(line);
            continue;
        }
        if trimmed.starts_with("#[prost(") {
            continue;
        }
        out.push(line);
        if trimmed == "}" {
            active = false;
        }
    }
    assert!(
        !active,
        "models/build.rs: unterminated manually driven message struct"
    );
    let seen: BTreeSet<&str> = seen.iter().map(String::as_str).collect();
    assert_eq!(
        seen, wanted,
        "models/build.rs: the prost-field-attribute strip did not find exactly the manual \
         Message cut set"
    );
    out.join("\n")
}

fn check_ord_join(stripped: &[String], cut_set: &[String]) {
    use std::collections::BTreeSet;

    let stripped: BTreeSet<&str> = stripped.iter().map(String::as_str).collect();
    let cut_set: BTreeSet<&str> = cut_set.iter().map(String::as_str).collect();
    assert!(
        !cut_set.is_empty(),
        "models/build.rs: the schema feedback-vertex set is empty, so no recursive `Ord` \
         implementation can have been replaced"
    );
    assert_eq!(
        stripped, cut_set,
        "models/build.rs: the `Ord`/`PartialOrd` derive strip and the descriptor-derived \
         feedback-vertex set disagree. A stripped-only item has no ordering implementation; \
         a cut-set-only item retains recursive derived ordering."
    );
}

fn check_debug_join(stripped: &[String], cut_set: &[String]) {
    use std::collections::BTreeSet;

    let stripped: BTreeSet<&str> = stripped.iter().map(String::as_str).collect();
    let cut_set: BTreeSet<&str> = cut_set.iter().map(String::as_str).collect();
    assert_eq!(
        stripped, cut_set,
        "models/build.rs: the prost `skip_debug` injection and the descriptor-derived \
         feedback-vertex set disagree. A stripped-only item has no Debug implementation; \
         a cut-set-only item retains recursive prost-derived Debug."
    );
}

fn check_message_join(stripped: &[String], cut_set: &[String]) {
    use std::collections::BTreeSet;

    let stripped: BTreeSet<&str> = stripped.iter().map(String::as_str).collect();
    let cut_set: BTreeSet<&str> = cut_set.iter().map(String::as_str).collect();
    assert_eq!(
        stripped, cut_set,
        "models/build.rs: the prost Message derive strip and the descriptor-derived feedback \
         vertex set disagree. A stripped-only item has no Message implementation; a cut-set-only \
         item retains recursive generated Message methods."
    );
}

/// ★★ Require the textual strip and the descriptor-derived emission to name the
/// SAME set of items.
///
/// # What it refuses
///
/// * an item the strip pass removed `Clone` from that the term-op pass emitted no
///   `impl Clone` for — the type then has **no** `Clone`, and while that is a
///   compile error, its message names a call site rather than the rule that
///   diverged;
/// * an item the term-op pass emitted an `impl Clone` for that still carries the
///   derive — **two** impls, `E0119`, again naming the wrong thing;
/// * a **vacuous** strip: zero stripped sites means the pass silently stopped
///   matching prost's derive-line shape (a prost-build upgrade that changed the
///   spacing would do exactly that), and every `Clone` would quietly stay
///   Θ(depth) with the build green.
fn check_clone_join(stripped: &[String], emitted: &[String]) {
    use std::collections::BTreeSet;

    // ── the non-vacuity floor ──
    assert!(
        !stripped.is_empty(),
        "models/build.rs: the derive-strip pass removed `Clone` from ZERO items. prost-build \
         hard-codes `#[derive(Clone, {{}}PartialEq, {{}}{{}}::Message)]`, so every generated \
         message carries the token and a non-`Copy` one must lose it — the `rhoapi` schema has \
         51 non-`Copy` messages and 4 non-`Copy` oneofs. Zero means the line-shape match has \
         stopped matching (a prost-build upgrade that changed the spacing does exactly this), \
         and the consequence is SILENT: every `Clone` stays the derived Θ(depth) walk and the \
         build reports success."
    );
    assert!(
        !emitted.is_empty(),
        "models/build.rs: the term-op pass emitted ZERO `impl Clone`s. See the non-vacuity \
         assertions in `schema_codegen::generate`, which refuse this at the source."
    );

    let stripped_set: BTreeSet<&str> = stripped.iter().map(String::as_str).collect();
    let emitted_set: BTreeSet<&str> = emitted.iter().map(String::as_str).collect();
    assert_eq!(
        stripped.len(),
        stripped_set.len(),
        "models/build.rs: the derive-strip pass attributed two derive lines to the same item \
         ({} lines, {} distinct names). `item_declared_after` scans forward to the next \
         declaration, so a duplicate means two derive attributes share one item — and the \
         second `Clone` would survive.",
        stripped.len(),
        stripped_set.len()
    );

    let stripped_only: Vec<&&str> = stripped_set.difference(&emitted_set).collect();
    assert!(
        stripped_only.is_empty(),
        "models/build.rs: {:?} had `Clone` STRIPPED from the generated `rhoapi.rs` but the \
         term-op pass emitted NO `impl Clone` for them.\n\
         \n\
         Those types now have no `Clone` at all. The two rules for \"which items are \
         non-`Copy`\" have diverged: the textual pass looks for a derive line naming `Clone` \
         and not `Copy`; `models/codegen/schema_codegen.rs`'s `ClonePlan` reproduces prost's own rule \
         (`prost-build-0.14.3/src/context.rs:183-233`) from the descriptor. Fix whichever is \
         wrong — do not paper over it by narrowing the strip, because the strip is what makes \
         the driven `Clone` reachable.",
        stripped_only
    );

    let emitted_only: Vec<&&str> = emitted_set.difference(&stripped_set).collect();
    assert!(
        emitted_only.is_empty(),
        "models/build.rs: the term-op pass emitted an `impl Clone` for {:?}, but the generated \
         `rhoapi.rs` still carries a `Clone` DERIVE for them.\n\
         \n\
         That is two `Clone` impls for one type (`E0119`). Either prost stopped deriving `Copy` \
         for an item `ClonePlan` still believes is `Copy`, or the strip's line-shape match \
         missed a line. See the `Copy` reproduction in `models/codegen/schema_codegen.rs` §4b.",
        emitted_only
    );
}

/// ★★ Require the generator's closed disposition table to agree, AS A SET, with
/// the `#[derive]` tokens actually present in the post-processed `rhoapi.rs`.
///
/// # Why textual, and why here
///
/// The generator reads the protobuf descriptor; the derives come from
/// `models/build.rs`'s own `.message_attribute` / `.enum_attribute` calls plus
/// whatever `prost-build` adds, and are then rewritten by the textual pass
/// above. Only this function sees all three. It is the exact counterpart of the
/// `locally_free` cross-check: two rules that must be the same rule, checked at
/// the one point where both are visible.
///
/// # What it refuses
///
/// * a token in the generated file with **no row** in `DERIVE_DISPOSITIONS` —
///   a seventh trait, arriving undispositioned. It fails the build naming
///   itself, rather than being carried as an unclassified surface nobody
///   notices;
/// * a token in `DERIVE_DISPOSITIONS` that appears **nowhere** — a stale
///   disposition is a claim about code that no longer exists, and a table full
///   of those is exactly the drift the Θ(depth) audit's prose copies suffered;
/// * a disagreement between the descriptor's **item counts** and the generated
///   file's — if `.extern_path` grows another entry, or a message stops being
///   generated, the tables would silently cover a different set of types than
///   the crate compiles.
fn check_derive_dispositions(rhoapi_rs: &str, counts: &schema_codegen::Counts) {
    use std::collections::BTreeSet;

    // Every token inside every `#[derive(...)]` in the generated file.
    //
    // ⚠ The textual pass above leaves DOUBLE SPACES behind where it removed
    // `PartialEq,` (`"#[derive(Clone,  ::prost::Message)]"`), so tokens are
    // split on `,` and trimmed rather than matched positionally.
    let mut found: BTreeSet<&str> = BTreeSet::new();
    let mut cursor = rhoapi_rs;
    while let Some(start) = cursor.find("#[derive(") {
        let rest = &cursor[start + "#[derive(".len()..];
        let Some(end) = rest.find(")]") else {
            panic!(
                "models/build.rs: an unterminated `#[derive(` in the generated `rhoapi.rs`. \
                 The derive cross-check parses that file textually and cannot proceed past a \
                 malformed attribute — a silent skip here would let an undispositioned trait \
                 through, which is the one thing this check exists to prevent."
            );
        };
        for token in rest[..end].split(',') {
            let token = token.trim();
            if !token.is_empty() {
                found.insert(token);
            }
        }
        cursor = &rest[end + 2..];
    }

    assert!(
        !found.is_empty(),
        "models/build.rs: the derive scan found NO `#[derive(...)]` in the generated \
         `rhoapi.rs`. A check that scans nothing passes vacuously, so this is a failure and \
         not a quiet success — either the textual pass stopped emitting derives or this \
         function is reading the wrong string."
    );

    let dispositioned: BTreeSet<&str> = schema_codegen::DERIVE_DISPOSITIONS
        .iter()
        .map(|d| d.token)
        .collect();

    // ★ Through the generator's own lookup, so "is this trait dispositioned?" is
    // answered by the same function a future caller would use rather than by a
    // set this check assembled for itself.
    let undispositioned: Vec<&&str> = found
        .iter()
        .filter(|token| schema_codegen::disposition_of(token).is_none())
        .collect();
    assert!(
        undispositioned.is_empty(),
        "models/build.rs: UNDISPOSITIONED `#[derive]` TRAIT(S) {:?} in the generated \
         `rhoapi.rs`.\n\
         \n\
         Every derive on an `.rhoapi` type produces run-time surfaces, and a recursive one \
         is a Θ(depth) walk over a term a deploy controls. The four-quadrant campaign's \
         driver list is DERIVED from this table precisely so that a new trait cannot join \
         the schema unnoticed — a hand-picked list of four missed `Hash` entirely.\n\
         \n\
         Add a row to `DERIVE_DISPOSITIONS` in `models/codegen/schema_codegen.rs` naming the \
         trait's surfaces and what has been decided about each. `Disposition::NotATraversal` \
         is available and requires only that you say WHY.",
        undispositioned
    );

    let stale: Vec<&&str> = dispositioned.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "models/build.rs: STALE disposition(s) {:?} — `DERIVE_DISPOSITIONS` in \
         `models/codegen/schema_codegen.rs` classifies {} trait tokens, but {:?} appear nowhere \
         in the generated `rhoapi.rs`.\n\
         \n\
         A disposition for a derive that is no longer applied is a claim about code that \
         does not exist, and a table of those is exactly the drift that made the Θ(depth) \
         audit's prose copies useless. Remove the row, or restore the derive.",
        stale,
        dispositioned.len(),
        stale
    );

    // ── the item counts, so the tables and the crate cover the same types ──
    let protobuf_messages = rhoapi_rs.matches("::prost::Message").count();
    let protobuf_oneofs = rhoapi_rs.matches("::prost::Oneof").count();
    assert_eq!(
        protobuf_messages,
        counts.message_count - counts.clone_cut_set.len(),
        "models/build.rs: the generated `rhoapi.rs` carries {} `::prost::Message` derives \
         but schema codegen emitted programs for {} messages, stripped {} feedback-vertex \
         Message derives, and extern-path'd {} more. The tables would otherwise cover a \
         different set of types than the crate compiles.",
        protobuf_messages,
        counts.message_count,
        counts.clone_cut_set.len(),
        counts.extern_count
    );
    assert_eq!(
        protobuf_oneofs, counts.oneof_count,
        "models/build.rs: the generated `rhoapi.rs` carries {} `::prost::Oneof` derives but \
         schema codegen resolved {} oneofs. The variant index tables are what the \
         bincode decoder dispatches on, so a missing oneof is a mis-decode, not a gap.",
        protobuf_oneofs, counts.oneof_count
    );
}
