extern crate tonic_prost_build;

// https://docs.rs/prost-build/latest/prost_build/struct.Config.html
// https://docs.rs/tonic-build/latest/tonic_build/struct.Builder.html#

use std::path::Path;
use std::{env, fs};

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
        "RhoTypes.proto",
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

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .btree_map(".")
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
    let modified_content = modified_content.replace(
        "pub locally_free: ::prost::alloc::vec::Vec<u8>,",
        "#[serde(serialize_with = \"crate::rust::serde_helpers::serialize_as_empty_bytes\")]\n    pub locally_free: ::prost::alloc::vec::Vec<u8>,",
    );

    let modified_content = [
        "cost_signed_terms",
        "cost_stacks",
        "cost_authority",
        "cost_stack",
        "cost_signature",
    ]
    .iter()
    .fold(modified_content, |content, field| {
        content.replace(
            &format!("    pub {field}: "),
            &format!(
                "    #[serde(serialize_with = \"crate::rust::record_encoding::serialize_cost_field\", deserialize_with = \"crate::rust::record_encoding::deserialize_cost_field\", default)]\n    pub {field}: "
            ),
        )
    });

    let modified_content = with_empty_vec_clone_fast_path(&modified_content);

    fs::write(file_path, modified_content).expect("Unable to write file");
}

fn with_empty_vec_clone_fast_path(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut output: Vec<String> = Vec::with_capacity(lines.len());
    let mut impls: Vec<String> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let derive = line.trim() == "#[derive(Clone,  ::prost::Message)]";
        let name = lines.get(index + 1).and_then(|next| {
            next.strip_prefix("pub struct ")
                .and_then(|rest| rest.strip_suffix(" {"))
                .filter(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        });
        match (derive, name) {
            (true, Some(name)) => {
                let mut fields = Vec::new();
                let mut end = index + 2;
                while end < lines.len() && lines[end] != "}" {
                    if let Some(field) = lines[end].trim().strip_prefix("pub ") {
                        if let Some((field_name, field_type)) = field.split_once(": ") {
                            fields.push((
                                field_name.to_string(),
                                field_type
                                    .trim_end_matches(',')
                                    .starts_with("::prost::alloc::vec::Vec<"),
                            ));
                        }
                    }
                    end += 1;
                }
                output.push(line.replace("Clone,  ", ""));
                let body = fields
                    .iter()
                    .map(|(field, is_vec)| {
                        if *is_vec {
                            format!(
                                "            {field}: if self.{field}.is_empty() {{ ::prost::alloc::vec::Vec::new() }} else {{ self.{field}.clone() }},"
                            )
                        } else {
                            format!("            {field}: ::core::clone::Clone::clone(&self.{field}),")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                impls.push(format!(
                    "impl ::core::clone::Clone for {name} {{\n    #[inline]\n    fn clone(&self) -> Self {{\n        {name} {{\n{body}\n        }}\n    }}\n}}"
                ));
            }
            _ => output.push(line.to_string()),
        }
        index += 1;
    }
    output.extend(impls);
    output.join("\n")
}
