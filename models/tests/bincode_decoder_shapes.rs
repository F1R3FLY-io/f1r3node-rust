//! # The four bincode shapes, each named and pinned at the byte level
//!
//! The differential (`bincode_decoder_differential.rs`) proves the machine and the
//! derive agree. That is the property that matters, but on its own it is a
//! *black-box* statement: it would still pass if both sides happened to be
//! wrong about the same thing, and it does not tell a reviewer *why* the four
//! hard cases are hard.
//!
//! This file names them. Each test states, in bytes, what the wire actually
//! looks like at the place a hand-written codec drifts — so a reader can check
//! the claim against `bincode`'s source rather than against another decoder.
//!
//! It also carries the module's two structural ledgers: the machine/leaf **type
//! partition** and the **variant-index agreement** with
//! `par_children`'s canonical table.

use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    Connective, ETuple, EZipper, Expr, ListParWithRandom, New, Par, ParWithRandom,
    TaggedContinuation,
};
use models::rust::rhoapi_ext::EPathMap;
use models::rust::rholang::par_children::{
    connective_instance_variant_index, expr_instance_variant_index,
    CONNECTIVE_INSTANCE_VARIANT_COUNT, EXPR_INSTANCE_VARIANT_COUNT,
};
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;

mod par_corpus;
use par_corpus as corpus;

fn par_of(instance: ExprInstance) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

/// Decode with both and require identical values; return the machine's.
fn both(par: &Par) -> Par {
    let bytes = bincode::serialize(par).expect("serialize");
    let oracle: Par = bincode::deserialize(&bytes).expect("oracle");
    let machine = Par::cold_decode(&bytes).expect("machine");
    assert!(machine == oracle, "machine and derived oracle disagree");
    machine
}

// ===========================================================================
// SHAPE 1 — `New.injections: BTreeMap<String, Par>`
// ===========================================================================

/// The **only** `btree_map` field in `RhoTypes.proto`
/// (`models/build.rs` calls `.btree_map(".")`, but `New.injections` is the sole
/// `map<string, Par>` in the schema).
///
/// serde emits a **map**, not a sequence: bincode's `deserialize_map` reads an
/// 8-byte `u64` count and then `count` key/value *pairs*
/// (`bincode/src/de/mod.rs:353-400`). A decoder that reached for
/// `deserialize_seq` would read the count and then try to parse a `Par` where a
/// `String` key sits.
///
/// The keys are interleaved with arbitrarily deep values, which is why the
/// machine's `Op::NewEntry` is a **resume point** rather than a loop.
#[test]
fn shape_1_injections_are_a_map_of_key_value_pairs() {
    let mut injections = BTreeMap::new();
    injections.insert("alpha".to_string(), corpus::gint(1));
    injections.insert("beta".to_string(), corpus::deep_par(8));

    let par = models::par_from_default! {
        news: vec![New {
            bind_count: 0,
            p: None,
            uri: vec![],
            injections: injections.clone(),
            locally_free: vec![],
        }],
        ..Default::default()
    };

    // Byte-level claim: encode just the map and check the leading count.
    let map_bytes = bincode::serialize(&injections).expect("serialize map");
    assert_eq!(
        u64::from_le_bytes(map_bytes[..8].try_into().expect("8 bytes")),
        2,
        "shape 1: a BTreeMap's wire form starts with a u64 ENTRY COUNT"
    );
    // …immediately followed by the first KEY (a length-prefixed string),
    // not by a value.
    assert_eq!(
        u64::from_le_bytes(map_bytes[8..16].try_into().expect("8 bytes")),
        "alpha".len() as u64,
        "shape 1: the count is followed by the first KEY, length-prefixed"
    );

    let decoded = both(&par);
    assert_eq!(decoded.news[0].injections, injections);
}

/// serde's map visitor `insert`s each pair in stream order, so a **duplicate
/// key overwrites** and the stream is **not required to be sorted**. A decoder
/// that assumed sortedness (or rejected duplicates) would refuse byte strings
/// the derived decoder accepts.
///
/// The encoder cannot produce such a stream — a `BTreeMap` has no duplicates —
/// so this is reached by building the bytes by hand, which is exactly the
/// situation `rspace_importer` puts the node in.
#[test]
fn shape_1_duplicate_and_unsorted_map_keys_behave_like_btreemap_insert() {
    // A hand-built `New` encoding whose injection map has THREE entries with
    // keys in DESCENDING order and one duplicate:
    //     "b" -> gint(1),  "a" -> gint(2),  "b" -> gint(3)
    // `BTreeMap::insert` semantics say the result is { "a": 2, "b": 3 }.
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&0i32.to_le_bytes()); // New.bind_count
    bytes.push(0); // New.p = None
    bytes.extend_from_slice(&0u64.to_le_bytes()); // New.uri = []
    bytes.extend_from_slice(&3u64.to_le_bytes()); // injections: 3 entries
    for (key, value) in [("b", 1i64), ("a", 2), ("b", 3)] {
        bytes.extend_from_slice(&(key.len() as u64).to_le_bytes());
        bytes.extend_from_slice(key.as_bytes());
        bytes.extend_from_slice(&bincode::serialize(&corpus::gint(value)).expect("gint"));
    }
    bytes.extend_from_slice(&0u64.to_le_bytes()); // New.locally_free = []

    // Wrap it in a `Par` with exactly one `New` and nothing else.
    let mut par_bytes: Vec<u8> = Vec::new();
    par_bytes.extend_from_slice(&0u64.to_le_bytes()); // sends
    par_bytes.extend_from_slice(&0u64.to_le_bytes()); // receives
    par_bytes.extend_from_slice(&1u64.to_le_bytes()); // news: 1
    par_bytes.extend_from_slice(&bytes);
    par_bytes.extend_from_slice(&0u64.to_le_bytes()); // exprs
    par_bytes.extend_from_slice(&0u64.to_le_bytes()); // matches
    par_bytes.extend_from_slice(&0u64.to_le_bytes()); // unforgeables
    par_bytes.extend_from_slice(&0u64.to_le_bytes()); // bundles
    par_bytes.extend_from_slice(&0u64.to_le_bytes()); // connectives
    par_bytes.extend_from_slice(&0u64.to_le_bytes()); // conditionals
    par_bytes.extend_from_slice(&0u64.to_le_bytes()); // locally_free
    par_bytes.push(0); // connective_used

    let oracle: Par = bincode::deserialize(&par_bytes)
        .expect("the derived decoder must ACCEPT an unsorted map with duplicates");
    let machine = Par::cold_decode(&par_bytes).expect("machine must accept it too");
    assert!(machine == oracle);

    let injections = &machine.news[0].injections;
    assert_eq!(injections.len(), 2, "the duplicate key must collapse");
    assert_eq!(injections["a"], corpus::gint(2));
    assert_eq!(
        injections["b"],
        corpus::gint(3),
        "the LAST occurrence of a duplicate key wins, as `BTreeMap::insert` does"
    );
}

// ===========================================================================
// SHAPE 2 — the 12 `serialize_as_empty_bytes` sites
// ===========================================================================

/// `locally_free` is **written** as `serialize_bytes(&[])` — eight zero bytes —
/// and **read** as a plain `Vec<u8>` through `deserialize_seq`
/// (`models/src/rust/serde_helpers.rs`). The asymmetry is deliberate:
/// `locally_free` is transient free-variable analysis data that must not affect
/// RSpace channel hashes.
///
/// Two consequences, both load-bearing:
///
/// 1. A round-trip **loses** `locally_free` — for the derived decoder too. So
///    "the decoder dropped my bitset" is correct behaviour, and a decoder that
///    "fixed" it would change what every node stores.
/// 2. The decoder must read the stream's **real** length. The encoder only ever
///    writes zero, but the bytes in LMDB did not necessarily come from this
///    encoder, and a decoder that skipped eight bytes unconditionally would
///    desynchronise on any stream that carries a non-empty bitset.
#[test]
fn shape_2_locally_free_is_written_empty_and_read_as_a_real_sequence() {
    let par = corpus::all_par_fields();
    assert!(
        !par.locally_free.is_empty(),
        "fixture must carry a non-empty locally_free"
    );

    let bytes = bincode::serialize(&par).expect("serialize");
    let oracle: Par = bincode::deserialize(&bytes).expect("oracle");
    let machine = Par::cold_decode(&bytes).expect("machine");
    assert!(machine == oracle);

    // Consequence 1: blanked on serialize, so both decoders see empty.
    assert!(
        machine.locally_free.is_empty(),
        "shape 2: `serialize_as_empty_bytes` blanks the field on the way OUT"
    );
    assert!(machine.sends[0].locally_free.is_empty());
    assert!(machine.receives[0].locally_free.is_empty());
    assert!(machine.news[0].locally_free.is_empty());
    assert!(machine.matches[0].locally_free.is_empty());
    assert!(machine.conditionals[0].locally_free.is_empty());

    // Consequence 2: hand-build a stream whose outermost `locally_free` is
    // NON-empty and check that both decoders read it back verbatim.
    let mut hand: Vec<u8> = Vec::new();
    for _ in 0..9 {
        hand.extend_from_slice(&0u64.to_le_bytes()); // the nine empty child seqs
    }
    hand.extend_from_slice(&3u64.to_le_bytes()); // locally_free: THREE bytes
    hand.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
    hand.push(1); // connective_used

    let oracle: Par = bincode::deserialize(&hand).expect("oracle reads real bytes");
    let machine = Par::cold_decode(&hand).expect("machine reads real bytes");
    assert!(machine == oracle);
    assert_eq!(
        machine.locally_free,
        vec![0xAA, 0xBB, 0xCC],
        "shape 2: the DECODER must read the stream's real length — the blanking is \
         serialize-only"
    );
    assert!(machine.connective_used);
}

/// `ETuple` has **no** `remainder` field, unlike `EList`, `ESet` and `EMap`.
/// Off by one field, and every tuple in the term desynchronises the stream.
/// (This is the drift a falsification experiment injected while building the
/// machine; `generate_par` never produces an `ETuple`, so only the constructed
/// corpus caught it.)
#[test]
fn shape_2_etuple_has_no_remainder_field() {
    let tuple = ETuple {
        ps: vec![corpus::gint(1), corpus::gint(2)],
        locally_free: vec![9],
        connective_used: true,
    };
    let list = models::rhoapi::EList {
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
        "EList carries exactly one extra wire byte over ETuple: the `remainder` \
         Option tag. If this ever stops being 1, the machine's ExprTupleBuild / \
         ExprSeqBuild split is wrong."
    );
    both(&par_of(ExprInstance::ETupleBody(tuple)));
}

// ===========================================================================
// SHAPE 3 — `EPathMap` has FOUR serde fields, and the first is EPM1 bytes
// ===========================================================================

/// `intern: OnceLock<Arc<InternedEPathMap>>` is `#[serde(skip)]`, so serde's
/// derived `Deserialize` emits `FIELDS.len() == 4` and bincode turns that into
/// `deserialize_tuple(4)` (`bincode/src/de/mod.rs:402-412`). The cell consumes
/// **nothing** and is left at `OnceLock::default()`.
///
/// ★★ **FORM ② — `ps` is one canonical EPM1 byte array.** `EntryTrie`
/// serializes the PathMap-native snapshot directly; bincode contributes only
/// the byte-array length prefix:
///
/// ```text
///   u64-LE |EPM1(m)| ‖ EPM1(m)  ← trie arena and, for map mode, values
/// ```
///
/// The field count remains four: the snapshot is slot 0, followed by the three
/// semantic metadata fields. `models/tests/epathmap_epm1_snapshot.rs` pins
/// contiguity, canonical ordering, and unbounded-depth round trips.
#[test]
fn shape_3_epathmap_is_four_serde_fields_whose_first_is_the_epm1_snapshot() {
    let map = EPathMap::new(
        vec![corpus::gint(1), corpus::gint(2)],
        vec![0xAA],
        true,
        None,
    );
    let bytes = bincode::serialize(&map).expect("serialize");
    let snapshot = map.trie_snapshot().to_vec();

    // Field 0: the canonical EPM1 snapshot, length-framed by bincode.
    assert_eq!(
        u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes")),
        snapshot.len() as u64,
        "shape 3: the FIRST thing an EPathMap writes is the EPM1 byte length"
    );
    assert_eq!(
        &bytes[8..8 + snapshot.len()],
        &snapshot[..],
        "shape 3: …followed by the EPM1 snapshot itself, verbatim"
    );

    // The whole encoding is exactly: EPM1(m) ++ locally_free ++
    // connective_used ++ remainder, and NOTHING for `intern`.
    let mut expected = Vec::new();
    expected.extend_from_slice(&(snapshot.len() as u64).to_le_bytes());
    expected.extend_from_slice(&snapshot);
    expected.extend_from_slice(&0u64.to_le_bytes()); // locally_free: BLANKED
    expected.push(1); // connective_used
    expected.push(0); // remainder: None
    assert_eq!(
        bytes, expected,
        "shape 3: EPathMap's wire form is exactly the trie snapshot, \
         and the three metadata fields; the `intern` cell contributes nothing"
    );

    // And the retained derived `Deserialize` reads REAL `locally_free` bytes,
    // which the machine must match: hand-build one with a non-empty bitset.
    let empty_snapshot = EPathMap::default().trie_snapshot().to_vec();
    let mut hand = (empty_snapshot.len() as u64).to_le_bytes().to_vec();
    hand.extend_from_slice(&empty_snapshot);
    hand.extend_from_slice(&2u64.to_le_bytes());
    hand.extend_from_slice(&[0x01, 0x02]);
    hand.push(0); // connective_used
    hand.push(0); // remainder
    let oracle: EPathMap = bincode::deserialize(&hand).expect("oracle");
    assert_eq!(
        oracle.locally_free,
        vec![0x01, 0x02],
        "shape 3: the derived Deserialize reads the stream's real locally_free"
    );

    // Reached through the `Par` machine (`EPathmapBody`).
    let decoded = both(&par_of(ExprInstance::EPathmapBody(map)));
    assert!(
        matches!(
            decoded.exprs[0].expr_instance.as_ref(),
            Some(ExprInstance::EPathmapBody(_))
        ),
        "expected EPathmapBody"
    );
}

/// `EZipper.pathmap` is an `Option<EPathMap>`: one tag byte, THEN the four
/// fields. The direct `EPathmapBody` arm has no such tag.
#[test]
fn shape_3_ezipper_wraps_the_pathmap_in_an_option() {
    let inner = EPathMap::new(vec![corpus::gint(5)], Vec::new(), false, None);
    let bare = bincode::serialize(&inner).expect("bare");
    let wrapped = bincode::serialize(&Some(inner.clone())).expect("wrapped");
    assert_eq!(
        wrapped.len(),
        bare.len() + 1,
        "shape 3: Option<EPathMap> costs exactly one tag byte over EPathMap"
    );
    assert_eq!(wrapped[0], 1, "Some(..) is tag 1");

    both(&par_of(ExprInstance::EZipperBody(EZipper {
        pathmap: Some(inner),
        current_path: vec![vec![1, 2], vec![]],
        is_write_zipper: true,
        locally_free: vec![7],
        connective_used: true,
        cursor_kind: 2,
    })));
    both(&par_of(ExprInstance::EZipperBody(EZipper {
        pathmap: None,
        current_path: vec![],
        is_write_zipper: false,
        locally_free: vec![],
        connective_used: false,
        cursor_kind: 0,
    })));
}

// ===========================================================================
// SHAPE 4 — the three oneofs: `Option` tag (1 B) THEN variant index (4 B)
// ===========================================================================

/// A `Some(oneof)` is **five** bytes of framing, not one: the `Option` tag
/// (`deserialize_option`, one byte, `bincode/src/de/mod.rs:332-342`) and then
/// the variant index (`variant_seed`, a `u32` fixint-LE, `:280-287`).
///
/// And the index is serde's **declaration order**, not the proto tag.
/// `EPathmapBody` is proto tag **32** and serde index **25**; reading proto tags
/// as serde indices would silently mis-decode 12 of the 36 arms.
#[test]
fn shape_4_oneofs_are_an_option_tag_then_a_u32_variant_index() {
    for (name, instance) in corpus::every_expr_instance() {
        let expected = expr_instance_variant_index(&instance);
        let bytes = bincode::serialize(&Expr {
            expr_instance: Some(instance),
        })
        .expect("serialize");
        assert_eq!(bytes[0], 1, "{name}: Some(..) is tag 1");
        assert_eq!(
            u32::from_le_bytes(bytes[1..5].try_into().expect("4 bytes")),
            expected,
            "{name}: the variant index is a u32 immediately after the Option tag"
        );
    }

    // The absent oneof is ONE byte and no index.
    let absent = bincode::serialize(&Expr {
        expr_instance: None,
    })
    .expect("serialize");
    assert_eq!(absent, vec![0u8], "an absent oneof is a single zero byte");

    // `EPathmapBody`: proto tag 32, serde index 25 — the discrepancy in the
    // flesh.
    assert_eq!(
        expr_instance_variant_index(&ExprInstance::EPathmapBody(corpus::ground_pathmap())),
        25,
        "EPathmapBody is proto tag 32 but serde index 25"
    );
}

/// The machine's index→arm dispatch must be the inverse of `par_children`'s
/// canonical index table, for **every** index in range. This is the gate that
/// stops `bincode_decoder` from becoming a fifth, independent enumeration of the
/// schema.
#[test]
fn bincode_decoder_variant_indices_agree() {
    let mut seen: Vec<u32> = Vec::with_capacity(EXPR_INSTANCE_VARIANT_COUNT);
    for (name, instance) in corpus::every_expr_instance() {
        let index = expr_instance_variant_index(&instance);
        let par = par_of(instance);
        let decoded = both(&par);
        let round_tripped = decoded.exprs[0]
            .expr_instance
            .as_ref()
            .map(expr_instance_variant_index)
            .expect("decoded Expr must carry an instance");
        assert_eq!(
            round_tripped, index,
            "{name}: the machine decoded wire index {index} to an arm whose canonical \
             index is {round_tripped}"
        );
        seen.push(index);
    }
    seen.sort_unstable();
    assert_eq!(
        seen,
        (0..EXPR_INSTANCE_VARIANT_COUNT as u32).collect::<Vec<u32>>(),
        "the machine was not exercised on every ExprInstance index"
    );

    let mut seen: Vec<u32> = Vec::with_capacity(CONNECTIVE_INSTANCE_VARIANT_COUNT);
    for (name, instance) in corpus::every_connective_instance() {
        let index = connective_instance_variant_index(&instance);
        let par = models::par_from_default! {
            connectives: vec![Connective {
                connective_instance: Some(instance),
            }],
            ..Default::default()
        };
        let decoded = both(&par);
        let round_tripped = decoded.connectives[0]
            .connective_instance
            .as_ref()
            .map(connective_instance_variant_index)
            .expect("decoded Connective must carry an instance");
        assert_eq!(round_tripped, index, "{name}: index mismatch");
        seen.push(index);
    }
    seen.sort_unstable();
    assert_eq!(
        seen,
        (0..CONNECTIVE_INSTANCE_VARIANT_COUNT as u32).collect::<Vec<u32>>(),
        "the machine was not exercised on every ConnectiveInstance index"
    );
}

/// `TaggedCont` is the third oneof, and it is the one whose *absent* form is
/// easy to conflate with `ScalaBodyRef(0)`.
#[test]
fn shape_4_tagged_cont_absent_is_not_variant_zero() {
    let absent = TaggedContinuation {
        guard: None,
        tagged_cont: None,
    };
    let par_body = TaggedContinuation {
        guard: None,
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: None,
            random_state: vec![],
        })),
    };
    let scala = TaggedContinuation {
        guard: None,
        tagged_cont: Some(TaggedCont::ScalaBodyRef(0)),
    };

    for value in [&absent, &par_body, &scala] {
        let bytes = bincode::serialize(value).expect("serialize");
        let oracle: TaggedContinuation = bincode::deserialize(&bytes).expect("oracle");
        let machine = TaggedContinuation::cold_decode(&bytes).expect("machine");
        assert!(machine == oracle);
        assert!(machine == *value);
    }

    // Byte-level: `guard: None` is one byte, then the `tagged_cont` Option tag.
    let a = bincode::serialize(&absent).expect("absent");
    assert_eq!(a, vec![0u8, 0u8], "guard tag 0, tagged_cont tag 0");
    let p = bincode::serialize(&par_body).expect("par_body");
    assert_eq!(p[0], 0, "guard tag");
    assert_eq!(p[1], 1, "tagged_cont Some tag");
    assert_eq!(
        u32::from_le_bytes(p[2..6].try_into().expect("4 bytes")),
        0,
        "ParBody is variant index 0"
    );
    let s = bincode::serialize(&scala).expect("scala");
    assert_eq!(
        u32::from_le_bytes(s[2..6].try_into().expect("4 bytes")),
        1,
        "ScalaBodyRef is variant index 1"
    );
}

// ===========================================================================
// The TYPE PARTITION ledger
// ===========================================================================

/// The 63 serde types of the `rhoapi` schema (62 generated + the hand-written
/// `EPathMap`), split into the 47 that transitively contain `Par` and go on the
/// machine, and the 16 that do not and keep the derived impl.
///
/// ## What makes this checkable rather than a comment
///
/// Two compile-time guards, not a count in a doc string:
///
/// 1. **A new FIELD on any machine type is a compile error.** Every `*Build` op
///    in `bincode_decoder.rs` constructs its type with a **complete struct literal** —
///    no `..Default::default()` anywhere in a decode path. Add a field to
///    `Send`, and `Op::SendBuild` stops compiling.
///
/// 2. **A new VARIANT on either big oneof is a compile error.** The matches in
///    `par_children.rs` (`expr_instance_child_pars`,
///    `expr_instance_variant_index`, `substitute_descends_into`, and their
///    connective twins) are exhaustive with no `_` arm, and
///    `EXPR_INSTANCE_VARIANT_COUNT` / `CONNECTIVE_INSTANCE_VARIANT_COUNT` are
///    asserted against a per-variant corpus.
///
/// The arithmetic below is the ledger those guards protect.
#[test]
fn bincode_decoder_type_partition() {
    /// Transitively contains `Par`; decoded by the machine.
    const MACHINE_TYPES: [&str; 47] = [
        // the `Par` SCC (41)
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
        // contain `Par` but are not IN the cycle (6)
        "TaggedContinuation",
        "TaggedCont",
        "ParWithRandom",
        "ListParWithRandom",
        "BindPattern",
        "ListBindPatterns",
    ];

    /// Outside the `Par` SCC: fixed maximum nesting, hence fixed maximum stack.
    /// Decoded by the bounded leaf readers in `bincode_decoder::Reader`.
    const BOUNDED_TYPES: [&str; 16] = [
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
    ];

    assert_eq!(MACHINE_TYPES.len(), 47);
    assert_eq!(BOUNDED_TYPES.len(), 16);
    assert_eq!(
        MACHINE_TYPES.len() + BOUNDED_TYPES.len(),
        63,
        "the schema has 62 generated serde types plus the hand-written EPathMap"
    );

    let mut names: Vec<&str> = MACHINE_TYPES
        .iter()
        .chain(BOUNDED_TYPES.iter())
        .copied()
        .collect();
    names.sort_unstable();
    let unique = {
        let mut n = names.clone();
        n.dedup();
        n.len()
    };
    assert_eq!(
        unique,
        names.len(),
        "a type appears on both sides of the partition"
    );
}

// ===========================================================================
// The property the partition exists to support
// ===========================================================================

/// The machine decodes a term far deeper than the derived decoder survives, on
/// a stack far smaller than the derived decoder needs.
///
/// Measured on 2026-07-26, the derived `Par` decode costs **28,362 B/level** in
/// debug and **12,894 B/level** in release. At depth 4,096 that is ~110 MiB
/// (debug) / ~50 MiB (release); this test gives the thread **256 KiB**.
///
/// The cross-process, bisected version of this claim — with the zero-slope
/// assertion over a 4 → 4,096 ladder in both profiles — lives in
/// `rholang/tests/stack_depth_gate.rs`.
#[test]
fn machine_decodes_depth_4096_on_a_256_kib_stack() {
    // Encode on a generous stack: `bincode::serialize` is Θ(depth) too
    // (3,052 / 329 B per level), and this test is about the DECODER.
    let bytes = std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(|| {
            let term = corpus::deep_par(4096);
            let b = bincode::serialize(&term).expect("serialize");
            models::rust::rholang::par_children::dismantle(term);
            b
        })
        .expect("spawn encoder")
        .join()
        .expect("encoder panicked");

    assert!(
        bytes.len() > 4096,
        "VACUOUS PROBE: a depth-4096 term encoded to only {} bytes",
        bytes.len()
    );

    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let decoded = Par::cold_decode(&bytes).expect("machine must decode depth 4096");
            // Confirm the nesting really came back — a decoder that returned an
            // empty `Par` would also "survive".
            let depth = {
                let mut d = 0usize;
                let mut current = &decoded;
                while let Some(ExprInstance::EListBody(list)) =
                    current.exprs.first().and_then(|e| e.expr_instance.as_ref())
                {
                    d += 1;
                    match list.ps.first() {
                        Some(next) => current = next,
                        None => break,
                    }
                }
                d
            };
            assert_eq!(depth, 4096, "the DECODED term must carry the nesting");
            // Tear down iteratively; `<Par as Drop>` is Θ(depth) too.
            models::rust::rholang::par_children::dismantle(decoded);
        })
        .expect("spawn decoder")
        .join()
        .expect("the machine overflowed a 256 KiB stack at depth 4096");
}

/// The same, on the error path. A decode that FAILS at the last byte of a deep
/// term still holds a deep partial result, and `<Par as Drop>` is Θ(depth) —
/// so a machine without iterative teardown would abort while *rejecting* a
/// hostile input, which is the exact failure mode this work removes.
#[test]
fn machine_rejects_a_truncated_deep_term_without_overflowing() {
    let bytes = std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(|| {
            let term = corpus::deep_par(4096);
            let b = bincode::serialize(&term).expect("serialize");
            models::rust::rholang::par_children::dismantle(term);
            b
        })
        .expect("spawn encoder")
        .join()
        .expect("encoder panicked");

    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            // Drop the final byte: everything parses until the outermost
            // `connective_used`, at which point ~4,096 levels are live on the
            // machine's value stacks and must be released iteratively.
            let truncated = &bytes[..bytes.len() - 1];
            let err =
                Par::cold_decode(truncated).expect_err("a truncated encoding must be REJECTED");
            let text = err.to_string();
            assert!(
                text.contains("unexpected end of input"),
                "expected an EOF rejection, got: {text}"
            );
        })
        .expect("spawn decoder")
        .join()
        .expect("the machine overflowed a 256 KiB stack while TEARING DOWN a rejected term");
}

// ===========================================================================
// The prefix contract
// ===========================================================================

/// `Datum<A>` is `{ a: A, persist: bool, source: Produce }`: `A` is a **prefix**
/// of a longer encoding, and bincode's format is not self-delimiting from the
/// outside. `cold_decode_prefix` therefore has to report the exact extent, and
/// the field that follows has to land where the derived decoder puts it.
#[test]
fn prefix_extents_let_the_following_field_be_read() {
    let value = ListParWithRandom {
        pars: vec![corpus::all_par_fields()],
        random_state: vec![1, 2, 3],
    };
    let mut bytes = bincode::serialize(&value).expect("serialize");
    let extent = bytes.len();
    // Append what `Datum` would put after `a`: `persist` and a marker.
    bytes.push(1);
    bytes.extend_from_slice(&[0xAB, 0xCD]);

    let (decoded, consumed) = ListParWithRandom::cold_decode_prefix(&bytes).expect("machine");
    // ⚠ Compared against the ORACLE, not against `value`. `all_par_fields`
    // carries non-empty `locally_free`, which shape 2 blanks on serialize, so
    // `decoded != value` is correct behaviour — for the derived decoder too.
    let oracle: ListParWithRandom = bincode::deserialize(&bytes).expect("oracle");
    assert!(decoded == oracle);
    assert_eq!(
        consumed, extent,
        "the reported extent must exclude the tail"
    );
    assert_eq!(
        bytes[consumed], 1,
        "the next field starts exactly at `consumed`"
    );
}
