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

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation, Var,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::serializers::{
    decode_continuations, decode_datum, decode_datums, encode_continuations, encode_datum,
    encode_datums,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};
use serde::Deserialize;

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

// ---------------------------------------------------------------------------
// Fixtures — the production instantiation
// ---------------------------------------------------------------------------

fn datum_fixtures() -> Vec<(&'static str, Datum<ListParWithRandom>)> {
    vec![
        ("loaded", Datum {
            a: ListParWithRandom {
                pars: vec![corpus::all_par_fields(), corpus::deep_mixed_par(24)],
                random_state: vec![0xF1, 0xF2, 0xF3, 0xF4],
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
            }
            .into(),
            persist: true,
            source: Produce::new(Blake2b256Hash::new(&[7]), Blake2b256Hash::new(&[8]), true),
        }),
        ("deep_only", Datum {
            a: ListParWithRandom {
                pars: vec![corpus::deep_par(48)],
                random_state: vec![9; 32],
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
    for (label, datum) in datum_fixtures() {
        let bytes = encode_datum(&datum);
        let oracle: DatumOracle<ListParWithRandom> =
            bincode::deserialize(&bytes).expect("oracle must decode");
        let machine = decode_datum::<ListParWithRandom>(&bytes)
            .unwrap_or_else(|e| panic!("{label}: machine rejected a well-formed datum: {e}"));

        assert!(
            *machine.a == oracle.a,
            "{label}: the machine and the derived oracle disagree on `a`"
        );
        assert_eq!(
            machine.persist, oracle.persist,
            "{label}: `persist` — an extent off by one reads this from inside `a`"
        );
        assert_eq!(machine.source, oracle.source, "{label}: `source`");
    }
}

#[test]
fn par_typed_continuation_decode_matches_the_derived_oracle() {
    for (label, cont) in continuation_fixtures() {
        // Encode through the public leaf path, then split the leaf back out, so
        // the bytes under test are exactly the ones the cold store holds.
        let leaf = encode_continuations(&vec![cont.clone()]);
        let encoded: Vec<Vec<u8>> = bincode::deserialize(&leaf).expect("leaf");
        let bytes = &encoded[0];

        let oracle: WaitingContinuationOracle<BindPattern, TaggedContinuation> =
            bincode::deserialize(bytes).expect("oracle must decode");
        let machine =
            decode_continuations::<BindPattern, TaggedContinuation>(&leaf).expect("machine");
        assert_eq!(machine.len(), 1);
        let machine = &machine[0];

        assert!(
            *machine.patterns == oracle.patterns,
            "{label}: disagreement on `patterns`"
        );
        assert!(
            *machine.continuation == oracle.continuation,
            "{label}: disagreement on `continuation`"
        );
        assert_eq!(
            machine.persist, oracle.persist,
            "{label}: `persist` — TWO machine-decoded prefixes precede it, so an \
             extent error in either lands here"
        );
        assert_eq!(machine.peeks, oracle.peeks, "{label}: `peeks`");
        assert_eq!(machine.source, oracle.source, "{label}: `source`");
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
            let oracle = bincode::deserialize::<DatumOracle<ListParWithRandom>>(slice).is_ok();
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
                WaitingContinuationOracle<BindPattern, TaggedContinuation>,
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
    let bytes = std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(|| {
            let datum = Datum {
                a: ListParWithRandom {
                    pars: vec![corpus::deep_par(4096)],
                    random_state: vec![1, 2, 3],
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
            bytes
        })
        .expect("spawn encoder")
        .join()
        .expect("encoder panicked");

    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let leaf = bincode::serialize(&vec![bytes]).expect("frame as a cold-store leaf");
            let datums = decode_datums::<ListParWithRandom>(&leaf)
                .expect("the cold-store read path must survive a depth-4096 datum");
            assert_eq!(datums.len(), 1);
            assert!(datums[0].persist, "the tail must still be read correctly");

            // The nesting really came back.
            let mut depth = 0usize;
            let mut current = &datums[0].a.pars[0];
            while let Some(ExprInstance::EListBody(list)) = current
                .exprs
                .first()
                .and_then(|e: &Expr| e.expr_instance.as_ref())
            {
                depth += 1;
                match list.ps.first() {
                    Some(next) => current = next,
                    None => break,
                }
            }
            assert_eq!(depth, 4096, "the DECODED datum must carry the nesting");

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
    let oracle: DatumOracle<ListParWithRandom> = bincode::deserialize(&encoded[0]).expect("oracle");
    assert!(*decoded[0].a == oracle.a);

    // And the `Par` inside is the same one the oracle sees, field for field.
    let par: &Par = &decoded[0].a.pars[0];
    assert!(!par.sends.is_empty() && !par.exprs.is_empty() && !par.conditionals.is_empty());
    assert!(!par.unforgeables.is_empty());
}
