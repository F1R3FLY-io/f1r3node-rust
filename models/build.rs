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

    fs::write(file_path, modified_content).expect("Unable to write file");

    // -----------------------------------------------------------------------
    // Stage 2: the wire-schema table (ONE table, both directions)
    // -----------------------------------------------------------------------
    //
    // This extends an existing two-stage pipeline: the textual pass above
    // already post-processes prost's output, and this pass reads the same
    // compilation's descriptor set.
    let descriptor_bytes = fs::read(&descriptor_path).expect("read the protobuf descriptor set");
    let descriptor_set = prost_types::FileDescriptorSet::decode(&descriptor_bytes[..])
        .expect("decode the protobuf descriptor set");
    let generated = wire_schema::generate(&descriptor_set);

    // ★ THE ANTI-DRIFT CROSS-CHECK. The textual pass above and the generator
    // must be applying the SAME rule to the SAME fields. The textual pass
    // counts the `locally_free` declarations it rewrote; the generator counts
    // the fields it classified `FieldKind::EmptyBytes`. If they ever disagree,
    // one of the two has silently stopped covering a field — and since the
    // rewrite is what makes `locally_free` serialize as eight zero bytes, a
    // divergence is a byte-level consensus hazard, not a cosmetic one.
    assert_eq!(
        locally_free_sites, generated.empty_bytes_fields,
        "models/build.rs: the serialize_as_empty_bytes TEXTUAL pass rewrote {} \
         `locally_free` declarations, but the wire-schema generator classified {} fields as \
         EmptyBytes. The two rules have drifted apart; one of them no longer covers every \
         `locally_free` field, and the serialize-only blanking is a consensus-visible \
         normalization.",
        locally_free_sites, generated.empty_bytes_fields
    );

    fs::write(out_dir_path.join("rhoapi_wire.rs"), &generated.source)
        .expect("write the generated wire schema");

    // Recorded in the build script's captured stdout (`target/*/build/models-*/output`)
    // rather than as a `cargo:warning`, so the evidence is retained without
    // decorating every build of every dependent crate.
    println!(
        "wire_schema: {} messages, {} oneofs, {} serialize-only `locally_free` fields \
         (cross-checked against the textual pass)",
        generated.message_count, generated.oneof_count, generated.empty_bytes_fields
    );
}
