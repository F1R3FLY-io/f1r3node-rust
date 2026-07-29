use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::pathmap_crate_type_mapper::PathMapCrateTypeMapper;
use models::rust::pathmap_integration::{
    create_pathmap_from_elements, par_to_path, render_trie_entry_divergences, segments_to_key,
    trie_entry_divergences, RholangPathMap,
};

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

fn make_list_par(elements: Vec<&str>) -> Par {
    let ps: Vec<Par> = elements.iter().map(|s| make_string_par(s)).collect();
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

#[test]
fn test_create_empty_pathmap() {
    let result = create_pathmap_from_elements(&[], None);
    assert!(result.map.is_empty());
    assert!(!result.connective_used);
    assert!(result.locally_free.is_empty());
}

#[test]
fn test_create_pathmap_single_element() {
    let par = make_list_par(vec!["books", "fiction", "gatsby"]);
    let result = create_pathmap_from_elements(&[par.clone()], None);
    assert!(!result.map.is_empty());
}

#[test]
fn test_pathmap_union() {
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["c", "d"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let union = map1.map.join(&map2.map);
    assert_eq!(union.val_count(), 2);
}

#[test]
fn test_pathmap_intersection() {
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "b"]);
    let par3 = make_list_par(vec!["c", "d"]);

    let map1 = create_pathmap_from_elements(&[par1, par3], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let intersection = map1.map.meet(&map2.map);
    assert_eq!(intersection.val_count(), 1);
}

#[test]
fn test_pathmap_subtraction() {
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "c"]);
    let par3 = make_list_par(vec!["a", "b"]);

    let map1 = create_pathmap_from_elements(&[par1, par2], None);
    let map2 = create_pathmap_from_elements(&[par3], None);

    let diff = map1.map.subtract(&map2.map);
    // Should have only ["a", "c"] remaining
    assert_eq!(diff.val_count(), 1);
}

#[test]
fn test_pathmap_restriction() {
    let par1 = make_list_par(vec!["books", "fiction", "gatsby"]);
    let par2 = make_list_par(vec!["books", "fiction", "moby"]);
    let par3 = make_list_par(vec!["books", "nonfiction", "history"]);
    let prefix = make_list_par(vec!["books", "fiction"]);

    let map = create_pathmap_from_elements(&[par1, par2, par3], None);
    // W2b-1 (why bytes moved): PathMap::restrict is a PREFIX/subtrie op; under
    // the codec a prefix is the NON-terminated segment concatenation (the FULL
    // encode_trie_path key terminates with 0x00 and is thus prefix-free of the
    // longer keys, degenerating restrict to exact-match). Build the restricting
    // map with prefix keys — mirroring the production `restriction` method
    // (reduce.rs) — so restrict prefix-matches and yields the 2 fiction books.
    let mut prefix_map = RholangPathMap::new();
    prefix_map.insert(segments_to_key(&par_to_path(&prefix), false), prefix.clone());

    let restricted = map.map.restrict(&prefix_map);
    // Should have only the 2 fiction books
    assert_eq!(restricted.val_count(), 2);
}

/// The entries of `elements` in TRIE ORDER (ascending codec key) and deduped —
/// the order and multiplicity every trie reader reports, and therefore the
/// expected `ps` of any `EPathMap` read back out of a trie built from them.
///
/// Computed from the codec, not from the readers under test, so it is an
/// INDEPENDENT expectation rather than a restatement of the implementation.
fn expected_entries_in_trie_order(elements: &[Par]) -> Vec<Par> {
    let mut keyed: Vec<(Vec<u8>, Par)> = elements
        .iter()
        .map(|par| (encode_trie_path(par), par.clone()))
        .collect();
    keyed.sort_by(|(left, _), (right, _)| left.cmp(right));
    keyed.dedup_by(|(left, _), (right, _)| left == right);
    keyed.into_iter().map(|(_, par)| par).collect()
}

/// The conversion is asserted by CONTENT, over a fixture holding both codec
/// arms. The retired `assert_eq!(ps.len(), 2)` would have passed with every
/// entry replaced by a different Par, and — the case that matters — with two
/// distinct keys carrying ONE shared value, which is precisely how entries get
/// lost (see [`trie_entry_divergences`] and the `setSubtrie` regression in
/// `rholang/tests/trie_entry_invariant_spec.rs`).
#[test]
fn test_pathmap_to_e_pathmap_conversion() {
    let original_ps = mixed_elements();
    let map = create_pathmap_from_elements(&original_ps, None);

    let e_pathmap = PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
        &map.map,
        map.connective_used,
        &map.locally_free,
        None,
    );

    assert_eq!(
        &e_pathmap.ps()[..],
        &expected_entries_in_trie_order(&original_ps)[..],
        "the converter must return the ENTRIES, in trie order — not merely the \
         right number of them"
    );
    // …and no two of them are the same entry, which `ps.len()` cannot see.
    let keys: Vec<Vec<u8>> = e_pathmap.ps().iter().map(encode_trie_path).collect();
    let mut distinct = keys.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        keys.len(),
        distinct.len(),
        "every returned entry must be a DISTINCT entry; duplicates here mean \
         distinct trie keys shared one value and the map has already lost \
         entries (they collapse on the next re-insertion)"
    );
}

/// ★ A test named "roundtrip" must compare CONTENTS. This one previously
/// asserted `e_pathmap2.ps.len() == e_pathmap1.ps.len()`, which is true of any
/// two maps of equal size — including one whose entries are all the same Par.
///
/// The round trip asserted here is the real one, on a fixture holding BOTH
/// codec arms (bare `1`, its singleton list `[1]`, bare `"a"`, and
/// `["a","x"]` — see `mixed_elements`):
///
/// ```text
///   EPathMap ──e_pathmap_to_rholang_pathmap──▶ trie ──rholang_pathmap_to_e_pathmap──▶ EPathMap
/// ```
///
/// is the identity on the ENTRY SET, and normalizes only the ORDER (to trie
/// order). Both legs are checked, and the trie in the middle is checked to
/// uphold the entry invariant.
#[test]
fn test_e_pathmap_roundtrip() {
    let original_ps = mixed_elements();

    // EPathMap fix P3 (PM-2): constructor instead of a struct literal
    // (the wrapper's shadow cell is private).
    let e_pathmap1 = EPathMap::new(original_ps.clone(), vec![], false, None);

    let result = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&e_pathmap1);
    assert_eq!(result.map.val_count(), original_ps.len());

    // Leg 1 — every entry went in under its OWN key, and no other.
    assert!(
        trie_entry_divergences(&result.map).is_empty(),
        "the trie in the middle of the round trip must uphold the entry \
         invariant: {}",
        render_trie_entry_divergences(&trie_entry_divergences(&result.map))
    );
    for element in &original_ps {
        assert_eq!(
            result.map.get(encode_trie_path(element)),
            Some(element),
            "each entry is readable at its own key inside the trie"
        );
    }

    let e_pathmap2 = PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
        &result.map,
        result.connective_used,
        &result.locally_free,
        None,
    );

    // Leg 2 — the ENTRIES come back, in trie order.
    assert_eq!(
        &e_pathmap2.ps()[..],
        &expected_entries_in_trie_order(&original_ps)[..],
        "the round trip must preserve the entries themselves"
    );

    // …and it is a FIXED POINT: a second lap moves nothing.
    let third = PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(
        &PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&e_pathmap2).map,
        false,
        &[],
        None,
    );
    assert_eq!(
        third.ps(), e_pathmap2.ps(),
        "trie order is already canonical — a second round trip is the identity"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ★ THE ENTRY INVARIANT AS A PROPERTY — over every trie this crate can build
// ═══════════════════════════════════════════════════════════════════════════
//
// `RholangPathMap` is not a general map: a value is a redundant mirror of its
// own key (`create_pathmap_from_elements` inserts
// `(encode_trie_path(par), par)`), and the whole read side depends on that.
// This section enumerates the element alphabet EXHAUSTIVELY — deterministic,
// so there is no regressions file and no seed to lose — and asserts the
// invariant, plus the two consequences that make it matter: the KEY-side and
// VALUE-side readers agree, and the entry count survives re-insertion.
//
// The interpreter-level twin, which is the one that goes red on a producer
// that files a value under a key it does not encode to, is
// `rholang/tests/trie_entry_invariant_spec.rs`.

/// The element alphabet: both codec arms, at both arities that can collide,
/// plus the nesting and sign edges. `1` and `[1]` differ by exactly the
/// terminator; `"a"` is a strict byte-prefix of `["a","x"]`.
fn alphabet() -> Vec<Par> {
    vec![
        make_int_par(1),
        make_int_par(-7),
        make_string_par("a"),
        make_string_par(""),
        make_list_of(vec![]),
        make_list_of(vec![make_int_par(1)]),
        make_list_of(vec![make_string_par("a")]),
        make_list_par(vec!["a", "x"]),
        make_list_of(vec![
            make_list_of(vec![make_int_par(2)]),
            make_string_par("q"),
        ]),
    ]
}

/// Every subset of the alphabet — `2^9 = 512` element sets, each built into a
/// trie and checked. Exhaustive over the alphabet, so no case is left to a
/// generator's luck.
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

/// ★ THE ONE DELIBERATE EXCEPTION, pinned — `restriction`'s prefix map.
///
/// `reduce.rs`'s `restriction` method builds a second map whose keys are
/// NON-terminated prefixes (`segments_to_key(.., false)`) paired with whole
/// entry values, because `PathMap::restrict` is a prefix/subtrie operation and
/// terminated keys would degenerate it to exact match. Those pairs violate the
/// entry invariant by construction.
///
/// That is safe for exactly ONE reason: `restrict` takes its result's VALUES
/// from the BASE map and uses the restricting map only for its PATHS, so the
/// prefix map's values never reach
/// [`PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap`]. Nothing in the
/// tree stated that dependency, so a change in `restrict`'s value provenance
/// would have turned a documented exception into a live divergence silently.
///
/// The `SENTINEL` below is a value that could not possibly belong at that key.
/// If it ever appears in a restriction result, this test names the reason.
#[test]
fn restrict_takes_its_values_from_the_base_so_the_prefix_map_never_escapes() {
    let kept = make_list_par(vec!["books", "fiction"]);
    let dropped = make_list_par(vec!["movies", "action"]);
    let base = create_pathmap_from_elements(&[kept.clone(), dropped], None);

    let prefix = make_list_par(vec!["books"]);
    let sentinel = make_string_par("SENTINEL — a value the base never held");
    let mut prefix_map = RholangPathMap::new();
    prefix_map.insert(
        segments_to_key(&par_to_path(&prefix), false),
        sentinel.clone(),
    );

    // The prefix map is a KNOWN divergence — stated, so the exception is
    // visible rather than merely absent from the checker's inputs.
    assert_eq!(
        trie_entry_divergences(&prefix_map).len(),
        1,
        "the prefix map is deliberately NOT an entry map"
    );

    let restricted = base.map.restrict(&prefix_map);

    assert_eq!(restricted.val_count(), 1, "one book under the prefix");
    for (_, value) in restricted.iter() {
        assert_ne!(
            value, &sentinel,
            "★ `restrict` took a value from the RESTRICTING map — the prefix \
             map's deliberate divergence now escapes into `restriction`'s \
             result and reaches the value-side converter"
        );
    }
    assert!(
        trie_entry_divergences(&restricted).is_empty(),
        "a restriction result is an ENTRY map and must uphold the invariant: {}",
        render_trie_entry_divergences(&trie_entry_divergences(&restricted))
    );
    assert_eq!(
        restricted.get(encode_trie_path(&kept)),
        Some(&kept),
        "the surviving entry is the base's own, at the base's own key"
    );
}

/// ★ THE PROPERTY: for every `(k, v)` in every trie built from program
/// elements, `encode_trie_path(v) == k`.
#[test]
fn every_trie_this_crate_builds_upholds_the_entry_invariant() {
    for subset in every_subset_of_the_alphabet() {
        let built = create_pathmap_from_elements(&subset, None);
        let divergences = trie_entry_divergences(&built.map);
        assert!(
            divergences.is_empty(),
            "entry invariant violated for {} elements:{}",
            subset.len(),
            render_trie_entry_divergences(&divergences)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ★ THE READ-SIDE IMAGE INVARIANT — the dual, over every reader key
// ═══════════════════════════════════════════════════════════════════════════
//
// The property above checks PRODUCERS: every value is filed under the key it
// encodes to. It cannot see a defect in which the trie is perfect and the
// READER asks for a key no Par could ever be filed under — which is exactly
// what defect #108 was. The dual property is
//
//     ∀ entry keys k a reader constructs .  decode_trie_path(k) ∈ Ok
//
// and it is decidable, exhaustively, because a reader's key is a function of
// (cursor Par, relative Par) alone.
//
// The stronger statement proved below is not merely membership of the image
// but WHICH element of the image: the key a reader builds below the root is
// the key the codec gives the COMPOSED path. That is the read-side dual of the
// repair that made `setSubtrie`'s key and value one route — here the reader's
// key and the writer's key become one route, so they cannot drift apart.

/// The elements of a Par READ AS A PATH: a split-arm carrier contributes its
/// own elements, and every other Par contributes ITSELF as a single element.
///
/// Built from `takes_split_arm` — the codec's own classifier, the one authority
/// — but independently of `par_to_path`/`segments_to_key`, so the expectation
/// below is not the implementation restated.
fn path_elements(par: &Par) -> Vec<Par> {
    use models::rust::canonical_path::takes_split_arm;
    match takes_split_arm(par) {
        true => match par.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EListBody(list)) => list.ps.clone(),
            _ => unreachable!("a split-arm carrier is an EList carrier"),
        },
        false => vec![par.clone()],
    }
}

/// ★ THE READ-SIDE PROPERTY, exhaustively: over all `2^9 = 512` maps and all
/// `9 × 9 = 81` (cursor, relative) pairs from the alphabet — 41 472 reads —
/// every key `entry_key_at` builds is in the codec image, IS the codec's key
/// for the path it names, and reads back exactly the entry that path names.
///
/// The three legs are separately load-bearing:
///
/// 1. **In the image.** A key outside it can never hit, on any map. This is the
///    leg that goes red on #108 and on every future member of its family.
/// 2. **The codec's own key.** Membership alone would admit a key that is in
///    the image but names the WRONG entry — which is precisely the shape of the
///    bare/singleton-list defect on the root arm (`03 0a` versus `03 0a 00`,
///    both canonical, different entries). Naming the composed path pins which.
/// 3. **Reads back.** The end-to-end statement a program experiences.
#[test]
fn every_reader_key_is_in_the_codec_image() {
    use models::rust::pathmap_integration::{entry_key_at, entry_key_is_in_codec_image};

    for subset in every_subset_of_the_alphabet() {
        let map = create_pathmap_from_elements(&subset, None).map;

        for cursor_par in alphabet() {
            let cursor = par_to_path(&cursor_par);

            for relative in alphabet() {
                let key = entry_key_at(&cursor, &relative, &map);

                // LEG 1 — in the image of `encode_trie_path`.
                assert!(
                    entry_key_is_in_codec_image(&key),
                    "★ a reader built a key in the image of no Par, so it can \
                     never hit: cursor = {cursor_par:?}, relative = \
                     {relative:?}, key = {key:02x?}"
                );

                // LEG 2 — WHICH element of the image: at the root the argument
                // IS the whole path; below it the composed path is the LIST of
                // the cursor's elements followed by the relative's.
                let named = match cursor.is_empty() {
                    true => relative.clone(),
                    false => {
                        let mut elements = path_elements(&cursor_par);
                        elements.extend(path_elements(&relative));
                        make_list_of(elements)
                    }
                };
                assert_eq!(
                    key,
                    encode_trie_path(&named),
                    "the reader's key must be the codec's key for the path it \
                     names: cursor = {cursor_par:?}, relative = {relative:?}"
                );

                // LEG 3 — and it reads back that entry, exactly when the map
                // holds it.
                let expected = subset.iter().find(|element| **element == named);
                assert_eq!(
                    map.get(&key),
                    expected,
                    "the reader's key must read back the entry it names: \
                     cursor = {cursor_par:?}, relative = {relative:?}"
                );
            }
        }
    }
}

/// ⚠ ANTI-VACUITY for leg 1 — the invariant REJECTS something. A `Bare` cursor
/// is inhabited at exactly one depth, because a bare entry's key is exactly one
/// segment: at depth 0 the key is empty and at depth ≥ 2 it is an unterminated
/// multi-segment concatenation, and `decode_trie_path` rejects both.
///
/// Without this, "every reader key is in the image" could hold because every
/// key is in the image, and the guard inside `cursor_entry_key` would be
/// checking a tautology.
#[test]
fn the_image_invariant_rejects_a_bare_cursor_at_any_other_depth() {
    use models::rust::canonical_path::CodecError;
    use models::rust::pathmap_integration::{entry_key_is_in_codec_image, segments_to_key};

    let one = par_to_path(&make_string_par("a")); // 04 01 61
    let two = {
        let mut segments = one.clone();
        segments.extend(par_to_path(&make_int_par(1))); // 03 02
        segments
    };

    // Depth 1 — INHABITED: this is the key of the bare entry `"a"`, and the
    // reason `cursor_entry_key` must keep taking a `CursorKind`.
    let depth_one = segments_to_key(&one, false);
    assert_eq!(depth_one, vec![0x04, 0x01, 0x61]);
    assert!(entry_key_is_in_codec_image(&depth_one));
    assert_eq!(
        models::rust::canonical_path::decode_trie_path(&depth_one),
        Ok(make_string_par("a"))
    );

    // Depth 2 — UNINHABITED: `04 01 61 03 02` is in the image of no Par.
    let depth_two = segments_to_key(&two, false);
    assert_eq!(depth_two, vec![0x04, 0x01, 0x61, 0x03, 0x02]);
    assert!(
        !entry_key_is_in_codec_image(&depth_two),
        "★ an unterminated multi-segment key must be rejected"
    );
    assert_eq!(
        models::rust::canonical_path::decode_trie_path(&depth_two),
        Err(CodecError::UnterminatedMultiSegment)
    );
    // …and the terminated key at the same segments IS inhabited, so the two
    // differ by exactly the terminator the composition law appends.
    let terminated = segments_to_key(&two, true);
    assert!(entry_key_is_in_codec_image(&terminated));

    // Depth 0 — UNINHABITED: the empty key. (The root-key corollary of the
    // write-side invariant, reached from the read side.)
    assert!(
        !entry_key_is_in_codec_image(&[]),
        "the empty key names no entry"
    );
}

/// The composition law itself, stated on its own: the arm is the ARGUMENT's at
/// the root and `Split` everywhere below, for every Par in the alphabet.
#[test]
fn the_composition_law_takes_the_argument_arm_only_at_the_root() {
    use models::rust::pathmap_integration::{composed_cursor_kind, CursorKind};

    for relative in alphabet() {
        assert_eq!(
            composed_cursor_kind(&[], &relative),
            CursorKind::of(&relative),
            "at the root the composed path IS the argument"
        );

        for cursor_par in alphabet() {
            let cursor = par_to_path(&cursor_par);
            if cursor.is_empty() {
                continue; // `[]` — the cursor contributes nothing, so it IS the root
            }
            assert_eq!(
                composed_cursor_kind(&cursor, &relative),
                CursorKind::Split,
                "below the root every composed path is a list: cursor = \
                 {cursor_par:?}, relative = {relative:?}"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ★ THE RETIRED RULE IS UNSPELLABLE
// ─────────────────────────────────────────────────────────────────────────────

/// `models/src/rust/pathmap_zipper.rs`, relative to this package.
const ZIPPER_MODULE: &str = "src/rust/pathmap_zipper.rs";

/// The retired rule, as the exact expression that encoded it: append the
/// split-list terminator to the concatenated segments **unconditionally**.
const RETIRED_RULE: &str = "segments_to_key(segments, true)";

/// The two methods that spelled it. Nothing named `descend_to` may be defined
/// in that module: a cursor move there can only be re-derived from the path's
/// segments, which is the derivation the rule got wrong.
const RETIRED_MOVE: &str = "fn descend_to";

/// A token that must be PRESENT, so the gate cannot be satisfied by the file
/// having been renamed, emptied, or deleted.
const MODULE_ANCHOR: &str = "pub fn decode_cursor";

fn zipper_module_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ZIPPER_MODULE);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// ★ **THE GATE.** The retired unconditional-terminate rule cannot be written
/// in `pathmap_zipper.rs`, because the helper that *was* that rule
/// (`flatten_segments`) and both methods that called it are deleted.
///
/// # Why a source-level assertion, and why pinned to these two tokens
///
/// The defect was not a wrong value produced by live code — it was a **retired
/// law kept in executable form, with no caller**, waiting for the next author
/// to reach for it. There is no input that makes dead code wrong, so no
/// behavioural test can hold it down; the property to preserve is *absence*.
///
/// The tokens are pinned rather than the whole file compared, because a
/// whole-file `assert_ne!` against a snapshot would survive any incidental edit
/// — every comment fix would have to be re-blessed, so it would be turned off
/// within a month. These two tokens are the rule itself and the only shape that
/// has ever carried it.
///
/// # What the rule got wrong, measured
///
/// On `{| 5, [5] |}` the codec files `5` under `03 0a` and `[5]` under
/// `03 0a 00`. The retired rule built `03 0a 00` for BOTH, so
/// `readZipper().descendTo(5)` answered `[5]` — a valid canonical key naming a
/// DIFFERENT element the same map holds. The corrected law is
/// [`entry_key_at`](models::rust::pathmap_integration::entry_key_at), whose
/// answers on those two rows are asserted below.
#[test]
fn the_retired_unconditional_terminate_rule_is_unspellable() {
    let source = zipper_module_source();

    // ── the gate is not vacuous: the file is there and is the right file ──
    assert!(
        source.contains(MODULE_ANCHOR),
        "{ZIPPER_MODULE} does not contain {MODULE_ANCHOR:?}, so this gate is \
         asserting the absence of tokens from a file it did not find. Fix the \
         anchor before trusting the two assertions below."
    );

    // ── the mutation, asserted to have applied ──
    assert!(
        !source.contains(RETIRED_RULE),
        "★ {ZIPPER_MODULE} spells the RETIRED rule {RETIRED_RULE:?}. It appends \
         the split-list terminator unconditionally, so for a bare (non-list) \
         path it builds the key of the SINGLETON LIST — a well-formed canonical \
         key naming a DIFFERENT element. Use \
         `pathmap_integration::entry_key_at` / `cursor_entry_key`, which spend \
         the split/bare discriminator instead of guessing it."
    );
    assert!(
        !source.contains(RETIRED_MOVE),
        "★ {ZIPPER_MODULE} defines {RETIRED_MOVE:?}. A cursor move on a zipper \
         wrapper cannot be derived from the path's segments alone; it must take \
         `(cursor_segments, path_par, map)` and call \
         `pathmap_integration::entry_key_at`, which is a DIFFERENT signature \
         from the deleted `(&mut self, path: &Par)`."
    );

    // ── ★ THE GATE CAN GO RED. A checker nobody has watched fail is not
    //    evidence: the same predicate, applied to the deleted text, rejects it.
    let as_it_was = "pub(crate) fn flatten_segments(segments: &[Vec<u8>]) -> Vec<u8> {\n    \
                     segments_to_key(segments, true)\n}\n\
                     pub fn descend_to(&mut self, path: &Par) -> Result<(), String> {}\n\
                     pub fn decode_cursor() {}";
    assert!(
        as_it_was.contains(RETIRED_RULE) && as_it_was.contains(RETIRED_MOVE),
        "the gate's own tokens no longer match the text they were written to \
         reject, so the two assertions above are passing vacuously"
    );
}

/// The behavioural half the gate replaces, kept for the arm it can still make:
/// on `{| 5, [5] |}` the corrected law separates the two entries, and the
/// SPLIT row — where the retired rule and the corrected law agree — is the
/// CONTROL that must not move.
///
/// ⚠ ANTI-VACUITY. Both entries are present, so `map.get(k).is_some()` holds
/// for either key and proves nothing. Every assertion here is on the decoded
/// `Par`. (This is the vacuity mode `entry_key_below_the_root_still_rebuilds`
/// fell into: it drove only the row where the two laws agree.)
#[test]
fn the_corrected_law_separates_the_bare_element_from_the_singleton_list() {
    use models::rust::pathmap_integration::entry_key_at;

    let five = make_int_par(5);
    let list_five = make_list_of(vec![make_int_par(5)]);
    let map = &create_pathmap_from_elements(&[five.clone(), list_five.clone()], None).map;

    let key_of_five = entry_key_at(&[], &five, map);
    let key_of_list_five = entry_key_at(&[], &list_five, map);

    // The measured keys, pinned: the rows differ by exactly the terminator the
    // retired rule appended unconditionally.
    assert_eq!(key_of_five, vec![0x03, 0x0a], "the BARE entry `5`");
    assert_eq!(
        key_of_list_five,
        vec![0x03, 0x0a, 0x00],
        "the SPLIT entry `[5]` — one terminator longer, and a different entry"
    );
    assert_ne!(
        key_of_five, key_of_list_five,
        "the fixture is vacuous unless `5` and `[5]` are two DIFFERENT entries"
    );

    // ── the DISCRIMINATING row: a bare argument names the bare entry ──
    assert_eq!(
        map.get(&key_of_five),
        Some(&five),
        "★ the bare argument must reach `5`, not the singleton list `[5]` that \
         the same map also holds"
    );

    // ── the CONTROL: the split row, where the two laws agree, is unmoved ──
    assert_eq!(
        map.get(&key_of_list_five),
        Some(&list_five),
        "CONTROL: the split arm answers `[5]` under the corrected law exactly as \
         it did under the retired one; a fix that moves this row is over-reaching"
    );
}

/// The FIRST consequence: the bulk converter (`rholang_pathmap_to_e_pathmap`)
/// and a hand-rolled `to_next_val` walk that decodes keys report the same
/// entries, in the same order, on every one of the 512 maps.
///
/// ⚠ This test predates the converter's move to the key side, when it was a
/// statement about two INDEPENDENT readers agreeing. It is now a statement that
/// the converter IS the key walk — weaker as a differential, and still the leg
/// that catches the converter being re-pointed at the values or acquiring its
/// own traversal. The independent statement it used to make is now made by
/// construction, which is the point; the residual value-side reader is the point
/// LOOKUP, covered by `the_point_lookup_value_agrees_with_the_key_when_the_invariant_holds`
/// in `models/src/rust/pathmap_crate_type_mapper.rs`.
#[test]
fn the_bulk_converter_is_the_key_walk_on_every_subset() {
    use pathmap::zipper::{ZipperIteration, ZipperMoving};

    for subset in every_subset_of_the_alphabet() {
        let built = create_pathmap_from_elements(&subset, None);

        let converted =
            PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(&built.map, false, &[], None);
        let by_converter = converted.ps();

        let mut by_key = Vec::new();
        let mut rz = built.map.read_zipper();
        while rz.to_next_val() {
            by_key.push(
                models::rust::canonical_path::decode_trie_path(rz.path())
                    .expect("a trie key built by the codec decodes"),
            );
        }

        assert_eq!(
            &by_converter[..],
            &by_key[..],
            "the bulk converter is no longer the key walk on a {}-element map",
            subset.len()
        );
    }
}

/// The SECOND consequence, and the one that costs entries: the number of
/// DISTINCT entries survives a conversion + re-insertion. A trie in which two
/// distinct keys share one value converts to a `ps` of the right LENGTH whose
/// entries collapse on the way back in — which is why a `ps.len()` assertion
/// cannot see the defect at all.
#[test]
fn entry_count_survives_conversion_and_reinsertion_on_every_subset() {
    for subset in every_subset_of_the_alphabet() {
        let built = create_pathmap_from_elements(&subset, None);
        let distinct_keys = built.map.val_count();

        let converted =
            PathMapCrateTypeMapper::rholang_pathmap_to_e_pathmap(&built.map, false, &[], None);
        let rebuilt = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&EPathMap::new(
            converted.ps().clone(),
            vec![],
            false,
            None,
        ));

        assert_eq!(
            rebuilt.map.val_count(),
            distinct_keys,
            "a conversion followed by a re-insertion lost entries: {} distinct \
             keys went in, {} came back (ps.len() was {}, which is why a \
             cardinality assertion on `ps` proves nothing)",
            distinct_keys,
            rebuilt.map.val_count(),
            converted.ps().len()
        );
    }
}

#[test]
fn test_pathmap_connective_used() {
    let mut par = make_list_par(vec!["a", "b"]);
    par.connective_used = true;

    let result = create_pathmap_from_elements(&[par], None);
    assert!(result.connective_used);
}

#[test]
fn test_pathmap_locally_free() {
    let mut par = make_list_par(vec!["a", "b"]);
    par.locally_free = vec![1, 2, 3];

    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.locally_free, vec![1, 2, 3]);
}

#[test]
fn test_pathmap_remainder_sets_connective() {
    let par = make_list_par(vec!["a", "b"]);
    let remainder = models::rhoapi::Var {
        var_instance: Some(models::rhoapi::var::VarInstance::FreeVar(0)),
    };

    let result = create_pathmap_from_elements(&[par], Some(remainder));
    assert!(result.connective_used);
}

#[test]
fn test_multiple_elements_union() {
    let par1 = make_list_par(vec!["a"]);
    let par2 = make_list_par(vec!["b"]);
    let par3 = make_list_par(vec!["c"]);

    let result = create_pathmap_from_elements(&[par1, par2, par3], None);
    assert_eq!(result.map.val_count(), 3);
}

// ============ EDGE CASES ============

#[test]
fn test_intersection_disjoint_pathmaps() {
    // Intersection of completely disjoint PathMaps should be empty
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["c", "d"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let intersection = map1.map.meet(&map2.map);
    assert!(
        intersection.is_empty(),
        "Intersection of disjoint maps should be empty"
    );
}

#[test]
fn test_intersection_empty_with_nonempty() {
    // Intersection with empty PathMap should be empty
    let par = make_list_par(vec!["a", "b"]);
    let map1 = create_pathmap_from_elements(&[par], None);
    let map2 = create_pathmap_from_elements(&[], None);

    let intersection = map1.map.meet(&map2.map);
    assert!(
        intersection.is_empty(),
        "Intersection with empty map should be empty"
    );
}

#[test]
fn test_union_overlapping_keys() {
    // Union with overlapping keys - should keep both (or one, depending on semantics)
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "b"]); // Same path

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let union = map1.map.join(&map2.map);
    // Should have 1 element (paths are identical)
    assert_eq!(union.val_count(), 1);
}

#[test]
fn test_subtraction_empty_result() {
    // Subtracting all elements should result in empty map
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "b"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let diff = map1.map.subtract(&map2.map);
    assert!(
        diff.is_empty(),
        "Subtracting identical maps should result in empty map"
    );
}

#[test]
fn test_subtraction_from_empty() {
    // Subtracting from empty map should remain empty
    let par = make_list_par(vec!["a", "b"]);
    let map1 = create_pathmap_from_elements(&[], None);
    let map2 = create_pathmap_from_elements(&[par], None);

    let diff = map1.map.subtract(&map2.map);
    assert!(
        diff.is_empty(),
        "Subtracting from empty map should be empty"
    );
}

#[test]
fn test_subtraction_disjoint() {
    // Subtracting disjoint set should leave original unchanged
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["c", "d"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let diff = map1.map.subtract(&map2.map);
    assert_eq!(
        diff.val_count(),
        1,
        "Subtracting disjoint set should preserve original"
    );
}

#[test]
fn test_restriction_no_match() {
    // Restriction with non-matching prefix should be empty
    let par = make_list_par(vec!["books", "fiction", "gatsby"]);
    let prefix = make_list_par(vec!["movies"]); // Different prefix

    let map = create_pathmap_from_elements(&[par], None);
    let prefix_map = create_pathmap_from_elements(&[prefix], None);

    let restricted = map.map.restrict(&prefix_map.map);
    assert!(
        restricted.is_empty(),
        "Restriction with non-matching prefix should be empty"
    );
}

#[test]
fn test_restriction_exact_match() {
    // Restriction with exact path match
    let par = make_list_par(vec!["books", "fiction"]);
    let prefix = make_list_par(vec!["books", "fiction"]);

    let map = create_pathmap_from_elements(&[par], None);
    let prefix_map = create_pathmap_from_elements(&[prefix], None);

    let restricted = map.map.restrict(&prefix_map.map);
    // Should match since prefix equals the path
    assert!(!restricted.is_empty());
}

#[test]
fn test_empty_pathmap_operations() {
    // Operations on empty PathMaps
    let empty1 = create_pathmap_from_elements(&[], None);
    let empty2 = create_pathmap_from_elements(&[], None);

    let union = empty1.map.join(&empty2.map);
    assert!(union.is_empty(), "Union of empty maps should be empty");

    let intersection = empty1.map.meet(&empty2.map);
    assert!(
        intersection.is_empty(),
        "Intersection of empty maps should be empty"
    );

    let diff = empty1.map.subtract(&empty2.map);
    assert!(diff.is_empty(), "Subtraction of empty maps should be empty");
}

#[test]
fn test_single_segment_paths() {
    // PathMaps with single-segment paths
    let par1 = make_list_par(vec!["a"]);
    let par2 = make_list_par(vec!["b"]);

    let map1 = create_pathmap_from_elements(&[par1], None);
    let map2 = create_pathmap_from_elements(&[par2], None);

    let union = map1.map.join(&map2.map);
    assert_eq!(union.val_count(), 2);
}

#[test]
fn test_deep_nested_paths() {
    // Very deep nested paths
    let par = make_list_par(vec!["a", "b", "c", "d", "e", "f", "g", "h"]);
    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.map.val_count(), 1);
}

#[test]
fn test_duplicate_elements() {
    // Adding duplicate elements
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["a", "b"]); // Duplicate

    let result = create_pathmap_from_elements(&[par1, par2], None);
    // Should have 1 element (duplicates merged)
    assert_eq!(result.map.val_count(), 1);
}

#[test]
fn test_non_list_par() {
    // Non-list Par (single string) should work too
    let par = make_string_par("simple");
    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.map.val_count(), 1);
}

#[test]
fn test_mixed_list_and_nonlist() {
    // Mix of list and non-list Pars
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_string_par("simple");

    let result = create_pathmap_from_elements(&[par1, par2], None);
    assert_eq!(result.map.val_count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// ★ THE MIXED FIXTURE — both codec arms in one map
// ═══════════════════════════════════════════════════════════════════════════
//
// `encode_trie_path` (`canonical_path.rs`, §1.3) is a TWO-ARM codec:
//
//   split arm (ground list `[e₁..e_k]`)  path = enc(e₁) ‖ … ‖ enc(e_k) ‖ 0x00
//   bare arm  (anything else)            path = enc(par)          — NO 0x00
//
// Nothing in this file, or in `rholang/tests/zipper_enumeration_spec.rs`, had
// ever built a map holding BOTH — `test_non_list_par` and
// `test_mixed_list_and_nonlist` above check only cardinality, which is the one
// question the defect does not affect. `{| 1, [1], "a", ["a","x"] |}` is the
// fixture that asks the rest.

/// `{| 1, [1], "a", ["a","x"] |}` — a bare int, its singleton list, a bare
/// string, and a two-element list sharing that string's segment.
fn mixed_elements() -> Vec<Par> {
    vec![
        make_int_par(1),
        make_list_of(vec![make_int_par(1)]),
        make_string_par("a"),
        make_list_par(vec!["a", "x"]),
    ]
}

/// The INSERT side is correct and injective — four elements, four distinct
/// keys, four entries — and the exact bytes are pinned so that any change to
/// the insert side is a visible diff rather than a silent re-key.
///
/// ★ In particular `1` and `[1]` differ by exactly the trailing terminator, so
/// "append a terminator at insert" would MERGE them. It is not the fix.
#[test]
fn mixed_arms_produce_four_distinct_entries() {
    let elements = mixed_elements();

    // Tag `0x03` = GInt (payload zigzag varint), `0x04` = GString
    // (payload uv(len) ++ UTF-8), `0x00` = the split-list terminator.
    assert_eq!(encode_trie_path(&elements[0]), vec![0x03, 0x02]);
    assert_eq!(encode_trie_path(&elements[1]), vec![0x03, 0x02, 0x00]);
    assert_eq!(encode_trie_path(&elements[2]), vec![0x04, 0x01, 0x61]);
    assert_eq!(
        encode_trie_path(&elements[3]),
        vec![0x04, 0x01, 0x61, 0x04, 0x01, 0x78, 0x00]
    );

    let result = create_pathmap_from_elements(&elements, None);
    assert_eq!(
        result.map.val_count(),
        4,
        "four distinct keys, so four entries — `1` and `[1]` do NOT collide"
    );
    for element in &elements {
        assert_eq!(
            result.map.get(encode_trie_path(element)),
            Some(element),
            "every element is readable at its own inserted key"
        );
    }
}

/// ⚠ WITNESS OF A DEFECT — the READ side. Every reader rebuilds an entry key
/// as `segments_to_key(par_to_path(p), true)`; for a bare element that is the
/// key of the SINGLETON LIST wrapping it, so the read lands on a different
/// entry (or on nothing).
///
/// Positive twin: `every_element_is_addressable_from_its_own_par`.
#[test]
fn witness_bare_elements_are_not_addressable_by_the_read_key() {
    let elements = mixed_elements();
    let map = create_pathmap_from_elements(&elements, None).map;

    // SPLIT elements: the rebuilt key is the inserted key, so the read lands.
    for split in [&elements[1], &elements[3]] {
        let read_key = segments_to_key(&par_to_path(split), true);
        assert_eq!(read_key, encode_trie_path(split));
        assert_eq!(map.get(&read_key), Some(split));
    }

    // ★ BARE `1`: the rebuilt key is `03 02 00` — the key of `[1]`, which is
    // ALSO in this map. The read succeeds and returns the WRONG element.
    let bare_int_read_key = segments_to_key(&par_to_path(&elements[0]), true);
    assert_eq!(bare_int_read_key, vec![0x03, 0x02, 0x00]);
    assert_eq!(
        map.get(&bare_int_read_key),
        Some(&elements[1]),
        "★ asking for the bare `1` returns the singleton list `[1]`"
    );

    // ★ BARE `"a"`: the rebuilt key is `04 01 61 00`, which is in no map here,
    // so the read is a miss — the same defect, presenting as Nil.
    let bare_string_read_key = segments_to_key(&par_to_path(&elements[2]), true);
    assert_eq!(bare_string_read_key, vec![0x04, 0x01, 0x61, 0x00]);
    assert_eq!(map.get(&bare_string_read_key), None);
}

/// ★ PREFIX vs ENTRY — the SETTLED semantics, stated executably.
///
/// A bare entry's WHOLE key is a strict byte-prefix of the split keys that
/// begin with the same element, so bare entries sit at INTERIOR trie
/// positions: at one element-prefix a map may hold a bare entry, a split
/// entry, and a whole branch of descendants.
///
/// The answer (module header of `pathmap_native_query.rs`, §BRANCH versus
/// ENTRY) is that the two questions get two different keys, and the cursor's
/// `CursorKind` is what lets an ENTRY query name which arm it means:
///
///   BRANCH  `segments_to_key(segments, false)` — the subtrie at the
///           element-prefix, which INCLUDES the bare entry sitting there,
///           because that entry's key literally is the prefix. Keeping it in
///           is what makes `leafCount()` a usable walk bound: a
///           `leafCount()`-bounded walk must visit exactly the entries the
///           count promised, and `toNextLeaf` does visit the bare entry.
///   ENTRY   `cursor_entry_key(segments, kind, map)` — exactly one entry.
#[test]
fn a_bare_entry_key_is_a_prefix_of_the_split_keys_beside_it() {
    let elements = mixed_elements();
    let map = create_pathmap_from_elements(&elements, None).map;

    let bare_a = encode_trie_path(&elements[2]); // 04 01 61
    let split_ax = encode_trie_path(&elements[3]); // 04 01 61 04 01 78 00
    assert!(
        split_ax.starts_with(&bare_a),
        "the bare entry's WHOLE key is a strict prefix of the split key"
    );

    // The BRANCH at `04 01 61` holds BOTH the bare entry and `["a","x"]` —
    // this is the helper `leafCount()` at a cursor calls, and 2 is the
    // deliberate answer.
    let branch = models::rust::pathmap_native_query::subtrie_value_count(&map, &bare_a);
    assert_eq!(branch, 2, "the bare entry is counted inside its own branch");

    // …and the ENTRY question is answered exactly, by the cursor's arm: the
    // SAME segment vector names two different entries under two different
    // kinds, and each reads back its own value.
    use models::rust::pathmap_integration::{cursor_entry_key, par_to_path, CursorKind};
    let segments = par_to_path(&elements[2]); // one segment: 04 01 61
    assert_eq!(
        map.get(cursor_entry_key(&segments, CursorKind::Bare, &map)),
        Some(&elements[2]),
        "Bare names the bare entry \"a\""
    );
    assert_eq!(
        map.get(cursor_entry_key(&segments, CursorKind::Split, &map)),
        None,
        "Split names [\"a\"], which this map does not hold"
    );
    // PREFIX — what a child-segment navigation move produces — resolves to the
    // SHORTEST key PRESENT, which here is the bare entry.
    assert_eq!(
        map.get(cursor_entry_key(&segments, CursorKind::Prefix, &map)),
        Some(&elements[2])
    );

    // On a map with NO bare entry at that prefix, PREFIX is indistinguishable
    // from SPLIT — which is exactly why every navigation move can be PREFIX
    // without the ground-LIST corpus moving a byte.
    let lists_only = create_pathmap_from_elements(
        &[make_list_par(vec!["a"]), make_list_par(vec!["a", "x"])],
        None,
    )
    .map;
    assert_eq!(
        cursor_entry_key(&segments, CursorKind::Prefix, &lists_only),
        cursor_entry_key(&segments, CursorKind::Split, &lists_only)
    );
    assert_eq!(
        lists_only.get(cursor_entry_key(&segments, CursorKind::Prefix, &lists_only)),
        Some(&make_list_par(vec!["a"]))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// `entry_key_at` — the ONE place a reader spends the whole-path Par it holds
// ═══════════════════════════════════════════════════════════════════════════

/// At the ROOT the argument IS the whole path, so the key comes from the codec
/// and every element — bare or split — is addressable by its own Par.
#[test]
fn every_element_is_addressable_from_its_own_par() {
    use models::rust::pathmap_integration::entry_key_at;

    let elements = mixed_elements();
    let map = create_pathmap_from_elements(&elements, None).map;
    for element in &elements {
        assert_eq!(
            map.get(entry_key_at(&[], element, &map)),
            Some(element),
            "the root arm addresses every element by its own Par"
        );
    }
}

/// ★ THE BYTE-DIFF CLAIM for stage 3, stated as a test: at the root,
/// `entry_key_at` is byte-identical to the expression it replaces on the SPLIT
/// arm, and differs from it ONLY where that expression was wrong.
///
/// This is what makes the change containable — the ground-LIST corpus (every
/// existing fixture) moves zero bytes.
#[test]
fn entry_key_at_the_root_moves_no_split_arm_bytes() {
    use models::rust::pathmap_integration::entry_key_at;

    // The ROOT arm never consults the map — it asks the codec — so an empty
    // one is the right witness that the answer depends on the Par alone.
    let empty = RholangPathMap::new();

    for split in [
        make_list_of(vec![make_int_par(1)]),
        make_list_par(vec!["a", "x"]),
        make_list_par(vec![]),
        make_list_par(vec!["books", "fiction", "gatsby"]),
    ] {
        assert_eq!(
            entry_key_at(&[], &split, &empty),
            segments_to_key(&par_to_path(&split), true),
            "split arm: byte-identical to the retired expression"
        );
        assert_eq!(entry_key_at(&[], &split, &empty), encode_trie_path(&split));
    }

    for bare in [make_int_par(1), make_string_par("a"), make_int_par(-7)] {
        assert_eq!(
            entry_key_at(&[], &bare, &empty),
            encode_trie_path(&bare),
            "bare arm: the key the entry was inserted under"
        );
        assert_ne!(
            entry_key_at(&[], &bare, &empty),
            segments_to_key(&par_to_path(&bare), true),
            "…which is exactly where it differs from the retired expression"
        );
    }
}

/// ★ Below the root, BOTH arms of the relative argument compose onto the
/// cursor and the composed path is a LIST — the three rows of the matrix, on
/// ONE map.
///
/// ⚠ This test previously asserted only the FIRST row (a LIST relative
/// argument), under the name `entry_key_below_the_root_still_rebuilds` and the
/// claim that the arm "still guesses split". That claim was true and harmless
/// for a list argument — whose arm IS split — and it was a wrong answer for a
/// bare one, which the row did not drive. A guard for `entry_key_at` that
/// exercised only `entry_key_at`'s working arm could not reject what it was
/// written to reject; defect #108 lived underneath it. Every row is here now,
/// and the second one is the one that used to be a miss on every map.
#[test]
fn entry_key_below_the_root_composes_both_arms_onto_the_cursor() {
    use models::rust::pathmap_integration::entry_key_at;

    let map = create_pathmap_from_elements(&mixed_elements(), None).map;
    let cursor = par_to_path(&make_string_par("a")); // one segment: 04 01 61

    // ROW 1 — a LIST relative argument. Terminated, as it always was.
    assert_eq!(
        entry_key_at(&cursor, &make_list_of(vec![make_string_par("x")]), &map),
        vec![0x04, 0x01, 0x61, 0x04, 0x01, 0x78, 0x00],
        "a list relative argument composes to the terminated key of [\"a\",\"x\"]"
    );

    // ★ ROW 2 — a BARE relative argument. The composed path is `["a", 1]`,
    // which is a LIST, so the key is TERMINATED. Before the composition law
    // existed this returned `04 01 61 03 02` — an unterminated multi-segment
    // key, in the image of no Par, which `map.get` could not match on any map.
    assert_eq!(
        entry_key_at(&cursor, &make_int_par(1), &map),
        vec![0x04, 0x01, 0x61, 0x03, 0x02, 0x00],
        "★ a bare relative argument composes to the terminated key of [\"a\",1]"
    );
    assert_eq!(
        entry_key_at(&cursor, &make_int_par(1), &map),
        encode_trie_path(&make_list_of(vec![make_string_par("a"), make_int_par(1)])),
        "…which is exactly the key the codec gives the composed path"
    );

    // ROW 3 — the EMPTY list contributes no segments, so the composed path is
    // the cursor's own elements as a list. This is the one composition where
    // the argument's own arm was already the composed arm.
    assert_eq!(
        entry_key_at(&cursor, &make_list_of(vec![]), &map),
        vec![0x04, 0x01, 0x61, 0x00],
        "an empty relative path names the cursor's path as a list"
    );
}

#[test]
fn test_empty_list_par() {
    // Empty list should be handled gracefully
    let par = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![], // Empty list
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    };

    let result = create_pathmap_from_elements(&[par], None);
    // Empty list might be stored differently, just ensure no panic
    assert!(result.map.val_count() <= 1);
}

// ============ ZIPPER TESTS ============

#[test]
fn test_read_zipper_creation() {
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["c", "d"]);

    let elements = vec![par1, par2];
    let result = create_pathmap_from_elements(&elements, None);

    // Verify the PathMap was created successfully
    assert_eq!(result.map.val_count(), 2);
}

#[test]
fn test_read_zipper_at_path() {
    let par1 = make_list_par(vec!["books", "fiction", "gatsby"]);
    let par2 = make_list_par(vec!["books", "fiction", "moby"]);
    let par3 = make_list_par(vec!["books", "nonfiction", "history"]);

    let elements = vec![par1, par2, par3];
    let result = create_pathmap_from_elements(&elements, None);

    // Verify we can create a PathMap at a specific path
    assert_eq!(result.map.val_count(), 3);
}

#[test]
fn test_write_zipper_set_val() {
    let mut map = RholangPathMap::new();

    // Create a simple path and set a value
    let par = make_string_par("value");
    map.insert(b"test_path".to_vec(), par.clone());

    assert_eq!(map.val_count(), 1);
}

#[test]
fn test_graft_operation() {
    // Test grafting one PathMap into another
    let src_par1 = make_list_par(vec!["one", "val"]);
    let src_par2 = make_list_par(vec!["one", "two", "val"]);

    let dst_par = make_list_par(vec!["prefix"]);

    let src_result = create_pathmap_from_elements(&[src_par1, src_par2], None);
    let dst_result = create_pathmap_from_elements(&[dst_par], None);

    // Verify both PathMaps were created
    assert_eq!(src_result.map.val_count(), 2);
    assert_eq!(dst_result.map.val_count(), 1);

    // In a real implementation, we would graft src into dst at a specific path
    // For now, just verify the union operation works
    let combined = dst_result.map.join(&src_result.map);
    assert_eq!(combined.val_count(), 3);
}

#[test]
fn test_join_into_operation() {
    // Test union-merge of two PathMaps
    let par1 = make_list_par(vec!["roman"]);
    let par2 = make_list_par(vec!["romulus"]);

    let par3 = make_list_par(vec!["room"]);
    let par4 = make_list_par(vec!["root"]);

    let map1 = create_pathmap_from_elements(&[par1, par2], None);
    let map2 = create_pathmap_from_elements(&[par3, par4], None);

    let result = map1.map.join(&map2.map);
    assert_eq!(result.val_count(), 4);
}

#[test]
fn test_zipper_empty_pathmap() {
    // Test zipper operations on empty PathMap
    let result = create_pathmap_from_elements(&[], None);
    assert!(result.map.is_empty());
    assert_eq!(result.map.val_count(), 0);
}

#[test]
fn test_zipper_single_element() {
    // Test zipper on single-element PathMap
    let par = make_list_par(vec!["single"]);
    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.map.val_count(), 1);
}

#[test]
fn test_zipper_deep_path() {
    // Test zipper with deeply nested path
    let par = make_list_par(vec!["a", "b", "c", "d", "e", "f"]);
    let result = create_pathmap_from_elements(&[par], None);
    assert_eq!(result.map.val_count(), 1);
}

// ============ ACTUAL DROP HEAD TESTS ============

/// `dropHead`'s RULE — "remove the first `n` elements from every entry's path"
/// — over the codec's notion of a path.
///
/// ⚠ This used to be a hand-copied duplicate of the reducer's loop, `if let
/// Some(EListBody(list)) = par.exprs.first()` and all, so these eight tests
/// asserted the behaviour of a COPY and could not have gone red on any change
/// to the reducer. It is now the rule expressed over the same public classifier
/// the reducer calls (`path_elements`), and the reducer's own behaviour is
/// pinned end-to-end, through the interpreter, in
/// `rholang/tests/drop_head_spec.rs` — including the two arms these fixtures
/// never build (a bare entry, and an entry the codec escapes).
fn perform_drophead(elements: Vec<Par>, n: usize) -> Vec<Par> {
    use models::rust::pathmap_integration::path_elements;

    let mut result_elements = Vec::with_capacity(elements.len());
    for par in &elements {
        let path = path_elements(par);
        match n {
            // Dropping nothing is the identity, at every path length.
            0 => result_elements.push(par.clone()),
            // A path of `n` or fewer elements is exhausted by the drop.
            _ if path.len() <= n => continue,
            // …otherwise the entry becomes the ground list of its path's tail.
            _ => result_elements.push(make_list_of(path[n..].to_vec())),
        }
    }
    result_elements
}

fn extract_list_from_par(par: &Par) -> Option<Vec<String>> {
    if let Some(ExprInstance::EListBody(list)) =
        par.exprs.first().and_then(|e| e.expr_instance.as_ref())
    {
        let strings: Vec<String> = list
            .ps
            .iter()
            .filter_map(|p| {
                if let Some(ExprInstance::GString(s)) =
                    p.exprs.first().and_then(|e| e.expr_instance.as_ref())
                {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect();
        Some(strings)
    } else {
        None
    }
}

#[test]
fn test_drophead_large_value() {
    // dropHead(10) on path ["a", "b", "c"] should remove all elements (n > path length)
    let par = make_list_par(vec!["a", "b", "c"]);
    let elements = vec![par];

    let result = perform_drophead(elements, 10);
    assert!(
        result.is_empty(),
        "dropHead with n > length should remove all elements"
    );
}

#[test]
fn test_drophead_zero() {
    // dropHead(0) should preserve all elements
    let par1 = make_list_par(vec!["a", "b", "c"]);
    let par2 = make_list_par(vec!["x", "y", "z"]);
    let elements = vec![par1.clone(), par2.clone()];

    let result = perform_drophead(elements, 0);
    assert_eq!(result.len(), 2, "dropHead(0) should preserve all elements");

    let list1 = extract_list_from_par(&result[0]).unwrap();
    assert_eq!(list1, vec!["a", "b", "c"]);
}

#[test]
fn test_drophead_exact_length() {
    // dropHead(3) on path ["a", "b", "c"] should remove all elements
    let par = make_list_par(vec!["a", "b", "c"]);
    let elements = vec![par];

    let result = perform_drophead(elements, 3);
    assert!(
        result.is_empty(),
        "dropHead with n == path length should remove all elements"
    );
}

#[test]
fn test_drophead_partial() {
    // dropHead(1) on path ["a", "b", "c"] should leave ["b", "c"]
    let par = make_list_par(vec!["a", "b", "c"]);
    let elements = vec![par];

    let result = perform_drophead(elements, 1);
    assert_eq!(result.len(), 1, "dropHead(1) should keep the entry");

    let remaining_list = extract_list_from_par(&result[0]).unwrap();
    assert_eq!(
        remaining_list,
        vec!["b", "c"],
        "Should have dropped first element"
    );
}

#[test]
fn test_drophead_multiple_paths_different_lengths() {
    // dropHead on PathMap with multiple paths of different lengths
    // dropHead(2): ["a", "b", "c", "d"] → ["c", "d"], ["x", "y"] → removed
    let par1 = make_list_par(vec!["a", "b", "c", "d"]);
    let par2 = make_list_par(vec!["x", "y"]);
    let elements = vec![par1, par2];

    let result = perform_drophead(elements, 2);
    assert_eq!(result.len(), 1, "Only the longer path should remain");

    let remaining_list = extract_list_from_par(&result[0]).unwrap();
    assert_eq!(
        remaining_list,
        vec!["c", "d"],
        "Should have dropped first 2 elements"
    );
}

#[test]
fn test_drophead_single_element_path() {
    // dropHead(1) on single-element path ["a"] should result in empty
    let par = make_list_par(vec!["a"]);
    let elements = vec![par];

    let result = perform_drophead(elements, 1);
    assert!(
        result.is_empty(),
        "dropHead(1) on 1-element path should remove it"
    );
}

#[test]
fn test_drophead_all_paths_too_short() {
    // dropHead(5) when all paths are shorter should result in empty
    let par1 = make_list_par(vec!["a", "b"]);
    let par2 = make_list_par(vec!["x", "y", "z"]);
    let elements = vec![par1, par2];

    let result = perform_drophead(elements, 5);
    assert!(
        result.is_empty(),
        "dropHead with n larger than all paths should remove everything"
    );
}

#[test]
fn test_drophead_mixed_survivability() {
    // Some paths survive, some don't
    let par1 = make_list_par(vec!["a", "b", "c", "d", "e"]); // Survives with 3 elements
    let par2 = make_list_par(vec!["x", "y"]); // Removed
    let par3 = make_list_par(vec!["p", "q", "r"]); // Survives with 1 element
    let elements = vec![par1, par2, par3];

    let result = perform_drophead(elements, 2);
    assert_eq!(result.len(), 2, "2 paths should survive dropHead(2)");

    let list1 = extract_list_from_par(&result[0]).unwrap();
    let list2 = extract_list_from_par(&result[1]).unwrap();

    // Verify correct elements were dropped
    assert!(
        list1.len() >= 1 && list2.len() >= 1,
        "Surviving paths should have elements"
    );
}
