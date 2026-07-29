extern crate tonic_prost_build;

// https://docs.rs/prost-build/latest/prost_build/struct.Config.html
// https://docs.rs/tonic-build/latest/tonic_build/struct.Builder.html#

use std::path::Path;
use std::{env, fs};

use prost::Message as _;

/// The wire-schema generator: ONE table, emitted from the protobuf descriptor,
/// consumed by BOTH the serializer (`wire_encode`) and the deserializer
/// (`par_codec`). See its module docs for why this is a build-script pass and
/// not a proc-macro.
#[path = "build/wire_schema.rs"]
mod wire_schema;

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
    // `build/wire_schema.rs` leaves a STALE generated table in `OUT_DIR` while
    // the build reports success. That is a silent, byte-visible divergence
    // between the generator in the tree and the table in the binary; it was
    // observed once, during this module's development, and cost a confusing
    // benchmark result.
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("build/wire_schema.rs").display()
    );

    // The descriptor set is what the wire-schema generator reads. It carries
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
        // EPathMap fix P3 (stage L1.5): `.rhoapi.EPathMap` is EXTERN — prost
        // does not generate the struct; every generated reference (the
        // `EPathmapBody` oneof variant, `EZipper.pathmap`) resolves to the
        // hand-maintained wrapper in models/src/rust/rhoapi_ext.rs, which
        // replicates the generated type's prost/serde/Ord/Debug behavior
        // byte-identically (P0-golden-gated) and adds the private shadow
        // cell for O(1) intern rendezvous + cached canonical-bytes encodes.
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

    // Remove PartialEq from specific generated structs from rhoapi.rs
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let file_path = format!("{}/rhoapi.rs", out_dir);
    let content = fs::read_to_string(&file_path).expect("Unable to read file");

    let modified_content = content
        .lines()
        .map(|line| {
            if line.contains("#[derive(Clone, PartialEq, ::prost::Message)]")
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
            }
        })
        .collect::<Vec<String>>()
        .join("\n");

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
    // Stage 2: the wire-schema pass — ONE walk, FOUR outputs
    // -----------------------------------------------------------------------
    //
    // This extends an existing two-stage pipeline: the textual pass above
    // already post-processes prost's output, and this pass reads the same
    // compilation's descriptor set.
    let descriptor_bytes = fs::read(&descriptor_path).expect("read the protobuf descriptor set");
    let descriptor_set = prost_types::FileDescriptorSet::decode(&descriptor_bytes[..])
        .expect("decode the protobuf descriptor set");
    let generated = wire_schema::generate(&descriptor_set);
    let counts = &generated.counts;

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
         `locally_free` declarations, but the wire-schema generator classified {} fields as \
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
    // `Ord`/`PartialOrd`, which nobody had named. `wire_schema.rs` holds the
    // closed `DERIVE_DISPOSITIONS` table; this scans the post-processed
    // `rhoapi.rs` for the tokens actually present and requires the two to agree
    // as SETS, in both directions.
    check_derive_dispositions(&modified_content, counts);

    // ── the four outputs of the one pass ──
    assert_eq!(
        generated.sources.len(),
        4,
        "models/build.rs: the wire-schema pass must produce exactly four outputs \
         (bincode table, prost table, term-op slot, schema meta); it produced {}. \
         `models/src/rust/rholang/mod.rs` includes four modules and a missing file is a \
         compile error whose message names `OUT_DIR`, not this pass.",
        generated.sources.len()
    );
    for (name, source) in &generated.sources {
        assert!(
            !source.is_empty(),
            "models/build.rs: the wire-schema pass produced an EMPTY `{name}`. Even the \
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
        "wire_schema: {} generated messages + {} extern, {} oneofs, {} serialize-only \
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
fn check_derive_dispositions(rhoapi_rs: &str, counts: &wire_schema::Counts) {
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

    let dispositioned: BTreeSet<&str> = wire_schema::DERIVE_DISPOSITIONS
        .iter()
        .map(|d| d.token)
        .collect();

    // ★ Through the generator's own lookup, so "is this trait dispositioned?" is
    // answered by the same function a future caller would use rather than by a
    // set this check assembled for itself.
    let undispositioned: Vec<&&str> = found
        .iter()
        .filter(|token| wire_schema::disposition_of(token).is_none())
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
         Add a row to `DERIVE_DISPOSITIONS` in `models/build/wire_schema.rs` naming the \
         trait's surfaces and what has been decided about each. `Disposition::NotATraversal` \
         is available and requires only that you say WHY.",
        undispositioned
    );

    let stale: Vec<&&str> = dispositioned.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "models/build.rs: STALE disposition(s) {:?} — `DERIVE_DISPOSITIONS` in \
         `models/build/wire_schema.rs` classifies {} trait tokens, but {:?} appear nowhere \
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
    let prost_messages = rhoapi_rs.matches("::prost::Message").count();
    let prost_oneofs = rhoapi_rs.matches("::prost::Oneof").count();
    assert_eq!(
        prost_messages, counts.message_count,
        "models/build.rs: the generated `rhoapi.rs` carries {} `::prost::Message` derives \
         but the wire-schema pass emitted programs for {} messages ({} more are \
         `.extern_path`'d). The tables would cover a different set of types than the crate \
         compiles, and the difference would show up as a byte-level surprise rather than a \
         compile error.",
        prost_messages, counts.message_count, counts.extern_count
    );
    assert_eq!(
        prost_oneofs, counts.oneof_count,
        "models/build.rs: the generated `rhoapi.rs` carries {} `::prost::Oneof` derives but \
         the wire-schema pass resolved {} oneofs. The variant index tables are what the \
         bincode decoder dispatches on, so a missing oneof is a mis-decode, not a gap.",
        prost_oneofs, counts.oneof_count
    );
}
