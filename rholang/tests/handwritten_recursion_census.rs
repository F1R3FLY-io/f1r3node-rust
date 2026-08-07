//! ★★★ **The DERIVED census of hand-written recursion over the term family.**
//!
//! # The gap this closes
//!
//! Three surfaces can grow a Θ(depth) traversal, and until this file only two of them could
//! notice:
//!
//! | surface | census | mechanism |
//! |---|---|---|
//! | **derived impls** | ✅ | `DERIVE_DISPOSITIONS` in `models/codegen/schema.rs` is a closed table over derive TOKENS; `models/build.rs` **fails the build** on an unknown one. |
//! | **generated files** (mettail) | ✅ | `GENERATED_FILE_CENSUS` + `defines_a_function_it_also_calls`, deliberately loose in the safe direction. |
//! | **hand-written traversals** | ❌ **nothing** | ⇐ this file |
//!
//! ## The witness, and why a SELF-CALL DETECTOR IS NOT ENOUGH
//!
//! `#121` was `pathmap_crate_type_mapper.rs`'s `eval_stable_par` ⇄ `eval_stable_expr`,
//! mutually recursive and unbounded through `EList.ps`/`ETuple.ps`. It is the ground-domain
//! gate — the predicate selecting proto field 8 versus the tag-1 walk — so it ran on **every
//! segment of every trie key**. It appeared in **no depth audit and no `TRIPWIRE_DEPTH`**,
//! and was found only by BISECTION, when a 256 KiB probe thread built to prove an unrelated
//! encoder fix **overflowed anyway and the encoder was not the frame that ran out**.
//!
//! ⚠★★ **A 2-cycle is invisible to every detector that looks for a function calling itself.**
//! Measured on this tree: **550 recursive components, of which 469 are self-calls and 81 are
//! MUTUAL.** A self-call detector reports the 469 and misses all 81 — including #121's.
//!
//! ⇒ This census computes **strongly connected components** of the call graph, so a cycle of
//! any length is one object. `the_census_sees_the_121_family` pins that it still finds the
//! defect that motivated it.
//!
//! # Why the nodes are `(file, function, source offset)` and not `function`
//!
//! ⚠ A name-keyed graph merges every `new`, `default` and `clone` in the workspace into single
//! nodes. Measured: it reported one **612-member** "mutual recursion" that was entirely an
//! artefact of the merge — a number with no referent, which is the exact failure this campaign
//! keeps finding. The source offset is also essential: Rust traits commonly define the same
//! method name in several impl blocks, and keying only by `(file, name)` silently retained just
//! the last body. That is precisely how the recursive `SpatialMatcher` impl family escaped the
//! earlier census. Calls conservatively reach every same-file definition with that name; a
//! cross-module edge is admitted only when every definition is in **exactly one** file. That
//! keeps genuinely cross-file cycles visible — `normalize.rs` ⇄
//! `normalize_drive.rs` ⇄ `p_input_normalizer.rs` is one, and a per-file scan cannot see it.
//!
//! # Loose in the safe direction, deliberately
//!
//! The scanner is textual: it finds `fn NAME`, delimits the body by brace count, and treats any
//! `ident(` inside it as a call. It therefore **over-reports** — a `#[test]` function in the
//! same file joins its neighbours' component — and it does not see through macros. Both are the
//! safe direction, and both follow the `defines_a_function_it_also_calls` precedent, whose own
//! doc says it *"counts helper reuse (`new(`) as recursion"*. **A census that under-reports is
//! worse than no census, because it certifies coverage it does not have.**
//!
//! # What this file does NOT establish
//!
//! * **It cannot see recursion that exists only after macro expansion.** mettail's
//!   `dovetail_report` is `term_depth`'s 40-site caller and every one of those sites is
//!   emitted; a source scan finds only the emitter's `quote!` fragments. That surface is
//!   `GENERATED_FILE_CENSUS`'s, in the other repository.
//! * **It cannot prove a component is BOUNDED.** It reports that a cycle exists over the term
//!   family; whether the cycle terminates within a stack is what `stack_depth_gate.rs`
//!   measures. A row here is an obligation to have an answer, not the answer.
//! * **It does not resolve trait dispatch.** A cycle closed through a `dyn` call or a generic
//!   instantiation is invisible to a syntactic scan.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Crate source roots, relative to the workspace. ★ Scope (a) — **both repos, all crates** —
/// is the owner's ruling; this is the f1r3node half.
const CRATE_ROOTS: &[&str] = &[
    "models/src",
    "rholang/src",
    "casper/src",
    "rspace++/src",
    "shared/src",
    "comm/src",
    "crypto/src",
    "block-storage/src",
    "rho-pure-eval/src",
    "node/src",
];

/// The recursive term family. A component is IN SCOPE when any of its functions mentions one
/// of these, which is the property that makes a cycle a depth exposure rather than merely a
/// loop.
///
/// ⚠ Matched as whole words against the function's text — deliberately coarse, because a
/// narrow match is how a traversal escapes a census.
const TERM_FAMILY: &[&str] = &[
    "Par",
    "Expr",
    "Send",
    "Receive",
    "New",
    "Match",
    "Bundle",
    "Connective",
    "EList",
    "ETuple",
    "ESet",
    "EMap",
    "EPathMap",
    "Proc",
];

/// What is known about a file's hand-written recursion over the term family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// A deliberate reference implementation, retained so a converted twin can be differentialled
    /// against it. Its recursion is the point.
    OracleTwin(&'static str),
    /// Measured by a `stack_depth_gate.rs` subject. The cycle exists and its cost is known.
    Measured(&'static str),
    /// The cycle is over a bounded structure rather than the term's own depth — a fixed-arity
    /// walk, a config tree, a transport handshake.
    NotATermDepthCycle(&'static str),
}

/// ★★ **The declared expectation. The FILE SET is derived; only the disposition is declared.**
///
/// This is `DERIVE_DISPOSITIONS`' shape and `GENERATED_FILE_CENSUS`' shape, for the third
/// surface: the census computes which files contain a term-family cycle, and this table says
/// what is known about each. A file that grows one without a row **fails this test by name**.
///
/// ⚠ It is NOT a list of cycles. 550 components over 205 files is far too many to enumerate,
/// and enumerating them would be *"complete the list"* — the non-repair this campaign has
/// shipped four times. A component is also not a stable identity: adding one helper renumbers
/// it. **A file is stable, and a file's traversal family is what a disposition is about.**
const RECURSION_DISPOSITIONS: &[(&str, Disposition)] = &[
    // ── the reducer and its method surface ─────────────────────────────────────────
    (
        "rholang/src/rust/interpreter/reduce.rs",
        Disposition::Measured(
            "production expression descent is `eval_drive`; async process descent is the \
             detached counted-task driver. Gate subjects \
             `async_reducer_scc_depth_4096_uses_a_fixed_small_native_stack`, the reported deploy \
             reproducer in `deploy_depth_ceiling`, and the async-driver differential cover the \
             formerly recursive consensus path",
        ),
    ),
    (
        "rholang/src/rust/interpreter/fused_pathmap_chain.rs",
        Disposition::Measured(
            "the recognizer and replay are explicit loops; gate subject \
             `fused_chain_depth_4096_uses_a_fixed_small_native_stack` drives the whole EMethod \
             spine on 256 KiB",
        ),
    ),
    (
        "rholang/src/rust/interpreter/reduce_expression_oracle.rs",
        Disposition::OracleTwin(
            "the six pre-PDA expression evaluators retained only for result/charge \
             differentials; the source is separate so its deliberate recursion cannot be \
             mistaken for the production reducer",
        ),
    ),
    // ── printers ───────────────────────────────────────────────────────────────────
    (
        "rholang/src/rust/interpreter/pretty_printer.rs",
        Disposition::Measured("gate subject `pretty`"),
    ),
    // ── normalizer ─────────────────────────────────────────────────────────────────
    (
        "rholang/src/rust/interpreter/compiler/normalize_recursive.rs",
        Disposition::OracleTwin(
            "★ the 26-member SCC oracle twin, extracted by normalize_oracle_provenance.rs. Its \
             recursion is deliberate and is the reference the converted normalizer is \
             differentialled against",
        ),
    ),
    (
        "rholang/src/rust/interpreter/compiler/normalize.rs",
        Disposition::Measured("gate subject `normalize`; the production path is converted"),
    ),
    (
        "rholang/src/rust/interpreter/compiler/normalize_drive.rs",
        Disposition::Measured("the driver half of `normalize`"),
    ),
    (
        "rholang/src/rust/interpreter/compiler/normalizer/processes/p_input_normalizer.rs",
        Disposition::Measured(
            "★ participates in a CROSS-FILE cycle with normalize.rs and normalize_drive.rs — \
             the shape a per-file scan structurally cannot see",
        ),
    ),
    // ── substitution ───────────────────────────────────────────────────────────────
    (
        "rholang/src/rust/interpreter/substitute_oracle.rs",
        Disposition::OracleTwin("the pre-conversion substitution, held for the differential"),
    ),
    (
        "rholang/src/rust/interpreter/substitute.rs",
        Disposition::Measured("gate subjects `substitute`, `substitute_no_sort`"),
    ),
    // ── matcher ────────────────────────────────────────────────────────────────────
    // ── codecs and canonical form ──────────────────────────────────────────────────
    (
        "models/src/rust/canonical_path.rs",
        Disposition::Measured("the escape arm; see the prost/bincode differentials"),
    ),
    (
        "models/src/rust/rholang/protobuf_encoder.rs",
        Disposition::Measured("gate subject `encode`"),
    ),
    (
        "models/src/rust/rhoapi_ext.rs",
        Disposition::Measured(
            "`epathmap_epm1_snapshot::epm1_round_trip_is_stack_safe_at_depth_4096_on_a_256_kib_stack` \
             drives the semantic view, EPM1 encode, both decoders, clone-shared cache and iterative teardown",
        ),
    ),
    (
        "models/src/rust/epathmap_trie_codec.rs",
        Disposition::NotATermDepthCycle(
            "textual false positive: the scanner intentionally erases qualification and joins \
             `PendingEpmDecode::new`, `TrieCodecError::new`, `PathMap::new`, \
             `EPathMapMode::try_from`, and `usize::try_from` through the bounded byte helpers \
             `read_len`, `read_protobuf_varint`, and `take`. None calls itself or another \
             member of that alleged cycle; ACTree descent is the explicit `Vec<ActFrame>` PDA",
        ),
    ),
    (
        "models/src/rust/par_set.rs",
        Disposition::NotATermDepthCycle(
            "textual false positive: `ParSet::deserialize` calls the qualified \
             `Vec::<Par>::deserialize`; it never calls `ParSet::deserialize`. The scanner \
             intentionally erases the qualifier and therefore invents a self-loop. Recursive \
             serde dispatch over generated Par is tracked by DERIVE_DISPOSITIONS instead",
        ),
    ),
    (
        "models/src/rust/par_map.rs",
        Disposition::NotATermDepthCycle(
            "textual false positive: `ParMap::deserialize` calls the qualified \
             `Vec::<(Par, Par)>::deserialize`; it never calls `ParMap::deserialize`. The scanner \
             intentionally erases the qualifier and therefore invents a self-loop. Recursive \
             serde dispatch over generated Par is tracked by DERIVE_DISPOSITIONS instead",
        ),
    ),
    // ── evaluator ──────────────────────────────────────────────────────────────────
    (
        "rho-pure-eval/src/eval.rs",
        Disposition::Measured("gate subject `eval_with_nots`"),
    ),
    // ── metering ───────────────────────────────────────────────────────────────────
    (
        "rholang/src/rust/interpreter/accounting/mod.rs",
        Disposition::Measured("gate subject `subst_and_charge`"),
    ),
    (
        "rholang/src/rust/interpreter/accounting/delta_sigma.rs",
        Disposition::NotATermDepthCycle("cost arithmetic over a fixed structure"),
    ),
    // ── test and API surfaces ──────────────────────────────────────────────────────
    (
        "models/src/rust/test_utils/test_utils.rs",
        Disposition::NotATermDepthCycle(
            "generate_* builds fixtures to a bounded depth chosen by the caller",
        ),
    ),
    (
        "casper/src/rust/test_utils/helper/test_node.rs",
        Disposition::NotATermDepthCycle("test harness"),
    ),
    (
        "node/src/rust/api/rho_expr_pda.rs",
        Disposition::Measured(
            "the production conversion, Clone, Serialize, Debug, and Drop implementations are \
             explicit worklist machines; `rho_expr_pda_tests::deep_conversion_clone_json_debug_and_drop_fit_small_stack` \
             drives their complete lifecycle to depth 16,384 on 256 KiB. The reported \
             clone/serialize SCCs are conservative name-collision over-reports: scalar \
             `String::clone` and `RawValue::serialize` calls share method names with the local \
             trait entry points but do not re-enter them",
        ),
    ),
    (
        "node/src/rust/runtime/servers_instances.rs",
        Disposition::NotATermDepthCycle(
            "textual false positive: `Send` is `std::marker::Send` in async server bounds, not \
             rhoapi::Send, and the builder methods called by `ServersInstances::build` do not \
             recursively invoke that constructor",
        ),
    ),
    (
        "comm/src/rust/transport/grpc_transport.rs",
        Disposition::NotATermDepthCycle("transport, not a term traversal"),
    ),
    (
        "rholang/src/rust/interpreter/system_processes.rs",
        Disposition::NotATermDepthCycle(
            "conservative same-name over-report: the two unrelated constructors named `new` call \
             qualified constructors such as `Arc::new`, `RwLock::new`, and \
             `PrettyPrinter::new`; neither SystemProcesses constructor calls itself or the \
             other, and neither walks a recursive term",
        ),
    ),
    // ── ⚠ THE OVER-REPORT, MADE VISIBLE ────────────────────────────────────────────
    //
    // The five rows below were all surfaced by the census on its first run and all five are
    // in scope only because `Send`, `New` and `Match` are simultaneously `rhoapi` message
    // names and ordinary Rust vocabulary — `Send` above all, since `std::marker::Send`
    // appears in a great many generic bounds.
    //
    // ★ They are dispositioned rather than filtered out, and the collision is NOT repaired by
    // narrowing `TERM_FAMILY`. Dropping `Send` would make this census blind to every genuine
    // traversal of a `Send`, which is the one direction a census may not err in. **The cost
    // of the loose match is five rows; the cost of the tight one is a missed exposure.**
    (
        "casper/src/rust/engine/genesis_ceremony_master.rs",
        Disposition::NotATermDepthCycle(
            "`waiting_for_approved_block_loop` re-enters itself as a polling loop over the \
             ceremony's state machine; the `Send` match is `std::marker::Send` in a bound, not \
             a `rhoapi::Send`",
        ),
    ),
    (
        "casper/src/rust/test_utils/util/comm/transport_layer_test_impl.rs",
        Disposition::NotATermDepthCycle(
            "test transport double; `send` is the transport verb, not the term",
        ),
    ),
    (
        "casper/src/rust/util/comm/fair_round_robin_dispatcher.rs",
        Disposition::NotATermDepthCycle(
            "dispatcher over a peer queue, bounded by the queue rather than by any term",
        ),
    ),
    (
        "rspace++/src/rspace/history/radix_tree.rs",
        Disposition::NotATermDepthCycle(
            "★ EIGHT self-recursive functions (`go`, `update`, `delete`, `make_actions`, \
             `find_next_non_empty_item`, `print_tree`, `put_item_into_array`, `new`) — a real \
             recursive tree walk, and genuinely NOT term-depth: a radix tree over 32-byte \
             hashes is bounded by KEY LENGTH, a constant, not by the depth of any term a \
             deploy controls. ⚠ Worth re-reading if the key domain ever widens",
        ),
    ),
];

/// ⚠ The non-vacuity floor on the DERIVED file set. If the scan ever returned nothing — a
/// moved crate root, a regex that stopped matching, a walk that found no `.rs` — every
/// assertion below would iterate an empty set and PASS.
///
/// ★ Measured at the commit that introduced this file: **35** files carry a term-family cycle.
/// The floor is set below that so a legitimate conversion can retire files without editing it,
/// and it may only ever move DOWN with a commit that says which files left and why.
const MIN_FILES_WITH_TERM_RECURSION: usize = 25;

/// ⚠ The floor on MUTUAL components specifically, because they are the ones every prior census
/// was blind to. Measured: **22** term-family components are mutual.
const MIN_MUTUAL_COMPONENTS: usize = 15;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("`rholang` is a workspace member, so its manifest dir has a parent")
        .to_path_buf()
}

/// Every `.rs` file under the declared crate roots.
fn source_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            if p.is_dir() {
                if matches!(name.as_ref(), "target" | ".git" | "node_modules") {
                    continue;
                }
                walk(&p, out);
            } else if name.ends_with(".rs") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for r in CRATE_ROOTS {
        let d = root.join(r);
        if d.is_dir() {
            walk(&d, &mut out);
        }
    }
    out.sort();
    out
}

/// `(name, source offset, body)` for every `fn`, with the body delimited by brace count.
///
/// ⚠★ **A body whose braces never balance runs to END OF FILE rather than being skipped**, and
/// the direction of that choice is the whole point. An unbalanced count comes from a brace
/// inside a string literal or a macro the scanner cannot parse; SKIPPING such a function hides
/// every call it makes, which UNDER-reports, while running to EOF folds extra callees in,
/// which OVER-reports. **Only one of those can cause a traversal to escape the census.**
///
/// An earlier draft skipped, and found fewer files than the same algorithm that did not.
fn fn_bodies(src: &str) -> Vec<(String, usize, String)> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(at) = src[i..].find("fn ") {
        let start = i + at;
        // must be a word boundary before `fn`
        let ok_before = start == 0
            || !matches!(bytes[start - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_');
        i = start + 3;
        if !ok_before {
            continue;
        }
        let rest = &src[i..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        let Some(brace) = src[i..].find('{') else {
            continue;
        };
        // A required trait method has no body. Without this check its `fn ...;`
        // steals the next method's opening brace and invents a call-graph node
        // containing that neighbour's body.
        if let Some(semi) = src[i..].find(';') {
            if semi < brace {
                i += semi + 1;
                continue;
            }
        }
        let bstart = i + brace;
        let mut depth = 0i32;
        let mut closed = None;
        for (k, c) in src[bstart..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        closed = Some(bstart + k);
                        break;
                    }
                }
                _ => {}
            }
        }
        // ⚠ Unbalanced ⇒ run to EOF. See this function's doc: over-report, never under-report.
        let end = closed.unwrap_or(src.len());
        out.push((name, start, src[bstart..end].to_string()));
    }
    out
}

/// Identifiers used in call position inside `body`.
///
/// ⚠ **Whitespace between the name and its `(` is skipped**, so `foo ()` and a call split
/// across a line break are both seen. An earlier draft required the paren to be adjacent and
/// silently found **seven fewer files** than the same algorithm written with the tolerance —
/// an UNDER-report, which is the one direction this census may not err in. The floors below
/// exist because a gap like that is invisible in a green run.
fn callees(body: &str) -> BTreeSet<String> {
    let b = body.as_bytes();
    let mut out = BTreeSet::new();
    let mut start: Option<usize> = None;
    for (i, c) in body.char_indices() {
        let is_word = c.is_ascii_alphanumeric() || c == '_';
        if is_word {
            if start.is_none() {
                start = Some(i);
            }
            continue;
        }
        let Some(s) = start.take() else { continue };
        if !b[s].is_ascii_lowercase() {
            continue;
        }
        // Skip any run of whitespace between the identifier and a candidate `(`.
        let mut j = i;
        while j < b.len() && (b[j] as char).is_whitespace() {
            j += 1;
        }
        if j < b.len() && b[j] == b'(' {
            out.insert(body[s..i].to_string());
        }
    }
    out
}

type Node = (usize, String, usize); // (file index, fn name, source offset)

/// Tarjan's strongly connected components, iterative so the census cannot itself overflow.
///
/// ★ That is not a joke: a recursive SCC algorithm inside the gate that exists to find
/// unbounded recursion would be the campaign's own defect, in its own instrument.
fn strongly_connected(graph: &BTreeMap<Node, BTreeSet<Node>>) -> Vec<Vec<Node>> {
    let nodes: Vec<Node> = graph.keys().cloned().collect();
    let idx_of: BTreeMap<Node, usize> = nodes
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, n)| (n, i))
        .collect();
    let adj: Vec<Vec<usize>> = nodes
        .iter()
        .map(|n| {
            graph[n]
                .iter()
                .filter_map(|m| idx_of.get(m).copied())
                .collect()
        })
        .collect();

    let n = nodes.len();
    let (mut index, mut low) = (vec![usize::MAX; n], vec![0usize; n]);
    let mut on_stack = vec![false; n];
    let (mut stack, mut counter, mut out) = (Vec::new(), 0usize, Vec::new());

    for s in 0..n {
        if index[s] != usize::MAX {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(s, 0)];
        index[s] = counter;
        low[s] = counter;
        counter += 1;
        stack.push(s);
        on_stack[s] = true;

        while let Some(&mut (v, ref mut pi)) = work.last_mut() {
            if *pi < adj[v].len() {
                let w = adj[v][*pi];
                *pi += 1;
                if index[w] == usize::MAX {
                    index[w] = counter;
                    low[w] = counter;
                    counter += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                work.pop();
                if let Some(&(u, _)) = work.last() {
                    low[u] = low[u].min(low[v]);
                }
                if low[v] == index[v] {
                    let mut comp = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        comp.push(nodes[w].clone());
                        if w == v {
                            break;
                        }
                    }
                    out.push(comp);
                }
            }
        }
    }
    out
}

struct Census {
    /// Components that are a cycle: mutual (`len > 1`) or a self-loop.
    recursive: Vec<Vec<Node>>,
    /// Of those, the ones touching the term family.
    term_family: Vec<Vec<Node>>,
    files: Vec<PathBuf>,
    rel: Vec<String>,
}

fn run_census() -> Census {
    let root = workspace_root();
    let files = source_files(&root);
    assert!(
        !files.is_empty(),
        "the census found NO source files under {CRATE_ROOTS:?}. Either a crate root moved or \
         the walk is broken — and an empty scan makes every assertion below vacuous."
    );

    let rel: Vec<String> = files
        .iter()
        .map(|p| {
            p.strip_prefix(&root)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    // (file, name, source offset) -> callee names ; name -> definitions
    let mut bodies: BTreeMap<Node, BTreeSet<String>> = BTreeMap::new();
    let mut text: BTreeMap<Node, String> = BTreeMap::new();
    let mut defined_in: BTreeMap<String, BTreeSet<Node>> = BTreeMap::new();

    for (fi, path) in files.iter().enumerate() {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        for (name, offset, body) in fn_bodies(&src) {
            let key = (fi, name.clone(), offset);
            defined_in.entry(name).or_default().insert(key.clone());
            bodies.insert(key.clone(), callees(&body));
            text.insert(key, body);
        }
    }

    // Edges: conservatively include every same-file overload. Cross-file calls are admitted
    // only when all definitions of the callee name live in one file.
    let mut graph: BTreeMap<Node, BTreeSet<Node>> = BTreeMap::new();
    for (node, cs) in &bodies {
        let mut out = BTreeSet::new();
        for c in cs {
            let Some(definitions) = defined_in.get(c) else {
                continue;
            };
            let same_file: Vec<Node> = definitions
                .iter()
                .filter(|definition| definition.0 == node.0)
                .cloned()
                .collect();
            if !same_file.is_empty() {
                out.extend(same_file);
            } else {
                let files: BTreeSet<usize> =
                    definitions.iter().map(|definition| definition.0).collect();
                if files.len() == 1 {
                    out.extend(definitions.iter().cloned());
                }
            }
        }
        graph.insert(node.clone(), out);
    }

    let comps = strongly_connected(&graph);
    let recursive: Vec<Vec<Node>> = comps
        .into_iter()
        .filter(|c| c.len() > 1 || graph.get(&c[0]).is_some_and(|adj| adj.contains(&c[0])))
        .collect();

    let term_family: Vec<Vec<Node>> = recursive
        .iter()
        .filter(|c| {
            c.iter().any(|n| {
                text.get(n).is_some_and(|t| {
                    TERM_FAMILY.iter().any(|ty| {
                        t.match_indices(ty).any(|(i, _)| {
                            let before = t[..i].chars().next_back();
                            let after = t[i + ty.len()..].chars().next();
                            !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                        })
                    })
                })
            })
        })
        .cloned()
        .collect();

    Census {
        recursive,
        term_family,
        files,
        rel,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════════════
// The assertions
// ═══════════════════════════════════════════════════════════════════════════════════════

/// ★★ **Every file carrying a term-family cycle has a disposition, and a new one FAILS BY NAME.**
#[test]
fn every_handwritten_term_recursion_has_a_disposition() {
    let c = run_census();

    let mut with_recursion: BTreeMap<String, usize> = BTreeMap::new();
    for comp in &c.term_family {
        for (fi, _, _) in comp {
            let e = with_recursion.entry(c.rel[*fi].clone()).or_insert(0);
            *e = (*e).max(comp.len());
        }
    }

    // ── non-vacuity, before any comparison ──
    assert!(
        with_recursion.len() >= MIN_FILES_WITH_TERM_RECURSION,
        "CENSUS WENT VACUOUS: only {} file(s) carry a term-family cycle, below the floor of \
         {MIN_FILES_WITH_TERM_RECURSION}.\n\
         That is not good news. It means the scan stopped seeing things — a moved crate root, a \
         `fn` shape the body-delimiter cannot close, or a term type renamed out of TERM_FAMILY. \
         A census that finds nothing certifies nothing.\n\
         Scanned {} file(s) across {:?}.",
        with_recursion.len(),
        c.files.len(),
        CRATE_ROOTS
    );

    let mutual = c.term_family.iter().filter(|x| x.len() > 1).count();
    assert!(
        mutual >= MIN_MUTUAL_COMPONENTS,
        "MUTUAL-RECURSION DETECTION WENT VACUOUS: {mutual} mutual component(s), below the floor \
         of {MIN_MUTUAL_COMPONENTS}.\n\
         ⚠ Mutual recursion is the ENTIRE reason this census computes SCCs rather than looking \
         for self-calls. #121 was a 2-cycle. If this count collapses, the census has degraded \
         into the detector it was built to replace."
    );

    let declared: BTreeSet<&str> = RECURSION_DISPOSITIONS.iter().map(|(f, _)| *f).collect();
    let undispositioned: Vec<String> = with_recursion
        .iter()
        .filter(|(f, _)| !declared.contains(f.as_str()))
        .map(|(f, n)| {
            let mut components: Vec<String> = c
                .term_family
                .iter()
                .filter(|component| component.iter().any(|(fi, _, _)| c.rel[*fi] == *f))
                .map(|component| {
                    let mut names: Vec<&str> = component
                        .iter()
                        .filter(|(fi, _, _)| c.rel[*fi] == *f)
                        .map(|(_, name, _)| name.as_str())
                        .collect();
                    names.sort_unstable();
                    names.join(" ↔ ")
                })
                .collect();
            components.sort_unstable();
            format!(
                "{f}  (largest component: {n}; members: {})",
                components.join("; ")
            )
        })
        .collect();

    assert!(
        undispositioned.is_empty(),
        "UNDISPOSITIONED HAND-WRITTEN RECURSION over the term family, in {} file(s):\n  {}\n\n\
         Each of these contains a cycle in the call graph whose members mention the term family, \
         and nothing says what is known about it. That is the state `#121` was in: mutually \
         recursive, unbounded, in no audit and no tripwire, found only when an unrelated probe \
         overflowed.\n\n\
         Add a row to `RECURSION_DISPOSITIONS`: `Measured(subject)`, \
         `OracleTwin(why)`, or `NotATermDepthCycle(why)`. A live unmeasured term-depth \
         cycle is intentionally unrepresentable: convert it to a PDA/iterative traversal \
         before updating this exact current-state table.",
        undispositioned.len(),
        undispositioned.join("\n  ")
    );

    assert_eq!(
        declared.len(),
        RECURSION_DISPOSITIONS.len(),
        "DUPLICATE RECURSION DISPOSITION: the declared table has {} row(s) but only {} unique \
         file name(s). A duplicate makes one disposition shadow another instead of describing \
         one exact derived file set.",
        RECURSION_DISPOSITIONS.len(),
        declared.len(),
    );
    let active: BTreeSet<&str> = with_recursion.keys().map(String::as_str).collect();
    let stale: Vec<&str> = declared.difference(&active).copied().collect();
    assert!(
        stale.is_empty(),
        "STALE HAND-WRITTEN RECURSION DISPOSITIONS remain for {} file(s):\n  {}\n\n\
         The file set is derived in both directions. Remove rows whose term-family SCC was \
         converted or disappeared; historical conversion evidence belongs in the living \
         stack-safety report, not in a current-state disposition table.",
        stale.len(),
        stale.join("\n  "),
    );

    println!(
        "  hand-written recursion census: {} recursive component(s), {} over the term family \
         ({} mutual), across {} exactly dispositioned file(s)",
        c.recursive.len(),
        c.term_family.len(),
        mutual,
        with_recursion.len(),
    );
}

/// ⭑★★ **The conversion witness: the census must stop finding #121, while its
/// independent mutual-recursion calibration remains live below.**
///
/// `#121` is the reason this file exists. The former budgeted recursion was not
/// enough for final closure: `eval_stable_par` is now a fully iterative PDA with
/// a tail cursor and a heap worklist containing only pending siblings. This test
/// makes the conversion observable instead of silently losing the calibration.
#[test]
fn the_census_confirms_the_121_family_was_converted() {
    let c = run_census();
    const FILE: &str = "models/src/rust/pathmap_crate_type_mapper.rs";

    let found: Vec<Vec<String>> = c
        .term_family
        .iter()
        .filter(|comp| comp.iter().any(|(fi, _, _)| c.rel[*fi] == FILE))
        .map(|comp| {
            let mut names: Vec<String> = comp.iter().map(|(_, n, _)| n.clone()).collect();
            names.sort();
            names
        })
        .collect();

    assert!(
        found.is_empty(),
        "#121 REGRESSED: `{FILE}` again contains a term-family recursion component: {found:?}.\n\
         The ground-domain classifier is on every EPathMap key encode and must remain a fully \
         iterative PDA; bounded recursion and native-stack chunking are not accepted."
    );

    println!("  #121 conversion witness: no term-family recursion remains in {FILE}");
}

/// ★ The oracle twin is a superset of the 26 names `normalize_oracle_provenance.rs` extracts.
///
/// ⚠ A SUPERSET, not an equality: this census is loose in the safe direction and legitimately
/// also catches the helpers the extraction does not rename. Asserting equality would make a
/// correct over-report look like a failure.
#[test]
fn the_normalizer_oracle_twin_is_found_and_is_a_superset_of_the_scc_oracle() {
    let c = run_census();
    const FILE: &str = "rholang/src/rust/interpreter/compiler/normalize_recursive.rs";
    const ORACLE_MEMBERS: usize = 26;

    let biggest = c
        .term_family
        .iter()
        .filter(|comp| comp.iter().any(|(fi, _, _)| c.rel[*fi] == FILE))
        .map(|comp| comp.len())
        .max()
        .unwrap_or(0);

    assert!(
        biggest >= ORACLE_MEMBERS,
        "CALIBRATION LOST against the SCC oracle: the largest term-family component in `{FILE}` \
         has {biggest} member(s), fewer than the {ORACLE_MEMBERS} that \
         `rholang/tests/normalize_oracle_provenance.rs`'s `SCC_NAMES` enumerates.\n\n\
         That oracle is an independently maintained list of the SAME cycle. If this census sees \
         fewer, it is under-reporting — the one direction a census may not err in."
    );
    println!(
        "  SCC-oracle calibration: largest component in the twin = {biggest} (oracle lists {ORACLE_MEMBERS})"
    );
}

/// Calibration for the exact defect that hid the old `SpatialMatcher` impl family.
/// The second same-named impl must not overwrite the first recursive body.
#[test]
fn the_census_keeps_every_same_named_trait_impl_body() {
    let source = r#"
        trait Walk<T> { fn walk(&mut self, value: T); }
        struct C;
        impl Walk<u8> for C {
            fn walk(&mut self, value: u8) { if value > 0 { self.walk(value - 1); } }
        }
        impl Walk<u16> for C {
            fn walk(&mut self, _value: u16) { leaf(); }
        }
    "#;

    let bodies: Vec<_> = fn_bodies(source)
        .into_iter()
        .filter(|(name, _, _)| name == "walk")
        .collect();
    assert_eq!(bodies.len(), 2, "both trait impl bodies must remain nodes");
    assert!(callees(&bodies[0].2).contains("walk"));
    assert!(!callees(&bodies[1].2).contains("walk"));
}
