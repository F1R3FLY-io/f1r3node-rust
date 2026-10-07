use std::num::NonZeroUsize;

use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::phlo_obligation::PhloObligationKeyLimits;
use models::rust::phlo_schedule::PhloScheduleLimits;
use models::rust::phlo_wire::PhloWireLimits;
use rholang::rust::interpreter::accounting::monetary_allocation::FundingSearchLimits;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativeBudgetTraceLimits, NativePhloAcquisitionLimits, NativePhloPurseLimits,
    NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    PhloCaptureLimits, PhloConsentLimits, PhloExecutionLimits, PhloFamilyFundingLimits,
    PhloFundingLimits, PhloOutcomeMatchLimits, RetainedBirthFundingLimits,
    RetainedCellBackingLimits, SignedPhloConsentLimits,
};
use rholang::rust::interpreter::accounting::{
    NativeOperationJournalLimits, NativeOperationTraceLimits, NativeRecordingWireLimits,
};

use super::direct_wallet_funding::{
    NativeAttemptSettlementLimits, NativeFundedReplayLimits, NativeOfferedFamilyLimits,
    NativeOfferedProductionLimits,
};
use super::prepaid_receipts::{
    NativeMeasuredSettlementLimits, NativePrepaidCellLimits, NativePrepaidDemandLimits,
    NativePrepaidInventoryLimits, NativeRetainedBirthLimits, NativeRetainedRecordLimits,
    NativeRetainedSettlementLimits, PrepaidCellLimits, PrepaidReceiptBucketLimits,
    PrepaidReceiptLimits, PrepaidStackCaptureLimits, PrepaidStackPopLimits,
};

const V6_NATIVE_PATH_SEGMENTS: usize = 1_024;

pub fn offered_funded_v6_trace_limits() -> NativeBudgetTraceLimits {
    let protocol = offered_funded_v6_limits();
    NativeBudgetTraceLimits {
        attempts: protocol.deploy_log_events,
        path_segments: V6_NATIVE_PATH_SEGMENTS,
        regions: NativePhloRegionLimits {
            regions: protocol.envelope.payload.funding.authority_nodes,
            encoded_authority_bytes: protocol.funding_case.key_bytes,
        },
    }
}

pub fn offered_funded_v6_recording_limits() -> NativeRecordingWireLimits {
    let protocol = offered_funded_v6_limits();
    NativeRecordingWireLimits {
        wire: PhloWireLimits {
            total_bytes: protocol.evidence.field_bytes,
            field_bytes: protocol.evidence.field_bytes,
        },
        budget: offered_funded_v6_trace_limits(),
        operations: protocol.deploy_log_events,
        source_entries: protocol.deploy_log_items,
        footprint_entries: protocol.deploy_log_items,
        footprint_bytes: protocol.evidence.field_bytes,
        predecessor_edges: protocol.deploy_log_items,
        total_path_segments: protocol.deploy_log_items,
    }
}

pub fn offered_funded_v6_replay_limits() -> NativeFundedReplayLimits {
    let protocol = offered_funded_v6_limits();
    let recording = offered_funded_v6_recording_limits();
    NativeFundedReplayLimits {
        journal: NativeOperationJournalLimits {
            budget: recording.budget,
            operations: recording.operations,
            total_path_segments: protocol.deploy_log_items,
            source_entries: recording.source_entries,
            footprint_entries: recording.footprint_entries,
            footprint_bytes: recording.footprint_bytes,
            predecessor_edges: recording.predecessor_edges,
        },
        trace: NativeOperationTraceLimits {
            events: protocol.deploy_log_events,
            source_entries: protocol.deploy_log_items,
            source_bytes: protocol.deploy_log_bytes,
            telemetry_items: protocol.deploy_log_items,
            telemetry_bytes: protocol.deploy_log_bytes,
        },
    }
}

pub fn offered_funded_v6_production_limits() -> NativeOfferedProductionLimits {
    let protocol = offered_funded_v6_limits();
    let funding = protocol.envelope.payload.funding;
    let case = protocol.funding_case;
    let prepaid = protocol.prepaid_delta;
    let one_case = NonZeroUsize::new(1).expect("one selected case");
    let source_cap = NonZeroUsize::new(funding.sources).expect("positive source limit");
    let obligation_cap = NonZeroUsize::new(case.obligations).expect("positive obligation limit");
    let execution = PhloExecutionLimits {
        resource_entries: case.obligations,
        authority_nodes: funding.authority_nodes,
        key_bytes: case.key_bytes,
    };
    let key = PhloObligationKeyLimits {
        wire: PhloWireLimits {
            total_bytes: case.wire.field_bytes,
            field_bytes: case.wire.field_bytes,
        },
        authority_nodes: funding.authority_nodes,
    };
    let receipt_bucket = PrepaidReceiptBucketLimits {
        occurrences: prepaid.positions,
        wire: prepaid.wire,
    };
    let prepaid_cell = NativePrepaidCellLimits {
        wire: prepaid.wire,
        authority_nodes: funding.authority_nodes,
        sources: funding.sources,
    };
    let cells = PrepaidCellLimits {
        cells: prepaid.positions,
        wire: prepaid.wire,
    };
    let receipts = PrepaidReceiptLimits {
        entries: prepaid.replacements,
        value_bytes: prepaid.wire.field_bytes,
        batch_bytes: prepaid.wire.total_bytes,
    };
    let retained = NativeRetainedBirthLimits {
        funding: RetainedBirthFundingLimits {
            births: prepaid.births,
            cells: prepaid.positions,
            obligations: case.obligations,
            authority_bytes: case.key_bytes,
        },
        physical_cells: prepaid.positions,
        physical_bytes: protocol.deploy_log_bytes,
    };
    let matching = PhloOutcomeMatchLimits {
        execution,
        key,
        aggregate_key_bytes: case.key_bytes,
        cases: one_case,
    };
    let capture = PhloCaptureLimits {
        funding: FundingSearchLimits {
            source_cap,
            obligation_cap,
        },
        key,
        aggregate_key_bytes: case.key_bytes,
    };
    NativeOfferedProductionLimits {
        family: NativeOfferedFamilyLimits {
            measured: NativeAttemptSettlementLimits {
                observations: protocol.deploy_log_events,
                regions: NativePhloRegionLimits {
                    regions: funding.authority_nodes,
                    encoded_authority_bytes: case.key_bytes,
                },
                purses: NativePhloPurseLimits {
                    bindings: funding.authority_nodes,
                    encoded_binding_bytes: case.custody_bytes,
                },
                acquisition: NativePhloAcquisitionLimits {
                    schedule: PhloScheduleLimits {
                        wire: funding.controls.wire,
                        classes: funding.controls.total_classes,
                    },
                    entries: case.obligations,
                    key,
                },
                demand: NativePrepaidDemandLimits {
                    draws: prepaid.draws,
                    authority_bytes: case.key_bytes,
                    execution,
                },
                settlement: NativeMeasuredSettlementLimits { matching, capture },
            },
            signed: SignedPhloConsentLimits {
                members: protocol.envelope.members,
                intent: funding,
                consent: PhloConsentLimits {
                    sources: funding.sources,
                    permission_entries: funding.resource_permissions,
                    case_cells: case.cells,
                    authority_nodes: funding.authority_nodes,
                    key_bytes: case.key_bytes,
                },
            },
            policy: PhloFamilyFundingLimits {
                funding: PhloFundingLimits {
                    sources: source_cap,
                    cases: one_case,
                    obligations: obligation_cap,
                    assignment_cells: case.cells,
                    custody_bytes: case.custody_bytes,
                },
                keys: key,
                aggregate_key_bytes: case.key_bytes,
            },
            retained,
        },
        stack_capture: PrepaidStackCaptureLimits {
            stacks: prepaid.draws,
            physical_cells: prepaid.positions,
            physical_bytes: protocol.deploy_log_bytes,
            bucket: receipt_bucket,
            cells,
            native_cell: prepaid_cell,
            receipts,
        },
        retained_record: NativeRetainedRecordLimits {
            backing: RetainedCellBackingLimits {
                cells: prepaid.positions,
                contributions: case.cells,
            },
            cell: prepaid_cell,
            stack: cells,
            aggregate_bytes: prepaid.wire.total_bytes,
        },
        receipt_bucket,
        retained_settlement: NativeRetainedSettlementLimits {
            births: retained,
            receipts,
        },
        stack_pop: PrepaidStackPopLimits {
            draws: prepaid.draws,
            bucket: receipt_bucket,
            cells,
            receipts,
        },
        prepaid_cell,
        physical_cells: prepaid.positions,
        physical_bytes: protocol.deploy_log_bytes,
        execution,
        recording: offered_funded_v6_recording_limits(),
    }
}

pub fn offered_funded_v6_prepaid_inventory_limits() -> NativePrepaidInventoryLimits {
    let production = offered_funded_v6_production_limits();
    NativePrepaidInventoryLimits {
        cells: production.stack_capture.cells,
        cell: production.stack_capture.native_cell,
        execution: production.execution,
    }
}

pub fn offered_funded_v6_prepaid_receipt_limits() -> PrepaidReceiptLimits {
    offered_funded_v6_production_limits().stack_capture.receipts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v6_nested_production_caps_are_finite_and_bound_to_wire() {
        let protocol = offered_funded_v6_limits();
        let limits = offered_funded_v6_production_limits();
        let family = limits.family;
        assert_eq!(
            family.signed.intent.sources,
            protocol.envelope.payload.funding.sources
        );
        assert_eq!(family.signed.members, protocol.envelope.members);
        assert_eq!(family.policy.funding.cases.get(), 1);
        assert_eq!(family.measured.settlement.matching.cases.get(), 1);
        assert_eq!(
            family.policy.funding.assignment_cells,
            protocol.funding_case.cells
        );
        assert_eq!(
            family.measured.settlement.capture.funding.source_cap.get(),
            protocol.funding_case.sources
        );
        assert_eq!(
            family.measured.acquisition.schedule.classes,
            protocol.envelope.payload.funding.controls.total_classes
        );
        assert_eq!(
            family.retained.funding.births,
            protocol.prepaid_delta.births
        );
        assert_eq!(
            limits.stack_capture.cells.cells,
            protocol.prepaid_delta.positions
        );
        assert_eq!(limits.stack_pop.draws, protocol.prepaid_delta.draws);
        assert_eq!(
            limits.retained_record.backing.contributions,
            protocol.funding_case.cells
        );
        assert_eq!(
            limits.retained_settlement.receipts.entries,
            protocol.prepaid_delta.replacements
        );
        assert_eq!(
            limits.receipt_bucket.wire.total_bytes,
            protocol.prepaid_delta.wire.total_bytes
        );
        assert!(limits.prepaid_cell.wire.field_bytes <= limits.prepaid_cell.wire.total_bytes);
        assert!(limits.physical_bytes < usize::MAX);
        assert!(limits.execution.resource_entries < usize::MAX);
        assert!(limits.recording.wire.total_bytes <= protocol.evidence.field_bytes);
    }

    #[test]
    fn v6_recording_and_replay_share_exact_structural_caps() {
        let protocol = offered_funded_v6_limits();
        let trace = offered_funded_v6_trace_limits();
        let recording = offered_funded_v6_recording_limits();
        let replay = offered_funded_v6_replay_limits();
        assert_eq!(trace.attempts, protocol.deploy_log_events);
        assert!(trace.path_segments > 0 && trace.path_segments < usize::MAX);
        assert_eq!(recording.operations, replay.journal.operations);
        assert_eq!(recording.budget.attempts, replay.journal.budget.attempts);
        assert_eq!(
            recording.budget.path_segments,
            replay.journal.budget.path_segments
        );
        assert_eq!(recording.footprint_bytes, replay.journal.footprint_bytes);
        assert_eq!(
            recording.predecessor_edges,
            replay.journal.predecessor_edges
        );
        assert_eq!(replay.trace.events, protocol.deploy_log_events);
        assert_eq!(replay.trace.telemetry_items, protocol.deploy_log_items);
        assert_eq!(replay.trace.telemetry_bytes, protocol.deploy_log_bytes);
        assert!(recording.wire.field_bytes <= recording.wire.total_bytes);
        assert!(recording.wire.total_bytes <= protocol.evidence.field_bytes);
    }

    #[test]
    fn v6_prepaid_entry_profiles_match_producer_limits() {
        let production = offered_funded_v6_production_limits();
        let inventory = offered_funded_v6_prepaid_inventory_limits();
        let receipts = offered_funded_v6_prepaid_receipt_limits();
        assert_eq!(inventory.cells, production.stack_capture.cells);
        assert_eq!(inventory.cell, production.stack_capture.native_cell);
        assert_eq!(inventory.execution, production.execution);
        assert_eq!(receipts, production.stack_capture.receipts);
    }
}
