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

#[path = "heartbeat_additional_tests.rs"]
mod additional_tests;
