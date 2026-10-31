use models::rhoapi::{ListParWithRandom, Par};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::{RevAmount, RevStackError, MAX_AMOUNT};
use crate::rust::interpreter::rho_type::{RhoNumber, RhoString, RhoTuple2};

pub const CELL_TAG: &str = "rev";

pub fn make_cell(
    channel: &Par,
    amount: RevAmount,
) -> Result<Datum<ListParWithRandom>, RevStackError> {
    if amount == 0 || amount > MAX_AMOUNT {
        return Err(RevStackError::InvalidAmount(amount));
    }
    let a = ListParWithRandom {
        pars: vec![RhoTuple2::create_par((
            RhoString::create_par(CELL_TAG.to_string()),
            RhoNumber::create_par(amount as i64),
        ))],
        random_state: Vec::new(),
    };
    let source = Produce::create(channel, &a, false);
    Ok(Datum {
        a,
        persist: false,
        source,
    })
}

pub fn cell_amount(datum: &Datum<ListParWithRandom>) -> Option<RevAmount> {
    if datum.persist {
        return None;
    }
    let [par] = datum.a.pars.as_slice() else {
        return None;
    };
    let (tag, amount) = RhoTuple2::unapply(par)?;
    if RhoString::unapply(&tag)?.as_str() != CELL_TAG {
        return None;
    }
    let amount = RhoNumber::unapply(&amount)?;
    if amount <= 0 {
        return None;
    }
    Some(amount as RevAmount)
}
