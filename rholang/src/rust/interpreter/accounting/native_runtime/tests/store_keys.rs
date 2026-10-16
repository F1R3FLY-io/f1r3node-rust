//! D-S1 (D-C2a, DR-96): two channels have the same native store key exactly
//! when they are equal.

use models::rhoapi::Par;
use proptest::prelude::*;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::{channel_key, StoreKey};

use crate::rust::interpreter::accounting::native_runtime::clone_backing::tests::term;

fn key(par: &Par) -> StoreKey {
    channel_key(par, &|_: usize, _: usize, _: usize| {
        Ok::<(), RSpaceError>(())
    })
    .expect("an unlimited meter accepts every charge")
}

/// A copy of `par` whose `locally_free` fields, in every `Par`, `Send` and
/// `New` that the generator builds, hold `noise`.
fn renoise(par: &Par, noise: &[u8]) -> Par {
    let mut par = par.clone();
    par.locally_free = noise.to_vec();
    for send in &mut par.sends {
        send.locally_free = noise.to_vec();
        if let Some(channel) = send.chan.as_mut() {
            *channel = renoise(channel, noise);
        }
        for data in &mut send.data {
            *data = renoise(data, noise);
        }
    }
    for new in &mut par.news {
        new.locally_free = noise.to_vec();
        if let Some(body) = new.p.as_mut() {
            *body = renoise(body, noise);
        }
        for value in new.injections.values_mut() {
            *value = renoise(value, noise);
        }
    }
    par
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// D-S1 (D-C2a, DR-96): channels that differ only in `locally_free`, at
    /// every level, are equal and have the same key. A change to another
    /// field, at the top or in a nested send, makes them unequal and changes
    /// the key. For generated pairs, key equality is channel equality.
    #[test]
    fn channel_digest_equality_matches_partial_eq(
        par in term(),
        other in term(),
        noise in prop::collection::vec(any::<u8>(), 0..4),
    ) {
        let renoised = renoise(&par, &noise);
        prop_assert_eq!(&renoised, &par);
        prop_assert_eq!(key(&renoised), key(&par));
        let mut mutated = par.clone();
        mutated.connective_used = !mutated.connective_used;
        prop_assert_ne!(&mutated, &par);
        prop_assert_ne!(key(&mutated), key(&par));
        let mut nested = par.clone();
        if let Some(send) = nested.sends.first_mut() {
            send.persistent = !send.persistent;
            prop_assert_ne!(&nested, &par);
            prop_assert_ne!(key(&nested), key(&par));
        }
        prop_assert_eq!(par == other, key(&par) == key(&other));
    }
}
