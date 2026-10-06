//! D-S2 (DR-95): History-mode decoding of the RSpace history records that
//! hold rhoapi terms.

use std::cell::Cell;

use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    cost_signature, BindPattern, CostAuthority, CostRegion, CostSignature, CostStack,
    ListParWithRandom, ParWithRandom, TaggedContinuation, Var,
};
use proptest::prelude::*;
use rspace_plus_plus::rspace::history::native_reader::{
    decode_history_record, decode_record, NativeReadCharge, NativeReadMeter,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use serde::de::DeserializeOwned;
use serde::Serialize;
use shared::rust::closed_decode::ClosedDecode;

use crate::rust::interpreter::accounting::native_runtime::clone_backing::tests::{measured, term};

/// Sums the reserved backing bytes without allocating.
struct Backing(Cell<usize>);

impl NativeReadMeter for Backing {
    type Error = ();

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), ()> {
        self.0
            .set(self.0.get().checked_add(charge.backing_bytes).ok_or(())?);
        Ok(())
    }
}

/// Decodes the history bytes of `value` in History mode and in the legacy
/// mode. Returns the History reservation, the legacy reservation and the
/// bytes that the History decode allocated.
fn decode_both<T>(value: &T) -> (usize, usize, usize)
where T: Serialize + DeserializeOwned + ClosedDecode {
    let bytes = bincode::serialize(value).expect("history record encodes");
    let history = Backing(Cell::new(0));
    let (decoded, allocated) = measured(|| decode_history_record::<T, _>(&bytes, &history));
    let decoded = decoded.expect("History mode decodes the record");
    assert_eq!(
        bincode::serialize(&decoded).expect("decoded record encodes"),
        bytes
    );
    let legacy = Backing(Cell::new(0));
    let reference = decode_record::<T, _>(&bytes, &legacy).expect("legacy mode decodes the record");
    assert_eq!(
        bincode::serialize(&reference).expect("reference record encodes"),
        bytes
    );
    (history.0.get(), legacy.0.get(), allocated)
}

fn pattern() -> impl Strategy<Value = BindPattern> {
    (
        prop::collection::vec(term(), 0..3),
        prop::option::of(any::<i32>()),
        any::<i32>(),
    )
        .prop_map(|(patterns, remainder, free_count)| BindPattern {
            patterns,
            remainder: remainder.map(|index| Var {
                var_instance: Some(VarInstance::FreeVar(index)),
            }),
            free_count,
        })
}

fn signature() -> impl Strategy<Value = CostSignature> {
    term().prop_map(|par| CostSignature {
        value: Some(cost_signature::Value::Quote(par)),
    })
}

fn authority() -> impl Strategy<Value = Option<CostAuthority>> {
    prop::option::of(
        prop::collection::vec(
            (prop::collection::vec(any::<u8>(), 0..33), signature()).prop_map(
                |(instance_id, signature)| CostRegion {
                    instance_id,
                    signature: Some(signature),
                },
            ),
            0..3,
        )
        .prop_map(|regions| CostAuthority { regions }),
    )
}

fn continuation() -> impl Strategy<Value = TaggedContinuation> {
    let body = prop_oneof![
        (term(), prop::collection::vec(any::<u8>(), 0..64)).prop_map(|(body, random_state)| {
            TaggedCont::ParBody(ParWithRandom {
                body: Some(body),
                random_state,
            })
        }),
        any::<i64>().prop_map(TaggedCont::ScalaBodyRef),
    ];
    (prop::option::of(term()), authority(), body).prop_map(|(guard, cost_authority, body)| {
        TaggedContinuation {
            guard,
            cost_authority,
            tagged_cont: Some(body),
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// D-S2 (DR-95): for generated waiting continuations, data and joined
    /// channel lists, History mode decodes the encoded value, reserves backing
    /// for every byte that the decode allocates, and reserves no more than the
    /// legacy rule.
    #[test]
    fn history_records_cover_actual_allocations(
        channels in prop::collection::vec(term(), 1..3),
        patterns in prop::collection::vec(pattern(), 1..3),
        continuation in continuation(),
        peeks in prop::collection::btree_set(any::<i32>(), 0..8),
        data in prop::collection::vec(term(), 0..3),
        random_state in prop::collection::vec(any::<u8>(), 0..64),
        cost_authority in authority(),
        cost_stack in prop::option::of(prop::collection::vec(signature(), 0..3)),
        persist in any::<bool>(),
    ) {
        let waiting = WaitingContinuation::create(&channels, &patterns, &continuation, persist, peeks);
        let datum = Datum::create(
            &channels[0],
            ListParWithRandom {
                pars: data.clone(),
                random_state,
                cost_authority,
                cost_stack: cost_stack.map(|cells| CostStack { cells }),
            },
            persist,
        );
        for (history, legacy, allocated) in [decode_both(&waiting), decode_both(&datum), decode_both(&data)] {
            prop_assert!(allocated <= history, "allocated {} reserved {}", allocated, history);
            prop_assert!(history <= legacy, "history {} legacy {}", history, legacy);
        }
    }
}
