use std::sync::atomic::{AtomicUsize, Ordering};

use casper::rust::casper::test_helpers::TestCasperWithSnapshot;

use super::*;

fn pending_fixture() -> (
    Arc<TestCasperWithSnapshot>,
    ValidatorIdentity,
    HeartbeatConf,
) {
    let validator = super::super::tests::create_test_validator_identity();
    let mut snapshot = TestCasperWithSnapshot::create_empty_snapshot();
    TestCasperWithSnapshot::bond_validator_in_snapshot(
        &mut snapshot,
        validator.public_key.bytes.clone(),
    );
    let mut lfb = models::rust::block_implicits::get_random_block_default();
    lfb.header.timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let mut own = lfb.clone();
    own.block_hash = vec![0x63; 32].into();
    own.body.deploys.clear();
    snapshot
        .dag
        .latest_messages_map
        .insert(validator.public_key.bytes.clone(), own.block_hash.clone());
    let casper = Arc::new(TestCasperWithSnapshot::new_with_pending_deploys(
        snapshot, lfb, 1,
    ));
    casper.insert_block(&own);
    let config = HeartbeatConf {
        check_interval: Duration::from_secs(5),
        max_lfb_age: Duration::from_secs(3600),
        self_propose_cooldown: Duration::from_secs(3600),
        ..HeartbeatConf::default()
    };
    (casper, validator, config)
}

#[tokio::test]
async fn cancelling_an_issued_heartbeat_keeps_its_exemption_credit_consumed() {
    let (casper, validator, config) = pending_fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let (issued_tx, mut issued_rx) = tokio::sync::mpsc::unbounded_channel();
    let trigger: Arc<ProposeFunction> = Arc::new(move |_, request| {
        observed.fetch_add(1, Ordering::SeqCst);
        issued_tx.send(request).unwrap();
        Box::pin(std::future::pending::<Result<ProposerResult, CasperError>>())
    });
    let mut progress = FinalityProgress::new(Instant::now());
    let mut mode = ResponsiveMode::default();
    let mut attempt = Box::pin(do_heartbeat_check_with_mode(
        casper.clone(),
        &*trigger,
        &validator,
        &config,
        false,
        false,
        &mut progress,
        Some(&mut mode),
    ));
    tokio::select! {
        request = issued_rx.recv() => {
            let request = request.unwrap();
            assert!(request.require_user_deploys);
            assert_eq!(request.expected_empty_self, Some(vec![0x63; 32].into()));
        }
        _ = &mut attempt => panic!("The issued request must remain pending until cancellation"),
    }
    drop(attempt);
    let reserved = mode.last_exempt_request_at.unwrap();
    assert!(!mode.can_exempt(Instant::now(), config.self_propose_cooldown, true));
    let next = tokio::time::timeout(
        Duration::from_secs(5),
        do_heartbeat_check_with_mode(
            casper,
            &*trigger,
            &validator,
            &config,
            false,
            false,
            &mut progress,
            Some(&mut mode),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(next.recheck_after.is_some());
    assert_eq!(mode.last_exempt_request_at, Some(reserved));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn delayed_request_completion_keeps_the_guard_and_does_not_refund_credit() {
    let (casper, validator, config) = pending_fixture();
    let (queued_tx, mut queued_rx) = tokio::sync::mpsc::channel(1);
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let reply_slot = Arc::new(std::sync::Mutex::new(Some(reply_rx)));
    let trigger: Arc<ProposeFunction> = Arc::new(move |_, request| {
        let tx = queued_tx.clone();
        let reply = reply_slot.lock().unwrap().take().unwrap();
        Box::pin(async move {
            tx.send(request)
                .await
                .map_err(|_| CasperError::Other("synthetic queue closed".into()))?;
            reply
                .await
                .map_err(|_| CasperError::Other("synthetic reply closed".into()))
        })
    });
    let mut progress = FinalityProgress::new(Instant::now());
    let mut mode = ResponsiveMode::default();
    let mut attempt = Box::pin(do_heartbeat_check_with_mode(
        casper,
        &*trigger,
        &validator,
        &config,
        false,
        false,
        &mut progress,
        Some(&mut mode),
    ));
    let request = tokio::select! {
        request = queued_rx.recv() => request.unwrap(),
        _ = &mut attempt => panic!("The request must await the queued result"),
    };
    assert!(request.is_async && request.require_user_deploys);
    assert_eq!(request.expected_empty_self, Some(vec![0x63; 32].into()));
    tokio::task::yield_now().await;
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(attempt.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(reply_tx
        .send(ProposerResult::Failure(
            ProposeStatus::Failure(ProposeFailure::NoNewDeploys),
            1,
        ))
        .is_ok());
    attempt.await.unwrap();
    assert!(mode.last_exempt_request_at.is_some());
    assert!(!mode.can_exempt(Instant::now(), config.self_propose_cooldown, true));
}

fn rolling_count(events: &[Instant], now: Instant, cooldown: Duration) -> usize {
    events
        .iter()
        .filter(|&&at| at <= now && now.duration_since(at) < cooldown)
        .count()
}

#[test]
fn a_credit_reset_negative_control_violates_the_rolling_request_bound() {
    let start = Instant::now();
    let cooldown = Duration::from_millis(3);
    let next = start + Duration::from_millis(1);
    let mut mode = ResponsiveMode::default();
    mode.observe(start, true, Duration::from_secs(5));
    mode.issue_exemption(start);
    assert!(!mode.can_exempt(next, cooldown, true));
    assert_eq!(rolling_count(&[start], next, cooldown), 1);
    mode.last_exempt_request_at = None;
    assert!(mode.can_exempt(next, cooldown, true));
    mode.issue_exemption(next);
    assert!(rolling_count(&[start, next], next, cooldown) > 1);
}

#[test]
fn a_lost_publication_clock_negative_control_violates_both_publication_bounds() {
    let start = Instant::now();
    let exempt = start + Duration::from_millis(1);
    let third = start + Duration::from_millis(2);
    let cooldown = Duration::from_millis(3);
    let mut mode = ResponsiveMode::default();
    mode.observe(start, true, Duration::from_secs(5));
    mode.record_routine_publication(start);
    mode.issue_exemption(exempt);
    mode.record_routine_publication(exempt);
    assert!(mode.cooldown_age_ms(third, Some(100)).unwrap() < cooldown.as_millis());
    assert!(!mode.can_exempt(third, cooldown, true));
    mode.last_routine_publication_at = None;
    assert!(mode.cooldown_age_ms(third, Some(100)).unwrap() >= cooldown.as_millis());
    assert!(rolling_count(&[start, exempt, third], third, cooldown) > 2);
    assert!(rolling_count(&[start, third], third, cooldown) > 1);
}
