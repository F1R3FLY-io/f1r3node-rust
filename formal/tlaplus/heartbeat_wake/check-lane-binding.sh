#!/usr/bin/env bash
set -euo pipefail

baseline=${1:?dev baseline commit is required}
artifact=node/src/rust/instances/heartbeat_proposer.rs
cd "$(git rev-parse --show-toplevel)"
scratch=$(mktemp -d)
trap 'rm -r "$scratch"' EXIT
git show "$baseline:$artifact" > "$scratch/baseline.rs"

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
    cat <<'RUST'
fn main() {
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
"$scratch/lane_binding"
