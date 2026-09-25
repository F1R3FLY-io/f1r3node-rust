use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::ByteCharge;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeAttemptStage, NativeBudgetAttempt, NativeBudgetOccurrence,
};
use crate::rust::interpreter::accounting::native_runtime::NativeBudgetRetry;

fn recording(decisions: &[bool]) -> NativeBudgetRecording {
    let attempts: Vec<_> = decisions
        .iter()
        .enumerate()
        .map(|(index, granted)| NativeBudgetAttempt {
            occurrence: NativeBudgetOccurrence {
                session: [9; 32],
                path: vec![(index as u64, 0)],
                stage: NativeAttemptStage::ProduceIntroduction,
            },
            observation: Arc::new(ByteObservation {
                event_id: [(index % 3) as u8; 32],
                kind: AuthorityByteEventKind::ProduceIntroduction,
                authority: Default::default(),
                measurement: Some(ByteCharge {
                    introduction_bytes: index as u64,
                    ..Default::default()
                }),
                legacy_amount: None,
            }),
            granted: *granted,
        })
        .collect();
    let retries: Vec<_> = attempts
        .iter()
        .enumerate()
        .filter(|(_, attempt)| attempt.granted)
        .map(|(index, attempt)| NativeBudgetRetry {
            occurrence: NativeBudgetOccurrence {
                session: [9; 32],
                path: vec![(index as u64, 1)],
                stage: NativeAttemptStage::ProduceIntroduction,
            },
            observation: Arc::clone(&attempt.observation),
            accepted_attempt: index,
            fresh_before: decisions.len(),
        })
        .collect();
    NativeBudgetRecording {
        session: [9; 32],
        attempts: attempts.into(),
        retries: retries.into(),
        used: 0,
    }
}

fn host(limit: u64) -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn completed_projection_preserves_fresh_grants_order_and_multiplicity(
        decisions in prop::collection::vec(any::<bool>(), 0..100),
        cut in any::<usize>(),
    ) {
        let input = recording(&decisions);
        let output = granted_observations(&input, &host(1_000_000)).unwrap();
        let expected: Vec<_> = input.attempts.iter().filter(|attempt| attempt.granted)
            .map(|attempt| Arc::clone(&attempt.observation)).collect();
        prop_assert_eq!(&output.rows, &expected);
        prop_assert!(output.has_complete_measurements());
        prop_assert!(output.rows.len() <= decisions.len());
        for (actual, expected) in output.rows.iter().zip(&expected) {
            prop_assert!(Arc::ptr_eq(actual, expected));
        }
        let split = cut % (decisions.len() + 1);
        let mut left = input.clone();
        left.attempts = input.attempts[..split].to_vec().into();
        let mut right = input.clone();
        right.attempts = input.attempts[split..].to_vec().into();
        let mut combined = granted_observations(&left, &host(1_000_000)).unwrap().rows;
        combined.extend(granted_observations(&right, &host(1_000_000)).unwrap().rows);
        prop_assert_eq!(&combined, &expected);
        let mut reversed = input.clone();
        reversed.attempts = input.attempts.iter().cloned().rev().collect::<Vec<_>>().into();
        let reversed = granted_observations(&reversed, &host(1_000_000)).unwrap().rows;
        prop_assert!(reversed.iter().eq(expected.iter().rev()));
    }
}

#[test]
fn completed_projection_requires_prepaid_iteration_and_backing() {
    let input = recording(&[true, false, true]);
    for dimension in [
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::SearchStateBytes,
    ] {
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000));
        limits.set(dimension, HostWorkLimit::new(0));
        let budget = HostWorkBudget::new(limits);
        assert!(granted_observations(&input, &budget).is_err());
        assert!(budget.is_rejected());
        assert_eq!(input.attempts.len(), 3);
        assert_eq!(input.retries.len(), 2);
    }
}
