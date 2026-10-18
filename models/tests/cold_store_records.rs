//! # The `Par`-typed record differential — `Datum` and `WaitingContinuation`
//!
//! `rspace++`'s own `mod tests` proves the composition claim on the test-double
//! instantiation (`Datum<String>`, `WaitingContinuation<String, String>`). This
//! file proves it on the **production** one —
//! `RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>` — which
//! `rspace++` cannot name, because `models` depends on `rspace_plus_plus` and a
//! reverse edge would be a cycle.
//!
//! ## The claim
//!
//! `decode_datum::<A>` is *not* a whole-struct read. It is
//!
//! ```text
//!     (a, extent) = A::cold_decode_prefix(bytes)        // the machine
//!     (persist, source) = bincode::deserialize(bytes[extent..])   // the tail
//! ```
//!
//! and the claim that this equals `bincode::deserialize::<DatumDe<A>>(bytes)`
//! rests on two facts about bincode 1.3.3:
//!
//! 1. a derived struct is `deserialize_struct` →
//!    `deserialize_tuple(fields.len())` (`src/de/mod.rs:402-412`), and
//! 2. a tuple is `visit_seq` over its elements with **no framing**
//!    (`:293-330`).
//!
//! So a struct's encoding is exactly the concatenation of its fields', and
//! splitting it at a field boundary is byte-identical — **provided the extent
//! is exact**. That proviso is the whole risk: an extent off by one silently
//! reads `persist` out of the last byte of `a`, and `source` out of the middle
//! of nothing, producing a *wrong value* rather than an error. Hence the
//! oracles below.
//!
//! The oracle structs are declared here rather than imported because the
//! library's own twins are `#[cfg(test)]` and therefore invisible to an
//! integration test. They are **compiler-generated from the same field list in
//! the same order**, which is the only property the oracle needs.

use std::collections::BTreeSet;

use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation, Var,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::serializers::{
    decode_continuations, decode_datum, decode_datums, encode_continuations, encode_datum,
    encode_datums,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};
use serde::{Deserialize, Serialize};

mod par_corpus;
use par_corpus as corpus;

// ---------------------------------------------------------------------------
// The oracles — the pre-conversion derived layouts, field for field
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct DatumOracle<A> {
    a: A,
    persist: bool,
    source: Produce,
}

#[derive(Deserialize)]
struct WaitingContinuationOracle<P, K> {
    patterns: Vec<P>,
    continuation: K,
    persist: bool,
    peeks: BTreeSet<i32>,
    source: Consume,
}

#[derive(Serialize, Deserialize)]
struct LegacyListParWithRandom {
    pars: Vec<Par>,
    random_state: Vec<u8>,
}

impl From<&ListParWithRandom> for LegacyListParWithRandom {
    fn from(value: &ListParWithRandom) -> Self {
        Self {
            pars: value.pars.clone(),
            random_state: value.random_state.clone(),
        }
    }
}

impl From<LegacyListParWithRandom> for ListParWithRandom {
    fn from(value: LegacyListParWithRandom) -> Self {
        Self {
            pars: value.pars,
            random_state: value.random_state,
            cost_region: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LegacyTaggedContinuation {
    guard: Option<Par>,
    tagged_cont: Option<TaggedCont>,
}

impl From<&TaggedContinuation> for LegacyTaggedContinuation {
    fn from(value: &TaggedContinuation) -> Self {
        Self {
            guard: value.guard.clone(),
            tagged_cont: value.tagged_cont.clone(),
        }
    }
}

impl From<LegacyTaggedContinuation> for TaggedContinuation {
    fn from(value: LegacyTaggedContinuation) -> Self {
        Self {
            guard: value.guard,
            tagged_cont: value.tagged_cont,
            cost_region: None,
            clause_regions: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Fixtures — the production instantiation
// ---------------------------------------------------------------------------

fn datum_fixtures() -> Vec<(&'static str, Datum<ListParWithRandom>)> {
    vec![
        ("loaded", Datum {
            a: ListParWithRandom {
                pars: vec![corpus::all_par_fields(), corpus::deep_mixed_par(24)],
                random_state: vec![0xF1, 0xF2, 0xF3, 0xF4],
                cost_region: None,
            }
            .into(),
            persist: false,
            source: Produce::new(
                Blake2b256Hash::new(&[1, 2, 3]),
                Blake2b256Hash::new(&[4, 5, 6]),
                false,
            ),
        }),
        ("empty", Datum {
            a: ListParWithRandom {
                pars: vec![],
                random_state: vec![],
                cost_region: None,
            }
            .into(),
            persist: true,
            source: Produce::new(Blake2b256Hash::new(&[7]), Blake2b256Hash::new(&[8]), true),
        }),
        ("deep_only", Datum {
            a: ListParWithRandom {
                pars: vec![corpus::deep_par(48)],
                random_state: vec![9; 32],
                cost_region: None,
            }
            .into(),
            persist: false,
            source: Produce::new(
                Blake2b256Hash::new(&[10]),
                Blake2b256Hash::new(&[11]),
                false,
            ),
        }),
    ]
}

fn continuation_fixtures() -> Vec<(
    &'static str,
    WaitingContinuation<BindPattern, TaggedContinuation>,
)> {
    vec![
        ("par_body", WaitingContinuation {
            patterns: vec![
                BindPattern {
                    patterns: vec![corpus::all_par_fields()],
                    remainder: Some(Var {
                        var_instance: Some(VarInstance::FreeVar(0)),
                    }),
                    free_count: 2,
                },
                BindPattern {
                    patterns: vec![],
                    remainder: None,
                    free_count: 0,
                },
            ]
            .into(),
            continuation: TaggedContinuation {
                guard: Some(corpus::gint(41)),
                tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                    body: Some(corpus::deep_mixed_par(20)),
                    random_state: vec![1, 2],
                })),
                cost_region: None,
                clause_regions: Vec::new(),
            }
            .into(),
            persist: true,
            peeks: BTreeSet::from([0, 2, -5]),
            source: Consume {
                channel_hashes: vec![Blake2b256Hash::new(&[13])],
                hash: Blake2b256Hash::new(&[16]),
                persistent: true,
            },
        }),
        ("scala_ref_empty_patterns", WaitingContinuation {
            patterns: Vec::<BindPattern>::new().into(),
            continuation: TaggedContinuation {
                guard: None,
                tagged_cont: Some(TaggedCont::ScalaBodyRef(-99)),
                cost_region: None,
                clause_regions: Vec::new(),
            }
            .into(),
            persist: false,
            peeks: BTreeSet::new(),
            source: Consume {
                channel_hashes: vec![],
                hash: Blake2b256Hash::new(&[19]),
                persistent: false,
            },
        }),
    ]
}

// ---------------------------------------------------------------------------
// The differential
// ---------------------------------------------------------------------------

#[test]
fn par_typed_datum_decode_matches_the_derived_oracle() {
    use rspace_plus_plus::rspace::data_index::RSpaceDataIndex;
    use rspace_plus_plus::rspace::rho_path::{RhoPathEncode, RhoPathV1};

    for (label, datum) in datum_fixtures() {
        let bytes = encode_datum(&datum);
        let oracle: DatumOracle<LegacyListParWithRandom> =
            bincode::deserialize(&bytes).expect("oracle must decode");
        let machine = decode_datum::<ListParWithRandom>(&bytes)
            .unwrap_or_else(|e| panic!("{label}: machine rejected a well-formed datum: {e}"));

        assert!(
            *machine.a == oracle.a.into(),
            "{label}: the machine and the derived oracle disagree on `a`"
        );
        assert_eq!(
            machine.persist, oracle.persist,
            "{label}: `persist` — an extent off by one reads this from inside `a`"
        );
        assert_eq!(machine.source, oracle.source, "{label}: `source`");
        assert_eq!(
            encode_datum(&machine),
            bytes,
            "{label}: canonical identity fixed point"
        );
        let path = RSpaceDataIndex::datum_path(&Par::default(), &machine);
        let view = RhoPathV1::parse_datum(&path).unwrap();
        assert_eq!(view.datum_identity(), bytes, "{label}: identity suffix");
        assert_eq!(
            view.datum_structure(),
            machine.a.rho_path_structure(),
            "{label}: structure"
        );
        assert_eq!(
            &path[view.channel_prefix_len()..],
            RhoPathV1::relative_datum_key(&machine.a.rho_path_structure(), &bytes),
            "{label}: exact channel-relative address",
        );
    }
}

#[test]
fn par_typed_continuation_decode_matches_the_derived_oracle() {
    for (label, cont) in continuation_fixtures() {
        assert_continuation_round_trip(label, &cont);
    }
}

fn assert_continuation_round_trip(
    label: &str,
    cont: &WaitingContinuation<BindPattern, TaggedContinuation>,
) -> WaitingContinuation<BindPattern, TaggedContinuation> {
    let leaf = encode_continuations(&vec![cont.clone()]);
    let encoded: Vec<Vec<u8>> = bincode::deserialize(&leaf).expect("leaf");
    let bytes = &encoded[0];
    let legacy_continuation = LegacyTaggedContinuation::from(cont.continuation.as_ref());
    let frozen = bincode::serialize(&(
        cont.patterns.as_ref(),
        &legacy_continuation,
        cont.persist,
        &cont.peeks,
        &cont.source,
    ))
    .unwrap();
    assert_eq!(*bytes, frozen, "{label}: frozen field sequence");
    for pattern in cont.patterns.iter() {
        assert_eq!(
            pattern.stable_hash_bytes(),
            bincode::serialize(pattern).unwrap(),
            "{label}: generated pattern encoder versus frozen field codec",
        );
    }
    assert_eq!(
        cont.continuation.stable_hash_bytes(),
        bincode::serialize(&legacy_continuation).unwrap(),
        "{label}: generated continuation encoder versus frozen field codec",
    );

    let oracle: WaitingContinuationOracle<BindPattern, LegacyTaggedContinuation> =
        bincode::deserialize(bytes).expect("oracle must decode");
    let mut decoded =
        decode_continuations::<BindPattern, TaggedContinuation>(&leaf).expect("machine");
    assert_eq!(decoded.len(), 1);
    let machine = decoded.pop().unwrap();

    assert_eq!(
        *machine.patterns, oracle.patterns,
        "{label}: oracle patterns"
    );
    assert_eq!(
        *machine.continuation,
        oracle.continuation.into(),
        "{label}: oracle body"
    );
    assert_eq!(
        machine.persist, oracle.persist,
        "{label}: oracle persistence"
    );
    assert_eq!(machine.peeks, oracle.peeks, "{label}: oracle raw peeks");
    assert_eq!(machine.source, oracle.source, "{label}: oracle source");
    assert_eq!(
        *machine.patterns, *cont.patterns,
        "{label}: original patterns"
    );
    assert_eq!(
        *machine.continuation, *cont.continuation,
        "{label}: original body"
    );
    assert_eq!(
        machine.persist, cont.persist,
        "{label}: original persistence"
    );
    assert_eq!(machine.peeks, cont.peeks, "{label}: original raw peeks");
    assert_eq!(machine.source, cont.source, "{label}: original source");
    assert_eq!(
        bincode::serialize(machine.patterns.as_ref()).unwrap(),
        bincode::serialize(cont.patterns.as_ref()).unwrap(),
        "{label}: exact pattern fields beyond semantic equality",
    );
    assert_eq!(
        bincode::serialize(machine.continuation.as_ref()).unwrap(),
        bincode::serialize(cont.continuation.as_ref()).unwrap(),
        "{label}: exact body, guard and random-state fields",
    );
    machine
}

fn frozen_identity(cont: &WaitingContinuation<BindPattern, TaggedContinuation>) -> Vec<u8> {
    let legacy_continuation = LegacyTaggedContinuation::from(cont.continuation.as_ref());
    bincode::serialize(&(
        cont.patterns.as_ref(),
        &legacy_continuation,
        cont.persist,
        &cont.peeks,
    ))
    .unwrap()
}

fn framed_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for &byte in bytes {
        result.push(byte);
        if byte == 0 {
            result.push(255);
        }
    }
    result.extend_from_slice(&[0, 0]);
    result
}

fn positional_continuation(
    fields: &[(u8, i32, bool)],
    peeks: BTreeSet<i32>,
) -> WaitingContinuation<BindPattern, TaggedContinuation> {
    let mut cont = continuation_fixtures().remove(0).1;
    cont.patterns = fields
        .iter()
        .enumerate()
        .map(|(position, (_, free_count, remainder))| BindPattern {
            patterns: vec![corpus::gint(i64::try_from(position).unwrap())],
            remainder: remainder.then_some(Var {
                var_instance: Some(VarInstance::FreeVar(i32::try_from(position).unwrap())),
            }),
            free_count: *free_count,
        })
        .collect::<Vec<_>>()
        .into();
    cont.peeks = peeks;
    cont
}

fn assert_ordered_receive_correspondence(fields: &[(u8, i32, bool)], peeks: BTreeSet<i32>) {
    use rspace_plus_plus::rspace::rho_path::RhoPathV2;

    let original = positional_continuation(fields, peeks);
    let decoded = assert_continuation_round_trip("ordered receive", &original);
    let channels = fields
        .iter()
        .map(|(channel, _, _)| bincode::serialize(&corpus::gint(i64::from(*channel))).unwrap())
        .collect::<Vec<_>>();
    let refs = channels.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let identity = frozen_identity(&original);
    let path = RhoPathV2::continuation_key(&refs, &identity).unwrap();
    let view = RhoPathV2::parse_continuation(&path).unwrap();
    assert_eq!(view.channel_count(), fields.len());
    assert_eq!(view.encoded_identity_frame(), framed_bytes(&identity));
    assert_eq!(view.channel_count(), decoded.patterns.len());
    for (position, frame) in view.channel_frames().enumerate() {
        assert_eq!(frame, framed_bytes(&channels[position]));
        assert_eq!(decoded.patterns[position], original.patterns[position]);
        assert_eq!(decoded.patterns[position].free_count, fields[position].1);
        assert_eq!(
            decoded.peeks.contains(&i32::try_from(position).unwrap()),
            original.peeks.contains(&i32::try_from(position).unwrap()),
        );
    }
    assert_eq!(!decoded.peeks.is_empty(), !original.peeks.is_empty());
    for query in 0..=4_u8 {
        let query_frame =
            framed_bytes(&bincode::serialize(&corpus::gint(i64::from(query))).unwrap());
        let actual = view
            .channel_frames()
            .enumerate()
            .filter_map(|(position, frame)| {
                (frame == query_frame).then(|| u64::try_from(position).unwrap())
            })
            .collect::<Vec<_>>();
        let expected = fields
            .iter()
            .enumerate()
            .filter_map(|(position, (channel, _, _))| {
                (*channel == query).then(|| u64::try_from(position).unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        assert!(actual.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(actual
            .iter()
            .all(|&position| position < u64::try_from(fields.len()).unwrap()));
    }
}

#[test]
fn ordered_receive_signed_fields_survive_repeated_channel_permutations() {
    for channels in [[0, 1, 0], [0, 0, 1]] {
        let fields = [
            (channels[0], i32::MIN, true),
            (channels[1], 0, false),
            (channels[2], i32::MAX, true),
        ];
        assert_ordered_receive_correspondence(&fields, BTreeSet::from([-1, 2, 7]));
    }
}

proptest::proptest! {
    #[test]
    fn ordered_receive_codec_preserves_arbitrary_signed_fields(
        fields in proptest::collection::vec((0..4_u8, proptest::num::i32::ANY, proptest::bool::ANY), 1..65),
        peeks in proptest::collection::btree_set(proptest::num::i32::ANY, 0..17),
    ) {
        assert_ordered_receive_correspondence(&fields, peeks);
    }
}

#[test]
fn ordered_receive_raw_peeks_and_source_suffix_are_not_erased() {
    let fields = [(0, i32::MIN, true), (1, 0, false), (0, i32::MAX, true)];
    let original = positional_continuation(&fields, BTreeSet::from([-1, 7]));
    let decoded = assert_continuation_round_trip("invalid-only raw peeks", &original);
    assert!(!decoded.peeks.is_empty());
    assert!((0..3_i32).all(|position| !decoded.peeks.contains(&position)));
    let mut erased = original.clone();
    erased.peeks.clear();
    assert_ne!(frozen_identity(&erased), frozen_identity(&original));

    let mut alias = original.clone();
    alias.source.hash = Blake2b256Hash::new(&[99]);
    assert_eq!(frozen_identity(&alias), frozen_identity(&original));
    for record in [&original, &alias] {
        let leaf = encode_continuations(&vec![record.clone()]);
        let encoded: Vec<Vec<u8>> = bincode::deserialize(&leaf).unwrap();
        let identity = frozen_identity(record);
        assert_eq!(&encoded[0][..identity.len()], identity);
        assert_eq!(
            &encoded[0][identity.len()..],
            bincode::serialize(&record.source).unwrap(),
        );
        assert_continuation_round_trip("source alias", record);
    }
    assert_ne!(
        encode_continuations(&vec![original]),
        encode_continuations(&vec![alias]),
    );
}

#[test]
fn ordered_receive_fixture_detects_erased_and_shifted_fields() {
    let fields = [(0, i32::MIN, true), (1, 0, false), (0, i32::MAX, true)];
    let original = positional_continuation(&fields, BTreeSet::from([-1, 2, 7]));
    let mut mutants = Vec::new();
    for mutation in 0..7 {
        let mut mutant = original.clone();
        match mutation {
            0 => std::sync::Arc::make_mut(&mut mutant.patterns).swap(0, 2),
            1 => std::sync::Arc::make_mut(&mut mutant.patterns)[0]
                .patterns
                .clear(),
            2 => std::sync::Arc::make_mut(&mut mutant.patterns)[0].remainder = None,
            3 => std::sync::Arc::make_mut(&mut mutant.patterns)[0].free_count = 0,
            4 => {
                mutant.peeks.remove(&2);
                mutant.peeks.insert(0);
            }
            5 => mutant.persist = false,
            6 => std::sync::Arc::make_mut(&mut mutant.continuation).guard = None,
            _ => unreachable!(),
        }
        mutants.push(mutant);
    }
    for (mutation, mutant) in mutants.iter().enumerate() {
        assert_ne!(
            frozen_identity(mutant),
            frozen_identity(&original),
            "mutation {mutation}"
        );
        assert_continuation_round_trip("field mutation", mutant);
    }
}

/// The record-level rejection set. Truncating a `Datum<ListParWithRandom>` at
/// every offset walks the boundary between "the prefix consumed everything" and
/// "the tail ran out", which is exactly where a wrong extent hides.
#[test]
fn truncated_par_typed_records_are_rejected_by_both() {
    let mut checked = 0usize;
    let mut rejections = 0usize;

    for (label, datum) in datum_fixtures() {
        let bytes = encode_datum(&datum);
        for cut in 0..bytes.len() {
            let slice = &bytes[..cut];
            let oracle =
                bincode::deserialize::<DatumOracle<LegacyListParWithRandom>>(slice).is_ok();
            let machine = decode_datum::<ListParWithRandom>(slice).is_ok();
            assert_eq!(
                oracle, machine,
                "{label} truncated at {cut}: oracle ok={oracle}, machine ok={machine}"
            );
            checked += 1;
            if !machine {
                rejections += 1;
            }
        }
    }

    for (label, cont) in continuation_fixtures() {
        let leaf = encode_continuations(&vec![cont]);
        let encoded: Vec<Vec<u8>> = bincode::deserialize(&leaf).expect("leaf");
        let bytes = &encoded[0];
        for cut in 0..bytes.len() {
            let slice = &bytes[..cut];
            let oracle = bincode::deserialize::<
                WaitingContinuationOracle<BindPattern, LegacyTaggedContinuation>,
            >(slice)
            .is_ok();
            // Re-frame the truncated record as a one-element leaf so it goes
            // through the same public entry point production uses.
            let framed = bincode::serialize(&vec![slice.to_vec()]).expect("frame");
            let machine = decode_continuations::<BindPattern, TaggedContinuation>(&framed).is_ok();
            assert_eq!(
                oracle, machine,
                "{label} truncated at {cut}: oracle ok={oracle}, machine ok={machine}"
            );
            checked += 1;
            if !machine {
                rejections += 1;
            }
        }
    }

    assert!(
        checked > 5_000,
        "ANTI-VACUITY: only {checked} truncations tried"
    );
    assert!(
        rejections > checked / 2,
        "ANTI-VACUITY: only {rejections} of {checked} truncated records were rejected"
    );
    println!("  record truncation: {checked} prefixes, {rejections} joint rejections");
}

/// A `Datum` whose payload is deeper than the derived decoder survives.
///
/// This is the point of the whole exercise stated as one assertion: the byte
/// string below is one that `rspace_importer` could commit to LMDB today, and
/// that the pre-conversion read path would abort on — permanently, on every
/// restart, on every peer that synced it.
#[test]
fn a_datum_too_deep_for_the_derived_decoder_still_reads_back() {
    // 4,096 levels. At the measured 28,362 B/level (debug) the derived decoder
    // would need ~110 MiB of native stack; this thread gets 256 KiB.
    let datum = Datum {
        a: ListParWithRandom {
            pars: vec![corpus::deep_par(4096)],
            random_state: vec![1, 2, 3],
            cost_region: None,
        }
        .into(),
        persist: true,
        source: Produce::new(Blake2b256Hash::new(&[1]), Blake2b256Hash::new(&[2]), true),
    };
    let bytes = encode_datum(&datum);
    models::rust::rholang::par_children::dismantle_all(
        std::sync::Arc::try_unwrap(datum.a)
            .map(|a| a.pars)
            .unwrap_or_default(),
    );

    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let leaf = bincode::serialize(&vec![bytes]).expect("frame as a cold-store leaf");
            let datums = decode_datums::<ListParWithRandom>(&leaf)
                .expect("the cold-store read path must survive a depth-4096 datum");
            assert_eq!(datums.len(), 1);
            assert!(datums[0].persist, "the tail must still be read correctly");

            // The nesting really came back. Inspect the canonical Expr body in
            // one byte-native pass: decoding every successively shorter suffix
            // would make this oracle quadratic and says nothing more about the
            // cold-store reader.
            let mut depth = None;
            datums[0].a.pars[0].visit_encoded::<Expr>(|entry| {
                assert!(depth.is_none(), "fixture has exactly one Expr endpoint");
                assert_eq!(entry.multiplicity().get(), 1);
                depth = entry.canonical_unary_elist_depth();
            });
            assert_eq!(
                depth,
                Some(4096),
                "the DECODED datum must carry the nesting"
            );

            let datums = datums;
            for d in datums {
                if let Ok(a) = std::sync::Arc::try_unwrap(d.a) {
                    models::rust::rholang::par_children::dismantle_all(a.pars);
                }
            }
        })
        .expect("spawn decoder")
        .join()
        .expect("the cold-store read path overflowed a 256 KiB stack at depth 4096");
}

/// The Step-A goldens decode through the new path to the same values the
/// oracle produces — tying the pre-change byte pins to the post-change reader.
#[test]
fn step_a_golden_shapes_read_back_through_the_new_path() {
    let datums = vec![datum_fixtures().remove(0).1];
    let leaf = encode_datums(&datums);
    let decoded = decode_datums::<ListParWithRandom>(&leaf).expect("machine");
    assert_eq!(decoded.len(), 1);

    let encoded: Vec<Vec<u8>> = bincode::deserialize(&leaf).expect("leaf");
    let oracle: DatumOracle<LegacyListParWithRandom> =
        bincode::deserialize(&encoded[0]).expect("oracle");
    assert!(*decoded[0].a == oracle.a.into());

    // And the `Par` inside is the same one the oracle sees, field for field.
    let par: &Par = &decoded[0].a.pars[0];
    assert!(
        par.family_cardinality::<models::rhoapi::Send>() != 0
            && par.family_cardinality::<Expr>() != 0
            && par.family_cardinality::<models::rhoapi::If>() != 0
    );
    assert!(par.family_cardinality::<models::rhoapi::GUnforgeable>() != 0);
}
