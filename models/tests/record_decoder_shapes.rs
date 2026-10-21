//! # DR-131 P2b: the wire shapes of the cold-store decode machine, pinned in bytes
//!
//! Ported from `feature/f1r3lang-mettail-only`
//! (`e5102e945:models/tests/bincode_decoder_shapes.rs`) to this branch's
//! machine, `models::rust::rholang::record_decoder`.
//!
//! The differential (`record_decoder_differential.rs`) shows that the machine
//! and the derived decoder agree. That is the property that matters, but it is
//! a black-box statement: it would pass if both sides were wrong in the same
//! way. This file states in bytes what the wire holds at each place where a
//! hand-written decoder drifts, so that a reader can check each claim against
//! the source of bincode.
//!
//! It also holds the pins of every variant index of the machine and the type
//! partition of the schema.
//!
//! ## Changes from the F1R3Lang file
//!
//! - Shape 3 is the plain `EPathMap` of this branch: four derived fields, the
//!   first a sequence of `Par`. There is no trie snapshot.
//! - `Par` has 13 fields here: the hand-built encodings end with
//!   `cost_signed_terms` and `cost_stacks`.
//! - The type partition is checked against the names in the generated schema
//!   (`$OUT_DIR/rhoapi.rs`), so a new type fails the test until it is
//!   classified.
//! - New pins: the indices of `ConnectiveInstance`, `CostSignature.Value`,
//!   `VarInstance` and `UnfInstance`, the 13 fields of `Par`, and the numeric
//!   arms whose values the derived decoder does not range-check.

use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::{VarInstance, WildcardMsg};
use models::rhoapi::{
    Connective, CostSignature, CostStack, EList, EPathMap, ETuple, EZipper, Expr, GUnforgeable,
    ListParWithRandom, New, Par, ParWithRandom, TaggedContinuation, Var,
};
use models::rust::rholang::dismantle::dismantle_term;
use models::rust::rholang::record_decoder::{
    connective_instance_variant_index, cost_value_variant_index, expr_instance_variant_index,
    CONNECTIVE_INSTANCE_VARIANT_COUNT, COST_VALUE_VARIANT_COUNT, EXPR_INSTANCE_VARIANT_COUNT,
    TAGGED_CONT_VARIANT_COUNT, UNF_INSTANCE_VARIANT_COUNT, VAR_INSTANCE_VARIANT_COUNT,
};
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;

mod record_corpus;
use record_corpus as corpus;

/// Decodes with both decoders, requires the same value on every field, and
/// returns the machine's value.
fn both(par: &Par) -> Par {
    let bytes = bincode::serialize(par).expect("serialize");
    let oracle: Par = bincode::deserialize(&bytes).expect("oracle");
    let machine = Par::cold_decode(&bytes).expect("machine");
    assert!(
        machine == oracle && format!("{machine:?}") == format!("{oracle:?}"),
        "the machine and the derived oracle disagree"
    );
    machine
}

/// The variant index that bincode writes after the `Some` tag of a oneof.
fn written_index(bytes: &[u8], what: &str) -> u32 {
    assert_eq!(bytes[0], 1, "{what}: Some(..) is tag 1");
    u32::from_le_bytes(bytes[1..5].try_into().expect("4 bytes"))
}

/// The encoding of an empty sequence: a zero `u64` count.
const EMPTY_SEQUENCE: [u8; 8] = [0; 8];

/// The encoding of a `Par`, field by field in declaration order. `news` and
/// `exprs` are the encodings of those two sequences. Every other sequence is
/// empty.
fn hand_built_par(
    news: &[u8],
    exprs: &[u8],
    locally_free: &[u8],
    connective_used: bool,
) -> Vec<u8> {
    let empty = EMPTY_SEQUENCE;
    let mut out =
        Vec::with_capacity(10 * 8 + news.len() + exprs.len() + 8 + locally_free.len() + 1);
    out.extend_from_slice(&empty); // sends
    out.extend_from_slice(&empty); // receives
    out.extend_from_slice(news); // news
    out.extend_from_slice(exprs); // exprs
    out.extend_from_slice(&empty); // matches
    out.extend_from_slice(&empty); // unforgeables
    out.extend_from_slice(&empty); // bundles
    out.extend_from_slice(&empty); // connectives
    out.extend_from_slice(&empty); // conditionals
    out.extend_from_slice(&(locally_free.len() as u64).to_le_bytes()); // locally_free
    out.extend_from_slice(locally_free);
    out.push(u8::from(connective_used)); // connective_used
    out.extend_from_slice(&empty); // cost_signed_terms
    out.extend_from_slice(&empty); // cost_stacks
    out
}

/// `Par` has 13 serde fields, in the order of [`hand_built_par`].
#[test]
fn par_is_thirteen_fields_in_declaration_order() {
    let empty = bincode::serialize(&Par::default()).expect("serialize the empty Par");
    assert_eq!(empty.len(), 12 * 8 + 1, "twelve sequences and one bool");
    assert_eq!(
        empty,
        hand_built_par(&EMPTY_SEQUENCE, &EMPTY_SEQUENCE, &[], false)
    );
    let both_flags = Par {
        connective_used: true,
        ..Default::default()
    };
    assert_eq!(
        bincode::serialize(&both_flags).expect("serialize"),
        hand_built_par(&EMPTY_SEQUENCE, &EMPTY_SEQUENCE, &[], true),
        "connective_used is the eleventh field"
    );
}

// ===========================================================================
// SHAPE 1: `New.injections` is a `BTreeMap<String, Par>`
// ===========================================================================

/// The only map field of the schema. serde writes a map, not a sequence:
/// bincode's `deserialize_map` reads a `u64` count and then that many key and
/// value pairs. A decoder that read a sequence would try to read a `Par`
/// where a `String` key sits. The keys are interleaved with values of any
/// depth, which is why the machine's `Op::NewEntry` is a resume point.
#[test]
fn shape_1_injections_are_a_map_of_key_value_pairs() {
    let mut injections = BTreeMap::new();
    injections.insert("alpha".to_string(), corpus::gint(1));
    injections.insert("beta".to_string(), corpus::deep_par(8));

    let par = Par {
        news: vec![New {
            bind_count: 0,
            p: None,
            uri: vec![],
            injections: injections.clone(),
            locally_free: vec![],
        }],
        ..Default::default()
    };

    let map_bytes = bincode::serialize(&injections).expect("serialize map");
    assert_eq!(
        u64::from_le_bytes(map_bytes[..8].try_into().expect("8 bytes")),
        2,
        "shape 1: the wire form of a BTreeMap starts with a u64 ENTRY COUNT"
    );
    assert_eq!(
        u64::from_le_bytes(map_bytes[8..16].try_into().expect("8 bytes")),
        "alpha".len() as u64,
        "shape 1: the count is followed by the first KEY, with its length first"
    );

    let decoded = both(&par);
    assert_eq!(decoded.news[0].injections, injections);
}

/// serde's map visitor inserts each pair in stream order. So a duplicate key
/// overwrites, and the stream need not be sorted. A decoder that assumed
/// sorted keys, or rejected duplicates, would refuse bytes that the derived
/// decoder accepts. The encoder cannot write such a stream, so the bytes are
/// built by hand: stored rows can come from other nodes.
#[test]
fn shape_1_duplicate_and_unsorted_map_keys_behave_like_btreemap_insert() {
    // One `New` whose map has three entries, in DESCENDING key order, with
    // one duplicate: "b" -> 1, "a" -> 2, "b" -> 3. `BTreeMap::insert` gives
    // { "a": 2, "b": 3 }.
    let mut news: Vec<u8> = Vec::new();
    news.extend_from_slice(&1u64.to_le_bytes()); // news: one New
    news.extend_from_slice(&0i32.to_le_bytes()); // New.bind_count
    news.push(0); // New.p = None
    news.extend_from_slice(&0u64.to_le_bytes()); // New.uri = []
    news.extend_from_slice(&3u64.to_le_bytes()); // New.injections: 3 entries
    for (key, value) in [("b", 1i64), ("a", 2), ("b", 3)] {
        news.extend_from_slice(&(key.len() as u64).to_le_bytes());
        news.extend_from_slice(key.as_bytes());
        news.extend_from_slice(&bincode::serialize(&corpus::gint(value)).expect("gint"));
    }
    news.extend_from_slice(&0u64.to_le_bytes()); // New.locally_free = []
    let par_bytes = hand_built_par(&news, &EMPTY_SEQUENCE, &[], false);

    let oracle: Par = bincode::deserialize(&par_bytes)
        .expect("the derived decoder must ACCEPT an unsorted map with duplicates");
    let machine = Par::cold_decode(&par_bytes).expect("the machine must accept it too");
    assert!(machine == oracle);

    let injections = &machine.news[0].injections;
    assert_eq!(injections.len(), 2, "the duplicate key must collapse");
    assert_eq!(injections["a"], corpus::gint(2));
    assert_eq!(
        injections["b"],
        corpus::gint(3),
        "the LAST value of a duplicate key wins, as `BTreeMap::insert` does"
    );
}

// ===========================================================================
// SHAPE 2: the 13 `serialize_as_empty_bytes` fields
// ===========================================================================

/// `locally_free` is written as `serialize_bytes(&[])`, eight zero bytes, and
/// read as a plain `Vec<u8>` (`models/src/rust/serde_helpers.rs`). Thirteen
/// structs have the attribute: `Par`, `Send`, `Receive`, `New`, `Match`, `If`,
/// `EList`, `ETuple`, `ESet`, `EMap`, `EPathMap`, `EZipper` and `EMethod`.
///
/// Two consequences:
///
/// 1. A round trip loses `locally_free`, for the derived decoder too. A
///    decoder that "fixed" this would change what every node stores.
/// 2. The decoder must read the real length from the stream. The encoder
///    writes only zero, but a stored row need not come from this encoder, and
///    a decoder that skipped eight bytes would lose its place on a row with a
///    non-empty field.
#[test]
fn shape_2_locally_free_is_written_empty_and_read_as_a_real_sequence() {
    let par = corpus::all_par_fields();
    assert!(
        !par.locally_free.is_empty(),
        "the fixture must hold a non-empty locally_free"
    );

    let decoded = both(&par);

    // Consequence 1: every one of the 13 kinds of field comes back empty.
    let debug = format!("{decoded:?}");
    let fields = debug.matches("locally_free: [").count();
    let empty_fields = debug.matches("locally_free: []").count();
    assert!(
        fields >= 13,
        "the fixture reaches each of the 13 structs with the attribute"
    );
    assert_eq!(
        fields, empty_fields,
        "shape 2: `serialize_as_empty_bytes` blanks every locally_free on the way OUT"
    );
    assert!(decoded.locally_free.is_empty());
    assert!(decoded.sends[0].locally_free.is_empty());
    assert!(decoded.receives[0].locally_free.is_empty());
    assert!(decoded.news[0].locally_free.is_empty());
    assert!(decoded.matches[0].locally_free.is_empty());
    assert!(decoded.conditionals[0].locally_free.is_empty());

    // Consequence 2: a stream whose outer `locally_free` is NOT empty. Both
    // decoders read it back exactly.
    let hand = hand_built_par(&EMPTY_SEQUENCE, &EMPTY_SEQUENCE, &[0xAA, 0xBB, 0xCC], true);
    let oracle: Par = bincode::deserialize(&hand).expect("the oracle reads real bytes");
    let machine = Par::cold_decode(&hand).expect("the machine reads real bytes");
    assert!(machine == oracle);
    assert_eq!(oracle.locally_free, vec![0xAA, 0xBB, 0xCC]);
    assert_eq!(
        machine.locally_free,
        vec![0xAA, 0xBB, 0xCC],
        "shape 2: the DECODER must read the real length; the blanking is on the way out only"
    );
    assert!(machine.connective_used);
}

/// `ETuple` has NO `remainder` field, unlike `EList`, `ESet` and `EMap`. One
/// field off, and every tuple in a term moves the stream. A random generator
/// never produces an `ETuple`, so only the constructed corpus catches this.
#[test]
fn shape_2_etuple_has_no_remainder_field() {
    let tuple = ETuple {
        ps: vec![corpus::gint(1), corpus::gint(2)],
        locally_free: vec![9],
        connective_used: true,
    };
    let list = EList {
        ps: vec![corpus::gint(1), corpus::gint(2)],
        locally_free: vec![9],
        connective_used: true,
        remainder: None,
    };
    let tuple_len = bincode::serialize(&tuple).expect("tuple").len();
    let list_len = bincode::serialize(&list).expect("list").len();
    assert_eq!(
        list_len - tuple_len,
        1,
        "EList has exactly one more wire byte than ETuple: the `remainder` Option tag. If \
         this is not 1, the split between ExprTupleBuild and ExprSeqBuild is wrong."
    );
    both(&corpus::par_of(ExprInstance::ETupleBody(tuple)));
}

// ===========================================================================
// SHAPE 3: `EPathMap` is four derived fields, the first a sequence of `Par`
// ===========================================================================

/// This branch's `EPathMap` is the plain generated message, with derived
/// serde: `ps`, `locally_free`, `connective_used`, `remainder`. So its wire
/// form is the concatenation of the four fields, and the machine reads `ps`
/// as a counted sequence of `Par`.
#[test]
fn shape_3_epathmap_is_four_derived_fields() {
    let map = EPathMap {
        ps: vec![corpus::gint(1), corpus::gint(2)],
        locally_free: vec![0xAA],
        connective_used: true,
        remainder: None,
    };
    let bytes = bincode::serialize(&map).expect("serialize");

    let mut expected = Vec::new();
    expected.extend_from_slice(&2u64.to_le_bytes()); // ps: two elements
    expected.extend_from_slice(&bincode::serialize(&corpus::gint(1)).expect("gint"));
    expected.extend_from_slice(&bincode::serialize(&corpus::gint(2)).expect("gint"));
    expected.extend_from_slice(&0u64.to_le_bytes()); // locally_free: BLANKED
    expected.push(1); // connective_used
    expected.push(0); // remainder: None
    assert_eq!(
        bytes, expected,
        "shape 3: the wire form of EPathMap is exactly its four fields"
    );

    // The derived decoder reads a real locally_free, and so must the machine.
    let mut hand = 0u64.to_le_bytes().to_vec(); // ps: none
    hand.extend_from_slice(&2u64.to_le_bytes());
    hand.extend_from_slice(&[0x01, 0x02]); // locally_free: two bytes
    hand.push(0); // connective_used
    hand.push(0); // remainder
    let oracle: EPathMap = bincode::deserialize(&hand).expect("oracle");
    assert_eq!(
        oracle.locally_free,
        vec![0x01, 0x02],
        "shape 3: the derived decoder reads the real locally_free"
    );
    let mut expr = Vec::with_capacity(5 + hand.len());
    expr.push(1); // Some(expr_instance)
    expr.extend_from_slice(&25u32.to_le_bytes()); // EPathmapBody
    expr.extend_from_slice(&hand);
    let mut exprs = 1u64.to_le_bytes().to_vec(); // exprs: one Expr
    exprs.extend_from_slice(&expr);
    let par_bytes = hand_built_par(&EMPTY_SEQUENCE, &exprs, &[], false);
    let oracle: Par = bincode::deserialize(&par_bytes).expect("oracle Par");
    let machine = Par::cold_decode(&par_bytes).expect("machine Par");
    assert!(machine == oracle && format!("{machine:?}") == format!("{oracle:?}"));
    match machine.exprs[0].expr_instance.as_ref() {
        Some(ExprInstance::EPathmapBody(decoded)) => {
            assert_eq!(decoded.locally_free, vec![0x01, 0x02])
        }
        other => panic!("expected EPathmapBody, got {other:?}"),
    }

    both(&corpus::par_of(ExprInstance::EPathmapBody(map)));
}

/// `EZipper.pathmap` is an `Option<EPathMap>`: one tag byte, then the four
/// fields. The direct `EPathmapBody` arm has no such tag.
#[test]
fn shape_3_ezipper_wraps_the_pathmap_in_an_option() {
    let inner = EPathMap {
        ps: vec![corpus::gint(5)],
        ..Default::default()
    };
    let bare = bincode::serialize(&inner).expect("bare");
    let wrapped = bincode::serialize(&Some(inner.clone())).expect("wrapped");
    assert_eq!(
        wrapped.len(),
        bare.len() + 1,
        "shape 3: Option<EPathMap> costs exactly one tag byte more than EPathMap"
    );
    assert_eq!(wrapped[0], 1, "Some(..) is tag 1");

    both(&corpus::par_of(ExprInstance::EZipperBody(EZipper {
        pathmap: Some(inner),
        current_path: vec![vec![1, 2], vec![]],
        is_write_zipper: true,
        locally_free: vec![7],
        connective_used: true,
    })));
    both(&corpus::par_of(ExprInstance::EZipperBody(EZipper {
        pathmap: None,
        current_path: vec![],
        is_write_zipper: false,
        locally_free: vec![],
        connective_used: false,
    })));
}

// ===========================================================================
// SHAPE 4: a oneof is an `Option` tag (1 byte), then a variant index (4 bytes)
// ===========================================================================

/// A `Some(oneof)` is five bytes of framing: the `Option` tag (one byte) and
/// the variant index (a fixint `u32`). The index is serde's declaration
/// order, not the proto tag: `EPathmapBody` has proto tag 32 and serde index
/// 25, so a decoder that read proto tags would decode 12 arms wrongly.
#[test]
fn shape_4_oneofs_are_an_option_tag_then_a_u32_variant_index() {
    for (name, instance) in corpus::every_expr_instance() {
        let expected = expr_instance_variant_index(&instance);
        let bytes = bincode::serialize(&Expr {
            expr_instance: Some(instance),
        })
        .expect("serialize");
        assert_eq!(
            written_index(&bytes, name),
            expected,
            "{name}: the variant index is a u32 right after the Option tag"
        );
    }

    let absent = bincode::serialize(&Expr {
        expr_instance: None,
    })
    .expect("serialize");
    assert_eq!(absent, vec![0u8], "an absent oneof is one zero byte");

    assert_eq!(
        expr_instance_variant_index(&ExprInstance::EPathmapBody(corpus::plain_pathmap())),
        25,
        "EPathmapBody is proto tag 32 but serde index 25"
    );
}

/// The machine's dispatch from index to arm is the inverse of the exhaustive
/// index functions, for every index in range. This stops the decoder from
/// becoming an independent enumeration of the schema.
#[test]
fn record_decoder_variant_indices_agree() {
    let mut seen: Vec<u32> = Vec::with_capacity(EXPR_INSTANCE_VARIANT_COUNT as usize);
    for (name, instance) in corpus::every_expr_instance() {
        let index = expr_instance_variant_index(&instance);
        let decoded = both(&corpus::par_of(instance));
        let round_tripped = decoded.exprs[0]
            .expr_instance
            .as_ref()
            .map(expr_instance_variant_index)
            .expect("a decoded Expr holds an instance");
        assert_eq!(
            round_tripped, index,
            "{name}: the machine decoded wire index {index} to an arm whose index is \
             {round_tripped}"
        );
        seen.push(index);
    }
    seen.sort_unstable();
    assert_eq!(
        seen,
        (0..EXPR_INSTANCE_VARIANT_COUNT).collect::<Vec<u32>>(),
        "the machine was not run on every ExprInstance index"
    );

    let mut seen: Vec<u32> = Vec::with_capacity(CONNECTIVE_INSTANCE_VARIANT_COUNT as usize);
    for (name, instance) in corpus::every_connective_instance() {
        let index = connective_instance_variant_index(&instance);
        let decoded = both(&Par {
            connectives: vec![Connective {
                connective_instance: Some(instance),
            }],
            ..Default::default()
        });
        let round_tripped = decoded.connectives[0]
            .connective_instance
            .as_ref()
            .map(connective_instance_variant_index)
            .expect("a decoded Connective holds an instance");
        assert_eq!(round_tripped, index, "{name}: index mismatch");
        seen.push(index);
    }
    seen.sort_unstable();
    assert_eq!(
        seen,
        (0..CONNECTIVE_INSTANCE_VARIANT_COUNT).collect::<Vec<u32>>(),
        "the machine was not run on every ConnectiveInstance index"
    );

    let mut seen: Vec<u32> = Vec::with_capacity(COST_VALUE_VARIANT_COUNT as usize);
    for (name, signature) in corpus::every_cost_signature() {
        let Some(value) = signature.value.as_ref() else {
            continue;
        };
        let index = cost_value_variant_index(value);
        let decoded = both(&Par {
            cost_stacks: vec![CostStack {
                cells: vec![signature.clone()],
            }],
            ..Default::default()
        });
        let round_tripped = decoded.cost_stacks[0].cells[0]
            .value
            .as_ref()
            .map(cost_value_variant_index)
            .expect("a decoded signature holds a value");
        assert_eq!(round_tripped, index, "{name}: index mismatch");
        seen.push(index);
    }
    seen.sort_unstable();
    assert_eq!(
        seen,
        (0..COST_VALUE_VARIANT_COUNT).collect::<Vec<u32>>(),
        "the machine was not run on every cost-signature index"
    );
}

/// The indices of the other oneofs, against the bytes that bincode writes.
/// The machine reads `VarInstance`, `UnfInstance` and `TaggedCont` with the
/// literal indices of these tables.
#[test]
fn the_variant_indices_of_every_oneof_are_pinned() {
    for (name, instance) in corpus::every_connective_instance() {
        let index = connective_instance_variant_index(&instance);
        let bytes = bincode::serialize(&Connective {
            connective_instance: Some(instance),
        })
        .expect("serialize");
        assert_eq!(written_index(&bytes, name), index, "{name}");
    }
    for (name, signature) in corpus::every_cost_signature() {
        let Some(value) = signature.value.as_ref() else {
            continue;
        };
        let bytes = bincode::serialize(&signature).expect("serialize");
        assert_eq!(
            written_index(&bytes, name),
            cost_value_variant_index(value),
            "{name}"
        );
    }

    let vars = [
        (0u32, VarInstance::BoundVar(5)),
        (1, VarInstance::FreeVar(6)),
        (2, VarInstance::Wildcard(WildcardMsg {})),
    ];
    assert_eq!(vars.len() as u32, VAR_INSTANCE_VARIANT_COUNT);
    for (index, instance) in vars {
        let bytes = bincode::serialize(&Var {
            var_instance: Some(instance),
        })
        .expect("serialize");
        assert_eq!(written_index(&bytes, "VarInstance"), index);
    }

    let unforgeables: Vec<(u32, GUnforgeable)> = corpus::every_unforgeable()
        .into_iter()
        .filter(|u| u.unf_instance.is_some())
        .enumerate()
        .map(|(index, u)| (index as u32, u))
        .collect();
    assert_eq!(unforgeables.len() as u32, UNF_INSTANCE_VARIANT_COUNT);
    for (index, unforgeable) in unforgeables {
        let name = match unforgeable.unf_instance.as_ref() {
            Some(UnfInstance::GPrivateBody(_)) => "GPrivateBody",
            Some(UnfInstance::GDeployIdBody(_)) => "GDeployIdBody",
            Some(UnfInstance::GDeployerIdBody(_)) => "GDeployerIdBody",
            Some(UnfInstance::GSysAuthTokenBody(_)) => "GSysAuthTokenBody",
            Some(UnfInstance::GAuthorityIdBody(_)) => "GAuthorityIdBody",
            Some(UnfInstance::GPrincipalIdBody(_)) => "GPrincipalIdBody",
            None => unreachable!("the absent unforgeable was filtered out"),
        };
        let bytes = bincode::serialize(&unforgeable).expect("serialize");
        assert_eq!(written_index(&bytes, name), index, "{name}");
    }

    for (index, cont) in [
        (
            0u32,
            TaggedCont::ParBody(ParWithRandom {
                body: None,
                random_state: vec![],
            }),
        ),
        (1, TaggedCont::ScalaBodyRef(0)),
    ] {
        let bytes = bincode::serialize(&Some(cont)).expect("serialize");
        assert_eq!(written_index(&bytes, "TaggedCont"), index);
    }
    assert_eq!(TAGGED_CONT_VARIANT_COUNT, 2);
}

/// `TaggedCont` is the oneof whose absent form is easy to confuse with
/// `ScalaBodyRef(0)`.
#[test]
fn shape_4_tagged_cont_absent_is_not_variant_zero() {
    let absent = TaggedContinuation {
        guard: None,
        cost_authority: None,
        tagged_cont: None,
    };
    let par_body = TaggedContinuation {
        guard: None,
        cost_authority: None,
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: None,
            random_state: vec![],
        })),
    };
    let scala = TaggedContinuation {
        guard: None,
        cost_authority: None,
        tagged_cont: Some(TaggedCont::ScalaBodyRef(0)),
    };

    for value in [&absent, &par_body, &scala] {
        let bytes = bincode::serialize(value).expect("serialize");
        let oracle: TaggedContinuation = bincode::deserialize(&bytes).expect("oracle");
        let machine = TaggedContinuation::cold_decode(&bytes).expect("machine");
        assert!(machine == oracle);
        assert!(machine == *value);
    }

    let a = bincode::serialize(&absent).expect("absent");
    assert_eq!(
        a,
        vec![0u8, 0u8, 0u8],
        "guard tag 0, cost_authority tag 0, tagged_cont tag 0"
    );
    let p = bincode::serialize(&par_body).expect("par_body");
    assert_eq!(p[0], 0, "guard tag");
    assert_eq!(p[1], 0, "cost_authority tag");
    assert_eq!(p[2], 1, "tagged_cont Some tag");
    assert_eq!(
        u32::from_le_bytes(p[3..7].try_into().expect("4 bytes")),
        0,
        "ParBody is variant index 0"
    );
    let s = bincode::serialize(&scala).expect("scala");
    assert_eq!(
        u32::from_le_bytes(s[3..7].try_into().expect("4 bytes")),
        1,
        "ScalaBodyRef is variant index 1"
    );
}

/// `GUint16` and `GUint8` hold a `u32` on the wire. The derived decoder does
/// not check the value range, so the machine must not check it either: a
/// value above the range decodes on both sides.
#[test]
fn the_narrow_numeric_arms_are_not_range_checked() {
    for instance in [
        ExprInstance::GUint16(0x0001_0000),
        ExprInstance::GUint8(0x0000_0100),
        ExprInstance::GUint16(u32::MAX),
        ExprInstance::GUint8(u32::MAX),
    ] {
        let decoded = both(&corpus::par_of(instance.clone()));
        assert_eq!(
            decoded.exprs[0].expr_instance,
            Some(instance),
            "a value above the range decodes unchanged"
        );
    }
}

// ===========================================================================
// The type partition
// ===========================================================================

/// The generated serde types of the `rhoapi` schema (72), split into the 54
/// that contain `Par` (directly or through other types) and go on the machine,
/// and the 18 that do not and are read by the bounded leaf readers.
///
/// Changed by G3 (DR-135): `CostCaller` makes 73 types and 55 machine types.
/// Its signature can quote a `Par`, so it goes on the machine (SA3).
///
/// Three guards keep the partition true:
///
/// 1. A new FIELD on a machine type fails to compile: every `*Build` op of
///    `record_decoder.rs` builds its type with a complete struct literal.
/// 2. A new ARM of `ExprInstance`, `ConnectiveInstance` or
///    `CostSignature.Value` fails to compile: the index functions are
///    exhaustive, and the counts are checked against the corpus.
/// 3. A new TYPE fails this test: the union of the two lists must equal the
///    names in the generated schema.
#[test]
fn record_decoder_type_partition() {
    /// Contain `Par`; decoded by the machine.
    // Changed by G3 (DR-135): one more machine type, `CostCaller`.
    // const MACHINE_TYPES: [&str; 54] = [
    const MACHINE_TYPES: [&str; 55] = [
        // the cycle through `Par` (46), the cost family included
        "Par",
        "Send",
        "Receive",
        "ReceiveBind",
        "New",
        "Match",
        "MatchCase",
        "If",
        "Bundle",
        "Expr",
        "ExprInstance",
        "Connective",
        "ConnectiveInstance",
        "ConnectiveBody",
        "EList",
        "ETuple",
        "ESet",
        "EMap",
        "KeyValuePair",
        "EMethod",
        "EPathMap",
        "EZipper",
        "ENot",
        "ENeg",
        "EMult",
        "EDiv",
        "EMod",
        "EPlus",
        "EMinus",
        "ELt",
        "ELte",
        "EGt",
        "EGte",
        "EEq",
        "ENeq",
        "EAnd",
        "EOr",
        "EMatches",
        "EPercentPercent",
        "EPlusPlus",
        "EMinusMinus",
        "CostSignature",
        "Value",
        "CostSignatureCompound",
        "CostSignedTerm",
        "CostStack",
        // contain `Par` but are not in the cycle (8)
        "TaggedContinuation",
        "TaggedCont",
        "ParWithRandom",
        "ListParWithRandom",
        "BindPattern",
        "ListBindPatterns",
        "CostRegion",
        "CostAuthority",
        // Added by G3 (DR-135): the caller of a region (9 types outside the
        // cycle).
        "CostCaller",
    ];

    /// Outside the cycle and without `Par`: a fixed maximum nesting, so a
    /// fixed maximum stack. Read by the bounded leaf readers of the machine.
    const BOUNDED_TYPES: [&str; 18] = [
        "Var",
        "VarInstance",
        "WildcardMsg",
        "VarRef",
        "EVar",
        "GUnforgeable",
        "UnfInstance",
        "GPrivate",
        "GDeployId",
        "GDeployerId",
        "GSysAuthToken",
        "DeployId",
        "DeployerId",
        "PCost",
        "GBigRational",
        "GFixedPoint",
        "GAuthorityId",
        "GPrincipalId",
    ];

    let machine: BTreeSet<&str> = MACHINE_TYPES.iter().copied().collect();
    let bounded: BTreeSet<&str> = BOUNDED_TYPES.iter().copied().collect();
    assert_eq!(
        machine.len(),
        MACHINE_TYPES.len(),
        "a duplicate machine type"
    );
    assert_eq!(
        bounded.len(),
        BOUNDED_TYPES.len(),
        "a duplicate bounded type"
    );
    assert!(
        machine.is_disjoint(&bounded),
        "a type is on both sides of the partition"
    );

    let generated_source = include_str!(concat!(env!("OUT_DIR"), "/rhoapi.rs"));
    let generated: BTreeSet<&str> = generated_source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            line.strip_prefix("pub struct ")
                .or_else(|| line.strip_prefix("pub enum "))
        })
        .map(|rest| {
            rest.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .expect("a type name")
        })
        .collect();
    // Changed by G3 (DR-135): `CostCaller` is the 73rd type.
    // assert_eq!(generated.len(), 72, "the generated schema has 72 types");
    assert_eq!(generated.len(), 73, "the generated schema has 73 types");
    let partition: BTreeSet<&str> = machine.union(&bounded).copied().collect();
    let unclassified: Vec<&&str> = generated.difference(&partition).collect();
    let stale: Vec<&&str> = partition.difference(&generated).collect();
    assert!(
        unclassified.is_empty() && stale.is_empty(),
        "the partition does not match the generated schema: unclassified types \
         {unclassified:?}, types that no longer exist {stale:?}"
    );
}

// ===========================================================================
// The property that the partition supports
// ===========================================================================

/// The depth of the deep terms of this section.
const DEEP_NESTING: usize = 4096;

/// The encoding of `deep_par(DEEP_NESTING)`. The derived encoder recurses once
/// for each level, so it runs on a thread with a large stack.
fn deep_encoding() -> Vec<u8> {
    std::thread::Builder::new()
        .name("deep-encoder".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let term = corpus::deep_par(DEEP_NESTING);
            let bytes = bincode::serialize(&term).expect("serialize");
            dismantle_term(term);
            bytes
        })
        .expect("spawn the encoder")
        .join()
        .expect("the encoder panicked")
}

/// The machine decodes a term far deeper than the derived decoder survives,
/// on a stack far smaller than the derived decoder needs. The derived decoder
/// costs about 28,362 B for each level in debug builds and 12,894 B in
/// release builds (measured on `feature/mettail`,
/// `5108d8543:rspace++/src/rspace/serializers/cold_store_decode.rs:6-12`).
/// At 4,096 levels that is about 110 MiB or 50 MiB. This thread gets 256 KiB.
#[test]
fn machine_decodes_depth_4096_on_a_256_kib_stack() {
    let bytes = deep_encoding();
    assert!(
        bytes.len() > DEEP_NESTING,
        "VACUOUS PROBE: a depth-{DEEP_NESTING} term encoded to only {} bytes",
        bytes.len()
    );

    std::thread::Builder::new()
        .name("deep-decoder".to_string())
        .stack_size(256 * 1024)
        .spawn(move || {
            let decoded = Par::cold_decode(&bytes).expect("the machine decodes depth 4096");
            // The nesting really came back: an empty `Par` would also survive.
            let mut depth = 0usize;
            let mut current = &decoded;
            while let Some(ExprInstance::EListBody(list)) =
                current.exprs.first().and_then(|e| e.expr_instance.as_ref())
            {
                depth += 1;
                match list.ps.first() {
                    Some(next) => current = next,
                    None => break,
                }
            }
            assert_eq!(depth, DEEP_NESTING, "the DECODED term holds the nesting");
            // The derived `Drop` of `Par` recurses once for each level too.
            dismantle_term(decoded);
        })
        .expect("spawn the decoder")
        .join()
        .expect("the machine overflowed a 256 KiB stack at depth 4096");
}

/// The same on the error path. A decode that FAILS at the last byte of a deep
/// term holds a deep partial result, and the derived `Drop` of `Par` recurses
/// once for each level. A machine without an iterative teardown would abort
/// while it REJECTS a hostile input.
#[test]
fn machine_rejects_a_truncated_deep_term_without_overflowing() {
    let bytes = deep_encoding();

    std::thread::Builder::new()
        .name("deep-truncated-decoder".to_string())
        .stack_size(256 * 1024)
        .spawn(move || {
            // Without the final byte, everything parses until the outer
            // `cost_stacks` count. At that point about 4,096 levels are live on
            // the machine's value stacks and must be released without
            // recursion.
            let truncated = &bytes[..bytes.len() - 1];
            let err =
                Par::cold_decode(truncated).expect_err("a truncated encoding must be REJECTED");
            let text = err.to_string();
            assert!(
                text.contains("unexpected end of input"),
                "expected an end-of-input rejection, got: {text}"
            );
        })
        .expect("spawn the decoder")
        .join()
        .expect("the machine overflowed a 256 KiB stack while TEARING DOWN a rejected term");
}

// ===========================================================================
// The prefix contract
// ===========================================================================

/// A data row is `{ a: A, persist: bool, source: Produce }`: `A` is a PREFIX
/// of a longer encoding, and bincode cannot find the end of `A` from outside.
/// So `cold_decode_prefix` must report the exact extent, and the next field
/// must be where the derived decoder puts it.
#[test]
fn prefix_extents_let_the_following_field_be_read() {
    let value = ListParWithRandom {
        pars: vec![corpus::all_par_fields()],
        random_state: vec![1, 2, 3],
        cost_authority: Some(corpus::cost_authority()),
        cost_stack: Some(corpus::cost_stack()),
    };
    let mut bytes = bincode::serialize(&value).expect("serialize");
    let extent = bytes.len();
    // Append what a data row puts after `a`: `persist` and a marker.
    bytes.push(1);
    bytes.extend_from_slice(&[0xAB, 0xCD]);

    let (decoded, consumed) = ListParWithRandom::cold_decode_prefix(&bytes).expect("machine");
    // Compared with the ORACLE, not with `value`: `all_par_fields` holds
    // non-empty `locally_free` fields, which shape 2 blanks on the way out.
    let oracle: ListParWithRandom = bincode::deserialize(&bytes).expect("oracle");
    assert!(decoded == oracle && format!("{decoded:?}") == format!("{oracle:?}"));
    assert_eq!(consumed, extent, "the extent must exclude the tail");
    assert_eq!(
        bytes[consumed], 1,
        "the next field starts exactly at `consumed`"
    );
}

/// The cost signatures of a `ReceiveBind` are part of the cycle through
/// `Par`: a signature can quote a `Par`. The machine reads them with its own
/// ops, so they meet the same differential.
#[test]
fn a_signature_that_quotes_a_par_decodes_as_the_derived_decoder_reads_it() {
    use models::rhoapi::cost_signature::Value as CostValue;

    let quoted = CostSignature {
        value: Some(CostValue::Quote(corpus::all_par_fields())),
    };
    let named = CostSignature {
        value: Some(CostValue::Name(corpus::deep_mixed_par(
            corpus::MIXED_SHAPES * 2,
        ))),
    };
    both(&Par {
        cost_stacks: vec![CostStack {
            cells: vec![quoted, named],
        }],
        ..Default::default()
    });
}
