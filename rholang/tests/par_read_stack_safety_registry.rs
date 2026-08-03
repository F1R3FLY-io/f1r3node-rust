//! Registry and executable closure gate for every production `Par` byte reader.
//!
//! The old registry classified call sites by prost's derived recursion limit.
//! That classification became actively misleading once `Par`'s generated
//! `Message` implementation and the dedicated protobuf decoder both became
//! explicit pushdown machines. Reader totality is now a property of the shared
//! primitive, not something each call site can weaken independently.
//!
//! This gate therefore checks three architectural facts:
//!
//! 1. consensus replay enters the generated protobuf decoder;
//! 2. all remaining direct `Par::decode` users enter the manual, generated
//!    stack-safe `Message` implementation;
//! 3. retired depth-limit and stack-growth symbols do not return to production
//!    source or configuration.

use std::path::{Path, PathBuf};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::rholang::{protobuf_decoder, protobuf_encoder};
use models::rust::utils::new_gint_par;
use prost::Message;
use rholang::rust::interpreter::dispatch::decode_non_deterministic_output;

const DEPTH: usize = 4_096;
const SMALL_STACK: usize = 256 * 1024;

const PRODUCTION_ROOTS: &[&str] = &[
    "models/src",
    "rholang/src",
    "rspace++/src",
    "rspace++/libs",
    "casper/src",
    "node/src",
    "rho-pure-eval/src",
];

const RETIRED_PRODUCTION_TOKENS: &[&str] = &[
    "STABILITY_DESCEND_BUDGET",
    "COLLECTION_DEPTH_LIMIT:",
    "SCANNER_STACK_CEILING:",
    "stacker::maybe_grow(",
    "StackGrowingFuture {",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rholang has a workspace parent")
        .to_path_buf()
}

fn rust_sources_below(root: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some("target") {
                    pending.push(path);
                }
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                sources.push(path);
            }
        }
    }
    sources
}

fn code_without_line_comments(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn nested_list(depth: usize) -> Par {
    let mut par = new_gint_par(0, Vec::new(), false);
    for _ in 0..depth {
        par = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![par],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
    }
    par
}

fn par_depth(par: &Par) -> usize {
    let mut depth = 0usize;
    let mut cursor = par;
    loop {
        match cursor
            .exprs
            .first()
            .and_then(|expr| expr.expr_instance.as_ref())
        {
            Some(ExprInstance::EListBody(list)) if !list.ps.is_empty() => {
                depth += 1;
                cursor = &list.ps[0];
            }
            _ => return depth,
        }
    }
}

#[test]
fn consensus_output_reader_is_the_generated_protobuf_pda() {
    let source =
        std::fs::read_to_string(workspace_root().join("rholang/src/rust/interpreter/dispatch.rs"))
            .expect("read dispatch source");
    let code = code_without_line_comments(&source);
    assert!(
        code.contains("models::rust::rholang::protobuf_decoder::decode_par("),
        "consensus output decoding no longer enters the generated protobuf PDA"
    );
    assert!(
        !code.contains("Par::decode("),
        "dispatch restored the derived-reader spelling instead of the generated PDA"
    );
}

#[test]
fn production_output_reader_accepts_depth_4096_on_a_fixed_small_stack() {
    std::thread::Builder::new()
        .name("output-reader-pda-depth-4096".to_owned())
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let term = nested_list(DEPTH);
            let bytes = protobuf_encoder::encode_to_vec(&term);
            let direct = protobuf_decoder::decode_par(bytes.as_slice())
                .expect("generated protobuf decoder accepts depth 4096");
            let dispatched = decode_non_deterministic_output(&[bytes])
                .expect("production output reader accepts depth 4096");
            assert_eq!(par_depth(&direct), DEPTH);
            assert_eq!(dispatched.len(), 1);
            assert_eq!(par_depth(&dispatched[0]), DEPTH);
        })
        .expect("spawn fixed-small-stack output-reader probe")
        .join()
        .expect("production output reader must not consume native stack by term depth");
}

#[test]
fn direct_message_reader_uses_the_stack_safe_generated_impl() {
    std::thread::Builder::new()
        .name("par-message-pda-depth-4096".to_owned())
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let term = nested_list(DEPTH);
            let bytes = term.encode_to_vec();
            let decoded = Par::decode(bytes.as_slice())
                .expect("manual generated Message implementation accepts depth 4096");
            assert_eq!(par_depth(&decoded), DEPTH);
            assert_eq!(decoded.encode_to_vec(), bytes);
        })
        .expect("spawn fixed-small-stack Message probe")
        .join()
        .expect("Message decode/encode/drop must remain stack-safe");
}

#[test]
fn retired_depth_and_stack_growth_mechanisms_are_absent_from_production() {
    let root = workspace_root();
    let mut violations = Vec::new();
    for relative in PRODUCTION_ROOTS {
        for path in rust_sources_below(&root.join(relative)) {
            let source = std::fs::read_to_string(&path).expect("read production Rust source");
            let code = code_without_line_comments(&source);
            for token in RETIRED_PRODUCTION_TOKENS {
                if code.contains(token) {
                    violations.push(format!("{}: `{token}`", path.display()));
                }
            }
        }
    }

    let cargo_config = std::fs::read_to_string(root.join(".cargo/config.toml")).unwrap_or_default();
    if cargo_config.contains("RUST_MIN_STACK") {
        violations.push(".cargo/config.toml: `RUST_MIN_STACK`".to_owned());
    }

    assert!(
        violations.is_empty(),
        "retired traversal limit or native-stack workaround returned:\n{}",
        violations.join("\n")
    );
}
