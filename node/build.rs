extern crate tonic_prost_build;

mod build_identity;

use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).to_path_buf();
    let proto_src_dir = manifest_dir.join("src/main/protobuf");
    let scala_proto_base_dir = manifest_dir.join("src");

    let proto_src_models_dir = manifest_dir.join("../models/src/main/protobuf");

    let proto_files = ["lsp.proto", "repl.proto"];

    let models_proto_files = ["DeployServiceV1.proto", "ProposeServiceV1.proto"];

    let absolute_proto_files: Vec<_> = proto_files.iter().map(|f| proto_src_dir.join(f)).collect();
    let absolute_models_proto_files: Vec<_> = models_proto_files
        .iter()
        .map(|f| proto_src_models_dir.join(f))
        .collect();

    let proto_files = [absolute_proto_files, absolute_models_proto_files].concat();

    // Rerun if any of the proto files would be changed
    for entry in proto_files.iter() {
        println!("cargo:rerun-if-changed={}", entry.display());
    }
    // Also watch the scalapb proto used for imports
    println!(
        "cargo:rerun-if-changed={}",
        scala_proto_base_dir.join("scalapb/scalapb.proto").display()
    );

    tonic_prost_build::configure()
        .file_descriptor_set_path("build/descriptors/reflection_protos.bin")
        .build_client(true)
        .build_server(true)
        .btree_map(".")
        .message_attribute(".", "#[repr(C)]")
        .bytes(".")
        .compile_protos(&proto_files, &[
            proto_src_dir.clone(),
            proto_src_models_dir.clone(),
            manifest_dir.clone(),
            scala_proto_base_dir.clone(),
        ])
        .expect("Failed to compile proto files");

    let explicit = env::var("F1R3NODE_BUILD_ID")
        .map(Some)
        .unwrap_or_else(|error| match error {
            env::VarError::NotPresent => None,
            env::VarError::NotUnicode(_) => panic!("F1R3NODE_BUILD_ID must contain valid UTF-8"),
        });
    let build_id = build_identity::resolve_build_id(explicit.as_deref(), || {
        Command::new("git")
            .current_dir(&manifest_dir)
            .args(["describe", "--tags", "--always"])
            .output()
    })
    .unwrap_or_else(|error| panic!("{error}"));

    println!("cargo:rustc-env=F1R3NODE_BUILD_ID={build_id}");
    println!("cargo:rerun-if-env-changed=F1R3NODE_BUILD_ID");
    println!("cargo:rerun-if-changed=build_identity.rs");

    for name in ["HEAD", "refs", "packed-refs"] {
        let path = Command::new("git")
            .current_dir(&manifest_dir)
            .args(["rev-parse", "--path-format=absolute", "--git-path", name])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok());
        if let Some(path) = path {
            let path = Path::new(path.trim());
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}
