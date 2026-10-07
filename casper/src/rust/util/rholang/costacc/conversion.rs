use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::host_work::{HostWorkDimension, HostWorkReservationError, HostWorkUnits};
use models::rust::phlo_intent::{
    quote_evidence_commitment, PhloConversionCompositionV2, PhloFundingIntentV2,
};
use models::rust::phlo_quote_v2::{
    PhloQuoteAmountV2, PhloQuoteEvidenceV2, PhloQuoteUseV2, PhloQuoteV2Error,
};
use models::rust::phlo_wire::PhloWireLimits;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use thiserror::Error;

mod accepted_trade;
pub use accepted_trade::{
    plan_separate_prior_receipt_transition, AcceptedTradeStateError,
    PreparedAcceptedTradeTransition,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootedPhysicalCapacity {
    root: [u8; 32],
    source_identity: [u8; 32],
    custody: Vec<u8>,
    asset: Vec<u8>,
    available: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootedInputPermission {
    root: [u8; 32],
    quote_commitment: [u8; 32],
    authority: Vec<u8>,
    source: RootedPhysicalCapacity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootedQuoteEscrow {
    root: [u8; 32],
    quote_commitment: [u8; 32],
    quote_id: [u8; 32],
    provider_authority: Vec<u8>,
    source: RootedPhysicalCapacity,
    prior_output_used: u128,
    accepted_operations: BTreeSet<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootedAcceptedTrade {
    root: [u8; 32],
    trade_id: [u8; 32],
    output_source_identity: [u8; 32],
    output_custody: Vec<u8>,
    output_asset: Vec<u8>,
    unspent_output: u128,
}

#[derive(Clone, Debug)]
pub struct AtomicQuoteUse<'a> {
    pub intent: &'a PhloFundingIntentV2<'a>,
    pub input: &'a RootedInputPermission,
    pub provider: &'a RootedQuoteEscrow,
    pub operation_id: [u8; 32],
    pub maximum_output: u128,
    pub realized_output: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedAtomicQuoteUse {
    pub operation_id: [u8; 32],
    pub quote_commitment: [u8; 32],
    pub input_custody: Vec<u8>,
    pub input_asset: Vec<u8>,
    pub provider_custody: Vec<u8>,
    pub output_asset: Vec<u8>,
    pub output_recipient: Vec<u8>,
    pub conversion_fee_recipient: Vec<u8>,
    pub input_hold: u128,
    pub provider_hold: u128,
    pub amount: PhloQuoteAmountV2,
    pub input_release: u128,
    pub provider_release: u128,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ConversionPlanError {
    #[error("conversion exceeds its protocol work or evidence limits")]
    ProtocolLimit,
    #[error("conversion composition does not match the requested planner")]
    Composition,
    #[error("conversion evidence is not bound to the selected root")]
    Root,
    #[error("conversion authority, custody, asset, or quote commitment differs")]
    Provenance,
    #[error("conversion quote is outside the authenticated context or validity interval")]
    Context,
    #[error("conversion contains a duplicate accepted operation")]
    Duplicate,
    #[error("conversion exceeds the quote use rule or signed exposure")]
    QuoteCapacity,
    #[error("conversion claims more than authenticated physical capacity")]
    PhysicalCapacity,
    #[error("conversion arithmetic exceeds u128")]
    Overflow,
    #[error(transparent)]
    Quote(#[from] PhloQuoteV2Error),
    #[error(transparent)]
    HostBudget(#[from] HostWorkReservationError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConversionPlanLimits {
    pub uses: usize,
    pub accepted_operations: usize,
    pub identity_bytes: usize,
    pub snapshot_bytes: usize,
    pub quote_wire: PhloWireLimits,
}

pub fn offered_conversion_plan_limits() -> ConversionPlanLimits {
    let protocol = offered_funded_v6_limits();
    let funding = protocol.envelope.payload.funding;
    ConversionPlanLimits {
        uses: funding.sources,
        accepted_operations: protocol.funding_case.obligations,
        identity_bytes: funding.wire.field_bytes,
        snapshot_bytes: protocol.evidence.total_bytes,
        quote_wire: PhloWireLimits {
            total_bytes: funding.wire.field_bytes,
            field_bytes: funding.wire.field_bytes,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConversionContext<'a> {
    pub root: [u8; 32],
    pub network: &'a [u8],
    pub shard: &'a [u8],
    pub context_commitment: [u8; 32],
    pub accepted_height: u64,
}

pub fn plan_atomic_quote_uses(
    context: ConversionContext<'_>,
    uses: &[AtomicQuoteUse<'_>],
    budget: &HostWorkBudget,
) -> Result<Vec<PlannedAtomicQuoteUse>, ConversionPlanError> {
    let limits = offered_conversion_plan_limits();
    if uses.len() > limits.uses {
        return Err(ConversionPlanError::ProtocolLimit);
    }
    reserve(budget, HostWorkDimension::SearchCandidates, uses.len())?;
    let plan_bytes = uses
        .len()
        .checked_mul(size_of::<PlannedAtomicQuoteUse>() + 2048)
        .ok_or(ConversionPlanError::Overflow)?;
    reserve(budget, HostWorkDimension::SearchStateBytes, plan_bytes)?;
    let mut operations = BTreeSet::new();
    let mut quote_uses =
        BTreeMap::<[u8; 32], ([u8; 32], u128, &BTreeSet<[u8; 32]>, u128, usize)>::new();
    let mut physical = BTreeMap::<(Vec<u8>, Vec<u8>), ([u8; 32], u128, u128)>::new();
    let mut plans = Vec::new();
    plans
        .try_reserve(uses.len())
        .map_err(|_| ConversionPlanError::Overflow)?;
    for use_ in uses {
        reserve(budget, HostWorkDimension::VerificationOperations, 64)?;
        if !operations.insert(use_.operation_id) {
            return Err(ConversionPlanError::Duplicate);
        }
        let PhloConversionCompositionV2::AtomicQuote {
            quote_commitment,
            quote_evidence,
            output_source_index,
            input_custody,
            input_asset,
            provider_custody,
            output_asset,
            max_input_debit,
            max_output_debit,
            input_hold_cap,
            provider_hold_cap,
        } = &use_.intent.conversion
        else {
            return Err(ConversionPlanError::Composition);
        };
        if quote_evidence.len() > limits.quote_wire.total_bytes {
            return Err(ConversionPlanError::ProtocolLimit);
        }
        reserve(
            budget,
            HostWorkDimension::VerificationBytes,
            quote_evidence
                .len()
                .checked_mul(2)
                .ok_or(ConversionPlanError::Overflow)?,
        )?;
        let quote = PhloQuoteEvidenceV2::decode(quote_evidence, limits.quote_wire)?;
        let recipient = use_
            .intent
            .base
            .sources
            .get(*output_source_index as usize)
            .ok_or(ConversionPlanError::Provenance)?
            .custody();
        if quote_evidence_commitment(quote_evidence) != *quote_commitment
            || quote.schedule_commitment != use_.intent.base.schedule_commitment
            || quote.input_custody != *input_custody
            || quote.input_asset != *input_asset
            || quote.provider_custody != *provider_custody
            || quote.output_asset != *output_asset
            || quote.output_recipient != recipient
            || quote.max_input_debit != *max_input_debit
            || quote.max_output != *max_output_debit
            || quote.input_hold_cap != *input_hold_cap
            || quote.provider_hold_cap != *provider_hold_cap
        {
            return Err(ConversionPlanError::Provenance);
        }
        if quote.network != context.network
            || quote.shard != context.shard
            || quote.context_commitment != context.context_commitment
            || quote
                .valid_from
                .is_some_and(|from| context.accepted_height < from)
            || quote
                .valid_until
                .is_some_and(|until| context.accepted_height > until)
        {
            return Err(ConversionPlanError::Context);
        }
        let input = use_.input;
        let provider = use_.provider;
        let identity_bytes = [
            input.authority.len(),
            input.source.custody.len(),
            input.source.asset.len(),
            provider.provider_authority.len(),
            provider.source.custody.len(),
            provider.source.asset.len(),
            recipient.len(),
            quote.fee_recipient.len(),
        ]
        .into_iter()
        .try_fold(0usize, |sum, length| {
            if length > limits.identity_bytes {
                return Err(ConversionPlanError::ProtocolLimit);
            }
            sum.checked_add(length).ok_or(ConversionPlanError::Overflow)
        })?;
        let snapshot_bytes = provider
            .accepted_operations
            .len()
            .checked_mul(32)
            .and_then(|size| size.checked_add(identity_bytes))
            .ok_or(ConversionPlanError::Overflow)?;
        if provider.accepted_operations.len() > limits.accepted_operations
            || snapshot_bytes > limits.snapshot_bytes
        {
            return Err(ConversionPlanError::ProtocolLimit);
        }
        reserve(budget, HostWorkDimension::VerificationBytes, snapshot_bytes)?;
        reserve(
            budget,
            HostWorkDimension::SearchStateBytes,
            identity_bytes
                .checked_mul(3)
                .ok_or(ConversionPlanError::Overflow)?,
        )?;
        if input.root != context.root
            || input.source.root != context.root
            || provider.root != context.root
            || provider.source.root != context.root
        {
            return Err(ConversionPlanError::Root);
        }
        if input.quote_commitment != *quote_commitment
            || input.authority != quote.input_authority
            || input.source.custody != quote.input_custody
            || input.source.asset != quote.input_asset
            || provider.quote_commitment != *quote_commitment
            || provider.quote_id != quote.quote_id
            || provider.provider_authority != quote.provider_authority
            || provider.source.custody != quote.provider_custody
            || provider.source.asset != quote.output_asset
        {
            return Err(ConversionPlanError::Provenance);
        }
        if provider.accepted_operations.contains(&use_.operation_id) {
            return Err(ConversionPlanError::Duplicate);
        }
        if use_.maximum_output > *max_output_debit
            || use_.maximum_output > *provider_hold_cap
            || use_.realized_output > use_.maximum_output
        {
            return Err(ConversionPlanError::QuoteCapacity);
        }
        let maximum = quote.evaluate_exact_output(use_.maximum_output)?;
        let amount = quote.evaluate_exact_output(use_.realized_output)?;
        if maximum.total_input > *input_hold_cap || amount.total_input > *max_input_debit {
            return Err(ConversionPlanError::QuoteCapacity);
        }
        let quote_state = quote_uses.entry(quote.quote_id).or_insert_with(|| {
            (
                *quote_commitment,
                provider.prior_output_used,
                &provider.accepted_operations,
                0,
                0,
            )
        });
        if quote_state.0 != *quote_commitment
            || quote_state.2 != &provider.accepted_operations
            || quote_state.1 != provider.prior_output_used
        {
            return Err(ConversionPlanError::Provenance);
        }
        let already_used = quote_state
            .1
            .checked_add(quote_state.3)
            .ok_or(ConversionPlanError::Overflow)?;
        match quote.use_policy {
            PhloQuoteUseV2::Once
                if already_used > 0 || !quote_state.2.is_empty() || quote_state.4 > 0 =>
            {
                return Err(ConversionPlanError::QuoteCapacity);
            }
            PhloQuoteUseV2::Cumulative { output_ceiling }
                if already_used
                    .checked_add(use_.maximum_output)
                    .ok_or(ConversionPlanError::Overflow)?
                    > output_ceiling =>
            {
                return Err(ConversionPlanError::QuoteCapacity);
            }
            _ => {}
        }
        quote_state.3 = quote_state
            .3
            .checked_add(use_.maximum_output)
            .ok_or(ConversionPlanError::Overflow)?;
        quote_state.4 += 1;
        reserve_physical(&mut physical, &input.source, maximum.total_input)?;
        reserve_physical(&mut physical, &provider.source, use_.maximum_output)?;
        plans.push(PlannedAtomicQuoteUse {
            operation_id: use_.operation_id,
            quote_commitment: *quote_commitment,
            input_custody: input.source.custody.clone(),
            input_asset: input.source.asset.clone(),
            provider_custody: provider.source.custody.clone(),
            output_asset: provider.source.asset.clone(),
            output_recipient: recipient.to_vec(),
            conversion_fee_recipient: quote.fee_recipient.to_vec(),
            input_hold: maximum.total_input,
            provider_hold: use_.maximum_output,
            amount,
            input_release: maximum.total_input - amount.total_input,
            provider_release: use_.maximum_output - use_.realized_output,
        });
    }
    Ok(plans)
}

fn reserve(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), ConversionPlanError> {
    let units = u64::try_from(amount).map_err(|_| ConversionPlanError::Overflow)?;
    budget.reserve(dimension, HostWorkUnits::new(units))?;
    Ok(())
}

fn reserve_physical(
    physical: &mut BTreeMap<(Vec<u8>, Vec<u8>), ([u8; 32], u128, u128)>,
    source: &RootedPhysicalCapacity,
    hold: u128,
) -> Result<(), ConversionPlanError> {
    let entry = physical
        .entry((source.custody.clone(), source.asset.clone()))
        .or_insert((source.source_identity, source.available, 0));
    if entry.0 != source.source_identity || entry.1 != source.available {
        return Err(ConversionPlanError::Provenance);
    }
    entry.2 = entry
        .2
        .checked_add(hold)
        .ok_or(ConversionPlanError::Overflow)?;
    if entry.2 > entry.1 {
        return Err(ConversionPlanError::PhysicalCapacity);
    }
    Ok(())
}

pub fn check_separate_prior(
    context_root: [u8; 32],
    intent: &PhloFundingIntentV2<'_>,
    trade: &RootedAcceptedTrade,
    output: &RootedPhysicalCapacity,
    required_hold: u128,
    budget: &HostWorkBudget,
) -> Result<(), ConversionPlanError> {
    let limits = offered_conversion_plan_limits();
    let provenance_bytes = trade
        .output_custody
        .len()
        .checked_add(trade.output_asset.len())
        .and_then(|length| length.checked_add(output.custody.len()))
        .and_then(|length| length.checked_add(output.asset.len()))
        .ok_or(ConversionPlanError::Overflow)?;
    if [
        trade.output_custody.len(),
        trade.output_asset.len(),
        output.custody.len(),
        output.asset.len(),
    ]
    .into_iter()
    .any(|length| length > limits.identity_bytes)
        || provenance_bytes > limits.snapshot_bytes
    {
        return Err(ConversionPlanError::ProtocolLimit);
    }
    reserve(
        budget,
        HostWorkDimension::VerificationBytes,
        provenance_bytes,
    )?;
    reserve(budget, HostWorkDimension::VerificationOperations, 16)?;
    let PhloConversionCompositionV2::SeparatePrior {
        accepted_trade,
        output_source_index,
        output_asset,
    } = &intent.conversion
    else {
        return Err(ConversionPlanError::Composition);
    };
    let custody = intent
        .base
        .sources
        .get(*output_source_index as usize)
        .ok_or(ConversionPlanError::Provenance)?
        .custody();
    if trade.root != context_root || output.root != context_root {
        return Err(ConversionPlanError::Root);
    }
    if trade.trade_id != *accepted_trade
        || trade.output_source_identity != output.source_identity
        || trade.output_asset != *output_asset
        || trade.output_custody != custody
        || output.asset != *output_asset
        || output.custody != custody
    {
        return Err(ConversionPlanError::Provenance);
    }
    if required_hold > trade.unspent_output || required_hold > output.available {
        return Err(ConversionPlanError::PhysicalCapacity);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
