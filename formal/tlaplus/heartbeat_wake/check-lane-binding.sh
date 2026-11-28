#!/usr/bin/env bash
set -euo pipefail

baseline=${1:?dev baseline commit is required}
artifact=node/src/rust/consensus/casper/instances/heartbeat_proposer.rs
baseline_artifact=node/src/rust/instances/heartbeat_proposer.rs
cd "$(git rev-parse --show-toplevel)"
model_config=formal/tlaplus/heartbeat_wake/HeartbeatWake.cfg
times=$(sed -nE 's/^[[:space:]]*Times[[:space:]]*=[[:space:]]*\{([0-9, ]+)\}$/\1/p' "$model_config")
minimum_recheck=$(sed -nE 's/^[[:space:]]*MinimumRecheck[[:space:]]*=[[:space:]]*([0-9]+)$/\1/p' "$model_config")
scratch=$(mktemp -d)
trap 'rm -r "$scratch"' EXIT
git show "$baseline:$baseline_artifact" > "$scratch/baseline.rs"

emit_module() {
    local module=$1 source=$2 cooldown_flag=$3
    printf 'mod %s {\n' "$module"
    sed -n -e '/^struct LaneInputs {/,/^}/p' \
        -e '/^struct LaneDecision {/,/^}/p' \
        -e '/^fn decide_lanes(/,/^}/p' "$source"
    cat <<'RUST'
    pub fn evaluate(flags: usize, limits: [i64; 3]) -> ([bool; 10], bool) {
        let active = |bit: u32| flags & (1 << bit) != 0;
        let input = LaneInputs {
            lfb_lag_blocks: 0,
            lfb_is_stale: active(0),
            has_pending_deploys: active(1),
            has_new_parents: active(2),
            has_new_parent_with_user_deploys: active(3),
            deploy_grace_active: active(4),
            self_recently_proposed: active(5),
            self_proposed_too_recently: active(6),
            self_idle_for_recovery_interval: active(7),
            stale_recovery_interval_elapsed: active(8),
            cone_has_user_work: active(9),
            idle_recovery_window_open: active(10),
            lag_recovery_leader: active(11),
            empty_frontier_backpressure: active(12),
            pending_deploy_max_lag: limits[0],
            deploy_recovery_max_lag: limits[1],
            effective_frontier_chase_cap: limits[2],
        };
        let result = decide_lanes(&input);
        let decisions = [
            result.pending_deploys_due,
            result.pending_deploy_backstop_due,
            result.frontier_follow_due,
            result.stale_lfb_recovery_due,
            result.convergence_recovery_selected,
            result.should_propose,
            result.can_propose_pending_deploys_while_ahead,
            result.can_chase_frontier_while_ahead,
            result.can_follow_frontier_without_pending_deploys,
            result.allow_frontier_follow_while_ahead_for_deploy_parent,
        ];
RUST
    printf '        (decisions, %s)\n    }\n}\n' "$cooldown_flag"
}

{
    emit_module dev "$scratch/baseline.rs" false
    emit_module candidate "$artifact" result.pending_blocked_by_cooldown
    printf 'mod timer {\n    use std::time::Duration;\n'
    sed -n -e '/^fn cooldown_recheck_delay(/,/^}/p' \
        -e '/^fn next_heartbeat_sleep(/,/^}/p' "$artifact"
    cat <<'RUST'
    pub fn evaluate(
        blocked: bool,
        cooldown: u64,
        age: Option<u64>,
        interval: u64,
    ) -> (Option<u128>, u128) {
        let recheck = cooldown_recheck_delay(blocked, cooldown.into(), age.map(u128::from));
        let sleep = next_heartbeat_sleep(Duration::from_millis(interval), recheck);
        (recheck.map(|delay| delay.as_millis()), sleep.as_millis())
    }
}

fn check_cooldown_cases(times: &[u64], minimum_recheck: u64) {
    let mut rows = 0;
    for blocked in [false, true] {
        for &cooldown in times {
            for age in std::iter::once(None).chain(times.iter().copied().map(Some)) {
                let remaining = age.and_then(|age| cooldown.checked_sub(age));
                let expected_delay = remaining.filter(|&remaining| blocked && remaining > 0);
                for &interval in times {
                    let expected_sleep = expected_delay
                        .map(|remaining| interval.min(minimum_recheck.max(remaining)))
                        .unwrap_or(interval);
                    assert_eq!(
                        timer::evaluate(blocked, cooldown, age, interval),
                        (expected_delay.map(u128::from), u128::from(expected_sleep)),
                        "blocked={blocked}, cooldown={cooldown}, age={age:?}, interval={interval}",
                    );
                    rows += 1;
                }
            }
        }
    }
    println!("{rows} heartbeat cooldown timing cases passed");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let times: Vec<u64> = args.next().expect("model times").split(',')
        .map(|time| time.trim().parse().expect("numeric model time"))
        .collect();
    let minimum_recheck: u64 = args.next().expect("model recheck floor")
        .parse().expect("numeric model recheck floor");
    check_cooldown_cases(&times, minimum_recheck);

    const COOLDOWN: usize = 1 << 6;
    let mut rows = 0;
    for flags in 0..(1 << 13) {
        for pending_lag in -1..=1 {
            for recovery_lag in -1..=1 {
                for frontier_lag in -1..=1 {
                    let limits = [pending_lag, recovery_lag, frontier_lag];
                    let (actual, waits_for_cooldown) = candidate::evaluate(flags, limits);
                    let (expected, _) = dev::evaluate(flags, limits);
                    let (allowed_after_cooldown, _) = dev::evaluate(flags & !COOLDOWN, limits);
                    assert_eq!(actual, expected, "flags={flags}, limits={limits:?}");
                    assert_eq!(
                        waits_for_cooldown,
                        flags & COOLDOWN != 0 && allowed_after_cooldown[0],
                        "flags={flags}, limits={limits:?}",
                    );
                    rows += 1;
                }
            }
        }
    }
    println!("{rows} heartbeat lane decision cases passed");
}
RUST
} > "$scratch/lane_binding.rs"
rustc --edition 2021 "$scratch/lane_binding.rs" -o "$scratch/lane_binding"
"$scratch/lane_binding" "$times" "$minimum_recheck"
