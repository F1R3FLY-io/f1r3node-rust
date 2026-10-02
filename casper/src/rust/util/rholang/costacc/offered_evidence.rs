use std::mem::size_of;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::native_cost_evidence::{
    NativeCostEvidenceV1, NativeFundingCaseLimits, NativeFundingCaseSource, NativeFundingCaseV1,
    NativeFundingObligation, NativePrepaidBirth, NativePrepaidDeltaLimits, NativePrepaidDeltaV1,
    NativePrepaidDraw, NativePrepaidReplacement,
};
use models::rust::phlo_wire::PhloWireLimits;
use rholang::rust::interpreter::accounting::monetary_allocation::FundingOutcomeCursorTransition;
use rholang::rust::interpreter::accounting::phlo_execution::CanonicalPhloFundingCapture;
use rholang::rust::interpreter::accounting::{
    decode_native_budget_recording, decode_native_operation_journal,
    encode_native_budget_recording, encode_native_operation_journal, NativeBudgetRecording,
    NativeOperationRecord, NativeRecordingWireLimits,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::EvaluateResult;

use super::prepaid_receipts::{
    CanonicalPrepaidSelection, NativeRetainedStackRecord, PrepaidReceiptChange,
};
use crate::rust::errors::CasperError;

const PREPAID_CELLS_HASH_DOMAIN: &[u8] = b"f1r3node:prepaid-cells-evidence:v1";
const PREPAID_RECEIPT_HASH_DOMAIN: &[u8] = b"f1r3node:prepaid-receipt-evidence:v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedNativeRuntimeRecording {
    pub budget: Vec<u8>,
    pub journal: Vec<u8>,
}

pub fn encode_committed_native_evidence(
    evidence: &NativeCostEvidenceV1<'_>,
    limits: PhloWireLimits,
    budget: &HostWorkBudget,
) -> Result<Vec<u8>, CasperError> {
    let bytes = evidence
        .encoded_len(limits)
        .map_err(|error| invalid(&error.to_string()))?;
    reserve(budget, HostWorkDimension::VerificationBytes, bytes)?;
    reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    reserve(budget, HostWorkDimension::VerificationOperations, 17)?;
    evidence
        .encode(limits)
        .map_err(|error| invalid(&error.to_string()))
}

fn invalid(reason: &str) -> CasperError { CasperError::RuntimeError(reason.to_owned()) }

fn reserve(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), CasperError> {
    let units = u64::try_from(amount).map_err(|_| invalid("native evidence work overflows"))?;
    budget
        .reserve(dimension, HostWorkUnits::new(units))
        .map_err(|error| invalid(&error.to_string()))?;
    Ok(())
}

fn add(total: &mut usize, amount: usize) -> Result<(), CasperError> {
    *total = total
        .checked_add(amount)
        .ok_or_else(|| invalid("native evidence size overflows"))?;
    Ok(())
}

fn source_index(position: usize, original: &[usize]) -> Result<u32, CasperError> {
    let canonical = original
        .iter()
        .position(|candidate| *candidate == position)
        .ok_or_else(|| invalid("native funding cursor has no canonical source"))?;
    u32::try_from(canonical).map_err(|_| invalid("native funding cursor exceeds wire range"))
}

pub fn encode_measured_runtime_recording(
    evaluation: &EvaluateResult,
    limits: NativeRecordingWireLimits,
    budget: &HostWorkBudget,
) -> Result<EncodedNativeRuntimeRecording, CasperError> {
    let used = evaluation
        .native_phlo_usage
        .ok_or_else(|| invalid("native evaluation has no measured phlo usage"))?;
    let recording = evaluation
        .native_budget_recording
        .as_ref()
        .ok_or_else(|| invalid("native evaluation has no complete budget recording"))?;
    let journal = evaluation
        .native_operation_recording
        .as_deref()
        .ok_or_else(|| invalid("native evaluation has no complete operation journal"))?;
    if recording.used != used {
        return Err(invalid(
            "native budget recording differs from measured phlo usage",
        ));
    }
    Ok(EncodedNativeRuntimeRecording {
        budget: encode_native_budget_recording(recording, limits, budget)
            .map_err(|error| invalid(&error.to_string()))?,
        journal: encode_native_operation_journal(journal, limits, budget)
            .map_err(|error| invalid(&error.to_string()))?,
    })
}

pub fn decode_committed_runtime_recording(
    evidence: &NativeCostEvidenceV1<'_>,
    limits: NativeRecordingWireLimits,
    budget: &HostWorkBudget,
) -> Result<
    (
        NativeBudgetRecording,
        std::sync::Arc<[NativeOperationRecord]>,
    ),
    CasperError,
> {
    let recording = decode_native_budget_recording(evidence.budget_recording, limits, budget)
        .map_err(|error| invalid(&error.to_string()))?;
    if recording.used != evidence.phlo_used || recording.session != evidence.envelope_commitment {
        return Err(invalid(
            "native budget recording differs from committed offer or usage",
        ));
    }
    let journal = decode_native_operation_journal(evidence.operation_journal, limits, budget)
        .map_err(|error| invalid(&error.to_string()))?;
    if journal
        .iter()
        .any(|row| row.occurrence.session != evidence.envelope_commitment)
    {
        return Err(invalid(
            "native operation journal differs from committed offer",
        ));
    }
    Ok((recording, journal))
}

pub fn encode_measured_funding_case(
    capture: &CanonicalPhloFundingCapture<'_>,
    transition: &FundingOutcomeCursorTransition,
    limits: NativeFundingCaseLimits,
    budget: &HostWorkBudget,
) -> Result<Vec<u8>, CasperError> {
    let n = capture.sources().len();
    let m = capture.obligation_keys().len();
    let cells = n
        .checked_mul(m)
        .ok_or_else(|| invalid("native funding matrix overflows"))?;
    if n == 0 || m == 0 || n > limits.sources || m > limits.obligations || cells > limits.cells {
        return Err(invalid("native funding case exceeds protocol limits"));
    }
    let original = capture.original_source_positions();
    if original.len() != n || transition.possible_fee_payers().len() != n {
        return Err(invalid(
            "native funding cursor source count differs from capture",
        ));
    }
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        n.checked_mul(n)
            .ok_or_else(|| invalid("native funding source permutation work overflows"))?,
    )?;
    if original.iter().any(|position| *position >= n)
        || original
            .iter()
            .enumerate()
            .any(|(index, position)| original[index + 1..].contains(position))
    {
        return Err(invalid(
            "native funding source positions are not a permutation",
        ));
    }
    if transition
        .resource_restriction_witness()
        .is_some_and(|witness| witness.len() != n)
    {
        return Err(invalid(
            "native funding restriction witness has wrong source count",
        ));
    }
    let mut encoded_size = 8usize + b"f1r3node:native-funding-case:v1".len() + 8;
    for source in capture.sources() {
        add(&mut encoded_size, 8 + source.source().custody.len() + 7 * 8)?;
    }
    for key in capture.obligation_keys() {
        add(&mut encoded_size, 8 + key.len() + 2 * 8)?;
    }
    add(
        &mut encoded_size,
        cells
            .checked_mul(9)
            .ok_or_else(|| invalid("native funding matrix overflows"))?,
    )?;
    add(&mut encoded_size, 2 * 5 + 1 + 1 + 4 + 4 + n)?;
    if transition.resource_restriction_witness().is_some() {
        add(
            &mut encoded_size,
            n.checked_mul(8)
                .ok_or_else(|| invalid("native funding witness overflows"))?,
        )?;
    }
    if encoded_size > limits.wire.total_bytes {
        return Err(invalid("native funding case exceeds wire limit"));
    }
    let allocated = n
        .checked_mul(
            size_of::<NativeFundingCaseSource<'_>>()
                + size_of::<Vec<bool>>()
                + size_of::<Vec<u64>>(),
        )
        .and_then(|bytes| bytes.checked_add(m * size_of::<NativeFundingObligation<'_>>()))
        .and_then(|bytes| bytes.checked_add(cells * (size_of::<bool>() + size_of::<u64>())))
        .and_then(|bytes| bytes.checked_add(n * (size_of::<bool>() + size_of::<u64>())))
        .and_then(|bytes| bytes.checked_add(encoded_size))
        .ok_or_else(|| invalid("native funding case allocation overflows"))?;
    reserve(budget, HostWorkDimension::SearchStateBytes, allocated)?;
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        n + m + cells + encoded_size,
    )?;
    let mut sources = Vec::new();
    let mut obligations = Vec::new();
    let mut eligible = Vec::new();
    let mut assignment = Vec::new();
    let mut possible_fee_payers = Vec::new();
    sources
        .try_reserve_exact(n)
        .map_err(|_| invalid("funding source allocation failed"))?;
    obligations
        .try_reserve_exact(m)
        .map_err(|_| invalid("funding obligation allocation failed"))?;
    eligible
        .try_reserve_exact(n)
        .map_err(|_| invalid("funding eligibility allocation failed"))?;
    assignment
        .try_reserve_exact(n)
        .map_err(|_| invalid("funding assignment allocation failed"))?;
    possible_fee_payers
        .try_reserve_exact(n)
        .map_err(|_| invalid("funding payer allocation failed"))?;
    for (canonical, source) in capture.sources().iter().copied().enumerate() {
        let terms = source.source();
        sources.push(NativeFundingCaseSource {
            custody: terms.custody,
            capacity: terms.capacity,
            exposure_limit: terms.exposure_limit,
            debit_limit: terms.debit_limit,
            hold: source.hold(),
            debit: source.debit(),
            fee: source.fee(),
            refund: source.refund(),
        });
        let original_position = original[canonical];
        possible_fee_payers.push(transition.possible_fee_payers()[original_position]);
        let mut mask = Vec::new();
        let mut row = Vec::new();
        mask.try_reserve_exact(m)
            .map_err(|_| invalid("funding mask allocation failed"))?;
        row.try_reserve_exact(m)
            .map_err(|_| invalid("funding row allocation failed"))?;
        mask.extend_from_slice(&capture.eligible()[canonical]);
        row.extend_from_slice(&capture.assignment()[canonical]);
        eligible.push(mask);
        assignment.push(row);
    }
    for (key, obligation) in capture.obligation_keys().iter().zip(capture.obligations()) {
        obligations.push(NativeFundingObligation {
            key,
            quantity: obligation.quantity(),
            amount: obligation.amount(),
        });
    }
    let resource_restriction_witness = transition
        .resource_restriction_witness()
        .map(|witness| {
            let mut canonical = Vec::new();
            canonical
                .try_reserve_exact(n)
                .map_err(|_| invalid("funding witness allocation failed"))?;
            for position in original {
                canonical.push(witness[*position]);
            }
            Ok::<_, CasperError>(canonical)
        })
        .transpose()?;
    let case = NativeFundingCaseV1 {
        sources,
        obligations,
        eligible,
        assignment,
        resource_next_cursor: transition
            .resource_next_cursor()
            .map(|position| source_index(position, original))
            .transpose()?,
        fee_next_cursor: transition
            .fee_next_cursor()
            .map(|position| source_index(position, original))
            .transpose()?,
        resource_unrestricted: transition.resource_unrestricted(),
        resource_restriction_witness,
        possible_fee_payers,
    };
    case.encode(limits)
        .map_err(|error| invalid(&error.to_string()))
}

pub fn encode_measured_prepaid_delta(
    selection: &CanonicalPrepaidSelection,
    retained: &[NativeRetainedStackRecord],
    changes: &[PrepaidReceiptChange<'_>],
    limits: NativePrepaidDeltaLimits,
    budget: &HostWorkBudget,
) -> Result<Vec<u8>, CasperError> {
    if selection.draws().len() > limits.draws
        || selection.demand_positions().len() > limits.positions
        || retained.len() > limits.births
        || changes.len() > limits.replacements
    {
        return Err(invalid("native prepaid delta exceeds protocol limits"));
    }
    let mut encoded_size = 8usize + b"f1r3node:native-prepaid-delta:v1".len() + 4 + 4 + 4;
    add(
        &mut encoded_size,
        selection
            .draws()
            .len()
            .checked_mul(8 + 32 + 4 + 4)
            .ok_or_else(|| invalid("prepaid draw size overflows"))?,
    )?;
    add(
        &mut encoded_size,
        selection
            .demand_positions()
            .len()
            .checked_mul(4)
            .ok_or_else(|| invalid("prepaid position size overflows"))?,
    )?;
    add(
        &mut encoded_size,
        retained
            .len()
            .checked_mul(3 * (8 + 32))
            .ok_or_else(|| invalid("prepaid birth size overflows"))?,
    )?;
    for change in changes {
        add(&mut encoded_size, 8 + 32 + 2 * (1 + 8))?;
        if change.expected.is_some() {
            add(&mut encoded_size, 32)?;
        }
        if change.replacement.is_some() {
            add(&mut encoded_size, 32)?;
        }
    }
    if encoded_size > limits.wire.total_bytes {
        return Err(invalid("native prepaid delta exceeds wire limit"));
    }
    let allocated = encoded_size
        .checked_add(selection.draws().len() * size_of::<NativePrepaidDraw>())
        .and_then(|bytes| bytes.checked_add(selection.demand_positions().len() * size_of::<u32>()))
        .and_then(|bytes| bytes.checked_add(retained.len() * size_of::<NativePrepaidBirth>()))
        .and_then(|bytes| bytes.checked_add(changes.len() * size_of::<NativePrepaidReplacement>()))
        .ok_or_else(|| invalid("native prepaid allocation overflows"))?;
    reserve(budget, HostWorkDimension::SearchStateBytes, allocated)?;
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        encoded_size + retained.len() + changes.len(),
    )?;
    let mut draws = Vec::new();
    draws
        .try_reserve_exact(selection.draws().len())
        .map_err(|_| invalid("prepaid draw allocation failed"))?;
    let mut positions = selection.demand_positions().iter();
    for draw in selection.draws() {
        let count = usize::try_from(draw.count)
            .map_err(|_| invalid("prepaid draw count exceeds wire range"))?;
        let mut selected = Vec::new();
        selected
            .try_reserve_exact(count)
            .map_err(|_| invalid("prepaid position allocation failed"))?;
        for _ in 0..count {
            let position = positions
                .next()
                .ok_or_else(|| invalid("prepaid draw lacks demand position"))?;
            selected.push(
                u32::try_from(*position)
                    .map_err(|_| invalid("prepaid position exceeds wire range"))?,
            );
        }
        draws.push(NativePrepaidDraw {
            stack_id: draw.stack_id,
            receipt_index: u32::try_from(draw.receipt_index)
                .map_err(|_| invalid("receipt index exceeds wire range"))?,
            positions: selected,
        });
    }
    if positions.next().is_some() {
        return Err(invalid("prepaid demand positions exceed selected draws"));
    }
    let mut births = Vec::new();
    births
        .try_reserve_exact(retained.len())
        .map_err(|_| invalid("prepaid birth allocation failed"))?;
    for record in retained {
        reserve(
            budget,
            HostWorkDimension::VerificationBytes,
            record.encoded_cells().len(),
        )?;
        births.push(NativePrepaidBirth {
            stack_id: *record.stack_id(),
            source_hash: *record.source_hash(),
            encoded_cells_hash: Blake2b256::hash_parts([
                PREPAID_CELLS_HASH_DOMAIN,
                record.encoded_cells(),
            ])
            .as_slice()
            .try_into()
            .map_err(|_| invalid("cell hash width differs from protocol"))?,
        });
    }
    let mut replacements = Vec::new();
    replacements
        .try_reserve_exact(changes.len())
        .map_err(|_| invalid("prepaid replacement allocation failed"))?;
    for change in changes {
        replacements.push(NativePrepaidReplacement {
            receipt_id: change.receipt_id,
            expected_hash: change
                .expected
                .map(|bytes| {
                    reserve(budget, HostWorkDimension::VerificationBytes, bytes.len())?;
                    Blake2b256::hash_parts([PREPAID_RECEIPT_HASH_DOMAIN, bytes])
                        .as_slice()
                        .try_into()
                        .map_err(|_| invalid("prior receipt hash width differs from protocol"))
                })
                .transpose()?,
            replacement_hash: change
                .replacement
                .map(|bytes| {
                    reserve(budget, HostWorkDimension::VerificationBytes, bytes.len())?;
                    Blake2b256::hash_parts([PREPAID_RECEIPT_HASH_DOMAIN, bytes])
                        .as_slice()
                        .try_into()
                        .map_err(|_| {
                            invalid("replacement receipt hash width differs from protocol")
                        })
                })
                .transpose()?,
        });
    }
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        changes
            .len()
            .checked_mul(8)
            .ok_or_else(|| invalid("prepaid replacement sort overflows"))?,
    )?;
    replacements.sort_unstable_by_key(|replacement| replacement.receipt_id);
    let delta = NativePrepaidDeltaV1 {
        draws,
        births,
        replacements,
    };
    delta
        .encode(limits)
        .map_err(|error| invalid(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::source_index;

    #[test]
    fn cursor_position_remaps_from_original_to_canonical_source_order() {
        let canonical_to_original = [2, 0, 1];
        assert_eq!(source_index(2, &canonical_to_original).unwrap(), 0);
        assert_eq!(source_index(0, &canonical_to_original).unwrap(), 1);
        assert_eq!(source_index(1, &canonical_to_original).unwrap(), 2);
        assert!(source_index(3, &canonical_to_original).is_err());
    }
}
