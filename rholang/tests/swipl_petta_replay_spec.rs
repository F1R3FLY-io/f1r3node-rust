use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::io::verify::par_stable_hash;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoByteArray, RhoList, RhoString};
use rholang::rust::interpreter::swi_prolog_service::petta_execute_framed;
use rholang::rust::interpreter::system_processes::{
    non_deterministic_ops, verifying_ops, BodyRefs,
};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::test_utils::utils::should_skip_petta_test;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

/// PeTTa is a deterministic, verifying system process. It is absent from
/// `non_deterministic_ops()`, so every node runs the interpreter, and it is in
/// `verifying_ops()`, so the event log records the hash of its output.
#[test]
fn test_petta_is_registered_as_deterministic() {
    assert!(
        !non_deterministic_ops().contains(&BodyRefs::SWIPL_EXECUTE_PETTA),
        "PETTA_EXECUTE must be deterministic (absent from non_deterministic_ops)"
    );
    assert!(
        verifying_ops().contains(&BodyRefs::SWIPL_EXECUTE_PETTA),
        "PETTA_EXECUTE must be a verifying op"
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
    assert_eq!(
        play_result.cost, replay_result.cost,
        "play and replay must charge the same phlo"
    );
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
    assert_eq!(
        play_result.cost, replay_result.cost,
        "play and replay must charge the same phlo"
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

/// A PeTTa program that exceeds a deterministic resource limit fails on play and
/// fails with the same canonical reason when replay re-runs it. The limits do not
/// depend on wall-clock time, so the failure is reproducible on every node.
async fn assert_same_canonical_error_on_play_and_replay(metta: &str, reason: &str) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();

    let (mut runtime, mut replay_runtime, _) = create_runtimes(store, false, &mut Vec::new()).await;

    let term = format!(
        r#"new executePetta(`rho:petta:execute`), retCh in {{ executePetta!("{}", *retCh) }}"#,
        metta
    );

    let rand = Blake2b512Random::create_from_bytes(&[]);
    let initial_phlo = Cost::create(i64::MAX, "resource limit replay test".to_string());

    let play_checkpoint = runtime.create_soft_checkpoint().await;
    let play_result = runtime
        .evaluate(&term, initial_phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("Play evaluation completed (with a resource-limit error)");

    let play_error_msg = format!("{:?}", play_result.errors);
    assert!(
        play_error_msg.contains(reason),
        "Play should fail with {}, got: {}",
        reason,
        play_error_msg
    );

    let event_log = runtime.take_event_log().await;
    replay_runtime
        .rig(event_log)
        .await
        .expect("Rig should work with a resource-limited deterministic op");

    let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;
    let replay_result = replay_runtime
        .evaluate(&term, initial_phlo.clone(), HashMap::new(), rand)
        .await
        .expect("Replay evaluation completed (with a resource-limit error)");

    let replay_error_msg = format!("{:?}", replay_result.errors);
    assert_eq!(
        play_error_msg, replay_error_msg,
        "Replay should re-run PeTTa and fail with the same canonical reason"
    );
    assert_eq!(
        play_result.cost, replay_result.cost,
        "play and replay must charge the same phlo"
    );
    assert!(
        !replay_result.errors.iter().any(|e| matches!(
            e,
            InterpreterError::CanNotReplayFailedNonDeterministicProcess
        )),
        "PeTTa is deterministic: replay must NOT short-circuit. Got: {:?}",
        replay_result.errors
    );

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}

#[tokio::test]
async fn test_petta_replay_inference_limit_error() {
    if should_skip_petta_test() {
        return;
    }
    assert_same_canonical_error_on_play_and_replay(
        "(= (nfib $n) (if (< $n 2) $n (+ (nfib (- $n 1)) (nfib (- $n 2))))) !(nfib 1000)",
        "inference_limit_exceeded",
    )
    .await;
}

/// A program that catches the injected inference-limit exception still fails:
/// `petta.sh` checks the measured inference count after the run.
#[tokio::test]
async fn test_petta_replay_caught_inference_limit_error() {
    if should_skip_petta_test() {
        return;
    }
    assert_same_canonical_error_on_play_and_replay(
        "(= (nfib $n) (if (< $n 2) $n (+ (nfib (- $n 1)) (nfib (- $n 2))))) !(catch (nfib 1000)) !(nfib 27)",
        "inference_limit_exceeded",
    )
    .await;
}

/// `atom_chars` of a 2^27-byte atom needs more than the 1 GiB stack limit in one step.
#[tokio::test]
async fn test_petta_replay_stack_limit_error() {
    if should_skip_petta_test() {
        return;
    }
    assert_same_canonical_error_on_play_and_replay(
        "(= (grow $a $n) (if (== $n 0) $a (grow (atom_concat $a $a) (- $n 1)))) !(size-atom (atom_chars (grow a 27)))",
        "stack_limit_exceeded",
    )
    .await;
}

/// Repeated squaring passes the 64 KiB `max_integer_size` limit after a few steps.
#[tokio::test]
async fn test_petta_replay_max_integer_size_error() {
    if should_skip_petta_test() {
        return;
    }
    assert_same_canonical_error_on_play_and_replay(
        "(= (sq $x $n) (if (== $n 0) $x (sq (* $x $x) (- $n 1)))) !(sq 3 40)",
        "max_integer_size_exceeded",
    )
    .await;
}

/// A doubling DAG of depth 18 is small in memory, but its serialized result passes
/// the 1 MiB output limit within the inference budget.
#[tokio::test]
async fn test_petta_replay_output_limit_error() {
    if should_skip_petta_test() {
        return;
    }
    assert_same_canonical_error_on_play_and_replay(
        "(= (dbl $x $n) (if (== $n 0) $x (dbl ($x $x) (- $n 1)))) !(dbl a 18)",
        "output_limit_exceeded",
    )
    .await;
}

/// Atoms are outside the stack limit. `atom_concat` doubling to an 8 GiB atom passes
/// the 4 GiB address-space limit, and the abort becomes a canonical reason.
#[tokio::test]
async fn test_petta_replay_memory_limit_error() {
    if should_skip_petta_test() {
        return;
    }
    assert_same_canonical_error_on_play_and_replay(
        "(= (grow $a $n) (if (== $n 0) $a (grow (atom_concat $a $a) (- $n 1)))) !(size-atom (grow a 33))",
        "memory_limit_exceeded",
    )
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
    assert_eq!(
        play_result.cost, replay_result.cost,
        "play and replay must charge the same phlo"
    );
    println!("Replay re-executed PeTTa and matched the recorded log");

    runtime.revert_to_soft_checkpoint(play_checkpoint).await;
    replay_runtime
        .revert_to_soft_checkpoint(replay_checkpoint)
        .await;
}

const PETTA_OUT_TERM: &str = r#"
    new executePetta(`rho:petta:execute`), retCh in {
        executePetta!("!(+ 1 2)", *retCh) |
        for(@r <- retCh) { @"petta-out"!(r) }
    }
"#;

fn petta_out_channel() -> Par { RhoString::create_par("petta-out".to_string()) }

async fn read_channel_data(runtime: &RhoRuntimeImpl, ch: Par) -> HashSet<Par> {
    runtime
        .get_hot_changes()
        .await
        .get(&vec![ch])
        .map(|row| row.data.iter().flat_map(|d| d.a.pars.clone()).collect())
        .unwrap_or_default()
}

fn recorded_outputs(log: &[Event]) -> Vec<Vec<Vec<u8>>> {
    log.iter()
        .filter_map(|e| match e {
            Event::IoEvent(IOEvent::Produce(p)) if !p.output_value.is_empty() => {
                Some(p.output_value.clone())
            }
            _ => None,
        })
        .collect()
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

/// Play records the 32-byte stable hash of the result and frames in the produce
/// event of the PeTTa call, not the result itself.
#[tokio::test]
async fn test_petta_play_records_output_hash() {
    if should_skip_petta_test() {
        return;
    }

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (mut runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;

    let play_result = runtime
        .evaluate(
            PETTA_OUT_TERM,
            Cost::create(i64::MAX, "hash record test".to_string()),
            HashMap::new(),
            Blake2b512Random::create_from_bytes(&[]),
        )
        .await
        .expect("Play evaluation failed");
    assert!(play_result.errors.is_empty(), "{:?}", play_result.errors);

    let (outcome, _) = petta_execute_framed("!(+ 1 2)").await;
    let (frames, result) = outcome.expect("Direct PeTTa run failed");
    assert!(frames.is_empty());
    let expected = par_stable_hash(&RhoList::create_par(vec![
        result,
        RhoList::create_par(vec![]),
    ]));

    let outputs = recorded_outputs(&runtime.take_event_log().await);
    assert_eq!(
        outputs.len(),
        1,
        "Exactly one produce must record an output"
    );
    let [recorded] = outputs[0].as_slice() else {
        panic!("The recorded output must be one Par, got {:?}", outputs[0]);
    };
    let recorded = RhoByteArray::unapply(&Par::decode(&recorded[..]).unwrap())
        .expect("The recorded output must be a byte array");
    assert_eq!(recorded, expected.to_vec());
}

/// Replay with a changed recorded hash logs a divergence on `f1r3fly.petta` and
/// still produces the fresh result.
#[tokio::test]
async fn test_petta_replay_reports_divergence_and_produces_fresh_result() {
    if should_skip_petta_test() {
        return;
    }

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (mut runtime, replay_runtime, _) = create_runtimes(store, false, &mut Vec::new()).await;

    let rand = Blake2b512Random::create_from_bytes(&[]);
    let initial_phlo = Cost::create(i64::MAX, "divergence test".to_string());

    let play_result = runtime
        .evaluate(
            PETTA_OUT_TERM,
            initial_phlo.clone(),
            HashMap::new(),
            rand.clone(),
        )
        .await
        .expect("Play evaluation failed");
    assert!(play_result.errors.is_empty(), "{:?}", play_result.errors);
    let play_out = read_channel_data(&runtime, petta_out_channel()).await;
    assert!(!play_out.is_empty());

    let tampered = vec![RhoByteArray::create_par(vec![0u8; 32]).encode_to_vec()];
    let tamper = |p: &mut Produce| {
        if !p.output_value.is_empty() {
            p.output_value = tampered.clone();
        }
    };
    let mut log = runtime.take_event_log().await;
    for event in log.iter_mut() {
        match event {
            Event::IoEvent(IOEvent::Produce(p)) => tamper(p),
            Event::Comm(comm) => comm.produces.iter_mut().for_each(tamper),
            _ => {}
        }
    }
    replay_runtime.rig(log).await.expect("Rig failed");

    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let replay_result = {
        let _guard = tracing::subscriber::set_default(subscriber);
        replay_runtime
            .evaluate(PETTA_OUT_TERM, initial_phlo, HashMap::new(), rand)
            .await
            .expect("Replay evaluation failed")
    };

    assert!(
        replay_result.errors.is_empty(),
        "{:?}",
        replay_result.errors
    );
    assert_eq!(
        read_channel_data(&replay_runtime, petta_out_channel()).await,
        play_out,
        "Replay must produce the fresh result"
    );
    let logs = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("f1r3fly.petta") && logs.contains("PeTTa replay divergence"),
        "Expected a divergence error, got: {logs}"
    );
    assert!(
        logs.contains(&format!("proposer_hash={}", "0".repeat(64))),
        "{logs}"
    );
}
