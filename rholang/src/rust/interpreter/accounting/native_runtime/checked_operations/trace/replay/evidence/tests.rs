use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::ByteCharge;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeAttemptStage, NativeBudgetAttempt, NativeBudgetOccurrence, NativeDenialCause,
};
use crate::rust::interpreter::accounting::native_runtime::{
    NativeBudgetRetry, NativePathTrie, PathId,
};

fn recording(decisions: &[bool]) -> NativeBudgetRecording {
    let mut paths = NativePathTrie::new(4);
    let mut node = |segment: (u64, u64)| paths.intern(PathId::ROOT, &[segment]).expect("path fits");
    let attempt_paths = (0..decisions.len())
        .map(|index| node((index as u64, 0)))
        .collect::<Vec<_>>();
    let retry_paths = (0..decisions.len())
        .map(|index| node((index as u64, 1)))
        .collect::<Vec<_>>();
    let attempts: Vec<_> = decisions
        .iter()
        .enumerate()
        .map(|(index, granted)| NativeBudgetAttempt {
            occurrence: NativeBudgetOccurrence {
                session: [9; 32],
                path: attempt_paths[index],
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
            cause: NativeDenialCause::Budget,
            // Added by G3 (DR-135, S4).
            owner_refused: false,
            // Added by G3 (DR-135, S4 review).
            restorations: Arc::from([]),
        })
        .collect();
    let retries: Vec<_> = attempts
        .iter()
        .enumerate()
        .filter(|(_, attempt)| attempt.granted)
        .map(|(index, attempt)| NativeBudgetRetry {
            occurrence: NativeBudgetOccurrence {
                session: [9; 32],
                path: retry_paths[index],
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
        transfers: Arc::from([]),
        used: 0,
        paths: Arc::new(paths),
        // Added by G3 (DR-135, S4).
        owner_table: Arc::from([]),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn completed_projection_preserves_fresh_grants_order_and_multiplicity(
        decisions in prop::collection::vec(any::<bool>(), 0..100),
        cut in any::<usize>(),
    ) {
        let input = recording(&decisions);
        let output = granted_observations(&input);
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
        let mut combined = granted_observations(&left).rows;
        combined.extend(granted_observations(&right).rows);
        prop_assert_eq!(&combined, &expected);
        let mut reversed = input.clone();
        reversed.attempts = input.attempts.iter().cloned().rev().collect::<Vec<_>>().into();
        let reversed = granted_observations(&reversed).rows;
        prop_assert!(reversed.iter().eq(expected.iter().rev()));
    }
}
