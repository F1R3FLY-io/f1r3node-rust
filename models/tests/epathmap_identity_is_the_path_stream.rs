//! ★ **THE DIFFERENTIAL** — every way an `EPathMap` can be compared, hashed, or
//! serialized is one function of ONE artifact.
//!
//! ```math
//! U(m) \;=\; \big\Vert_{k \in \mathrm{keys}(m)}
//!            \big(\mathrm{u32\text{-}LE}(|k|) \,\Vert\, k\big)
//! ```
//!
//! `U(m)` is the field-8 path stream (`RhoTypes.proto`, `serialized_paths`): the
//! uncompressed, trie-ordered, length-framed key stream produced by a read-zipper
//! walk over the map's trie, with **no sort**. `RhoTypes.proto:333-338` already
//! calls it *"the canonical identity + hash preimage of a ground map"*. The claim
//! this file makes executable is that the description is now literally true of
//! every comparator:
//!
//! ```text
//! Eq           a == b          ⟺  U(a) == U(b)
//! Hash         hash(a)             = f(U(a))
//! serde bytes  bincode(a)          = f(U(a))
//! prost bytes  encode(a)           = f(U(a))
//! ```
//!
//! # ★ The order is not defined here — the trie already has one
//!
//! A trie's children are indexed **by byte**, so a zipper walk **is**
//! byte-lexicographic **by construction**. Nothing chooses that order and nothing
//! could choose a different one. That is exactly why
//! `pathmap_crate_type_mapper::path_stream_of` emits its key stream **with no
//! sort**: the structure supplies the order, and a sort would be a second opinion
//! about something that is not in question.
//!
//! So the defect this file names was never *"pathmaps lack a canonical order"*.
//! It is that the `Vec<Par>` in `EPathMap.ps` carries a **second, competing
//! order — insertion order — that shadows the trie's**. `==`, `Hash`, and `Ord`
//! read the `Vec`'s; serde and prost read the trie's. **The two already
//! disagree**, and the property below is what makes the disagreement visible.
//!
//! The repair is therefore subtractive: deleting the `Vec` does not *impose*
//! canonicity, it **removes the competitor**, leaving exactly one order. The
//! disagreement becomes unrepresentable rather than merely fixed — and that is
//! also why the repair is byte-preserving. It invents no encoding; it makes the
//! in-memory comparators read the order the wire has used all along.
//!
//! # Why one artifact rather than four agreeing implementations
//!
//! The alternative — four canonicalisation routines that each remember to sort,
//! dedup, and recurse — is the design that failed five times (defects #83, #89,
//! #91, #108, and the `setSubtrie` bare-entry loss). Each failure was one
//! consumer forgetting a discipline the others kept. Making all four *functions
//! of the same bytes* removes the possibility of one drifting: there is no second
//! route to disagree with.
//!
//! ⚠ **The order comes from the zipper walk over keys, never from a hash.**
//! `PathMap::hash` and `PathMap::merkleize` exist and look like they would serve;
//! they must never be used for identity, `Ord`, or any hash preimage.
//! `pathmap-0.2.2/src/lib.rs:15-21` selects `gxhash` (AES intrinsics) normally
//! and *"a simple XOR hasher"* on `riscv64`/miri, so both are
//! **architecture-dependent** and would make consensus identity depend on the
//! machine that computed it. Neither has a call site in this repository, and this
//! note exists so the first one is a deliberate act rather than an accident.
//!
//! # The corpus, and why it is exhaustive rather than randomised
//!
//! The alphabet is the nine-element set of `models/tests/pathmap_integration_tests.rs`
//! — both codec arms, at both arities that can collide, plus the nesting and sign
//! edges — and the corpus is **all `2^9 = 512` subsets** of it. Exhaustive over
//! the alphabet means deterministic: no seed to lose, no `proptest-regressions`
//! file to keep, and no case left to a generator's luck. Each subset is presented
//! in three constructions — forward, reversed, and duplicated — which are exactly
//! the three ways a producer can differ while naming the same entry set.
//!
//! # ⚠ `Ord` is deliberately absent from this file at this stage
//!
//! `Ord` is the fifth member of the family and belongs here, but moving it costs
//! a consensus version bump (sorted containers and the sorter's output move) and
//! collides with the deliberate `Ord`-includes-`locally_free` wart pinned at
//! `84a0fbe4` (`models/src/rust/rhoapi_ext.rs`). It joins this differential in
//! the stage that resolves that ruling, not before. Its absence here is a
//! recorded scope boundary, not an oversight.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::pathmap_integration::create_pathmap_from_elements;
use prost::Message;

// ─────────────────────────────────────────────────────────────────────────────
// The alphabet and the corpus (mirroring pathmap_integration_tests.rs)
// ─────────────────────────────────────────────────────────────────────────────

fn make_string_par(s: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        }],
        ..Default::default()
    }
}

fn make_int_par(i: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i)),
        }],
        ..Default::default()
    }
}

fn make_list_of(ps: Vec<Par>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

/// The element alphabet: both codec arms, at both arities that can collide, plus
/// the nesting and sign edges. `1` and `[1]` differ by exactly the terminator;
/// `"a"` is a strict byte-prefix of `["a","x"]`.
fn alphabet() -> Vec<Par> {
    vec![
        make_int_par(1),
        make_int_par(-7),
        make_string_par("a"),
        make_string_par(""),
        make_list_of(vec![]),
        make_list_of(vec![make_int_par(1)]),
        make_list_of(vec![make_string_par("a")]),
        make_list_of(vec![make_string_par("a"), make_string_par("x")]),
        make_list_of(vec![
            make_list_of(vec![make_int_par(2)]),
            make_string_par("q"),
        ]),
    ]
}

/// Every subset of the alphabet — `2^9 = 512` element sets.
fn every_subset_of_the_alphabet() -> Vec<Vec<Par>> {
    let alphabet = alphabet();
    let mut subsets = Vec::with_capacity(1 << alphabet.len());
    for mask in 0u32..(1u32 << alphabet.len()) {
        let mut subset = Vec::with_capacity(alphabet.len());
        for (index, element) in alphabet.iter().enumerate() {
            if mask & (1 << index) != 0 {
                subset.push(element.clone());
            }
        }
        subsets.push(subset);
    }
    subsets
}

// ─────────────────────────────────────────────────────────────────────────────
// The artifact, and the four things that must be functions of it
// ─────────────────────────────────────────────────────────────────────────────

/// `U(m)` computed INDEPENDENTLY of the comparators under test — from the
/// entries, through the sole program-facing trie constructor, by a read-zipper
/// walk. Deliberately not a call into whatever `Eq` happens to use, so the
/// expectation is not the implementation restated.
fn path_stream(entries: &[Par]) -> Vec<u8> {
    use pathmap::zipper::{ZipperIteration, ZipperMoving};

    let built = create_pathmap_from_elements(entries, None);
    let mut rz = built.map.read_zipper();
    let mut stream = Vec::new();
    while rz.to_next_val() {
        let key = rz.path();
        stream.extend_from_slice(&(key.len() as u32).to_le_bytes());
        stream.extend_from_slice(key);
    }
    stream
}

fn ground_map(entries: Vec<Par>) -> EPathMap { EPathMap::new(entries, Vec::new(), false, None) }

fn hash_of(map: &EPathMap) -> u64 {
    let mut hasher = DefaultHasher::new();
    map.hash(&mut hasher);
    hasher.finish()
}

fn prost_bytes(map: &EPathMap) -> Vec<u8> {
    let mut buf = Vec::with_capacity(map.encoded_len());
    map.encode_raw(&mut buf);
    buf
}

fn serde_bytes(map: &EPathMap) -> Vec<u8> {
    bincode::serialize(map).expect("an EPathMap serializes with bincode")
}

/// The three constructions that name one entry set and differ only in the order
/// and multiplicity a producer happened to use: forward, reversed, and every
/// element repeated. A representation that observes construction order tells
/// them apart; one that is a function of the entry set cannot.
fn constructions_of(subset: &[Par]) -> Vec<(&'static str, Vec<Par>)> {
    let mut reversed = subset.to_vec();
    reversed.reverse();

    let mut duplicated = Vec::with_capacity(subset.len() * 2);
    for element in subset {
        duplicated.push(element.clone());
        duplicated.push(element.clone());
    }

    vec![
        ("forward", subset.to_vec()),
        ("reversed", reversed),
        ("duplicated", duplicated),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE PROPERTY
// ─────────────────────────────────────────────────────────────────────────────

/// ★ Over all 512 entry sets × 3 constructions: two maps naming the same entry
/// set are indistinguishable by `==`, by `Hash`, by their serde bytes, and by
/// their prost bytes — because all four are functions of `U(m)`, and the three
/// constructions share one `U(m)`.
///
/// The four legs are separately load-bearing, and today they do **not** all
/// hold. `encode_raw` and `serialize` already canonicalise a ground map through
/// its trie, while `==` and `Hash` compare `ps` positionally
/// (`models/src/rust/rhoapi_ext.rs`). So the wire and the comparators **already
/// disagree**: two producers can build one map, agree byte-for-byte on the
/// consensus encoding, and still fail to recognise each other's value in a
/// `HashSet`. That is defect #83 stated at its sharpest, and it is what the
/// `Eq`/`Hash` legs below name.
#[test]
fn identity_hash_and_both_encodings_are_functions_of_the_path_stream() {
    for subset in every_subset_of_the_alphabet() {
        let expected_stream = path_stream(&subset);
        let constructions = constructions_of(&subset);

        let (_, first_entries) = &constructions[0];
        let first = ground_map(first_entries.clone());

        for (label, entries) in &constructions {
            let candidate = ground_map(entries.clone());

            // The premise: every construction of this entry set has the SAME
            // U(m). If this leg fails the rest are meaningless, so it is stated
            // first and separately.
            assert_eq!(
                path_stream(entries),
                expected_stream,
                "U(m) is not a function of the entry set: the `{label}` \
                 construction of a {}-element set produced a different path \
                 stream",
                subset.len()
            );

            // LEG 1 — `==`.
            assert_eq!(
                first,
                candidate,
                "★ two maps with one entry set are NOT `==`: the `{label}` \
                 construction of a {}-element set is a different value from the \
                 `forward` one, although both encode to the same consensus \
                 bytes",
                subset.len()
            );

            // LEG 2 — `Hash`, which must agree with `==` or every hash container
            // holding an `EPathMap` is unsound.
            assert_eq!(
                hash_of(&first),
                hash_of(&candidate),
                "★ two `==` maps hash differently: the `{label}` construction of \
                 a {}-element set lands in a different bucket",
                subset.len()
            );

            // LEG 3 — the serde bytes, which are the event-hash preimage.
            assert_eq!(
                serde_bytes(&first),
                serde_bytes(&candidate),
                "the `{label}` construction of a {}-element set produced \
                 different serde bytes — two producers of one map would emit \
                 different event hashes",
                subset.len()
            );

            // LEG 4 — the prost bytes, which are the consensus encoding.
            assert_eq!(
                prost_bytes(&first),
                prost_bytes(&candidate),
                "the `{label}` construction of a {}-element set produced \
                 different prost bytes",
                subset.len()
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE NON-GROUND ARM — measured, and it is STILL A LIST
// ─────────────────────────────────────────────────────────────────────────────

/// ★ **The wire is still a list for non-ground maps, and this pins the fact.**
///
/// A ground map encodes as proto field 8 — `U(m)`, the trie's key stream — and
/// the `Vec` never reaches the wire. A NON-ground map has no such arm: tag 1,
/// `repeated Par`, is its only encoding, and prost writes those entries **in
/// `ps` order**. So for non-ground maps the protobuf model genuinely still
/// stores a **list**, and this test measures that rather than asserting it: the
/// two constructions below differ only in the order two entries were written in,
/// and their prost bytes differ.
///
/// # Why this is pinned as a passing test rather than fixed here
///
/// Fixing it means routing non-ground entries through the trie too, which makes
/// them order-insensitive and deduplicated — a real improvement and the direct
/// answer to *"does the pathmap protobuf model still store it as a list?"*. It
/// is also **consensus-visible**: `sort_combine::combine_epathmap`'s non-ground
/// arm recursively sorts each entry and **preserves entry order**, so today two
/// non-ground maps built in different orders are different terms all the way
/// through normalisation. Changing that moves bytes, and moving bytes requires a
/// coordinated protocol version bump, which is F1r3node's decision and not this
/// campaign's to make.
///
/// Pinning it means the day it changes is a deliberate act: this test must be
/// rewritten, and rewriting it is the moment the bump gets discussed.
///
/// ⚠ And a limit worth stating so it is not over-claimed later: routing
/// non-ground entries through the trie would give them a canonical **SYNTACTIC**
/// identity — byte-lex over their escape-arm encodings — and **not a semantic
/// one**. Two non-ground entries can be different terms that match the same
/// things, and no representation can collapse those, because pattern
/// equivalence is undecidable in general.
#[test]
fn a_non_ground_map_still_encodes_as_an_ORDER_SENSITIVE_LIST() {
    // An `EVar` is non-`eval_stable` by content, so a map holding one takes the
    // tag-1 field walk rather than the field-8 value arm.
    let var_entry = models::rust::utils::new_boundvar_par(1, Vec::new(), false);
    let ground_entry = make_list_of(vec![make_string_par("a")]);

    let forward = ground_map(vec![var_entry.clone(), ground_entry.clone()]);
    let backward = ground_map(vec![ground_entry, var_entry]);

    // The premise: these are genuinely non-ground, so neither took the field-8
    // arm. A ground map's encoding starts with the field-8 key (tag 8,
    // length-delimited = 0x42); a non-ground one starts with tag 1 (0x0a).
    let forward_bytes = prost_bytes(&forward);
    let backward_bytes = prost_bytes(&backward);
    assert_eq!(
        forward_bytes.first(),
        Some(&0x0au8),
        "the fixture must exercise the TAG-1 arm, not the field-8 value arm"
    );

    // ★ THE MEASUREMENT: one entry set, two orders, two different encodings.
    assert_ne!(
        forward_bytes, backward_bytes,
        "★ if this now passes with EQUAL bytes, the non-ground arm has been \
         routed through the trie — that is the intended end state, and it MOVES \
         CONSENSUS BYTES. Rewrite this test, and do not land the change without \
         the coordinated protocol version bump it requires."
    );

    // …and the comparators agree with the wire about that, which is the whole
    // point of the identity family: they read the entries positionally in
    // exactly the arm where the wire does too.
    assert_ne!(
        forward, backward,
        "the comparators must not disagree with the wire in the non-ground arm \
         either — a map the wire distinguishes must not compare equal"
    );
}

/// ★ **THE #83 WITNESS**, minimal: two entries, one permutation, no duplicates
/// and no metadata in play. Stated on its own because the exhaustive property
/// above reaches a duplicate construction first, and *"duplicates are equal"* is
/// a weaker and more arguable claim than *"the order two entries were written in
/// is not part of the value"*.
///
/// The map is `{| 1, "a" |}` against `{| "a", 1 |}`. They have one entry set, one
/// trie, one path stream, and — already, today — one prost encoding and one serde
/// encoding. The only thing that can tell them apart is the `Vec`'s insertion
/// order, and nothing in the semantics of a pathmap says a program can observe
/// it.
#[test]
fn a_permutation_of_two_entries_is_the_same_map() {
    let one = make_int_par(1);
    let letter = make_string_par("a");

    let forward = ground_map(vec![one.clone(), letter.clone()]);
    let backward = ground_map(vec![letter, one]);

    // The wire already agrees — this is not a change being proposed, it is the
    // state of the tree.
    assert_eq!(
        prost_bytes(&forward),
        prost_bytes(&backward),
        "the consensus encoding already treats the two as one map"
    );
    assert_eq!(
        serde_bytes(&forward),
        serde_bytes(&backward),
        "the event-hash preimage already treats the two as one map"
    );

    // …and the comparators must not be the one place that disagrees.
    assert_eq!(
        forward, backward,
        "★ `{{| 1, \"a\" |}}` and `{{| \"a\", 1 |}}` are not `==`, although they \
         encode to the same consensus bytes: the `Vec`'s insertion order is a \
         second, competing order shadowing the trie's"
    );
    assert_eq!(
        hash_of(&forward),
        hash_of(&backward),
        "★ …and they hash into different buckets"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// ⚠ ANTI-VACUITY — the property above must SEPARATE things, not merely agree
// ─────────────────────────────────────────────────────────────────────────────

/// ⚠ Without this, "all four agree" could hold because all four collapse
/// everything to one value. Distinct entry sets have distinct `U(m)`, and all
/// four artifacts must see the difference.
#[test]
fn distinct_entry_sets_are_distinguished_by_all_four() {
    let subsets = every_subset_of_the_alphabet();

    // Every subset against its immediate successor in the enumeration — 511
    // distinct pairs, each differing in at least one element.
    for window in subsets.windows(2) {
        let (left, right) = (&window[0], &window[1]);
        let left_map = ground_map(left.clone());
        let right_map = ground_map(right.clone());

        assert_ne!(
            path_stream(left),
            path_stream(right),
            "two DIFFERENT entry sets share a path stream — U(m) is not \
             injective on entry sets, and every leg below is vacuous"
        );
        assert_ne!(left_map, right_map, "two different entry sets compare `==`");
        assert_ne!(
            serde_bytes(&left_map),
            serde_bytes(&right_map),
            "two different entry sets share serde bytes"
        );
        assert_ne!(
            prost_bytes(&left_map),
            prost_bytes(&right_map),
            "two different entry sets share prost bytes"
        );
    }
}

/// ⚠ The leg that catches a canonical key naming the WRONG entry.
///
/// Membership in the codec's image is not enough: `1` and `[1]` are BOTH
/// canonical keys — `03 02` and `03 02 00`, differing by exactly the split-list
/// terminator — and a reader that confuses them gets a valid key for the wrong
/// element. `a1feb437` found exactly this shape. So the identity family must
/// separate the bare element from the singleton list containing it, at every
/// one of the four legs.
#[test]
fn a_bare_element_and_its_singleton_list_are_different_maps() {
    let bare = make_int_par(1);
    let singleton = make_list_of(vec![make_int_par(1)]);

    let bare_map = ground_map(vec![bare.clone()]);
    let singleton_map = ground_map(vec![singleton.clone()]);

    assert_ne!(
        path_stream(&[bare.clone()]),
        path_stream(&[singleton.clone()]),
        "the bare element and the singleton list share a path stream — the \
         terminator that separates `03 02` from `03 02 00` was lost"
    );
    assert_ne!(bare_map, singleton_map, "`{{| 1 |}}` == `{{| [1] |}}`");
    assert_ne!(hash_of(&bare_map), hash_of(&singleton_map));
    assert_ne!(serde_bytes(&bare_map), serde_bytes(&singleton_map));
    assert_ne!(prost_bytes(&bare_map), prost_bytes(&singleton_map));

    // …and a map holding BOTH holds two entries, not one.
    let both = ground_map(vec![bare, singleton]);
    assert_eq!(
        create_pathmap_from_elements(&both.ps, None).map.val_count(),
        2,
        "a map holding the bare element and its singleton list holds two \
         distinct entries"
    );
}
