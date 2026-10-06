use proptest::prelude::*;

use super::*;

fn count(value: usize) -> NonZeroUsize { NonZeroUsize::new(value).unwrap() }

#[test]
fn cursor_rejects_negative_and_out_of_cohort_native_values() {
    assert_eq!(
        MonetaryCursor::new(-1, 0, count(2)),
        Err(MonetaryCursorError::InvalidRevision)
    );
    for position in [i64::MIN, -1, 2, i64::MAX] {
        assert_eq!(
            MonetaryCursor::new(0, position, count(2)),
            Err(MonetaryCursorError::InvalidPosition)
        );
    }
    let cursor = MonetaryCursor::new(9, 3, count(4)).unwrap();
    assert_eq!(cursor.position_index(count(4)), Ok(3));
    assert_eq!(
        cursor.validate(count(3)),
        Err(MonetaryCursorError::InvalidPosition)
    );
}

#[test]
fn cursor_revision_advances_when_position_does_not_change() {
    let scope = [1; 32];
    let transition =
        MonetaryCursorTransition::new(scope, MonetaryCursor::INITIAL, 0, count(1)).unwrap();
    let next = transition
        .checked_successor(&scope, MonetaryCursor::INITIAL, count(1))
        .unwrap();
    assert_eq!(next.position(), 0);
    assert_eq!(next.revision(), 1);
    assert_eq!(
        transition.checked_successor(&scope, next, count(1)),
        Err(MonetaryCursorError::StaleTransition)
    );
}

#[test]
fn cursor_revalidation_checks_the_next_position_after_a_count_change() {
    let scope = [1; 32];
    let current = MonetaryCursor::INITIAL;
    let transition = MonetaryCursorTransition::new(scope, current, 1, count(2)).unwrap();
    assert_eq!(current.validate(count(1)), Ok(()));
    assert_eq!(
        transition.checked_successor(&scope, current, count(1)),
        Err(MonetaryCursorError::InvalidPosition)
    );
}

#[test]
fn cursor_full_rotation_does_not_revalidate_an_old_transition() {
    let scope = [2; 32];
    for arity in 1..=65 {
        let payers = count(arity);
        let original = MonetaryCursorTransition::new(
            scope,
            MonetaryCursor::INITIAL,
            (1 % arity) as i64,
            payers,
        )
        .unwrap();
        let mut cursor = MonetaryCursor::INITIAL;
        for next in 1..=arity {
            cursor = MonetaryCursorTransition::new(scope, cursor, (next % arity) as i64, payers)
                .unwrap()
                .checked_successor(&scope, cursor, payers)
                .unwrap();
        }
        assert_eq!(cursor.position(), 0);
        assert_eq!(cursor.revision(), arity as i64);
        assert_eq!(
            original.checked_successor(&scope, cursor, payers),
            Err(MonetaryCursorError::StaleTransition)
        );
    }
}

#[test]
fn cursor_last_revision_is_readable_but_cannot_settle_again() {
    let scope = [3; 32];
    let before = MonetaryCursor::new(i64::MAX - 1, 0, count(1)).unwrap();
    let transition = MonetaryCursorTransition::new(scope, before, 0, count(1)).unwrap();
    let exhausted = transition
        .checked_successor(&scope, before, count(1))
        .unwrap();
    assert_eq!(exhausted.revision(), i64::MAX);
    assert_eq!(exhausted.validate(count(1)), Ok(()));
    assert_eq!(
        MonetaryCursorTransition::new(scope, exhausted, 0, count(1)),
        Err(MonetaryCursorError::RevisionExhausted)
    );
}

#[test]
fn cursor_parts_cannot_skip_or_reuse_a_revision() {
    let scope = [4; 32];
    let before = MonetaryCursor::new(2, 0, count(2)).unwrap();
    for revision in [0, 1, 2, 4, i64::MAX] {
        let next = MonetaryCursor::new(revision, 1, count(2)).unwrap();
        assert_eq!(
            MonetaryCursorTransition::from_parts(scope, before, next, count(2)),
            Err(MonetaryCursorError::InvalidSuccessor)
        );
    }
    assert_eq!(
        MonetaryCursorTransition::new(scope, before, -1, count(2)),
        Err(MonetaryCursorError::InvalidPosition)
    );
}

#[test]
fn cursor_exhaustive_small_states_match_the_formal_transition_guards() {
    let payers = count(2);
    for scope in 0..2 {
        for target in 0..2 {
            for expected_revision in 0..=2 {
                for expected_position in 0..2 {
                    for next_position in 0..2 {
                        let expected =
                            MonetaryCursor::new(expected_revision, expected_position, payers)
                                .unwrap();
                        let transition = MonetaryCursorTransition::new(
                            [scope; 32],
                            expected,
                            next_position,
                            payers,
                        )
                        .unwrap();
                        for current_revision in 0..=2 {
                            for current_position in 0..2 {
                                let current =
                                    MonetaryCursor::new(current_revision, current_position, payers)
                                        .unwrap();
                                let result =
                                    transition.checked_successor(&[target; 32], current, payers);
                                let valid = scope == target
                                    && expected_revision == current_revision
                                    && expected_position == current_position;
                                assert_eq!(result.is_ok(), valid);
                                if let Ok(after) = result {
                                    assert_eq!(after.revision(), current_revision + 1);
                                    assert_eq!(after.position(), next_position);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn cursor_interleaved_plans_preserve_scope_revisions_and_abort_state(
        arities in prop::array::uniform3(1_usize..=65),
        operations in prop::collection::vec((0_usize..2, 0_usize..3, 0_u8..4, any::<u8>()), 1..96),
    ) {
        let mut cursors = [MonetaryCursor::INITIAL; 3];
        let mut planned = [None::<MonetaryCursorTransition>; 2];
        let mut committed = [0_i64; 3];
        for (worker, scope, action, selection) in operations {
            let before = cursors;
            let payers = count(arities[scope]);
            match action {
                0 => {
                    let position = (usize::from(selection) % arities[scope]) as i64;
                    planned[worker] = Some(MonetaryCursorTransition::new(
                        [scope as u8; 32], cursors[scope], position, payers,
                    ).unwrap());
                    prop_assert_eq!(cursors, before);
                }
                1 | 2 => {
                    if let Some(plan) = planned[worker] {
                        let result = plan.checked_successor(&[scope as u8; 32], cursors[scope], payers);
                        let valid = plan.scope() == &[scope as u8; 32]
                            && plan.expected() == cursors[scope];
                        prop_assert_eq!(result.is_ok(), valid);
                        if let Ok(next) = result {
                            cursors[scope] = next;
                            committed[scope] += 1;
                        }
                        for other in 0..3 {
                            if other != scope {
                                prop_assert_eq!(cursors[other], before[other]);
                            }
                        }
                        if action == 2 {
                            planned[worker] = None;
                        }
                    }
                }
                _ => {
                    planned[worker] = None;
                    prop_assert_eq!(cursors, before);
                }
            }
            for scope in 0..3 {
                prop_assert_eq!(cursors[scope].revision(), committed[scope]);
                prop_assert!(cursors[scope].validate(count(arities[scope])).is_ok());
            }
        }
    }
}
