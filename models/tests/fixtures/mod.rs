//! EPathMap fix P0 — parity-harness FIXTURE BUILDERS (test-only).
//!
//! Hand-built mirrors of the E-6a subject-index shapes from
//! `mettail-rust/rholang-runtime/src/e6a_support.rs:305-409` (READ-ONLY
//! reference; nothing here links against mettail). The fixtures exist to pin
//! the 84a0fbe4 derived truths — canonical PROST bytes, SERDE bytes
//! (bincode + JSON, including the `locally_free`-as-empty serialize-only
//! normalization), produce/consume event hashes, and the derived
//! declaration-order `Ord` — that later phases of the principled EPathMap
//! value-handling fix (P1 intern store, P2 chain fusion, P3 L1.5 wrapper,
//! P4 transport/spliced hashing) must preserve byte-identically.
//!
//! Fixture families (per the P0 charter):
//!   1. [`e6a_index_epathmap`] — an E-6a-shaped subject index: «s» (site/tag)
//!      and «v» (σ-carrier) entry families for the subject `Pair(A, B)` at
//!      root site `site0`.
//!   2. [`nested_epathmap_value`] — an EPathMap whose entry list carries a
//!      NESTED EPathMap value.
//!   3. [`ezipper_value`] — an EZipper value (pathmap + focus path).
//!   4. [`epathmap_locally_free_entries`] — non-empty `locally_free` on both
//!      the map and its entry Pars (exercises the AlwaysEqual/serde/Ord
//!      asymmetries).
//!   5. [`epathmap_remainder_connective`] — `remainder: Some(FreeVar)` +
//!      `connective_used: true` (the pattern-position shape).

use models::create_bit_vector;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{EPathMap, ETuple, EZipper, Expr, Par, Var};
use models::rust::rholang::implicits::GPrivateBuilder;
use models::rust::utils::{new_elist_par, new_gstring_par};

/// The fixed language fingerprint of every fixture. The E-6a trie tag uses
/// its first 8 characters (`e6a_support.rs` `TAG_FINGERPRINT_PREFIX`); the
/// reflect-tag mirror uses the full string.
pub const FIXTURE_FINGERPRINT: &str = "deadbeefcafe";

/// The fixed root site of the fixture subject.
pub const FIXTURE_ROOT_SITE: &str = "site0";

/// Mirror of `e6a_support::e6a_tag_string`: `t.{fp≤8}.{ctor}`.
pub fn e6a_tag_string(language_fingerprint: &str, constructor: &str) -> String {
    let prefix_len = language_fingerprint.len().min(8);
    format!("t.{}.{constructor}", &language_fingerprint[..prefix_len])
}

/// Mirror of `rho_net_lower::reflect_tag`: `mettail.term.{fp}.{ctor}`
/// (FULL fingerprint — the trie tag's 8-char cap is E-6a-local).
pub fn reflect_tag(language_fingerprint: &str, constructor: &str) -> String {
    format!("mettail.term.{language_fingerprint}.{constructor}")
}

/// A ground `GString` Par (mirror of `e6a_support::quoted`).
pub fn gstring_par(value: &str) -> Par {
    new_gstring_par(value.to_string(), Vec::new(), false)
}

/// A ground `EList` of the given elements — no free vars, no remainder
/// (mirror of `e6a_support::ground_list`).
pub fn ground_list(elements: Vec<Par>) -> Par {
    new_elist_par(elements, Vec::new(), false, None, Vec::new(), false)
}

/// A ground 1-`ETuple` wrapping `inner` — the σ-carrier wrapper (mirror of
/// `e6a_support::ground_tuple`).
pub fn ground_tuple(inner: Par) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![inner],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }],
        ..Par::default()
    }
}

/// Mirror of `rho_net_lower::reflect_ground_term_par` for GROUND leaf-arity
/// terms: `EList[ GPrivate(reflect_tag), ⟦child₀⟧, … ]`. All fixture subjects
/// are ground, so every `locally_free` in the reflected tree is empty —
/// exactly what the real reflect computes for ground terms.
pub fn reflect_mirror(constructor: &str, children: Vec<Par>) -> Par {
    let tag = GPrivateBuilder::new_par_from_string(reflect_tag(FIXTURE_FINGERPRINT, constructor));
    let mut elements = Vec::with_capacity(1 + children.len());
    elements.push(tag);
    elements.extend(children);
    ground_list(elements)
}

/// Wrap an entry list in the canonical index-value Par shape
/// (`build_pathmap_index`'s return: one `EPathmapBody` expr, all metadata
/// fields at their ground defaults).
pub fn epathmap_par(map: EPathMap) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map)),
    }])
}

/// FIXTURE 1 — the E-6a-shaped subject index for `Pair(A, B)` at `site0`,
/// fingerprint [`FIXTURE_FINGERPRINT`]. Entry order mirrors
/// `push_index_entries`' DFS pre-order with «s» before «v» per node:
///
/// ```text
/// 1. «s» root    : [ t.deadbeef.Pair, site0 ]
/// 2. «v» root    : [ v, site0, (⟦Pair(A,B)⟧,) ]
/// 3. «s» child 0 : [ t.deadbeef.A, site0, Pair.0 ]
/// 4. «v» child 0 : [ v, site0, Pair.0, (⟦A⟧,) ]
/// 5. «s» child 1 : [ t.deadbeef.B, site0, Pair.1 ]
/// 6. «v» child 1 : [ v, site0, Pair.1, (⟦B⟧,) ]
/// ```
///
/// where `⟦·⟧` is the [`reflect_mirror`] (GPrivate-tagged EList tree) and the
/// path components derive as `spread_child_location`'s `{op}.{index}` steps.
pub fn e6a_index_epathmap() -> EPathMap {
    let tag_pair = e6a_tag_string(FIXTURE_FINGERPRINT, "Pair");
    let tag_a = e6a_tag_string(FIXTURE_FINGERPRINT, "A");
    let tag_b = e6a_tag_string(FIXTURE_FINGERPRINT, "B");

    let reflect_a = reflect_mirror("A", Vec::new());
    let reflect_b = reflect_mirror("B", Vec::new());
    let reflect_pair = reflect_mirror("Pair", vec![reflect_a.clone(), reflect_b.clone()]);

    let entries = vec![
        // «s» root
        ground_list(vec![gstring_par(&tag_pair), gstring_par(FIXTURE_ROOT_SITE)]),
        // «v» root
        ground_list(vec![
            gstring_par("v"),
            gstring_par(FIXTURE_ROOT_SITE),
            ground_tuple(reflect_pair),
        ]),
        // «s» child 0
        ground_list(vec![
            gstring_par(&tag_a),
            gstring_par(FIXTURE_ROOT_SITE),
            gstring_par("Pair.0"),
        ]),
        // «v» child 0
        ground_list(vec![
            gstring_par("v"),
            gstring_par(FIXTURE_ROOT_SITE),
            gstring_par("Pair.0"),
            ground_tuple(reflect_a),
        ]),
        // «s» child 1
        ground_list(vec![
            gstring_par(&tag_b),
            gstring_par(FIXTURE_ROOT_SITE),
            gstring_par("Pair.1"),
        ]),
        // «v» child 1
        ground_list(vec![
            gstring_par("v"),
            gstring_par(FIXTURE_ROOT_SITE),
            gstring_par("Pair.1"),
            ground_tuple(reflect_b),
        ]),
    ];

    // EPathMap fix P3 (PM-2): constructor instead of a struct literal
    // (the wrapper's shadow cell is private).
    EPathMap::new(entries, Vec::new(), false, None)
}

/// FIXTURE 2 — an EPathMap carrying a NESTED EPathMap value: entry
/// `[ "nest", {| ["inner","leaf"] |} ]`. Exercises recursive
/// encode/serialize/Ord through the `ps: Vec<Par>` → `Expr` →
/// `EPathmapBody` cycle.
pub fn nested_epathmap_value() -> EPathMap {
    let inner = EPathMap::new(
        vec![ground_list(vec![gstring_par("inner"), gstring_par("leaf")])],
        Vec::new(),
        false,
        None,
    );
    EPathMap::new(
        vec![ground_list(vec![gstring_par("nest"), epathmap_par(inner)])],
        Vec::new(),
        false,
        None,
    )
}

/// FIXTURE 3 — an EZipper value: the small map `{| [a,x], [a,y], [b] |}` with
/// focus `["a"]`, read mode, NON-EMPTY `locally_free` (bit 0) on the zipper
/// itself. `current_path` segments are raw segment bytes exactly as
/// `readZipperAt` stores them.
pub fn ezipper_value() -> EZipper {
    let map = EPathMap::new(
        vec![
            ground_list(vec![gstring_par("a"), gstring_par("x")]),
            ground_list(vec![gstring_par("a"), gstring_par("y")]),
            ground_list(vec![gstring_par("b")]),
        ],
        Vec::new(),
        false,
        None,
    );
    EZipper {
        pathmap: Some(map),
        current_path: vec![b"a".to_vec()],
        is_write_zipper: false,
        locally_free: create_bit_vector(&[0]),
        connective_used: false,
    }
}

/// FIXTURE 4 — non-empty `locally_free` on BOTH the map and an entry Par:
/// the map's own bitset is `{0}`, and the second entry Par carries bitset
/// `{1}`. This is the fixture family that separates the three regimes the
/// harness pins: PROST bytes INCLUDE `locally_free` (field 3), SERDE
/// serializes it as EMPTY bytes (models/build.rs `serialize_as_empty_bytes`),
/// `==`/`Hash` IGNORE it (AlwaysEqual, models/src/lib.rs:613-627), and the
/// derived `Ord` COMPARES it (declaration order).
pub fn epathmap_locally_free_entries() -> EPathMap {
    let mut tagged_entry = ground_list(vec![gstring_par("lf"), gstring_par("entry")]);
    tagged_entry.locally_free = create_bit_vector(&[1]);
    EPathMap::new(
        vec![
            ground_list(vec![gstring_par("plain")]),
            tagged_entry,
        ],
        create_bit_vector(&[0]),
        false,
        None,
    )
}

/// FIXTURE 5 — the pattern-position shape: `remainder: Some(FreeVar(0))` and
/// `connective_used: true` (an EPathMap pattern `{| p₀ … | ...rest |}`).
pub fn epathmap_remainder_connective() -> EPathMap {
    EPathMap::new(
        vec![ground_list(vec![gstring_par("head")])],
        Vec::new(),
        true,
        Some(Var {
            var_instance: Some(VarInstance::FreeVar(0)),
        }),
    )
}
