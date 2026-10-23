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

/// PeTTa is a deterministic system process. It is NOT registered in
/// `non_deterministic_ops()`, so its output is not cached in the event log:
/// every node (proposer and replaying validators) runs the interpreter.
#[test]
fn test_petta_is_registered_as_deterministic() {
    let non_det_ops = non_deterministic_ops();

    assert!(
        !non_det_ops.contains(&BodyRefs::SWIPL_EXECUTE_PETTA),
        "PETTA_EXECUTE must be deterministic (absent from non_deterministic_ops)"
    );
}

/// PeTTa execution replays consistently. Because it is deterministic, replay
/// re-runs the interpreter and derives the same output and frames, which match
/// the COMMs recorded during play.
///
/// 1. Play evaluates the term and records the event log.
/// 2. The replay runtime is rigged with that log.
/// 3. Replay re-executes PeTTa and completes without errors.
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

    assert!(
        !event_log.is_empty(),
        "Event log should contain the PeTTa execution COMMs"
    );

    // 3. Rig replay runtime with event log
    replay_runtime.rig(event_log).await.expect("Rig failed");

    // 4. Execute same term in replay mode - PeTTa is re-run deterministically
    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation failed");

    assert!(
        replay_result.errors.is_empty(),
        "Replay should succeed by re-executing PeTTa deterministically: {:?}",
        replay_result.errors
    );

    println!("Play cost: {:?}", play_result.cost);
    println!("Replay cost: {:?}", replay_result.cost);
    println!("Replay re-executed PeTTa and matched the recorded log");

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
    println!("Replay re-executed multiple PeTTa calls deterministically");

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}

/// A failing PeTTa program fails deterministically. Because PeTTa is
/// deterministic, replay re-runs the interpreter and reproduces the same
/// `SwiplError` — it does NOT short-circuit with
/// `CanNotReplayFailedNonDeterministicProcess`. This is how replaying
/// validators agree on failures as well as successes.
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

    // Step 1: Play fails with a syntax error
    let play_checkpoint = runtime.create_soft_checkpoint().await;
    let play_result = runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("Play evaluation completed (with errors)");

    assert!(
        play_result
            .errors
            .iter()
            .any(|e| matches!(e, InterpreterError::SwiplError(_))),
        "Play should fail with a SwiplError for invalid MeTTa code, got: {:?}",
        play_result.errors
    );

    // Step 2: Rig replay runtime with the event log
    let event_log = runtime.take_event_log().await;
    replay_runtime
        .rig(event_log)
        .await
        .expect("Rig should work with a failed deterministic op");

    // Step 3: Replay re-executes PeTTa and reproduces the same error
    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation completed (with errors)");

    assert!(
        replay_result
            .errors
            .iter()
            .any(|e| matches!(e, InterpreterError::SwiplError(_))),
        "Replay should re-run PeTTa and reproduce the SwiplError, got: {:?}",
        replay_result.errors
    );
    assert!(
        !replay_result.errors.iter().any(|e| matches!(
            e,
            InterpreterError::CanNotReplayFailedNonDeterministicProcess
        )),
        "PeTTa is deterministic: replay must NOT short-circuit with \
         CanNotReplayFailedNonDeterministicProcess. Got: {:?}",
        replay_result.errors
    );

    println!("✓ Failing PeTTa program reproduces the same SwiplError on replay");

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}

/// A PeTTa program that exceeds the wall-clock timeout fails on play and, being
/// deterministic in intent, fails again when replay re-runs it. Note: the
/// timeout is wall-clock, so this failure mode is only reproducible when every
/// node is slower than the limit — a genuinely divergent case is rejected as an
/// invalid transaction rather than replayed from a cache.
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

    let play_error_msg = format!("{:?}", play_result.errors);
    assert!(
        play_error_msg.contains("timed out") || play_error_msg.contains("timeout"),
        "Play should time out with a large fibonacci, got: {}",
        play_error_msg
    );

    let event_log = runtime.take_event_log().await;
    replay_runtime
        .rig(event_log)
        .await
        .expect("Rig should work with a timed-out deterministic op");

    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation completed (with timeout)");

    // Replay re-runs PeTTa and times out again (no cache, no short-circuit).
    let replay_error_msg = format!("{:?}", replay_result.errors);
    assert!(
        replay_error_msg.contains("timed out") || replay_error_msg.contains("timeout"),
        "Replay should re-run PeTTa and time out again, got: {}",
        replay_error_msg
    );
    assert!(
        !replay_result.errors.iter().any(|e| matches!(
            e,
            InterpreterError::CanNotReplayFailedNonDeterministicProcess
        )),
        "PeTTa is deterministic: replay must NOT short-circuit. Got: {:?}",
        replay_result.errors
    );

    println!("✓ Timeout reproduced on replay by re-running PeTTa");

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}

/// Replay re-executes PeTTa rather than reading a cached output: the interpreter
/// runs during both play and replay, and the deterministic result matches the
/// recorded log.
#[tokio::test]
async fn test_petta_replay_re_executes() {
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
    let initial_phlo = Cost::create(i64::MAX, "replay re-exec test".to_string());

    let play_checkpoint = runtime.create_soft_checkpoint().await;
    let play_result = runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("Play evaluation failed");

    let event_log = runtime.take_event_log().await;

    assert!(
        !event_log.is_empty(),
        "Event log should contain PeTTa execution COMMs"
    );

    replay_runtime.rig(event_log).await.expect("Rig failed");

    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation failed");

    assert!(
        replay_result.errors.is_empty(),
        "Replay should succeed by re-executing PeTTa: {:?}",
        replay_result.errors
    );

    println!("Re-exec test - Play cost: {:?}", play_result.cost);
    println!("Re-exec test - Replay cost: {:?}", replay_result.cost);
    println!("Replay re-executed PeTTa and matched the recorded log");

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}
