//! DR-131 P2a: the four roots of the native history records decode through
//! `ColdStoreDecode` exactly as the derived bincode decoder reads them.
//!
//! The oracle is the derived `Deserialize` with the options of dev's reader
//! (`bincode::deserialize`). For every value the boundary must give the
//! oracle's value and report the exact length of the encoding, accept
//! trailing bytes, and fail on exactly the truncations and single-byte flips
//! on which the oracle fails. The comparison is with the oracle's decoding of
//! the same bytes, not with the original value, because some encoders
//! normalize their input.
//!
//! Since P2b the roots decode through the explicit-stack machine
//! (`record_decoder.rs`), so these tests are its differential against the
//! derived decoder on random terms. The `PartialEq` of the `rhoapi` types
//! skips `locally_free`, so every comparison also compares the `Debug` text.
//! The random terms come from `record_corpus::random_par`, because the
//! workspace generator `generate_par` gives only empty collections on this
//! branch (see `record_corpus/mod.rs`).

use std::fmt::Debug;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, CostSignature, CostStack, ListParWithRandom, Par, ParWithRandom,
    TaggedContinuation, Var,
};
use proptest::prelude::*;
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use serde::de::DeserializeOwned;
use serde::Serialize;

mod record_corpus;
use record_corpus::random_par;

const TRAILING: [u8; 3] = [0xA5, 0x00, 0xFF];

/// The flips of one value cost one decode per byte, so only the first bytes
/// and then every seventh byte are flipped.
const DENSE_FLIP_BYTES: usize = 256;

fn agree<T>(value: &T)
where T: ColdStoreDecode + Serialize + DeserializeOwned + PartialEq + Debug {
    let bytes = bincode::serialize(value).expect("serialize the value");
    let oracle: T =
        bincode::deserialize(&bytes).expect("the derived decoder reads its own encoding");
    let (decoded, consumed) =
        T::cold_decode_prefix(&bytes).expect("the boundary reads the encoding");
    assert_eq!(
        consumed,
        bytes.len(),
        "the boundary consumes the whole encoding"
    );
    assert_eq!(decoded, oracle);
    assert_eq!(format!("{decoded:?}"), format!("{oracle:?}"));

    let mut padded = bytes.clone();
    padded.extend_from_slice(&TRAILING);
    let (decoded, consumed) =
        T::cold_decode_prefix(&padded).expect("the boundary accepts trailing bytes");
    assert_eq!(consumed, bytes.len(), "trailing bytes are not consumed");
    assert_eq!(decoded, oracle);
    assert_eq!(format!("{decoded:?}"), format!("{oracle:?}"));

    for end in 0..bytes.len() {
        assert_eq!(
            T::cold_decode(&bytes[..end]).is_ok(),
            bincode::deserialize::<T>(&bytes[..end]).is_ok(),
            "a prefix of {end} bytes"
        );
    }

    let positions =
        (0..bytes.len()).filter(|position| *position < DENSE_FLIP_BYTES || position % 7 == 0);
    for position in positions {
        let mut flipped = bytes.clone();
        flipped[position] ^= 0xFF;
        match (
            T::cold_decode(&flipped),
            bincode::deserialize::<T>(&flipped),
        ) {
            (Ok(decoded), Ok(oracle)) => {
                assert_eq!(decoded, oracle, "a flip at byte {position}");
                assert_eq!(
                    format!("{decoded:?}"),
                    format!("{oracle:?}"),
                    "a flip at byte {position}"
                );
            }
            (Err(_), Err(_)) => {}
            (decoded, oracle) => panic!(
                "a flip at byte {position}: the boundary gives {:?}, the derived decoder {:?}",
                decoded.map(|_| ()),
                oracle.map(|_| ())
            ),
        }
    }
}

fn ground_signature() -> CostSignature {
    CostSignature {
        value: Some(Value::Ground(vec![0x5A, 0x01])),
    }
}

fn list_par_with_random(pars: Vec<Par>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        random_state: vec![3, 1, 4, 1, 5],
        cost_authority: Some(CostAuthority::default()),
        cost_stack: Some(CostStack {
            cells: vec![ground_signature()],
        }),
    }
}

fn bind_pattern(patterns: Vec<Par>) -> BindPattern {
    BindPattern {
        patterns,
        remainder: Some(Var {
            var_instance: Some(VarInstance::FreeVar(2)),
        }),
        free_count: 3,
    }
}

fn par_body_continuation(body: Par) -> TaggedContinuation {
    TaggedContinuation {
        guard: Some(Par::default()),
        cost_authority: None,
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body),
            random_state: vec![9, 9, 9, 9],
        })),
    }
}

fn scala_body_continuation() -> TaggedContinuation {
    TaggedContinuation {
        guard: None,
        cost_authority: Some(CostAuthority::default()),
        tagged_cont: Some(TaggedCont::ScalaBodyRef(-7)),
    }
}

#[test]
fn empty_roots_decode_as_the_derived_decoder_reads_them() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    agree(&Par::default());
    agree(&ListParWithRandom::default());
    agree(&BindPattern::default());
    agree(&TaggedContinuation::default());
    agree(&scala_body_continuation());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_roots_decode_as_the_derived_decoder_reads_them(
        par in random_par(2),
        other in random_par(2),
    ) {
        models::rust::record_encoding::set_record_encoding(models::rust::record_encoding::RecordEncoding::Cost);
        agree(&par);
        agree(&list_par_with_random(vec![par.clone(), other.clone()]));
        agree(&bind_pattern(vec![other.clone(), par.clone()]));
        agree(&par_body_continuation(par));
        agree(&par_body_continuation(other));
    }
}
