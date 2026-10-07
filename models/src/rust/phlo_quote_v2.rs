use thiserror::Error;

use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const PHLO_QUOTE_V2_DOMAIN: &[u8] = b"f1r3node:phlo-atomic-quote:v2";
pub const PHLO_RATIONAL_UPWARD_FIXED_FEE_V2: u8 = 1;
pub const PHLO_ATOMIC_ORIGINAL_INPUT_RELEASE_V2: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhloQuoteUseV2 {
    Once,
    Cumulative { output_ceiling: u128 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloQuoteEvidenceV2<'a> {
    pub quote_id: [u8; 32],
    pub network: &'a [u8],
    pub shard: &'a [u8],
    pub context_commitment: [u8; 32],
    pub input_custody: &'a [u8],
    pub input_asset: &'a [u8],
    pub input_authority: &'a [u8],
    pub provider_custody: &'a [u8],
    pub provider_authority: &'a [u8],
    pub output_asset: &'a [u8],
    pub output_recipient: &'a [u8],
    pub fee_recipient: &'a [u8],
    pub schedule_commitment: [u8; 32],
    pub valid_from: Option<u64>,
    pub valid_until: Option<u64>,
    pub use_policy: PhloQuoteUseV2,
    pub max_output: u128,
    pub max_input_debit: u128,
    pub input_hold_cap: u128,
    pub provider_hold_cap: u128,
    pub rate_numerator: u128,
    pub rate_denominator: u128,
    pub fixed_input_fee: u128,
    pub input_scale: u128,
    pub output_scale: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloQuoteAmountV2 {
    pub base_input: u128,
    pub conversion_fee: u128,
    pub total_input: u128,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloQuoteV2Error {
    #[error("unsupported atomic quote format")]
    Format,
    #[error("atomic quote has a noncanonical field width")]
    FieldWidth,
    #[error("atomic quote has an empty identity or authority")]
    EmptyIdentity,
    #[error("atomic quote has invalid validity or capacity bounds")]
    Bounds,
    #[error("atomic quote has an unsupported use rule")]
    UseRule,
    #[error("atomic quote has an unsupported pricing rule")]
    PricingRule,
    #[error("atomic quote has an unsupported failure rule")]
    FailureRule,
    #[error("atomic quote output is outside its signed domain")]
    OutputDomain,
    #[error("atomic quote arithmetic exceeds u128")]
    ArithmeticOverflow,
    #[error("atomic quote input exceeds its signed debit cap")]
    InputCap,
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

impl<'a> PhloQuoteEvidenceV2<'a> {
    pub fn encode(&self, limits: PhloWireLimits) -> Result<Vec<u8>, PhloQuoteV2Error> {
        self.validate()?;
        let mut output = PhloWireEncoder::new(limits);
        let use_rule = match self.use_policy {
            PhloQuoteUseV2::Once => [0u8; 17],
            PhloQuoteUseV2::Cumulative { output_ceiling } => {
                let mut bytes = [0u8; 17];
                bytes[0] = 1;
                bytes[1..].copy_from_slice(&output_ceiling.to_be_bytes());
                bytes
            }
        };
        for field in [
            PHLO_QUOTE_V2_DOMAIN,
            &self.quote_id,
            self.network,
            self.shard,
            &self.context_commitment,
            self.input_custody,
            self.input_asset,
            self.input_authority,
            self.provider_custody,
            self.provider_authority,
            self.output_asset,
            self.output_recipient,
            self.fee_recipient,
            &self.schedule_commitment,
            &encode_endpoint(self.valid_from),
            &encode_endpoint(self.valid_until),
            &use_rule,
            &self.max_output.to_be_bytes(),
            &self.max_input_debit.to_be_bytes(),
            &self.input_hold_cap.to_be_bytes(),
            &self.provider_hold_cap.to_be_bytes(),
            &[PHLO_RATIONAL_UPWARD_FIXED_FEE_V2],
            &self.rate_numerator.to_be_bytes(),
            &self.rate_denominator.to_be_bytes(),
            &self.fixed_input_fee.to_be_bytes(),
            &self.input_scale.to_be_bytes(),
            &self.output_scale.to_be_bytes(),
            &[PHLO_ATOMIC_ORIGINAL_INPUT_RELEASE_V2],
        ] {
            output.bytes(field)?;
        }
        Ok(output.into_bytes())
    }

    pub fn decode(input: &'a [u8], limits: PhloWireLimits) -> Result<Self, PhloQuoteV2Error> {
        let mut fields = PhloWireDecoder::new(input, limits)?;
        if fields.bytes()? != PHLO_QUOTE_V2_DOMAIN {
            return Err(PhloQuoteV2Error::Format);
        }
        let quote_id = fixed_field(&mut fields)?;
        let network = fields.bytes()?;
        let shard = fields.bytes()?;
        let context_commitment = fixed_field(&mut fields)?;
        let input_custody = fields.bytes()?;
        let input_asset = fields.bytes()?;
        let input_authority = fields.bytes()?;
        let provider_custody = fields.bytes()?;
        let provider_authority = fields.bytes()?;
        let output_asset = fields.bytes()?;
        let output_recipient = fields.bytes()?;
        let fee_recipient = fields.bytes()?;
        let schedule_commitment = fixed_field(&mut fields)?;
        let valid_from = decode_endpoint(fields.bytes()?)?;
        let valid_until = decode_endpoint(fields.bytes()?)?;
        let use_policy = match fields.bytes()? {
            [0, rest @ ..] if rest == [0u8; 16] => PhloQuoteUseV2::Once,
            [1, rest @ ..] if rest.len() == 16 => PhloQuoteUseV2::Cumulative {
                output_ceiling: u128::from_be_bytes(
                    rest.try_into().map_err(|_| PhloQuoteV2Error::FieldWidth)?,
                ),
            },
            _ => return Err(PhloQuoteV2Error::UseRule),
        };
        let max_output = u128::from_be_bytes(fixed_field(&mut fields)?);
        let max_input_debit = u128::from_be_bytes(fixed_field(&mut fields)?);
        let input_hold_cap = u128::from_be_bytes(fixed_field(&mut fields)?);
        let provider_hold_cap = u128::from_be_bytes(fixed_field(&mut fields)?);
        if fields.bytes()? != [PHLO_RATIONAL_UPWARD_FIXED_FEE_V2] {
            return Err(PhloQuoteV2Error::PricingRule);
        }
        let rate_numerator = u128::from_be_bytes(fixed_field(&mut fields)?);
        let rate_denominator = u128::from_be_bytes(fixed_field(&mut fields)?);
        let fixed_input_fee = u128::from_be_bytes(fixed_field(&mut fields)?);
        let input_scale = u128::from_be_bytes(fixed_field(&mut fields)?);
        let output_scale = u128::from_be_bytes(fixed_field(&mut fields)?);
        if fields.bytes()? != [PHLO_ATOMIC_ORIGINAL_INPUT_RELEASE_V2] {
            return Err(PhloQuoteV2Error::FailureRule);
        }
        fields.finish()?;
        let quote = Self {
            quote_id,
            network,
            shard,
            context_commitment,
            input_custody,
            input_asset,
            input_authority,
            provider_custody,
            provider_authority,
            output_asset,
            output_recipient,
            fee_recipient,
            schedule_commitment,
            valid_from,
            valid_until,
            use_policy,
            max_output,
            max_input_debit,
            input_hold_cap,
            provider_hold_cap,
            rate_numerator,
            rate_denominator,
            fixed_input_fee,
            input_scale,
            output_scale,
        };
        quote.validate()?;
        Ok(quote)
    }

    pub fn evaluate_exact_output(
        &self,
        output: u128,
    ) -> Result<PhloQuoteAmountV2, PhloQuoteV2Error> {
        self.validate()?;
        if output > self.max_output {
            return Err(PhloQuoteV2Error::OutputDomain);
        }
        if output == 0 {
            return Ok(PhloQuoteAmountV2 {
                base_input: 0,
                conversion_fee: 0,
                total_input: 0,
            });
        }
        let product = self
            .rate_numerator
            .checked_mul(output)
            .ok_or(PhloQuoteV2Error::ArithmeticOverflow)?;
        let base_input =
            product / self.rate_denominator + u128::from(product % self.rate_denominator != 0);
        let total_input = base_input
            .checked_add(self.fixed_input_fee)
            .ok_or(PhloQuoteV2Error::ArithmeticOverflow)?;
        if total_input > self.max_input_debit {
            return Err(PhloQuoteV2Error::InputCap);
        }
        Ok(PhloQuoteAmountV2 {
            base_input,
            conversion_fee: self.fixed_input_fee,
            total_input,
        })
    }

    fn validate(&self) -> Result<(), PhloQuoteV2Error> {
        if [
            self.network,
            self.shard,
            self.input_custody,
            self.input_asset,
            self.input_authority,
            self.provider_custody,
            self.provider_authority,
            self.output_asset,
            self.output_recipient,
            self.fee_recipient,
        ]
        .iter()
        .any(|identity| identity.is_empty())
        {
            return Err(PhloQuoteV2Error::EmptyIdentity);
        }
        if self
            .valid_from
            .zip(self.valid_until)
            .is_some_and(|(from, until)| from > until)
            || self.max_input_debit > self.input_hold_cap
            || self.max_output > self.provider_hold_cap
            || matches!(self.use_policy, PhloQuoteUseV2::Cumulative { output_ceiling } if output_ceiling < self.max_output)
        {
            return Err(PhloQuoteV2Error::Bounds);
        }
        if self.rate_numerator == 0
            || self.rate_denominator == 0
            || self.input_scale == 0
            || self.output_scale == 0
        {
            return Err(PhloQuoteV2Error::PricingRule);
        }
        if self.max_output > 0 {
            let product = self
                .rate_numerator
                .checked_mul(self.max_output)
                .ok_or(PhloQuoteV2Error::ArithmeticOverflow)?;
            let base =
                product / self.rate_denominator + u128::from(product % self.rate_denominator != 0);
            let total = base
                .checked_add(self.fixed_input_fee)
                .ok_or(PhloQuoteV2Error::ArithmeticOverflow)?;
            if total > self.max_input_debit {
                return Err(PhloQuoteV2Error::InputCap);
            }
        }
        Ok(())
    }
}

fn encode_endpoint(value: Option<u64>) -> Vec<u8> {
    match value {
        None => vec![0],
        Some(value) => [vec![1], value.to_be_bytes().to_vec()].concat(),
    }
}

fn decode_endpoint(bytes: &[u8]) -> Result<Option<u64>, PhloQuoteV2Error> {
    match bytes {
        [0] => Ok(None),
        [1, rest @ ..] if rest.len() == 8 => Ok(Some(u64::from_be_bytes(
            rest.try_into().map_err(|_| PhloQuoteV2Error::FieldWidth)?,
        ))),
        _ => Err(PhloQuoteV2Error::FieldWidth),
    }
}

fn fixed_field<const N: usize>(
    fields: &mut PhloWireDecoder<'_>,
) -> Result<[u8; N], PhloQuoteV2Error> {
    fields
        .bytes()?
        .try_into()
        .map_err(|_| PhloQuoteV2Error::FieldWidth)
}

#[cfg(test)]
mod tests;
