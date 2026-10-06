use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::BYTE_COST_SCHEDULE_V1;
use crate::rust::interpreter::rho_runtime::RhoRuntime;
use crate::rust::interpreter::test_utils::resources::with_runtime;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn byte_receipts_follow_actual_runtime_events_and_keep_owned_results() {
    with_runtime("byte-observation-", |mut runtime| async move {
        let result = runtime
            .evaluate_with_term(
                "new x in { x!(42) | for (@value <- x) { @\"receipt-result\"!(value) } }",
            )
            .await
            .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(result.byte_observations.has_complete_measurements());
        assert!(!result.byte_observations.rows.is_empty());
        assert!(result
            .byte_observations
            .rows
            .iter()
            .any(|row| row.kind == AuthorityByteEventKind::Comm));
        assert_eq!(
            result.byte_observations.legacy_events(),
            result.authority_byte_events
        );
        for row in &result.byte_observations.rows {
            let measured_cost = row
                .measurement
                .unwrap()
                .cost(BYTE_COST_SCHEDULE_V1)
                .unwrap();
            if let Some(amount) = row.legacy_amount {
                assert_eq!(amount, measured_cost);
            }
        }
        assert_eq!(
            result.quantitative_byte_cost,
            result
                .authority_byte_events
                .iter()
                .map(|event| event.amount)
                .sum::<u64>()
        );
        let saved = result.byte_observations.clone();
        let empty = runtime.evaluate_with_term("Nil").await.unwrap();
        assert!(empty.errors.is_empty());
        assert!(empty.byte_observations.has_complete_measurements());
        assert!(empty.byte_observations.rows.is_empty());
        assert_eq!(result.byte_observations, saved);
        let malformed = runtime.evaluate_with_term("new").await.unwrap();
        assert!(!malformed.errors.is_empty());
        assert!(!malformed.byte_observations.has_complete_measurements());
        assert!(malformed.byte_observations.rows.is_empty());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn byte_receipts_preserve_accepted_observations_on_user_abort() {
    with_runtime("byte-observation-abort-", |mut runtime| async move {
        let result = runtime
            .evaluate_with_term("new abort(`rho:execution:abort`) in { abort!(Nil) }")
            .await
            .unwrap();
        assert!(!result.errors.is_empty());
        assert!(result.byte_observations.has_complete_measurements());
        assert!(!result.byte_observations.rows.is_empty());
        assert_eq!(
            result.byte_observations.legacy_events(),
            result.authority_byte_events
        );
    })
    .await;
}
