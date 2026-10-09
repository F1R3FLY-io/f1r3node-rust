use std::mem::size_of;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::casper::protocol::casper_message::Event as CasperEvent;
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::{
    offered_funded_v6_host_work_limits, offered_funded_v6_limits,
};
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
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;

use super::prepaid_receipts::{
    CanonicalPrepaidSelection, NativeRetainedStackRecord, PrepaidReceiptChange,
};
use super::production_limits::{
    offered_funded_v6_recording_limits, offered_funded_v6_replay_limits,
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
        journal: encode_native_operation_journal(journal, &recording.paths, limits, budget)
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
    let mut recording = decode_native_budget_recording(evidence.budget_recording, limits, budget)
        .map_err(|error| invalid(&error.to_string()))?;
    if recording.used != evidence.phlo_used || recording.session != evidence.envelope_commitment {
        return Err(invalid(
            "native budget recording differs from committed offer or usage",
        ));
    }
    let journal =
        decode_native_operation_journal(evidence.operation_journal, &mut recording, limits, budget)
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

// Moved by DR-115 from direct_wallet_funding/execution/replay.rs, unchanged,
// so the merge index slices the same user-event boundary as replay.
pub(crate) fn native_user_event_count(
    operations: &[NativeOperationRecord],
    maximum: usize,
    host: &HostWorkBudget,
) -> Result<usize, CasperError> {
    host.reserve(
        HostWorkDimension::VerificationOperations,
        HostWorkUnits::new(
            u64::try_from(operations.len())
                .map_err(|_| invalid("native user event count overflows"))?,
        ),
    )
    .map_err(|error| invalid(&error.to_string()))?;
    operations.iter().try_fold(0usize, |count, operation| {
        let width = match operation.completion {
            RSpaceOperationCompletion::Rejected => 0,
            RSpaceOperationCompletion::Stored => 1,
            RSpaceOperationCompletion::Matched => 2,
        };
        count
            .checked_add(width)
            .filter(|count| *count <= maximum)
            .ok_or_else(|| invalid("native user event count exceeds replay limit"))
    })
}

/// Added by DR-115: the number of user events that the committed operation
/// journal records. Replay binds the same prefix of the deploy log to the
/// native user trace.
pub fn committed_user_event_count(
    evidence: &NativeCostEvidenceV1<'_>,
    budget: &HostWorkBudget,
) -> Result<usize, CasperError> {
    let (_recording, operations) =
        decode_committed_runtime_recording(evidence, offered_funded_v6_recording_limits(), budget)?;
    native_user_event_count(
        &operations,
        offered_funded_v6_replay_limits().trace.events,
        budget,
    )
}

/// Added by DR-115: the settlement suffix of a deploy log after its
/// `user_events`. Fails closed when the log cannot hold the user events and
/// the committed wallet settlement events.
pub(crate) fn committed_settlement_suffix<'l>(
    log: &'l [CasperEvent],
    user_events: usize,
    evidence: &NativeCostEvidenceV1<'_>,
) -> Result<&'l [CasperEvent], CasperError> {
    let wallet_events = usize::try_from(evidence.wallet_settlement_log_events)
        .map_err(|_| invalid("offered wallet settlement event count overflows"))?;
    let suffix = log
        .get(user_events..)
        .ok_or_else(|| invalid("failed offered deploy log omits native user events"))?;
    if suffix.len() < wallet_events {
        return Err(invalid(
            "failed offered settlement suffix omits wallet settlement events",
        ));
    }
    Ok(suffix)
}

/// Added by DR-115: the committed part of a failed offered deploy's log. A
/// user failure rolls back the user events, and the settlement events after
/// them (the wallet settlement and the receipts) stay committed.
pub fn failed_offered_committed_suffix(
    processed: &OfferedProcessedDeploy,
) -> Result<&[CasperEvent], CasperError> {
    if !processed.is_failed() {
        return Err(invalid(
            "a committed settlement suffix belongs only to a failed offered deploy",
        ));
    }
    let budget = HostWorkBudget::new(offered_funded_v6_host_work_limits());
    let evidence = processed
        .evidence(offered_funded_v6_limits().evidence)
        .map_err(|error| invalid(&error))?;
    let user_events = committed_user_event_count(&evidence, &budget)?;
    committed_settlement_suffix(processed.deploy_log(), user_events, &evidence)
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

    // DR-115: the committed settlement suffix of a failed offered deploy.
    use models::rust::casper::protocol::casper_message::{Event as CasperEvent, ProduceEvent};
    use models::rust::native_cost_evidence::{NativeCostEvidenceV1, NativeCostFailureClass};
    use proptest::prelude::*;
    use proptest::test_runner::RngSeed;

    use super::committed_settlement_suffix;

    /// A log event that names its position, so slices compare by position.
    fn event(position: usize) -> CasperEvent {
        let name = prost::bytes::Bytes::from(position.to_be_bytes().to_vec());
        CasperEvent::Produce(ProduceEvent {
            channels_hash: name.clone(),
            hash: name,
            persistent: false,
            times_repeated: 0,
            is_deterministic: true,
            output_value: Vec::new(),
            failed: false,
        })
    }

    /// A log of `user` rolled-back user events, then `wallet` wallet settlement
    /// events, then `receipts` receipt events.
    fn deploy_log(user: usize, wallet: usize, receipts: usize) -> Vec<CasperEvent> {
        (0..user + wallet + receipts).map(event).collect()
    }

    fn evidence_with_wallet_events(wallet_events: u64) -> NativeCostEvidenceV1<'static> {
        NativeCostEvidenceV1 {
            envelope_commitment: [0; 32],
            genesis_policy_commitment: [0; 32],
            schedule_commitment: [0; 32],
            original_funding_root: [0; 32],
            settlement_runtime_root: [0; 32],
            post_state_root: [0; 32],
            phlo_used: 0,
            fresh_phlo: 0,
            retained_phlo: 0,
            phlo_limit: 0,
            phlo_price: 0,
            fee_rev: 0,
            failure_class: NativeCostFailureClass::UserFailure,
            wallet_settlement_log_events: wallet_events,
            budget_recording: &[],
            operation_journal: &[],
            funding_case: &[],
            prepaid_delta: &[],
            wallet_settlement: &[],
        }
    }

    #[test]
    fn committed_settlement_suffix_is_the_log_after_the_user_events() {
        let log = deploy_log(3, 2, 1);
        let suffix = committed_settlement_suffix(&log, 3, &evidence_with_wallet_events(2))
            .expect("the log holds the user and wallet events");
        assert_eq!(suffix, &log[3..]);
        assert_eq!(suffix.len(), 3);
    }

    #[test]
    fn committed_settlement_suffix_fails_closed_when_the_log_is_short() {
        let log = deploy_log(3, 2, 0);
        assert!(committed_settlement_suffix(&log, 6, &evidence_with_wallet_events(0)).is_err());
        assert!(committed_settlement_suffix(&log, 3, &evidence_with_wallet_events(3)).is_err());
        assert_eq!(
            committed_settlement_suffix(&log, 5, &evidence_with_wallet_events(0))
                .expect("an empty suffix holds zero wallet events"),
            &log[5..]
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 256,
            rng_seed: RngSeed::Fixed(115),
            ..ProptestConfig::default()
        })]

        /// The suffix is exactly the wallet and receipt events. It shares no
        /// event with the rolled-back user prefix, and either off-by-one
        /// boundary gives another slice.
        #[test]
        fn prop_committed_settlement_suffix_splits_the_log(
            user in 0usize..12,
            wallet in 0usize..12,
            receipts in 0usize..12,
        ) {
            let log = deploy_log(user, wallet, receipts);
            let suffix = committed_settlement_suffix(
                &log,
                user,
                &evidence_with_wallet_events(wallet as u64),
            )
            .expect("the log holds the user and wallet events");
            prop_assert_eq!(suffix, &log[user..]);
            prop_assert_eq!(suffix.len(), wallet + receipts);
            prop_assert!(suffix.iter().all(|event| !log[..user].contains(event)));
            if user > 0 {
                prop_assert_ne!(&log[user - 1..], suffix);
            }
            if !suffix.is_empty() {
                prop_assert_ne!(&log[user + 1..], suffix);
            }
        }

        /// A log that cannot hold the user events, or that holds fewer events
        /// after them than the committed wallet events, fails closed.
        #[test]
        fn prop_committed_settlement_suffix_fails_closed_on_truncation(
            user in 0usize..12,
            wallet in 1usize..12,
            receipts in 0usize..12,
            cut in 1usize..12,
        ) {
            let log = deploy_log(user, wallet, receipts);
            let evidence = evidence_with_wallet_events(wallet as u64);
            prop_assert!(committed_settlement_suffix(&log, log.len() + cut, &evidence).is_err());
            let short = &log[..user + wallet - cut.min(wallet)];
            prop_assert!(committed_settlement_suffix(short, user, &evidence).is_err());
        }
    }
}
