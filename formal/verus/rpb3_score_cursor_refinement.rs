use vstd::prelude::*;

mod rpb3_score_transcript;
use rpb3_score_transcript as score;

verus! {

pub enum CursorOp {
    Score { handle: usize },
    Slice { bytes: Seq<u8>, offset: usize },
    Integer { bytes: Seq<u8>, offset: usize },
    Escaped { bytes: Seq<u8>, offset: usize, pending_ff: bool },
    IndexedEscaped { bytes: Seq<u8>, offset: usize, pending_ff: bool },
    Byte { byte: u8 },
}

pub struct CursorState {
    pub ghost arena: score::ScoreArena,
    pub ghost work: Seq<CursorOp>,
}

pub enum CursorStep {
    Silent { next: CursorState },
    Emit { byte: u8, next: CursorState },
    End,
}

pub open spec fn cursor_op_wf(arena: score::ScoreArena, op: CursorOp) -> bool {
    match op {
        CursorOp::Score { handle } => (handle as int) < arena.entries.len(),
        CursorOp::Slice { bytes, offset }
        | CursorOp::Integer { bytes, offset } =>
            (offset as int) < bytes.len() && bytes.len() <= usize::MAX as int,
        CursorOp::Escaped { bytes, offset, .. }
        | CursorOp::IndexedEscaped { bytes, offset, .. } =>
            offset as int <= bytes.len() && bytes.len() <= usize::MAX as int,
        CursorOp::Byte { .. } => true,
    }
}

pub open spec fn cursor_state_wf(state: CursorState) -> bool {
    score::arena_well_formed(state.arena)
        && forall|i: int| 0 <= i < state.work.len()
            ==> cursor_op_wf(state.arena, state.work[i])
}

pub open spec fn cursor_initial(
    arena: score::ScoreArena, handle: usize,
) -> Option<CursorState> {
    if !score::arena_well_formed(arena) || (handle as int) >= arena.entries.len() {
        None
    } else {
        Some(CursorState {
            arena,
            work: seq![CursorOp::Score { handle }],
        })
    }
}

pub open spec fn escaped_remaining(
    bytes: Seq<u8>, offset: usize, pending_ff: bool,
) -> Seq<u8> {
    (if pending_ff { seq![0xffu8] } else { Seq::empty() })
        + score::escape_zeroes(bytes.subrange(offset as int, bytes.len() as int))
}

pub open spec fn cursor_op_bytes(
    arena: score::ScoreArena, op: CursorOp,
) -> Seq<u8> {
    match op {
        CursorOp::Score { handle } =>
            arena.entries[handle as int].transcript,
        CursorOp::Slice { bytes, offset }
        | CursorOp::Integer { bytes, offset } =>
            bytes.subrange(offset as int, bytes.len() as int),
        CursorOp::Escaped { bytes, offset, pending_ff }
        | CursorOp::IndexedEscaped { bytes, offset, pending_ff } =>
            escaped_remaining(bytes, offset, pending_ff),
        CursorOp::Byte { byte } => seq![byte],
    }
}

pub open spec fn cursor_work_bytes(
    arena: score::ScoreArena, work: Seq<CursorOp>,
) -> Seq<u8>
    decreases work.len(),
{
    if work.len() == 0 {
        Seq::empty()
    } else {
        cursor_op_bytes(arena, work.last())
            + cursor_work_bytes(arena, work.drop_last())
    }
}

pub open spec fn cursor_push_children_reverse(
    work: Seq<CursorOp>, children: Seq<usize>,
) -> Seq<CursorOp>
    decreases children.len(),
{
    if children.len() == 0 {
        work
    } else {
        cursor_push_children_reverse(
            work, children.drop_first(),
        ).push(CursorOp::Score { handle: children[0] })
    }
}

pub open spec fn cursor_after(
    before: CursorState, work: Seq<CursorOp>,
) -> CursorState {
    CursorState { arena: before.arena, work }
}

pub proof fn cursor_work_push(
    arena: score::ScoreArena, work: Seq<CursorOp>, op: CursorOp,
)
    ensures cursor_work_bytes(arena, work.push(op))
        == cursor_op_bytes(arena, op) + cursor_work_bytes(arena, work),
{
    assert(work.push(op).len() > 0);
    assert(work.push(op).last() == op);
    assert(work.push(op).drop_last() =~= work);
}

pub proof fn cursor_work_empty(arena: score::ScoreArena)
    ensures cursor_work_bytes(arena, Seq::empty()) == Seq::<u8>::empty(),
{}

pub proof fn cursor_reversed_children_bytes(
    arena: score::ScoreArena, work: Seq<CursorOp>, children: Seq<usize>,
)
    requires score::child_handles_valid(arena.entries, children),
    ensures cursor_work_bytes(
        arena, cursor_push_children_reverse(work, children),
    ) == score::children_transcript(arena.entries, children)
        + cursor_work_bytes(arena, work),
    decreases children.len(),
{
    if children.len() > 0 {
        assert(score::child_handles_valid(
            arena.entries, children.drop_first(),
        ));
        cursor_reversed_children_bytes(arena, work, children.drop_first());
        assert((children[0] as int) < arena.entries.len());
        assert(cursor_op_bytes(
            arena, CursorOp::Score { handle: children[0] },
        ) == arena.entries[children[0] as int].transcript);
        assert(cursor_push_children_reverse(work, children)
            == cursor_push_children_reverse(
                work, children.drop_first(),
            ).push(CursorOp::Score { handle: children[0] }));
        cursor_work_push(
            arena,
            cursor_push_children_reverse(work, children.drop_first()),
            CursorOp::Score { handle: children[0] },
        );
        assert(score::children_transcript(arena.entries, children)
            == arena.entries[children[0] as int].transcript
                + score::children_transcript(arena.entries, children.drop_first()));
        vstd::seq_lib::lemma_concat_associative(
            arena.entries[children[0] as int].transcript,
            score::children_transcript(arena.entries, children.drop_first()),
            cursor_work_bytes(arena, work),
        );
        assert(cursor_work_bytes(
            arena, cursor_push_children_reverse(work, children),
        ) == arena.entries[children[0] as int].transcript
            + (score::children_transcript(arena.entries, children.drop_first())
                + cursor_work_bytes(arena, work)));
        assert(cursor_work_bytes(
            arena, cursor_push_children_reverse(work, children),
        ) == score::children_transcript(arena.entries, children)
            + cursor_work_bytes(arena, work));
    } else {
        Seq::<u8>::add_empty_left(
            score::children_transcript(arena.entries, children),
            cursor_work_bytes(arena, work),
        );
    }
}

pub proof fn cursor_slice_head_exact(bytes: Seq<u8>, offset: usize)
    requires (offset as int) < bytes.len(),
    ensures bytes.subrange(offset as int, bytes.len() as int)
        == seq![bytes[offset as int]]
            + bytes.subrange(offset as int + 1, bytes.len() as int),
{
    assert(bytes.subrange(offset as int, bytes.len() as int)
        =~= seq![bytes[offset as int]]
            + bytes.subrange(offset as int + 1, bytes.len() as int));
}

pub proof fn cursor_escape_pending_exact(bytes: Seq<u8>, offset: usize)
    requires (offset as int) <= bytes.len(),
    ensures escaped_remaining(bytes, offset, true)
        == seq![0xffu8] + escaped_remaining(bytes, offset, false),
{
    Seq::<u8>::add_empty_left(
        Seq::empty(),
        score::escape_zeroes(bytes.subrange(offset as int, bytes.len() as int)),
    );
}

pub proof fn cursor_escape_finished_exact(bytes: Seq<u8>)
    requires bytes.len() <= usize::MAX as int,
    ensures escaped_remaining(bytes, bytes.len() as usize, false) == Seq::<u8>::empty(),
{
    assert(bytes.subrange(bytes.len() as int, bytes.len() as int) == Seq::<u8>::empty());
    Seq::<u8>::add_empty_left(Seq::empty(), Seq::empty());
}

pub proof fn cursor_escape_head_exact(bytes: Seq<u8>, offset: usize)
    requires (offset as int) < bytes.len(), bytes.len() <= usize::MAX as int,
    ensures escaped_remaining(bytes, offset, false)
        == seq![bytes[offset as int]]
            + escaped_remaining(
                bytes, (offset as int + 1) as usize,
                bytes[offset as int] == 0,
            ),
{
    let tail = bytes.subrange(offset as int + 1, bytes.len() as int);
    assert(bytes.subrange(offset as int, bytes.len() as int).drop_first() =~= tail);
    if bytes[offset as int] == 0 {
        vstd::seq_lib::lemma_concat_associative(
            seq![0u8], seq![0xffu8], score::escape_zeroes(tail),
        );
    } else {
        Seq::<u8>::add_empty_left(Seq::empty(), score::escape_zeroes(tail));
    }
}

pub open spec fn cursor_step(before: CursorState) -> CursorStep {
    if before.work.len() == 0 {
        CursorStep::End
    } else {
        let rest = before.work.drop_last();
        match before.work.last() {
            CursorOp::Score { handle } => {
                match before.arena.entries[handle as int].kind {
                    score::ScoreNode::Integer(value) => {
                        let word = score::big_endian_eight(score::sign_flipped_word(value));
                        CursorStep::Silent { next: cursor_after(before,
                            rest.push(CursorOp::Integer { bytes: word, offset: 0 })
                                .push(CursorOp::Slice { bytes: seq![1u8, 1u8], offset: 0 }),
                        ) }
                    },
                    score::ScoreNode::StringAtom(bytes) => CursorStep::Silent {
                        next: cursor_after(before,
                            rest.push(CursorOp::Slice { bytes: seq![0u8, 0u8], offset: 0 })
                                .push(CursorOp::Escaped {
                                    bytes, offset: 0, pending_ff: false,
                                })
                                .push(CursorOp::Slice { bytes: seq![1u8, 2u8], offset: 0 }),
                        ),
                    },
                    score::ScoreNode::BytesAtom(bytes) => CursorStep::Silent {
                        next: cursor_after(before,
                            rest.push(CursorOp::Slice { bytes: seq![0u8, 0u8], offset: 0 })
                                .push(CursorOp::Escaped {
                                    bytes, offset: 0, pending_ff: false,
                                })
                                .push(CursorOp::Slice { bytes: seq![1u8, 3u8], offset: 0 }),
                        ),
                    },
                    score::ScoreNode::Node(children) => CursorStep::Silent {
                        next: cursor_after(before,
                            cursor_push_children_reverse(
                                rest.push(CursorOp::Byte { byte: 0u8 }), children,
                            ).push(CursorOp::Byte { byte: 2u8 }),
                        ),
                    },
                }
            },
            CursorOp::Slice { bytes, offset }
            | CursorOp::Integer { bytes, offset } => {
                let byte = bytes[offset as int];
                let remaining = if offset as int + 1 < bytes.len() {
                    match before.work.last() {
                        CursorOp::Slice { .. } => rest.push(CursorOp::Slice {
                            bytes, offset: (offset as int + 1) as usize,
                        }),
                        _ => rest.push(CursorOp::Integer {
                            bytes, offset: (offset as int + 1) as usize,
                        }),
                    }
                } else { rest };
                CursorStep::Emit { byte, next: cursor_after(before, remaining) }
            },
            CursorOp::Escaped { bytes, offset, pending_ff } => {
                if pending_ff {
                    CursorStep::Emit { byte: 0xffu8,
                        next: cursor_after(before, rest.push(CursorOp::Escaped {
                            bytes, offset, pending_ff: false,
                        })),
                    }
                } else if offset as int == bytes.len() {
                    CursorStep::Silent { next: cursor_after(before, rest) }
                } else {
                    let byte = bytes[offset as int];
                    CursorStep::Emit { byte,
                        next: cursor_after(before, rest.push(CursorOp::Escaped {
                            bytes, offset: (offset as int + 1) as usize,
                            pending_ff: byte == 0,
                        })),
                    }
                }
            },
            CursorOp::IndexedEscaped { bytes, offset, pending_ff } => {
                if pending_ff {
                    CursorStep::Emit { byte: 0xffu8,
                        next: cursor_after(before, rest.push(CursorOp::IndexedEscaped {
                            bytes, offset, pending_ff: false,
                        })),
                    }
                } else if offset as int == bytes.len() {
                    CursorStep::Silent { next: cursor_after(before, rest) }
                } else {
                    let byte = bytes[offset as int];
                    CursorStep::Emit { byte,
                        next: cursor_after(before, rest.push(CursorOp::IndexedEscaped {
                            bytes, offset: (offset as int + 1) as usize,
                            pending_ff: byte == 0,
                        })),
                    }
                }
            },
            CursorOp::Byte { byte } => CursorStep::Emit {
                byte, next: cursor_after(before, rest),
            },
        }
    }
}

pub proof fn cursor_non_score_step_conserves(before: CursorState)
    requires
        cursor_state_wf(before),
        before.work.len() > 0,
        match before.work.last() {
            CursorOp::Score { .. } => false,
            _ => true,
        },
    ensures match cursor_step(before) {
        CursorStep::Silent { next } =>
            cursor_work_bytes(before.arena, before.work)
                == cursor_work_bytes(next.arena, next.work),
        CursorStep::Emit { byte, next } =>
            cursor_work_bytes(before.arena, before.work)
                == seq![byte] + cursor_work_bytes(next.arena, next.work),
        CursorStep::End => false,
    },
{
    let rest = before.work.drop_last();
    let op = before.work.last();
    cursor_work_push(before.arena, rest, op);
    match op {
        CursorOp::Score { .. } => assert(false),
        CursorOp::Slice { bytes, offset }
        | CursorOp::Integer { bytes, offset } => {
            cursor_slice_head_exact(bytes, offset);
            if offset as int + 1 < bytes.len() {
                match op {
                    CursorOp::Slice { .. } => cursor_work_push(
                        before.arena, rest, CursorOp::Slice {
                            bytes, offset: (offset as int + 1) as usize,
                        },
                    ),
                    _ => cursor_work_push(
                        before.arena, rest, CursorOp::Integer {
                            bytes, offset: (offset as int + 1) as usize,
                        },
                    ),
                }
            } else {
                assert(offset as int + 1 == bytes.len());
                assert(bytes.subrange(bytes.len() as int, bytes.len() as int)
                    == Seq::<u8>::empty());
                Seq::<u8>::add_empty_right(seq![bytes[offset as int]], Seq::empty());
            }
        },
        CursorOp::Escaped { bytes, offset, pending_ff } => {
            if pending_ff {
                cursor_escape_pending_exact(bytes, offset);
                cursor_work_push(before.arena, rest, CursorOp::Escaped {
                    bytes, offset, pending_ff: false,
                });
            } else if offset as int == bytes.len() {
                cursor_escape_finished_exact(bytes);
                Seq::<u8>::add_empty_left(Seq::empty(), cursor_work_bytes(before.arena, rest));
            } else {
                cursor_escape_head_exact(bytes, offset);
                cursor_work_push(before.arena, rest, CursorOp::Escaped {
                    bytes, offset: (offset as int + 1) as usize,
                    pending_ff: bytes[offset as int] == 0,
                });
            }
        },
        CursorOp::IndexedEscaped { bytes, offset, pending_ff } => {
            if pending_ff {
                cursor_escape_pending_exact(bytes, offset);
                cursor_work_push(before.arena, rest, CursorOp::IndexedEscaped {
                    bytes, offset, pending_ff: false,
                });
            } else if offset as int == bytes.len() {
                cursor_escape_finished_exact(bytes);
                Seq::<u8>::add_empty_left(Seq::empty(), cursor_work_bytes(before.arena, rest));
            } else {
                cursor_escape_head_exact(bytes, offset);
                cursor_work_push(before.arena, rest, CursorOp::IndexedEscaped {
                    bytes, offset: (offset as int + 1) as usize,
                    pending_ff: bytes[offset as int] == 0,
                });
            }
        },
        CursorOp::Byte { .. } => {},
    }
}

pub proof fn cursor_children_prefix_same(
    arena: score::ScoreArena, handle: usize, children: Seq<usize>,
)
    requires
        (handle as int) <= arena.entries.len(),
        score::child_handles_valid(arena.entries.take(handle as int), children),
    ensures
        score::child_handles_valid(arena.entries, children),
        score::children_transcript(
            arena.entries.take(handle as int), children,
        ) == score::children_transcript(arena.entries, children),
    decreases children.len(),
{
    if children.len() > 0 {
        assert((children[0] as int) < handle as int);
        assert(arena.entries.take(handle as int)[children[0] as int]
            == arena.entries[children[0] as int]);
        assert(score::child_handles_valid(
            arena.entries.take(handle as int), children.drop_first(),
        ));
        cursor_children_prefix_same(arena, handle, children.drop_first());
    }
}

pub proof fn cursor_score_step_conserves(before: CursorState)
    requires
        cursor_state_wf(before),
        before.work.len() > 0,
        match before.work.last() {
            CursorOp::Score { .. } => true,
            _ => false,
        },
    ensures match cursor_step(before) {
        CursorStep::Silent { next } =>
            cursor_work_bytes(before.arena, before.work)
                == cursor_work_bytes(next.arena, next.work),
        _ => false,
    },
{
    let rest = before.work.drop_last();
    if let CursorOp::Score { handle } = before.work.last() {
        let arena = before.arena;
        let entry = arena.entries[handle as int];
        assert(score::entry_well_formed(arena.entries, handle as int));
        cursor_work_push(arena, rest, CursorOp::Score { handle });
        match entry.kind {
            score::ScoreNode::Integer(value) => {
                let word = score::big_endian_eight(score::sign_flipped_word(value));
                let integer = CursorOp::Integer { bytes: word, offset: 0 };
                let prefix = CursorOp::Slice { bytes: seq![1u8, 1u8], offset: 0 };
                cursor_work_push(arena, rest, integer);
                cursor_work_push(arena, rest.push(integer), prefix);
                assert(word.subrange(0, word.len() as int) =~= word);
                assert(seq![1u8, 1u8].subrange(0, 2) =~= seq![1u8, 1u8]);
                vstd::seq_lib::lemma_concat_associative(
                    seq![1u8, 1u8], word, cursor_work_bytes(arena, rest),
                );
                assert(entry.transcript == score::integer_transcript(value));
            },
            score::ScoreNode::StringAtom(bytes) => {
                let end = CursorOp::Slice { bytes: seq![0u8, 0u8], offset: 0 };
                let escaped = CursorOp::Escaped {
                    bytes, offset: 0, pending_ff: false,
                };
                let prefix = CursorOp::Slice { bytes: seq![1u8, 2u8], offset: 0 };
                cursor_work_push(arena, rest, end);
                cursor_work_push(arena, rest.push(end), escaped);
                cursor_work_push(arena, rest.push(end).push(escaped), prefix);
                assert(bytes.subrange(0, bytes.len() as int) =~= bytes);
                Seq::<u8>::add_empty_left(Seq::empty(), score::escape_zeroes(bytes));
                assert(entry.transcript == score::string_transcript(bytes));
                vstd::seq_lib::lemma_concat_associative(
                    score::escape_zeroes(bytes), seq![0u8, 0u8],
                    cursor_work_bytes(arena, rest),
                );
                vstd::seq_lib::lemma_concat_associative(
                    seq![1u8, 2u8], score::escape_zeroes(bytes),
                    seq![0u8, 0u8] + cursor_work_bytes(arena, rest),
                );
            },
            score::ScoreNode::BytesAtom(bytes) => {
                let end = CursorOp::Slice { bytes: seq![0u8, 0u8], offset: 0 };
                let escaped = CursorOp::Escaped {
                    bytes, offset: 0, pending_ff: false,
                };
                let prefix = CursorOp::Slice { bytes: seq![1u8, 3u8], offset: 0 };
                cursor_work_push(arena, rest, end);
                cursor_work_push(arena, rest.push(end), escaped);
                cursor_work_push(arena, rest.push(end).push(escaped), prefix);
                assert(bytes.subrange(0, bytes.len() as int) =~= bytes);
                Seq::<u8>::add_empty_left(Seq::empty(), score::escape_zeroes(bytes));
                assert(entry.transcript == score::bytes_transcript(bytes));
                vstd::seq_lib::lemma_concat_associative(
                    score::escape_zeroes(bytes), seq![0u8, 0u8],
                    cursor_work_bytes(arena, rest),
                );
                vstd::seq_lib::lemma_concat_associative(
                    seq![1u8, 3u8], score::escape_zeroes(bytes),
                    seq![0u8, 0u8] + cursor_work_bytes(arena, rest),
                );
            },
            score::ScoreNode::Node(children) => {
                let end = CursorOp::Byte { byte: 0u8 };
                let start = CursorOp::Byte { byte: 2u8 };
                assert(score::child_handles_valid(
                    arena.entries.take(handle as int), children,
                ));
                cursor_children_prefix_same(arena, handle, children);
                cursor_work_push(arena, rest, end);
                cursor_reversed_children_bytes(arena, rest.push(end), children);
                cursor_work_push(
                    arena,
                    cursor_push_children_reverse(rest.push(end), children),
                    start,
                );
                assert(entry.transcript == score::kind_transcript(
                    arena.entries.take(handle as int), entry.kind,
                ));
                vstd::seq_lib::lemma_concat_associative(
                    score::children_transcript(arena.entries, children),
                    seq![0u8], cursor_work_bytes(arena, rest),
                );
                vstd::seq_lib::lemma_concat_associative(
                    seq![2u8],
                    score::children_transcript(arena.entries, children),
                    seq![0u8] + cursor_work_bytes(arena, rest),
                );
            },
        }
    } else {
        assert(false);
    }
}

pub enum FlatIndexedRegion {
    Borrowed { backing: Seq<u8>, start: usize, len: usize },
    Contiguous { bytes: Seq<u8> },
}

pub struct FlatIndexedCursor {
    pub region: FlatIndexedRegion,
    pub at: usize,
}

pub open spec fn flat_indexed_region_wf(region: FlatIndexedRegion) -> bool {
    match region {
        FlatIndexedRegion::Borrowed { backing, start, len } =>
            (start as int) + (len as int) <= backing.len(),
        FlatIndexedRegion::Contiguous { bytes } =>
            bytes.len() <= usize::MAX as int,
    }
}

pub open spec fn flat_indexed_region_bytes(region: FlatIndexedRegion) -> Seq<u8> {
    match region {
        FlatIndexedRegion::Borrowed { backing, start, len } =>
            backing.subrange(start as int, (start as int) + (len as int)),
        FlatIndexedRegion::Contiguous { bytes } => bytes,
    }
}

pub open spec fn flat_indexed_cursor_wf(cursor: FlatIndexedCursor) -> bool {
    flat_indexed_region_wf(cursor.region)
        && (cursor.at as int) <= flat_indexed_region_bytes(cursor.region).len()
}

pub open spec fn flat_indexed_initial(region: FlatIndexedRegion) -> Option<FlatIndexedCursor> {
    if flat_indexed_region_wf(region) {
        Some(FlatIndexedCursor { region, at: 0 })
    } else {
        None
    }
}

pub open spec fn flat_indexed_next(
    cursor: FlatIndexedCursor,
) -> (Option<u8>, FlatIndexedCursor) {
    let bytes = flat_indexed_region_bytes(cursor.region);
    if (cursor.at as int) < bytes.len() {
        (Some(bytes[cursor.at as int]), FlatIndexedCursor {
            region: cursor.region,
            at: (cursor.at as int + 1) as usize,
        })
    } else {
        (None, cursor)
    }
}

pub open spec fn flat_indexed_score_op(
    cursor: FlatIndexedCursor, pending_ff: bool,
) -> CursorOp {
    CursorOp::IndexedEscaped {
        bytes: flat_indexed_region_bytes(cursor.region),
        offset: cursor.at,
        pending_ff,
    }
}

pub proof fn flat_indexed_next_projects(cursor: FlatIndexedCursor)
    requires flat_indexed_cursor_wf(cursor),
    ensures {
        let bytes = flat_indexed_region_bytes(cursor.region);
        let (byte, next) = flat_indexed_next(cursor);
        &&& flat_indexed_cursor_wf(next)
        &&& if (cursor.at as int) < bytes.len() {
            &&& byte == Some(bytes[cursor.at as int])
            &&& next.at as int == cursor.at as int + 1
            &&& flat_indexed_region_bytes(next.region) == bytes
        } else {
            &&& byte.is_none()
            &&& next.at == cursor.at
            &&& flat_indexed_region_bytes(next.region) == bytes
        }
    },
{
    match cursor.region {
        FlatIndexedRegion::Borrowed { backing, start, len } => {
            assert(flat_indexed_region_bytes(cursor.region).len() == len as int);
        },
        FlatIndexedRegion::Contiguous { .. } => {},
    }
}

pub proof fn flat_indexed_score_step_projects(
    arena: score::ScoreArena, cursor: FlatIndexedCursor,
)
    requires
        flat_indexed_cursor_wf(cursor),
        score::arena_well_formed(arena),
        (cursor.at as int) < flat_indexed_region_bytes(cursor.region).len(),
    ensures {
        let op = flat_indexed_score_op(cursor, false);
        let state = CursorState { arena, work: seq![op] };
        let (byte, next) = flat_indexed_next(cursor);
        match cursor_step(state) {
            CursorStep::Emit { byte: emitted, next: score_next } => {
                &&& byte == Some(emitted)
                &&& score_next.work == seq![flat_indexed_score_op(
                    next, emitted == 0,
                )]
            },
            _ => false,
        }
    },
{
    flat_indexed_next_projects(cursor);
    assert(flat_indexed_region_bytes(cursor.region).len() <= usize::MAX as int);
}

pub struct FrozenOwnedSegment {
    pub page_bytes: Seq<u8>,
    pub page_start: usize,
    pub used: usize,
}

pub struct FrozenOwnedView {
    pub segment: FrozenOwnedSegment,
    pub start: usize,
    pub len: usize,
}

pub struct FrozenOwnedCursor {
    pub view: FrozenOwnedView,
    pub at: usize,
}

pub open spec fn frozen_owned_view_wf(view: FrozenOwnedView) -> bool {
    &&& (view.segment.page_start as int) + (view.segment.used as int)
        <= view.segment.page_bytes.len()
    &&& (view.start as int) + (view.len as int) <= view.segment.used as int
}

pub open spec fn frozen_owned_view_bytes(view: FrozenOwnedView) -> Seq<u8> {
    view.segment.page_bytes.subrange(
        (view.segment.page_start as int) + (view.start as int),
        (view.segment.page_start as int) + (view.start as int) + (view.len as int),
    )
}

pub open spec fn frozen_owned_subview(
    view: FrozenOwnedView, start: usize, end: usize,
) -> Option<FrozenOwnedView> {
    if start > end || end > view.len {
        None
    } else {
        Some(FrozenOwnedView {
            segment: view.segment,
            start: (view.start as int + start as int) as usize,
            len: (end as int - start as int) as usize,
        })
    }
}

pub open spec fn frozen_owned_cursor_wf(cursor: FrozenOwnedCursor) -> bool {
    frozen_owned_view_wf(cursor.view)
        && (cursor.at as int) <= cursor.view.len as int
}

pub open spec fn frozen_owned_initial(view: FrozenOwnedView) -> Option<FrozenOwnedCursor> {
    if frozen_owned_view_wf(view) {
        Some(FrozenOwnedCursor { view, at: 0 })
    } else {
        None
    }
}

pub open spec fn frozen_owned_next(
    cursor: FrozenOwnedCursor,
) -> (Option<u8>, FrozenOwnedCursor) {
    if cursor.at < cursor.view.len {
        let at = cursor.view.segment.page_start as int
            + cursor.view.start as int + cursor.at as int;
        (Some(cursor.view.segment.page_bytes[at]), FrozenOwnedCursor {
            view: cursor.view,
            at: (cursor.at as int + 1) as usize,
        })
    } else {
        (None, cursor)
    }
}

pub open spec fn frozen_owned_score_op(
    cursor: FrozenOwnedCursor, pending_ff: bool,
) -> CursorOp {
    CursorOp::IndexedEscaped {
        bytes: frozen_owned_view_bytes(cursor.view),
        offset: cursor.at,
        pending_ff,
    }
}

pub proof fn frozen_owned_subview_projects(
    view: FrozenOwnedView, start: usize, end: usize,
)
    requires
        frozen_owned_view_wf(view),
        start <= end <= view.len,
    ensures {
        let child = frozen_owned_subview(view, start, end).unwrap();
        &&& frozen_owned_subview(view, start, end).is_some()
        &&& frozen_owned_view_wf(child)
        &&& frozen_owned_view_bytes(child)
            == frozen_owned_view_bytes(view).subrange(start as int, end as int)
    },
{
    let child = frozen_owned_subview(view, start, end).unwrap();
    assert(child.start as int == view.start as int + start as int);
    assert(child.len as int == end as int - start as int);
    assert(frozen_owned_view_bytes(child) =~=
        frozen_owned_view_bytes(view).subrange(start as int, end as int));
}

pub proof fn frozen_owned_next_projects(cursor: FrozenOwnedCursor)
    requires frozen_owned_cursor_wf(cursor),
    ensures {
        let bytes = frozen_owned_view_bytes(cursor.view);
        let (byte, next) = frozen_owned_next(cursor);
        &&& frozen_owned_cursor_wf(next)
        &&& if cursor.at < cursor.view.len {
            &&& byte == Some(bytes[cursor.at as int])
            &&& next.at as int == cursor.at as int + 1
            &&& next.view == cursor.view
        } else {
            &&& byte.is_none()
            &&& next.at == cursor.at
        }
    },
{
    let view = cursor.view;
    assert(frozen_owned_view_bytes(view).len() == view.len as int);
    if cursor.at < view.len {
        let absolute = view.segment.page_start as int
            + view.start as int + cursor.at as int;
        assert(frozen_owned_view_bytes(view)[cursor.at as int]
            == view.segment.page_bytes[absolute]);
    }
}

pub proof fn frozen_owned_score_step_projects(
    arena: score::ScoreArena, cursor: FrozenOwnedCursor,
)
    requires
        frozen_owned_cursor_wf(cursor),
        score::arena_well_formed(arena),
        cursor.at < cursor.view.len,
    ensures {
        let op = frozen_owned_score_op(cursor, false);
        let state = CursorState { arena, work: seq![op] };
        let (byte, next) = frozen_owned_next(cursor);
        match cursor_step(state) {
            CursorStep::Emit { byte: emitted, next: score_next } => {
                &&& byte == Some(emitted)
                &&& score_next.work == seq![frozen_owned_score_op(
                    next, emitted == 0,
                )]
            },
            _ => false,
        }
    },
{
    frozen_owned_next_projects(cursor);
    assert(frozen_owned_view_bytes(cursor.view).len() == cursor.view.len as int);
}

pub struct FrozenPageLeafModel {
    pub page: usize,
    pub page_start: usize,
    pub len: usize,
    pub logical_start: usize,
}

pub struct FrozenPageDataModel {
    pub pages: Seq<Seq<u8>>,
    pub leaves: Seq<FrozenPageLeafModel>,
    pub logical_pages: Seq<usize>,
    pub len: usize,
}

pub struct FrozenMultiView {
    pub data: FrozenPageDataModel,
    pub start: usize,
    pub len: usize,
}

pub struct FrozenMultiCursor {
    pub view: FrozenMultiView,
    pub leaf: usize,
    pub absolute: usize,
    pub end: usize,
}

pub open spec fn frozen_page_leaf_bytes(
    data: FrozenPageDataModel, index: int,
) -> Seq<u8> {
    let leaf = data.leaves[index];
    data.pages[leaf.page as int].subrange(
        leaf.page_start as int,
        leaf.page_start as int + leaf.len as int,
    )
}

pub open spec fn frozen_page_prefix_bytes(
    data: FrozenPageDataModel, count: int,
) -> Seq<u8>
    decreases count,
{
    if count <= 0 {
        Seq::empty()
    } else {
        frozen_page_prefix_bytes(data, count - 1)
            + frozen_page_leaf_bytes(data, count - 1)
    }
}

pub open spec fn frozen_page_transcript(data: FrozenPageDataModel) -> Seq<u8> {
    frozen_page_prefix_bytes(data, data.leaves.len() as int)
}

pub open spec fn frozen_page_prefix_leaf(
    data: FrozenPageDataModel, count: int, absolute: int,
) -> int
    decreases count,
{
    if count <= 1 {
        0
    } else if absolute >= frozen_page_prefix_bytes(data, count - 1).len() {
        count - 1
    } else {
        frozen_page_prefix_leaf(data, count - 1, absolute)
    }
}

pub open spec fn frozen_page_find_from(
    data: FrozenPageDataModel, absolute: int, index: int,
) -> Option<int>
    decreases data.leaves.len() - index,
{
    if index < 0 || index >= data.leaves.len() {
        None
    } else {
        let leaf = data.leaves[index];
        if leaf.logical_start as int + leaf.len as int <= absolute {
            frozen_page_find_from(data, absolute, index + 1)
        } else {
            Some(index)
        }
    }
}

pub open spec fn frozen_page_indexed_leaf_at(
    data: FrozenPageDataModel, absolute: usize,
) -> Option<int> {
    if absolute >= data.len {
        None
    } else {
        let block = absolute as int / 32;
        if block >= data.logical_pages.len() {
            None
        } else {
            frozen_page_find_from(
                data, absolute as int,
                data.logical_pages[block] as int,
            )
        }
    }
}

pub open spec fn frozen_multi_data_wf(data: FrozenPageDataModel) -> bool {
    &&& data.len as int == frozen_page_transcript(data).len()
    &&& data.leaves.len() <= usize::MAX as int
    &&& data.logical_pages.len() as int
        == data.len as int / 32 + (if data.len % 32 == 0 { 0int } else { 1int })
    &&& forall|i: int| 0 <= i < data.leaves.len() ==> {
        let leaf = #[trigger] data.leaves[i];
        &&& leaf.len > 0
        &&& (leaf.page as int) < data.pages.len()
        &&& leaf.page_start as int + leaf.len as int
            <= data.pages[leaf.page as int].len()
        &&& leaf.logical_start as int
            == frozen_page_prefix_bytes(data, i).len()
    }
    &&& forall|absolute: usize| absolute < data.len ==> {
        let found = #[trigger] frozen_page_indexed_leaf_at(data, absolute);
        let expected = frozen_page_prefix_leaf(
            data, data.leaves.len() as int, absolute as int,
        );
        &&& found == Some(expected)
        &&& 0 <= expected < data.leaves.len()
        &&& (data.leaves[expected].logical_start as int) <= (absolute as int)
        &&& (absolute as int) < (data.leaves[expected].logical_start as int)
            + (data.leaves[expected].len as int)
    }
    &&& forall|absolute: usize, i: int|
        #![trigger data.leaves[i], frozen_page_prefix_leaf(data, data.leaves.len() as int, absolute as int)]
        absolute < data.len && 0 <= i < data.leaves.len()
            && (data.leaves[i].logical_start as int) <= (absolute as int)
            && (absolute as int) < (data.leaves[i].logical_start as int)
                + (data.leaves[i].len as int)
        ==> i == frozen_page_prefix_leaf(
            data, data.leaves.len() as int, absolute as int,
        )
}

pub open spec fn frozen_multi_view_wf(view: FrozenMultiView) -> bool {
    frozen_multi_data_wf(view.data)
        && view.start as int + view.len as int <= view.data.len as int
}

pub open spec fn frozen_multi_view_bytes(view: FrozenMultiView) -> Seq<u8> {
    frozen_page_transcript(view.data).subrange(
        view.start as int, view.start as int + view.len as int,
    )
}

pub open spec fn frozen_multi_byte_at(
    view: FrozenMultiView, logical_offset: usize,
) -> Option<u8> {
    if logical_offset >= view.len {
        None
    } else {
        let absolute = view.start as int + logical_offset as int;
        match frozen_page_indexed_leaf_at(view.data, absolute as usize) {
            Some(index) => {
                let leaf = view.data.leaves[index];
                let at = leaf.page_start as int + absolute - leaf.logical_start as int;
                Some(view.data.pages[leaf.page as int][at])
            },
            None => None,
        }
    }
}

pub proof fn frozen_page_prefix_byte_exact(
    data: FrozenPageDataModel, count: int, absolute: int,
)
    requires
        frozen_multi_data_wf(data),
        0 < count <= data.leaves.len(),
        0 <= absolute < frozen_page_prefix_bytes(data, count).len(),
    ensures {
        let index = frozen_page_prefix_leaf(data, count, absolute);
        let leaf = data.leaves[index];
        &&& 0 <= index < count
        &&& leaf.logical_start as int <= absolute
        &&& absolute < leaf.logical_start as int + leaf.len as int
        &&& frozen_page_prefix_bytes(data, count)[absolute]
            == data.pages[leaf.page as int][
                leaf.page_start as int + absolute - leaf.logical_start as int
            ]
    },
    decreases count,
{
    let prefix = frozen_page_prefix_bytes(data, count - 1);
    let leaf = data.leaves[count - 1];
    assert(leaf.logical_start as int == prefix.len());
    if absolute < prefix.len() {
        frozen_page_prefix_byte_exact(data, count - 1, absolute);
        assert(frozen_page_prefix_bytes(data, count)[absolute] == prefix[absolute]);
    } else {
        assert(absolute < prefix.len() + frozen_page_leaf_bytes(data, count - 1).len());
        assert(frozen_page_prefix_bytes(data, count)[absolute]
            == frozen_page_leaf_bytes(data, count - 1)[absolute - prefix.len()]);
        assert(frozen_page_leaf_bytes(data, count - 1)[absolute - prefix.len()]
            == data.pages[leaf.page as int][
                leaf.page_start as int + absolute - leaf.logical_start as int
            ]);
    }
}

pub proof fn frozen_multi_byte_at_exact(view: FrozenMultiView, offset: usize)
    requires frozen_multi_view_wf(view), offset < view.len,
    ensures frozen_multi_byte_at(view, offset)
        == Some(frozen_multi_view_bytes(view)[offset as int]),
{
    let absolute = view.start as int + offset as int;
    frozen_page_prefix_byte_exact(
        view.data, view.data.leaves.len() as int, absolute,
    );
    assert(frozen_multi_view_bytes(view)[offset as int]
        == frozen_page_transcript(view.data)[absolute]);
}

pub open spec fn frozen_multi_leaf_covers(
    data: FrozenPageDataModel, index: usize, absolute: usize,
) -> bool {
    (index as int) < data.leaves.len()
        && (data.leaves[index as int].logical_start as int) <= (absolute as int)
        && (absolute as int) < (data.leaves[index as int].logical_start as int)
            + (data.leaves[index as int].len as int)
}

pub open spec fn frozen_multi_cursor_wf(cursor: FrozenMultiCursor) -> bool {
    &&& frozen_multi_view_wf(cursor.view)
    &&& cursor.end as int == cursor.view.start as int + cursor.view.len as int
    &&& cursor.view.start <= cursor.absolute <= cursor.end
    &&& (cursor.leaf as int) <= cursor.view.data.leaves.len()
}

pub open spec fn frozen_multi_initial(view: FrozenMultiView) -> Option<FrozenMultiCursor> {
    if !frozen_multi_view_wf(view) {
        None
    } else {
        let leaf = if view.len == 0 {
            view.data.leaves.len() as usize
        } else {
            frozen_page_indexed_leaf_at(view.data, view.start).unwrap() as usize
        };
        Some(FrozenMultiCursor {
            view,
            leaf,
            absolute: view.start,
            end: (view.start as int + view.len as int) as usize,
        })
    }
}

pub open spec fn frozen_multi_locate(cursor: FrozenMultiCursor) -> usize {
    if frozen_multi_leaf_covers(cursor.view.data, cursor.leaf, cursor.absolute) {
        cursor.leaf
    } else if frozen_multi_leaf_covers(
        cursor.view.data, cursor.leaf.saturating_add(1), cursor.absolute,
    ) {
        cursor.leaf.saturating_add(1)
    } else {
        frozen_page_indexed_leaf_at(cursor.view.data, cursor.absolute).unwrap() as usize
    }
}

pub open spec fn frozen_multi_next(
    cursor: FrozenMultiCursor,
) -> (Option<u8>, FrozenMultiCursor) {
    if cursor.absolute == cursor.end {
        (None, cursor)
    } else {
        let leaf_id = frozen_multi_locate(cursor);
        let leaf = cursor.view.data.leaves[leaf_id as int];
        let at = leaf.page_start as int + cursor.absolute as int
            - leaf.logical_start as int;
        let byte = cursor.view.data.pages[leaf.page as int][at];
        (Some(byte), FrozenMultiCursor {
            view: cursor.view,
            leaf: leaf_id,
            absolute: (cursor.absolute as int + 1) as usize,
            end: cursor.end,
        })
    }
}

pub proof fn frozen_multi_locate_exact(cursor: FrozenMultiCursor)
    requires
        frozen_multi_cursor_wf(cursor),
        cursor.absolute < cursor.end,
    ensures frozen_multi_locate(cursor) as int
        == frozen_page_indexed_leaf_at(
            cursor.view.data, cursor.absolute,
        ).unwrap(),
{
    let data = cursor.view.data;
    let expected = frozen_page_prefix_leaf(
        data, data.leaves.len() as int, cursor.absolute as int,
    );
    assert(cursor.absolute < data.len);
    assert(frozen_page_indexed_leaf_at(data, cursor.absolute) == Some(expected));
    assert(0 <= expected < data.leaves.len());
    assert(expected <= usize::MAX as int);
    if frozen_multi_leaf_covers(data, cursor.leaf, cursor.absolute) {
        assert(cursor.leaf as int == expected);
        assert(frozen_multi_locate(cursor) as int == expected);
    } else if frozen_multi_leaf_covers(
        data, cursor.leaf.saturating_add(1), cursor.absolute,
    ) {
        assert(cursor.leaf.saturating_add(1) as int == expected);
        assert(frozen_multi_locate(cursor) as int == expected);
    } else {
        assert((expected as usize) as int == expected);
        assert(frozen_multi_locate(cursor) as int == expected);
    }
    assert(frozen_page_indexed_leaf_at(data, cursor.absolute).unwrap() == expected);
}

pub proof fn frozen_multi_next_projects(cursor: FrozenMultiCursor)
    requires frozen_multi_cursor_wf(cursor),
    ensures {
        let (byte, next) = frozen_multi_next(cursor);
        &&& frozen_multi_cursor_wf(next)
        &&& if cursor.absolute < cursor.end {
            let offset = cursor.absolute as int - cursor.view.start as int;
            &&& byte == Some(frozen_multi_view_bytes(cursor.view)[offset])
            &&& next.absolute as int == cursor.absolute as int + 1
        } else {
            &&& byte.is_none()
            &&& next.absolute == cursor.absolute
        }
    },
{
    if cursor.absolute < cursor.end {
        frozen_multi_locate_exact(cursor);
        let offset = (cursor.absolute as int - cursor.view.start as int) as usize;
        assert(offset < cursor.view.len);
        frozen_multi_byte_at_exact(cursor.view, offset);
        let leaf_id = frozen_multi_locate(cursor);
        let expected = frozen_page_indexed_leaf_at(
            cursor.view.data, cursor.absolute,
        ).unwrap();
        assert(cursor.absolute < cursor.view.data.len);
        assert(frozen_multi_data_wf(cursor.view.data));
        assert(leaf_id as int == expected);
        assert(0 <= expected < cursor.view.data.leaves.len());
        assert((leaf_id as int) < cursor.view.data.leaves.len());
        assert(frozen_multi_next(cursor).0
            == frozen_multi_byte_at(cursor.view, offset));
        let (_, next) = frozen_multi_next(cursor);
        assert(next.leaf == leaf_id);
        assert(next.absolute as int == cursor.absolute as int + 1);
        assert(frozen_multi_cursor_wf(next));
        assert(frozen_multi_next(cursor).0
            == Some(frozen_multi_view_bytes(cursor.view)[offset as int]));
    } else {
        assert(cursor.absolute == cursor.end);
        assert(frozen_multi_next(cursor) == (None, cursor));
    }
}

pub open spec fn frozen_multi_score_op(
    cursor: FrozenMultiCursor, pending_ff: bool,
) -> CursorOp {
    CursorOp::IndexedEscaped {
        bytes: frozen_multi_view_bytes(cursor.view),
        offset: (cursor.absolute as int - cursor.view.start as int) as usize,
        pending_ff,
    }
}

pub proof fn frozen_multi_score_step_projects(
    arena: score::ScoreArena, cursor: FrozenMultiCursor,
)
    requires
        frozen_multi_cursor_wf(cursor),
        score::arena_well_formed(arena),
        cursor.absolute < cursor.end,
    ensures {
        let op = frozen_multi_score_op(cursor, false);
        let state = CursorState { arena, work: seq![op] };
        let (byte, next) = frozen_multi_next(cursor);
        match cursor_step(state) {
            CursorStep::Emit { byte: emitted, next: score_next } => {
                &&& byte == Some(emitted)
                &&& score_next.work == seq![frozen_multi_score_op(
                    next, emitted == 0,
                )]
            },
            _ => false,
        }
    },
{
    frozen_multi_next_projects(cursor);
    assert(frozen_multi_view_bytes(cursor.view).len()
        == cursor.view.len as int);
}

pub open spec fn frozen_two_leaf_data() -> FrozenPageDataModel {
    FrozenPageDataModel {
        pages: seq![Seq::new(32nat, |i: int| 0x41u8), seq![0x00u8]],
        leaves: seq![
            FrozenPageLeafModel {
                page: 0,
                page_start: 0,
                len: 32,
                logical_start: 0,
            },
            FrozenPageLeafModel {
                page: 1,
                page_start: 0,
                len: 1,
                logical_start: 32,
            },
        ],
        logical_pages: seq![0usize, 1usize],
        len: 33,
    }
}

pub open spec fn frozen_two_leaf_crossing_view() -> FrozenMultiView {
    FrozenMultiView { data: frozen_two_leaf_data(), start: 31, len: 2 }
}

pub proof fn frozen_two_leaf_crossing_witness()
    ensures {
        let data = frozen_two_leaf_data();
        let view = frozen_two_leaf_crossing_view();
        let cursor = frozen_multi_initial(view).unwrap();
        let (first, next) = frozen_multi_next(cursor);
        &&& frozen_multi_data_wf(data)
        &&& frozen_multi_view_wf(view)
        &&& frozen_multi_cursor_wf(cursor)
        &&& first == Some(0x41u8)
        &&& next.absolute == 32
        &&& frozen_multi_next(next).0 == Some(0x00u8)
        &&& frozen_multi_view_bytes(view) == seq![0x41u8, 0x00u8]
        &&& match cursor_step(CursorState {
            arena: score::empty_arena(),
            work: seq![frozen_multi_score_op(cursor, false)],
        }) {
            CursorStep::Emit { byte, .. } => byte == 0x41u8,
            _ => false,
        }
        &&& match cursor_step(CursorState {
            arena: score::empty_arena(),
            work: seq![frozen_multi_score_op(next, false)],
        }) {
            CursorStep::Emit { byte, .. } => byte == 0x00u8,
            _ => false,
        }
    },
{
    let data = frozen_two_leaf_data();
    assert(data.leaves.len() == 2);
    assert(data.pages.len() == 2);
    assert(data.logical_pages.len() == 2);
    assert(data.pages[0].len() == 32);
    assert(data.leaves[0].page == 0);
    assert(data.leaves[0].page_start == 0);
    assert(data.leaves[0].len == 32);
    assert(data.pages[0].subrange(0, 32) =~= data.pages[0]);
    assert(frozen_page_leaf_bytes(data, 0) =~= data.pages[0]);
    assert(frozen_page_prefix_bytes(data, 0) =~= Seq::<u8>::empty());
    assert(frozen_page_prefix_bytes(data, 1)
        =~= frozen_page_leaf_bytes(data, 0));
    assert(frozen_page_prefix_bytes(data, 1).len() == 32);
    assert(frozen_page_transcript(data).len() == 33);
    assert forall|i: int| 0 <= i < data.leaves.len() implies {
        let leaf = #[trigger] data.leaves[i];
        &&& leaf.len > 0
        &&& (leaf.page as int) < data.pages.len()
        &&& leaf.page_start as int + leaf.len as int
            <= data.pages[leaf.page as int].len()
        &&& leaf.logical_start as int
            == frozen_page_prefix_bytes(data, i).len()
    } by {
        if i == 0 {
            assert(frozen_page_prefix_bytes(data, 0).len() == 0);
        } else {
            assert(i == 1);
        }
    }
    assert forall|absolute: usize| absolute < data.len implies {
        let found = #[trigger] frozen_page_indexed_leaf_at(data, absolute);
        let expected = frozen_page_prefix_leaf(
            data, data.leaves.len() as int, absolute as int,
        );
        &&& found == Some(expected)
        &&& 0 <= expected < data.leaves.len()
        &&& (data.leaves[expected].logical_start as int) <= (absolute as int)
        &&& (absolute as int) < (data.leaves[expected].logical_start as int)
            + (data.leaves[expected].len as int)
    } by {
        if absolute < 32 {
            assert(absolute as int / 32 == 0);
            assert((absolute as int) < frozen_page_prefix_bytes(data, 1).len());
            assert(frozen_page_prefix_leaf(data, 1, absolute as int) == 0);
            assert(frozen_page_prefix_leaf(data, 2, absolute as int) == 0);
            assert(frozen_page_find_from(data, absolute as int, 0) == Some(0));
        } else {
            assert(absolute == 32);
            assert(frozen_page_prefix_leaf(data, 2, absolute as int) == 1);
            assert(frozen_page_find_from(data, absolute as int, 1) == Some(1));
        }
    }
    assert forall|absolute: usize, i: int|
        #![trigger data.leaves[i], frozen_page_prefix_leaf(data, data.leaves.len() as int, absolute as int)]
        absolute < data.len && 0 <= i < data.leaves.len()
            && (data.leaves[i].logical_start as int) <= (absolute as int)
            && (absolute as int) < (data.leaves[i].logical_start as int)
                + (data.leaves[i].len as int)
        implies i == frozen_page_prefix_leaf(
            data, data.leaves.len() as int, absolute as int,
        ) by {
        if i == 0 {
            assert(data.leaves[i].logical_start == 0);
            assert(data.leaves[i].len == 32);
            assert(absolute < 32);
            assert((absolute as int) < frozen_page_prefix_bytes(data, 1).len());
            assert(frozen_page_prefix_leaf(data, 1, absolute as int) == 0);
            assert(frozen_page_prefix_leaf(data, 2, absolute as int) == 0);
        } else {
            assert(i == 1);
            assert(data.leaves[i].logical_start == 32);
            assert(data.leaves[i].len == 1);
            assert(absolute == 32);
            assert((absolute as int) >= frozen_page_prefix_bytes(data, 1).len());
            assert(frozen_page_prefix_leaf(data, 2, absolute as int) == 1);
        }
    }
    assert(frozen_multi_data_wf(data));
    let view = frozen_two_leaf_crossing_view();
    let cursor = frozen_multi_initial(view).unwrap();
    assert(frozen_multi_view_bytes(view) == seq![0x41u8, 0x00u8]);
    assert(frozen_multi_next(cursor).0 == Some(0x41u8));
    frozen_multi_next_projects(cursor);
    let (_, next) = frozen_multi_next(cursor);
    assert(next.absolute == 32);
    assert(frozen_multi_next(next).0 == Some(0x00u8));
    frozen_multi_next_projects(next);
    frozen_multi_score_step_projects(score::empty_arena(), cursor);
    frozen_multi_score_step_projects(score::empty_arena(), next);
}

pub enum ComposedLeafPayload {
    Borrowed { backing: Seq<u8>, start: usize, len: usize },
    FrozenOwned { view: FrozenOwnedView },
}

pub struct ComposedLeafWindow {
    pub payload: ComposedLeafPayload,
    pub start: usize,
    pub len: usize,
}

pub struct ComposedLeafCursor {
    pub window: ComposedLeafWindow,
    pub at: usize,
    pub remaining: usize,
}

pub open spec fn composed_leaf_payload_wf(payload: ComposedLeafPayload) -> bool {
    match payload {
        ComposedLeafPayload::Borrowed { backing, start, len } =>
            start as int + len as int <= backing.len(),
        ComposedLeafPayload::FrozenOwned { view } =>
            frozen_owned_view_wf(view),
    }
}

pub open spec fn composed_leaf_payload_len(payload: ComposedLeafPayload) -> usize {
    match payload {
        ComposedLeafPayload::Borrowed { len, .. } => len,
        ComposedLeafPayload::FrozenOwned { view } => view.len,
    }
}

pub open spec fn composed_leaf_window_wf(window: ComposedLeafWindow) -> bool {
    composed_leaf_payload_wf(window.payload)
        && window.start as int + window.len as int
            <= composed_leaf_payload_len(window.payload) as int
}

pub open spec fn composed_leaf_window_bytes(window: ComposedLeafWindow) -> Seq<u8> {
    match window.payload {
        ComposedLeafPayload::Borrowed { backing, start, .. } =>
            backing.subrange(
                start as int + window.start as int,
                start as int + window.start as int + window.len as int,
            ),
        ComposedLeafPayload::FrozenOwned { view } =>
            frozen_owned_view_bytes(view).subrange(
                window.start as int, window.start as int + window.len as int,
            ),
    }
}

pub open spec fn composed_leaf_cursor_wf(cursor: ComposedLeafCursor) -> bool {
    composed_leaf_window_wf(cursor.window)
        && cursor.at as int + cursor.remaining as int == cursor.window.len as int
}

pub open spec fn composed_leaf_initial(
    window: ComposedLeafWindow,
) -> Option<ComposedLeafCursor> {
    if composed_leaf_window_wf(window) {
        Some(ComposedLeafCursor {
            window, at: 0, remaining: window.len,
        })
    } else {
        None
    }
}

pub open spec fn composed_leaf_next(
    cursor: ComposedLeafCursor,
) -> (Option<u8>, ComposedLeafCursor) {
    if cursor.remaining == 0 {
        (None, cursor)
    } else {
        let byte = match cursor.window.payload {
            ComposedLeafPayload::Borrowed { backing, start, .. } =>
                backing[start as int + cursor.window.start as int + cursor.at as int],
            ComposedLeafPayload::FrozenOwned { view } =>
                frozen_owned_view_bytes(view)[
                    cursor.window.start as int + cursor.at as int
                ],
        };
        (Some(byte), ComposedLeafCursor {
            window: cursor.window,
            at: (cursor.at as int + 1) as usize,
            remaining: (cursor.remaining as int - 1) as usize,
        })
    }
}

pub open spec fn composed_leaf_score_op(
    cursor: ComposedLeafCursor, pending_ff: bool,
) -> CursorOp {
    CursorOp::IndexedEscaped {
        bytes: composed_leaf_window_bytes(cursor.window),
        offset: cursor.at,
        pending_ff,
    }
}

pub proof fn composed_leaf_next_projects(cursor: ComposedLeafCursor)
    requires composed_leaf_cursor_wf(cursor),
    ensures {
        let bytes = composed_leaf_window_bytes(cursor.window);
        let (byte, next) = composed_leaf_next(cursor);
        &&& composed_leaf_cursor_wf(next)
        &&& if cursor.remaining > 0 {
            &&& byte == Some(bytes[cursor.at as int])
            &&& next.at as int == cursor.at as int + 1
            &&& next.remaining as int + 1 == cursor.remaining as int
        } else {
            &&& byte.is_none()
            &&& next.at == cursor.at
            &&& next.remaining == 0
        }
    },
{
    let window = cursor.window;
    match window.payload {
        ComposedLeafPayload::Borrowed { .. } => {
            assert(composed_leaf_window_bytes(window).len() == window.len as int);
        },
        ComposedLeafPayload::FrozenOwned { view } => {
            assert(frozen_owned_view_bytes(view).len() == view.len as int);
            assert(composed_leaf_window_bytes(window).len() == window.len as int);
        },
    }
}

pub proof fn composed_leaf_score_step_projects(
    arena: score::ScoreArena, cursor: ComposedLeafCursor,
)
    requires
        composed_leaf_cursor_wf(cursor),
        score::arena_well_formed(arena),
        cursor.remaining > 0,
    ensures {
        let op = composed_leaf_score_op(cursor, false);
        let state = CursorState { arena, work: seq![op] };
        let (byte, next) = composed_leaf_next(cursor);
        match cursor_step(state) {
            CursorStep::Emit { byte: emitted, next: score_next } => {
                &&& byte == Some(emitted)
                &&& score_next.work == seq![composed_leaf_score_op(
                    next, emitted == 0,
                )]
            },
            _ => false,
        }
    },
{
    composed_leaf_next_projects(cursor);
    assert(composed_leaf_window_bytes(cursor.window).len()
        == cursor.window.len as int);
}

pub struct ComposedPendingWindow {
    pub subtree_bytes: Seq<u8>,
    pub start: usize,
    pub len: usize,
}

pub open spec fn composed_pending_window_wf(window: ComposedPendingWindow) -> bool {
    window.start as int + window.len as int <= window.subtree_bytes.len()
}

pub open spec fn composed_pending_window_bytes(
    window: ComposedPendingWindow,
) -> Seq<u8> {
    window.subtree_bytes.subrange(
        window.start as int,
        window.start as int + window.len as int,
    )
}

pub open spec fn composed_pending_bytes(
    pending: Seq<ComposedPendingWindow>,
) -> Seq<u8>
    decreases pending.len(),
{
    if pending.len() == 0 {
        Seq::empty()
    } else {
        composed_pending_window_bytes(pending.last())
            + composed_pending_bytes(pending.drop_last())
    }
}

pub open spec fn composed_children_bytes(children: Seq<Seq<u8>>) -> Seq<u8>
    decreases children.len(),
{
    if children.len() == 0 {
        Seq::empty()
    } else {
        composed_children_bytes(children.drop_last()) + children.last()
    }
}

pub open spec fn composed_branch_expand_reverse(
    pending: Seq<ComposedPendingWindow>,
    children: Seq<Seq<u8>>,
    end: int,
    range_start: int,
    range_end: int,
) -> Seq<ComposedPendingWindow>
    decreases children.len(),
{
    if children.len() == 0 {
        pending
    } else {
        let child = children.last();
        let child_start = end - child.len();
        let selected_start = if child_start > range_start {
            child_start
        } else { range_start };
        let selected_end = if end < range_end { end } else { range_end };
        let next_pending = if selected_start < selected_end {
            pending.push(ComposedPendingWindow {
                subtree_bytes: child,
                start: (selected_start - child_start) as usize,
                len: (selected_end - selected_start) as usize,
            })
        } else { pending };
        composed_branch_expand_reverse(
            next_pending, children.drop_last(), child_start,
            range_start, range_end,
        )
    }
}

pub proof fn composed_pending_push(
    pending: Seq<ComposedPendingWindow>, window: ComposedPendingWindow,
)
    ensures composed_pending_bytes(pending.push(window))
        == composed_pending_window_bytes(window) + composed_pending_bytes(pending),
{
    assert(pending.push(window).last() == window);
    assert(pending.push(window).drop_last() =~= pending);
}

pub proof fn composed_branch_full_expand_at(
    pending: Seq<ComposedPendingWindow>, children: Seq<Seq<u8>>,
    range_end: int,
)
    requires
        composed_children_bytes(children).len() <= range_end,
        range_end <= usize::MAX as int,
    ensures composed_pending_bytes(composed_branch_expand_reverse(
        pending, children, composed_children_bytes(children).len() as int,
        0, range_end,
    )) == composed_children_bytes(children) + composed_pending_bytes(pending),
    decreases children.len(),
{
    if children.len() > 0 {
        let prefix = children.drop_last();
        let child = children.last();
        let prefix_len = composed_children_bytes(prefix).len();
        let total_len = composed_children_bytes(children).len();
        assert(total_len == prefix_len + child.len());
        if child.len() > 0 {
            let window = ComposedPendingWindow {
                subtree_bytes: child, start: 0, len: child.len() as usize,
            };
            composed_pending_push(pending, window);
            composed_branch_full_expand_at(pending.push(window), prefix, range_end);
            assert(composed_pending_window_bytes(window) =~= child);
        } else {
            composed_branch_full_expand_at(pending, prefix, range_end);
            Seq::<u8>::add_empty_right(composed_children_bytes(prefix), child);
        }
        vstd::seq_lib::lemma_concat_associative(
            composed_children_bytes(prefix), child, composed_pending_bytes(pending),
        );
    } else {
        Seq::<u8>::add_empty_left(Seq::empty(), composed_pending_bytes(pending));
    }
}

pub proof fn composed_branch_full_expand_preserves(
    pending: Seq<ComposedPendingWindow>, children: Seq<Seq<u8>>,
)
    requires composed_children_bytes(children).len() <= usize::MAX as int,
    ensures composed_pending_bytes(composed_branch_expand_reverse(
        pending, children, composed_children_bytes(children).len() as int,
        0, composed_children_bytes(children).len() as int,
    )) == composed_children_bytes(children) + composed_pending_bytes(pending),
{
    composed_branch_full_expand_at(
        pending, children, composed_children_bytes(children).len() as int,
    );
}

pub open spec fn composed_range_bytes(
    bytes: Seq<u8>, range_start: int, range_end: int,
) -> Seq<u8> {
    let start = if range_start < bytes.len() { range_start } else { bytes.len() as int };
    let end = if range_end < bytes.len() { range_end } else { bytes.len() as int };
    bytes.subrange(start, end)
}

pub open spec fn composed_right_overlap_bytes(
    left: Seq<u8>, right: Seq<u8>,
    range_start: int, range_end: int,
) -> Seq<u8> {
    let child_start = left.len() as int;
    let end = child_start + right.len();
    let selected_start = if child_start > range_start {
        child_start
    } else { range_start };
    let selected_end = if end < range_end { end } else { range_end };
    if selected_start < selected_end {
        right.subrange(
            selected_start - child_start,
            selected_end - child_start,
        )
    } else {
        Seq::empty()
    }
}

pub proof fn composed_range_concat(
    left: Seq<u8>, right: Seq<u8>,
    range_start: int, range_end: int,
)
    requires 0 <= range_start <= range_end,
    ensures composed_range_bytes(left + right, range_start, range_end)
        == composed_range_bytes(left, range_start, range_end)
            + composed_right_overlap_bytes(left, right, range_start, range_end),
{
    assert(composed_range_bytes(left + right, range_start, range_end) =~=
        composed_range_bytes(left, range_start, range_end)
            + composed_right_overlap_bytes(left, right, range_start, range_end));
}

pub proof fn composed_branch_selected_expand_preserves(
    pending: Seq<ComposedPendingWindow>,
    children: Seq<Seq<u8>>,
    range_start: int,
    range_end: int,
)
    requires
        0 <= range_start <= range_end <= usize::MAX as int,
        composed_children_bytes(children).len() <= usize::MAX as int,
    ensures composed_pending_bytes(composed_branch_expand_reverse(
        pending, children, composed_children_bytes(children).len() as int,
        range_start, range_end,
    )) == composed_range_bytes(
        composed_children_bytes(children), range_start, range_end,
    ) + composed_pending_bytes(pending),
    decreases children.len(),
{
    if children.len() > 0 {
        let prefix = children.drop_last();
        let child = children.last();
        let left = composed_children_bytes(prefix);
        let child_start = left.len() as int;
        let end = composed_children_bytes(children).len() as int;
        let selected_start = if child_start > range_start {
            child_start
        } else { range_start };
        let selected_end = if end < range_end { end } else { range_end };
        assert(composed_children_bytes(children) == left + child);
        if selected_start < selected_end {
            let window = ComposedPendingWindow {
                subtree_bytes: child,
                start: (selected_start - child_start) as usize,
                len: (selected_end - selected_start) as usize,
            };
            composed_pending_push(pending, window);
            composed_branch_selected_expand_preserves(
                pending.push(window), prefix, range_start, range_end,
            );
            assert(composed_pending_window_bytes(window)
                == composed_right_overlap_bytes(
                    left, child, range_start, range_end,
                ));
        } else {
            composed_branch_selected_expand_preserves(
                pending, prefix, range_start, range_end,
            );
            assert(composed_right_overlap_bytes(
                left, child, range_start, range_end,
            ) == Seq::<u8>::empty());
        }
        composed_range_concat(left, child, range_start, range_end);
        vstd::seq_lib::lemma_concat_associative(
            composed_range_bytes(left, range_start, range_end),
            composed_right_overlap_bytes(left, child, range_start, range_end),
            composed_pending_bytes(pending),
        );
    } else {
        Seq::<u8>::add_empty_left(Seq::empty(), composed_pending_bytes(pending));
    }
}

pub struct ComposedActiveState {
    pub active: Option<ComposedLeafCursor>,
    pub pending: Seq<ComposedPendingWindow>,
    pub remaining: usize,
}

pub open spec fn composed_active_bytes(state: ComposedActiveState) -> Seq<u8> {
    match state.active {
        Some(cursor) => composed_leaf_window_bytes(cursor.window).subrange(
            cursor.at as int, cursor.window.len as int,
        ) + composed_pending_bytes(state.pending),
        None => composed_pending_bytes(state.pending),
    }
}

pub open spec fn composed_active_state_wf(state: ComposedActiveState) -> bool {
    &&& forall|i: int| 0 <= i < state.pending.len()
        ==> composed_pending_window_wf(state.pending[i])
    &&& match state.active {
        Some(cursor) => composed_leaf_cursor_wf(cursor),
        None => true,
    }
    &&& state.remaining as int == composed_active_bytes(state).len()
}

pub open spec fn composed_active_next(
    state: ComposedActiveState,
) -> (Option<u8>, ComposedActiveState) {
    match state.active {
        Some(cursor) if cursor.remaining > 0 => {
            let (byte, next) = composed_leaf_next(cursor);
            (byte, ComposedActiveState {
                active: Some(next),
                pending: state.pending,
                remaining: (state.remaining as int - 1) as usize,
            })
        },
        _ => (None, state),
    }
}

pub open spec fn composed_discard_exhausted(
    state: ComposedActiveState,
) -> ComposedActiveState {
    ComposedActiveState {
        active: None,
        pending: state.pending,
        remaining: state.remaining,
    }
}

pub proof fn composed_discard_exhausted_conserves(
    state: ComposedActiveState,
)
    requires
        composed_active_state_wf(state),
        state.active.is_some(),
        state.active.unwrap().remaining == 0,
    ensures
        composed_active_bytes(composed_discard_exhausted(state))
            == composed_active_bytes(state),
        composed_active_state_wf(composed_discard_exhausted(state)),
{
    let cursor = state.active.unwrap();
    assert(cursor.at == cursor.window.len);
    assert(composed_leaf_window_bytes(cursor.window).subrange(
        cursor.window.len as int, cursor.window.len as int,
    ) == Seq::<u8>::empty());
    Seq::<u8>::add_empty_left(
        Seq::empty(), composed_pending_bytes(state.pending),
    );
}

pub proof fn composed_active_next_conserves(
    state: ComposedActiveState,
)
    requires
        composed_active_state_wf(state),
        state.active.is_some(),
        state.active.unwrap().remaining > 0,
    ensures {
        let (byte, next) = composed_active_next(state);
        &&& byte.is_some()
        &&& composed_active_state_wf(next)
        &&& composed_active_bytes(state)
            == seq![byte.unwrap()] + composed_active_bytes(next)
        &&& next.remaining as int + 1 == state.remaining as int
    },
{
    let cursor = state.active.unwrap();
    let bytes = composed_leaf_window_bytes(cursor.window);
    let (byte, next_cursor) = composed_leaf_next(cursor);
    composed_leaf_next_projects(cursor);
    assert(bytes.len() == cursor.window.len as int);
    cursor_slice_head_exact(bytes, cursor.at);
    assert(composed_leaf_window_bytes(next_cursor.window) == bytes);
    vstd::seq_lib::lemma_concat_associative(
        seq![byte.unwrap()],
        bytes.subrange(cursor.at as int + 1, bytes.len() as int),
        composed_pending_bytes(state.pending),
    );
}

pub open spec fn composed_branch_parent_window(
    children: Seq<Seq<u8>>, start: usize, len: usize,
) -> ComposedPendingWindow {
    ComposedPendingWindow {
        subtree_bytes: composed_children_bytes(children),
        start,
        len,
    }
}

pub open spec fn composed_branch_pop(
    state: ComposedActiveState,
    children: Seq<Seq<u8>>,
) -> ComposedActiveState {
    let parent = state.pending.last();
    let prefix = state.pending.drop_last();
    ComposedActiveState {
        active: None,
        pending: composed_branch_expand_reverse(
            prefix, children, composed_children_bytes(children).len() as int,
            parent.start as int, parent.start as int + parent.len as int,
        ),
        remaining: state.remaining,
    }
}

pub proof fn composed_branch_expand_windows_wf(
    pending: Seq<ComposedPendingWindow>,
    children: Seq<Seq<u8>>,
    range_start: int,
    range_end: int,
)
    requires
        0 <= range_start <= range_end <= usize::MAX as int,
        composed_children_bytes(children).len() <= usize::MAX as int,
        forall|i: int| 0 <= i < pending.len()
            ==> composed_pending_window_wf(pending[i]),
    ensures forall|i: int|
        0 <= i < composed_branch_expand_reverse(
            pending, children, composed_children_bytes(children).len() as int,
            range_start, range_end,
        ).len() ==> composed_pending_window_wf(
            composed_branch_expand_reverse(
                pending, children, composed_children_bytes(children).len() as int,
                range_start, range_end,
            )[i],
        ),
    decreases children.len(),
{
    let expanded = composed_branch_expand_reverse(
        pending, children, composed_children_bytes(children).len() as int,
        range_start, range_end,
    );
    if children.len() > 0 {
        let prefix = children.drop_last();
        let child = children.last();
        let child_start = composed_children_bytes(prefix).len() as int;
        let end = composed_children_bytes(children).len() as int;
        let selected_start = if child_start > range_start {
            child_start
        } else { range_start };
        let selected_end = if end < range_end { end } else { range_end };
        assert(end == child_start + child.len());
        if selected_start < selected_end {
            let window = ComposedPendingWindow {
                subtree_bytes: child,
                start: (selected_start - child_start) as usize,
                len: (selected_end - selected_start) as usize,
            };
            assert(composed_pending_window_wf(window));
            assert forall|i: int| 0 <= i < pending.push(window).len()
                implies composed_pending_window_wf(pending.push(window)[i]) by {}
            composed_branch_expand_windows_wf(
                pending.push(window), prefix, range_start, range_end,
            );
            assert(expanded == composed_branch_expand_reverse(
                pending.push(window), prefix, child_start,
                range_start, range_end,
            ));
            assert forall|i: int| 0 <= i < expanded.len()
                implies composed_pending_window_wf(expanded[i]) by {}
        } else {
            composed_branch_expand_windows_wf(
                pending, prefix, range_start, range_end,
            );
            assert(expanded == composed_branch_expand_reverse(
                pending, prefix, child_start,
                range_start, range_end,
            ));
            assert forall|i: int| 0 <= i < expanded.len()
                implies composed_pending_window_wf(expanded[i]) by {}
        }
    } else {
        assert(expanded == pending);
        assert forall|i: int| 0 <= i < expanded.len()
            implies composed_pending_window_wf(expanded[i]) by {}
    }
}

pub proof fn composed_branch_pop_conserves(
    state: ComposedActiveState,
    children: Seq<Seq<u8>>,
)
    requires
        composed_active_state_wf(state),
        state.active.is_none(),
        state.pending.len() > 0,
        children.len() == 2 || children.len() == 3,
        composed_children_bytes(children).len() <= usize::MAX as int,
        state.pending.last() == composed_branch_parent_window(
            children, state.pending.last().start, state.pending.last().len,
        ),
        composed_pending_window_wf(state.pending.last()),
    ensures
        composed_active_bytes(composed_branch_pop(state, children))
            == composed_active_bytes(state),
        composed_active_state_wf(composed_branch_pop(state, children)),
{
    let parent = state.pending.last();
    let prefix = state.pending.drop_last();
    let bytes = composed_children_bytes(children);
    assert(parent.start as int + parent.len as int <= bytes.len());
    composed_pending_push(prefix, parent);
    composed_branch_selected_expand_preserves(
        prefix, children,
        parent.start as int, parent.start as int + parent.len as int,
    );
    composed_branch_expand_windows_wf(
        prefix, children,
        parent.start as int, parent.start as int + parent.len as int,
    );
    assert(composed_range_bytes(
        bytes, parent.start as int, parent.start as int + parent.len as int,
    ) == composed_pending_window_bytes(parent));
}

pub enum ComposedSourceNodeKind {
    Borrowed { backing: Seq<u8>, start: usize, len: usize },
    FrozenOwned { view: FrozenOwnedView },
    Branch2 { left: usize, right: usize },
    Branch3 { first: usize, second: usize, third: usize },
}

pub struct ComposedSourceNode {
    pub kind: ComposedSourceNodeKind,
    pub len: usize,
}

pub struct ComposedSourceArena {
    pub nodes: Seq<ComposedSourceNode>,
}

pub open spec fn composed_source_node_bytes(
    arena: ComposedSourceArena, node_id: usize,
) -> Seq<u8>
    decreases node_id,
{
    if (node_id as int) >= arena.nodes.len() {
        Seq::empty()
    } else {
        match arena.nodes[node_id as int].kind {
            ComposedSourceNodeKind::Borrowed { backing, start, len } =>
                backing.subrange(start as int, start as int + len as int),
            ComposedSourceNodeKind::FrozenOwned { view } =>
                frozen_owned_view_bytes(view),
            ComposedSourceNodeKind::Branch2 { left, right } => {
                if left < node_id && right < node_id {
                    composed_source_node_bytes(arena, left)
                        + composed_source_node_bytes(arena, right)
                } else { Seq::empty() }
            },
            ComposedSourceNodeKind::Branch3 { first, second, third } => {
                if first < node_id && second < node_id && third < node_id {
                    composed_source_node_bytes(arena, first)
                        + composed_source_node_bytes(arena, second)
                        + composed_source_node_bytes(arena, third)
                } else { Seq::empty() }
            },
        }
    }
}

pub open spec fn composed_source_node_wf(
    arena: ComposedSourceArena, node_id: usize,
) -> bool {
    &&& (node_id as int) < arena.nodes.len()
    &&& match arena.nodes[node_id as int].kind {
        ComposedSourceNodeKind::Borrowed { backing, start, len } =>
            start as int + len as int <= backing.len(),
        ComposedSourceNodeKind::FrozenOwned { view } =>
            frozen_owned_view_wf(view),
        ComposedSourceNodeKind::Branch2 { left, right } =>
            left < node_id && right < node_id,
        ComposedSourceNodeKind::Branch3 { first, second, third } =>
            first < node_id && second < node_id && third < node_id,
    }
    &&& arena.nodes[node_id as int].len as int
        == composed_source_node_bytes(arena, node_id).len()
}

pub open spec fn composed_source_arena_wf(arena: ComposedSourceArena) -> bool {
    arena.nodes.len() <= usize::MAX as int
        && forall|i: usize| (i as int) < arena.nodes.len()
            ==> #[trigger] composed_source_node_wf(arena, i)
}

pub open spec fn composed_source_branch_children(
    arena: ComposedSourceArena, node_id: usize,
) -> Seq<Seq<u8>> {
    match arena.nodes[node_id as int].kind {
        ComposedSourceNodeKind::Branch2 { left, right } =>
            seq![
                composed_source_node_bytes(arena, left),
                composed_source_node_bytes(arena, right),
            ],
        ComposedSourceNodeKind::Branch3 { first, second, third } =>
            seq![
                composed_source_node_bytes(arena, first),
                composed_source_node_bytes(arena, second),
                composed_source_node_bytes(arena, third),
            ],
        _ => Seq::empty(),
    }
}

pub open spec fn composed_source_branch_kind(
    arena: ComposedSourceArena, node_id: usize,
) -> bool {
    match arena.nodes[node_id as int].kind {
        ComposedSourceNodeKind::Branch2 { .. }
        | ComposedSourceNodeKind::Branch3 { .. } => true,
        _ => false,
    }
}

pub proof fn composed_source_branch_children_exact(
    arena: ComposedSourceArena, node_id: usize,
)
    requires
        composed_source_arena_wf(arena),
        (node_id as int) < arena.nodes.len(),
        composed_source_branch_kind(arena, node_id),
    ensures {
        let children = composed_source_branch_children(arena, node_id);
        &&& (children.len() == 2 || children.len() == 3)
        &&& composed_children_bytes(children)
            == composed_source_node_bytes(arena, node_id)
        &&& composed_children_bytes(children).len()
            == arena.nodes[node_id as int].len as int
        &&& composed_children_bytes(children).len() <= usize::MAX as int
    },
{
    reveal_with_fuel(composed_children_bytes, 4);
    assert(composed_source_node_wf(arena, node_id));
    let children = composed_source_branch_children(arena, node_id);
    match arena.nodes[node_id as int].kind {
        ComposedSourceNodeKind::Branch2 { left, right } => {
            assert(left < node_id && right < node_id);
            let left_bytes = composed_source_node_bytes(arena, left);
            let right_bytes = composed_source_node_bytes(arena, right);
            assert(children == seq![
                left_bytes, right_bytes,
            ]);
            assert(children.drop_last() =~= seq![left_bytes]);
            assert(children.drop_last().drop_last() =~= Seq::<Seq<u8>>::empty());
            assert(children.drop_last() == seq![left_bytes]);
            assert(seq![left_bytes].drop_last() == Seq::<Seq<u8>>::empty());
            assert(composed_children_bytes(seq![left_bytes])
                == composed_children_bytes(Seq::<Seq<u8>>::empty()) + left_bytes);
            Seq::<u8>::add_empty_left(Seq::empty(), left_bytes);
            assert(composed_children_bytes(children.drop_last()) == left_bytes);
            assert(composed_children_bytes(children)
                == left_bytes + right_bytes);
        },
        ComposedSourceNodeKind::Branch3 { first, second, third } => {
            assert(first < node_id && second < node_id && third < node_id);
            let first_bytes = composed_source_node_bytes(arena, first);
            let second_bytes = composed_source_node_bytes(arena, second);
            let third_bytes = composed_source_node_bytes(arena, third);
            assert(children == seq![
                first_bytes, second_bytes, third_bytes,
            ]);
            assert(children.drop_last() =~= seq![first_bytes, second_bytes]);
            assert(children.drop_last() == seq![first_bytes, second_bytes]);
            assert(children.drop_last().drop_last() =~= seq![first_bytes]);
            assert(children.drop_last().drop_last().drop_last()
                =~= Seq::<Seq<u8>>::empty());
            assert(children.drop_last().drop_last() == seq![first_bytes]);
            assert(seq![first_bytes].drop_last() == Seq::<Seq<u8>>::empty());
            assert(composed_children_bytes(seq![first_bytes])
                == composed_children_bytes(Seq::<Seq<u8>>::empty()) + first_bytes);
            Seq::<u8>::add_empty_left(Seq::empty(), first_bytes);
            assert(composed_children_bytes(children.drop_last().drop_last())
                == first_bytes);
            assert(seq![first_bytes, second_bytes].drop_last()
                == seq![first_bytes]);
            assert(composed_children_bytes(seq![first_bytes, second_bytes])
                == composed_children_bytes(seq![first_bytes]) + second_bytes);
            assert(composed_children_bytes(children.drop_last())
                == first_bytes + second_bytes);
            vstd::seq_lib::lemma_concat_associative(
                first_bytes, second_bytes, third_bytes,
            );
            assert(composed_children_bytes(children)
                == first_bytes + second_bytes + third_bytes);
        },
        _ => assert(false),
    }
    assert(composed_children_bytes(children)
        == composed_source_node_bytes(arena, node_id));
    assert(composed_children_bytes(children).len()
        == arena.nodes[node_id as int].len as int);
}

pub proof fn composed_source_branch_pop_conserves(
    arena: ComposedSourceArena,
    node_id: usize,
    state: ComposedActiveState,
)
    requires
        composed_source_arena_wf(arena),
        (node_id as int) < arena.nodes.len(),
        composed_source_branch_kind(arena, node_id),
        composed_active_state_wf(state),
        state.active.is_none(),
        state.pending.len() > 0,
        state.pending.last().subtree_bytes
            == composed_source_node_bytes(arena, node_id),
    ensures {
        let children = composed_source_branch_children(arena, node_id);
        let next = composed_branch_pop(state, children);
        &&& composed_active_bytes(next) == composed_active_bytes(state)
        &&& composed_active_state_wf(next)
    },
{
    composed_source_branch_children_exact(arena, node_id);
    let children = composed_source_branch_children(arena, node_id);
    assert(state.pending.last() == composed_branch_parent_window(
        children, state.pending.last().start, state.pending.last().len,
    ));
    composed_branch_pop_conserves(state, children);
}

pub proof fn cursor_initial_transcript(
    arena: score::ScoreArena, handle: usize,
)
    requires cursor_initial(arena, handle).is_some(),
    ensures
        cursor_state_wf(cursor_initial(arena, handle).unwrap()),
        cursor_work_bytes(
            arena, cursor_initial(arena, handle).unwrap().work,
        ) == score::handle_transcript(arena, handle).unwrap(),
{
    let initial = cursor_initial(arena, handle).unwrap();
    assert(initial.work == seq![CursorOp::Score { handle }]);
    cursor_work_empty(arena);
    cursor_work_push(arena, Seq::empty(), CursorOp::Score { handle });
    assert(cursor_op_bytes(arena, CursorOp::Score { handle })
        == arena.entries[handle as int].transcript);
    Seq::<u8>::add_empty_right(
        arena.entries[handle as int].transcript, Seq::empty(),
    );
    assert(cursor_work_bytes(arena, initial.work)
        == arena.entries[handle as int].transcript);
    assert(score::handle_transcript(arena, handle)
        == Some(arena.entries[handle as int].transcript));
}

pub proof fn cursor_literal_byte_exact(
    arena: score::ScoreArena, work: Seq<CursorOp>, byte: u8,
)
    requires score::arena_well_formed(arena),
    ensures {
        let before = CursorState {
            arena, work: work.push(CursorOp::Byte { byte }),
        };
        match cursor_step(before) {
            CursorStep::Emit { byte: emitted, next } => {
                &&& emitted == byte
                &&& next.work == work
                &&& cursor_work_bytes(arena, before.work)
                    == seq![byte] + cursor_work_bytes(arena, next.work)
            },
            _ => false,
        }
    },
{}

pub proof fn cursor_integer_score_expands_exactly(
    arena: score::ScoreArena, handle: usize, value: i64,
)
    requires
        score::arena_well_formed(arena),
        (handle as int) < arena.entries.len(),
        arena.entries[handle as int].kind == score::ScoreNode::Integer(value),
    ensures {
        let before = CursorState {
            arena, work: seq![CursorOp::Score { handle }],
        };
        match cursor_step(before) {
            CursorStep::Silent { next } =>
                cursor_work_bytes(arena, next.work)
                    == score::integer_transcript(value),
            _ => false,
        }
    },
{
    assert(score::entry_well_formed(arena.entries, handle as int));
    let before = CursorState {
        arena, work: seq![CursorOp::Score { handle }],
    };
    let word = score::big_endian_eight(score::sign_flipped_word(value));
    assert(score::integer_transcript(value) == seq![1u8, 1u8] + word);
    assert(word.subrange(0, word.len() as int) =~= word);
    cursor_work_empty(arena);
    cursor_work_push(arena, Seq::empty(), CursorOp::Integer {
        bytes: word, offset: 0,
    });
    Seq::<u8>::add_empty_right(word, Seq::empty());
    match cursor_step(before) {
        CursorStep::Silent { next } => {
            assert(next.work == seq![
                CursorOp::Integer { bytes: word, offset: 0 },
                CursorOp::Slice { bytes: seq![1u8, 1u8], offset: 0 },
            ]);
            assert(cursor_op_bytes(arena, CursorOp::Integer {
                bytes: word, offset: 0,
            }) == word);
            assert(cursor_work_bytes(arena, seq![
                CursorOp::Integer { bytes: word, offset: 0 },
            ]) == word);
            cursor_work_push(arena, seq![CursorOp::Integer {
                bytes: word, offset: 0,
            }], CursorOp::Slice { bytes: seq![1u8, 1u8], offset: 0 });
            assert(next.work =~= seq![CursorOp::Integer {
                bytes: word, offset: 0,
            }].push(CursorOp::Slice {
                bytes: seq![1u8, 1u8], offset: 0,
            }));
            assert(seq![1u8, 1u8].subrange(0, 2) =~= seq![1u8, 1u8]);
            assert(cursor_op_bytes(arena, CursorOp::Slice {
                bytes: seq![1u8, 1u8], offset: 0,
            }) == seq![1u8, 1u8]);
            assert(cursor_work_bytes(arena, next.work)
                == cursor_op_bytes(arena, CursorOp::Slice {
                    bytes: seq![1u8, 1u8], offset: 0,
                }) + cursor_work_bytes(arena, seq![
                    CursorOp::Integer { bytes: word, offset: 0 },
                ]));
            assert(cursor_work_bytes(arena, next.work)
                == seq![1u8, 1u8] + word);
        },
        _ => assert(false),
    }
}

pub open spec fn first_difference_loop(
    expected: Seq<u8>, claimed: Seq<u8>, at: int,
) -> Option<int>
    decreases expected.len() - at,
{
    if at < 0 {
        Some(0)
    } else if at >= expected.len() || at >= claimed.len() {
        if at == expected.len() && at == claimed.len() {
            None
        } else { Some(at) }
    } else if expected[at] != claimed[at] {
        Some(at)
    } else {
        first_difference_loop(expected, claimed, at + 1)
    }
}

pub proof fn first_difference_loop_prefix(
    expected: Seq<u8>, claimed: Seq<u8>, at: int,
)
    requires
        0 <= at <= expected.len(),
        at <= claimed.len(),
        forall|i: int| 0 <= i < at ==> expected[i] == claimed[i],
    ensures match first_difference_loop(expected, claimed, at) {
        None => expected == claimed,
        Some(first) => {
            &&& at <= first
            &&& first <= expected.len()
            &&& first <= claimed.len()
            &&& forall|i: int| 0 <= i < first
                ==> expected[i] == claimed[i]
            &&& (first == expected.len()
                || first == claimed.len()
                || expected[first] != claimed[first])
        },
    },
    decreases expected.len() - at,
{
    if at < expected.len() && at < claimed.len()
        && expected[at] == claimed[at]
    {
        assert(forall|i: int| 0 <= i < at + 1
            ==> expected[i] == claimed[i]);
        first_difference_loop_prefix(expected, claimed, at + 1);
    }
}

pub proof fn equal_scores_have_no_difference(
    expected: Seq<u8>, claimed: Seq<u8>, at: int,
)
    requires expected == claimed, 0 <= at <= expected.len(),
    ensures first_difference_loop(expected, claimed, at).is_none(),
    decreases expected.len() - at,
{
    if at < expected.len() {
        equal_scores_have_no_difference(expected, claimed, at + 1);
    }
}

pub proof fn first_difference_loop_exact(expected: Seq<u8>, claimed: Seq<u8>)
    ensures
        first_difference_loop(expected, claimed, 0).is_none()
            == (expected == claimed),
        match first_difference_loop(expected, claimed, 0) {
            None => true,
            Some(first) => {
                &&& first <= expected.len()
                &&& first <= claimed.len()
                &&& forall|i: int| 0 <= i < first
                    ==> expected[i] == claimed[i]
                &&& (first == expected.len()
                    || first == claimed.len()
                    || expected[first] != claimed[first])
            },
        },
{
    first_difference_loop_prefix(expected, claimed, 0);
    if first_difference_loop(expected, claimed, 0).is_none() {
        assert forall|i: int| 0 <= i < expected.len()
            implies expected[i] == claimed[i] by {}
        assert(expected == claimed);
    } else if expected == claimed {
        equal_scores_have_no_difference(expected, claimed, 0);
    }
}

pub proof fn score_handle_first_difference_exact(
    arena: score::ScoreArena, handle: usize, claimed: Seq<u8>,
)
    requires
        score::arena_well_formed(arena),
        (handle as int) < arena.entries.len(),
    ensures
        first_difference_loop(
            arena.entries[handle as int].transcript, claimed, 0,
        ).is_none() == score::claimed_score_matches(arena, handle, claimed),
{
    first_difference_loop_exact(
        arena.entries[handle as int].transcript, claimed,
    );
}

}

fn main() {}
