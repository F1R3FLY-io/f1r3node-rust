use std::num::NonZeroUsize;

use models::rust::phlo_obligation::PhloObligationKeyLimits;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::phlo_wire::PhloWireLimits;

use super::*;
use crate::rust::interpreter::accounting::byte_receipts::{
    ByteObservation, ByteObservationSnapshot,
};
use crate::rust::interpreter::accounting::economic_failure::EvaluationFailureSummary;
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchLimits;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloAcquisitionLimits, NativePhloPurseLimits, NativePhloRegionLimits, NativePhloRules,
};
use crate::rust::interpreter::accounting::native_runtime::tests::native_schedule;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, check_phlo_funding_family, prepare_counted_phlo_discharge,
    project_phlo_obligations, PhloDischargeLimits, PhloExecutionLimits, PhloFundingCase,
    PhloFundingLimits, PhloFundingSource, PhloObligationFundingInput, PhloObligationFundingLimits,
    PhloObligationFundingProposal, PhloOutcome,
};
use crate::rust::interpreter::accounting::{funding_sig_compound, Sig};

#[derive(Clone, Copy)]
pub(super) struct FundingFixture {
    pub owners: usize,
    pub price: u64,
    pub weights: [u64; 4],
}

impl Default for FundingFixture {
    fn default() -> Self {
        Self {
            owners: 1,
            price: 3,
            weights: [0, 1, 1, 0],
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Projection {
    keys: Vec<Vec<u8>>,
    quantities: Vec<u64>,
    amounts: Vec<u64>,
    assignment: Vec<Vec<u64>>,
    native: Vec<(Vec<u8>, i64, i64, i64, i64)>,
    resource_cursor: Option<usize>,
    fee_cursor: Option<usize>,
}

fn cap(value: usize) -> NonZeroUsize { NonZeroUsize::new(value).unwrap() }

impl FundingFixture {
    fn identities(self) -> Vec<Vec<u8>> {
        (1..=self.owners)
            .map(|owner| (owner as u64).to_be_bytes().to_vec())
            .collect()
    }

    pub(super) fn authority(self) -> Sig {
        let identities = self.identities();
        funding_sig_compound(&identities.iter().map(Vec::as_slice).collect::<Vec<_>>())
    }

    pub(super) fn charge(self, observation: &ByteObservation) -> u64 {
        use models::rhoapi::cost_signature::Value;
        let mut units = 0_u64;
        let mut pending: Vec<_> = observation
            .authority
            .regions
            .iter()
            .map(|region| region.signature.as_ref().unwrap())
            .collect();
        while let Some(signature) = pending.pop() {
            match signature.value.as_ref().unwrap() {
                Value::Unit(true) => {}
                Value::Ground(_) | Value::Quote(_) | Value::Name(_) => units += 1,
                Value::Compound(compound) => pending.extend(&compound.elements),
                other => panic!("unsupported test funding authority: {other:?}"),
            }
        }
        let raw = observation.measurement.unwrap();
        units
            * [
                u64::from(observation.kind == AuthorityByteEventKind::Comm),
                raw.introduction_bytes,
                raw.transfer_bytes,
                raw.trace_bytes,
            ]
            .into_iter()
            .zip(self.weights)
            .map(|(quantity, weight)| quantity * weight)
            .sum::<u64>()
    }

    pub(super) fn project(
        self,
        observations: &ByteObservationSnapshot,
        limit: u64,
        usage: u64,
        failures: EvaluationFailureSummary,
    ) -> Projection {
        let descriptor = native_schedule(self.weights, self.price);
        let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
        let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
        let host = priced_config(limit, self.weights, self.price).host_work();
        let rules = NativePhloRules::resolve(&descriptor).unwrap();
        assert_eq!(
            observations
                .rows
                .iter()
                .map(|row| self.charge(row))
                .sum::<u64>(),
            usage
        );
        let measured = rules.measure(observations, 100_000).unwrap();
        let regions = measured
            .region_demands(
                NativePhloRegionLimits {
                    regions: 100_000,
                    encoded_authority_bytes: 10_000_000,
                },
                &host,
            )
            .unwrap();
        let located = regions
            .locate_purses(
                NativePhloPurseLimits {
                    bindings: 100_000,
                    encoded_binding_bytes: 10_000_000,
                },
                &host,
            )
            .unwrap();
        let demand = located
            .prepare_acquisition_demand(
                &binding,
                &terms,
                NativePhloAcquisitionLimits {
                    schedule: PhloGenesisPolicy::LIMITS,
                    entries: 100_000,
                },
                &host,
            )
            .unwrap();
        let schedules = [binding.schedule()];
        let ceilings = vec![self.price; self.owners];
        let controls = check_phlo_controls(
            schedules[0].environment,
            0,
            u64::MAX,
            SignedPhloControls {
                limit,
                price_ceiling: self.price,
                required_owner_ceilings: &ceilings,
                permitted_schedules: &schedules,
            },
            schedules[0],
            limit,
        )
        .unwrap();
        demand.check_controls(controls).unwrap();
        let execution_limits = PhloExecutionLimits {
            resource_entries: 100_000,
            authority_nodes: 1_000_000,
            key_bytes: 20_000_000,
        };
        let key_limits = PhloObligationKeyLimits {
            wire: PhloWireLimits {
                total_bytes: 1_000_000,
                field_bytes: 500_000,
            },
            authority_nodes: 10_000,
        };
        let discharge = prepare_counted_phlo_discharge(
            &[],
            demand.resources(),
            PhloDischargeLimits {
                execution: execution_limits,
                key: key_limits,
                aggregate_key_bytes: 20_000_000,
            },
            &host,
        )
        .unwrap();
        let execution =
            check_counted_phlo_execution(controls, discharge.witness(), execution_limits).unwrap();
        assert_eq!(execution.usage(), usage);
        assert_eq!(execution.fresh_usage(), usage);
        assert_eq!(execution.prepaid_usage(), 0);
        let failure_classes: Vec<_> = [
            PhloFailure::User,
            PhloFailure::Platform,
            PhloFailure::Certificate,
            PhloFailure::Unclassified,
        ]
        .into_iter()
        .filter(|failure| failures.contains(*failure))
        .collect();
        let outcome = PhloOutcome::Accepted(&failure_classes);
        let obligations = project_phlo_obligations(execution, outcome, cap(100_000)).unwrap();
        let expected = if failures.permits_retained_charge() {
            1 + usage * self.price
        } else {
            0
        };
        assert_eq!(obligations.total(), expected);
        let identities = self.identities();
        let keys: Vec<_> = identities.iter().map(Vec::as_slice).collect();
        let capacities = vec![expected.max(1); self.owners];
        let eligible = vec![vec![true; obligations.amounts().len()]; self.owners];
        let input = PhloObligationFundingInput {
            source_keys: &keys,
            capacities: &capacities,
            eligible: &eligible,
            canonical_resource_cursor: 0,
            canonical_fee_cursor: self.owners - 1,
        };
        let selection_limits = PhloObligationFundingLimits {
            search: FundingSearchLimits {
                source_cap: cap(self.owners),
                obligation_cap: cap(100_000),
            },
            keys: key_limits,
            aggregate_key_bytes: 20_000_000,
        };
        let selected = obligations
            .select_funding(input, selection_limits, &host)
            .unwrap()
            .unwrap();
        assert!(obligations
            .verify_funding(
                input,
                PhloObligationFundingProposal {
                    assignment: selected.assignment(),
                    resource_next_cursor: selected.resource_next_cursor(),
                    fee_next_cursor: selected.fee_next_cursor(),
                },
                selection_limits,
                &host
            )
            .unwrap());
        let sources: Vec<_> = keys
            .iter()
            .zip(&capacities)
            .map(|(custody, capacity)| PhloFundingSource {
                custody,
                capacity: *capacity,
                exposure_limit: *capacity,
                debit_limit: *capacity,
            })
            .collect();
        let cases = [PhloFundingCase {
            obligations: &obligations,
            eligible: &eligible,
            assignment: selected.assignment(),
        }];
        let family =
            check_phlo_funding_family(&sources, &cases, expected.into(), PhloFundingLimits {
                sources: cap(self.owners),
                cases: cap(1),
                obligations: cap(100_000),
                assignment_cells: self.owners * obligations.amounts().len(),
                custody_bytes: self.owners * 8,
            })
            .unwrap();
        let amounts = family.native_amounts(0).unwrap();
        assert_eq!(
            amounts
                .iter()
                .map(|amount| amount.acquisition() + amount.fee())
                .sum::<i64>(),
            expected as i64
        );
        for amount in &amounts {
            assert_eq!(
                amount.hold(),
                amount.acquisition() + amount.fee() + amount.refund()
            );
        }
        if expected > 0 {
            let short = vec![0; self.owners];
            assert!(obligations
                .select_funding(
                    PhloObligationFundingInput {
                        capacities: &short,
                        ..input
                    },
                    selection_limits,
                    &host
                )
                .unwrap()
                .is_none());
        }
        let resource_debits: Vec<_> = selected
            .assignment()
            .iter()
            .map(|row| row[1..].iter().sum::<u64>())
            .collect();
        assert!(resource_debits.iter().max().unwrap() - resource_debits.iter().min().unwrap() <= 1);
        Projection {
            keys: obligations.encoded_keys(key_limits, 20_000_000).unwrap(),
            quantities: obligations.quantities().to_vec(),
            amounts: obligations.amounts().to_vec(),
            assignment: selected.assignment().to_vec(),
            native: amounts
                .iter()
                .map(|amount| {
                    (
                        amount.custody().to_vec(),
                        amount.hold(),
                        amount.acquisition(),
                        amount.fee(),
                        amount.refund(),
                    )
                })
                .collect(),
            resource_cursor: selected.resource_next_cursor(),
            fee_cursor: selected.fee_next_cursor(),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_replay_funding_accepts_thirty_three_owners() {
    replay_process_with_funding(
        r#"@"a"!(7) | for (@x <- @"a") { @"out"!(x) }"#,
        1_000_000,
        false,
        true,
        Interruption::Export,
        FundingFixture {
            owners: 33,
            price: 5,
            weights: [2, 1, 1, 1],
        },
    )
    .await;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn native_replay_preserves_multi_owner_priced_funding(
        owners in 1_usize..10, price in 1_u64..8, value in 0_i64..1000,
    ) {
        let term = format!("@\"a\"!({value}) | @\"b\"!(1) | for (@x <- @\"a\"; @y <- @\"b\") {{ @\"out\"!(x + y) }}");
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(async {
            let funding = FundingFixture { owners, price, weights: [2, 1, 1, 1] };
            let (denial_limit, _) = replay_process_with_funding(&term, 1_000_000, false, true, Interruption::Export, funding).await;
            if let Some(limit) = denial_limit {
                assert!(replay_process_with_funding(&term, limit, false, false, Interruption::None, funding).await.1);
            }
        });
    }
}
