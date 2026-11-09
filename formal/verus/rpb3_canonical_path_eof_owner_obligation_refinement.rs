//! Focused-owner shape at a successful CanonicalPath root EOF value.
//!
//! `CanonicalPathStreamReducer::value` selects the outermost saved owner when
//! focus is active. `focus_path` saves that owner while its stack contains a
//! Region frame. `finish_path_with_pending` rejects every nonempty stack
//! before it can return a summary. Thus a successful value has no focused
//! owner and the selected current path has no stack. The reducer's
//! `active_obligation_in` begins by reading that same current stack; with no
//! stack, it cannot reach an Obligation frame.
//!
//! The retained-owner-stack predicate is a structural invariant of the
//! focused reducer, not a premise that says the obligation is absent. The
//! modeled capture, settlement, selection, and removal operations preserve
//! it. Binding those operations and the real `value` result to arbitrary
//! Rust execution remains a source-extraction obligation.

use vstd::prelude::*;

verus! {

pub enum PathStatusImage {
    Open,
    SplitComplete,
    Dead,
}

pub struct InnerPathImage {
    pub status: PathStatusImage,
    pub stack: Option<usize>,
    pub completed_segments: usize,
    pub first_tag_is_elist: bool,
    pub stable: bool,
}

pub struct FocusedOwnerImage {
    pub owner: InnerPathImage,
    pub captured_region: usize,
}

pub struct FocusedReducerImage {
    pub current: InnerPathImage,
    pub ghost owners: Seq<FocusedOwnerImage>,
}

pub enum RootSummaryImage {
    Bare { stable: bool },
    Split { stable: bool },
}

pub enum RootValueImage {
    NotCalled,
    Failed,
    Succeeded { summary: RootSummaryImage },
}

/// `focus_path` captures `self.current.clone()` only after obtaining its
/// `Some(id)` Region stack. Truncation and settlement may remove owners, but
/// retained owner snapshots keep the captured stack.
pub open spec fn focused_owner_stack_retained(reducer: FocusedReducerImage) -> bool {
    forall|i: int| 0 <= i < reducer.owners.len() ==>
        reducer.owners[i].owner.stack == Some(reducer.owners[i].captured_region)
}

pub open spec fn focused_owner_sequence_retains_stack(owners: Seq<FocusedOwnerImage>) -> bool {
    forall|i: int| 0 <= i < owners.len() ==>
        owners[i].owner.stack == Some(owners[i].captured_region)
}

/// `settle_focus` keeps the owner vector intact and changes only its local
/// `remaining` count. A due path boundary sets `remaining = due.owner`;
/// otherwise the loop stops with the current count. The bound on `due.owner`
/// is the separate boundary-index invariant needed to justify the Rust loop.
pub enum SettleFocusOwnerEvent {
    Stop,
    FinishDue { due_owner: usize },
}

pub struct SettleFocusOwnerImage {
    pub ghost owners: Seq<FocusedOwnerImage>,
    pub remaining: usize,
}

pub open spec fn settle_focus_remaining_after(
    remaining: usize,
    event: SettleFocusOwnerEvent,
) -> usize {
    match event {
        SettleFocusOwnerEvent::Stop => remaining,
        SettleFocusOwnerEvent::FinishDue { due_owner } => due_owner,
    }
}

pub open spec fn settle_focus_owner_image(
    before: FocusedReducerImage,
    remaining: usize,
    event: SettleFocusOwnerEvent,
) -> SettleFocusOwnerImage {
    SettleFocusOwnerImage {
        owners: before.owners,
        remaining: settle_focus_remaining_after(remaining, event),
    }
}

/// `select_focus` either clears all owners after a dead path, or retains the
/// prefix named by `settle_focus` before `focus_path` captures new owners.
pub open spec fn select_focus_retained_owners(
    before: FocusedReducerImage,
    remaining: usize,
    dead: bool,
) -> Seq<FocusedOwnerImage> {
    if dead && before.owners.len() > 0 {
        Seq::empty()
    } else {
        before.owners.subrange(0, remaining as int)
    }
}

/// Every entry appended by the subsequent `focus_path` loop was captured
/// from a path with a `Some(Region id)` stack. The single-entry constructor
/// below proves this condition for one actual-shaped capture.
pub open spec fn focus_captures_retain_stack(captures: Seq<FocusedOwnerImage>) -> bool {
    focused_owner_sequence_retains_stack(captures)
}

/// A source-shaped trace of paths seen by the `focus_path` loop. Each path
/// was captured only after its Region stack was read as `Some(id)`.
pub open spec fn focus_captures_from_paths(
    paths: Seq<InnerPathImage>,
) -> Seq<FocusedOwnerImage> {
    Seq::new(paths.len(), |i: int| FocusedOwnerImage {
        owner: paths[i],
        captured_region: if paths[i].stack.is_some() { paths[i].stack.unwrap() } else { 0 },
    })
}

pub proof fn focus_captures_from_paths_retain_stack(
    paths: Seq<InnerPathImage>,
)
    requires forall|i: int| 0 <= i < paths.len() ==> paths[i].stack.is_some(),
    ensures focus_captures_retain_stack(focus_captures_from_paths(paths)),
{
    let captures = focus_captures_from_paths(paths);
    assert forall|i: int| 0 <= i < captures.len() implies
        captures[i].owner.stack == Some(captures[i].captured_region) by {
        assert(captures[i].owner == paths[i]);
        assert(captures[i].captured_region == paths[i].stack.unwrap());
    }
}

pub proof fn settle_focus_owner_vector_unchanged(
    before: FocusedReducerImage,
    remaining: usize,
    event: SettleFocusOwnerEvent,
)
    requires
        focused_owner_stack_retained(before),
        remaining as int <= before.owners.len(),
        match event {
            SettleFocusOwnerEvent::Stop => true,
            SettleFocusOwnerEvent::FinishDue { due_owner } => due_owner < remaining,
        },
    ensures
        settle_focus_owner_image(before, remaining, event).owners == before.owners,
        settle_focus_owner_image(before, remaining, event).remaining as int
            <= before.owners.len(),
        focused_owner_sequence_retains_stack(
            settle_focus_owner_image(before, remaining, event).owners,
        ),
{}

pub proof fn select_focus_retained_prefix_preserves_stack(
    before: FocusedReducerImage,
    remaining: usize,
    dead: bool,
)
    requires
        focused_owner_stack_retained(before),
        remaining as int <= before.owners.len(),
    ensures
        focused_owner_sequence_retains_stack(
            select_focus_retained_owners(before, remaining, dead),
        ),
{
    let after = select_focus_retained_owners(before, remaining, dead);
    if !(dead && before.owners.len() > 0) {
        assert forall|i: int| 0 <= i < after.len() implies
            after[i].owner.stack == Some(after[i].captured_region) by {
            assert(after[i] == before.owners[i]);
        }
    }
}

/// This is the owner-vector effect of `select_focus` followed by zero or
/// more `focus_path` captures. It says nothing about the independently
/// updated current path, graph stamp, or frame-storage checkpoint.
pub proof fn select_focus_then_capture_preserves_owner_stack(
    before: FocusedReducerImage,
    remaining: usize,
    dead: bool,
    captures: Seq<FocusedOwnerImage>,
)
    requires
        focused_owner_stack_retained(before),
        remaining as int <= before.owners.len(),
        dead ==> captures.len() == 0,
        focus_captures_retain_stack(captures),
    ensures
        focused_owner_sequence_retains_stack(
            select_focus_retained_owners(before, remaining, dead).add(captures),
        ),
{
    select_focus_retained_prefix_preserves_stack(before, remaining, dead);
    let retained = select_focus_retained_owners(before, remaining, dead);
    let after = retained.add(captures);
    assert forall|i: int| 0 <= i < after.len() implies
        after[i].owner.stack == Some(after[i].captured_region) by {
        if i < retained.len() {
            assert(after[i] == retained[i]);
        } else {
            assert(after[i] == captures[i - retained.len()]);
        }
    }
}

pub proof fn select_focus_then_source_captures_preserves_owner_stack(
    before: FocusedReducerImage,
    remaining: usize,
    dead: bool,
    captured_paths: Seq<InnerPathImage>,
)
    requires
        focused_owner_stack_retained(before),
        remaining as int <= before.owners.len(),
        dead ==> captured_paths.len() == 0,
        forall|i: int| 0 <= i < captured_paths.len() ==>
            captured_paths[i].stack.is_some(),
    ensures
        focused_owner_sequence_retains_stack(
            select_focus_retained_owners(before, remaining, dead).add(
                focus_captures_from_paths(captured_paths),
            ),
        ),
{
    focus_captures_from_paths_retain_stack(captured_paths);
    select_focus_then_capture_preserves_owner_stack(
        before, remaining, dead, focus_captures_from_paths(captured_paths),
    );
}

/// The summary-producing branches of `finish_path_with_pending`. The exact
/// error kind for a nonempty stack depends on `pending`, but neither kind
/// yields a summary; that distinction is immaterial to this success proof.
pub open spec fn finish_path_summary(path: InnerPathImage) -> Option<RootSummaryImage> {
    if path.status == PathStatusImage::Dead || path.stack.is_some() {
        None
    } else if path.status == PathStatusImage::SplitComplete {
        Some(RootSummaryImage::Split { stable: path.stable })
    } else if path.completed_segments == 1 && !path.first_tag_is_elist {
        Some(RootSummaryImage::Bare { stable: path.stable })
    } else {
        None
    }
}

/// Source branch order: absent value, outermost saved owner, current path.
/// The real `value` returns `Option<Result<...>>`; this image retains exactly
/// the success distinction needed here without inventing an error code.
pub open spec fn root_eof_value_image(
    reducer: FocusedReducerImage,
    has_value: bool,
) -> RootValueImage {
    if !has_value {
        RootValueImage::NotCalled
    } else {
        let selected = if reducer.owners.len() > 0 {
            reducer.owners[0].owner
        } else {
            reducer.current
        };
        match finish_path_summary(selected) {
            Some(summary) => RootValueImage::Succeeded { summary },
            None => RootValueImage::Failed,
        }
    }
}

/// This is the first step of `active_obligation_frame`: `let top =
/// path.stack?`. Absence of a candidate proves the actual query returns
/// `None` without needing to inspect nested Region/Obligation frames.
pub open spec fn active_obligation_query_has_candidate(path: InnerPathImage) -> bool {
    path.status != PathStatusImage::Dead && path.stack.is_some()
}

pub proof fn focused_owner_capture_retains_stack(
    before: FocusedReducerImage,
    held: InnerPathImage,
)
    requires
        focused_owner_stack_retained(before),
        before.current.stack.is_some(),
    ensures {
        let captured = FocusedReducerImage {
            current: held,
            owners: before.owners.push(FocusedOwnerImage {
                owner: before.current,
                captured_region: before.current.stack.unwrap(),
            }),
        };
        focused_owner_stack_retained(captured)
    },
{
    let new_len = before.owners.len();
    let captured = FocusedReducerImage {
        current: held,
        owners: before.owners.push(FocusedOwnerImage {
            owner: before.current,
            captured_region: before.current.stack.unwrap(),
        }),
    };
    assert(forall|i: int| 0 <= i < captured.owners.len() ==>
        captured.owners[i].owner.stack == Some(captured.owners[i].captured_region)) by {
        assert forall|i: int| 0 <= i < captured.owners.len() implies
            captured.owners[i].owner.stack == Some(captured.owners[i].captured_region) by {
            if i < new_len {
                assert(captured.owners[i] == before.owners[i]);
            } else {
                assert(i == new_len);
                assert(captured.owners[i].owner.stack == before.current.stack);
            }
        }
    }
}

pub proof fn focused_owner_root_eof_cannot_succeed(
    reducer: FocusedReducerImage,
)
    requires
        focused_owner_stack_retained(reducer),
        reducer.owners.len() > 0,
    ensures
        !matches!(root_eof_value_image(reducer, true), RootValueImage::Succeeded { .. }),
{
    assert(reducer.owners[0].owner.stack == Some(reducer.owners[0].captured_region));
    assert(finish_path_summary(reducer.owners[0].owner).is_none());
}

/// The conclusion does not assume `active_obligation() == None`. It derives
/// the empty current stack from the actual-shaped, successful value branch.
/// The source query's `stack?` then cannot find an active Obligation frame.
pub proof fn successful_root_eof_value_has_no_active_obligation(
    reducer: FocusedReducerImage,
    has_value: bool,
)
    requires
        focused_owner_stack_retained(reducer),
        matches!(root_eof_value_image(reducer, has_value), RootValueImage::Succeeded { .. }),
    ensures
        has_value,
        reducer.owners.len() == 0,
        reducer.current.stack.is_none(),
        !active_obligation_query_has_candidate(reducer.current),
{
    if reducer.owners.len() > 0 {
        focused_owner_root_eof_cannot_succeed(reducer);
        assert(false);
    }
    assert(finish_path_summary(reducer.current).is_some());
}

}

fn main() {}
