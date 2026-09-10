use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use models::rust::host_work::{
    HostWorkDimension, HostWorkLimits, HostWorkReservation, HostWorkReservationError,
    HostWorkUnits, HostWorkUsage, HostWorkUsages, HOST_WORK_DIMENSION_COUNT,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostWorkReport {
    pub usages: HostWorkUsages,
    pub rejection: Option<HostWorkReservationError>,
}

struct HostWorkBudgetState {
    limits: HostWorkLimits,
    usage: [AtomicU64; HOST_WORK_DIMENSION_COUNT],
    rejected: AtomicBool,
    rejection: OnceLock<HostWorkReservationError>,
}

#[derive(Clone)]
pub struct HostWorkBudget {
    state: Arc<HostWorkBudgetState>,
}

impl HostWorkBudget {
    pub fn new(limits: HostWorkLimits) -> Self {
        Self {
            state: Arc::new(HostWorkBudgetState {
                limits,
                usage: std::array::from_fn(|_| AtomicU64::new(0)),
                rejected: AtomicBool::new(false),
                rejection: OnceLock::new(),
            }),
        }
    }

    pub fn limits(&self) -> HostWorkLimits { self.state.limits }

    pub fn usage(&self, dimension: HostWorkDimension) -> HostWorkUsage {
        HostWorkUsage::new(self.state.usage[dimension.index()].load(Ordering::Acquire))
    }

    pub fn usages(&self) -> HostWorkUsages {
        HostWorkUsages::new(std::array::from_fn(|index| {
            HostWorkUsage::new(self.state.usage[index].load(Ordering::Acquire))
        }))
    }

    pub fn is_rejected(&self) -> bool { self.state.rejected.load(Ordering::Acquire) }

    pub fn rejection(&self) -> Option<HostWorkReservationError> {
        if !self.is_rejected() {
            return None;
        }
        self.state.rejection.get().copied()
    }

    pub fn report(&self) -> HostWorkReport {
        HostWorkReport {
            usages: self.usages(),
            rejection: self.rejection(),
        }
    }

    pub fn reserve(
        &self,
        dimension: HostWorkDimension,
        requested: HostWorkUnits,
    ) -> Result<HostWorkReservation, HostWorkReservationError> {
        if requested == HostWorkUnits::ZERO {
            let usage = self.usage(dimension);
            return Ok(HostWorkReservation {
                dimension,
                usage_before: usage,
                requested,
                usage_after: usage,
            });
        }
        if self.is_rejected() {
            return Err(HostWorkReservationError::BudgetRejected {
                dimension,
                usage: self.usage(dimension),
                requested,
            });
        }
        let counter = &self.state.usage[dimension.index()];
        let limit = self.state.limits.get(dimension);
        let mut current = counter.load(Ordering::Acquire);
        loop {
            let transition =
                HostWorkUsage::new(current).checked_reserve(dimension, limit, requested);
            let reservation = match transition {
                Ok(reservation) => reservation,
                Err(error) => {
                    self.reject(error);
                    return Err(error);
                }
            };
            match counter.compare_exchange_weak(
                current,
                reservation.usage_after.get(),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(reservation),
                Err(observed) => current = observed,
            }
        }
    }

    fn reject(&self, error: HostWorkReservationError) {
        let _ = self.state.rejection.set(error);
        self.state.rejected.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Barrier;
    use std::thread;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rust::host_work::{HostWorkLimit, HostWorkPhase};
    use proptest::prelude::*;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::rho_runtime::RhoRuntime;
    use crate::rust::interpreter::test_utils::resources::with_runtime;

    #[test]
    fn dimensions_in_one_phase_reserve_independently() {
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(8));
        limits.set(HostWorkDimension::SearchStateBytes, HostWorkLimit::new(2));
        let budget = HostWorkBudget::new(limits);

        budget
            .reserve(HostWorkDimension::SearchStateBytes, HostWorkUnits::new(2))
            .unwrap();
        budget
            .reserve(HostWorkDimension::SearchCandidates, HostWorkUnits::new(7))
            .unwrap();

        assert_eq!(
            budget.usage(HostWorkDimension::SearchStateBytes),
            HostWorkUsage::new(2)
        );
        assert_eq!(
            budget.usage(HostWorkDimension::SearchCandidates),
            HostWorkUsage::new(7)
        );
        assert_eq!(
            HostWorkDimension::SearchStateBytes.phase(),
            HostWorkPhase::PhysicalSearch
        );
    }

    #[test]
    fn concurrent_reservations_accumulate_exact_accepted_sum() {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000)));
        let barrier = Arc::new(Barrier::new(9));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let budget = budget.clone();
            let barrier = barrier.clone();
            workers.push(thread::spawn(move || {
                barrier.wait();
                for _ in 0..100 {
                    budget
                        .reserve(HostWorkDimension::ReductionSteps, HostWorkUnits::new(1))
                        .unwrap();
                }
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().unwrap();
        }

        assert_eq!(
            budget.usage(HostWorkDimension::ReductionSteps),
            HostWorkUsage::new(800)
        );
        assert!(!budget.is_rejected());
    }

    #[test]
    fn dimensions_do_not_cross_subsidize() {
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100));
        limits.set(HostWorkDimension::StructuralItems, HostWorkLimit::new(1));
        limits.set(
            HostWorkDimension::StructuralBytes,
            HostWorkLimit::new(1_000),
        );
        let budget = HostWorkBudget::new(limits);

        budget
            .reserve(HostWorkDimension::StructuralBytes, HostWorkUnits::new(900))
            .unwrap();
        let error = budget
            .reserve(HostWorkDimension::StructuralItems, HostWorkUnits::new(2))
            .unwrap_err();

        assert!(matches!(error, HostWorkReservationError::LimitExceeded {
            dimension: HostWorkDimension::StructuralItems,
            ..
        }));
        assert_eq!(
            budget.usage(HostWorkDimension::StructuralItems),
            HostWorkUsage::ZERO
        );
        assert_eq!(
            budget.usage(HostWorkDimension::StructuralBytes),
            HostWorkUsage::new(900)
        );
    }

    #[test]
    fn first_failure_sticks_without_fabricating_usage() {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(5)));
        budget
            .reserve(HostWorkDimension::AuthorityNodes, HostWorkUnits::new(4))
            .unwrap();
        let first = budget
            .reserve(HostWorkDimension::AuthorityNodes, HostWorkUnits::new(2))
            .unwrap_err();
        let subsequent = budget
            .reserve(HostWorkDimension::WitnessBytes, HostWorkUnits::new(1))
            .unwrap_err();

        assert!(matches!(
            first,
            HostWorkReservationError::LimitExceeded { .. }
        ));
        assert!(matches!(
            subsequent,
            HostWorkReservationError::BudgetRejected { .. }
        ));
        assert!(budget.is_rejected());
        assert_eq!(budget.rejection(), Some(first));
        assert_eq!(
            budget.usage(HostWorkDimension::AuthorityNodes),
            HostWorkUsage::new(4)
        );
        assert_eq!(
            budget.usage(HostWorkDimension::WitnessBytes),
            HostWorkUsage::ZERO
        );
    }

    #[test]
    fn concurrent_failure_rejects_later_nonzero_reservations() {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10)));
        let barrier = Arc::new(Barrier::new(5));
        let accepted = Arc::new(AtomicU64::new(0));
        let mut workers = Vec::new();
        for requested in [3, 4, 11, 2] {
            let budget = budget.clone();
            let barrier = barrier.clone();
            let accepted = accepted.clone();
            workers.push(thread::spawn(move || {
                barrier.wait();
                if budget
                    .reserve(
                        HostWorkDimension::PrimitiveCalls,
                        HostWorkUnits::new(requested),
                    )
                    .is_ok()
                {
                    accepted.fetch_add(requested, Ordering::Relaxed);
                }
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().unwrap();
        }

        let exact_accepted = accepted.load(Ordering::Relaxed);
        assert!(budget.is_rejected());
        assert_eq!(
            budget.usage(HostWorkDimension::PrimitiveCalls),
            HostWorkUsage::new(exact_accepted)
        );
        assert!(matches!(
            budget.reserve(HostWorkDimension::PrimitiveCalls, HostWorkUnits::new(1)),
            Err(HostWorkReservationError::BudgetRejected { .. })
        ));
        assert_eq!(
            budget.usage(HostWorkDimension::PrimitiveCalls),
            HostWorkUsage::new(exact_accepted)
        );
    }

    #[test]
    fn overflow_is_typed_without_changing_usage() {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        budget
            .reserve(
                HostWorkDimension::VerificationOperations,
                HostWorkUnits::new(u64::MAX - 1),
            )
            .unwrap();
        let error = budget
            .reserve(
                HostWorkDimension::VerificationOperations,
                HostWorkUnits::new(2),
            )
            .unwrap_err();

        assert!(matches!(
            error,
            HostWorkReservationError::UsageOverflow { .. }
        ));
        assert_eq!(
            budget.usage(HostWorkDimension::VerificationOperations),
            HostWorkUsage::new(u64::MAX - 1)
        );
        assert!(budget.is_rejected());
    }

    #[test]
    fn failed_reservation_does_not_authorize_local_mutation() {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(2)));
        let mutations = AtomicUsize::new(0);

        if budget
            .reserve(
                HostWorkDimension::SubstitutionBindings,
                HostWorkUnits::new(2),
            )
            .is_ok()
        {
            mutations.fetch_add(1, Ordering::Relaxed);
        }
        let rejected = budget.reserve(
            HostWorkDimension::SubstitutionBindings,
            HostWorkUnits::new(1),
        );
        if rejected.is_ok() {
            mutations.fetch_add(1, Ordering::Relaxed);
        }

        assert!(matches!(
            rejected,
            Err(HostWorkReservationError::LimitExceeded { .. })
        ));
        assert_eq!(mutations.load(Ordering::Relaxed), 1);
        assert_eq!(
            budget.usage(HostWorkDimension::SubstitutionBindings),
            HostWorkUsage::new(2)
        );
    }

    #[test]
    fn zero_reservation_remains_a_no_op_after_rejection() {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
        budget
            .reserve(HostWorkDimension::WitnessFields, HostWorkUnits::new(1))
            .unwrap_err();
        let zero = budget
            .reserve(HostWorkDimension::WitnessBytes, HostWorkUnits::ZERO)
            .unwrap();

        assert_eq!(zero.requested, HostWorkUnits::ZERO);
        assert_eq!(zero.usage_before, HostWorkUsage::ZERO);
        assert_eq!(zero.usage_after, HostWorkUsage::ZERO);
        assert!(budget.is_rejected());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn sticky_rejection_reverts_the_complete_evaluation() {
        with_runtime("host-work-rollback-", |mut runtime| async move {
            let hot_before = runtime.get_hot_changes().await;
            let _ = runtime.take_event_log().await;
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
            limits.set(HostWorkDimension::PrimitiveCalls, HostWorkLimit::new(0));
            let (result, report) = runtime
                .evaluate_with_host_work(
                    "new x in { x!(0) | for (_ <- x) { @\"changed\"!(1 + 2) } }",
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_length(128),
                    limits,
                )
                .await
                .unwrap();

            assert_eq!(result.errors, vec![InterpreterError::HostWorkRejected]);
            assert!(matches!(
                report.rejection,
                Some(HostWorkReservationError::LimitExceeded {
                    dimension: HostWorkDimension::PrimitiveCalls,
                    ..
                })
            ));
            assert_eq!(runtime.get_hot_changes().await, hot_before);
            assert!(runtime.take_event_log().await.is_empty());
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn enabled_evaluation_reports_each_instrumented_reducer_dimension() {
        with_runtime("host-work-usage-", |runtime| async move {
            let source = "@\"host-work-success\"!(1 + 2)";
            let (result, report) = runtime
                .evaluate_with_host_work(
                    source,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_length(128),
                    HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)),
                )
                .await
                .unwrap();

            assert!(result.errors.is_empty());
            assert_eq!(report.rejection, None);
            assert!(
                report.usages.get(HostWorkDimension::StructuralBytes).get() > source.len() as u64
            );
            assert_eq!(
                report.usages.get(HostWorkDimension::StructuralItems).get(),
                1
            );
            assert_eq!(
                report.usages.get(HostWorkDimension::ReductionSteps).get(),
                1
            );
            assert!(
                report
                    .usages
                    .get(HostWorkDimension::ReductionTermBytes)
                    .get()
                    > 0
            );
            assert_eq!(
                report.usages.get(HostWorkDimension::PrimitiveCalls).get(),
                1
            );
            assert!(
                report
                    .usages
                    .get(HostWorkDimension::PrimitiveInputBytes)
                    .get()
                    > 0
            );
            assert!(
                report
                    .usages
                    .get(HostWorkDimension::SubstitutionBytes)
                    .get()
                    > 0
            );
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn caller_owned_budget_accumulates_and_reverts_the_rejected_evaluation() {
        with_runtime("host-work-shared-", |runtime| async move {
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
            limits.set(HostWorkDimension::ReductionSteps, HostWorkLimit::new(1));
            let budget = HostWorkBudget::new(limits);

            let first = runtime
                .evaluate_with_authority_and_host_work_budget(
                    "@\"first\"!(0)",
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_length(128),
                    None,
                    budget.clone(),
                )
                .await
                .unwrap();
            assert!(first.errors.is_empty());
            let after_first = runtime.get_hot_changes().await;

            let second = runtime
                .evaluate_with_authority_and_host_work_budget(
                    "@\"second\"!(0)",
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_length(128),
                    None,
                    budget.clone(),
                )
                .await
                .unwrap();

            assert_eq!(second.errors, vec![InterpreterError::HostWorkRejected]);
            assert_eq!(
                budget.usage(HostWorkDimension::ReductionSteps),
                HostWorkUsage::new(1)
            );
            assert!(budget.is_rejected());
            assert_eq!(runtime.get_hot_changes().await, after_first);
        })
        .await;
    }

    proptest! {
        #[test]
        fn dimension_usage_is_independent(
            left_index in 0..HOST_WORK_DIMENSION_COUNT,
            right_index in 0..HOST_WORK_DIMENSION_COUNT,
            requested in 1_u64..=1_000,
        ) {
            prop_assume!(left_index != right_index);
            let left = HostWorkDimension::ALL[left_index];
            let right = HostWorkDimension::ALL[right_index];
            let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000)));

            budget.reserve(left, HostWorkUnits::new(requested)).unwrap();

            prop_assert_eq!(budget.usage(left), HostWorkUsage::new(requested));
            prop_assert_eq!(budget.usage(right), HostWorkUsage::ZERO);
            prop_assert!(!budget.is_rejected());
        }

        #[test]
        fn unused_capacity_cannot_subsidize_an_exhausted_dimension(
            target_index in 0..HOST_WORK_DIMENSION_COUNT,
            donor_index in 0..HOST_WORK_DIMENSION_COUNT,
            target_limit in 0_u64..1_000,
            donor_usage in 1_u64..=1_000,
        ) {
            prop_assume!(target_index != donor_index);
            let target = HostWorkDimension::ALL[target_index];
            let donor = HostWorkDimension::ALL[donor_index];
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(1_000));
            limits.set(target, HostWorkLimit::new(target_limit));
            let budget = HostWorkBudget::new(limits);
            budget.reserve(donor, HostWorkUnits::new(donor_usage)).unwrap();

            let rejected = budget.reserve(target, HostWorkUnits::new(target_limit + 1));

            let limit_exceeded = matches!(
                rejected,
                Err(HostWorkReservationError::LimitExceeded { .. })
            );
            prop_assert!(limit_exceeded);
            prop_assert_eq!(budget.usage(target), HostWorkUsage::ZERO);
            prop_assert_eq!(budget.usage(donor), HostWorkUsage::new(donor_usage));
        }

        #[test]
        fn first_failure_sticky_rejects_every_later_nonzero_dimension(
            failing_index in 0..HOST_WORK_DIMENSION_COUNT,
            limit in 0_u64..1_000,
        ) {
            let failing = HostWorkDimension::ALL[failing_index];
            let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)));

            let first = budget.reserve(failing, HostWorkUnits::new(limit + 1));
            let limit_exceeded = matches!(
                first,
                Err(HostWorkReservationError::LimitExceeded { .. })
            );
            prop_assert!(limit_exceeded);
            for dimension in HostWorkDimension::ALL {
                let sticky_rejected = matches!(
                    budget.reserve(dimension, HostWorkUnits::new(1)),
                    Err(HostWorkReservationError::BudgetRejected { .. })
                );
                prop_assert!(sticky_rejected);
                prop_assert_eq!(budget.usage(dimension), HostWorkUsage::ZERO);
            }
        }

        #[test]
        fn local_mutation_runs_only_after_successful_reservation(
            limit in 0_u64..1_000,
        ) {
            let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)));
            let mutations = AtomicUsize::new(0);

            if budget
                .reserve(
                    HostWorkDimension::SubstitutionBytes,
                    HostWorkUnits::new(limit + 1),
                )
                .is_ok()
            {
                mutations.fetch_add(1, Ordering::Relaxed);
            }

            prop_assert_eq!(mutations.load(Ordering::Relaxed), 0);
            prop_assert_eq!(
                budget.usage(HostWorkDimension::SubstitutionBytes),
                HostWorkUsage::ZERO
            );
            prop_assert!(budget.is_rejected());
        }
    }
}
