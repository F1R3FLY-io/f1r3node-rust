//! EPathMap fix P1 — INTERN-STORE test suite.
//!
//! Gates (task charter + plan v1 §1-P1 + amendments PM-4(c)/PM-7, user
//! decision D1 = K2):
//!
//!   * STREAMING ADAPTER: the streamed store-key digest equals the
//!     Blake2b-256 of `encode_to_vec` for EVERY P0 fixture (cross-checked
//!     against crypto's independent one-shot `Blake2b256`), and the K2
//!     byte-verify comparator accepts exactly the byte-equal encodings.
//!   * DIGEST-vs-BYTES KEY EQUIVALENCE (proptest): over random EPathMaps
//!     INCLUDING `locally_free` variants, same digest ⟺ same
//!     `encode_to_vec` bytes, and the K2 verify accepts exactly the
//!     byte-equal — pinning that the store's keying has FULL prost fidelity
//!     where the generated `AlwaysEqual` `==` (which ignores
//!     `locally_free`) would alias distinct values.
//!   * FORCED COLLISION: two entries injected into one digest bucket via
//!     the test seam; the collision list disambiguates by byte verify and
//!     the once-per-process diagnostic fires (observable via the event
//!     counter).
//!   * EVICTION: capacity 64 buckets, LRU by last-use tick; a touched
//!     entry survives, the least-recently-used entry is evicted.
//!   * `eval_stable` CLASSIFIER: per-category units — the positive
//!     ground-normal families (including the P0 `e6a_index` fixture, which
//!     embeds reflect GPrivate leaf tags) and EVERY negative category of
//!     the PM-4(c) grammar.
//!
//! Store-touching tests serialize on a file-local mutex: the intern store
//! is process-wide and this binary's tests otherwise run on parallel
//! threads (under nextest's process-per-test model the lock is free).

mod fixtures;

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::create_bit_vector;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, EList, EMap, EMethod, EPathMap, EPlus, ESet, EVar, Expr, GBigRational,
    GDeployId, GFixedPoint, GUnforgeable, If, Match, New, Par, Receive, Send, Var,
};
use models::rust::pathmap_crate_type_mapper::{
    canonical_prost_digest, clear_intern_store_for_test, digest_collision_events,
    eval_stable_epathmap, inject_intern_entry_for_test, intern_store_len_for_test,
    interned_epathmap, matches_canonical_prost, InternedEPathMap, PathMapCrateTypeMapper,
};
use models::rust::rholang::implicits::GPrivateBuilder;
use proptest::prelude::*;

use fixtures::{
    e6a_index_epathmap, epathmap_locally_free_entries, epathmap_par,
    epathmap_remainder_connective, ezipper_value, gstring_par, ground_list, ground_tuple,
    nested_epathmap_value, reflect_mirror,
};

// ─────────────────────────────────────────────────────────────────────────────
// Harness plumbing
// ─────────────────────────────────────────────────────────────────────────────

/// Serializes every test that touches the process-wide intern store.
static STORE_LOCK: Mutex<()> = Mutex::new(());

fn store_guard() -> MutexGuard<'static, ()> {
    STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn single_expr_par(instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

fn map_of(ps: Vec<Par>) -> EPathMap {
    // EPathMap fix P3 (PM-2): constructor instead of a struct literal
    // (the wrapper's shadow cell is private).
    EPathMap::new(ps, Vec::new(), false, None)
}

fn free_var(index: i32) -> Var {
    Var {
        var_instance: Some(VarInstance::FreeVar(index)),
    }
}

fn bound_var_par(index: i32) -> Par {
    single_expr_par(ExprInstance::EVarBody(EVar {
        v: Some(Var {
            var_instance: Some(VarInstance::BoundVar(index)),
        }),
    }))
}

fn gprivate_unforgeable(seed: &str) -> GUnforgeable {
    GPrivateBuilder::new_par_from_string(seed.to_string()).unforgeables[0].clone()
}

/// All five P0 fixture EPathMaps (the EZipper fixture contributes its inner
/// map — the store keys EPathMaps).
fn all_fixture_maps() -> Vec<(&'static str, EPathMap)> {
    vec![
        ("e6a_index", e6a_index_epathmap()),
        ("nested", nested_epathmap_value()),
        (
            "ezipper_inner",
            ezipper_value()
                .pathmap
                .expect("the EZipper fixture carries a map"),
        ),
        ("locally_free", epathmap_locally_free_entries()),
        ("remainder_connective", epathmap_remainder_connective()),
    ]
}

fn full_stream(map: &models::rust::pathmap_integration::RholangPathMap) -> Vec<(Vec<u8>, Par)> {
    map.iter().map(|(k, v)| (k, v.clone())).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Streaming adapter (mandate: streamed digest == digest of encode_to_vec
// bytes for every P0 fixture)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn streamed_digest_matches_one_shot_digest_for_every_p0_fixture() {
    for (name, fixture) in all_fixture_maps() {
        let bytes = prost::Message::encode_to_vec(&fixture);
        let reference = Blake2b256::hash(bytes.clone());
        assert_eq!(
            canonical_prost_digest(&fixture).as_slice(),
            reference.as_slice(),
            "streamed digest diverged from the one-shot Blake2b-256 for fixture {name}"
        );
        assert!(
            matches_canonical_prost(&fixture, &bytes),
            "the K2 verify must accept the fixture's own canonical bytes ({name})"
        );
    }
}

#[test]
fn k2_verify_rejects_every_cross_fixture_pairing() {
    let fixture_maps = all_fixture_maps();
    for (i, (name_a, a)) in fixture_maps.iter().enumerate() {
        for (j, (name_b, b)) in fixture_maps.iter().enumerate() {
            if i == j {
                continue;
            }
            let bytes_b = prost::Message::encode_to_vec(b);
            assert!(
                !matches_canonical_prost(a, &bytes_b),
                "the K2 verify must reject {name_a} against {name_b}'s canonical bytes"
            );
        }
    }
}

/// THE K2 justification: the generated `AlwaysEqual` `==` IGNORES
/// `locally_free` (P0-pinned), so it would alias byte-distinct values; the
/// digest and the byte verify must NOT.
#[test]
fn always_equal_equality_is_unusable_for_keying_but_k2_is_not() {
    let base = e6a_index_epathmap();

    let mut map_level = base.clone();
    map_level.locally_free = create_bit_vector(&[0]);
    let mut entry_level = base.clone();
    // L2: CoW index-write through the sanctioned mutator (the raw
    // `SharedPars` has no `DerefMut`, so `ps[0] = ..` is deliberately loud).
    entry_level.ps_make_mut()[0].locally_free = create_bit_vector(&[1]);

    for (name, variant) in [("map-level", map_level), ("entry-level", entry_level)] {
        assert_eq!(
            base, variant,
            "precondition: AlwaysEqual == ignores {name} locally_free"
        );
        let base_bytes = prost::Message::encode_to_vec(&base);
        let variant_bytes = prost::Message::encode_to_vec(&variant);
        assert_ne!(
            base_bytes, variant_bytes,
            "precondition: prost retains {name} locally_free"
        );
        assert_ne!(
            canonical_prost_digest(&base),
            canonical_prost_digest(&variant),
            "the digest must separate {name} locally_free variants"
        );
        assert!(
            !matches_canonical_prost(&base, &variant_bytes),
            "the K2 verify must reject the {name} locally_free variant"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Digest-vs-bytes key equivalence (proptest; pure — no store interaction)
// ─────────────────────────────────────────────────────────────────────────────

fn arb_ground_atom() -> impl Strategy<Value = ExprInstance> {
    prop_oneof![
        any::<bool>().prop_map(ExprInstance::GBool),
        any::<i64>().prop_map(ExprInstance::GInt),
        "[a-z]{0,8}".prop_map(ExprInstance::GString),
        proptest::collection::vec(any::<u8>(), 0..6).prop_map(ExprInstance::GByteArray),
    ]
}

fn arb_entry() -> impl Strategy<Value = Par> {
    (
        proptest::collection::vec(arb_ground_atom(), 1..4),
        proptest::option::of(0usize..3),
    )
        .prop_map(|(atoms, locally_free_bit)| {
            let elements: Vec<Par> = atoms.into_iter().map(single_expr_par).collect();
            let mut entry = ground_list(elements);
            if let Some(bit) = locally_free_bit {
                entry.locally_free = create_bit_vector(&[bit]);
            }
            entry
        })
}

fn arb_epathmap() -> impl Strategy<Value = EPathMap> {
    (
        proptest::collection::vec(arb_entry(), 0..4),
        any::<bool>(),
        any::<bool>(),
        proptest::option::of(0usize..2),
    )
        .prop_map(
            |(ps, with_remainder, connective_used, locally_free_bit)| {
                EPathMap::new(
                    ps,
                    locally_free_bit
                        .map(|bit| create_bit_vector(&[bit]))
                        .unwrap_or_default(),
                    connective_used,
                    with_remainder.then(|| free_var(0)),
                )
            },
        )
}

/// A near-miss (or identical) variant of `base`: the mutation menu keeps
/// both branches of the ⟺ meaningful and puts `locally_free` flips — the
/// AlwaysEqual blind spot — front and center.
fn mutate(base: &EPathMap, which: u8) -> EPathMap {
    let mut variant = base.clone();
    match which {
        0 => {} // byte-identical clone
        1 => {
            variant.locally_free = if variant.locally_free.is_empty() {
                create_bit_vector(&[0])
            } else {
                Vec::new()
            };
        }
        // L2: every `ps` write below goes through the sanctioned CoW
        // mutator `ps_make_mut` (detaches the payload shared with `base`
        // and takes any inherited cell — `base` itself is never touched).
        2 => match variant.ps_make_mut().first_mut() {
            Some(first) => {
                first.locally_free = if first.locally_free.is_empty() {
                    create_bit_vector(&[1])
                } else {
                    Vec::new()
                };
            }
            None => variant.connective_used = !variant.connective_used,
        },
        3 => variant.connective_used = !variant.connective_used,
        4 => {
            variant.remainder = match variant.remainder {
                Some(_) => None,
                None => Some(free_var(0)),
            };
        }
        5 => variant
            .ps_make_mut()
            .push(ground_list(vec![gstring_par("mutationProbe")])),
        _ => {
            let entries = variant.ps_make_mut();
            if entries.pop().is_none() {
                entries.push(ground_list(vec![gstring_par("refill")]));
            }
        }
    }
    // EPathMap fix P3: rebuild WITHOUT the shadow cell. The menu above edits
    // fields of a clone; had `base` already been interned, the clone would
    // carry base's filled cell into a value with DIFFERENT canonical bytes —
    // exactly the stale-cell hazard the wrapper's debug_assert polices. (In
    // the current call order no base is interned before mutation, but the
    // rebuild makes the helper order-independent insurance.)
    EPathMap::new(
        variant.ps,
        variant.locally_free,
        variant.connective_used,
        variant.remainder,
    )
}

fn arb_epathmap_pair() -> impl Strategy<Value = (EPathMap, EPathMap)> {
    (arb_epathmap(), 0u8..7).prop_map(|(base, which)| {
        let variant = mutate(&base, which);
        (base, variant)
    })
}

proptest! {
    /// Same digest ⟺ same `encode_to_vec` bytes, and the K2 verify accepts
    /// exactly the byte-equal (a real Blake2b-256 collision — probability
    /// ~2^-128 — is the only falsifier of the ⟹ direction).
    #[test]
    fn digest_keying_coincides_with_full_prost_byte_keying(
        (a, b) in arb_epathmap_pair()
    ) {
        let bytes_a = prost::Message::encode_to_vec(&a);
        let bytes_b = prost::Message::encode_to_vec(&b);
        let bytes_equal = bytes_a == bytes_b;

        prop_assert_eq!(
            canonical_prost_digest(&a) == canonical_prost_digest(&b),
            bytes_equal,
            "digest equality must coincide with canonical-byte equality"
        );
        prop_assert_eq!(
            matches_canonical_prost(&a, &bytes_b),
            bytes_equal,
            "the K2 verify must accept exactly the byte-equal candidate"
        );
        prop_assert_eq!(
            matches_canonical_prost(&b, &bytes_a),
            bytes_equal,
            "the K2 verify must be symmetric on byte equality"
        );
        prop_assert!(matches_canonical_prost(&a, &bytes_a));
        prop_assert!(matches_canonical_prost(&b, &bytes_b));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Interned-entry envelope + shim value parity (store-touching)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn interned_entry_pins_canonical_bytes_len_digest_count_and_lazy_serde() {
    let _guard = store_guard();
    for ((name, fixture), (_, fresh_twin)) in
        all_fixture_maps().into_iter().zip(all_fixture_maps())
    {
        let interned = interned_epathmap(&fixture);
        // P3: `fixture`'s cell is now FILLED, so encoding it would serve the
        // cached bytes — assert against a FRESH structurally-equal twin
        // (empty cell ⇒ field-walk encoding) to keep the pin
        // non-self-referential.
        let bytes = prost::Message::encode_to_vec(&fresh_twin);
        assert_eq!(
            interned.canonical_prost, bytes,
            "canonical_prost must be the full prost encoding ({name})"
        );
        assert_eq!(
            interned.encoded_len,
            bytes.len(),
            "encoded_len must equal the canonical byte length ({name})"
        );
        assert_eq!(
            interned.digest,
            canonical_prost_digest(&fresh_twin),
            "the stored digest must be the streamed digest ({name})"
        );
        assert_eq!(
            interned.entry_count,
            fixture.ps.len(),
            "entry_count must be ps.len() ({name})"
        );
        assert!(
            interned.serde_bytes.get().is_none(),
            "P1 must NOT populate serde_bytes — that is P4's lazy consumer ({name})"
        );

        let again = interned_epathmap(&fixture);
        assert!(
            Arc::ptr_eq(&interned, &again),
            "a SHADOW-CELL hit must return the SAME interned Arc ({name})"
        );

        // The store leg (P3: a fresh instance has an empty cell, so this
        // exercises the digest-walk + K2-verify store hit).
        let via_store = interned_epathmap(&fresh_twin);
        assert!(
            Arc::ptr_eq(&interned, &via_store),
            "a STORE digest hit must dedup to the same interned Arc ({name})"
        );
    }
}

#[test]
fn shim_result_is_value_identical_to_the_interned_entry() {
    let _guard = store_guard();
    for (name, fixture) in all_fixture_maps() {
        let interned = interned_epathmap(&fixture);
        let shim = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&fixture);
        assert_eq!(
            full_stream(&shim.map),
            full_stream(&interned.map),
            "shim trie must stream identically to the interned trie ({name})"
        );
        assert_eq!(shim.connective_used, interned.connective_used, "{name}");
        assert_eq!(shim.locally_free, interned.locally_free, "{name}");
    }
}

#[test]
fn results_are_value_identical_across_miss_hit_and_evicted_paths() {
    let _guard = store_guard();
    let source = map_of(vec![
        ground_list(vec![gstring_par("valueParity"), gstring_par("x")]),
        ground_list(vec![gstring_par("valueParity"), gstring_par("y")]),
    ]);

    let miss = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&source);
    let hit = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&source);
    clear_intern_store_for_test(); // the evicted regime: the rebuild path
    // P3: `source`'s shadow cell survives the store clear (per-instance
    // handle), so the rebuild path needs a FRESH structurally-equal instance
    // (empty cell ⇒ store miss ⇒ rebuild).
    let source_evicted = map_of(vec![
        ground_list(vec![gstring_par("valueParity"), gstring_par("x")]),
        ground_list(vec![gstring_par("valueParity"), gstring_par("y")]),
    ]);
    let rebuilt = PathMapCrateTypeMapper::e_pathmap_to_rholang_pathmap(&source_evicted);

    let reference = full_stream(&miss.map);
    for (regime, result) in [("hit", &hit), ("evicted", &rebuilt)] {
        assert_eq!(
            full_stream(&result.map),
            reference,
            "the {regime} result must be value-identical to the miss result"
        );
        assert_eq!(result.connective_used, miss.connective_used, "{regime}");
        assert_eq!(result.locally_free, miss.locally_free, "{regime}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Forced collision (K2 collision-list disambiguation + diagnostic)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn forced_digest_collision_disambiguates_and_fires_the_diagnostic() {
    let _guard = store_guard();
    clear_intern_store_for_test();

    let donor = map_of(vec![ground_list(vec![
        gstring_par("collisionDonor"),
        gstring_par("x"),
    ])]);
    let victim = map_of(vec![ground_list(vec![
        gstring_par("collisionVictim"),
        gstring_par("y"),
    ])]);

    let donor_interned = interned_epathmap(&donor);
    let victim_digest = canonical_prost_digest(&victim);
    assert_ne!(
        donor_interned.digest, victim_digest,
        "precondition: the two maps have distinct digests"
    );

    // Simulate the ~2^-128 event: an entry whose digest claims the victim's
    // bucket but whose canonical bytes are the donor's.
    let forged = Arc::new(InternedEPathMap {
        map: donor_interned.map.clone(),
        connective_used: donor_interned.connective_used,
        locally_free: donor_interned.locally_free.clone(),
        path_stream: donor_interned.path_stream.clone(),
        canonical_prost: donor_interned.canonical_prost.clone(),
        encoded_len: donor_interned.encoded_len,
        digest: victim_digest,
        entry_count: donor_interned.entry_count,
        eval_stable: donor_interned.eval_stable,
        serde_bytes: OnceLock::new(),
    });
    inject_intern_entry_for_test(Arc::clone(&forged));
    assert_eq!(
        intern_store_len_for_test(),
        2,
        "donor bucket + forged victim-digest bucket"
    );

    let events_before = digest_collision_events();
    let victim_interned = interned_epathmap(&victim);
    assert_eq!(
        digest_collision_events(),
        events_before + 1,
        "the digest-collision diagnostic must fire exactly once for the miss"
    );
    assert!(
        !Arc::ptr_eq(&victim_interned, &forged),
        "the K2 verify must refuse the byte-mismatching candidate"
    );
    assert_eq!(
        victim_interned.canonical_prost,
        prost::Message::encode_to_vec(&victim),
        "the collision-path result must be built from the victim's own bytes"
    );
    assert_eq!(
        intern_store_len_for_test(),
        2,
        "the collision goes into the existing bucket's list, not a new bucket"
    );

    // P3: a fresh victim instance (empty cell) so the STORE's collision
    // list — not the instance's shadow cell — does the disambiguating.
    let victim_fresh = map_of(vec![ground_list(vec![
        gstring_par("collisionVictim"),
        gstring_par("y"),
    ])]);
    let resolved = interned_epathmap(&victim_fresh);
    assert!(
        Arc::ptr_eq(&resolved, &victim_interned),
        "the collision list must disambiguate to the byte-matching entry"
    );
    assert_eq!(
        digest_collision_events(),
        events_before + 1,
        "a successful collision-list resolution is not a collision event"
    );

    // P3: likewise a fresh donor instance to route through the store.
    let donor_fresh = map_of(vec![ground_list(vec![
        gstring_par("collisionDonor"),
        gstring_par("x"),
    ])]);
    let donor_again = interned_epathmap(&donor_fresh);
    assert!(
        Arc::ptr_eq(&donor_again, &donor_interned),
        "the donor's own bucket must be untouched by the forgery"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Eviction (capacity 64, LRU)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn lru_eviction_at_capacity_64_retains_touched_and_evicts_least_recent() {
    let _guard = store_guard();
    clear_intern_store_for_test();

    // P3: every store-level probe below uses a FRESH instance from this
    // builder — a re-used instance would hit its own shadow cell and never
    // reach the store (cell hits do not refresh LRU ticks, by design).
    let probe = |i: usize| map_of(vec![ground_list(vec![gstring_par(&format!("evictProbe{i}"))])]);
    let maps: Vec<EPathMap> = (0..65).map(probe).collect();

    let arcs: Vec<Arc<InternedEPathMap>> =
        maps[..64].iter().map(interned_epathmap).collect();
    assert_eq!(intern_store_len_for_test(), 64, "the store fills to capacity");

    // Touch entry 0 (fresh instance ⇒ store hit ⇒ LRU tick refresh) so
    // entry 1 becomes the least recently used.
    let touched = interned_epathmap(&probe(0));
    assert!(Arc::ptr_eq(&touched, &arcs[0]));

    // Insert the 65th distinct map: capacity forces one eviction.
    let _ = interned_epathmap(&maps[64]);
    assert_eq!(
        intern_store_len_for_test(),
        64,
        "capacity must hold at 64 buckets"
    );

    let retained = interned_epathmap(&probe(0));
    assert!(
        Arc::ptr_eq(&retained, &arcs[0]),
        "the recently-touched entry must survive the eviction"
    );

    let rebuilt = interned_epathmap(&probe(1));
    assert!(
        !Arc::ptr_eq(&rebuilt, &arcs[1]),
        "the least-recently-used entry must have been evicted (and rebuilt on demand)"
    );
    assert_eq!(intern_store_len_for_test(), 64);
}

// ─────────────────────────────────────────────────────────────────────────────
// eval_stable classifier — positive families
// ─────────────────────────────────────────────────────────────────────────────

/// MANDATED VERDICT: the P0 e6a_index fixture is ground-normal (its entries
/// are GString/EList/ETuple trees over reflect GPrivate leaf tags — the
/// exact alphabet `reflect_ground_term_par` emits for positional
/// constructors).
#[test]
fn eval_stable_true_on_the_e6a_index_fixture() {
    assert!(eval_stable_epathmap(&e6a_index_epathmap()));
}

#[test]
fn eval_stable_true_on_the_nested_epathmap_fixture() {
    assert!(eval_stable_epathmap(&nested_epathmap_value()));
}

#[test]
fn eval_stable_true_on_the_ezipper_fixture_inner_map() {
    let inner = ezipper_value()
        .pathmap
        .expect("the EZipper fixture carries a map");
    assert!(eval_stable_epathmap(&inner));
}

#[test]
fn eval_stable_true_on_every_ground_atom_and_recursive_carrier() {
    let inner_map = map_of(vec![ground_list(vec![gstring_par("innerKey")])]);
    let map = map_of(vec![
        single_expr_par(ExprInstance::GBool(true)),
        single_expr_par(ExprInstance::GInt(-7)),
        single_expr_par(ExprInstance::GString("s".to_string())),
        single_expr_par(ExprInstance::GUri("rho:probe".to_string())),
        single_expr_par(ExprInstance::GByteArray(vec![0, 255])),
        single_expr_par(ExprInstance::GDouble(1.5f64.to_bits())),
        single_expr_par(ExprInstance::GBigInt(vec![1])),
        single_expr_par(ExprInstance::GBigRat(GBigRational::default())),
        single_expr_par(ExprInstance::GFixedPoint(GFixedPoint::default())),
        ground_tuple(gstring_par("tupleElement")),
        ground_list(vec![gstring_par("listElement"), gstring_par("deep")]),
        epathmap_par(inner_map),
    ]);
    assert!(eval_stable_epathmap(&map));
}

/// The reflect GPrivate LEAF is admitted (an unforgeable INSTEAD of an
/// expr): `eval_expr` returns a zero-expr Par unchanged and
/// `update_locally_free_par` draws nothing from `unforgeables`.
#[test]
fn eval_stable_true_on_reflect_gprivate_leaf_shapes() {
    let map = map_of(vec![
        reflect_mirror("Leaf", Vec::new()),
        ground_tuple(reflect_mirror("Pair", vec![
            reflect_mirror("A", Vec::new()),
            reflect_mirror("B", Vec::new()),
        ])),
    ]);
    assert!(eval_stable_epathmap(&map));
}

#[test]
fn eval_stable_true_on_the_empty_map() {
    assert!(eval_stable_epathmap(&map_of(Vec::new())));
}

/// The store carries the classifier verdict on every interned entry.
#[test]
fn interned_entries_carry_the_classifier_verdict() {
    let _guard = store_guard();
    for (name, fixture) in all_fixture_maps() {
        let interned = interned_epathmap(&fixture);
        assert_eq!(
            interned.eval_stable,
            eval_stable_epathmap(&fixture),
            "the interned verdict must equal the classifier's ({name})"
        );
    }
    assert!(interned_epathmap(&e6a_index_epathmap()).eval_stable);
    assert!(!interned_epathmap(&epathmap_locally_free_entries()).eval_stable);
    assert!(!interned_epathmap(&epathmap_remainder_connective()).eval_stable);
}

// ─────────────────────────────────────────────────────────────────────────────
// eval_stable classifier — every negative category (PM-4(c))
// ─────────────────────────────────────────────────────────────────────────────

fn ground_entry() -> Par {
    ground_list(vec![gstring_par("negativeProbe")])
}

/// MANDATED VERDICT: `remainder: Some(_)` + `connective_used: true` at the
/// top level (the P0 pattern-position fixture).
#[test]
fn eval_stable_false_on_the_remainder_connective_fixture() {
    assert!(!eval_stable_epathmap(&epathmap_remainder_connective()));
}

/// MANDATED VERDICT: non-empty `locally_free` on the map AND an entry (the
/// P0 fixture).
#[test]
fn eval_stable_false_on_the_locally_free_fixture() {
    assert!(!eval_stable_epathmap(&epathmap_locally_free_entries()));
}

#[test]
fn eval_stable_false_on_top_level_remainder_alone() {
    let mut map = map_of(vec![ground_entry()]);
    map.remainder = Some(free_var(0));
    assert!(!eval_stable_epathmap(&map));
}

#[test]
fn eval_stable_false_on_top_level_connective_used_alone() {
    let mut map = map_of(vec![ground_entry()]);
    map.connective_used = true;
    assert!(!eval_stable_epathmap(&map));
}

#[test]
fn eval_stable_false_on_map_level_locally_free_alone() {
    let mut map = map_of(vec![ground_entry()]);
    map.locally_free = create_bit_vector(&[0]);
    assert!(!eval_stable_epathmap(&map));
}

#[test]
fn eval_stable_false_on_entry_level_locally_free_alone() {
    let mut entry = ground_entry();
    entry.locally_free = create_bit_vector(&[0]);
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_entry_level_connective_used() {
    let mut entry = ground_entry();
    entry.connective_used = true;
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_nested_elist_remainder() {
    let entry = single_expr_par(ExprInstance::EListBody(EList {
        ps: vec![gstring_par("x")],
        locally_free: Vec::new(),
        connective_used: false,
        remainder: Some(free_var(0)),
    }));
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_nested_epathmap_remainder() {
    let inner = EPathMap::new(vec![gstring_par("x")], Vec::new(), false, Some(free_var(0)));
    assert!(!eval_stable_epathmap(&map_of(vec![epathmap_par(inner)])));
}

/// MANDATED VERDICT (the "nested-var" fixture family): an `EVar` anywhere —
/// top-level entry or nested inside a ground list — is unstable
/// (evaluation substitutes it and drops `var_eval_cost`).
#[test]
fn eval_stable_false_on_evar_entry_and_nested_evar() {
    assert!(!eval_stable_epathmap(&map_of(vec![bound_var_par(0)])));
    let nested = ground_list(vec![gstring_par("head"), bound_var_par(0)]);
    assert!(!eval_stable_epathmap(&map_of(vec![nested])));
}

#[test]
fn eval_stable_false_on_eset_entry() {
    let entry = single_expr_par(ExprInstance::ESetBody(ESet::default()));
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_emap_entry() {
    let entry = single_expr_par(ExprInstance::EMapBody(EMap::default()));
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_emethod_entry() {
    let entry = single_expr_par(ExprInstance::EMethodBody(EMethod::default()));
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_operator_expr_entry() {
    let entry = single_expr_par(ExprInstance::EPlusBody(EPlus::default()));
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_ezipper_entry() {
    let entry = single_expr_par(ExprInstance::EZipperBody(ezipper_value()));
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_send_bearing_entry() {
    let entry = Par {
        sends: vec![Send::default()],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_receive_bearing_entry() {
    let entry = Par {
        receives: vec![Receive::default()],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_new_bearing_entry() {
    let entry = Par {
        news: vec![New::default()],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_match_bearing_entry() {
    let entry = Par {
        matches: vec![Match::default()],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_bundle_bearing_entry() {
    let entry = Par {
        bundles: vec![Bundle::default()],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_conditional_bearing_entry() {
    let entry = Par {
        conditionals: vec![If::default()],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_connective_bearing_entry() {
    let entry = Par {
        connectives: vec![Connective::default()],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_nil_entry() {
    assert!(!eval_stable_epathmap(&map_of(vec![Par::default()])));
}

#[test]
fn eval_stable_false_on_multi_expr_entry() {
    let entry = Par::default().with_exprs(vec![
        Expr {
            expr_instance: Some(ExprInstance::GString("a".to_string())),
        },
        Expr {
            expr_instance: Some(ExprInstance::GString("b".to_string())),
        },
    ]);
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

/// An unforgeable riding ALONGSIDE an expr ("in expr position") is
/// unstable — only the pure GPrivate leaf is admitted.
#[test]
fn eval_stable_false_on_mixed_expr_and_unforgeable_entry() {
    let mut entry = gstring_par("mixed");
    entry.unforgeables = vec![gprivate_unforgeable("mixedTag")];
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_multiple_unforgeables_entry() {
    let entry = Par {
        unforgeables: vec![
            gprivate_unforgeable("tagOne"),
            gprivate_unforgeable("tagTwo"),
        ],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}

#[test]
fn eval_stable_false_on_non_gprivate_unforgeable_leaf() {
    let entry = Par {
        unforgeables: vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId::default())),
        }],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));

    let unset = Par {
        unforgeables: vec![GUnforgeable { unf_instance: None }],
        ..Par::default()
    };
    assert!(!eval_stable_epathmap(&map_of(vec![unset])));
}

#[test]
fn eval_stable_false_on_gprivate_leaf_with_locally_free() {
    let mut entry = GPrivateBuilder::new_par_from_string("taggedLeaf".to_string());
    entry.locally_free = create_bit_vector(&[0]);
    assert!(!eval_stable_epathmap(&map_of(vec![entry])));
}
