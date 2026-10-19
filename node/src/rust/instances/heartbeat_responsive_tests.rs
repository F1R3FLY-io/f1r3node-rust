use super::*;

fn pending_lane() -> LaneInputs {
    LaneInputs {
        lfb_is_stale: false,
        lfb_lag_blocks: 2,
        has_pending_deploys: true,
        has_new_parents: false,
        has_new_parent_with_user_deploys: false,
        deploy_grace_active: false,
        self_recently_proposed: true,
        self_proposed_too_recently: true,
        cooldown_exempt_for_deploys: true,
        self_idle_for_recovery_interval: false,
        stale_recovery_interval_elapsed: false,
        cone_has_user_work: true,
        idle_recovery_window_open: false,
        lag_recovery_leader: false,
        empty_frontier_backpressure: false,
        pending_deploy_max_lag: 20,
        deploy_recovery_max_lag: 64,
        effective_frontier_chase_cap: 20,
    }
}

#[test]
fn responsive_mode_enters_refreshes_and_exits_at_the_boundary() {
    let start = Instant::now();
    let interval = Duration::from_secs(5);
    let mut mode = ResponsiveMode::default();
    assert_eq!(mode.observe(start, false, interval), (false, false));
    assert_eq!(mode.observe(start, true, interval), (true, false));
    assert_eq!(
        mode.observe(start + Duration::from_secs(9), false, interval),
        (false, false)
    );
    assert_eq!(
        mode.observe(start + Duration::from_secs(10), true, interval),
        (false, false)
    );
    assert_eq!(
        mode.observe(start + Duration::from_secs(19), false, interval),
        (false, false)
    );
    assert_eq!(
        mode.observe(start + Duration::from_secs(20), false, interval),
        (false, true)
    );
    assert!(!mode.active);
}

#[test]
fn responsive_mode_uses_the_configured_interval() {
    for milliseconds in [1, 100, 1_000, 5_000, 15_000] {
        let start = Instant::now();
        let interval = Duration::from_millis(milliseconds);
        let mut mode = ResponsiveMode::default();
        mode.observe(start, true, interval);
        mode.observe(
            start + interval * 2 - Duration::from_nanos(1),
            false,
            interval,
        );
        assert!(mode.active);
        mode.observe(start + interval * 2, false, interval);
        assert!(!mode.active);
    }
}

#[test]
fn mode_reentry_and_pending_observations_do_not_restore_credit() {
    let start = Instant::now();
    let cooldown = Duration::from_secs(10);
    let mut mode = ResponsiveMode::default();
    mode.observe(start, true, Duration::from_secs(1));
    assert!(mode.can_exempt(start, cooldown, true));
    mode.issue_exemption(start);
    mode.observe(
        start + Duration::from_secs(2),
        false,
        Duration::from_secs(1),
    );
    assert!(!mode.active);
    mode.observe(start + Duration::from_secs(3), true, Duration::from_secs(1));
    assert!(!mode.can_exempt(start + Duration::from_secs(3), cooldown, true));
    mode.observe(start + cooldown, true, Duration::from_secs(1));
    assert!(mode.can_exempt(start + cooldown, cooldown, true));
    assert!(!mode.can_exempt(start + cooldown, cooldown, false));
}

#[test]
fn every_issued_request_consumes_credit_without_a_result_dependency() {
    for _outcome in [
        "success",
        "failure",
        "busy",
        "coalesced",
        "cancelled",
        "no_new_deploys",
    ] {
        let start = Instant::now();
        let cooldown = Duration::from_secs(3);
        let mut mode = ResponsiveMode::default();
        mode.observe(start, true, Duration::from_secs(5));
        mode.issue_exemption(start);
        assert!(!mode.can_exempt(start + cooldown - Duration::from_nanos(1), cooldown, true));
        assert!(mode.can_exempt(start + cooldown, cooldown, true));
    }
}

#[test]
fn only_the_pending_lane_receives_the_exception() {
    let inputs = pending_lane();
    let decision = decide_lanes(&inputs);
    assert!(decision.pending_deploys_due && decision.pending_cooldown_exempt);
    assert!(!decision.pending_deploy_backstop_due && !decision.frontier_follow_due);
    let inactive = decide_lanes(&LaneInputs {
        cooldown_exempt_for_deploys: false,
        ..pending_lane()
    });
    assert!(!inactive.should_propose);
    let lagged = decide_lanes(&LaneInputs {
        lfb_lag_blocks: 100,
        ..pending_lane()
    });
    assert!(!lagged.pending_deploys_due && !lagged.pending_cooldown_exempt);
    let frontier = decide_lanes(&LaneInputs {
        has_pending_deploys: false,
        has_new_parents: true,
        ..pending_lane()
    });
    assert!(!frontier.frontier_follow_due && !frontier.pending_cooldown_exempt);
}

#[test]
fn responsive_credit_never_preempts_due_recovery_or_convergence() {
    let stale = decide_lanes(&LaneInputs {
        lfb_is_stale: true,
        stale_recovery_interval_elapsed: true,
        ..pending_lane()
    });
    assert!(stale.stale_lfb_recovery_due);
    assert!(!stale.pending_cooldown_exempt);
    let convergence = decide_lanes(&LaneInputs {
        idle_recovery_window_open: true,
        lag_recovery_leader: true,
        ..pending_lane()
    });
    assert!(convergence.convergence_recovery_selected);
    assert!(!convergence.pending_cooldown_exempt);
}

#[test]
fn exempt_publication_restarts_routine_cooldown_but_not_recovery_clock() {
    let start = Instant::now();
    let cooldown = Duration::from_secs(3);
    let mut mode = ResponsiveMode::default();
    mode.observe(start, true, Duration::from_secs(5));
    mode.issue_exemption(start);
    let published = start + Duration::from_secs(20);
    mode.record_routine_publication(published);
    assert_eq!(mode.cooldown_age_ms(published, Some(20_000)), Some(0));
    assert_eq!(
        mode.cooldown_age_ms(published + cooldown, Some(23_000)),
        Some(3_000)
    );
    assert!(!stale_recovery_window_is_open(100_000, Some(2_999), 3_000));
    assert!(stale_recovery_window_is_open(100_000, Some(3_000), 3_000));
}

#[test]
fn queued_exemptions_obey_the_publication_bound() {
    let start = Instant::now();
    let cooldown = Duration::from_secs(3);
    let mut mode = ResponsiveMode::default();
    mode.observe(start, true, Duration::from_secs(5));
    mode.record_routine_publication(start);
    mode.issue_exemption(start + Duration::from_secs(1));
    let first = start + Duration::from_secs(100);
    mode.record_routine_publication(first);
    let second_issue = first + Duration::from_millis(10);
    mode.observe(second_issue, true, Duration::from_secs(5));
    assert!(mode.can_exempt(second_issue, cooldown, true));
    mode.issue_exemption(second_issue);
    let second = second_issue + Duration::from_millis(10);
    mode.record_routine_publication(second);
    assert!(!mode.can_exempt(second, cooldown, true));
    assert!(mode.cooldown_age_ms(second, Some(100_000)).unwrap() < cooldown.as_millis());
}

#[test]
fn newly_observed_own_block_restores_the_clock_after_a_lost_reply() {
    let start = Instant::now();
    let first: BlockHash = vec![1; 32].into();
    let second: BlockHash = vec![2; 32].into();
    let mut mode = ResponsiveMode::default();
    mode.observe_self(&first, start);
    assert_eq!(mode.cooldown_age_ms(start, Some(50_000)), Some(50_000));
    let observed = start + Duration::from_secs(20);
    mode.observe_self(&second, observed);
    assert_eq!(mode.cooldown_age_ms(observed, Some(50_000)), Some(0));
    mode.observe_self(&second, observed + Duration::from_secs(1));
    assert_eq!(
        mode.cooldown_age_ms(observed + Duration::from_secs(1), Some(50_000)),
        Some(1_000)
    );
}

#[test]
fn responsive_credit_does_not_change_backstop_or_frontier_eligibility() {
    for credit in [false, true] {
        let backstop = decide_lanes(&LaneInputs {
            lfb_lag_blocks: 100,
            self_proposed_too_recently: false,
            self_idle_for_recovery_interval: true,
            cooldown_exempt_for_deploys: credit,
            ..pending_lane()
        });
        assert!(backstop.pending_deploy_backstop_due);
        assert!(!backstop.pending_cooldown_exempt);
        let frontier = decide_lanes(&LaneInputs {
            has_pending_deploys: false,
            has_new_parents: true,
            self_proposed_too_recently: false,
            cooldown_exempt_for_deploys: credit,
            ..pending_lane()
        });
        assert!(frontier.frontier_follow_due);
        assert!(!frontier.pending_cooldown_exempt);
    }
}

#[test]
fn bounded_timed_scenarios_check_both_request_and_publication_windows() {
    let start = Instant::now();
    let cooldown = Duration::from_millis(3);
    for delay in 0..8 {
        for pattern in 0..16 {
            let mut mode = ResponsiveMode::default();
            let mut tick = 0;
            let mut requests = Vec::new();
            let mut publications = Vec::new();
            for step in 0..80 {
                let now = start + Duration::from_millis(tick);
                let pending = (step + pattern) % 4 != 0;
                mode.observe(now, pending, Duration::from_millis(5));
                let hot = mode
                    .cooldown_age_ms(now, Some(100))
                    .is_some_and(|age| age < 3);
                let previous_empty = (step + pattern) % 3 != 0;
                let exempt = hot && pending && mode.can_exempt(now, cooldown, previous_empty);
                if !hot || exempt {
                    if exempt {
                        mode.issue_exemption(now);
                        requests.push(now);
                    }
                    tick += delay;
                    let published = start + Duration::from_millis(tick);
                    mode.record_routine_publication(published);
                    publications.push((published, !pending));
                    assert!(
                        requests
                            .iter()
                            .filter(|&&at| published.saturating_duration_since(at) < cooldown)
                            .count()
                            <= 1
                    );
                    let window: Vec<_> = publications
                        .iter()
                        .filter(|(at, _)| published.saturating_duration_since(*at) < cooldown)
                        .collect();
                    assert!(window.len() <= 2);
                    assert!(window.iter().filter(|(_, empty)| *empty).count() <= 1);
                }
                tick += 1;
            }
        }
    }
}

#[test]
fn zero_intervals_do_not_grant_an_exemption() {
    let now = Instant::now();
    let mut mode = ResponsiveMode::default();
    mode.observe(now, true, Duration::ZERO);
    assert!(!mode.can_exempt(now, Duration::ZERO, true));
    mode.observe(now, false, Duration::ZERO);
    assert!(!mode.active);
    assert_eq!(
        next_heartbeat_sleep(Duration::ZERO, None),
        Duration::from_millis(10)
    );
}

#[tokio::test]
async fn actual_heartbeat_requests_consume_credit_before_failure_or_busy() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
    for outcome in 0..3 {
        let validator = super::tests::create_test_validator_identity();
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
        own.block_hash = vec![0x62; 32].into();
        own.body.deploys.clear();
        snapshot
            .dag
            .latest_messages_map
            .insert(validator.public_key.bytes.clone(), own.block_hash.clone());
        let casper = Arc::new(TestCasperWithSnapshot::new_with_pending_deploys(
            snapshot, lfb, 1,
        ));
        casper.insert_block(&own);
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let trigger: Arc<ProposeFunction> = Arc::new(move |_, request| {
            assert!(request.is_async && request.require_user_deploys);
            observed.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                match outcome {
                    0 => Ok(ProposerResult::Empty),
                    1 => Ok(ProposerResult::Failure(
                        ProposeStatus::Failure(ProposeFailure::NoNewDeploys),
                        1,
                    )),
                    _ => Err(CasperError::Other("synthetic proposal failure".into())),
                }
            })
        });
        let config = HeartbeatConf {
            check_interval: Duration::from_secs(5),
            max_lfb_age: Duration::from_secs(60),
            self_propose_cooldown: Duration::from_secs(30),
            ..HeartbeatConf::default()
        };
        let mut progress = FinalityProgress::new(Instant::now());
        let mut mode = ResponsiveMode::default();
        let first = do_heartbeat_check_with_mode(
            casper.clone(),
            &*trigger,
            &validator,
            &config,
            false,
            false,
            &mut progress,
            Some(&mut mode),
        )
        .await;
        assert_eq!(first.is_err(), outcome == 2);
        assert!(mode.last_exempt_request_at.is_some());
        let second = do_heartbeat_check_with_mode(
            casper,
            &*trigger,
            &validator,
            &config,
            false,
            false,
            &mut progress,
            Some(&mut mode),
        )
        .await
        .unwrap();
        assert!(second.recheck_after.is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

mod additional_tests {
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
}
