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

/// ★★ **THE `401ed168` WITNESS, UPDATED DELIBERATELY.**
///
/// The commit that added it left an instruction on the assertion: *"if this now
/// passes with EQUAL bytes, the non-ground arm has been routed through the trie
/// — that is the intended end state, and it MOVES CONSENSUS BYTES. Rewrite this
/// test."* This is that rewrite, and it is written by the change that earned it.
///
/// # What it measured, and what it measures now
///
/// It measured that two constructions of one non-ground entry set, differing
/// only in the order the entries were written in, produced **different** prost
/// bytes — because tag 1, `repeated Par`, was the arm's only encoding and prost
/// wrote those entries in `ps` order. **The wire really was still a list.**
///
/// `EPathMap` stores an entry trie now, so `ps()` is the trie's own order for
/// every map. The wire is STILL tag 1 `repeated Par` in this arm — that part of
/// the finding was and remains true, and this test still checks it — but the
/// sequence written into it is no longer a producer's. Same arm, canonical
/// contents.
///
/// # ⚠ The consequence, stated rather than buried
///
/// This MOVES CONSENSUS BYTES for non-ground maps: prost bytes, serde bytes,
/// event-hash preimages, and sorted-container order all follow `ps()`.
/// `Validate::version` is exact equality (`casper/src/rust/validate.rs`) with no
/// activation-height machinery, so there is no gradual rollout for it. **The
/// version constant is deliberately NOT touched by this change**: bumping it is
/// a network-coordination act that belongs to F1r3node, not a code act, and
/// writing the code does not perform it.
///
/// # ⚠ And the limit, so it is not over-claimed
///
/// This is a canonical **SYNTACTIC** identity — byte-lex over the entries'
/// escape-arm encodings — and **not a semantic one**. Two non-ground entries can
/// be different terms that match the same things, and no representation
/// collapses those, because pattern equivalence is undecidable in general. The
/// gain is order-insensitivity and deduplication, which is real.
#[test]
fn a_non_ground_map_now_encodes_as_an_ORDER_INSENSITIVE_LIST() {
    // An `EVar` is non-`eval_stable` by content, so a map holding one takes the
    // tag-1 field walk rather than the field-8 value arm.
    let var_entry = models::rust::utils::new_boundvar_par(1, Vec::new(), false);
    let ground_entry = make_list_of(vec![make_string_par("a")]);

    let forward = ground_map(vec![var_entry.clone(), ground_entry.clone()]);
    let backward = ground_map(vec![ground_entry, var_entry]);

    // The premise, unchanged: these are genuinely non-ground, so neither took
    // the field-8 arm. A ground map's encoding starts with the field-8 key
    // (tag 8, length-delimited = 0x42); a non-ground one starts with tag 1
    // (0x0a). Without this the measurement could pass vacuously by drifting
    // onto the ground path.
    let forward_bytes = prost_bytes(&forward);
    let backward_bytes = prost_bytes(&backward);
    assert_eq!(
        forward_bytes.first(),
        Some(&0x0au8),
        "the fixture must exercise the TAG-1 arm, not the field-8 value arm"
    );

    // ★ THE MEASUREMENT, INVERTED: one entry set, two orders, ONE encoding.
    assert_eq!(
        forward_bytes, backward_bytes,
        "the non-ground arm is routed through the trie now — two orders of one \
         entry set are one value on the wire"
    );

    // …and the comparators agree with the wire, which is the whole point of the
    // identity family: they read the same projection the encoder writes.
    assert_eq!(
        forward, backward,
        "the comparators must not disagree with the wire in the non-ground arm"
    );
    assert_eq!(hash_of(&forward), hash_of(&backward), "…nor must the hash");
    assert_eq!(
        serde_bytes(&forward),
        serde_bytes(&backward),
        "…nor the event-hash preimage"
    );

    // ★ THE POSITIVE CONTROL. Equality above must not come from the encoder
    // having stopped distinguishing non-ground maps at all: a genuinely
    // DIFFERENT non-ground entry set must still differ, in the same arm.
    let different = ground_map(vec![
        models::rust::utils::new_boundvar_par(2, Vec::new(), false),
        make_list_of(vec![make_string_par("a")]),
    ]);
    let different_bytes = prost_bytes(&different);
    assert_eq!(
        different_bytes.first(),
        Some(&0x0au8),
        "the control must exercise the same TAG-1 arm"
    );
    assert_ne!(
        forward_bytes, different_bytes,
        "positive control: a different non-ground entry set must still encode differently"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// ★★ MULTIPLICITY — the question the `epathmap_spliced_event_bytes` proptest
//    counterexamples ask, answered by measurement and with a positive control
// ─────────────────────────────────────────────────────────────────────────────

/// ★★ **`{| false, false |}` and `{| false |}` are the same GROUND map, and they
/// were before this change too.**
///
/// `models/tests/epathmap_spliced_event_bytes.proptest-regressions` holds
/// exactly this pair as two shrunk counterexamples. They look like a question
/// this change has to answer — *"does making the trie the field collapse
/// duplicate entries, and does that move the event-hash preimage?"* — and for
/// GROUND maps the answer is that the collapse was already there, in the
/// encoder, before the entries moved.
///
/// # The documentary half
///
/// At `ab1908e0` (the commit this change is built on), a ground map's prost
/// encoding was
///
/// ```text
/// encode_raw            → encode_ground_field8(&ground_path_stream(&self.ps), buf)
/// ground_path_stream(ps) = path_stream_of(&create_pathmap_from_elements(ps, None).map)
/// ```
///
/// — a trie insertion per entry, and `PathMap::insert` is idempotent — and its
/// serde / event-hash preimage was `ground_canonical_ps`, which is
/// `canonical_ps_from_trie` over the same trie. **Both were already deduplicated.**
///
/// # The measured half (this test)
///
/// The reproduction below calls `create_pathmap_from_elements` and the zipper
/// walk directly — the two functions that composed `ground_path_stream`, neither
/// touched by this change — so it measures the pre-change encoder rather than
/// quoting it, and then checks the current encoder agrees.
///
/// # ⚠ WHAT IS ACTUALLY NEW, AND IT IS NOT THIS
///
/// The change extends deduplication to **NON-ground** maps, which genuinely did
/// preserve duplicates (their only encoding was tag 1 `repeated Par` in `ps`
/// order). That is consensus-visible and is the same motion as the
/// order-insensitivity: see
/// `a_non_ground_map_now_encodes_as_an_ORDER_INSENSITIVE_LIST`.
///
/// ★ **For the matcher lane (task #125), the answer to *"is `{| 1, 1 |}` the
/// same pattern as `{| 1 |}`?"* is YES, uniformly, and it is a property of the
/// representation rather than of either lane's code** — a trie has one slot per
/// key, so no consumer has to remember to dedup and none can disagree about it.
#[test]
fn a_ground_map_has_never_distinguished_multiplicity() {
    let gbool_false = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GBool(false)),
        }],
        ..Default::default()
    };

    // ── the pre-change encoder, reproduced from its own parts ────────────────
    let once_stream = path_stream(std::slice::from_ref(&gbool_false));
    let twice_stream = path_stream(&[gbool_false.clone(), gbool_false.clone()]);
    assert_eq!(
        once_stream, twice_stream,
        "the PRE-change ground encoder already deduplicated: `ground_path_stream` \
         was `path_stream_of(create_pathmap_from_elements(ps))`, and a trie \
         insertion is idempotent"
    );

    // ── the current encoder agrees, on every surface ─────────────────────────
    let once = ground_map(vec![gbool_false.clone()]);
    let twice = ground_map(vec![gbool_false.clone(), gbool_false.clone()]);

    // The premise: this really is the field-8 arm (tag 8, length-delimited =
    // 0x42), not the tag-1 list — otherwise the case is not the one the
    // counterexamples name.
    let once_bytes = prost_bytes(&once);
    assert_eq!(
        once_bytes.first(),
        Some(&0x42u8),
        "the fixture must exercise the FIELD-8 ground arm"
    );

    assert_eq!(once_bytes, prost_bytes(&twice), "prost bytes");
    assert_eq!(serde_bytes(&once), serde_bytes(&twice), "event-hash preimage");
    assert_eq!(hash_of(&once), hash_of(&twice), "hash");
    assert_eq!(once, twice, "==");
    assert_eq!(
        once.ps().len(),
        1,
        "the duplicate is absorbed, not carried"
    );

    // ★ THE POSITIVE CONTROL. None of the above may come from the encoder
    // having stopped separating ground maps: a map with a genuinely different
    // entry set must still differ on every one of those surfaces.
    let gbool_true = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GBool(true)),
        }],
        ..Default::default()
    };
    let different = ground_map(vec![gbool_false, gbool_true]);
    assert_ne!(once_bytes, prost_bytes(&different), "control: prost bytes");
    assert_ne!(
        serde_bytes(&once),
        serde_bytes(&different),
        "control: event-hash preimage"
    );
    assert_ne!(once, different, "control: ==");
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ The O(1) ground fold is EXACT — it selects the wire arm
// ─────────────────────────────────────────────────────────────────────────────

/// ★ `EntryTrie::entries_stable` is folded forward as entries arrive rather than
/// derived from the projection, and this pins that the shortcut is EXACT.
///
/// It has to be. The fold is the entry half of `eval_stable_epathmap`, which is
/// what `encode_raw` consults to choose between proto field 8 (`U(m)`) and the
/// tag-1 field walk. A fold that erred conservatively — `false` where the truth
/// is `true` — would move a ground map onto the wrong arm, which is a
/// consensus-visible byte change dressed as caution.
///
/// The reason it is folded at all is that the alternative is not free: deriving
/// it would force the entry projection on the *encode* path, and a ground map's
/// encoding needs only the trie's key stream, never the decoded entries.
///
/// Exhaustive over all `2^9 = 512` subsets of the alphabet, in three
/// constructions each, plus a deliberately non-ground control so the agreement
/// cannot be vacuous.
#[test]
fn the_o1_ground_fold_agrees_with_the_projection_on_every_subset() {
    let mut saw_ground = 0usize;
    let mut saw_non_ground = 0usize;

    for subset in every_subset_of_the_alphabet() {
        let mut duplicated = subset.clone();
        duplicated.extend(subset.iter().cloned());
        let mut reversed = subset.clone();
        reversed.reverse();

        for entries in [subset.clone(), reversed, duplicated] {
            let map = ground_map(entries);
            let folded = map.entry_trie().entries_stable();
            let derived = map
                .ps()
                .iter()
                .all(models::rust::pathmap_crate_type_mapper::eval_stable_par_for_test);
            assert_eq!(
                folded, derived,
                "the O(1) fold must equal the projection walk, exactly"
            );
            assert_eq!(
                map.entry_trie().len(),
                map.ps().len(),
                "the O(1) length must equal the projection's"
            );
            assert_eq!(
                map.entry_trie().is_empty(),
                map.ps().is_empty(),
                "the O(1) emptiness must equal the projection's"
            );
            if folded {
                saw_ground += 1;
            } else {
                saw_non_ground += 1;
            }
        }
    }

    // Positive control on the `true` side: the alphabet is entirely ground, so
    // every subset folds `true` and the loop above would agree vacuously if the
    // fold were hard-wired. Feed it something that is NOT ground.
    let non_ground = ground_map(vec![
        models::rust::utils::new_boundvar_par(1, Vec::new(), false),
        make_int_par(1),
    ]);
    assert!(
        !non_ground.entry_trie().entries_stable(),
        "positive control: the fold must be able to answer `false`"
    );
    saw_non_ground += 1;

    assert!(saw_ground > 0, "the corpus must reach the ground arm");
    assert!(
        saw_non_ground > 0,
        "…and the non-ground arm, or the agreement is one-sided"
    );
}
