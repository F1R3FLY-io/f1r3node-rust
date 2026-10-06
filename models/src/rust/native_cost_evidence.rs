use thiserror::Error;

use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

mod sections;
pub use sections::{
    NativeFundingCaseLimits, NativeFundingCaseSource, NativeFundingCaseV1, NativeFundingObligation,
    NativePrepaidBirth, NativePrepaidDeltaLimits, NativePrepaidDeltaV1, NativePrepaidDraw,
    NativePrepaidReplacement, NativeSectionError,
};

pub const NATIVE_COST_EVIDENCE_V1_DOMAIN: &[u8] = b"f1r3node:native-cost-evidence:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeCostFailureClass {
    Success,
    UserFailure,
}

impl NativeCostFailureClass {
    fn from_byte(value: u8) -> Result<Self, NativeCostEvidenceError> {
        match value {
            0 => Ok(Self::Success),
            1 => Ok(Self::UserFailure),
            _ => Err(NativeCostEvidenceError::FailureClass),
        }
    }

    fn as_byte(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::UserFailure => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCostEvidenceV1<'a> {
    pub envelope_commitment: [u8; 32],
    pub genesis_policy_commitment: [u8; 32],
    pub schedule_commitment: [u8; 32],
    pub original_funding_root: [u8; 32],
    pub settlement_runtime_root: [u8; 32],
    pub post_state_root: [u8; 32],
    pub phlo_used: u64,
    pub fresh_phlo: u64,
    pub retained_phlo: u64,
    pub phlo_limit: u64,
    pub phlo_price: u64,
    pub fee_rev: u128,
    pub failure_class: NativeCostFailureClass,
    pub wallet_settlement_log_events: u64,
    pub budget_recording: &'a [u8],
    pub operation_journal: &'a [u8],
    pub funding_case: &'a [u8],
    pub prepaid_delta: &'a [u8],
    pub wallet_settlement: &'a [u8],
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum NativeCostEvidenceError {
    #[error("unsupported native cost evidence format domain")]
    FormatDomain,
    #[error("native cost evidence has a noncanonical field width")]
    FieldWidth,
    #[error("native cost evidence has an invalid failure class")]
    FailureClass,
    #[error("native cost evidence exceeds the signed phlo limit")]
    PhloLimit,
    #[error("native cost evidence has an invalid fresh phlo breakdown")]
    PhloBreakdown,
    #[error("native cost evidence REV amount overflows")]
    RevOverflow,
    #[error(transparent)]
    Section(#[from] NativeSectionError),
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

impl NativeCostEvidenceV1<'_> {
    pub fn encoded_len(&self, limits: PhloWireLimits) -> Result<usize, NativeCostEvidenceError> {
        let fixed = 20usize
            .checked_mul(8)
            .and_then(|bytes| bytes.checked_add(NATIVE_COST_EVIDENCE_V1_DOMAIN.len()))
            .and_then(|bytes| bytes.checked_add(6 * 32 + 6 * 8 + 16 + 1))
            .ok_or(PhloWireError::LimitExceeded)?;
        let size = [
            self.budget_recording,
            self.operation_journal,
            self.funding_case,
            self.prepaid_delta,
            self.wallet_settlement,
        ]
        .iter()
        .try_fold(fixed, |size, field| {
            if field.len() > limits.field_bytes {
                return Err(PhloWireError::LimitExceeded);
            }
            size.checked_add(field.len())
                .ok_or(PhloWireError::LimitExceeded)
        })?;
        if size > limits.total_bytes {
            return Err(PhloWireError::LimitExceeded.into());
        }
        Ok(size)
    }

    pub fn rev_ceiling(&self) -> Result<u128, NativeCostEvidenceError> {
        u128::from(self.phlo_limit)
            .checked_mul(u128::from(self.phlo_price))
            .and_then(|amount| amount.checked_add(self.fee_rev))
            .ok_or(NativeCostEvidenceError::RevOverflow)
    }

    pub fn resource_rev(&self) -> Result<u128, NativeCostEvidenceError> {
        if self.fresh_phlo > self.phlo_used {
            return Err(NativeCostEvidenceError::PhloBreakdown);
        }
        if self
            .phlo_used
            .checked_add(self.retained_phlo)
            .filter(|used| *used <= self.phlo_limit)
            .is_none()
        {
            return Err(NativeCostEvidenceError::PhloLimit);
        }
        u128::from(self.fresh_phlo)
            .checked_add(u128::from(self.retained_phlo))
            .and_then(|used| used.checked_mul(u128::from(self.phlo_price)))
            .ok_or(NativeCostEvidenceError::RevOverflow)
    }

    pub fn rev_spent(&self) -> Result<u128, NativeCostEvidenceError> {
        self.resource_rev()?
            .checked_add(self.fee_rev)
            .ok_or(NativeCostEvidenceError::RevOverflow)
    }

    pub fn encode(&self, limits: PhloWireLimits) -> Result<Vec<u8>, NativeCostEvidenceError> {
        self.rev_ceiling()?;
        self.rev_spent()?;
        let mut wire = PhloWireEncoder::with_capacity(limits, self.encoded_len(limits)?)?;
        for field in [
            NATIVE_COST_EVIDENCE_V1_DOMAIN,
            &self.envelope_commitment,
            &self.genesis_policy_commitment,
            &self.schedule_commitment,
            &self.original_funding_root,
            &self.settlement_runtime_root,
            &self.post_state_root,
            &self.phlo_used.to_be_bytes(),
            &self.fresh_phlo.to_be_bytes(),
            &self.retained_phlo.to_be_bytes(),
            &self.phlo_limit.to_be_bytes(),
            &self.phlo_price.to_be_bytes(),
            &self.fee_rev.to_be_bytes(),
            &[self.failure_class.as_byte()],
            &self.wallet_settlement_log_events.to_be_bytes(),
            self.budget_recording,
            self.operation_journal,
            self.funding_case,
            self.prepaid_delta,
            self.wallet_settlement,
        ] {
            wire.bytes(field)?;
        }
        Ok(wire.into_bytes())
    }
}

impl<'a> NativeCostEvidenceV1<'a> {
    pub fn decoded_sections(
        &self,
        funding_limits: NativeFundingCaseLimits,
        prepaid_limits: NativePrepaidDeltaLimits,
    ) -> Result<(NativeFundingCaseV1<'a>, NativePrepaidDeltaV1), NativeCostEvidenceError> {
        Ok((
            NativeFundingCaseV1::decode(self.funding_case, funding_limits)?,
            NativePrepaidDeltaV1::decode(self.prepaid_delta, prepaid_limits)?,
        ))
    }

    pub fn decode(
        input: &'a [u8],
        limits: PhloWireLimits,
    ) -> Result<Self, NativeCostEvidenceError> {
        let mut wire = PhloWireDecoder::new(input, limits)?;
        if wire.bytes()? != NATIVE_COST_EVIDENCE_V1_DOMAIN {
            return Err(NativeCostEvidenceError::FormatDomain);
        }
        let record = Self {
            envelope_commitment: fixed_field(&mut wire)?,
            genesis_policy_commitment: fixed_field(&mut wire)?,
            schedule_commitment: fixed_field(&mut wire)?,
            original_funding_root: fixed_field(&mut wire)?,
            settlement_runtime_root: fixed_field(&mut wire)?,
            post_state_root: fixed_field(&mut wire)?,
            phlo_used: u64::from_be_bytes(fixed_field(&mut wire)?),
            fresh_phlo: u64::from_be_bytes(fixed_field(&mut wire)?),
            retained_phlo: u64::from_be_bytes(fixed_field(&mut wire)?),
            phlo_limit: u64::from_be_bytes(fixed_field(&mut wire)?),
            phlo_price: u64::from_be_bytes(fixed_field(&mut wire)?),
            fee_rev: u128::from_be_bytes(fixed_field(&mut wire)?),
            failure_class: NativeCostFailureClass::from_byte(fixed_field::<1>(&mut wire)?[0])?,
            wallet_settlement_log_events: u64::from_be_bytes(fixed_field(&mut wire)?),
            budget_recording: wire.bytes()?,
            operation_journal: wire.bytes()?,
            funding_case: wire.bytes()?,
            prepaid_delta: wire.bytes()?,
            wallet_settlement: wire.bytes()?,
        };
        wire.finish()?;
        record.rev_ceiling()?;
        record.rev_spent()?;
        Ok(record)
    }
}

fn fixed_field<const N: usize>(
    wire: &mut PhloWireDecoder<'_>,
) -> Result<[u8; N], NativeCostEvidenceError> {
    wire.bytes()?
        .try_into()
        .map_err(|_| NativeCostEvidenceError::FieldWidth)
}

#[cfg(test)]
mod tests;
