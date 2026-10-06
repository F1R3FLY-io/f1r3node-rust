use std::mem::size_of;

use models::rhoapi::{ListParWithRandom, Par};
use models::rust::host_work::{HostWorkDimension, HostWorkReservationError, HostWorkUnits};
use models::rust::phlo_intent::PhloFundingIntentV2;
use models::rust::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireLimits};
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::rho_type::{RhoByteArray, RhoList};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::native_reader::{
    decode_record, NativeLeafKind, NativeReadCharge, NativeReadError, NativeReadMeter,
};
use rspace_plus_plus::rspace::internal::Datum;
use thiserror::Error;

use super::{
    check_separate_prior, offered_conversion_plan_limits, ConversionPlanError, RootedAcceptedTrade,
    RootedPhysicalCapacity,
};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

const ACCEPTED_TRADE_DOMAIN: &[u8] = b"f1r3node:accepted-conversion-trade:v1";
const ACCEPTED_TRADE_CHANNEL_DOMAIN: &[u8] = b"f1r3node:accepted-conversion-trade:state:v1";

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AcceptedTradeStateError {
    #[error("accepted trade native issuer is unavailable or receipt is absent")]
    IssuerUnavailable,
    #[error("accepted trade belongs to another authenticated root")]
    Root,
    #[error("accepted trade record is malformed or noncanonical")]
    Record,
    #[error("accepted trade history lookup failed")]
    History,
    #[error("accepted trade receipt transition is stale")]
    Stale,
    #[error("accepted trade receipt transition exceeds its authorized amount")]
    Amount,
    #[error(transparent)]
    Plan(#[from] ConversionPlanError),
    #[error(transparent)]
    HostBudget(#[from] HostWorkReservationError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedAcceptedTradeTransition {
    funding_root: [u8; 32],
    trade_id: [u8; 32],
    expected: Vec<u8>,
    replacement: Vec<u8>,
    realized_debit: u128,
}

impl PreparedAcceptedTradeTransition {
    pub fn funding_root(&self) -> [u8; 32] { self.funding_root }
    pub fn trade_id(&self) -> [u8; 32] { self.trade_id }
    pub fn expected(&self) -> &[u8] { &self.expected }
    pub fn replacement(&self) -> &[u8] { &self.replacement }
    pub fn realized_debit(&self) -> u128 { self.realized_debit }

    pub fn check_live_old_value(
        &self,
        authenticated_funding_root: [u8; 32],
        live: Option<&[u8]>,
    ) -> Result<(), AcceptedTradeStateError> {
        if authenticated_funding_root != self.funding_root {
            return Err(AcceptedTradeStateError::Root);
        }
        if live != Some(self.expected.as_slice()) {
            return Err(AcceptedTradeStateError::Stale);
        }
        Ok(())
    }
}

struct Receipt<'a> {
    trade_id: [u8; 32],
    source_identity: [u8; 32],
    custody: &'a [u8],
    asset: &'a [u8],
    unspent: u128,
}

impl Receipt<'_> {
    fn limits() -> PhloWireLimits {
        let limits = offered_conversion_plan_limits();
        PhloWireLimits {
            total_bytes: limits.snapshot_bytes,
            field_bytes: limits.identity_bytes,
        }
    }

    fn encode(&self) -> Result<Vec<u8>, AcceptedTradeStateError> {
        let limits = Self::limits();
        if self.custody.is_empty()
            || self.asset.is_empty()
            || self.custody.len() > limits.field_bytes
            || self.asset.len() > limits.field_bytes
        {
            return Err(AcceptedTradeStateError::Record);
        }
        let mut wire = PhloWireEncoder::new(limits);
        for field in [
            ACCEPTED_TRADE_DOMAIN,
            self.trade_id.as_slice(),
            self.source_identity.as_slice(),
            self.custody,
            self.asset,
            self.unspent.to_be_bytes().as_slice(),
        ] {
            wire.bytes(field)
                .map_err(|_| AcceptedTradeStateError::Record)?;
        }
        Ok(wire.into_bytes())
    }

    fn decode(bytes: &[u8]) -> Result<Receipt<'_>, AcceptedTradeStateError> {
        let mut wire = PhloWireDecoder::new(bytes, Self::limits())
            .map_err(|_| AcceptedTradeStateError::Record)?;
        if wire.bytes().map_err(|_| AcceptedTradeStateError::Record)? != ACCEPTED_TRADE_DOMAIN {
            return Err(AcceptedTradeStateError::Record);
        }
        let trade_id: [u8; 32] = wire
            .bytes()
            .map_err(|_| AcceptedTradeStateError::Record)?
            .try_into()
            .map_err(|_| AcceptedTradeStateError::Record)?;
        let source_identity: [u8; 32] = wire
            .bytes()
            .map_err(|_| AcceptedTradeStateError::Record)?
            .try_into()
            .map_err(|_| AcceptedTradeStateError::Record)?;
        let custody = wire.bytes().map_err(|_| AcceptedTradeStateError::Record)?;
        let asset = wire.bytes().map_err(|_| AcceptedTradeStateError::Record)?;
        let unspent = u128::from_be_bytes(
            wire.bytes()
                .map_err(|_| AcceptedTradeStateError::Record)?
                .try_into()
                .map_err(|_| AcceptedTradeStateError::Record)?,
        );
        wire.finish().map_err(|_| AcceptedTradeStateError::Record)?;
        let receipt = Receipt {
            trade_id,
            source_identity,
            custody,
            asset,
            unspent,
        };
        if receipt.encode()? != bytes {
            return Err(AcceptedTradeStateError::Record);
        }
        Ok(receipt)
    }
}

fn channel(trade_id: [u8; 32]) -> Par {
    RhoList::create_par(vec![
        models::rust::utils::new_gsys_auth_token_par(Vec::new(), false),
        RhoByteArray::create_par(ACCEPTED_TRADE_CHANNEL_DOMAIN.to_vec()),
        RhoByteArray::create_par(trade_id.to_vec()),
    ])
}

struct Meter<'a>(&'a HostWorkBudget);

impl NativeReadMeter for Meter<'_> {
    type Error = AcceptedTradeStateError;

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error> {
        for (dimension, amount) in [
            (HostWorkDimension::VerificationOperations, charge.operations),
            (HostWorkDimension::VerificationBytes, charge.scanned_bytes),
            (HostWorkDimension::SearchStateBytes, charge.backing_bytes),
        ] {
            self.0.reserve(
                dimension,
                HostWorkUnits::new(
                    u64::try_from(amount).map_err(|_| AcceptedTradeStateError::Record)?,
                ),
            )?;
        }
        Ok(())
    }
}

fn read_error(error: NativeReadError<AcceptedTradeStateError>) -> AcceptedTradeStateError {
    match error {
        NativeReadError::Host(error) | NativeReadError::Consumer(error) => error,
        NativeReadError::Store(_) | NativeReadError::Invalid(_) => AcceptedTradeStateError::History,
    }
}

impl RuntimeManager {
    pub fn read_accepted_trade_receipt(
        &self,
        root: [u8; 32],
        trade_id: [u8; 32],
        budget: &HostWorkBudget,
    ) -> Result<RootedAcceptedTrade, AcceptedTradeStateError> {
        Meter(budget).reserve(NativeReadCharge {
            operations: 2,
            scanned_bytes: 32,
            backing_bytes: size_of::<RootedAcceptedTrade>(),
        })?;
        let root_hash = Blake2b256Hash::from_bytes(root.to_vec());
        if !self
            .history_repo
            .contains_root(&root_hash)
            .map_err(|_| AcceptedTradeStateError::History)?
        {
            return Err(AcceptedTradeStateError::Root);
        }
        let name = channel(trade_id);
        let channel_hash = stable_hash_provider::hash(&name);
        let hash: [u8; 32] = channel_hash
            .0
            .as_slice()
            .try_into()
            .map_err(|_| AcceptedTradeStateError::History)?;
        let reader = self.history_repo.native_history_reader(root);
        let observed = reader
            .with_records(NativeLeafKind::Data, &hash, &Meter(budget), |records| {
                if records.len() != 1 {
                    return Err(AcceptedTradeStateError::Record);
                }
                let raw = records
                    .iter()
                    .next()
                    .ok_or(AcceptedTradeStateError::Record)?;
                let datum: Datum<ListParWithRandom> =
                    decode_record(raw, &Meter(budget)).map_err(read_error)?;
                let [par] = datum.a.pars.as_slice() else {
                    return Err(AcceptedTradeStateError::Record);
                };
                let [models::rhoapi::Expr {
                    expr_instance: Some(models::rhoapi::expr::ExprInstance::GByteArray(bytes)),
                }] = par.exprs.as_slice()
                else {
                    return Err(AcceptedTradeStateError::Record);
                };
                Meter(budget).reserve(NativeReadCharge {
                    operations: 1,
                    scanned_bytes: bytes.len(),
                    backing_bytes: bytes
                        .len()
                        .checked_mul(2)
                        .ok_or(AcceptedTradeStateError::Record)?,
                })?;
                let receipt = Receipt::decode(bytes)?;
                let copy_bytes = bytes
                    .len()
                    .checked_mul(4)
                    .and_then(|size| size.checked_add(receipt.custody.len()))
                    .and_then(|size| size.checked_add(receipt.asset.len()))
                    .ok_or(AcceptedTradeStateError::Record)?;
                Meter(budget).reserve(NativeReadCharge {
                    operations: 2,
                    scanned_bytes: bytes.len(),
                    backing_bytes: copy_bytes,
                })?;
                if receipt.trade_id != trade_id
                    || datum
                        != Datum::create(
                            &name,
                            ListParWithRandom {
                                pars: vec![RhoByteArray::create_par(bytes.to_vec())],
                                random_state: Vec::new(),
                                cost_authority: None,
                                cost_stack: None,
                            },
                            false,
                        )
                {
                    return Err(AcceptedTradeStateError::Record);
                }
                Ok(RootedAcceptedTrade {
                    root,
                    trade_id,
                    output_source_identity: receipt.source_identity,
                    output_custody: receipt.custody.to_vec(),
                    output_asset: receipt.asset.to_vec(),
                    unspent_output: receipt.unspent,
                })
            })
            .map_err(read_error)?;
        observed.ok_or(AcceptedTradeStateError::IssuerUnavailable)
    }
}

pub fn plan_separate_prior_receipt_transition(
    funding_root: [u8; 32],
    intent: &PhloFundingIntentV2<'_>,
    trade: &RootedAcceptedTrade,
    output: &RootedPhysicalCapacity,
    authorized_hold: u128,
    realized_debit: u128,
    budget: &HostWorkBudget,
) -> Result<Option<PreparedAcceptedTradeTransition>, AcceptedTradeStateError> {
    check_separate_prior(funding_root, intent, trade, output, authorized_hold, budget)?;
    if realized_debit > authorized_hold {
        return Err(AcceptedTradeStateError::Amount);
    }
    if realized_debit == 0 {
        return Ok(None);
    }
    let replacement_unspent = trade
        .unspent_output
        .checked_sub(realized_debit)
        .ok_or(AcceptedTradeStateError::Amount)?;
    let record_bytes = ACCEPTED_TRADE_DOMAIN
        .len()
        .checked_add(32 + 32 + 16 + 6 * 8)
        .and_then(|size| size.checked_add(trade.output_custody.len()))
        .and_then(|size| size.checked_add(trade.output_asset.len()))
        .ok_or(AcceptedTradeStateError::Record)?;
    Meter(budget).reserve(NativeReadCharge {
        operations: 2,
        scanned_bytes: record_bytes,
        backing_bytes: record_bytes
            .checked_mul(4)
            .ok_or(AcceptedTradeStateError::Record)?,
    })?;
    let old = Receipt {
        trade_id: trade.trade_id,
        source_identity: trade.output_source_identity,
        custody: &trade.output_custody,
        asset: &trade.output_asset,
        unspent: trade.unspent_output,
    }
    .encode()?;
    let new = Receipt {
        trade_id: trade.trade_id,
        source_identity: trade.output_source_identity,
        custody: &trade.output_custody,
        asset: &trade.output_asset,
        unspent: replacement_unspent,
    }
    .encode()?;
    Meter(budget).reserve(NativeReadCharge {
        operations: 4,
        scanned_bytes: old.len() + new.len(),
        backing_bytes: old.len() + new.len(),
    })?;
    Ok(Some(PreparedAcceptedTradeTransition {
        funding_root,
        trade_id: trade.trade_id,
        expected: old,
        replacement: new,
        realized_debit,
    }))
}

#[cfg(test)]
mod tests;
