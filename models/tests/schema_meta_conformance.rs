//! # The SCHEMA-META conformance probe — what the generator says about itself
//!
//! `models/codegen/schema.rs` makes one walk of the protobuf descriptor and
//! emits four files. Two of them are field tables whose bytes are checked
//! elsewhere (`bincode_schema_tables_conformance.rs` against serde's own derive,
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
//! | 2 | `TaggedContinuation` differs between generated formats; external `Par` is tested on its hand-written surfaces | "the two orders are the same in practice" |
//! | 3 | external `Par` breaks every generated cycle, and the longest generated path to that boundary is 3 edges | "externalizing the type probably removed the recursion" |
//! | 4 | every derive surface is dispositioned and the registry is not degenerate | a hand-picked driver list that missed `Hash` |
//!
//! ⚠ None of these is a byte-level claim, so none of them can fork consensus by
//! itself. They are claims about the TABLE the drivers are generated from, which
//! is the layer at which a wrong belief becomes a wrong encoder.

use std::collections::{BTreeMap, BTreeSet};

use models::rust::rholang::bincode_schema_tables::CONFORMANCE_REGISTRY;
use models::rust::rholang::protobuf_schema::{ProtobufField, ProtobufKind};
use models::rust::rholang::protobuf_schema_tables::PROTOBUF_CONFORMANCE_REGISTRY;
use models::rust::rholang::schema_meta::Disposition;
use models::rust::rholang::schema_meta_tables::{
    DERIVE_DISPOSITION_REGISTRY, DISPOSITIONED_DERIVES, HAND_WRITTEN_TRAVERSALS, RECURSIVE_TYPES,
    SCHEMA_CHILDREN, SCHEMA_SCC,
};
use prost::Message;

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
        CONFORMANCE_REGISTRY.len() >= 56,
        "the bincode registry collapsed to {} rows",
        CONFORMANCE_REGISTRY.len()
    );
    assert_eq!(
        PROTOBUF_CONFORMANCE_REGISTRY.len(),
        CONFORMANCE_REGISTRY.len(),
        "★ ONE walk, TWO tables: the prost registry and the bincode registry must cover \
         EXACTLY the same messages. A difference means one emitter skipped a type the other \
         kept, and the tables would describe two different schemas."
    );
    assert!(
        SCHEMA_CHILDREN.len() > PROTOBUF_CONFORMANCE_REGISTRY.len(),
        "the child relation must additionally carry the `extern_path`'d types ({} rows vs {} \
         generated messages) — they are nodes of the containment graph even though they have \
         no descriptor-driven program",
        SCHEMA_CHILDREN.len(),
        PROTOBUF_CONFORMANCE_REGISTRY.len()
    );
    assert!(!SCHEMA_SCC.is_empty(), "the SCC decomposition is empty");
    assert!(
        SCHEMA_SCC.iter().all(|component| component.len() == 1),
        "external PathMap-backed Par must break every generated cycle; found {:?}",
        SCHEMA_SCC
            .iter()
            .filter(|component| component.len() != 1)
            .collect::<Vec<_>>()
    );
    assert!(
        RECURSIVE_TYPES.is_empty(),
        "the generated child graph is acyclic after externalizing Par, but RECURSIVE_TYPES is \
         {:?}",
        RECURSIVE_TYPES
    );
    assert!(
        DERIVE_DISPOSITION_REGISTRY.len() >= 8 * PROTOBUF_CONFORMANCE_REGISTRY.len(),
        "the derive registry has {} rows for {} messages — fewer than eight surfaces per \
         type, which cannot cover `::prost::Message` alone",
        DERIVE_DISPOSITION_REGISTRY.len(),
        PROTOBUF_CONFORMANCE_REGISTRY.len()
    );
    // Every message in one registry is in the other, BY NAME.
    let bincode: BTreeSet<&str> = CONFORMANCE_REGISTRY.iter().map(|(n, _, _)| *n).collect();
    let protobuf: BTreeSet<&str> = PROTOBUF_CONFORMANCE_REGISTRY
        .iter()
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(
        bincode, protobuf,
        "the two registries must name the same messages"
    );
    assert!(
        !bincode.contains("Par"),
        "repository-owned PathMap Par must remain outside descriptor-generated codec tables"
    );
    assert!(
        SCHEMA_CHILDREN.iter().any(|(name, _)| *name == "Par"),
        "external Par must remain in the containment graph even though it has no generated program"
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
fn every_protobuf_program_is_in_ascending_tag_order() {
    let mut checked = 0usize;
    for (name, program) in PROTOBUF_CONFORMANCE_REGISTRY {
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
    assert_eq!(checked, PROTOBUF_CONFORMANCE_REGISTRY.len());
}

/// The prost table and the bincode table must be a PERMUTATION of one another,
/// per message — same fields, same count, possibly different order.
///
/// ★ This is what makes "one walk, two orders" checkable. If the prost emitter
/// had dropped a field, or invented one, the two programs would differ as SETS
/// and no ordering assertion would notice.
#[test]
fn the_two_tables_are_permutations_of_one_another() {
    let protobuf: BTreeMap<&str, &[ProtobufField]> =
        PROTOBUF_CONFORMANCE_REGISTRY.iter().copied().collect();
    for (name, kinds, field_names) in CONFORMANCE_REGISTRY {
        let program = protobuf
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
        let protobuf_names: BTreeSet<&str> = program.iter().map(|f| f.name).collect();
        assert_eq!(
            serde_names, protobuf_names,
            "`{name}`'s two programs name different fields"
        );
    }
}

// ===========================================================================
// §2  ★★ The generated message where the orders DIFFER — and only it
// ===========================================================================

/// ★★★ **The whole reason the order table is per-format, made executable.**
///
/// An earlier brief in this campaign cited prost-build's struct-writing order —
/// "plain fields first, then oneofs" — as prost's *emission* order. It is not:
/// that is the serde/bincode order. The protobuf order is ascending minimum tag,
/// and for `TaggedContinuation` the oneof moves from **last** (serde) to
/// **first** (protobuf).
///
/// ⚠⚠ `TaggedContinuation` is the message whose SERDE order already cost this
/// campaign "a 95-byte encoding with its halves exchanged" — same length, same
/// byte multiset, invisible to a length check and to a round-trip. A generator
/// that reused the bincode order table for the prost driver would reproduce that
/// defect **in mirror**, on the hottest type in the schema.
///
/// So this test asserts the difference in both directions:
///
/// * `TaggedContinuation` differs, arm for arm;
/// * **no other generated message differs**. Repository-owned `Par` is absent
///   by construction and its independent tag-order mutation proof lives in
///   `protobuf_encoder_differential.rs`.
#[test]
fn exactly_one_generated_message_orders_differently_under_the_two_formats() {
    let protobuf: BTreeMap<&str, &[ProtobufField]> =
        PROTOBUF_CONFORMANCE_REGISTRY.iter().copied().collect();

    let mut differing: Vec<&str> = Vec::new();
    for (name, _, field_names) in CONFORMANCE_REGISTRY {
        let program = protobuf[name];
        let protobuf_order: Vec<&str> = program.iter().map(|f| f.name).collect();
        if protobuf_order != *field_names {
            differing.push(name);
        }
    }
    differing.sort_unstable();

    assert_eq!(
        differing,
        vec!["TaggedContinuation"],
        "EXACTLY one generated message must order differently under serde and protobuf. Found {:?}.\n\
         \n\
         If this grew, a `.proto` edit introduced another message whose declaration order and \
         tag order disagree — which is fine, but it means any code that assumed \"the orders \
         coincide except for the known case\" is now wrong. If it SHRANK, an emitter has \
         stopped applying its own sort key, and one table is now describing the \
         other format's layout.",
        differing
    );

    // ── the exact order, so "differs" cannot be satisfied by differing wrongly ──
    let tc_serde: Vec<&str> = CONFORMANCE_REGISTRY
        .iter()
        .find(|(n, _, _)| *n == "TaggedContinuation")
        .map(|(_, _, names)| names.to_vec())
        .expect("TaggedContinuation is in the bincode registry");
    assert_eq!(
        tc_serde,
        vec!["guard", "cost_region", "clause_regions", "tagged_cont"],
        "`TaggedContinuation`'s SERDE order puts the plain fields FIRST — prost-build writes \
         every plain field before every oneof, whatever the `.proto` declared"
    );
    let tc_protobuf: Vec<&str> = protobuf["TaggedContinuation"]
        .iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(
        tc_protobuf,
        vec!["tagged_cont", "guard", "cost_region", "clause_regions"],
        "★★ `TaggedContinuation`'s PROTOBUF order puts the ONEOF FIRST: the oneof occupies \
         tags 1-2, `guard`, `cost_region` and `clause_regions` are tags 3-5, and prost places \
         a oneof at the position of its LOWEST tag. This is the message whose serde order \
         already produced a 95-byte encoding with its halves exchanged; here the correct \
         order moves the oneof back to the front."
    );
    assert_eq!(
        protobuf["TaggedContinuation"][0].kind,
        ProtobufKind::Oneof,
        "the field that sorts first must be the ONEOF"
    );
    assert_eq!(protobuf["TaggedContinuation"][0].tag, 1);
    assert_eq!(protobuf["TaggedContinuation"][1].tag, 3);
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
    for (name, program) in PROTOBUF_CONFORMANCE_REGISTRY {
        for field in *program {
            if field.name == "locally_free" {
                assert_eq!(
                    field.kind,
                    ProtobufKind::Bytes,
                    "`{name}.locally_free` is {:?} in the prost table. It must be ordinary \
                     `Bytes`: the eight-zero-bytes normalization is serde-only, and \
                     `bincode_encoder_differential.rs` pins that prost keeps the real value.",
                    field.kind
                );
                seen += 1;
            }
        }
    }
    assert_eq!(
        seen, 11,
        "the generated schema has 11 `locally_free` fields; external PathMap-backed `Par` owns \
         the twelfth protobuf field independently, so the generated table carries {seen}"
    );

    let original = models::rhoapi::Par::default().with_locally_free(vec![0xA5, 0x5A, 0x01]);
    let encoded = original.encode_to_vec();
    assert!(
        encoded
            .windows(5)
            .any(|window| window == [0x4A, 0x03, 0xA5, 0x5A, 0x01]),
        "external Par did not emit locally_free as ordinary protobuf field 9: {encoded:02x?}"
    );
    let decoded = models::rhoapi::Par::decode(encoded.as_slice())
        .expect("external Par must decode its own protobuf field 9");
    assert_eq!(decoded.locally_free(), original.locally_free());
}

// ===========================================================================
// §3  ★★ external `Par` is the generated graph's acyclic boundary
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
                    && adjacency
                        .get(component[0])
                        .is_some_and(|k| k.contains(&component[0]));
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

/// The `.rhoapi.Par` descriptor remains a node so generated fields can name the
/// boundary, but its child row is empty because the implementation and its
/// recursive payload traversal live in `models/src/rust/par.rs`. The generated
/// graph must therefore be acyclic in its entirety; an SCC here would be a new
/// recursive family that bypasses the external stack-safe boundary.
#[test]
fn external_par_breaks_every_generated_schema_cycle() {
    let all: BTreeSet<&str> = SCHEMA_CHILDREN.iter().map(|(n, _)| *n).collect();
    assert!(all.contains("Par"), "the child relation must contain `Par`");
    let par_children = SCHEMA_CHILDREN
        .iter()
        .find(|(name, _)| *name == "Par")
        .map(|(_, children)| *children)
        .expect("external Par must have a child-relation row");
    assert!(
        par_children.is_empty(),
        "external Par's generated boundary row must be childless, found {par_children:?}"
    );
    assert!(
        SCHEMA_CHILDREN
            .iter()
            .any(|(name, children)| *name != "Par" && children.contains(&"Par")),
        "no generated field reaches external Par, so this boundary check would be vacuous"
    );

    let whole = cyclic_components(&all);
    assert!(
        whole.is_empty(),
        "external Par failed to break {} generated cyclic component(s): {whole:?}",
        whole.len()
    );
    assert!(
        RECURSIVE_TYPES.is_empty(),
        "RECURSIVE_TYPES must be the empty flattening of the generated cyclic SCCs; found \
         {RECURSIVE_TYPES:?}"
    );
}

/// How much DERIVED recursion runs before the iterative teardown is reached.
///
/// A generated field walk starting at a bare `Expr` can descend through the
/// finite generated type graph until it reaches external `Par`, at which point
/// the PathMap-owned implementation takes over. The depth of that prelude is a
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
fn the_generated_prelude_before_reaching_external_par_is_bounded_by_the_schema() {
    let adjacency: BTreeMap<&str, &[&str]> = SCHEMA_CHILDREN.iter().copied().collect();

    // Reverse-topological order over the `Par`-free subgraph, which
    // `external_par_breaks_every_generated_schema_cycle` proves is a DAG.
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
                         not the DAG `external_par_breaks_every_generated_schema_cycle` proves it is."
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
         This is the native generated prelude before an operation reaches the external \
         PathMap-backed `Par`. It is bounded by the SCHEMA \
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
    let types: BTreeSet<&str> = DERIVE_DISPOSITION_REGISTRY
        .iter()
        .map(|(t, _, _)| *t)
        .collect();
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

/// The surfaces already converted, and the ones still owed a driver.
///
/// ★ Asserted by DISPOSITION rather than by name, so a stage that lands a driver
/// moves this test by changing the table — not by editing an expectation.
///
/// ⚠ **It is a RATCHET, and it fired.** Landing stage F-4 made this test go RED
/// with `Clone::clone` under "converted", because the list is deliberately
/// closed: a surface that becomes `Converted` without anybody saying so is
/// exactly what this refuses. The list grows by one, with the driver named,
/// which is the intended cost of landing a conversion.
#[test]
fn every_recursive_derive_surface_has_a_driver_or_a_bounded_cut_set_path() {
    let mut converted: BTreeSet<&str> = BTreeSet::new();
    for (_, surface, disposition) in DERIVE_DISPOSITION_REGISTRY {
        if matches!(disposition, Disposition::Converted(_)) {
            converted.insert(surface);
        }
    }
    assert_eq!(
        converted.iter().copied().collect::<Vec<_>>(),
        vec!["Deserialize::deserialize", "Serialize::serialize"]
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>(),
        "only the two whole-value bincode surfaces remain converted in the generated acyclic \
         schema; recursive protobuf and term operations now terminate at external Par"
    );
}

/// ★★ **The `Clone::clone` disposition is PER ITEM, and the split is DERIVED.**
///
/// One surface, three dispositions, and none of them is a list somebody keeps in
/// step:
///
/// | item | disposition | derived from |
/// |---|---|---|
/// | not in `EMITTED_TRAVERSALS` | `NotATraversal` | prost derives `Copy`; `Clone` is `*self` |
/// | in `CLONE_CUT_SET` | `Converted` | the feedback vertex set the emitter computed |
/// | emitted, reaches a non-empty cut set | `FollowsFrom` | a field-wise prelude to a driven clone |
/// | emitted in the current acyclic graph | `Bounded` | a field-wise walk bounded by the generated DAG |
///
/// ⚠ A uniform `Converted("term_ops::clone")` would be a **false claim about 61
/// of the 62 generated items** — which is what the row said before
/// `refine_disposition` existed. This test is the reason it cannot say that
/// again: the registry is checked against the two tables the *emitter* published,
/// so "which items were converted" is answered once, by the generator, and
/// consumed twice.
#[test]
fn the_clone_disposition_agrees_with_what_the_emitter_actually_did() {
    use models::rust::rholang::term_ops::{
        CLONE_CUT_SET, CLONE_DESCEND_SET, CLONE_EXTERN_BOUNDED, EMITTED_TRAVERSALS,
    };

    let emitted: BTreeMap<&str, &str> = EMITTED_TRAVERSALS.iter().copied().collect();
    let cut: BTreeSet<&str> = CLONE_CUT_SET.iter().copied().collect();

    assert!(
        cut.is_empty(),
        "external Par must make the generated clone graph acyclic; cut set is {cut:?}"
    );
    assert!(
        CLONE_DESCEND_SET.is_empty(),
        "an empty clone cut set cannot have driven descend nodes: {CLONE_DESCEND_SET:?}"
    );
    let extern_bounded: BTreeSet<&str> = CLONE_EXTERN_BOUNDED.iter().copied().collect();
    assert_eq!(
        extern_bounded,
        BTreeSet::from(["EPathMap", "Par"]),
        "the acyclic proof is only sound while both external recursive payload types remain \
         explicit O(1) CoW-clone obligations"
    );
    assert!(
        !emitted.is_empty(),
        "`EMITTED_TRAVERSALS` is empty — see the non-vacuity floors in \
         `bincode_schema_tables::generate`."
    );

    let mut rows = 0usize;
    let mut converted = 0usize;
    let mut bounded = 0usize;
    let mut not_a_traversal = 0usize;
    for (item, surface, disposition) in DERIVE_DISPOSITION_REGISTRY {
        if *surface != "Clone::clone" {
            continue;
        }
        rows += 1;
        let shape = emitted.get(item).copied();
        match (shape, cut.contains(item)) {
            // A cut-set member: DRIVEN, and the emitter must agree on the shape.
            (Some("driven"), true) => {
                assert!(
                    matches!(disposition, Disposition::Converted(_)),
                    "`{item}` is in `CLONE_CUT_SET` and `EMITTED_TRAVERSALS` calls its body \
                     `driven`, so its `Clone::clone` disposition must be `Converted`; it is \
                     {disposition:?}"
                );
                converted += 1;
            }
            // With external Par the generated graph itself is a DAG, so every
            // emitted field-wise clone has a schema-bounded native prelude.
            (Some("field-wise"), false) => {
                assert!(
                    matches!(disposition, Disposition::Bounded(_)),
                    "`{item}`'s `Clone` is emitted FIELD-WISE over an acyclic generated graph, \
                     so it must be `Bounded`, not {disposition:?}"
                );
                bounded += 1;
            }
            // Not emitted at all: prost derived `Copy`, so `Clone` is `*self`.
            (None, false) => {
                assert!(
                    matches!(disposition, Disposition::NotATraversal(_)),
                    "`{item}` has no emitted `impl Clone`, so it kept its derive — which means \
                     prost derived `Copy` for it and its `Clone` is a bitwise copy. That is \
                     `NotATraversal`, not {disposition:?}."
                );
                not_a_traversal += 1;
            }
            (shape, in_cut) => panic!(
                "`{item}` is in an impossible state: `EMITTED_TRAVERSALS` says {shape:?} and \
                 `CLONE_CUT_SET` membership is {in_cut}. A cut-set member must be `driven`, a \
                 non-member must be `field-wise` or absent, and an absent item must not be in \
                 the cut set. This is the emitter's own two tables disagreeing with each other."
            ),
        }
    }

    assert_eq!(
        rows,
        DERIVE_DISPOSITION_REGISTRY
            .iter()
            .map(|(item, _, _)| *item)
            .collect::<BTreeSet<_>>()
            .len(),
        "every generated item must carry exactly one `Clone::clone` row — the derive is \
         blanket (`.message_attribute` / `.enum_attribute` in `models/build.rs`), so an item \
         without one would be an item this check silently skipped"
    );
    assert_eq!(
        converted,
        cut.len(),
        "every cut-set member must have a `Converted` row: {} converted rows against {} cut-set \
         members",
        converted,
        cut.len()
    );
    assert_eq!(
        bounded,
        emitted.len(),
        "every emitted field-wise `impl Clone` must be dispositioned `Bounded`: {} against {} \
         emitted",
        bounded,
        emitted.len()
    );
    assert!(
        not_a_traversal > 0,
        "NO item was dispositioned `NotATraversal`, which would mean prost derived `Copy` for \
         nothing — but `Var`, `EVar`, `VarRef`, `PCost`, `GSysAuthToken`, `Var.WildcardMsg` and \
         `VarInstance` are all scalar-only. Zero here means the `Copy` reproduction in \
         `bincode_schema_tables.rs` §4b has stopped agreeing with prost, and the three-way split has \
         silently collapsed into two."
    );
    println!(
        "  Clone::clone — {converted} Converted (the cut set), {bounded} Bounded \
         (field-wise), {not_a_traversal} NotATraversal (`Copy`); {rows} rows"
    );
}

#[test]
fn the_ord_disposition_agrees_with_the_schema_cut_set() {
    use models::rust::rholang::term_ops::{CLONE_CUT_SET, EMITTED_TRAVERSALS};

    let cut: BTreeSet<&str> = CLONE_CUT_SET.iter().copied().collect();
    let non_copy: BTreeSet<&str> = EMITTED_TRAVERSALS.iter().map(|(item, _)| *item).collect();
    let mut rows = 0usize;
    let mut converted = 0usize;
    for (item, surface, disposition) in DERIVE_DISPOSITION_REGISTRY {
        if *surface != "Ord::cmp" {
            continue;
        }
        rows += 1;
        if cut.contains(item) {
            assert!(
                matches!(disposition, Disposition::Converted(_)),
                "`{item}` is in the schema feedback vertex set, so its Ord implementation must \
                 be the generated PDA; found {disposition:?}"
            );
            converted += 1;
        } else if non_copy.contains(item) {
            assert!(
                matches!(disposition, Disposition::Bounded(_)),
                "non-Copy `{item}` retains field-wise ordering bounded by the acyclic generated \
                 graph; found {disposition:?}"
            );
        } else {
            assert!(
                matches!(disposition, Disposition::NotATraversal(_)),
                "scalar-only Copy item `{item}` has no recursive ordering walk; found \
                 {disposition:?}"
            );
        }
    }
    assert_eq!(converted, cut.len());
    assert_eq!(
        rows,
        DERIVE_DISPOSITION_REGISTRY
            .iter()
            .map(|(item, _, _)| *item)
            .collect::<BTreeSet<_>>()
            .len(),
        "every generated item must have exactly one Ord::cmp disposition"
    );
}

#[test]
fn the_debug_disposition_agrees_with_the_schema_cut_set() {
    use models::rust::rholang::term_ops::{CLONE_CUT_SET, EMITTED_TRAVERSALS};

    let cut: BTreeSet<&str> = CLONE_CUT_SET.iter().copied().collect();
    let non_copy: BTreeSet<&str> = EMITTED_TRAVERSALS.iter().map(|(item, _)| *item).collect();
    let mut rows = 0usize;
    let mut converted = 0usize;
    for (item, surface, disposition) in DERIVE_DISPOSITION_REGISTRY {
        if *surface != "Debug::fmt" {
            continue;
        }
        rows += 1;
        if cut.contains(item) {
            assert!(
                matches!(disposition, Disposition::Converted(_)),
                "`{item}` is in the schema feedback vertex set, so its Debug implementation \
                 must be the generated PDA; found {disposition:?}"
            );
            converted += 1;
        } else if non_copy.contains(item) {
            assert!(
                matches!(disposition, Disposition::Bounded(_)),
                "non-Copy `{item}` retains field-wise Debug bounded by the acyclic generated \
                 graph; found {disposition:?}"
            );
        } else {
            assert!(
                matches!(disposition, Disposition::NotATraversal(_)),
                "scalar-only Copy item `{item}` has no recursive Debug walk; found \
                 {disposition:?}"
            );
        }
    }
    assert_eq!(converted, cut.len());
    assert_eq!(
        rows,
        DERIVE_DISPOSITION_REGISTRY
            .iter()
            .map(|(item, _, _)| *item)
            .collect::<BTreeSet<_>>()
            .len(),
        "every generated item must have exactly one Debug::fmt disposition"
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
/// `HAND_WRITTEN_TRAVERSALS`. External PathMap-backed `Par` no longer has any
/// native recursive rhoapi walk: equality, hashing, clone, and drop terminate at
/// the compressed-trie boundary.
#[test]
fn no_obsolete_generated_par_walk_remains_in_the_hand_written_registry() {
    let surfaces: BTreeSet<&str> = DERIVE_DISPOSITION_REGISTRY
        .iter()
        .map(|(_, s, _)| *s)
        .collect();
    let hand: BTreeSet<&str> = HAND_WRITTEN_TRAVERSALS.iter().map(|(s, _)| *s).collect();

    assert!(
        hand.is_empty(),
        "external PathMap-backed Par removed the generated recursive family, so stale \
         hand-written traversal claims remain: {hand:?}"
    );
    for surface in ["PartialEq::eq", "Hash::hash", "Drop::drop"] {
        assert!(!surfaces.contains(surface));
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
    for token in [
        "Clone",
        "Ord",
        "PartialOrd",
        "::prost::Message",
        "::prost::Oneof",
    ] {
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
