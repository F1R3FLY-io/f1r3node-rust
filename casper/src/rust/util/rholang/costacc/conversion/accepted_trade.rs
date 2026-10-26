use models::rhoapi::{ListParWithRandom, Par};
use models::rust::phlo_intent::PhloFundingIntentV2;
use models::rust::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireLimits};
use rholang::rust::interpreter::rho_type::{RhoByteArray, RhoList};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::native_reader::{
    decode_datum, NativeLeafKind, NativeReadError,
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

fn read_error(error: NativeReadError<AcceptedTradeStateError>) -> AcceptedTradeStateError {
    match error {
        NativeReadError::Consumer(error) => error,
        NativeReadError::Store(_) | NativeReadError::Invalid(_) => AcceptedTradeStateError::History,
    }
}

impl RuntimeManager {
    pub fn read_accepted_trade_receipt(
        &self,
        root: [u8; 32],
        trade_id: [u8; 32],
    ) -> Result<RootedAcceptedTrade, AcceptedTradeStateError> {
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
            .with_records(NativeLeafKind::Data, &hash, |records| {
                if records.len() != 1 {
                    return Err(AcceptedTradeStateError::Record);
                }
                let raw = records
                    .iter()
                    .next()
                    .ok_or(AcceptedTradeStateError::Record)?;
                let datum: Datum<ListParWithRandom> =
                    decode_datum(raw).map_err(|error| read_error(error.into()))?;
                let [par] = datum.a.pars.as_slice() else {
                    return Err(AcceptedTradeStateError::Record);
                };
                let [models::rhoapi::Expr {
                    expr_instance: Some(models::rhoapi::expr::ExprInstance::GByteArray(bytes)),
                }] = par.exprs.as_slice()
                else {
                    return Err(AcceptedTradeStateError::Record);
                };
                let receipt = Receipt::decode(bytes)?;
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
) -> Result<Option<PreparedAcceptedTradeTransition>, AcceptedTradeStateError> {
    check_separate_prior(funding_root, intent, trade, output, authorized_hold)?;
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
