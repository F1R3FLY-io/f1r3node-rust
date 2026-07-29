//! # The SCHEMA-META conformance probe — what the generator says about itself
//!
//! `models/build/wire_schema.rs` makes one walk of the protobuf descriptor and
//! emits four files. Two of them are field tables whose bytes are checked
//! elsewhere (`wire_schema_conformance.rs` against serde's own derive,
//! `serializer_par_byte_goldens.rs` against recorded bytes). This file checks the
//! other two — the **prost order** and the **schema meta** — and it checks the
//! claims the generator's own documentation makes, because a comment that is not
//! executed is a comment that drifts.
//!
//! Four properties, and each replaces a specific way this campaign has already
//! been wrong:
//!
//! | § | property | the mistake it forecloses |
//! |---|---|---|
//! | 1 | the prost order IS ascending minimum tag, per message | reusing the bincode order table for the protobuf driver |
//! | 2 | `Par` and `TaggedContinuation` differ from declaration order — and no other message does | "the two orders are the same in practice" |
//! | 3 | `Par` is a feedback vertex set of size 1, and the `Par`-free chain is 3 edges | "one `impl Drop` is probably enough" |
//! | 4 | every derive surface is dispositioned and the registry is not degenerate | a hand-picked driver list that missed `Hash` |
//!
//! ⚠ None of these is a byte-level claim, so none of them can fork consensus by
//! itself. They are claims about the TABLE the drivers are generated from, which
//! is the layer at which a wrong belief becomes a wrong encoder.

use std::collections::{BTreeMap, BTreeSet};

use models::rust::rholang::prost_wire::{ProstField, ProstKind};
use models::rust::rholang::schema_meta::Disposition;
use models::rust::rholang::prost_wire_schema::PROST_CONFORMANCE_REGISTRY;
use models::rust::rholang::schema_meta_tables::{
    DERIVE_DISPOSITION_REGISTRY, DISPOSITIONED_DERIVES, HAND_WRITTEN_TRAVERSALS, RECURSIVE_TYPES,
    SCHEMA_CHILDREN, SCHEMA_SCC,
};
use models::rust::rholang::wire_schema::CONFORMANCE_REGISTRY;

// ===========================================================================
// §0  The four outputs exist and are not degenerate
// ===========================================================================

/// A table that silently emptied would make every assertion below vacuous.
///
/// ★ The counts are lower bounds derived from the schema as it stands, not
/// equalities: an equality would have to be edited every time a message is
/// added, and a bound that has to be edited is a bound somebody edits without
/// thinking.
#[test]
fn the_four_generated_tables_are_populated() {
    assert!(
        CONFORMANCE_REGISTRY.len() >= 57,
        "the bincode registry collapsed to {} rows",
        CONFORMANCE_REGISTRY.len()
    );
    assert_eq!(
        PROST_CONFORMANCE_REGISTRY.len(),
        CONFORMANCE_REGISTRY.len(),
        "★ ONE walk, TWO tables: the prost registry and the bincode registry must cover \
         EXACTLY the same messages. A difference means one emitter skipped a type the other \
         kept, and the tables would describe two different schemas."
    );
    assert!(
        SCHEMA_CHILDREN.len() > PROST_CONFORMANCE_REGISTRY.len(),
        "the child relation must additionally carry the `extern_path`'d types ({} rows vs {} \
         generated messages) — they are nodes of the containment graph even though they have \
         no descriptor-driven program",
        SCHEMA_CHILDREN.len(),
        PROST_CONFORMANCE_REGISTRY.len()
    );
    assert!(
        !SCHEMA_SCC.is_empty() && !RECURSIVE_TYPES.is_empty(),
        "the SCC decomposition is empty, so every claim about recursion below is vacuous"
    );
    assert!(
        DERIVE_DISPOSITION_REGISTRY.len() >= 8 * PROST_CONFORMANCE_REGISTRY.len(),
        "the derive registry has {} rows for {} messages — fewer than eight surfaces per \
         type, which cannot cover `::prost::Message` alone",
        DERIVE_DISPOSITION_REGISTRY.len(),
        PROST_CONFORMANCE_REGISTRY.len()
    );
    // Every message in one registry is in the other, BY NAME.
    let bincode: BTreeSet<&str> = CONFORMANCE_REGISTRY.iter().map(|(n, _, _)| *n).collect();
    let prost: BTreeSet<&str> = PROST_CONFORMANCE_REGISTRY.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        bincode, prost,
        "the two registries must name the same messages"
    );
}

// ===========================================================================
// §1  The prost order IS ascending minimum tag
// ===========================================================================

/// ★★ **The sort key, executed.**
///
/// `prost-derive-0.14.3/src/lib.rs:87-92` sorts by
/// `field.tags().into_iter().min()` before building `encode_raw` and
/// `encoded_len`. The generated table claims to reproduce that. This asserts it
/// for every message at once, rather than for the two somebody looked at.
///
/// ⚠ It is `<` and not `<=`: prost-derive *bails* on a duplicate tag
/// (`src/lib.rs:94-101`), so equal keys are not merely unordered, they are
/// impossible — and a table that admitted them would have an ambiguous order
/// that no assertion could pin.
#[test]
fn every_prost_program_is_in_ascending_tag_order() {
    let mut checked = 0usize;
    for (name, program) in PROST_CONFORMANCE_REGISTRY {
        for pair in program.windows(2) {
            assert!(
                pair[0].tag < pair[1].tag,
                "`{name}`'s prost program is not in ascending tag order: `{}` (tag {}) \
                 precedes `{}` (tag {}). The protobuf encoder emits fields in this order, so \
                 a mis-sorted table writes a well-formed message with its fields transposed \
                 — which every protobuf decoder accepts.",
                pair[0].name,
                pair[0].tag,
                pair[1].name,
                pair[1].tag
            );
        }
        for field in *program {
            assert!(
                field.tag > 0,
                "`{name}.{}` carries proto tag 0, which protobuf does not permit and which \
                 would sort ahead of every real field",
                field.name
            );
        }
        checked += 1;
    }
    assert_eq!(checked, PROST_CONFORMANCE_REGISTRY.len());
}

/// The prost table and the bincode table must be a PERMUTATION of one another,
/// per message — same fields, same count, possibly different order.
///
/// ★ This is what makes "one walk, two orders" checkable. If the prost emitter
/// had dropped a field, or invented one, the two programs would differ as SETS
/// and no ordering assertion would notice.
#[test]
fn the_two_tables_are_permutations_of_one_another() {
    let prost: BTreeMap<&str, &[ProstField]> =
        PROST_CONFORMANCE_REGISTRY.iter().copied().collect();
    for (name, kinds, field_names) in CONFORMANCE_REGISTRY {
        let program = prost
            .get(name)
            .unwrap_or_else(|| panic!("`{name}` has a bincode program but no prost program"));
        assert_eq!(
            kinds.len(),
            field_names.len(),
            "`{name}`'s bincode kinds and names are different lengths"
        );
        assert_eq!(
            program.len(),
            field_names.len(),
            "`{name}` has {} prost fields and {} serde fields. The two tables come from ONE \
             resolved vector under two sort keys, so a length difference means an emitter \
             dropped or invented a field.",
            program.len(),
            field_names.len()
        );
        let serde_names: BTreeSet<&str> = field_names.iter().copied().collect();
        let prost_names: BTreeSet<&str> = program.iter().map(|f| f.name).collect();
        assert_eq!(
            serde_names, prost_names,
            "`{name}`'s two programs name different fields"
        );
    }
}

// ===========================================================================
// §2  ★★ The two messages where the orders DIFFER — and only those two
// ===========================================================================

/// ★★★ **The whole reason the order table is per-format, made executable.**
///
/// An earlier brief in this campaign cited prost-build's struct-writing order —
/// "plain fields first, then oneofs" — as prost's *emission* order. It is not:
/// that is the serde/bincode order. The protobuf order is ascending minimum tag,
/// and for `TaggedContinuation` the two are **exactly reversed**.
///
/// ⚠⚠ `TaggedContinuation` is the message whose SERDE order already cost this
/// campaign "a 95-byte encoding with its halves exchanged" — same length, same
/// byte multiset, invisible to a length check and to a round-trip. A generator
/// that reused the bincode order table for the prost driver would reproduce that
/// defect **in mirror**, on the hottest type in the schema.
///
/// So this test asserts the difference in both directions:
///
/// * the two named messages DO differ, arm for arm;
/// * **no other message differs** — which is what makes the two named ones a
///   finding rather than an anecdote, and what fails if a future `.proto` edit
///   introduces a third.
#[test]
fn exactly_two_messages_order_differently_under_the_two_formats() {
    let prost: BTreeMap<&str, &[ProstField]> =
        PROST_CONFORMANCE_REGISTRY.iter().copied().collect();

    let mut differing: Vec<&str> = Vec::new();
    for (name, _, field_names) in CONFORMANCE_REGISTRY {
        let program = prost[name];
        let prost_order: Vec<&str> = program.iter().map(|f| f.name).collect();
        if prost_order != *field_names {
            differing.push(name);
        }
    }
    differing.sort_unstable();

    assert_eq!(
        differing,
        vec!["Par", "TaggedContinuation"],
        "EXACTLY two messages must order differently under serde and protobuf. Found {:?}.\n\
         \n\
         If this grew, a `.proto` edit introduced a third message whose declaration order and \
         tag order disagree — which is fine, but it means any code that assumed \"the orders \
         coincide except for the two known cases\" is now wrong. If it SHRANK, an emitter has \
         stopped applying its own sort key, and one of the two tables is now describing the \
         other format's layout.",
        differing
    );

    // ── the exact orders, so "differs" cannot be satisfied by differing wrongly ──
    let par_serde: Vec<&str> = CONFORMANCE_REGISTRY
        .iter()
        .find(|(n, _, _)| *n == "Par")
        .map(|(_, _, names)| names.to_vec())
        .expect("Par is in the bincode registry");
    assert_eq!(
        par_serde,
        vec![
            "sends",
            "receives",
            "news",
            "exprs",
            "matches",
            "unforgeables",
            "bundles",
            "connectives",
            "conditionals",
            "locally_free",
            "connective_used",
        ],
        "`Par`'s SERDE order is declaration order: tags 1,2,4,5,6,7,11,8,12,9,10"
    );
    let par_prost: Vec<u32> = prost["Par"].iter().map(|f| f.tag).collect();
    assert_eq!(
        par_prost,
        vec![1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "`Par`'s PROTOBUF order is ascending tag — `bundles` (11) and `conditionals` (12) move \
         to the END, past `connectives` (8), `locally_free` (9) and `connective_used` (10)"
    );

    let tc_serde: Vec<&str> = CONFORMANCE_REGISTRY
        .iter()
        .find(|(n, _, _)| *n == "TaggedContinuation")
        .map(|(_, _, names)| names.to_vec())
        .expect("TaggedContinuation is in the bincode registry");
    assert_eq!(
        tc_serde,
        vec!["guard", "tagged_cont"],
        "`TaggedContinuation`'s SERDE order puts the plain field FIRST — prost-build writes \
         every plain field before every oneof, whatever the `.proto` declared"
    );
    let tc_prost: Vec<&str> = prost["TaggedContinuation"].iter().map(|f| f.name).collect();
    assert_eq!(
        tc_prost,
        vec!["tagged_cont", "guard"],
        "★★ `TaggedContinuation`'s PROTOBUF order is THE OPPOSITE: the oneof occupies tags \
         1-2 and `guard` is tag 3, and prost places a oneof at the position of its LOWEST \
         tag. This is the message whose serde order already produced a 95-byte encoding with \
         its halves exchanged; here the correct order is the reverse of that fix."
    );
    assert_eq!(
        prost["TaggedContinuation"][0].kind,
        ProstKind::Oneof,
        "the field that sorts first must be the ONEOF"
    );
    assert_eq!(prost["TaggedContinuation"][0].tag, 1);
    assert_eq!(prost["TaggedContinuation"][1].tag, 3);
}

/// ⚠ `locally_free` reaches the PROTOBUF wire unblanked.
///
/// The bincode table gives it a dedicated `FieldKind::EmptyBytes` because
/// `models/build.rs` injects `serialize_with = serialize_as_empty_bytes`. That
/// is a **serde-only** normalization; prost RETAINS the field. A prost table
/// that inherited the blanking would drop a field from every protobuf encoding
/// in the node.
#[test]
fn locally_free_is_ordinary_bytes_on_the_protobuf_wire() {
    let mut seen = 0usize;
    for (name, program) in PROST_CONFORMANCE_REGISTRY {
        for field in *program {
            if field.name == "locally_free" {
                assert_eq!(
                    field.kind,
                    ProstKind::Bytes,
                    "`{name}.locally_free` is {:?} in the prost table. It must be ordinary \
                     `Bytes`: the eight-zero-bytes normalization is serde-only, and \
                     `wire_encode_differential.rs` pins that prost keeps the real value.",
                    field.kind
                );
                seen += 1;
            }
        }
    }
    assert_eq!(
        seen, 12,
        "the schema has 12 `locally_free` fields (the count `models/build.rs` cross-checks \
         against its own textual rewrite); the prost table carries {seen}"
    );
}

// ===========================================================================
// §3  ★★ `Par` is a FEEDBACK VERTEX SET of size one
// ===========================================================================

/// Iterative Tarjan over a NAMED SUBSET of [`SCHEMA_CHILDREN`].
///
/// ⚠ Iterative, in a test about recursive traversals, for the obvious reason:
/// a recursive checker would measure itself.
fn cyclic_components(nodes: &BTreeSet<&'static str>) -> Vec<Vec<&'static str>> {
    let adjacency: BTreeMap<&str, Vec<&str>> = SCHEMA_CHILDREN
        .iter()
        .filter(|(n, _)| nodes.contains(n))
        .map(|(n, kids)| {
            (
                *n,
                kids.iter().copied().filter(|k| nodes.contains(k)).collect(),
            )
        })
        .collect();

    let mut index: BTreeMap<&str, usize> = BTreeMap::new();
    let mut lowlink: BTreeMap<&str, usize> = BTreeMap::new();
    let mut on_stack: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut next = 0usize;
    let mut out: Vec<Vec<&str>> = Vec::new();

    for root in nodes {
        if index.contains_key(root) {
            continue;
        }
        index.insert(root, next);
        lowlink.insert(root, next);
        next += 1;
        stack.push(root);
        on_stack.insert(root);
        let mut work: Vec<(&str, usize)> = vec![(root, 0)];

        while let Some(&mut (v, ref mut slot)) = work.last_mut() {
            let children = adjacency.get(v).map(Vec::as_slice).unwrap_or(&[]);
            if *slot < children.len() {
                let w = children[*slot];
                *slot += 1;
                if !index.contains_key(w) {
                    index.insert(w, next);
                    lowlink.insert(w, next);
                    next += 1;
                    stack.push(w);
                    on_stack.insert(w);
                    work.push((w, 0));
                } else if on_stack.contains(w) {
                    let iw = index[w];
                    let lv = lowlink[v];
                    lowlink.insert(v, lv.min(iw));
                }
                continue;
            }
            work.pop();
            if lowlink[v] == index[v] {
                let mut component = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack.remove(w);
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                let self_loop = component.len() == 1
                    && adjacency.get(component[0]).is_some_and(|k| k.contains(&component[0]));
                if component.len() > 1 || self_loop {
                    out.push(component);
                }
            }
            if let Some(&mut (parent, _)) = work.last_mut() {
                let lp = lowlink[parent];
                let lv = lowlink[v];
                lowlink.insert(parent, lp.min(lv));
            }
        }
    }
    out
}

/// ★★★ **`Par` alone suffices, and this is the proof — not the assumption.**
///
/// The question is whether a single `impl Drop for Par` (forwarding into the
/// existing iterative teardown, `par_children::dismantle`) bounds the schema's
/// recursion, or whether other types need one too. It does **iff every cycle in
/// the containment graph passes through `Par`** — i.e. iff `{Par}` is a feedback
/// vertex set.
///
/// The test is direct: remove `Par` from the graph and recompute the strongly
/// connected components. If none of them is cyclic, no cycle avoided `Par`.
///
/// ⚠ It was NOT assumed. The claim arrived as "the cycle is
/// `Par → Expr → ExprInstance → Par`, so ~3 frames"; the schema in fact has ONE
/// cyclic component of **37 members**, and a 37-member component can perfectly
/// well contain sub-cycles that avoid any given vertex. It does not — but that is
/// a computation, and this is where it is computed.
#[test]
fn par_is_a_feedback_vertex_set_of_size_one() {
    let all: BTreeSet<&str> = SCHEMA_CHILDREN.iter().map(|(n, _)| *n).collect();
    assert!(all.contains("Par"), "the child relation must contain `Par`");

    // ── the whole graph: exactly ONE cyclic component, and `Par` is in it ──
    let whole = cyclic_components(&all);
    assert_eq!(
        whole.len(),
        1,
        "the schema must have exactly ONE cyclic component; found {}: {:?}",
        whole.len(),
        whole
    );
    assert!(
        whole[0].contains(&"Par"),
        "the sole cyclic component must contain `Par`; it contains {:?}",
        whole[0]
    );
    assert_eq!(
        whole[0].len(),
        RECURSIVE_TYPES.len(),
        "`RECURSIVE_TYPES` ({}) must be exactly the members of the cyclic component ({})",
        RECURSIVE_TYPES.len(),
        whole[0].len()
    );

    // ── ★ THE PROPERTY: remove `Par` and nothing cyclic remains ──
    let without_par: BTreeSet<&str> = all.iter().copied().filter(|n| *n != "Par").collect();
    let remaining = cyclic_components(&without_par);
    assert!(
        remaining.is_empty(),
        "REMOVING `Par` LEFT {} CYCLIC COMPONENT(S): {:?}.\n\
         \n\
         `{{Par}}` is therefore NOT a feedback vertex set, and a single `impl Drop for Par` \
         would leave those cycles recursive — a term nested through them would still consume \
         native stack proportional to its depth. Each listed component needs a member with \
         its own `Drop`, and the count is what it is.",
        remaining.len(),
        remaining
    );
}

/// How much DERIVED recursion runs before the iterative teardown is reached.
///
/// With `Drop` on `Par` alone, dropping a bare `Expr` (say) descends through
/// rustc's derived glue until it reaches a `Par`, at which point the whole
/// remaining subtree unwinds iteratively. The depth of that prelude is a
/// property of the **type graph**, not of the term — and this is the number.
///
/// ## ⚠★ The quantity is "longest path that ENDS AT a `Par`", not "longest path"
///
/// The first version of this test computed the latter, and got the right answer
/// for the wrong reason: it scored `Receive → ReceiveBind → Var → WildcardMsg`
/// as a 3-edge prelude, when that chain never reaches a `Par` at all and drops
/// in constant stack. It also carried a `map_or(0, …)` fails-open default that
/// silently scored an uncomputed child as zero. Two defects, one of which
/// happened to cancel the other on this schema.
///
/// So the recurrence is stated with an explicit "does not reach `Par`" case:
///
/// ```math
/// D(v) \;=\; \max_{w \,\in\, \mathrm{children}(v)}
///   \begin{cases}
///     1        & w = \texttt{Par} \\
///     1 + D(w) & w \neq \texttt{Par},\; D(w) \text{ defined} \\
///     \text{undefined} & \text{otherwise}
///   \end{cases}
/// ```
///
/// and a node with no defined `` $D$ `` is *excluded* rather than scored zero.
/// Sixteen of the 57 types are in that class — `Var`, `GUnforgeable`, `EZipper`
/// and friends contain no `Par` on any path — and scoring them at all is what
/// produced the wrong attribution.
///
/// ★ The bound is asserted against the graph rather than transcribed: a `.proto`
/// edit that lengthens the chain fails here with the new number and the new
/// witness.
#[test]
fn the_derived_prelude_before_reaching_a_par_is_bounded_by_the_schema() {
    let adjacency: BTreeMap<&str, &[&str]> = SCHEMA_CHILDREN.iter().copied().collect();

    // Reverse-topological order over the `Par`-free subgraph, which
    // `par_is_a_feedback_vertex_set_of_size_one` proves is a DAG.
    //
    // ⚠ Iterative, again: the checker must not be the thing it is measuring.
    let mut order: Vec<&str> = Vec::new();
    let mut opened: BTreeSet<&str> = BTreeSet::new();
    let mut closed: BTreeSet<&str> = BTreeSet::new();
    let mut work: Vec<(&str, bool)> = SCHEMA_CHILDREN
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| *n != "Par")
        .map(|n| (n, false))
        .collect();
    while let Some((v, expanded)) = work.pop() {
        if expanded {
            if closed.insert(v) {
                order.push(v);
            }
            continue;
        }
        if !opened.insert(v) {
            continue;
        }
        work.push((v, true));
        for child in adjacency.get(v).copied().unwrap_or(&[]) {
            if *child != "Par" && !opened.contains(child) {
                work.push((child, false));
            }
        }
    }

    // `None` = this type contains no `Par` on ANY path, so it is not a prelude
    // at all. It is EXCLUDED, never scored zero.
    let mut reach: BTreeMap<&str, Option<usize>> = BTreeMap::new();
    for v in &order {
        let mut best: Option<usize> = None;
        for child in adjacency.get(v).copied().unwrap_or(&[]) {
            let candidate = if *child == "Par" {
                Some(1)
            } else {
                // ⚠ LOUD, not defaulted. Reverse-topological order guarantees a
                // child is scored before its parent; if it is not, the ordering
                // is wrong and every number below it is meaningless.
                let scored = reach.get(child).unwrap_or_else(|| {
                    panic!(
                        "`{v}`'s child `{child}` was not scored before it. The reverse \
                         topological order is broken, which means the `Par`-free subgraph is \
                         not the DAG `par_is_a_feedback_vertex_set_of_size_one` proves it is."
                    )
                });
                scored.map(|d| d + 1)
            };
            best = match (best, candidate) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, None) => a,
                (None, b) => b,
            };
        }
        reach.insert(v, best);
    }

    let reaching: Vec<(&str, usize)> = reach
        .iter()
        .filter_map(|(t, d)| d.map(|d| (*t, d)))
        .collect();
    assert!(
        !reaching.is_empty(),
        "no type reaches a `Par`, so this measurement is about nothing"
    );

    let worst = reaching.iter().map(|(_, d)| *d).max().expect("non-empty");
    let witnesses: Vec<&str> = reaching
        .iter()
        .filter(|(_, d)| *d == worst)
        .map(|(t, _)| *t)
        .collect();

    assert_eq!(
        (worst, witnesses.as_slice()),
        (3, ["Expr"].as_slice()),
        "the longest `Par`-terminating containment chain is {worst} edges, attained by {:?}.\n\
         \n\
         This is the derived recursion an `impl Drop for Par` does NOT bound: dropping a bare \
         value of one of those types runs that many levels of rustc's derived glue before it \
         reaches a `Par` and the iterative teardown takes over. It is bounded by the SCHEMA \
         and not by the input — which is why one `Drop` suffices — but the constant is worth \
         knowing, and a `.proto` edit that lengthens it should say so here rather than in a \
         stack trace.\n\
         \n\
         The expected witness is `Expr`, via `Expr -> EMap -> KeyValuePair -> Par`.",
        witnesses
    );

    // ⚠ And the excluded class is real, not empty — if every type reached a
    // `Par`, the `None` case above would be dead code and the recurrence's
    // careful handling of it would be untested.
    let unreaching: Vec<&str> = reach
        .iter()
        .filter(|(_, d)| d.is_none())
        .map(|(t, _)| *t)
        .collect();
    assert!(
        unreaching.contains(&"Var") && unreaching.contains(&"GUnforgeable"),
        "`Var` and `GUnforgeable` contain no `Par` on any path and must be EXCLUDED from the \
         prelude measurement rather than scored. Found the excluded set to be {unreaching:?}."
    );
}

// ===========================================================================
// §4  The derive disposition registry
// ===========================================================================

/// ★★ **Every derive surface is dispositioned, and the registry is not
/// degenerate.**
///
/// The build script already refuses an undispositioned trait — that check runs
/// at `models/build.rs` where the textual scan of `OUT_DIR/rhoapi.rs` is
/// visible. This is the run-time half: the EMITTED registry must actually carry
/// rows, cover every type, and carry no disposition whose payload is empty.
///
/// ⚠ A disposition with an empty payload would satisfy the type system and say
/// nothing: `NotATraversal("")` is exactly the unexamined entry the closed table
/// exists to prevent.
#[test]
fn every_derive_surface_is_dispositioned_with_a_reason() {
    let types: BTreeSet<&str> = DERIVE_DISPOSITION_REGISTRY.iter().map(|(t, _, _)| *t).collect();
    let messages: BTreeSet<&str> = CONFORMANCE_REGISTRY.iter().map(|(n, _, _)| *n).collect();
    for message in &messages {
        assert!(
            types.contains(message),
            "`{message}` has a generated wire program but NO rows in \
             DERIVE_DISPOSITION_REGISTRY. Every generated message carries the blanket \
             `.message_attribute` derives, so a type with no rows means the cross product \
             skipped it."
        );
    }

    for (ty, surface, disposition) in DERIVE_DISPOSITION_REGISTRY {
        assert!(!ty.is_empty(), "a registry row has no type");
        assert!(!surface.is_empty(), "`{ty}` has a row with no surface name");
        assert!(
            !disposition.detail().is_empty(),
            "`{ty}::{surface}` carries {disposition:?} with an EMPTY payload. Every \
             disposition's string is load-bearing — the reason, the driver, or the stage — \
             and an empty one is an unexamined entry wearing a decision's clothes."
        );
    }

    // ── the surfaces that must be present, because they are the campaign's ──
    let surfaces: BTreeSet<&str> = DERIVE_DISPOSITION_REGISTRY
        .iter()
        .map(|(_, s, _)| *s)
        .collect();
    for required in [
        "Serialize::serialize",
        "Deserialize::deserialize",
        "Clone::clone",
        "Ord::cmp",
        "PartialOrd::partial_cmp",
        "Message::encode_raw",
        "Message::encoded_len",
        "Message::merge_field",
        "Message::clear",
        "Debug::fmt",
        "Oneof::encode",
        "Oneof::merge",
    ] {
        assert!(
            surfaces.contains(required),
            "the registry does not mention `{required}`. The driver list is DERIVED from \
             this table; a missing surface is a walk nobody owes a driver for."
        );
    }
}

/// The two surfaces already converted, and the ones still owed a driver.
///
/// ★ Asserted by DISPOSITION rather than by name, so a stage that lands a driver
/// moves this test by changing the table — not by editing an expectation.
#[test]
fn the_converted_surfaces_are_the_two_this_campaign_has_landed() {
    let mut converted: BTreeSet<&str> = BTreeSet::new();
    let mut remaining: BTreeSet<&str> = BTreeSet::new();
    for (_, surface, disposition) in DERIVE_DISPOSITION_REGISTRY {
        match disposition {
            Disposition::Converted(_) => {
                converted.insert(surface);
            }
            Disposition::Remaining(_) => {
                remaining.insert(surface);
            }
            _ => {}
        }
    }
    assert_eq!(
        converted.iter().copied().collect::<Vec<_>>(),
        vec!["Deserialize::deserialize", "Serialize::serialize"],
        "exactly two derive surfaces have been converted: the bincode encoder (Stage H, \
         `wire_encode`) and the bincode decoder (Stage F, `par_codec`)"
    );
    assert!(
        remaining.contains("Message::encode_raw") && remaining.contains("Message::encoded_len"),
        "the protobuf encoder's two surfaces must still be `Remaining` until a driver lands \
         AND production call sites route through it; a table that marked them converted while \
         `prost::Message::encode_to_vec` is still what the node calls would be a claim about \
         code nobody runs"
    );
}

/// ⚠★ **The registry is a LOWER BOUND, and the bound is executable.**
///
/// `models/build.rs` STRIPS `PartialEq`, `Eq` and `Hash` from prost's output and
/// `models/src/lib.rs` writes them by hand — `<Par as PartialEq>::eq`
/// deliberately ignores `locally_free`, which no derive would do. A driver list
/// read off a `#[derive]` scan alone would therefore miss them, and a
/// hand-picked list of four already did miss `Hash`.
///
/// This asserts both halves: they are ABSENT from the derive registry (so the
/// registry is not silently claiming to cover them), and PRESENT in
/// `HAND_WRITTEN_TRAVERSALS` (so the campaign has not lost them).
#[test]
fn the_hand_written_traversals_are_named_and_not_in_the_derive_registry() {
    let surfaces: BTreeSet<&str> = DERIVE_DISPOSITION_REGISTRY
        .iter()
        .map(|(_, s, _)| *s)
        .collect();
    let hand: BTreeSet<&str> = HAND_WRITTEN_TRAVERSALS.iter().map(|(s, _)| *s).collect();

    for surface in ["PartialEq::eq", "Hash::hash", "Drop::drop"] {
        assert!(
            hand.contains(surface),
            "`{surface}` must be in HAND_WRITTEN_TRAVERSALS — it is a recursive walk over \
             the `Par` family that no `#[derive]` scan can see"
        );
        assert!(
            !surfaces.contains(surface),
            "`{surface}` appears in the DERIVE registry, but it is not a derive: \
             `models/build.rs` strips it and `models/src/lib.rs` (or rustc's implicit glue) \
             provides it. A registry that claimed it would make the cross-check against the \
             `#[derive]` scan fail for the wrong reason."
        );
    }
    for (surface, why) in HAND_WRITTEN_TRAVERSALS {
        assert!(
            !why.is_empty(),
            "`{surface}` is named with no provenance; the whole point of this list is that \
             the reader can find the code"
        );
    }

    assert!(
        !DISPOSITIONED_DERIVES.is_empty(),
        "the closed derive set is empty, so `models/build.rs`'s cross-check compares against \
         nothing and passes vacuously"
    );
    for token in ["Clone", "Ord", "PartialOrd", "::prost::Message", "::prost::Oneof"] {
        assert!(
            DISPOSITIONED_DERIVES.contains(&token),
            "`{token}` must be in the closed disposition set"
        );
    }
    for absent in ["PartialEq", "Hash"] {
        assert!(
            !DISPOSITIONED_DERIVES.contains(&absent),
            "`{absent}` must NOT be in the closed disposition set: `models/build.rs` removes \
             it from the generated file, so a row for it would be stale by construction and \
             the build-time cross-check would refuse it"
        );
    }
}
