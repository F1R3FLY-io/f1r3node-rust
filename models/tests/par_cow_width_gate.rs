//! Production representation gates for PathMap-backed `Par`.
//!
//! The retired `drive_step_width_gate` measured the generated recursive-AST
//! clone PDA. That traversal no longer exists for `Par`: cloning shares the
//! immutable `ParBag` trie root and its memoized compact snapshot. These tests
//! pin the replacement properties instead of keeping a dead PDA API alive.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};

const WORD: usize = std::mem::size_of::<usize>();

fn integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

#[test]
fn par_is_a_compact_trie_handle_not_a_repeated_field_aggregate() {
    let measured = std::mem::size_of::<Par>();
    assert!(
        measured <= 16 * WORD,
        "PathMap-backed Par widened to {measured} bytes (over sixteen machine words); \
         investigate added resident state rather than raising this budget"
    );
}

#[test]
fn clones_share_the_memoized_compact_snapshot() {
    let original = integer(42);
    let clone = original.clone();

    let original_snapshot = original.compact_snapshot();
    let clone_snapshot = clone.compact_snapshot();
    assert_eq!(original_snapshot, clone_snapshot);
    assert_eq!(
        original_snapshot.as_ptr(),
        clone_snapshot.as_ptr(),
        "an unchanged Par clone must share, not copy, its compact trie snapshot"
    );
}

#[test]
fn mutating_a_clone_detaches_without_changing_the_original() {
    let original = integer(42);
    let mut changed = original.clone();
    changed.replace_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(43)),
    }]);

    assert_eq!(original, integer(42));
    assert_eq!(changed, integer(43));
    assert_ne!(original.compact_snapshot(), changed.compact_snapshot());
}
