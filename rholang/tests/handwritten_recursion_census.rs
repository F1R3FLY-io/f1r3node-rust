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
//! The original census found both self-calls and dozens of mutual components. A self-call
//! detector necessarily misses every mutual-only component — including #121's.
//!
//! ⇒ This census computes **strongly connected components** of the call graph, so a cycle of
//! any length is one object. The #121 conversion witness and independent recursive normalizer
//! oracle pin both sides: the repaired production cycle stays absent while mutual detection stays
//! live.
//!
//! # Why the nodes are `(file, function, same-name definition ordinal)` and not `function`
//!
//! ⚠ A name-keyed graph merges every `new`, `default` and `clone` in the workspace into single
//! nodes. Measured: it reported one **612-member** "mutual recursion" that was entirely an
//! artefact of the merge — a number with no referent, which is the exact failure this campaign
//! keeps finding. Rust traits commonly define the same method name in several impl blocks, and
//! keying only by `(file, name)` silently retained just the last body. That is precisely how the
//! recursive `SpatialMatcher` impl family escaped the earlier census. A source-order ordinal
//! among definitions with the same `(file, name)` distinguishes overloads while remaining stable
//! across whitespace, comments, and unrelated-function edits. Calls conservatively reach every
//! same-file definition with that name; a
//! cross-module edge is admitted only when every definition is in **exactly one** file. That
//! keeps genuinely cross-file cycles visible — `normalize.rs` ⇄
//! `normalize_drive.rs` ⇄ `p_input_normalizer.rs` is one, and a per-file scan cannot see it.
//!
//! # Loose in the safe direction, deliberately
//!
//! The scanner is textual: it token-scans `fn` across all Rust whitespace, delimits bodies by
//! structural braces, and treats any lower-case `ident(` in executable source as a call. A tiny
//! lexer removes comments/literals and proves the extent of exact `#[cfg(test)]` items; ambiguous
//! cfg predicates remain included. Receiver and qualification erasure still deliberately
//! over-reports (`Arc::new` can reach local `new` definitions), and macro expansion remains a
//! separate generated-source obligation. **A census that under-reports is worse than no census,
//! because it certifies coverage it does not have.**
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
use std::fmt::Write as _;
use std::ops::Range;
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

/// Recursive reference implementations must be linkable to unit tests without
/// being stored in a production source root. The second path in each pair is
/// the former production-source location and must stay absent.
const TEST_SUPPORT_ORACLE_MOVES: &[(&str, &str)] = &[
    (
        "models/tests/support/sort_recursive.rs",
        "models/src/rust/rholang/sorter/sort_recursive.rs",
    ),
    (
        "rholang/tests/support/normalize_recursive.rs",
        "rholang/src/rust/interpreter/compiler/normalize_recursive.rs",
    ),
    (
        "rholang/tests/support/normalize_differential.rs",
        "rholang/src/rust/interpreter/compiler/normalize_differential.rs",
    ),
    (
        "rholang/tests/support/substitute_oracle.rs",
        "rholang/src/rust/interpreter/substitute_oracle.rs",
    ),
    (
        "rholang/tests/support/reduce_expression_oracle.rs",
        "rholang/src/rust/interpreter/reduce_expression_oracle.rs",
    ),
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

/// What is known about one exact SCC of hand-written recursion over the term family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// Measured by a `stack_depth_gate.rs` subject. The cycle exists and its cost is known.
    Measured(&'static str),
    /// The cycle is over a bounded structure rather than the term's own depth — a fixed-arity
    /// walk, a config tree, a transport handshake.
    NotATermDepthCycle(&'static str),
}

const QUALIFIED_DELEGATION: Disposition = Disposition::NotATermDepthCycle(
    "textual receiver/qualification erasure: this exact component delegates to a distinct \
     receiver, trait implementation, constructor, codec, or collection operation with the same \
     spelling; there is no runtime back-edge through the listed function definitions",
);

const SAME_NAME_OVERLOADS: Disposition = Disposition::NotATermDepthCycle(
    "the conservative graph connects every same-file definition with the called name; these \
     exact overloads/trait implementations are independent adapters and do not recursively \
     invoke one another",
);

macro_rules! members {
    ($(($file:literal, $function:literal, $definition_ordinal:literal)),+ $(,)?) => {
        &[
            $(MemberSpec {
                file: $file,
                function: $function,
                definition_ordinal: $definition_ordinal,
            }),+
        ]
    };
}

/// ★★ Exact current-state obligations, one row per derived term-family SCC.
///
/// The member set—not a file—is the classified object. Adding a second cycle to a file that
/// already has a measured traversal therefore creates an unmatched key and fails by member.
const RECURSION_DISPOSITIONS: &[ComponentDisposition] = &[
    ComponentDisposition {
        label: "casper.api.block_api.casper_deploy-deploy",
        members: members![
            ("casper/src/rust/api/block_api.rs", "casper_deploy", 0),
            ("casper/src/rust/api/block_api.rs", "deploy", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.api.block_api.casper_deploy_cosigned-deploy_cosigned",
        members: members![
            ("casper/src/rust/api/block_api.rs", "casper_deploy_cosigned", 0),
            ("casper/src/rust/api/block_api.rs", "deploy_cosigned", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.engine.engine.with_casper",
        members: members![
            ("casper/src/rust/engine/engine.rs", "with_casper", 0),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "casper.engine.genesis_ceremony_master.waiting_for_approved_block_loop",
        members: members![
            ("casper/src/rust/engine/genesis_ceremony_master.rs", "waiting_for_approved_block_loop", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("async genesis polling re-enters until external approval state changes; it does not descend through a Rholang term"),
    },
    ComponentDisposition {
        label: "casper.engine.genesis_validator.new",
        members: members![
            ("casper/src/rust/engine/genesis_validator.rs", "new", 0),
            ("casper/src/rust/engine/genesis_validator.rs", "new", 1),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "casper.engine.initializing.new",
        members: members![
            ("casper/src/rust/engine/initializing.rs", "new", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.engine.running.new",
        members: members![
            ("casper/src/rust/engine/running.rs", "new", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.genesis.genesis.bitmask_or_mergeable_tag_name",
        members: members![
            ("casper/src/rust/genesis/genesis.rs", "bitmask_or_mergeable_tag_name", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.genesis.genesis.default_mergeable_tags",
        members: members![
            ("casper/src/rust/genesis/genesis.rs", "default_mergeable_tags", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.genesis.genesis.non_negative_mergeable_tag_name",
        members: members![
            ("casper/src/rust/genesis/genesis.rs", "non_negative_mergeable_tag_name", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.helper.test_result_collector.unapply",
        members: members![
            ("casper/src/rust/helper/test_result_collector.rs", "unapply", 0),
            ("casper/src/rust/helper/test_result_collector.rs", "unapply", 1),
            ("casper/src/rust/helper/test_result_collector.rs", "unapply", 2),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "casper.merging.deploy_chain_index.new",
        members: members![
            ("casper/src/rust/merging/deploy_chain_index.rs", "new", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.reporting_casper.get_report",
        members: members![
            ("casper/src/rust/reporting_casper.rs", "get_report", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.rholang.runtime.consume_result",
        members: members![
            ("casper/src/rust/rholang/runtime.rs", "consume_result", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.test_utils.helper.test_node.handle_receive",
        members: members![
            ("casper/src/rust/test_utils/helper/test_node.rs", "handle_receive", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("test-node receive handling is a harness loop over network events, not recursive descent through the carried Par"),
    },
    ComponentDisposition {
        label: "casper.util.comm.fair_round_robin_dispatcher.new",
        members: members![
            ("casper/src/rust/util/comm/fair_round_robin_dispatcher.rs", "new", 0),
            ("casper/src/rust/util/comm/fair_round_robin_dispatcher.rs", "new", 2),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "casper.util.rholang.runtime_manager.capture_results",
        members: members![
            ("casper/src/rust/util/rholang/runtime_manager.rs", "capture_results", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "casper.util.rholang.runtime_manager.play_exploratory_deploy",
        members: members![
            ("casper/src/rust/util/rholang/runtime_manager.rs", "play_exploratory_deploy", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "comm.rp.handle_messages.handle_packet",
        members: members![
            ("comm/src/rust/rp/handle_messages.rs", "handle_packet", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "comm.transport.grpc_transport.stream",
        members: members![
            ("comm/src/rust/transport/grpc_transport.rs", "stream", 0),
            ("comm/src/rust/transport/grpc_transport.rs", "stream", 1),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "models.canonical_path+models.rhoapi_ext.validation-and-representation-adoption",
        members: members![
            ("models/src/rust/canonical_path.rs", "close_path", 0),
            ("models/src/rust/canonical_path.rs", "drive", 0),
            ("models/src/rust/canonical_path.rs", "epathmap_carrier", 0),
            ("models/src/rust/canonical_path.rs", "parse_segment_start", 0),
            ("models/src/rust/canonical_path.rs", "run_validation", 0),
            ("models/src/rust/canonical_path.rs", "validate_canonical_path", 0),
            ("models/src/rust/canonical_path.rs", "validate_path_with_root", 0),
            ("models/src/rust/rhoapi_ext.rs", "canonical_key_metadata", 0),
            ("models/src/rust/rhoapi_ext.rs", "from_owned_repr_with_key_validation", 0),
            ("models/src/rust/rhoapi_ext.rs", "replace_decoded_representation_deferred_key_validation", 0),
            ("models/src/rust/rhoapi_ext.rs", "replace_decoded_representation_with", 0),
            ("models/src/rust/rhoapi_ext.rs", "replace_owned_decoded_representation_with", 0),
            ("models/src/rust/rhoapi_ext.rs", "replace_trie_snapshot_deferred_key_validation", 0),
        ],
        disposition: Disposition::Measured("canonical-path validation is an explicit frame machine and EPathMap adoption is zipper/iterator based; canonical-path and EPM1 depth gates drive this exact cross-file component"),
    },
    // Retired 2026-10-17: `PendingEpmDecode::<'static, Bytes>::new` is now `#[cfg(test)]`
    // (clippy reported it never used outside unit tests), and the census excludes `cfg(test)`
    // items, so the receiver-erased `new` edges that joined this component no longer exist.
    // The component dissolved; its two remaining same-spelling self-edges are declared as
    // `models.rhoapi_ext.get_map_value` and `rholang.interpreter.reduce.get_or_else`.
    // ComponentDisposition {
    //     label: "models.canonical_path+models.epathmap_trie_codec+models.rhoapi_ext+rholang.reduce+rspace++.pathmap_codec-and-cursor-adapters",
    //     members: members![
    //         ("models/src/rust/canonical_path.rs", "encode_segment", 0),
    //         ("models/src/rust/canonical_path.rs", "encode_trie_path", 0),
    //         ("models/src/rust/canonical_path.rs", "encode_trie_path_with_stability", 0),
    //         ("models/src/rust/canonical_path.rs", "run", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "encode_with_layout", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "from", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "from", 1),
    //         ("models/src/rust/epathmap_trie_codec.rs", "from_parts", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "layout_with_certified_value_count", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "new", 1),
    //         ("models/src/rust/epathmap_trie_codec.rs", "new", 4),
    //         ("models/src/rust/epathmap_trie_codec.rs", "new", 5),
    //         ("models/src/rust/epathmap_trie_codec.rs", "new_with_provider", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "parse_indexed_canonical_snapshot_prefix", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "parse_pending_framing", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "read_indexed_len", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "read_len", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "read_protobuf_varint", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "take", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "try_from", 0),
    //         ("models/src/rust/epathmap_trie_codec.rs", "validate_canonical_act_summary", 0),
    //         ("models/src/rust/rhoapi_ext.rs", "epm_layout", 0),
    //         ("models/src/rust/rhoapi_ext.rs", "get_map_value", 0),
    //         ("models/src/rust/rhoapi_ext.rs", "get_map_value", 1),
    //         ("models/src/rust/rhoapi_ext.rs", "trie_snapshot", 0),
    //         ("models/src/rust/rhoapi_ext.rs", "trie_snapshot", 1),
    //         ("rholang/src/rust/interpreter/reduce.rs", "ascend", 0),
    //         ("rholang/src/rust/interpreter/reduce.rs", "get", 0),
    //         ("rholang/src/rust/interpreter/reduce.rs", "get_or_else", 0),
    //         ("rspace++/src/rspace/bag.rs", "get_or_insert", 0),
    //         ("rspace++/src/rspace/bag.rs", "get_or_insert_from_cursor", 0),
    //         ("rspace++/src/rspace/guarded_pathmap.rs", "with_write_cursor", 0),
    //         ("rspace++/src/rspace/pathmap_mutation.rs", "move_to_path", 0),
    //         ("rspace++/src/rspace/pathmap_mutation.rs", "new", 0),
    //         ("rspace++/src/rspace/pathmap_mutation.rs", "new_counted", 0),
    //         ("rspace++/src/rspace/pathmap_mutation.rs", "with_authority", 0),
    //     ],
    //     disposition: Disposition::NotATermDepthCycle("receiver erasure joins codec constructors and parser helpers named new/from/take with Bag and PathMap cursor operations and reducer get/get_or_else; the crate dependency direction rholang -> models -> rspace++ rules out the reported cross-crate back-edge, while canonical-path encoding uses EncMachine ops and EPM1 map-value encoding uses the iterative protobuf writer, both covered by the trie escape-arm and EPM1 depth-4096 small-stack gates"),
    // },
    ComponentDisposition {
        label: "models.channel_image.channel_identity_from_par",
        members: members![
            ("models/src/rust/channel_image.rs", "from_par", 1),
        ],
        disposition: Disposition::NotATermDepthCycle("ChannelIdentityV2::from_par delegates to ChannelImageV2::from_par; the receiver-erased same-name call has no back-edge"),
    },
    ComponentDisposition {
        label: "models.par.merge_field",
        members: members![
            ("models/src/rust/par.rs", "merge_field", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("the sole same-name call is the cfg(test) protobuf_field_oracle::merge_field, not a recursive production call to the Par Message implementation"),
    },
    ComponentDisposition {
        label: "models.par.canonical_unary_bundle_terminal",
        members: members![
            ("models/src/rust/par.rs", "canonical_unary_bundle_terminal", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.par.canonical_unary_connective_spine",
        members: members![
            ("models/src/rust/par.rs", "canonical_unary_connective_spine", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.par.canonical_unary_connective_terminal",
        members: members![
            ("models/src/rust/par.rs", "canonical_unary_connective_terminal", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.par_bag.append_bytes_score_atom-append_key-append_key_suffix-append_leaf_expr_score-append_pathmap_expr_score-append_string_score_atom-append_terminated_score_bytes-contains-with_component_key",
        members: members![
            ("models/src/rust/par_bag.rs", "append_bytes_score_atom", 0),
            ("models/src/rust/par_bag.rs", "append_key", 0),
            ("models/src/rust/par_bag.rs", "append_key_suffix", 1),
            ("models/src/rust/par_bag.rs", "append_key_suffix", 2),
            ("models/src/rust/par_bag.rs", "append_leaf_expr_score", 0),
            ("models/src/rust/par_bag.rs", "append_pathmap_expr_score", 0),
            ("models/src/rust/par_bag.rs", "append_string_score_atom", 0),
            ("models/src/rust/par_bag.rs", "append_terminated_score_bytes", 0),
            ("models/src/rust/par_bag.rs", "contains", 0),
            ("models/src/rust/par_bag.rs", "with_component_key", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("the scanner joins finite score-key append helpers and same-named PathMap operations; component-key traversal is iterative and no listed helper calls upward"),
    },
    ComponentDisposition {
        label: "models.par_construction.finish",
        members: members![
            ("models/src/rust/par_construction.rs", "finish", 0),
            ("models/src/rust/par_construction.rs", "finish", 1),
        ],
        disposition: Disposition::NotATermDepthCycle("the scanner joins ProtobufImportBuilder::finish and ParConstruction::finish by name; finish_epm_repr calls the latter on planned values, and neither finish method calls the other"),
    },
    ComponentDisposition {
        label: "models.par_map.connective_used",
        members: members![
            ("models/src/rust/par_map.rs", "connective_used", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.par_map.create_from_vec-new-update_locally_free",
        members: members![
            ("models/src/rust/par_map.rs", "create_from_vec", 0),
            ("models/src/rust/par_map.rs", "new", 0),
            ("models/src/rust/par_map.rs", "update_locally_free", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("legacy ParMap construction performs bounded adapter calls plus flat collection folds; it does not recursively descend term depth"),
    },
    ComponentDisposition {
        label: "models.par_map.deserialize",
        members: members![
            ("models/src/rust/par_map.rs", "deserialize", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.par_set.create_from_vec-create_from_vec_and_remainder-new-update_locally_free",
        members: members![
            ("models/src/rust/par_set.rs", "create_from_vec", 0),
            ("models/src/rust/par_set.rs", "create_from_vec_and_remainder", 0),
            ("models/src/rust/par_set.rs", "new", 0),
            ("models/src/rust/par_set.rs", "update_locally_free", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("legacy ParSet construction performs bounded adapter calls plus flat collection folds; it does not recursively descend term depth"),
    },
    ComponentDisposition {
        label: "models.par_set.deserialize",
        members: members![
            ("models/src/rust/par_set.rs", "deserialize", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.clone",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "clone", 0),
            ("models/src/rust/rhoapi_ext.rs", "clone", 1),
            ("models/src/rust/rhoapi_ext.rs", "clone", 2),
        ],
        disposition: Disposition::Measured("the generated stack-safe EPathMap/EntryTrie clone lifecycle is driven by the EPM1 depth-4096 256-KiB gate; the apparent clone cycle is same-name dispatch"),
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.contains_entry",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "contains_entry", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.create_path",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "create_path", 0),
            ("models/src/rust/rhoapi_ext.rs", "create_path", 1),
            ("models/src/rust/rhoapi_ext.rs", "native_create_path", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("EPathMap::create_path forwards to EntryTrie::create_path, which calls native_create_path and then NativePathCursor::create_path; receiver erasure invents the back-edge"),
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.deserialize",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "deserialize", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.drain_owned_pars",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "drain_owned_pars", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.drop_head",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "drop_head", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.entry_key_at",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "entry_key_at", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.extend_entries",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "extend_entries", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.for_each_map_entry",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "for_each_map_entry", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.from",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "from", 2),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.get_map_value",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "get_map_value", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.insert_entry",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "insert_entry", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.insert_map_entry",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "insert_map_entry", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.insert_map_value_by_encoded_key",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "insert_map_value_by_encoded_key", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.into_raw_map_entries",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "into_raw_map_entries", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.into_raw_set_entries",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "into_raw_set_entries", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext+rholang.interpreter.reduce.join_exact_with_grafts-try_join-try_join_exact_zipper-graft-union",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "join_exact_with_grafts", 0),
            ("models/src/rust/rhoapi_ext.rs", "try_join", 0),
            ("models/src/rust/rhoapi_ext.rs", "try_join_exact_zipper", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "graft", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "union", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("the scanner merges reducer union/graft names with exact PathMap join helpers; the implementations use native algebraic operations and explicit zipper walks without a runtime back-edge"),
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.leaf_at_encoded_key",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "leaf_at_encoded_key", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.remove_branches_at",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "native_remove_branches", 0),
            ("models/src/rust/rhoapi_ext.rs", "remove_branches_at", 0),
            ("models/src/rust/rhoapi_ext.rs", "remove_branches_at", 1),
        ],
        disposition: Disposition::NotATermDepthCycle("EPathMap delegates to EntryTrie and then native_remove_branches, whose remove_branches_at receiver is the external PathMap; there is no EPathMap back-edge"),
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.remove_greatest_entry",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "remove_greatest_entry", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.remove_greatest_map_entry",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "remove_greatest_map_entry", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.replace_subtrie",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "replace_subtrie", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.representation",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "representation", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.subtrie",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "subtrie", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rhoapi_ext.try_insert_entry",
        members: members![
            ("models/src/rust/rhoapi_ext.rs", "try_insert_entry", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    // Retired 2026-10-17: both `try_insert_entry_replacing` definitions are now `#[cfg(test)]`
    // (clippy reported them never used outside unit tests), and the census excludes
    // `cfg(test)` items, so this delegation self-edge is no longer derived.
    // ComponentDisposition {
    //     label: "models.rhoapi_ext.try_insert_entry_replacing",
    //     members: members![
    //         ("models/src/rust/rhoapi_ext.rs", "try_insert_entry_replacing", 1),
    //     ],
    //     disposition: QUALIFIED_DELEGATION,
    // },
    ComponentDisposition {
        label: "models.rholang.implicits.single_bundle",
        members: members![
            ("models/src/rust/rholang/implicits.rs", "single_bundle", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.rholang.match_schema.next",
        members: members![
            ("models/src/rust/rholang/match_schema.rs", "next", 0),
        ],
        disposition: Disposition::Measured("RhoPathCursor::next is an explicit Vec<RhoPathStep> machine; the textual self-loop is BTreeMap::Iter::next, and the cursor gate drives depth and width 20,000"),
    },
    ComponentDisposition {
        label: "models.rholang.protobuf_admission.frame_accept",
        members: members![
            ("models/src/rust/rholang/protobuf_admission.rs", "accept", 4),
        ],
        disposition: Disposition::NotATermDepthCycle("Frame::accept dispatches to distinct MessageFrame, MapFrame, EPathMapFrame, and CanonicalPathFrame accept methods; the receiver-erased self-edge does not occur at runtime"),
    },
    ComponentDisposition {
        label: "models.rholang.protobuf_admission.frame_step",
        members: members![
            ("models/src/rust/rholang/protobuf_admission.rs", "step", 1),
        ],
        disposition: Disposition::NotATermDepthCycle("Frame::step dispatches to distinct per-variant step functions and CanonicalPathFrame::step; the receiver-erased self-edge does not occur at runtime"),
    },
    ComponentDisposition {
        label: "models.rholang.protobuf_schema.epathmap_try_encode_certified_egress",
        members: members![
            ("models/src/rust/rholang/protobuf_schema.rs", "try_encode_certified_egress", 1),
        ],
        disposition: Disposition::NotATermDepthCycle("the receiver-erased call is statically EPathMap -> Var and follows the generated strictly decreasing descriptor-depth registry; PathMap<Par> values use the opaque emit_par callback rather than this trait method, so nested term depth cannot close the apparent self-edge"),
    },
    ComponentDisposition {
        label: "models.sorted_par_hash_set.contains",
        members: members![
            ("models/src/rust/sorted_par_hash_set.rs", "contains", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.sorted_par_hash_set.insert",
        members: members![
            ("models/src/rust/sorted_par_hash_set.rs", "insert", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.sorted_par_hash_set.remove",
        members: members![
            ("models/src/rust/sorted_par_hash_set.rs", "remove", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.sorted_par_hash_set.union",
        members: members![
            ("models/src/rust/sorted_par_hash_set.rs", "union", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.sorted_par_map.get",
        members: members![
            ("models/src/rust/sorted_par_map.rs", "get", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.sorted_par_map.insert",
        members: members![
            ("models/src/rust/sorted_par_map.rs", "insert", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.sorted_par_map.remove",
        members: members![
            ("models/src/rust/sorted_par_map.rs", "remove", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "models.test_utils.test_utils.generate_bundle-generate_connective-generate_expr-generate_match-generate_new-generate_par-generate_receive-generate_send",
        members: members![
            ("models/src/rust/test_utils/test_utils.rs", "generate_bundle", 0),
            ("models/src/rust/test_utils/test_utils.rs", "generate_connective", 0),
            ("models/src/rust/test_utils/test_utils.rs", "generate_expr", 0),
            ("models/src/rust/test_utils/test_utils.rs", "generate_match", 0),
            ("models/src/rust/test_utils/test_utils.rs", "generate_new", 0),
            ("models/src/rust/test_utils/test_utils.rs", "generate_par", 0),
            ("models/src/rust/test_utils/test_utils.rs", "generate_receive", 0),
            ("models/src/rust/test_utils/test_utils.rs", "generate_send", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("fixture generators recurse only to the explicit depth selected by the test caller; this is test-data construction, not production term evaluation"),
    },
    ComponentDisposition {
        label: "node.api.rho_expr_pda.clone-layout-clone_rho_expr",
        members: members![
            ("node/src/rust/api/rho_expr_pda.rs", "clone", 0),
            ("node/src/rust/api/rho_expr_pda.rs", "clone_legacy_map_layout", 0),
            ("node/src/rust/api/rho_expr_pda.rs", "clone_rho_expr", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("clone_rho_expr enters clone_legacy_map_layout only to copy HashMap bucket layout; during that scoped call HashMap::clone invokes RhoExpr::clone only for inert placeholders, so the guarded branch cannot reenter clone_rho_expr. The PDA later clones and installs the real children; the 16,384-level 256-KiB node depth gate exercises this path"),
    },
    ComponentDisposition {
        label: "node.effects.repl_client.new",
        members: members![
            ("node/src/rust/effects/repl_client.rs", "new", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "node.runtime.servers_instances.build",
        members: members![
            ("node/src/rust/runtime/servers_instances.rs", "build", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.build.compile_rholang_source.new",
        members: members![
            ("rholang/src/rust/build/compile_rholang_source.rs", "new", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.accounting.mod.clone",
        members: members![
            ("rholang/src/rust/interpreter/accounting/mod.rs", "clone", 0),
            ("rholang/src/rust/interpreter/accounting/mod.rs", "clone", 1),
            ("rholang/src/rust/interpreter/accounting/mod.rs", "clone", 2),
        ],
        disposition: Disposition::Measured("signature/token/signed-process Clone implementations are explicit worklist machines driven to depth 20,000 on 128 KiB"),
    },
    ComponentDisposition {
        label: "rholang.interpreter.accounting.mod.fmt-fmt_sig_pretty-fmt_signed_process_pretty-fmt_token_pretty",
        members: members![
            ("rholang/src/rust/interpreter/accounting/mod.rs", "fmt", 1),
            ("rholang/src/rust/interpreter/accounting/mod.rs", "fmt", 2),
            ("rholang/src/rust/interpreter/accounting/mod.rs", "fmt", 3),
            ("rholang/src/rust/interpreter/accounting/mod.rs", "fmt_sig_pretty", 0),
            ("rholang/src/rust/interpreter/accounting/mod.rs", "fmt_signed_process_pretty", 0),
            ("rholang/src/rust/interpreter/accounting/mod.rs", "fmt_token_pretty", 0),
        ],
        disposition: Disposition::Measured("signature/token/signed-process Debug implementations are explicit worklist machines driven to depth 20,000 on 128 KiB"),
    },
    ComponentDisposition {
        label: "rholang.interpreter.accounting.resource_logic.demand",
        members: members![
            ("rholang/src/rust/interpreter/accounting/resource_logic.rs", "demand", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.chromadb_service.into",
        members: members![
            ("rholang/src/rust/interpreter/chromadb_service.rs", "into", 1),
            ("rholang/src/rust/interpreter/chromadb_service.rs", "into", 3),
            ("rholang/src/rust/interpreter/chromadb_service.rs", "into", 5),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "rholang.interpreter.chromadb_service.unapply",
        members: members![
            ("rholang/src/rust/interpreter/chromadb_service.rs", "unapply", 0),
            ("rholang/src/rust/interpreter/chromadb_service.rs", "unapply", 1),
            ("rholang/src/rust/interpreter/chromadb_service.rs", "unapply", 2),
            ("rholang/src/rust/interpreter/chromadb_service.rs", "unapply", 3),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "rholang.interpreter.compiler.normalize.default-new",
        members: members![
            ("rholang/src/rust/interpreter/compiler/normalize.rs", "default", 0),
            ("rholang/src/rust/interpreter/compiler/normalize.rs", "new", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.matcher.fold_match+rholang.interpreter.matcher.match+rholang.interpreter.matcher.spatial_matcher+rholang.interpreter.matcher.spatial_matcher_pda.fold_match-admitted_expr_is_dynamic-compile_ordered_nested_query-compile_ordered_region_query-get-record_linear_capture-rho_path_query_atom-rho_path_query_cursor_atom-rho_path_query_region_atom-spatial_match-spatial_match_par_ref-compile_candidate_plan-compile_par_candidate_plan-drive-drive_job-init_list-match_connective-match_par-match_par_pair-rho_path_candidate_plan-rho_path_query",
        members: members![
            ("rholang/src/rust/interpreter/matcher/fold_match.rs", "fold_match", 0),
            ("rholang/src/rust/interpreter/matcher/fold_match.rs", "fold_match", 1),
            ("rholang/src/rust/interpreter/matcher/match.rs", "admitted_expr_is_dynamic", 0),
            ("rholang/src/rust/interpreter/matcher/match.rs", "compile_ordered_nested_query", 0),
            ("rholang/src/rust/interpreter/matcher/match.rs", "compile_ordered_region_query", 0),
            ("rholang/src/rust/interpreter/matcher/match.rs", "get", 0),
            ("rholang/src/rust/interpreter/matcher/match.rs", "record_linear_capture", 0),
            ("rholang/src/rust/interpreter/matcher/match.rs", "rho_path_query_atom", 0),
            ("rholang/src/rust/interpreter/matcher/match.rs", "rho_path_query_cursor_atom", 0),
            ("rholang/src/rust/interpreter/matcher/match.rs", "rho_path_query_region_atom", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher.rs", "spatial_match", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher.rs", "spatial_match", 1),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher.rs", "spatial_match", 2),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher.rs", "spatial_match_par_ref", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "compile_candidate_plan", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "compile_par_candidate_plan", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "drive", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "drive_job", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "init_list", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "match_connective", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "match_par", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "match_par_pair", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "rho_path_candidate_plan", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "rho_path_query", 0),
        ],
        disposition: Disposition::Measured("all heterogeneous spatial descent is the explicit Job/FrameStack PDA; the protobuf-region query route is a generated CanonicalRhoPathRegionCursor plus an explicit Scan worklist and is driven to depth 4,096 on 512 KiB; depth, width, connective, set, and map gates cover the remaining cross-file component. The apparent return edge is conservative receiver erasure joining RhoPathCaptureId::get to Matcher::get, not runtime recursion"),
    },
    ComponentDisposition {
        label: "rholang.interpreter.matcher.has_locally_free.connective_used",
        members: members![
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 0),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 1),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 6),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 7),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 9),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 11),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 13),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 16),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 17),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 19),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "connective_used", 20),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "rholang.interpreter.matcher.has_locally_free.locally_free",
        members: members![
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "locally_free", 0),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "locally_free", 6),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "locally_free", 9),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "locally_free", 11),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "locally_free", 13),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "locally_free", 16),
            ("rholang/src/rust/interpreter/matcher/has_locally_free.rs", "locally_free", 17),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "rholang.interpreter.matcher.spatial_matcher_pda+rspace++.rspace.rho_path.all-push-singleton-is_exact-retained_prefix",
        members: members![
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "all", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "all", 1),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "push", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "push", 1),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "singleton", 0),
            ("rspace++/src/rspace/rho_path.rs", "is_exact", 0),
            ("rspace++/src/rspace/rho_path.rs", "retained_prefix", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("same-name all/push/singleton query helpers are flat iterator/worklist operations; RhoPath retained-prefix walks contiguous encoded vectors and has no runtime back-edge to the matcher"),
    },
    ComponentDisposition {
        label: "rholang.interpreter.matcher.spatial_matcher_pda.connective_used-expr_connective_used",
        members: members![
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "connective_used", 0),
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "expr_connective_used", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.matcher.par_count.next",
        members: members![
            ("rholang/src/rust/interpreter/matcher/par_count.rs", "next", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("CountChildren::next advances stored slice, vector, or encoded child iterators; each receiver-erased next call targets an iterator rather than CountChildren"),
    },
    ComponentDisposition {
        label: "rholang.interpreter.matcher.spatial_matcher_pda.shares_cow_bincode_identity",
        members: members![
            ("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs", "shares_cow_bincode_identity", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.consume-consume_inner-continue_consume_process",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "consume", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "consume_inner", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "continue_consume_process", 0),
        ],
        disposition: Disposition::Measured("the consume/consume_inner/continuation component runs through the detached counted-task reducer driver; async_reducer_scc_depth_4096_uses_a_fixed_small_native_stack drives this exact SCC"),
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.contains",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "contains", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.continue_produce_process-produce-produce_inner-produce_peeks",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "continue_produce_process", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "produce", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "produce_inner", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "produce_peeks", 0),
        ],
        disposition: Disposition::Measured("the produce/produce_inner/peek-continuation component runs through the detached counted-task reducer driver; async_reducer_scc_depth_4096_uses_a_fixed_small_native_stack drives this exact SCC"),
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.create_path",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "create_path", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.dispatch-dispatch_inner",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "dispatch", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "dispatch_inner", 0),
        ],
        disposition: Disposition::Measured("dispatch and dispatch_inner are the bounded entry/inner pair in the counted-task reducer driver; the async reducer SCC depth gate exercises the pair without native-stack growth"),
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.drop_head",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "drop_head", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.eval_bundle-eval_if-eval_inner-eval_match-eval_new-eval_with_path-generated_message_eval",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "eval_bundle", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "eval_if", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "eval_inner", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "eval_match", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "eval_new", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "eval_with_path", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "generated_message_eval", 0),
        ],
        disposition: Disposition::Measured("eval_inner and its process adapters use the counted-task/explicit eval drivers; async reducer and method-reentry depth-4096 gates cover this exact process-evaluation SCC"),
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.expr_locally_free_ref",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "expr_locally_free_ref", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.get_or_else",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "get_or_else", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.idle-idle_drive_cell-new",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "idle", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "idle_drive_cell", 0),
            ("rholang/src/rust/interpreter/reduce.rs", "new", 1),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.keys",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "keys", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.local_nth",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "local_nth", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.reduce.take",
        members: members![
            ("rholang/src/rust/interpreter/reduce.rs", "take", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.rho_runtime.consume_result",
        members: members![
            ("rholang/src/rust/interpreter/rho_runtime.rs", "consume_result", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.rho_runtime.create_soft_checkpoint",
        members: members![
            ("rholang/src/rust/interpreter/rho_runtime.rs", "create_soft_checkpoint", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.rho_runtime.get_data",
        members: members![
            ("rholang/src/rust/interpreter/rho_runtime.rs", "get_data", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.rho_runtime.get_joins",
        members: members![
            ("rholang/src/rust/interpreter/rho_runtime.rs", "get_joins", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.rho_runtime.inj",
        members: members![
            ("rholang/src/rust/interpreter/rho_runtime.rs", "inj", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.rho_runtime.revert_to_soft_checkpoint",
        members: members![
            ("rholang/src/rust/interpreter/rho_runtime.rs", "revert_to_soft_checkpoint", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.rho_type.unapply",
        members: members![
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 15),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 16),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 17),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 18),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 19),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 20),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 21),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 22),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 23),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 24),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 25),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 26),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 27),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 28),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 29),
            ("rholang/src/rust/interpreter/rho_type.rs", "unapply", 30),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "rholang.interpreter.substitute.substitute",
        members: members![
            ("rholang/src/rust/interpreter/substitute.rs", "substitute", 2),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.substitute_drive.get",
        members: members![
            ("rholang/src/rust/interpreter/substitute_drive.rs", "get", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.system_processes.new",
        members: members![
            ("rholang/src/rust/interpreter/system_processes.rs", "new", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rholang.interpreter.system_processes.unapply",
        members: members![
            ("rholang/src/rust/interpreter/system_processes.rs", "unapply", 0),
            ("rholang/src/rust/interpreter/system_processes.rs", "unapply", 1),
            ("rholang/src/rust/interpreter/system_processes.rs", "unapply", 2),
        ],
        disposition: SAME_NAME_OVERLOADS,
    },
    ComponentDisposition {
        label: "rspace++.rspace.pathmap_mutation.remove_value_preserving_path-remove_val",
        members: members![
            ("rspace++/src/rspace/guarded_pathmap.rs", "remove_value_preserving_path", 0),
            ("rspace++/src/rspace/pathmap_mutation.rs", "remove_val", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("PathMapMutation::remove_val calls remove_value_preserving_path on its inner zipper; the latter calls ZipperWriting::remove_val, not PathMapMutation::remove_val"),
    },
    ComponentDisposition {
        label: "rspace++.rspace.history.history_repository_impl.reset",
        members: members![
            ("rspace++/src/rspace/history/history_repository_impl.rs", "reset", 0),
        ],
        disposition: QUALIFIED_DELEGATION,
    },
    ComponentDisposition {
        label: "rspace++.rspace.history.radix_tree.make_actions",
        members: members![
            ("rspace++/src/rspace/history/radix_tree.rs", "make_actions", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("make_actions recursively walks a radix tree over fixed 32-byte hash keys, so its native depth is bounded by the constant key domain rather than term depth"),
    },
    ComponentDisposition {
        label: "rspace++.rspace.merger.state_change.compute_value_change-hash-multiset_diff-new",
        members: members![
            ("rspace++/src/rspace/merger/state_change.rs", "compute_value_change", 0),
            ("rspace++/src/rspace/merger/state_change.rs", "hash", 0),
            ("rspace++/src/rspace/merger/state_change.rs", "multiset_diff", 0),
            ("rspace++/src/rspace/merger/state_change.rs", "new", 0),
        ],
        disposition: Disposition::NotATermDepthCycle("state-change construction and multiset difference are flat HashMap/vector passes; Hash::hash and constructor name erasure creates the apparent component"),
    },
];

/// Anti-vacuity is structural rather than a pinned observation count:
///
/// * every declared crate root must still exist;
/// * the independently maintained recursive normalizer oracle calibrates mutual-SCC detection;
/// * the derived canonical SCC-member set must equal `RECURSION_DISPOSITIONS` in both directions.
///
/// A numerical floor becomes stale precisely when this campaign succeeds at retiring a cycle.
/// Exact set equality catches missing roots, scanner regressions, new cycles, and retired cycles
/// without requiring a human to bless a progressively smaller magic number.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("`rholang` is a workspace member, so its manifest dir has a parent")
        .to_path_buf()
}

/// Every `.rs` file under the declared crate roots.
fn source_files(root: &Path, roots: &[&str]) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|error| {
            panic!(
                "recursion census cannot enumerate source directory {}: {error}",
                dir.display()
            )
        });
        for entry in entries {
            let e = entry.unwrap_or_else(|error| {
                panic!(
                    "recursion census cannot enumerate an entry under {}: {error}",
                    dir.display()
                )
            });
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
    for r in roots {
        let d = root.join(r);
        if d.is_dir() {
            walk(&d, &mut out);
        } else if d.is_file() && d.extension().is_some_and(|ext| ext == "rs") {
            out.push(d);
        }
    }
    out.sort();
    out
}

#[derive(Debug)]
struct FunctionBody {
    name: String,
    source_offset: usize,
    /// Only structural code in the body is searched for calls. Comments and literals cannot
    /// call a function; excluding them removes false edges without excluding executable code.
    body: String,
    /// The signature and body are both searched for term-family names. A traversal whose
    /// parameter is `Par` must not disappear merely because its body calls it `value`.
    term_text: String,
}

/// Replace comments and string/character literal contents with spaces while preserving byte
/// offsets, newlines, and structural punctuation in real Rust code.
///
/// This is intentionally a tiny lexer, not a parser. Its only security property is that braces
/// in prose cannot make a proven `#[cfg(test)]` item appear to extend over production code. An
/// unterminated literal/comment masks to EOF; that makes the cfg-only exclusion decline to find
/// a balanced item and therefore over-report rather than hide production.
fn structural_rust_bytes(src: &str) -> Vec<u8> {
    fn blank(bytes: &mut [u8], range: Range<usize>) {
        for byte in &mut bytes[range] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }

    let raw = src.as_bytes();
    let mut structural = raw.to_vec();
    let mut i = 0usize;
    while i < raw.len() {
        if raw[i..].starts_with(b"//") {
            let end = raw[i + 2..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(raw.len(), |offset| i + 2 + offset);
            blank(&mut structural, i..end);
            i = end;
            continue;
        }
        if raw[i..].starts_with(b"/*") {
            let mut end = i + 2;
            let mut depth = 1usize;
            while end < raw.len() && depth != 0 {
                if raw[end..].starts_with(b"/*") {
                    depth += 1;
                    end += 2;
                } else if raw[end..].starts_with(b"*/") {
                    depth -= 1;
                    end += 2;
                } else {
                    end += 1;
                }
            }
            blank(&mut structural, i..end);
            i = end;
            continue;
        }

        // `r"..."`, `r#"..."#`, and the `r` portion of `br#"..."#`/`cr#"..."#`.
        if raw[i] == b'r' {
            let mut quote = i + 1;
            while quote < raw.len() && raw[quote] == b'#' {
                quote += 1;
            }
            if quote < raw.len() && raw[quote] == b'"' {
                let hashes = quote - (i + 1);
                let mut end = quote + 1;
                while end < raw.len() {
                    if raw[end] == b'"'
                        && end + 1 + hashes <= raw.len()
                        && raw[end + 1..end + 1 + hashes]
                            .iter()
                            .all(|byte| *byte == b'#')
                    {
                        end += 1 + hashes;
                        break;
                    }
                    end += 1;
                }
                blank(&mut structural, i..end);
                i = end;
                continue;
            }
        }

        if raw[i] == b'"' {
            let mut end = i + 1;
            while end < raw.len() {
                if raw[end] == b'\\' {
                    end = (end + 2).min(raw.len());
                } else if raw[end] == b'"' {
                    end += 1;
                    break;
                } else {
                    end += 1;
                }
            }
            blank(&mut structural, i..end);
            i = end;
            continue;
        }

        // Distinguish a character literal from a lifetime by requiring the closing quote after
        // exactly one escaped byte or one UTF-8 scalar.
        if raw[i] == b'\'' {
            let mut end = i + 1;
            if end < raw.len() && raw[end] == b'\\' {
                end += 1;
                match raw.get(end).copied() {
                    Some(b'x') => end = (end + 3).min(raw.len()),
                    Some(b'u') if raw.get(end + 1) == Some(&b'{') => {
                        end += 2;
                        while end < raw.len() && raw[end] != b'}' {
                            end += 1;
                        }
                        end = (end + 1).min(raw.len());
                    }
                    Some(_) => end += 1,
                    None => {}
                }
            } else if end < raw.len() {
                end += 1;
                while end < raw.len() && raw[end] & 0xc0 == 0x80 {
                    end += 1;
                }
            }
            if end < raw.len() && raw[end] == b'\'' {
                end += 1;
                blank(&mut structural, i..end);
                i = end;
                continue;
            }
        }
        i += 1;
    }
    structural
}

/// The first body-opening brace or declaration-ending semicolon outside parameter/array
/// delimiters. In particular, the semicolon in `fn f(_: [u8; 32]) {}` is not a trait-method
/// terminator.
fn item_delimiter(structural: &[u8], start: usize) -> Option<(usize, u8)> {
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    for (offset, byte) in structural[start..].iter().copied().enumerate() {
        match byte {
            b'(' => parentheses += 1,
            b')' => parentheses = parentheses.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'{' | b';' if parentheses == 0 && brackets == 0 => {
                return Some((start + offset, byte));
            }
            _ => {}
        }
    }
    None
}

/// Byte ranges belonging to items carrying the exact `#[cfg(test)]` predicate.
///
/// The exclusion is deliberately narrow. `cfg(any(test, ...))`, out-of-line modules, and any
/// item whose braces cannot be proven balanced remain in the production census as conservative
/// over-reports. Strings and comments have already been masked, so a brace in either cannot make
/// this routine swallow a following production item.
fn cfg_test_item_ranges(src: &str) -> Vec<Range<usize>> {
    const ATTRIBUTE: &[u8] = b"#[cfg(test)]";
    let structural = structural_rust_bytes(src);
    let mut ranges = Vec::new();
    let mut search = 0usize;

    while search + ATTRIBUTE.len() <= structural.len() {
        let Some(relative) = structural[search..]
            .windows(ATTRIBUTE.len())
            .position(|window| window == ATTRIBUTE)
        else {
            break;
        };
        let start = search + relative;
        let mut item = start + ATTRIBUTE.len();
        while item < structural.len() && structural[item].is_ascii_whitespace() {
            item += 1;
        }

        // Other attributes may sit between cfg(test) and the item.
        while structural[item..].starts_with(b"#[") {
            let mut cursor = item + 1;
            let mut depth = 0usize;
            let mut closed = false;
            while cursor < structural.len() {
                match structural[cursor] {
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            cursor += 1;
                            closed = true;
                            break;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
            if !closed {
                break;
            }
            item = cursor;
            while item < structural.len() && structural[item].is_ascii_whitespace() {
                item += 1;
            }
        }

        if let Some((brace, b'{')) = item_delimiter(&structural, item) {
            let mut cursor = brace;
            let mut depth = 0usize;
            let mut end = None;
            while cursor < structural.len() {
                match structural[cursor] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(cursor + 1);
                            break;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
            if let Some(end) = end {
                ranges.push(start..end);
            }
        }
        search = start + 1;
    }
    ranges
}

/// Every production `fn`, with its body delimited by brace count.
///
/// ⚠★ **A body whose braces never balance runs to END OF FILE rather than being skipped**, and
/// the direction of that choice is the whole point. Comments and literals are structurally
/// masked, but a macro token shape can still defeat this deliberately small lexer. SKIPPING such
/// a function hides every call it makes, which UNDER-reports, while running to EOF folds extra callees in,
/// which OVER-reports. **Only one of those can cause a traversal to escape the census.**
///
/// An earlier draft skipped, and found fewer files than the same algorithm that did not.
fn fn_bodies(src: &str) -> Vec<FunctionBody> {
    let structural = structural_rust_bytes(src);
    let structural_text = std::str::from_utf8(&structural)
        .expect("masking Rust comments and literals preserves valid UTF-8");
    let bytes = structural_text.as_bytes();
    let excluded = cfg_test_item_ranges(src);
    let mut out = Vec::new();
    let mut search = 0usize;
    while search + 2 <= bytes.len() {
        let Some(relative) = bytes[search..]
            .windows(2)
            .position(|window| window == b"fn")
        else {
            break;
        };
        let start = search + relative;
        search = start + 2;
        let word_before = start != 0
            && matches!(bytes[start - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_');
        if word_before || search == bytes.len() || !bytes[search].is_ascii_whitespace() {
            continue;
        }
        while search < bytes.len() && bytes[search].is_ascii_whitespace() {
            search += 1;
        }
        let name_start = search;
        while search < bytes.len()
            && matches!(bytes[search], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        {
            search += 1;
        }
        if search == name_start {
            continue;
        }
        let name = src[name_start..search].to_owned();
        let Some((delimiter, kind)) = item_delimiter(&structural, search) else {
            continue;
        };
        // A required trait method has no body. Without this check its `fn ...;`
        // steals the next method's opening brace and invents a call-graph node
        // containing that neighbour's body.
        if kind == b';' {
            search = delimiter + 1;
            continue;
        }
        let bstart = delimiter;
        let mut depth = 0i32;
        let mut closed = None;
        for (k, c) in structural[bstart..].iter().copied().enumerate() {
            match c {
                b'{' => depth += 1,
                b'}' => {
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
        if excluded.iter().any(|range| range.contains(&start)) {
            continue;
        }
        out.push(FunctionBody {
            name,
            source_offset: start,
            body: structural_text[bstart..end].to_string(),
            term_text: structural_text[start..end].to_string(),
        });
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

/// A function definition's stable-enough source identity. The ordinal is counted among
/// production definitions with the same `(file, name)`, in source order. Unlike a byte offset it
/// survives whitespace, comment, and unrelated-function edits; unlike `(file, name)` it does not
/// merge overloaded trait methods.
type Node = (usize, String, usize); // (file index, fn name, same-name definition ordinal)

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ComponentMember {
    file: String,
    function: String,
    definition_ordinal: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ComponentKey(Vec<ComponentMember>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MemberSpec {
    file: &'static str,
    function: &'static str,
    definition_ordinal: usize,
}

#[derive(Debug, Clone, Copy)]
struct ComponentDisposition {
    label: &'static str,
    members: &'static [MemberSpec],
    disposition: Disposition,
}

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
    rel: Vec<String>,
    /// `(byte offset, one-based line)`, diagnostic metadata only, never component identity.
    source_locations: BTreeMap<Node, (usize, usize)>,
}

fn component_key(component: &[Node], relative_files: &[String]) -> ComponentKey {
    let mut members = component
        .iter()
        .map(|(file, function, definition_ordinal)| ComponentMember {
            file: relative_files[*file].clone(),
            function: function.clone(),
            definition_ordinal: *definition_ordinal,
        })
        .collect::<Vec<_>>();
    members.sort_unstable();
    ComponentKey(members)
}

fn declared_component_key(members: &[MemberSpec]) -> ComponentKey {
    let mut members = members
        .iter()
        .map(|member| ComponentMember {
            file: member.file.to_owned(),
            function: member.function.to_owned(),
            definition_ordinal: member.definition_ordinal,
        })
        .collect::<Vec<_>>();
    members.sort_unstable();
    ComponentKey(members)
}

fn display_member(member: &ComponentMember) -> String {
    format!(
        "{}::{}#{}",
        member.file, member.function, member.definition_ordinal
    )
}

fn component_literal(component: &ComponentKey) -> String {
    component
        .0
        .iter()
        .map(|member| {
            format!(
                "({:?}, {:?}, {}),",
                member.file, member.function, member.definition_ordinal
            )
        })
        .collect::<Vec<_>>()
        .join("\n            ")
}

fn run_census_for_sources(sources: &[(String, String)]) -> Census {
    let rel = sources.iter().map(|(path, _)| path.clone()).collect();

    // (file, name, same-name ordinal) -> callee names ; name -> definitions
    let mut bodies: BTreeMap<Node, BTreeSet<String>> = BTreeMap::new();
    let mut text: BTreeMap<Node, String> = BTreeMap::new();
    let mut defined_in: BTreeMap<String, BTreeSet<Node>> = BTreeMap::new();
    let mut source_locations = BTreeMap::new();

    for (fi, (_, src)) in sources.iter().enumerate() {
        let mut definition_ordinals = BTreeMap::<String, usize>::new();
        for function in fn_bodies(src) {
            let definition_ordinal = definition_ordinals
                .entry(function.name.clone())
                .or_default();
            let key = (fi, function.name.clone(), *definition_ordinal);
            *definition_ordinal += 1;
            defined_in
                .entry(function.name)
                .or_default()
                .insert(key.clone());
            bodies.insert(key.clone(), callees(&function.body));
            text.insert(key.clone(), function.term_text);
            let line = src[..function.source_offset]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            source_locations.insert(key, (function.source_offset, line));
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
        rel,
        source_locations,
    }
}

fn run_census_for_roots(roots: &[&str]) -> Census {
    let root = workspace_root();
    let files = source_files(&root, roots);
    assert!(
        !files.is_empty(),
        "the census found NO source files under {roots:?}. Either a root moved or \
         the walk is broken — and an empty scan makes every assertion below vacuous."
    );
    let sources = files
        .iter()
        .map(|path| {
            (
                path.strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                std::fs::read_to_string(path).unwrap_or_else(|error| {
                    panic!(
                        "recursion census cannot read Rust source {}: {error}",
                        path.display()
                    )
                }),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        !sources.is_empty(),
        "the census found source paths but could read none of them under {roots:?}"
    );
    run_census_for_sources(&sources)
}

fn run_census() -> Census { run_census_for_roots(CRATE_ROOTS) }

// ═══════════════════════════════════════════════════════════════════════════════════════
// The assertions
// ═══════════════════════════════════════════════════════════════════════════════════════

/// The recursive twins are test evidence, not production implementation
/// modules. This catches both halves of a bad relocation: losing the oracle and
/// retaining a duplicate under `src`.
#[test]
fn recursive_oracles_live_only_under_test_support() {
    let root = workspace_root();
    for (test_path, old_source_path) in TEST_SUPPORT_ORACLE_MOVES {
        assert!(
            root.join(test_path).is_file(),
            "recursive oracle missing from test support: {test_path}"
        );
        assert!(
            !root.join(old_source_path).exists(),
            "recursive oracle leaked back into a production source root: {old_source_path}"
        );
    }

    let score_tree =
        std::fs::read_to_string(root.join("models/src/rust/rholang/sorter/score_tree.rs"))
            .expect("read production score-tree comparator");
    assert!(
        !score_tree.contains("fn compare_score_recursive"),
        "the recursive score comparator must remain in models/tests/support/score_tree_oracle.rs"
    );
    assert!(
        root.join("models/tests/support/score_tree_oracle.rs")
            .is_file(),
        "recursive score-tree oracle missing from test support"
    );
}

/// ★★ Every derived production term-family SCC has one exact member-level disposition.
///
/// Exact two-way equality is the essential property: a new cycle in `reduce.rs` cannot inherit
/// the evaluator driver's disposition merely because both happen to occupy the same file.
#[test]
fn every_handwritten_term_recursion_has_an_exact_component_disposition() {
    let c = run_census();

    let root = workspace_root();
    let missing_roots: Vec<&str> = CRATE_ROOTS
        .iter()
        .copied()
        .filter(|path| !root.join(path).is_dir())
        .collect();
    assert!(
        missing_roots.is_empty(),
        "RECURSION CENSUS ROOTS DISAPPEARED: {missing_roots:?}"
    );

    let mut labels = BTreeSet::new();
    let mut expected = BTreeMap::<ComponentKey, &ComponentDisposition>::new();
    for declaration in RECURSION_DISPOSITIONS {
        assert!(
            labels.insert(declaration.label),
            "DUPLICATE RECURSION COMPONENT LABEL: {:?}",
            declaration.label
        );
        assert!(
            !declaration.members.is_empty(),
            "VACUOUS RECURSION COMPONENT {:?}: its member list is empty",
            declaration.label
        );
        let unique_members = declaration.members.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            unique_members.len(),
            declaration.members.len(),
            "DUPLICATE MEMBER inside recursion component {:?}",
            declaration.label
        );
        let reason = match declaration.disposition {
            Disposition::Measured(reason) | Disposition::NotATermDepthCycle(reason) => reason,
        };
        assert!(
            !reason.trim().is_empty(),
            "VACUOUS RECURSION DISPOSITION {:?}: its engineering reason is empty",
            declaration.label
        );
        let key = declared_component_key(declaration.members);
        if let Some(previous) = expected.insert(key.clone(), declaration) {
            panic!(
                "DUPLICATE RECURSION COMPONENT KEY: {:?} and {:?} declare the same members:\n  {}",
                previous.label,
                declaration.label,
                key.0
                    .iter()
                    .map(display_member)
                    .collect::<Vec<_>>()
                    .join("\n  ")
            );
        }
    }
    assert!(
        !expected.is_empty(),
        "RECURSION CENSUS DECLARATIONS WENT VACUOUS: the exact component set is empty"
    );

    let actual = c
        .term_family
        .iter()
        .map(|component| (component_key(component, &c.rel), component))
        .collect::<BTreeMap<_, _>>();
    let actual_keys = actual.keys().cloned().collect::<BTreeSet<_>>();
    let expected_keys = expected.keys().cloned().collect::<BTreeSet<_>>();
    let unclassified = actual_keys
        .difference(&expected_keys)
        .cloned()
        .collect::<Vec<_>>();
    let stale = expected_keys
        .difference(&actual_keys)
        .cloned()
        .collect::<Vec<_>>();

    if !unclassified.is_empty() || !stale.is_empty() {
        fn overlap(left: &ComponentKey, right: &ComponentKey) -> usize {
            left.0
                .iter()
                .filter(|member| right.0.binary_search(member).is_ok())
                .count()
        }

        let mut report = String::new();
        writeln!(
            report,
            "EXACT HAND-WRITTEN RECURSION CENSUS CHANGED: {} unclassified, {} stale",
            unclassified.len(),
            stale.len()
        )
        .unwrap();

        for component in &unclassified {
            writeln!(
                report,
                "\nUNCLASSIFIED SCC ({} member(s)):",
                component.0.len()
            )
            .unwrap();
            let nodes = actual
                .get(component)
                .expect("an unclassified key came from the actual component map");
            for member in &component.0 {
                let node = nodes
                    .iter()
                    .find(|(file, function, ordinal)| {
                        c.rel[*file].as_str() == member.file.as_str()
                            && function.as_str() == member.function.as_str()
                            && *ordinal == member.definition_ordinal
                    })
                    .expect("canonical member came from this SCC");
                let (offset, line) = c.source_locations[node];
                writeln!(
                    report,
                    "  + {} @ line {}, byte {}",
                    display_member(member),
                    line,
                    offset
                )
                .unwrap();
            }
            writeln!(
                report,
                "\nCopy-pastable member list:\n        members: members![\n            {}\n        ],",
                component_literal(component)
            )
            .unwrap();

            let mut nearest = expected
                .iter()
                .filter_map(|(candidate, declaration)| {
                    let shared = overlap(component, candidate);
                    (shared != 0).then_some((shared, declaration.label, candidate))
                })
                .collect::<Vec<_>>();
            nearest.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(right.1)));
            for (shared, label, candidate) in nearest.into_iter().take(3) {
                writeln!(
                    report,
                    "\nClosest declaration {:?} ({} shared):",
                    label, shared
                )
                .unwrap();
                for added in component
                    .0
                    .iter()
                    .filter(|member| candidate.0.binary_search(member).is_err())
                {
                    writeln!(report, "  + {}", display_member(added)).unwrap();
                }
                for removed in candidate
                    .0
                    .iter()
                    .filter(|member| component.0.binary_search(member).is_err())
                {
                    writeln!(report, "  - {}", display_member(removed)).unwrap();
                }
            }
        }

        for component in &stale {
            let declaration = expected[component];
            writeln!(
                report,
                "\nSTALE/SPLIT SCC {:?} ({} member(s)):",
                declaration.label,
                component.0.len()
            )
            .unwrap();
            for member in &component.0 {
                writeln!(report, "  - {}", display_member(member)).unwrap();
            }
        }

        panic!(
            "{report}\nA live unmeasured term-depth cycle is intentionally unrepresentable. \
             Convert it to an iterative traversal before adding a disposition. If this is a \
             conservative textual SCC, classify this exact member set and explain the false \
             back-edge; never add a file-wide exemption."
        );
    }

    let mutual = c
        .term_family
        .iter()
        .filter(|component| component.len() > 1)
        .count();
    let active_files = c
        .term_family
        .iter()
        .flat_map(|component| component.iter().map(|(file, _, _)| c.rel[*file].as_str()))
        .collect::<BTreeSet<_>>();
    println!(
        "  hand-written recursion census: {} recursive component(s), {} over the term family \
         ({} mutual), across {} file(s), all {} exactly dispositioned",
        c.recursive.len(),
        c.term_family.len(),
        mutual,
        active_files.len(),
        expected.len(),
    );
    for (component, declaration) in &expected {
        println!(
            "    {}: {}",
            declaration.label,
            component
                .0
                .iter()
                .map(display_member)
                .collect::<Vec<_>>()
                .join(" <-> ")
        );
    }
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
    let c = run_census_for_roots(&["rholang/tests/support/normalize_recursive.rs"]);
    const FILE: &str = "rholang/tests/support/normalize_recursive.rs";
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
        struct Par;
        trait Walk<T> { fn walk(&mut self, value: T, par: &Par); }
        struct C;
        impl Walk<u8> for C {
            fn walk(&mut self, value: u8, par: &Par) {
                if value > 0 { self.walk(value - 1, par); }
            }
        }
        impl Walk<u16> for C {
            fn walk(&mut self, _value: u16, _par: &Par) { leaf(); }
        }
    "#;

    let bodies: Vec<_> = fn_bodies(source)
        .into_iter()
        .filter(|function| function.name == "walk")
        .collect();
    assert_eq!(bodies.len(), 2, "both trait impl bodies must remain nodes");
    assert!(callees(&bodies[0].body).contains("walk"));
    assert!(!callees(&bodies[1].body).contains("walk"));

    let first_recursive = run_census_for_sources(&[("overloads.rs".into(), source.into())]);
    assert_eq!(first_recursive.term_family.len(), 1);
    assert_eq!(
        component_key(&first_recursive.term_family[0], &first_recursive.rel),
        ComponentKey(vec![ComponentMember {
            file: "overloads.rs".into(),
            function: "walk".into(),
            definition_ordinal: 0,
        }]),
        "the recursive first overload must be identified as #0 rather than merged with #1"
    );

    let second_recursive_source = source
        .replace(
            "if value > 0 { self.walk(value - 1, par); }",
            "if value > 0 { leaf(); }",
        )
        .replace(
            "fn walk(&mut self, _value: u16, _par: &Par) { leaf(); }",
            "fn walk(&mut self, value: u16, par: &Par) { self.walk(value, par); }",
        );
    let second_recursive =
        run_census_for_sources(&[("overloads.rs".into(), second_recursive_source)]);
    assert_eq!(second_recursive.term_family.len(), 1);
    assert_eq!(
        component_key(&second_recursive.term_family[0], &second_recursive.rel),
        ComponentKey(vec![ComponentMember {
            file: "overloads.rs".into(),
            function: "walk".into(),
            definition_ordinal: 1,
        }]),
        "moving recursion to the second overload must change the exact component identity"
    );
}

#[test]
fn function_tokens_accept_all_rust_whitespace_and_term_detection_includes_the_header() {
    let source = "struct Par;\nfn\nnewline(value: &Par) { newline(value); }\n\
                  fn\ttab(value: &Par) { tab(value); }\n";
    let bodies = fn_bodies(source);
    assert_eq!(
        bodies
            .iter()
            .map(|function| function.name.as_str())
            .collect::<Vec<_>>(),
        ["newline", "tab"]
    );
    assert!(bodies
        .iter()
        .all(|function| function.term_text.contains("Par")));

    let census = run_census_for_sources(&[("whitespace.rs".into(), source.into())]);
    let keys = census
        .term_family
        .iter()
        .map(|component| component_key(component, &census.rel))
        .collect::<BTreeSet<_>>();
    assert_eq!(keys.len(), 2, "both valid fn spellings must remain visible");
}

/// Calibration for the declaration-delimiter defect that previously hid reducer SCCs whose
/// signatures contained fixed-size arrays. The `;` belongs to the array type, not to a bodyless
/// trait declaration, so function discovery must continue through the opening brace.
#[test]
fn array_type_semicolons_cannot_terminate_function_discovery() {
    let source = "struct Par; fn walk(value: &[Par; 32]) { walk(value); }";
    let census = run_census_for_sources(&[("array.rs".into(), source.into())]);
    assert_eq!(census.term_family.len(), 1);
    assert_eq!(
        component_key(&census.term_family[0], &census.rel),
        ComponentKey(vec![ComponentMember {
            file: "array.rs".into(),
            function: "walk".into(),
            definition_ordinal: 0,
        }])
    );
}

#[test]
fn cfg_test_exclusion_is_structural_narrow_and_cannot_swallow_following_production() {
    let source = r##"
        struct Par;
        #[cfg(test)]
        mod tests {
            const BRACES: &str = "}} fn escaped(value: &Par) { escaped(value); } {{";
            /* } a comment cannot close the module { */
            fn hidden(value: &Par) { hidden(value); }
        }
        #[cfg(test)]
        fn directly_hidden(value: &Par) { directly_hidden(value); }
        fn visible(value: &Par) { visible(value); }

        // Not an exact test-only predicate: conservative inclusion is required because this
        // function can exist under a non-test feature.
        #[cfg(any(test, feature = "fixture"))]
        fn conditional(value: &Par) { conditional(value); }
    "##;
    let census = run_census_for_sources(&[("cfg.rs".into(), source.into())]);
    let keys = census
        .term_family
        .iter()
        .map(|component| component_key(component, &census.rel))
        .collect::<BTreeSet<_>>();
    let names = keys
        .iter()
        .flat_map(|component| component.0.iter().map(|member| member.function.as_str()))
        .collect::<BTreeSet<_>>();
    assert_eq!(names, BTreeSet::from(["conditional", "visible"]));
}

#[test]
fn comments_and_literals_cannot_invent_calls_but_executable_calls_remain_visible() {
    let source = r#"
        struct Par;
        fn prose(value: &Par) {
            let _ = "prose ( value )";
            // prose(value);
            /* prose(value); */
        }
        fn executable(value: &Par) { executable (value); }
    "#;
    let census = run_census_for_sources(&[("lexical.rs".into(), source.into())]);
    assert_eq!(census.term_family.len(), 1);
    assert_eq!(
        component_key(&census.term_family[0], &census.rel).0[0]
            .function
            .as_str(),
        "executable"
    );
}

#[test]
fn a_second_cycle_in_an_already_known_file_is_not_covered_by_the_first() {
    let baseline = r#"
        struct Par;
        fn known(value: &Par) { known(value); }
        fn later(value: &Par) { leaf(value); }
    "#;
    let mutated = baseline.replace("leaf(value)", "later(value)");
    let before = run_census_for_sources(&[("known.rs".into(), baseline.into())]);
    let after = run_census_for_sources(&[("known.rs".into(), mutated)]);
    let expected = before
        .term_family
        .iter()
        .map(|component| component_key(component, &before.rel))
        .collect::<BTreeSet<_>>();
    let actual = after
        .term_family
        .iter()
        .map(|component| component_key(component, &after.rel))
        .collect::<BTreeSet<_>>();
    let added = actual.difference(&expected).collect::<Vec<_>>();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].0[0].function.as_str(), "later");
    assert_eq!(
        expected
            .iter()
            .flat_map(|component| component.0.iter().map(|member| member.file.as_str()))
            .collect::<BTreeSet<_>>(),
        actual
            .iter()
            .flat_map(|component| component.0.iter().map(|member| member.file.as_str()))
            .collect::<BTreeSet<_>>(),
        "the old file-level gate would see identical active-file sets and miss this mutation"
    );
}

#[test]
fn adding_a_member_to_an_existing_scc_invalidates_its_exact_key() {
    let baseline = "struct Par; fn left(v: &Par) { right(v); } fn right(v: &Par) { left(v); }";
    let mutated = "struct Par; fn left(v: &Par) { right(v); } \
                   fn right(v: &Par) { left(v); bridge(v); } \
                   fn bridge(v: &Par) { left(v); }";
    let before = run_census_for_sources(&[("members.rs".into(), baseline.into())]);
    let after = run_census_for_sources(&[("members.rs".into(), mutated.into())]);
    let before_key = component_key(&before.term_family[0], &before.rel);
    let after_key = component_key(&after.term_family[0], &after.rel);
    assert_ne!(before_key, after_key);
    assert_eq!(before_key.0.len(), 2);
    assert_eq!(after_key.0.len(), 3);
    assert!(after_key
        .0
        .iter()
        .any(|member| member.function.as_str() == "bridge"));
}

#[test]
fn ordinal_identity_survives_offsets_and_unrelated_function_insertions() {
    let baseline = "struct Par; fn walk(v: &Par) { walk(v); }";
    let shifted = "// leading text changes every byte offset\n\n\
                   fn unrelated() {}\n\
                   struct Par; fn walk(v: &Par) { walk(v); }";
    let before = run_census_for_sources(&[("stable.rs".into(), baseline.into())]);
    let after = run_census_for_sources(&[("stable.rs".into(), shifted.into())]);
    assert_eq!(
        component_key(&before.term_family[0], &before.rel),
        component_key(&after.term_family[0], &after.rel)
    );
    assert_ne!(
        before.source_locations[&before.term_family[0][0]].0,
        after.source_locations[&after.term_family[0][0]].0,
        "the proof must actually perturb the byte offset"
    );
}

#[test]
fn a_cross_file_scc_has_one_exact_key_containing_every_member() {
    let census = run_census_for_sources(&[
        (
            "left.rs".into(),
            "struct Par; fn left(value: &Par) { right(value); }".into(),
        ),
        (
            "right.rs".into(),
            "struct Par; fn right(value: &Par) { left(value); }".into(),
        ),
    ]);
    assert_eq!(census.term_family.len(), 1);
    assert_eq!(
        component_key(&census.term_family[0], &census.rel),
        ComponentKey(vec![
            ComponentMember {
                file: "left.rs".into(),
                function: "left".into(),
                definition_ordinal: 0,
            },
            ComponentMember {
                file: "right.rs".into(),
                function: "right".into(),
                definition_ordinal: 0,
            },
        ])
    );
}
