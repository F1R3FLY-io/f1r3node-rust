use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, ListParWithRandom, Par};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::channel::wallet_channel_id;
use super::ops::CellStore;
use super::owner::WalletOwner;
use super::{RevAmount, RevStackError, MAX_AMOUNT};
use crate::rust::interpreter::rho_type::{RhoNumber, RhoString, RhoTuple2};

pub const RESERVATION_DOMAIN: &[u8] = b"f1r3fly/rev-stack/reservation/v1";

const RESERVATION_TAG: &str = "reserved";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reservation {
    pub visible: RevAmount,
    pub held: RevAmount,
}

pub fn reservation_channel(owner: &WalletOwner) -> Par {
    let mut input = RESERVATION_DOMAIN.to_vec();
    input.extend_from_slice(&wallet_channel_id(owner));
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: Blake2b256::hash(input),
        })),
    }])
}

pub fn reservation(
    store: &dyn CellStore,
    owner: &WalletOwner,
) -> Result<Reservation, RevStackError> {
    let data = store.get_data(&reservation_channel(owner));
    match data.as_slice() {
        [] => Ok(Reservation::default()),
        [datum] => decode(datum)
            .ok_or_else(|| RevStackError::CorruptWallet("a reservation datum is malformed".into())),
        _ => Err(RevStackError::CorruptWallet(
            "more than one reservation datum".into(),
        )),
    }
}

pub fn set_reservation(
    store: &dyn CellStore,
    owner: &WalletOwner,
    reservation: Reservation,
) -> Result<(), RevStackError> {
    clear_reservation(store, owner)?;
    if reservation == Reservation::default() {
        return Ok(());
    }
    for amount in [reservation.visible, reservation.held] {
        if amount > MAX_AMOUNT {
            return Err(RevStackError::InvalidAmount(amount));
        }
    }
    let channel = reservation_channel(owner);
    let a = ListParWithRandom {
        pars: vec![RhoTuple2::create_par((
            RhoString::create_par(RESERVATION_TAG.to_string()),
            RhoTuple2::create_par((
                RhoNumber::create_par(reservation.visible as i64),
                RhoNumber::create_par(reservation.held as i64),
            )),
        ))],
        random_state: Vec::new(),
    };
    let source = Produce::create(&channel, &a, false);
    store.put_datum(
        &channel,
        Datum {
            a,
            persist: false,
            source,
        },
    );
    Ok(())
}

pub fn clear_reservation(store: &dyn CellStore, owner: &WalletOwner) -> Result<(), RevStackError> {
    let channel = reservation_channel(owner);
    let n = store.get_data(&channel).len();
    for index in (0..n).rev() {
        store.remove_datum(&channel, index as i32)?;
    }
    Ok(())
}

fn decode(datum: &Datum<ListParWithRandom>) -> Option<Reservation> {
    if datum.persist {
        return None;
    }
    let [par] = datum.a.pars.as_slice() else {
        return None;
    };
    let (tag, value) = RhoTuple2::unapply(par)?;
    if RhoString::unapply(&tag)?.as_str() != RESERVATION_TAG {
        return None;
    }
    let (visible, held) = RhoTuple2::unapply(&value)?;
    Some(Reservation {
        visible: RevAmount::try_from(RhoNumber::unapply(&visible)?).ok()?,
        held: RevAmount::try_from(RhoNumber::unapply(&held)?).ok()?,
    })
}
