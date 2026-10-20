use crypto::rust::hash::blake2b256::Blake2b256;
use thiserror::Error;

use super::{PhloFundingIntentError, PhloFundingIntentLimits, PhloFundingIntentV1};
use crate::rust::phlo_quote_v2::{PhloQuoteEvidenceV2, PhloQuoteV2Error};
use crate::rust::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const PHLO_FUNDING_INTENT_V2_DOMAIN: &[u8] = b"f1r3node:phlo-funding-intent:v2";
pub const PHLO_QUOTE_EVIDENCE_V2_DOMAIN: &[u8] = b"f1r3node:phlo-quote-evidence:v2";

pub fn quote_evidence_commitment(bytes: &[u8]) -> [u8; 32] {
    Blake2b256::hash_parts([PHLO_QUOTE_EVIDENCE_V2_DOMAIN, bytes])
        .try_into()
        .expect("Blake2b256 always produces 32 bytes")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloFundingIntentV2Limits {
    pub wire: PhloWireLimits,
    pub base: PhloFundingIntentLimits,
    pub grant_uses: usize,
    pub grant_id_bytes: usize,
    pub quote_evidence_bytes: usize,
}

impl PhloFundingIntentV2Limits {
    fn nested(self) -> PhloWireLimits {
        PhloWireLimits {
            total_bytes: self.wire.field_bytes,
            field_bytes: self.wire.field_bytes,
        }
    }

    fn base(self) -> PhloFundingIntentLimits {
        PhloFundingIntentLimits {
            wire: PhloWireLimits {
                total_bytes: self.base.wire.total_bytes.min(self.wire.field_bytes),
                field_bytes: self.base.wire.field_bytes.min(self.wire.field_bytes),
            },
            ..self.base
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloFundingGrantUseV2<'a> {
    pub source_index: u32,
    pub grant_id: &'a [u8],
    pub authority_version: u64,
    pub operation_id: [u8; 32],
    pub max_draw: u128,
    pub cumulative_ceiling: u128,
    pub valid_from: Option<u64>,
    pub valid_until: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhloConversionCompositionV2<'a> {
    NoConversion,
    SeparatePrior {
        accepted_trade: [u8; 32],
        output_source_index: u32,
        output_asset: &'a [u8],
    },
    AtomicQuote {
        quote_commitment: [u8; 32],
        quote_evidence: &'a [u8],
        output_source_index: u32,
        input_custody: &'a [u8],
        input_asset: &'a [u8],
        provider_custody: &'a [u8],
        output_asset: &'a [u8],
        max_input_debit: u128,
        max_output_debit: u128,
        input_hold_cap: u128,
        provider_hold_cap: u128,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloFundingIntentV2<'a> {
    pub base: PhloFundingIntentV1<'a>,
    pub grant_uses: Vec<PhloFundingGrantUseV2<'a>>,
    pub conversion: PhloConversionCompositionV2<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhloFundingIntentVersioned<'a> {
    V1(PhloFundingIntentV1<'a>),
    V2(PhloFundingIntentV2<'a>),
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PhloFundingIntentV2Error {
    #[error("unsupported phlo funding-intent format domain")]
    FormatDomain,
    #[error("phlo funding intent exceeds its grant-use limit")]
    GrantUseLimit,
    #[error("phlo funding grant use has an invalid source index")]
    SourceIndex,
    #[error("phlo funding grant use has an empty or oversized grant identity")]
    GrantId,
    #[error("phlo funding grant uses are not in strict canonical order")]
    GrantOrder,
    #[error("phlo funding grant use has invalid draw, ceiling, or validity bounds")]
    GrantBounds,
    #[error("phlo conversion has an unknown composition tag")]
    ConversionTag,
    #[error("phlo conversion requires a nonempty identity, asset, or quote")]
    ConversionIdentity,
    #[error("phlo conversion has invalid debit or hold caps")]
    ConversionBounds,
    #[error("phlo conversion quote commitment does not match signed quote evidence")]
    QuoteCommitment,
    #[error("phlo conversion quote terms do not match signed funding terms")]
    QuoteMismatch,
    #[error("phlo funding-intent field has a noncanonical width")]
    FieldWidth,
    #[error(transparent)]
    Base(#[from] PhloFundingIntentError),
    #[error(transparent)]
    Wire(#[from] PhloWireError),
    #[error(transparent)]
    QuoteEvidence(#[from] PhloQuoteV2Error),
}

impl<'a> PhloFundingIntentV2<'a> {
    pub fn encode(
        &self,
        limits: PhloFundingIntentV2Limits,
    ) -> Result<Vec<u8>, PhloFundingIntentV2Error> {
        check_grants(&self.grant_uses, self.base.sources.len(), limits)?;
        check_conversion(&self.conversion, &self.base, limits)?;
        let base = self.base.encode(limits.base())?;
        let mut grants = PhloWireEncoder::new(limits.nested());
        let count = u32::try_from(self.grant_uses.len())
            .map_err(|_| PhloFundingIntentV2Error::GrantUseLimit)?;
        grants.bytes(&count.to_be_bytes())?;
        for grant in &self.grant_uses {
            grants.bytes(&encode_grant(grant, limits.nested())?)?;
        }
        let conversion = encode_conversion(&self.conversion, limits.nested())?;
        let mut output = PhloWireEncoder::new(limits.wire);
        for field in [
            PHLO_FUNDING_INTENT_V2_DOMAIN,
            &base,
            grants.as_bytes(),
            &conversion,
        ] {
            output.bytes(field)?;
        }
        Ok(output.into_bytes())
    }

    pub fn decode(
        input: &'a [u8],
        limits: PhloFundingIntentV2Limits,
    ) -> Result<Self, PhloFundingIntentV2Error> {
        let mut fields = PhloWireDecoder::new(input, limits.wire)?;
        if fields.bytes()? != PHLO_FUNDING_INTENT_V2_DOMAIN {
            return Err(PhloFundingIntentV2Error::FormatDomain);
        }
        let base = PhloFundingIntentV1::decode(fields.bytes()?, limits.base())?;
        let mut grant_fields = PhloWireDecoder::new(fields.bytes()?, limits.nested())?;
        let conversion_bytes = fields.bytes()?;
        fields.finish()?;
        let count = usize::try_from(u32::from_be_bytes(fixed_field(&mut grant_fields)?))
            .map_err(|_| PhloFundingIntentV2Error::GrantUseLimit)?;
        if count > limits.grant_uses {
            return Err(PhloFundingIntentV2Error::GrantUseLimit);
        }
        let mut grant_uses = Vec::new();
        for _ in 0..count {
            grant_uses
                .try_reserve(1)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            grant_uses.push(decode_grant(grant_fields.bytes()?, limits.nested())?);
        }
        grant_fields.finish()?;
        let conversion = decode_conversion(conversion_bytes, limits.nested())?;
        let intent = Self {
            base,
            grant_uses,
            conversion,
        };
        check_grants(&intent.grant_uses, intent.base.sources.len(), limits)?;
        check_conversion(&intent.conversion, &intent.base, limits)?;
        Ok(intent)
    }
}

impl<'a> PhloFundingIntentVersioned<'a> {
    pub fn decode(
        input: &'a [u8],
        limits: PhloFundingIntentV2Limits,
    ) -> Result<Self, PhloFundingIntentV2Error> {
        let mut fields = PhloWireDecoder::new(input, limits.wire)?;
        match fields.bytes()? {
            super::PHLO_FUNDING_INTENT_V1_DOMAIN => {
                Ok(Self::V1(PhloFundingIntentV1::decode(input, limits.base())?))
            }
            PHLO_FUNDING_INTENT_V2_DOMAIN => {
                Ok(Self::V2(PhloFundingIntentV2::decode(input, limits)?))
            }
            _ => Err(PhloFundingIntentV2Error::FormatDomain),
        }
    }

    pub fn encode(
        &self,
        limits: PhloFundingIntentV2Limits,
    ) -> Result<Vec<u8>, PhloFundingIntentV2Error> {
        match self {
            Self::V1(intent) => Ok(intent.encode(limits.base())?),
            Self::V2(intent) => intent.encode(limits),
        }
    }
}

fn check_grants(
    grants: &[PhloFundingGrantUseV2<'_>],
    source_count: usize,
    limits: PhloFundingIntentV2Limits,
) -> Result<(), PhloFundingIntentV2Error> {
    if grants.len() > limits.grant_uses || u32::try_from(grants.len()).is_err() {
        return Err(PhloFundingIntentV2Error::GrantUseLimit);
    }
    let mut previous = None;
    for grant in grants {
        if usize::try_from(grant.source_index).map_or(true, |index| index >= source_count) {
            return Err(PhloFundingIntentV2Error::SourceIndex);
        }
        if grant.grant_id.is_empty() || grant.grant_id.len() > limits.grant_id_bytes {
            return Err(PhloFundingIntentV2Error::GrantId);
        }
        if grant.max_draw > grant.cumulative_ceiling
            || grant
                .valid_from
                .zip(grant.valid_until)
                .is_some_and(|(from, until)| from > until)
        {
            return Err(PhloFundingIntentV2Error::GrantBounds);
        }
        let key = (grant.source_index, grant.grant_id, grant.operation_id);
        if previous.is_some_and(|prior| prior >= key) {
            return Err(PhloFundingIntentV2Error::GrantOrder);
        }
        previous = Some(key);
    }
    Ok(())
}

fn check_conversion(
    conversion: &PhloConversionCompositionV2<'_>,
    base: &PhloFundingIntentV1<'_>,
    limits: PhloFundingIntentV2Limits,
) -> Result<(), PhloFundingIntentV2Error> {
    match conversion {
        PhloConversionCompositionV2::NoConversion => Ok(()),
        PhloConversionCompositionV2::SeparatePrior {
            output_source_index,
            output_asset,
            ..
        } => {
            if usize::try_from(*output_source_index)
                .map_or(true, |index| index >= base.sources.len())
            {
                return Err(PhloFundingIntentV2Error::SourceIndex);
            }
            if output_asset.is_empty() {
                return Err(PhloFundingIntentV2Error::ConversionIdentity);
            }
            Ok(())
        }
        PhloConversionCompositionV2::AtomicQuote {
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
            ..
        } => {
            if usize::try_from(*output_source_index)
                .map_or(true, |index| index >= base.sources.len())
            {
                return Err(PhloFundingIntentV2Error::SourceIndex);
            }
            if quote_evidence.is_empty()
                || quote_evidence.len() > limits.quote_evidence_bytes
                || input_custody.is_empty()
                || input_asset.is_empty()
                || provider_custody.is_empty()
                || output_asset.is_empty()
            {
                return Err(PhloFundingIntentV2Error::ConversionIdentity);
            }
            if max_input_debit > input_hold_cap || max_output_debit > provider_hold_cap {
                return Err(PhloFundingIntentV2Error::ConversionBounds);
            }
            if *quote_commitment != quote_evidence_commitment(quote_evidence) {
                return Err(PhloFundingIntentV2Error::QuoteCommitment);
            }
            let quote = PhloQuoteEvidenceV2::decode(quote_evidence, PhloWireLimits {
                total_bytes: limits.quote_evidence_bytes,
                field_bytes: limits.quote_evidence_bytes,
            })?;
            let recipient = &base.sources[*output_source_index as usize];
            if quote.input_custody != *input_custody
                || quote.input_asset != *input_asset
                || quote.provider_custody != *provider_custody
                || quote.output_asset != *output_asset
                || quote.output_recipient != recipient.custody()
                || quote.schedule_commitment != base.schedule_commitment
                || quote.max_input_debit != *max_input_debit
                || quote.max_output != *max_output_debit
                || quote.input_hold_cap != *input_hold_cap
                || quote.provider_hold_cap != *provider_hold_cap
            {
                return Err(PhloFundingIntentV2Error::QuoteMismatch);
            }
            Ok(())
        }
    }
}

fn encode_grant(
    grant: &PhloFundingGrantUseV2<'_>,
    limits: PhloWireLimits,
) -> Result<Vec<u8>, PhloWireError> {
    let mut output = PhloWireEncoder::new(limits);
    for field in [
        grant.source_index.to_be_bytes().as_slice(),
        grant.grant_id,
        grant.authority_version.to_be_bytes().as_slice(),
        &grant.operation_id,
        grant.max_draw.to_be_bytes().as_slice(),
        grant.cumulative_ceiling.to_be_bytes().as_slice(),
        &encode_endpoint(grant.valid_from),
        &encode_endpoint(grant.valid_until),
    ] {
        output.bytes(field)?;
    }
    Ok(output.into_bytes())
}

fn decode_grant<'a>(
    input: &'a [u8],
    limits: PhloWireLimits,
) -> Result<PhloFundingGrantUseV2<'a>, PhloFundingIntentV2Error> {
    let mut fields = PhloWireDecoder::new(input, limits)?;
    let source_index = u32::from_be_bytes(fixed_field(&mut fields)?);
    let grant_id = fields.bytes()?;
    let authority_version = u64::from_be_bytes(fixed_field(&mut fields)?);
    let operation_id = fixed_field(&mut fields)?;
    let max_draw = u128::from_be_bytes(fixed_field(&mut fields)?);
    let cumulative_ceiling = u128::from_be_bytes(fixed_field(&mut fields)?);
    let valid_from = decode_endpoint(fields.bytes()?)?;
    let valid_until = decode_endpoint(fields.bytes()?)?;
    fields.finish()?;
    Ok(PhloFundingGrantUseV2 {
        source_index,
        grant_id,
        authority_version,
        operation_id,
        max_draw,
        cumulative_ceiling,
        valid_from,
        valid_until,
    })
}

fn encode_conversion(
    conversion: &PhloConversionCompositionV2<'_>,
    limits: PhloWireLimits,
) -> Result<Vec<u8>, PhloWireError> {
    let mut output = PhloWireEncoder::new(limits);
    match conversion {
        PhloConversionCompositionV2::NoConversion => output.bytes(&[0])?,
        PhloConversionCompositionV2::SeparatePrior {
            accepted_trade,
            output_source_index,
            output_asset,
        } => {
            for field in [
                [1u8].as_slice(),
                accepted_trade,
                output_source_index.to_be_bytes().as_slice(),
                output_asset,
            ] {
                output.bytes(field)?;
            }
        }
        PhloConversionCompositionV2::AtomicQuote {
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
        } => {
            for field in [
                [2u8].as_slice(),
                quote_commitment,
                quote_evidence,
                output_source_index.to_be_bytes().as_slice(),
                input_custody,
                input_asset,
                provider_custody,
                output_asset,
                max_input_debit.to_be_bytes().as_slice(),
                max_output_debit.to_be_bytes().as_slice(),
                input_hold_cap.to_be_bytes().as_slice(),
                provider_hold_cap.to_be_bytes().as_slice(),
            ] {
                output.bytes(field)?;
            }
        }
    }
    Ok(output.into_bytes())
}

fn decode_conversion<'a>(
    input: &'a [u8],
    limits: PhloWireLimits,
) -> Result<PhloConversionCompositionV2<'a>, PhloFundingIntentV2Error> {
    let mut fields = PhloWireDecoder::new(input, limits)?;
    let composition = match fields.bytes()? {
        [0] => PhloConversionCompositionV2::NoConversion,
        [1] => PhloConversionCompositionV2::SeparatePrior {
            accepted_trade: fixed_field(&mut fields)?,
            output_source_index: u32::from_be_bytes(fixed_field(&mut fields)?),
            output_asset: fields.bytes()?,
        },
        [2] => PhloConversionCompositionV2::AtomicQuote {
            quote_commitment: fixed_field(&mut fields)?,
            quote_evidence: fields.bytes()?,
            output_source_index: u32::from_be_bytes(fixed_field(&mut fields)?),
            input_custody: fields.bytes()?,
            input_asset: fields.bytes()?,
            provider_custody: fields.bytes()?,
            output_asset: fields.bytes()?,
            max_input_debit: u128::from_be_bytes(fixed_field(&mut fields)?),
            max_output_debit: u128::from_be_bytes(fixed_field(&mut fields)?),
            input_hold_cap: u128::from_be_bytes(fixed_field(&mut fields)?),
            provider_hold_cap: u128::from_be_bytes(fixed_field(&mut fields)?),
        },
        _ => return Err(PhloFundingIntentV2Error::ConversionTag),
    };
    fields.finish()?;
    Ok(composition)
}

fn encode_endpoint(endpoint: Option<u64>) -> Vec<u8> {
    match endpoint {
        None => vec![0],
        Some(value) => {
            let mut bytes = Vec::with_capacity(9);
            bytes.push(1);
            bytes.extend_from_slice(&value.to_be_bytes());
            bytes
        }
    }
}

fn decode_endpoint(bytes: &[u8]) -> Result<Option<u64>, PhloFundingIntentV2Error> {
    match bytes {
        [0] => Ok(None),
        [1, rest @ ..] if rest.len() == 8 => Ok(Some(u64::from_be_bytes(
            rest.try_into()
                .map_err(|_| PhloFundingIntentV2Error::FieldWidth)?,
        ))),
        _ => Err(PhloFundingIntentV2Error::FieldWidth),
    }
}

fn fixed_field<const N: usize>(
    fields: &mut PhloWireDecoder<'_>,
) -> Result<[u8; N], PhloFundingIntentV2Error> {
    fields
        .bytes()?
        .try_into()
        .map_err(|_| PhloFundingIntentV2Error::FieldWidth)
}

#[cfg(test)]
mod tests;
