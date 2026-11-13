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

use models::rhoapi::Par;
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
    Par::wrap_unary_elist(depth, new_gint_par(0, Vec::new(), false))
        .expect("requested unary-list depth is addressable")
}

fn par_depth(par: &Par) -> usize {
    par.canonical_unary_elist_gint_parts()
        .filter(|(_depth, terminal)| *terminal == 0)
        .map(|(depth, _terminal)| depth)
        .expect("fixture is exactly a unary EList spine ending in GInt(0)")
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

const CAP_WORDS: &[&str] = &[
    "MAX", "LIMIT", "LIMITS", "CAP", "CAPACITY", "BUDGET", "CEILING",
];

const CAP_SCAN_DIRECTORIES: &[(&str, usize)] = &[
    ("rholang/src/rust/interpreter/matcher", 10),
    ("models/src/rust/rholang", 60),
];

const CAP_SCAN_FILES: &[&str] = &[
    "rspace++/src/rspace/data_index.rs",
    "rspace++/src/rspace/retained_set_automaton.rs",
    "rspace++/src/rspace/rho_path.rs",
    "rspace++/src/rspace/space_matcher.rs",
];

const MINIMUM_CONSTANT_ITEMS: usize = 175;

const PREALLOCATION_HINT: &str = "preallocation hint";
const REUSE_POOL_OR_CACHE_BOUND: &str = "reuse-pool or cache bound";
const FORMAT_OR_STRUCTURAL_BOUND: &str = "format or structural bound that no valid input exceeds";
const DEBUG_ONLY_INVARIANT_CHECK: &str = "debug-only invariant check";

const ADMISSIBLE_CAP_CLASSES: &[&str] = &[
    PREALLOCATION_HINT,
    REUSE_POOL_OR_CACHE_BOUND,
    FORMAT_OR_STRUCTURAL_BOUND,
    DEBUG_ONLY_INVARIANT_CHECK,
];

const CAP_DISPOSITIONS: &[(&str, &str, &str, &str)] = &[
    (
        "models/src/rust/rholang/bincode_decoder.rs",
        "MAX_PREALLOC_BYTES",
        PREALLOCATION_HINT,
        "caps only the capacity that cautious_capacity hands to Vec::with_capacity",
    ),
    (
        "models/src/rust/rholang/bincode_encoder.rs",
        "OP_STACK_CAPACITY",
        PREALLOCATION_HINT,
        "initial capacity of the pooled op stack, which grows geometrically past it",
    ),
    (
        "models/src/rust/rholang/bincode_encoder.rs",
        "OUT_CAPACITY",
        PREALLOCATION_HINT,
        "initial capacity of an output buffer; the reused buffer shrinks back to it",
    ),
    (
        "models/src/rust/rholang/bincode_encoder.rs",
        "MAX_POOLED_OPS",
        REUSE_POOL_OR_CACHE_BOUND,
        "an op stack with a larger capacity is dropped instead of parked in the thread pool",
    ),
    (
        "models/src/rust/rholang/canonical_source_tree.rs",
        "RELEASE_CAPACITY",
        FORMAT_OR_STRUCTURAL_BOUND,
        "release frontier 1 + 2 * height of a balanced tree whose height stays below usize::BITS",
    ),
    (
        "models/src/rust/rholang/drive.rs",
        "WORK_CAPACITY",
        PREALLOCATION_HINT,
        "initial capacity of the work stack that drive allocates",
    ),
    (
        "models/src/rust/rholang/drive.rs",
        "VAL_CAPACITY",
        PREALLOCATION_HINT,
        "initial capacity of the value stack that drive allocates",
    ),
    (
        "models/src/rust/rholang/drive.rs",
        "MAX_TAILS_PER_DESCENT",
        DEBUG_ONLY_INVARIANT_CHECK,
        "read only by the debug_assertions Ledger; the release Ledger has empty methods",
    ),
    (
        "models/src/rust/rholang/protobuf_encoder.rs",
        "OP_STACK_CAPACITY",
        PREALLOCATION_HINT,
        "initial capacity of the op and frame stacks of the length pass",
    ),
    (
        "models/src/rust/rholang/protobuf_encoder.rs",
        "LEN_TABLE_CAPACITY",
        PREALLOCATION_HINT,
        "starting capacity of the length table, which grows geometrically past it",
    ),
    (
        "models/src/rust/rholang/protobuf_encoder.rs",
        "OUT_CAPACITY",
        PREALLOCATION_HINT,
        "initial capacity of the output buffer of op_stack_high_water",
    ),
    (
        "models/src/rust/rholang/protobuf_schema.rs",
        "MAX_SCHEMA_DEPTH",
        FORMAT_OR_STRUCTURAL_BOUND,
        "compile-time fact of the descriptor graph, not a run-time traversal limit",
    ),
];

const CAP_FIXTURE: &str = concat!(
    "const MAX_FIXTURE_DEPTH: usize = 1;\n",
    "// const MAX_COMMENTED_OUT: usize = 2;\n",
    "    /// static FIXTURE_DOCUMENTED_LIMIT: usize = 3;\n",
    "pub(crate) static mut FIXTURE_BUDGET: u8 = 4;\n",
    "lazy_static! { static ref FIXTURE_CAP: usize = 5; }\n",
    "struct Ring<const CAPACITY: usize>;\n",
    "impl<T, const LIMIT: usize> Bounded for [T; LIMIT] {}\n",
    "const fn fixture_capacity() -> usize { 6 }\n",
    "fn pointers<T>() where *const T: Send, &'static T: Sized {}\n",
    "trait Bounded {\n    const FIXTURE_CEILING: usize;\n}\n",
    "const FIXTURE_THRESHOLD: usize = 7;\n",
);

type CapLikeConstants = std::collections::BTreeMap<(String, String), usize>;

fn is_identifier_byte(byte: u8) -> bool { byte.is_ascii_alphanumeric() || byte == b'_' }

fn constant_item_names(code: &str) -> Vec<&str> {
    let bytes = code.as_bytes();
    let mut names = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if !is_identifier_byte(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_identifier_byte(bytes[at]) {
            at += 1;
        }
        if !matches!(&code[start..at], "const" | "static") {
            continue;
        }
        if code[..start].trim_end().ends_with(['<', ',', '*', '\'']) {
            continue;
        }
        let mut rest = code[at..].trim_start();
        for qualifier in ["mut", "ref"] {
            rest = rest
                .strip_prefix(qualifier)
                .filter(|tail| tail.starts_with(char::is_whitespace))
                .map_or(rest, str::trim_start);
        }
        let length = rest
            .bytes()
            .take_while(|byte| is_identifier_byte(*byte))
            .count();
        let after = rest[length..].trim_start();
        if length > 0 && after.starts_with(':') && !after.starts_with("::") {
            names.push(&rest[..length]);
        }
    }
    names
}

fn is_cap_like(name: &str) -> bool {
    name.split('_')
        .any(|word| CAP_WORDS.iter().any(|cap| word.eq_ignore_ascii_case(cap)))
}

fn cap_like_constants(file: &str, source: &str) -> (usize, CapLikeConstants) {
    let code = code_without_line_comments(source);
    let names = constant_item_names(&code);
    let mut constants = CapLikeConstants::new();
    for name in names.iter().filter(|name| is_cap_like(name)) {
        *constants
            .entry((file.to_owned(), (*name).to_owned()))
            .or_insert(0) += 1;
    }
    (names.len(), constants)
}

fn constants_without_a_row(constants: &CapLikeConstants) -> Vec<String> {
    constants
        .keys()
        .filter(|(file, name)| {
            !CAP_DISPOSITIONS.iter().any(|(row_file, row_name, ..)| {
                *row_file == file.as_str() && *row_name == name.as_str()
            })
        })
        .map(|(file, name)| format!("{file}: {name}"))
        .collect()
}

#[test]
fn every_cap_like_constant_of_the_matcher_sources_has_a_disposition() {
    let (fixture_items, fixture_constants) = cap_like_constants("fixture", CAP_FIXTURE);
    assert_eq!(
        fixture_items, 5,
        "the fixture declares five const and static items outside comment lines and generic \
         parameter lists"
    );
    assert_eq!(
        constants_without_a_row(&fixture_constants),
        [
            "fixture: FIXTURE_BUDGET",
            "fixture: FIXTURE_CAP",
            "fixture: FIXTURE_CEILING",
            "fixture: MAX_FIXTURE_DEPTH",
        ],
        "the scanner must report exactly the four cap-like constants of the fixture as unlisted"
    );

    let root = workspace_root();
    let mut sources = Vec::new();
    for (directory, minimum_sources) in CAP_SCAN_DIRECTORIES {
        let found = rust_sources_below(&root.join(directory));
        assert!(
            found.len() >= *minimum_sources,
            "{directory}: found {} Rust sources, expected at least {minimum_sources}",
            found.len()
        );
        sources.extend(found);
    }
    sources.extend(CAP_SCAN_FILES.iter().map(|file| root.join(file)));

    let mut items = 0usize;
    let mut declarations = 0usize;
    let mut constants = CapLikeConstants::new();
    for source in &sources {
        let file = source
            .strip_prefix(&root)
            .expect("a scanned source is below the workspace root")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(source)
            .unwrap_or_else(|error| panic!("cannot read {file}: {error}"));
        let (found_items, found_constants) = cap_like_constants(&file, &text);
        items += found_items;
        declarations += found_constants.values().sum::<usize>();
        constants.extend(found_constants);
    }
    assert!(
        items >= MINIMUM_CONSTANT_ITEMS,
        "the scan found {items} const and static items, expected at least {MINIMUM_CONSTANT_ITEMS}"
    );
    println!(
        "scanned {} Rust sources and {items} const and static items; {declarations} declarations \
         of {} cap-like constants",
        sources.len(),
        constants.len()
    );

    let unlisted = constants_without_a_row(&constants);
    assert!(
        unlisted.is_empty(),
        "a cap-like constant has no row in CAP_DISPOSITIONS. Give it one of the four classes and \
         a one-line reason read from the code. A bound that refuses, truncates or changes a \
         result when a valid input reaches it has no admissible class and must not get a row:\n{}",
        unlisted.join("\n")
    );

    let mut rows = std::collections::BTreeSet::new();
    for (file, name, class, reason) in CAP_DISPOSITIONS {
        assert!(
            rows.insert((*file, *name)),
            "{file}: {name} has more than one row in CAP_DISPOSITIONS"
        );
        assert!(
            constants.contains_key(&((*file).to_owned(), (*name).to_owned())),
            "{file}: {name} has a row in CAP_DISPOSITIONS but is not declared in that file"
        );
        assert!(
            ADMISSIBLE_CAP_CLASSES.contains(class),
            "{file}: {name} has the class `{class}`, which is not one of the four admissible ones"
        );
        assert!(
            !reason.trim().is_empty() && !reason.contains('\n'),
            "{file}: {name} needs a one-line reason"
        );
    }
}
