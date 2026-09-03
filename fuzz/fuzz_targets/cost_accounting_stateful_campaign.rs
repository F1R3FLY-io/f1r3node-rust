//! Fuzz stateful cost-accounting campaigns.
//!
//! This target drives reset, reservation, OOP, diagnostic clearing, and
//! unmetered scopes through the production `RuntimeBudget` so generated v3
//! campaigns have a production-path oracle rather than a shadow-only model.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rholang::rust::interpreter::accounting::{Sig, Token};

mod cost_accounting_fuzz_support;

#[derive(Arbitrary, Debug)]
enum Step {
    Reserve { weight: u8, tag: u8 },
    ForceOop { weight: u8, tag: u8 },
    InvalidZero { tag: u8 },
    Reset { tokens: u8 },
    ClearDiagnostic,
    Unmetered { weight: u8, tag: u8 },
}

#[derive(Arbitrary, Debug)]
struct Input {
    initial: u8,
    steps: Vec<Step>,
}

fuzz_target!(|input: Input| {
    let budget =
        cost_accounting_fuzz_support::runtime_budget(u16::from(input.initial), "stateful");
    let mut expected_initial = i64::from(input.initial);

    for (index, step) in input.steps.iter().take(128).enumerate() {
        match step {
            Step::Reserve { weight, tag } => {
                let event = cost_accounting_fuzz_support::billable_event(
                    index as u64,
                    *tag,
                    u64::from((*weight).max(1)),
                    8,
                    1,
                );
                let _ = budget.reserve_canonical(event);
            }
            Step::ForceOop { weight, tag } => {
                let over = expected_initial.max(0) as u64 + u64::from((*weight).max(1)) + 1;
                let event = cost_accounting_fuzz_support::billable_event(
                    index as u64,
                    *tag,
                    over,
                    8,
                    1,
                );
                let _ = budget.reserve_canonical(event);
            }
            Step::InvalidZero { tag } => {
                let before_cost = budget.total_cost().value;
                let before_remaining = budget.remaining().value;
                let before_count = budget.cost_trace_event_count();
                let event =
                    cost_accounting_fuzz_support::billable_event(index as u64, *tag, 0, 8, 1);
                assert!(budget.reserve_canonical(event).is_err());
                assert_eq!(budget.total_cost().value, before_cost);
                assert_eq!(budget.remaining().value, before_remaining);
                assert_eq!(budget.cost_trace_event_count(), before_count);
            }
            Step::Reset { tokens } => {
                expected_initial = i64::from(*tokens);
                budget.reset_from_token(&Token::coalesced(Sig::Unit, u64::from(*tokens)));
            }
            Step::ClearDiagnostic => {
                let before = budget.cost_trace_digest();
                budget.clear_event_log();
                assert_eq!(budget.cost_trace_digest(), before);
            }
            Step::Unmetered { weight, tag } => {
                let before = budget.cost_trace_event_count();
                {
                    let _scope = budget.enter_unmetered_scope();
                    let event = cost_accounting_fuzz_support::billable_event(
                        index as u64,
                        *tag,
                        u64::from((*weight).max(1)),
                        8,
                        1,
                    );
                    budget.reserve_canonical(event).unwrap();
                }
                assert_eq!(budget.cost_trace_event_count(), before);
            }
        }

        assert_eq!(
            budget.total_cost().value + budget.remaining().value,
            expected_initial
        );
        assert!(budget.total_cost().value >= 0);
        assert!(budget.remaining().value >= 0);
    }
});
