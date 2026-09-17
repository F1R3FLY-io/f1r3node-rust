use thiserror::Error;

use super::{
    check_phlo_controls, CheckedPhloControls, PhloControlsError, PhloEnvironment, PhloSchedule,
    SignedPhloControls,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloOffer {
    pub limit: i64,
    pub price: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckedPhloOffer<'a> {
    offer: PhloOffer,
    controls: CheckedPhloControls<'a>,
}

impl<'a> CheckedPhloOffer<'a> {
    pub fn offer(self) -> PhloOffer { self.offer }

    pub fn controls(self) -> CheckedPhloControls<'a> { self.controls }
}

impl<'a> CheckedPhloControls<'a> {
    pub fn bind_offer(self, offer: PhloOffer) -> Result<CheckedPhloOffer<'a>, PhloOfferError> {
        check_offer_fields(offer, self.terms(), self.schedule())?;
        Ok(CheckedPhloOffer {
            offer,
            controls: self,
        })
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloOfferError {
    #[error("offered phlo limit must be nonnegative")]
    NegativeLimit,
    #[error("offered phlo price must be nonnegative")]
    NegativePrice,
    #[error("funding limit differs from the offered phlo limit")]
    LimitMismatch,
    #[error("schedule price differs from the offered phlo price")]
    PriceMismatch,
    #[error(transparent)]
    Controls(#[from] PhloControlsError),
}

fn check_offer_fields(
    offer: PhloOffer,
    terms: SignedPhloControls<'_>,
    schedule: PhloSchedule<'_>,
) -> Result<(), PhloOfferError> {
    let limit = u64::try_from(offer.limit).map_err(|_| PhloOfferError::NegativeLimit)?;
    let price = u64::try_from(offer.price).map_err(|_| PhloOfferError::NegativePrice)?;
    if limit != terms.limit {
        return Err(PhloOfferError::LimitMismatch);
    }
    if price != schedule.actual_price {
        return Err(PhloOfferError::PriceMismatch);
    }
    Ok(())
}

pub fn check_offered_phlo_controls<'a>(
    environment: PhloEnvironment<'_>,
    minimum_price: u64,
    machine_max: u64,
    offer: PhloOffer,
    terms: SignedPhloControls<'a>,
    schedule: PhloSchedule<'a>,
    resource_bound: u64,
) -> Result<CheckedPhloOffer<'a>, PhloOfferError> {
    check_offer_fields(offer, terms, schedule)?;
    let controls = check_phlo_controls(
        environment,
        minimum_price,
        machine_max,
        terms,
        schedule,
        resource_bound,
    )?;
    Ok(CheckedPhloOffer { offer, controls })
}

#[cfg(test)]
mod tests;
