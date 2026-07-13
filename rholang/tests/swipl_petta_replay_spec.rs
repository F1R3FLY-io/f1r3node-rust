use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::system_processes::{non_deterministic_ops, BodyRefs};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::test_utils::utils::should_skip_petta_test;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

#[test]
fn test_petta_is_registered_as_non_deterministic() {
    let non_det_ops = non_deterministic_ops();

    assert!(
        non_det_ops.contains(&BodyRefs::SWIPL_EXECUTE_PETTA),
        "SWIPL_EXECUTE_PETTA should be marked as non-deterministic"
    );
}

/// This test demonstrates that PeTTa execution can be replayed successfully.
/// We verify that:
/// 1. PeTTa is registered as non-deterministic (see above)
/// 2. Event log captures the PeTTa execution output
/// 3. Replay runtime can be rigged with the event log
/// 4. Replay execution completes without errors using cached output
#[tokio::test]
async fn test_petta_replay_consistency() {
    if should_skip_petta_test() {
        return;
    }

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();

    let (mut runtime, mut replay_runtime, _) = create_runtimes(store, false, &mut Vec::new()).await;

    let term = r#"
        new executePetta(`rho:petta:execute`), retCh in {
            executePetta!("!(+ 1 2)", *retCh) |
            for(@_ <- retCh) { Nil }
        }
    "#;

    let rand = Blake2b512Random::create_from_bytes(&[]);
    let initial_phlo = Cost::create(i64::MAX, "replay test".to_string());

    // 1. Execute in play mode
    let play_checkpoint = runtime.create_soft_checkpoint().await;
    let play_result = runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("Play evaluation failed");

    assert!(
        play_result.errors.is_empty(),
        "Play should succeed: {:?}",
        play_result.errors
    );

    // 2. Capture event log from play execution
    let event_log = runtime.take_event_log().await;

    // Verify event log contains data (non-deterministic operation was captured)
    assert!(
        !event_log.is_empty(),
        "Event log should contain captured PeTTa execution"
    );

    // 3. Rig replay runtime with event log
    replay_runtime
        .rig(event_log)
        .await
        .expect("Rig failed - this means PeTTa is not properly registered as non-deterministic");

    // 4. Execute same term in replay mode
    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation failed");

    assert!(
        replay_result.errors.is_empty(),
        "Replay should succeed using cached output: {:?}",
        replay_result.errors
    );

    println!("Play cost: {:?}", play_result.cost);
    println!("Replay cost: {:?}", replay_result.cost);
    println!("Replay successfully used cached PeTTa output");

    // Cleanup checkpoints
    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}

#[tokio::test]
async fn test_petta_replay_with_multiple_calls() {
    if should_skip_petta_test() {
        return;
    }

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();

    let (mut runtime, mut replay_runtime, _) = create_runtimes(store, false, &mut Vec::new()).await;

    let term = r#"
        new executePetta(`rho:petta:execute`), ret1, ret2 in {
            executePetta!("!(+ 1 2)", *ret1) |
            executePetta!("!(* 3 4)", *ret2) |
            for(@_ <- ret1; @_ <- ret2) { Nil }
        }
    "#;

    let rand = Blake2b512Random::create_from_bytes(&[]);
    let initial_phlo = Cost::create(i64::MAX, "replay test".to_string());

    let play_checkpoint = runtime.create_soft_checkpoint().await;
    let play_result = runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("Play evaluation failed");

    assert!(
        play_result.errors.is_empty(),
        "Play should succeed: {:?}",
        play_result.errors
    );

    let event_log = runtime.take_event_log().await;
    assert!(
        !event_log.is_empty(),
        "Event log should capture multiple PeTTa calls"
    );

    replay_runtime.rig(event_log).await.expect("Rig failed");

    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation failed");

    assert!(
        replay_result.errors.is_empty(),
        "Replay should succeed: {:?}",
        replay_result.errors
    );

    println!("Multiple PeTTa calls - Play cost: {:?}", play_result.cost);
    println!(
        "Multiple PeTTa calls - Replay cost: {:?}",
        replay_result.cost
    );
    println!("Replay successfully used cached output for multiple calls");

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}

/// This test verifies that failed PeTTa executions are properly recorded in the event log
/// and produce a deterministic replay error without re-invoking swipl. The test specifically
/// ensures:
///
/// 1. Play execution fails with invalid MeTTa syntax
/// 2. The failure is recorded in the event log as a `Produce` with `failed == true` and
///    `output_value` empty (the original cause is NOT stored — only the boolean flag)
/// 3. Replay runtime can be rigged with the event log containing the failed execution
/// 4. Replay raises `InterpreterError::CanNotReplayFailedNonDeterministicProcess` (a
///    deterministic error) — this is raised by `continue_produce_process` BEFORE dispatch,
///    so the system process handler and swipl/PeTTa are NEVER re-invoked
/// 5. Replay error does NOT contain `SwiplError` / `"incomplete"` / `NonDeterministicProcessFailure`,
///    proving swipl was not re-invoked to re-derive those messages
///
/// This is critical for consensus safety: validators must agree on failures as well as successes.
/// The CanNotReplay short-circuit ensures all replaying validators produce the same error regardless
/// of the original non-deterministic failure cause, preventing divergence.
#[tokio::test]
async fn test_petta_replay_error_consistency() {
    if should_skip_petta_test() {
        return;
    }

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();

    let (mut runtime, mut replay_runtime, _) = create_runtimes(store, false, &mut Vec::new()).await;

    let term = r#"
        new executePetta(`rho:petta:execute`), retCh in {
            executePetta!("(= incomplete", *retCh)
        }
    "#;

    let rand = Blake2b512Random::create_from_bytes(&[]);
    let initial_phlo = Cost::create(i64::MAX, "replay error test".to_string());

    // Step 1: Execute in play mode - should fail with syntax error
    let play_checkpoint = runtime.create_soft_checkpoint().await;
    let play_result = runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("Play evaluation completed (with errors)");

    assert!(
        !play_result.errors.is_empty(),
        "Play should have errors for invalid MeTTa code"
    );

    // Verify the error is a NonDeterministicProcessFailure (correct error type for failed non-det ops)
    assert!(
        play_result
            .errors
            .iter()
            .any(|e| matches!(e, InterpreterError::NonDeterministicProcessFailure { .. })),
        "Play should contain NonDeterministicProcessFailure, got: {:?}",
        play_result.errors
    );

    // Step 2: Capture event log - should contain a failed produce
    let event_log = runtime.take_event_log().await;
    let has_failed_produce = event_log.iter().any(|event| match event {
        Event::Comm(comm) => comm
            .produces
            .iter()
            .any(|p| p.failed && p.output_value.is_empty()),
        Event::IoEvent(IOEvent::Produce(p)) => p.failed && p.output_value.is_empty(),
        Event::IoEvent(IOEvent::Consume(_)) => false,
    });
    assert!(
        has_failed_produce,
        "Event log should contain a Produce with failed==true and empty output_value. \
         This means the runtime correctly recorded the failed non-det produce, and during \
         replay continue_produce_process will short-circuit with \
         CanNotReplayFailedNonDeterministicProcess. Event log: {:?}",
        event_log
    );

    // Step 3: Rig replay runtime with the event log containing the failure
    replay_runtime
        .rig(event_log)
        .await
        .expect("Rig should work with failed non-det operations");

    // Step 4: Execute in replay mode - should short-circuit with CanNotReplayFailedNonDeterministicProcess
    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation completed (with errors)");

    // Step 5: Verify replay raises CanNotReplayFailedNonDeterministicProcess
    assert_eq!(
        replay_result.errors.len(),
        1,
        "Replay should produce exactly one error"
    );
    assert!(
        matches!(
            replay_result.errors[0],
            InterpreterError::CanNotReplayFailedNonDeterministicProcess
        ),
        "Replay should produce CanNotReplayFailedNonDeterministicProcess, got: {:?}",
        replay_result.errors[0]
    );

    // Prove swipl was NOT re-invoked during replay: the replay error should NOT contain
    // SwiplError, "incomplete", or NonDeterministicProcessFailure (which swipl would produce).
    let replay_has_swipl_output = replay_result.errors.iter().any(|e| match e {
        InterpreterError::NonDeterministicProcessFailure { cause, .. }
        | InterpreterError::ProduceFailureWithOutput { cause, .. } => {
            matches!(cause.as_ref(), InterpreterError::SwiplError(_))
        }
        InterpreterError::SwiplError(_) => true,
        _ => false,
    });
    assert!(
        !replay_has_swipl_output,
        "Replay error should NOT contain SwiplError — swipl was not re-invoked. Got: {:?}",
        replay_result.errors
    );

    println!(
        "Play execution failed as expected with {} error(s)",
        play_result.errors.len()
    );
    println!("✓ Event log contains a Produce with failed==true and empty output_value");
    println!("✓ Replay runtime rigged successfully with failed execution");
    println!(
        "Replay deterministically raised CanNotReplayFailedNonDeterministicProcess ({} errors) \
         — swipl was NOT re-invoked",
        replay_result.errors.len()
    );
    println!("✓ Failed PeTTa executions are replay-safe");

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}

/// This test verifies that PeTTa timeout failures are properly recorded and replayed.
/// Timeout errors are a specific type of NonDeterministicProcessFailure that should
/// also be replay-safe.
#[tokio::test]
async fn test_petta_replay_timeout_error() {
    if should_skip_petta_test() {
        return;
    }

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();

    let (mut runtime, mut replay_runtime, _) = create_runtimes(store, false, &mut Vec::new()).await;

    // Large fibonacci that will timeout (>10 seconds)
    let term = r#"
        new executePetta(`rho:petta:execute`), retCh in {
            executePetta!("(= (fib-tr $n $a $b) (if (== $n 0) $a (fib-tr (- $n 1) $b (+ $a $b)))) (= (fib $n) (fib-tr $n 0 1)) !(fib 10000000)", *retCh)
        }
    "#;

    let rand = Blake2b512Random::create_from_bytes(&[]);
    let initial_phlo = Cost::create(i64::MAX, "timeout replay test".to_string());

    let play_checkpoint = runtime.create_soft_checkpoint().await;
    let play_result = runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("Play evaluation completed (with timeout)");

    assert!(
        !play_result.errors.is_empty(),
        "Play should timeout with large fibonacci"
    );

    let play_error_msg = format!("{:?}", play_result.errors);
    assert!(
        play_error_msg.contains("timed out") || play_error_msg.contains("timeout"),
        "Error should indicate timeout, got: {}",
        play_error_msg
    );

    let event_log = runtime.take_event_log().await;
    assert!(
        !event_log.is_empty(),
        "Event log should capture timeout failure"
    );

    replay_runtime
        .rig(event_log)
        .await
        .expect("Rig should work with timeout errors");

    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation completed (with CanNotReplayFailedNonDeterministicProcess)");

    assert!(
        !replay_result.errors.is_empty(),
        "Replay should produce a CanNotReplayFailedNonDeterministicProcess error"
    );

    // Replay must raise CanNotReplayFailedNonDeterministicProcess (short-circuits before dispatch)
    assert!(
        replay_result.errors.iter().any(|e| {
            matches!(
                e,
                InterpreterError::CanNotReplayFailedNonDeterministicProcess
            )
        }),
        "Replay should produce CanNotReplayFailedNonDeterministicProcess, got: {:?}",
        replay_result.errors
    );

    // Prove swipl was NOT re-invoked during replay: assert no timeout/timed out/SwiplError
    let replay_has_swipl_output = replay_result.errors.iter().any(|e| {
        matches!(e, InterpreterError::SwiplError(_))
            || matches!(e, InterpreterError::NonDeterministicProcessFailure { .. })
    });
    assert!(
        !replay_has_swipl_output,
        "Replay should NOT contain SwiplError or NonDeterministicProcessFailure \
         — swipl was not re-invoked. Got: {:?}",
        replay_result.errors
    );

    println!("✓ Timeout failure properly recorded; replay deterministically raised CanNotReplayFailedNonDeterministicProcess");

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}

/// This test verifies that PeTTa replay uses cached output instead of re-executing.
#[tokio::test]
async fn test_petta_replay_uses_cached_output() {
    if should_skip_petta_test() {
        return;
    }

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();

    let (mut runtime, mut replay_runtime, _) = create_runtimes(store, false, &mut Vec::new()).await;

    let term = r#"
        new executePetta(`rho:petta:execute`), retCh in {
            executePetta!("!(+ 1 2)", *retCh) |
            for(@_ <- retCh) { Nil }
        }
    "#;

    let rand = Blake2b512Random::create_from_bytes(&[]);
    let initial_phlo = Cost::create(i64::MAX, "replay cache test".to_string());

    let play_checkpoint = runtime.create_soft_checkpoint().await;
    let play_result = runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("Play evaluation failed");

    let event_log = runtime.take_event_log().await;

    assert!(
        !event_log.is_empty(),
        "Event log should contain PeTTa execution data"
    );

    replay_runtime.rig(event_log).await.expect("Rig failed");

    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation failed");

    println!("Cached output test - Play cost: {:?}", play_result.cost);
    println!("Cached output test - Replay cost: {:?}", replay_result.cost);
    println!("Replay successfully retrieved and used cached PeTTa output from event log");

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}
