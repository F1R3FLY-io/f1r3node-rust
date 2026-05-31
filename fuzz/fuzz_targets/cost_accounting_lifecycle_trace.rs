//! Fuzz small runtime-budget lifecycle traces.
//!
//! Exercises reset, unmetered scope, diagnostic clearing, valid reservation,
//! invalid reservation, and OOP paths in short generated sequences.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, Sig, SourcePath, Token,
};

#[derive(Arbitrary, Debug)]
enum Step {
    Reserve { weight: u8 },
    InvalidZero,
    ForceOop { weight: u8 },
    Reset { tokens: u8 },
    Unmetered { weight: u8 },
    ClearDiagnostic,
}

#[derive(Arbitrary, Debug)]
struct Input {
    initial: u8,
    steps: Vec<Step>,
}

fn event(index: u64, weight: u64) -> BillableTokenEvent {
    BillableTokenEvent {
        deploy_id: [3; 32],
        // D0: per-signature lane key, constant within this single deploy.
        sig_hash: [0; 32],
        source_path: SourcePath(vec![index as u32]),
        redex_id: RedexId(index),
        local_index: index,
        kind: BillableKind::Comm,
        weight,
    }
}

fuzz_target!(|input: Input| {
    let budget = RuntimeBudget::new(Cost::create(i64::from(input.initial), "lifecycle"));
    let mut expected_initial = i64::from(input.initial);

    for (index, step) in input.steps.iter().take(128).enumerate() {
        match step {
            Step::Reserve { weight } => {
                let _ = budget.reserve_canonical(event(index as u64, u64::from((*weight).max(1))));
            }
            Step::InvalidZero => {
                let before = budget.cost_trace_event_count();
                assert!(budget.reserve_canonical(event(index as u64, 0)).is_err());
                assert_eq!(budget.cost_trace_event_count(), before);
            }
            Step::ForceOop { weight } => {
                let over = u64::from((*weight).max(1)) + expected_initial.max(0) as u64 + 1;
                let _ = budget.reserve_canonical(event(index as u64, over));
            }
            Step::Reset { tokens } => {
                expected_initial = i64::from(*tokens);
                budget.reset_from_token(&Token::coalesced(Sig::Unit, u64::from(*tokens)));
            }
            Step::Unmetered { weight } => {
                let before = budget.cost_trace_event_count();
                {
                    let _scope = budget.enter_unmetered_scope();
                    budget
                        .reserve_canonical(event(index as u64, u64::from((*weight).max(1))))
                        .unwrap();
                }
                assert_eq!(budget.cost_trace_event_count(), before);
            }
            Step::ClearDiagnostic => {
                let before = budget.cost_trace_digest();
                budget.clear_event_log();
                assert_eq!(budget.cost_trace_digest(), before);
            }
        }

        assert_eq!(
            budget.total_cost().value + budget.remaining().value,
            expected_initial
        );
        assert!(budget.cost_trace_event_count() <= 1_048_576);
    }
});
