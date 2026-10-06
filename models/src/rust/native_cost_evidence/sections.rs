use std::collections::BTreeSet;

use thiserror::Error;

use super::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

const FUNDING_CASE_DOMAIN: &[u8] = b"f1r3node:native-funding-case:v1";
const PREPAID_DELTA_DOMAIN: &[u8] = b"f1r3node:native-prepaid-delta:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeFundingCaseLimits {
    pub wire: PhloWireLimits,
    pub sources: usize,
    pub obligations: usize,
    pub cells: usize,
    pub custody_bytes: usize,
    pub key_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFundingCaseSource<'a> {
    pub custody: &'a [u8],
    pub capacity: u64,
    pub exposure_limit: u64,
    pub debit_limit: u64,
    pub hold: u64,
    pub debit: u64,
    pub fee: u64,
    pub refund: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFundingObligation<'a> {
    pub key: &'a [u8],
    pub quantity: u64,
    pub amount: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFundingCaseV1<'a> {
    pub sources: Vec<NativeFundingCaseSource<'a>>,
    pub obligations: Vec<NativeFundingObligation<'a>>,
    pub eligible: Vec<Vec<bool>>,
    pub assignment: Vec<Vec<u64>>,
    pub resource_next_cursor: Option<u32>,
    pub fee_next_cursor: Option<u32>,
    pub resource_unrestricted: bool,
    pub resource_restriction_witness: Option<Vec<u64>>,
    pub possible_fee_payers: Vec<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePrepaidDeltaLimits {
    pub wire: PhloWireLimits,
    pub draws: usize,
    pub positions: usize,
    pub births: usize,
    pub replacements: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePrepaidDraw {
    pub stack_id: [u8; 32],
    pub receipt_index: u32,
    pub positions: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePrepaidBirth {
    pub stack_id: [u8; 32],
    pub source_hash: [u8; 32],
    pub encoded_cells_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePrepaidReplacement {
    pub receipt_id: [u8; 32],
    pub expected_hash: Option<[u8; 32]>,
    pub replacement_hash: Option<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePrepaidDeltaV1 {
    pub draws: Vec<NativePrepaidDraw>,
    pub births: Vec<NativePrepaidBirth>,
    pub replacements: Vec<NativePrepaidReplacement>,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum NativeSectionError {
    #[error("unsupported native evidence section format")]
    FormatDomain,
    #[error("native evidence section exceeds its configured count or byte limit")]
    Limit,
    #[error("native evidence section contains a noncanonical field")]
    Noncanonical,
    #[error("native funding case does not conserve its assignments or source debits")]
    Conservation,
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

fn bounded_count(count: usize, cap: usize) -> Result<u32, NativeSectionError> {
    if count > cap {
        return Err(NativeSectionError::Limit);
    }
    u32::try_from(count).map_err(|_| NativeSectionError::Limit)
}

fn decoded_count(wire: &mut PhloWireDecoder<'_>, cap: usize) -> Result<usize, NativeSectionError> {
    let count = usize::try_from(wire.u32()?).map_err(|_| NativeSectionError::Limit)?;
    if count > cap {
        return Err(NativeSectionError::Limit);
    }
    Ok(count)
}

fn flag(wire: &mut PhloWireDecoder<'_>) -> Result<bool, NativeSectionError> {
    match wire.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(NativeSectionError::Noncanonical),
    }
}

fn write_flag(wire: &mut PhloWireEncoder, value: bool) -> Result<(), NativeSectionError> {
    Ok(wire.u8(u8::from(value))?)
}

fn optional_cursor(wire: &mut PhloWireDecoder<'_>) -> Result<Option<u32>, NativeSectionError> {
    let present = flag(wire)?;
    let value = wire.u32()?;
    if !present && value != 0 {
        return Err(NativeSectionError::Noncanonical);
    }
    Ok(present.then_some(value))
}

fn write_cursor(wire: &mut PhloWireEncoder, value: Option<u32>) -> Result<(), NativeSectionError> {
    write_flag(wire, value.is_some())?;
    Ok(wire.u32(value.unwrap_or_default())?)
}

impl NativeFundingCaseV1<'_> {
    fn check(&self, limits: NativeFundingCaseLimits) -> Result<(), NativeSectionError> {
        let source_count = self.sources.len();
        let obligation_count = self.obligations.len();
        if source_count == 0 || obligation_count == 0 {
            return Err(NativeSectionError::Noncanonical);
        }
        bounded_count(source_count, limits.sources)?;
        bounded_count(obligation_count, limits.obligations)?;
        source_count
            .checked_mul(obligation_count)
            .filter(|count| *count <= limits.cells)
            .ok_or(NativeSectionError::Limit)?;
        if self.eligible.len() != source_count
            || self.assignment.len() != source_count
            || self.possible_fee_payers.len() != source_count
            || self
                .resource_restriction_witness
                .as_ref()
                .is_some_and(|rows| rows.len() != source_count)
        {
            return Err(NativeSectionError::Noncanonical);
        }
        let mut custody_bytes = 0usize;
        let mut previous_custody: Option<&[u8]> = None;
        for (index, source) in self.sources.iter().enumerate() {
            custody_bytes = custody_bytes
                .checked_add(source.custody.len())
                .ok_or(NativeSectionError::Limit)?;
            if source.custody.is_empty()
                || previous_custody.is_some_and(|prior| prior >= source.custody)
                || source.hold > source.capacity
                || source.hold > source.exposure_limit
                || source.debit > source.hold
                || source.debit > source.debit_limit
                || source.fee > source.debit
                || source.refund != source.hold - source.debit
                || self.eligible[index].len() != obligation_count
                || self.assignment[index].len() != obligation_count
            {
                return Err(NativeSectionError::Noncanonical);
            }
            let mut debit = 0u64;
            for (eligible, assigned) in self.eligible[index].iter().zip(&self.assignment[index]) {
                if !eligible && *assigned != 0 {
                    return Err(NativeSectionError::Conservation);
                }
                debit = debit
                    .checked_add(*assigned)
                    .ok_or(NativeSectionError::Conservation)?;
            }
            if debit != source.debit {
                return Err(NativeSectionError::Conservation);
            }
            previous_custody = Some(source.custody);
        }
        if custody_bytes > limits.custody_bytes {
            return Err(NativeSectionError::Limit);
        }
        let mut key_bytes = 0usize;
        let mut previous_key: Option<&[u8]> = None;
        for (column, obligation) in self.obligations.iter().enumerate() {
            key_bytes = key_bytes
                .checked_add(obligation.key.len())
                .ok_or(NativeSectionError::Limit)?;
            if obligation.key.is_empty()
                || previous_key.is_some_and(|prior| prior >= obligation.key)
                || obligation.quantity == 0
            {
                return Err(NativeSectionError::Noncanonical);
            }
            let mut funded = 0u64;
            for row in &self.assignment {
                funded = funded
                    .checked_add(row[column])
                    .ok_or(NativeSectionError::Conservation)?;
            }
            if funded != obligation.amount {
                return Err(NativeSectionError::Conservation);
            }
            previous_key = Some(obligation.key);
        }
        if key_bytes > limits.key_bytes {
            return Err(NativeSectionError::Limit);
        }
        for cursor in [self.resource_next_cursor, self.fee_next_cursor]
            .into_iter()
            .flatten()
        {
            if cursor as usize >= source_count {
                return Err(NativeSectionError::Noncanonical);
            }
        }
        Ok(())
    }

    pub fn encode(&self, limits: NativeFundingCaseLimits) -> Result<Vec<u8>, NativeSectionError> {
        self.check(limits)?;
        let mut wire = PhloWireEncoder::new(limits.wire);
        wire.bytes(FUNDING_CASE_DOMAIN)?;
        wire.u32(bounded_count(self.sources.len(), limits.sources)?)?;
        wire.u32(bounded_count(self.obligations.len(), limits.obligations)?)?;
        for source in &self.sources {
            wire.bytes(source.custody)?;
            for value in [
                source.capacity,
                source.exposure_limit,
                source.debit_limit,
                source.hold,
                source.debit,
                source.fee,
                source.refund,
            ] {
                wire.u64(value)?;
            }
        }
        for obligation in &self.obligations {
            wire.bytes(obligation.key)?;
            wire.u64(obligation.quantity)?;
            wire.u64(obligation.amount)?;
        }
        for (eligible, assigned) in self.eligible.iter().zip(&self.assignment) {
            for (allowed, value) in eligible.iter().zip(assigned) {
                write_flag(&mut wire, *allowed)?;
                wire.u64(*value)?;
            }
        }
        write_cursor(&mut wire, self.resource_next_cursor)?;
        write_cursor(&mut wire, self.fee_next_cursor)?;
        write_flag(&mut wire, self.resource_unrestricted)?;
        if let Some(witness) = &self.resource_restriction_witness {
            write_flag(&mut wire, true)?;
            wire.u32(bounded_count(witness.len(), limits.sources)?)?;
            for value in witness {
                wire.u64(*value)?;
            }
        } else {
            write_flag(&mut wire, false)?;
            wire.u32(0)?;
        }
        wire.u32(bounded_count(
            self.possible_fee_payers.len(),
            limits.sources,
        )?)?;
        for payer in &self.possible_fee_payers {
            write_flag(&mut wire, *payer)?;
        }
        Ok(wire.into_bytes())
    }
}

impl<'a> NativeFundingCaseV1<'a> {
    pub fn decode(
        input: &'a [u8],
        limits: NativeFundingCaseLimits,
    ) -> Result<Self, NativeSectionError> {
        let mut wire = PhloWireDecoder::new(input, limits.wire)?;
        if wire.bytes()? != FUNDING_CASE_DOMAIN {
            return Err(NativeSectionError::FormatDomain);
        }
        let source_count = decoded_count(&mut wire, limits.sources)?;
        let obligation_count = decoded_count(&mut wire, limits.obligations)?;
        if source_count == 0 || obligation_count == 0 {
            return Err(NativeSectionError::Noncanonical);
        }
        source_count
            .checked_mul(obligation_count)
            .filter(|count| *count <= limits.cells)
            .ok_or(NativeSectionError::Limit)?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(source_count)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        for _ in 0..source_count {
            sources.push(NativeFundingCaseSource {
                custody: wire.bytes()?,
                capacity: wire.u64()?,
                exposure_limit: wire.u64()?,
                debit_limit: wire.u64()?,
                hold: wire.u64()?,
                debit: wire.u64()?,
                fee: wire.u64()?,
                refund: wire.u64()?,
            });
        }
        let mut obligations = Vec::new();
        obligations
            .try_reserve_exact(obligation_count)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        for _ in 0..obligation_count {
            obligations.push(NativeFundingObligation {
                key: wire.bytes()?,
                quantity: wire.u64()?,
                amount: wire.u64()?,
            });
        }
        let mut eligible = Vec::new();
        let mut assignment = Vec::new();
        eligible
            .try_reserve_exact(source_count)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        assignment
            .try_reserve_exact(source_count)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        for _ in 0..source_count {
            let mut mask = Vec::new();
            let mut amounts = Vec::new();
            mask.try_reserve_exact(obligation_count)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            amounts
                .try_reserve_exact(obligation_count)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            for _ in 0..obligation_count {
                mask.push(flag(&mut wire)?);
                amounts.push(wire.u64()?);
            }
            eligible.push(mask);
            assignment.push(amounts);
        }
        let resource_next_cursor = optional_cursor(&mut wire)?;
        let fee_next_cursor = optional_cursor(&mut wire)?;
        let resource_unrestricted = flag(&mut wire)?;
        let witness_present = flag(&mut wire)?;
        let witness_count = decoded_count(&mut wire, limits.sources)?;
        if !witness_present && witness_count != 0 {
            return Err(NativeSectionError::Noncanonical);
        }
        let resource_restriction_witness = if witness_present {
            let mut values = Vec::new();
            values
                .try_reserve_exact(witness_count)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            for _ in 0..witness_count {
                values.push(wire.u64()?);
            }
            Some(values)
        } else {
            None
        };
        let payer_count = decoded_count(&mut wire, limits.sources)?;
        let mut possible_fee_payers = Vec::new();
        possible_fee_payers
            .try_reserve_exact(payer_count)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        for _ in 0..payer_count {
            possible_fee_payers.push(flag(&mut wire)?);
        }
        wire.finish()?;
        let case = Self {
            sources,
            obligations,
            eligible,
            assignment,
            resource_next_cursor,
            fee_next_cursor,
            resource_unrestricted,
            resource_restriction_witness,
            possible_fee_payers,
        };
        case.check(limits)?;
        Ok(case)
    }
}

impl NativePrepaidDeltaV1 {
    fn check(&self, limits: NativePrepaidDeltaLimits) -> Result<(), NativeSectionError> {
        bounded_count(self.draws.len(), limits.draws)?;
        bounded_count(self.births.len(), limits.births)?;
        bounded_count(self.replacements.len(), limits.replacements)?;
        let mut previous_stack = None;
        let mut used_positions = BTreeSet::new();
        for draw in &self.draws {
            if previous_stack.is_some_and(|prior| prior >= draw.stack_id)
                || draw.positions.is_empty()
            {
                return Err(NativeSectionError::Noncanonical);
            }
            let mut previous_position = None;
            for position in &draw.positions {
                if previous_position.is_some_and(|prior| prior >= *position)
                    || !used_positions.insert(*position)
                {
                    return Err(NativeSectionError::Noncanonical);
                }
                previous_position = Some(*position);
            }
            previous_stack = Some(draw.stack_id);
        }
        if used_positions.len() > limits.positions {
            return Err(NativeSectionError::Limit);
        }
        let mut previous_birth = None;
        for birth in &self.births {
            if previous_birth.is_some_and(|prior| prior >= birth.stack_id) {
                return Err(NativeSectionError::Noncanonical);
            }
            previous_birth = Some(birth.stack_id);
        }
        let mut previous_replacement = None;
        for replacement in &self.replacements {
            if previous_replacement.is_some_and(|prior| prior >= replacement.receipt_id)
                || (replacement.expected_hash.is_none() && replacement.replacement_hash.is_none())
                || replacement.expected_hash == replacement.replacement_hash
            {
                return Err(NativeSectionError::Noncanonical);
            }
            previous_replacement = Some(replacement.receipt_id);
        }
        Ok(())
    }

    pub fn encode(&self, limits: NativePrepaidDeltaLimits) -> Result<Vec<u8>, NativeSectionError> {
        self.check(limits)?;
        let mut wire = PhloWireEncoder::new(limits.wire);
        wire.bytes(PREPAID_DELTA_DOMAIN)?;
        wire.u32(bounded_count(self.draws.len(), limits.draws)?)?;
        for draw in &self.draws {
            wire.bytes(&draw.stack_id)?;
            wire.u32(draw.receipt_index)?;
            wire.u32(bounded_count(draw.positions.len(), limits.positions)?)?;
            for position in &draw.positions {
                wire.u32(*position)?;
            }
        }
        wire.u32(bounded_count(self.births.len(), limits.births)?)?;
        for birth in &self.births {
            wire.bytes(&birth.stack_id)?;
            wire.bytes(&birth.source_hash)?;
            wire.bytes(&birth.encoded_cells_hash)?;
        }
        wire.u32(bounded_count(self.replacements.len(), limits.replacements)?)?;
        for replacement in &self.replacements {
            wire.bytes(&replacement.receipt_id)?;
            write_flag(&mut wire, replacement.expected_hash.is_some())?;
            wire.bytes(
                replacement
                    .expected_hash
                    .as_ref()
                    .map_or(&[][..], |hash| hash.as_slice()),
            )?;
            write_flag(&mut wire, replacement.replacement_hash.is_some())?;
            wire.bytes(
                replacement
                    .replacement_hash
                    .as_ref()
                    .map_or(&[][..], |hash| hash.as_slice()),
            )?;
        }
        Ok(wire.into_bytes())
    }

    pub fn decode(
        input: &[u8],
        limits: NativePrepaidDeltaLimits,
    ) -> Result<Self, NativeSectionError> {
        let mut wire = PhloWireDecoder::new(input, limits.wire)?;
        if wire.bytes()? != PREPAID_DELTA_DOMAIN {
            return Err(NativeSectionError::FormatDomain);
        }
        let draw_count = decoded_count(&mut wire, limits.draws)?;
        let mut draws = Vec::new();
        draws
            .try_reserve_exact(draw_count)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        let mut remaining_positions = limits.positions;
        for _ in 0..draw_count {
            let stack_id = wire
                .bytes()?
                .try_into()
                .map_err(|_| NativeSectionError::Noncanonical)?;
            let receipt_index = wire.u32()?;
            let count = decoded_count(&mut wire, remaining_positions)?;
            remaining_positions -= count;
            let mut positions = Vec::new();
            positions
                .try_reserve_exact(count)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            for _ in 0..count {
                positions.push(wire.u32()?);
            }
            draws.push(NativePrepaidDraw {
                stack_id,
                receipt_index,
                positions,
            });
        }
        let birth_count = decoded_count(&mut wire, limits.births)?;
        let mut births = Vec::new();
        births
            .try_reserve_exact(birth_count)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        for _ in 0..birth_count {
            births.push(NativePrepaidBirth {
                stack_id: wire
                    .bytes()?
                    .try_into()
                    .map_err(|_| NativeSectionError::Noncanonical)?,
                source_hash: wire
                    .bytes()?
                    .try_into()
                    .map_err(|_| NativeSectionError::Noncanonical)?,
                encoded_cells_hash: wire
                    .bytes()?
                    .try_into()
                    .map_err(|_| NativeSectionError::Noncanonical)?,
            });
        }
        let replacement_count = decoded_count(&mut wire, limits.replacements)?;
        let mut replacements = Vec::new();
        replacements
            .try_reserve_exact(replacement_count)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        for _ in 0..replacement_count {
            let receipt_id = wire
                .bytes()?
                .try_into()
                .map_err(|_| NativeSectionError::Noncanonical)?;
            let expected_present = flag(&mut wire)?;
            let expected_bytes = wire.bytes()?;
            let expected_hash = if expected_present {
                Some(
                    expected_bytes
                        .try_into()
                        .map_err(|_| NativeSectionError::Noncanonical)?,
                )
            } else if expected_bytes.is_empty() {
                None
            } else {
                return Err(NativeSectionError::Noncanonical);
            };
            let replacement_present = flag(&mut wire)?;
            let replacement_bytes = wire.bytes()?;
            let replacement_hash = if replacement_present {
                Some(
                    replacement_bytes
                        .try_into()
                        .map_err(|_| NativeSectionError::Noncanonical)?,
                )
            } else if replacement_bytes.is_empty() {
                None
            } else {
                return Err(NativeSectionError::Noncanonical);
            };
            replacements.push(NativePrepaidReplacement {
                receipt_id,
                expected_hash,
                replacement_hash,
            });
        }
        wire.finish()?;
        let delta = Self {
            draws,
            births,
            replacements,
        };
        delta.check(limits)?;
        Ok(delta)
    }
}

#[cfg(test)]
mod tests;
