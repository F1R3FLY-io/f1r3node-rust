use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
mod rpb3_score_transcript;
#[cfg(not(rpb3_integrated))]
use rpb3_score_transcript as score;

#[cfg(rpb3_integrated)]
use crate::rpb3_score_transcript as score;

verus! {

pub enum RuntimeScoreKind {
    Integer(i64),
    StringOwned(Seq<u8>),
    StringShared(Seq<u8>),
    StringRegion(Seq<u8>),
    StringIndexed(Seq<u8>),
    BytesOwned(Seq<u8>),
    BytesShared(Seq<u8>),
    BytesRegion(Seq<u8>),
    BytesIndexed(Seq<u8>),
    Node(Seq<usize>),
}

pub struct RuntimeScoreEntry {
    pub kind: RuntimeScoreKind,
    pub encoded_len: usize,
}

pub struct RuntimeScoreArena {
    pub entries: Seq<RuntimeScoreEntry>,
}

pub open spec fn runtime_kind_image(kind: RuntimeScoreKind) -> score::ScoreNode {
    match kind {
        RuntimeScoreKind::Integer(value) => score::ScoreNode::Integer(value),
        RuntimeScoreKind::StringOwned(bytes) | RuntimeScoreKind::StringShared(bytes)
            | RuntimeScoreKind::StringRegion(bytes) | RuntimeScoreKind::StringIndexed(bytes) =>
                score::ScoreNode::StringAtom(bytes),
        RuntimeScoreKind::BytesOwned(bytes) | RuntimeScoreKind::BytesShared(bytes)
            | RuntimeScoreKind::BytesRegion(bytes) | RuntimeScoreKind::BytesIndexed(bytes) =>
            score::ScoreNode::BytesAtom(bytes),
        RuntimeScoreKind::Node(children) => score::ScoreNode::Node(children),
    }
}

pub open spec fn runtime_projects_score(
    runtime: RuntimeScoreArena, transcript: score::ScoreArena,
) -> bool {
    &&& score::arena_well_formed(transcript)
    &&& runtime.entries.len() == transcript.entries.len()
    &&& forall|i: int| 0 <= i < runtime.entries.len() ==> {
        &&& runtime_kind_image(runtime.entries[i].kind) == transcript.entries[i].kind
        &&& runtime.entries[i].encoded_len == transcript.entries[i].encoded_len
    }
}

pub open spec fn runtime_children_len(
    entries: Seq<RuntimeScoreEntry>, children: Seq<usize>,
) -> int
    decreases children.len(),
{
    if children.len() == 0 {
        0
    } else if (children[0] as int) < entries.len() {
        entries[children[0] as int].encoded_len as int
            + runtime_children_len(entries, children.drop_first())
    } else {
        0
    }
}

pub open spec fn runtime_score_len(
    entries: Seq<RuntimeScoreEntry>, kind: RuntimeScoreKind,
) -> int {
    match kind {
        RuntimeScoreKind::Integer(_) => 10,
        RuntimeScoreKind::StringOwned(bytes) | RuntimeScoreKind::StringShared(bytes)
            | RuntimeScoreKind::StringRegion(bytes) | RuntimeScoreKind::StringIndexed(bytes)
            | RuntimeScoreKind::BytesOwned(bytes) | RuntimeScoreKind::BytesShared(bytes)
            | RuntimeScoreKind::BytesRegion(bytes) | RuntimeScoreKind::BytesIndexed(bytes) =>
            4 + bytes.len() + score::zero_count(bytes),
        RuntimeScoreKind::Node(children) => 2 + runtime_children_len(entries, children),
    }
}

pub open spec fn runtime_push_success(
    before: RuntimeScoreArena, after: RuntimeScoreArena,
    kind: RuntimeScoreKind, handle: usize,
) -> bool {
    &&& before.entries.len() < usize::MAX as int
    &&& 0 <= runtime_score_len(before.entries, kind) <= usize::MAX as int
    &&& handle as int == before.entries.len()
    &&& after.entries == before.entries.push(RuntimeScoreEntry {
        kind,
        encoded_len: runtime_score_len(before.entries, kind) as usize,
    })
}

pub enum RuntimeAppendOutcome {
    LengthOverflow(RuntimeScoreArena),
    ReservationFailed(RuntimeScoreArena),
    Appended(RuntimeScoreArena, usize),
}

pub open spec fn runtime_append_attempt(
    before: RuntimeScoreArena, kind: RuntimeScoreKind, reservation_granted: bool,
) -> RuntimeAppendOutcome {
    if before.entries.len() >= usize::MAX as int
        || runtime_score_len(before.entries, kind) > usize::MAX as int
    {
        RuntimeAppendOutcome::LengthOverflow(before)
    } else if !reservation_granted {
        RuntimeAppendOutcome::ReservationFailed(before)
    } else {
        RuntimeAppendOutcome::Appended(
            RuntimeScoreArena {
                entries: before.entries.push(RuntimeScoreEntry {
                    kind,
                    encoded_len: runtime_score_len(before.entries, kind) as usize,
                }),
            },
            before.entries.len() as usize,
        )
    }
}

pub proof fn runtime_children_len_projects(
    runtime: RuntimeScoreArena, transcript: score::ScoreArena, children: Seq<usize>,
)
    requires
        runtime_projects_score(runtime, transcript),
        score::child_handles_valid(transcript.entries, children),
    ensures runtime_children_len(runtime.entries, children)
        == score::children_encoded_len(transcript.entries, children),
    decreases children.len(),
{
    if children.len() > 0 {
        assert(score::child_handles_valid(transcript.entries, children.drop_first()));
        runtime_children_len_projects(runtime, transcript, children.drop_first());
    }
}

pub proof fn runtime_score_len_projects(
    runtime: RuntimeScoreArena, transcript: score::ScoreArena, kind: RuntimeScoreKind,
)
    requires
        runtime_projects_score(runtime, transcript),
        score::kind_valid(transcript.entries, runtime_kind_image(kind)),
    ensures runtime_score_len(runtime.entries, kind)
        == score::kind_transcript(transcript.entries, runtime_kind_image(kind)).len(),
{
    match kind {
        RuntimeScoreKind::Integer(value) => score::integer_transcript_exact(value),
        RuntimeScoreKind::StringOwned(bytes) | RuntimeScoreKind::StringShared(bytes)
            | RuntimeScoreKind::StringRegion(bytes) | RuntimeScoreKind::StringIndexed(bytes)
            | RuntimeScoreKind::BytesOwned(bytes) | RuntimeScoreKind::BytesShared(bytes)
            | RuntimeScoreKind::BytesRegion(bytes) | RuntimeScoreKind::BytesIndexed(bytes) =>
            score::string_and_bytes_transcripts_exact(bytes),
        RuntimeScoreKind::Node(children) => {
            runtime_children_len_projects(runtime, transcript, children);
            assert forall|i: int| 0 <= i < transcript.entries.len()
                implies transcript.entries[i].encoded_len as int
                    == transcript.entries[i].transcript.len() by {
                assert(score::entry_well_formed(transcript.entries, i));
            }
            score::node_transcript_exact(transcript.entries, children);
        },
    }
}

pub proof fn runtime_push_refines_score_append(
    before: RuntimeScoreArena, transcript: score::ScoreArena,
    after: RuntimeScoreArena, kind: RuntimeScoreKind, handle: usize,
)
    requires
        runtime_projects_score(before, transcript),
        score::kind_valid(transcript.entries, runtime_kind_image(kind)),
        runtime_push_success(before, after, kind, handle),
    ensures match score::checked_append(transcript, runtime_kind_image(kind)) {
        Some((next, projected_handle)) => {
            projected_handle == handle && runtime_projects_score(after, next)
        },
        None => false,
    },
{
    runtime_score_len_projects(before, transcript, kind);
    assert(0 <= runtime_score_len(before.entries, kind));
    score::checked_append_exact(transcript, runtime_kind_image(kind));
    if let Some((next, projected_handle)) = score::checked_append(
        transcript, runtime_kind_image(kind),
    ) {
        assert forall|i: int| 0 <= i < after.entries.len()
            implies runtime_kind_image(after.entries[i].kind) == next.entries[i].kind
                && after.entries[i].encoded_len == next.entries[i].encoded_len by {
            if i < before.entries.len() {
                assert(after.entries[i] == before.entries[i]);
                assert(next.entries[i] == transcript.entries[i]);
            } else {
                assert(i == before.entries.len());
            }
        }
    }
}

pub proof fn runtime_append_attempt_refines_score_append(
    before: RuntimeScoreArena, transcript: score::ScoreArena,
    kind: RuntimeScoreKind, reservation_granted: bool,
)
    requires
        runtime_projects_score(before, transcript),
        score::kind_valid(transcript.entries, runtime_kind_image(kind)),
    ensures match runtime_append_attempt(before, kind, reservation_granted) {
        RuntimeAppendOutcome::LengthOverflow(after) => {
            &&& after.entries == before.entries
            &&& match score::checked_append(transcript, runtime_kind_image(kind)) {
                None => true,
                Some(_) => false,
            }
        },
        RuntimeAppendOutcome::ReservationFailed(after) => {
            &&& after.entries == before.entries
            &&& runtime_projects_score(after, transcript)
            &&& match score::checked_append(transcript, runtime_kind_image(kind)) {
                Some(_) => true,
                None => false,
            }
        },
        RuntimeAppendOutcome::Appended(after, handle) => {
            &&& runtime_push_success(before, after, kind, handle)
            &&& match score::checked_append(transcript, runtime_kind_image(kind)) {
                Some((next, projected_handle)) => {
                    projected_handle == handle && runtime_projects_score(after, next)
                },
                None => false,
            }
        },
    },
{
    runtime_score_len_projects(before, transcript, kind);
    assert(0 <= runtime_score_len(before.entries, kind));
    score::checked_append_exact(transcript, runtime_kind_image(kind));
    if before.entries.len() >= usize::MAX as int
        || runtime_score_len(before.entries, kind) > usize::MAX as int
    {
        if before.entries.len() >= usize::MAX as int {
            assert(transcript.entries.len() >= usize::MAX as int);
        } else {
            assert(score::kind_transcript(
                transcript.entries, runtime_kind_image(kind),
            ).len() > usize::MAX as int);
        }
    } else if !reservation_granted {
        assert(transcript.entries.len() < usize::MAX as int);
        assert(score::kind_transcript(
            transcript.entries, runtime_kind_image(kind),
        ).len() <= usize::MAX as int);
    } else {
        let after = RuntimeScoreArena {
            entries: before.entries.push(RuntimeScoreEntry {
                kind,
                encoded_len: runtime_score_len(before.entries, kind) as usize,
            }),
        };
        let handle = before.entries.len() as usize;
        assert(runtime_push_success(before, after, kind, handle));
        runtime_push_refines_score_append(before, transcript, after, kind, handle);
    }
}

pub proof fn runtime_reservation_failure_preserves_next_handle(
    before: RuntimeScoreArena, transcript: score::ScoreArena,
    kind: RuntimeScoreKind,
)
    requires
        runtime_projects_score(before, transcript),
        score::kind_valid(transcript.entries, runtime_kind_image(kind)),
        before.entries.len() < usize::MAX as int,
        runtime_score_len(before.entries, kind) <= usize::MAX as int,
    ensures match runtime_append_attempt(before, kind, false) {
        RuntimeAppendOutcome::ReservationFailed(unchanged) => {
            unchanged.entries == before.entries
                && match runtime_append_attempt(unchanged, kind, true) {
                    RuntimeAppendOutcome::Appended(after, handle) => {
                        &&& handle as int == before.entries.len()
                        &&& runtime_push_success(before, after, kind, handle)
                    },
                    _ => false,
                }
        },
        _ => false,
    },
{
    runtime_append_attempt_refines_score_append(before, transcript, kind, false);
    runtime_append_attempt_refines_score_append(before, transcript, kind, true);
}

pub open spec fn runtime_kind_valid(
    entries: Seq<RuntimeScoreEntry>, kind: RuntimeScoreKind,
) -> bool {
    match kind {
        RuntimeScoreKind::Node(children) => forall|i: int| 0 <= i < children.len()
            ==> (children[i] as int) < entries.len(),
        _ => true,
    }
}

pub proof fn runtime_kind_valid_projects(
    runtime: RuntimeScoreArena, transcript: score::ScoreArena, kind: RuntimeScoreKind,
)
    requires runtime_projects_score(runtime, transcript),
    ensures runtime_kind_valid(runtime.entries, kind)
        == score::kind_valid(transcript.entries, runtime_kind_image(kind)),
{}

pub open spec fn runtime_append_trace_step(entries: Seq<RuntimeScoreEntry>, i: int) -> bool {
    let before = RuntimeScoreArena { entries: entries.take(i) };
    let after = RuntimeScoreArena { entries: entries.take(i + 1) };
    &&& runtime_kind_valid(before.entries, entries[i].kind)
    &&& runtime_push_success(before, after, entries[i].kind, i as usize)
}

pub open spec fn runtime_append_trace_valid(entries: Seq<RuntimeScoreEntry>) -> bool {
    &&& entries.len() <= usize::MAX as int
    &&& forall|i: int| 0 <= i < entries.len()
        ==> #[trigger] runtime_append_trace_step(entries, i)
}

pub proof fn runtime_success_preserves_append_trace(
    before: RuntimeScoreArena, after: RuntimeScoreArena,
    kind: RuntimeScoreKind, handle: usize,
)
    requires
        runtime_append_trace_valid(before.entries),
        runtime_kind_valid(before.entries, kind),
        runtime_push_success(before, after, kind, handle),
    ensures runtime_append_trace_valid(after.entries),
{
    assert forall|i: int| 0 <= i < after.entries.len()
        implies #[trigger] runtime_append_trace_step(after.entries, i) by {
        if i < before.entries.len() {
            assert(runtime_append_trace_step(before.entries, i));
            assert(after.entries.take(i) == before.entries.take(i));
            assert(after.entries.take(i + 1) == before.entries.take(i + 1));
            assert(after.entries[i] == before.entries[i]);
        } else {
            assert(i == before.entries.len());
            assert(after.entries.take(i) == before.entries);
            assert(after.entries.take(i + 1) == after.entries);
            assert(after.entries[i].kind == kind);
        }
    }
}

pub proof fn runtime_failed_attempt_preserves_append_trace(
    before: RuntimeScoreArena, kind: RuntimeScoreKind,
    reservation_granted: bool,
)
    requires runtime_append_trace_valid(before.entries),
    ensures match runtime_append_attempt(before, kind, reservation_granted) {
        RuntimeAppendOutcome::LengthOverflow(after)
        | RuntimeAppendOutcome::ReservationFailed(after) => {
            &&& after.entries == before.entries
            &&& runtime_append_trace_valid(after.entries)
            &&& runtime_replay(after.entries) == runtime_replay(before.entries)
        },
        RuntimeAppendOutcome::Appended(_, _) => true,
    },
{}

pub proof fn runtime_take_of_take(
    entries: Seq<RuntimeScoreEntry>, n: int, m: int,
)
    requires 0 <= m <= n <= entries.len(),
    ensures entries.take(n).take(m) == entries.take(m),
{
    assert(entries.take(n).take(m) =~= entries.take(m)) by {
        assert forall|j: int| 0 <= j < m implies
            entries.take(n).take(m)[j] == entries.take(m)[j] by {
            assert(entries.take(n).take(m)[j] == entries.take(n)[j]);
            assert(entries.take(n)[j] == entries[j]);
            assert(entries.take(m)[j] == entries[j]);
        }
    }
}

pub open spec fn runtime_replay(entries: Seq<RuntimeScoreEntry>) -> Option<score::ScoreArena>
    decreases entries.len(),
{
    if entries.len() == 0 {
        Some(score::empty_arena())
    } else {
        let prefix = entries.take(entries.len() - 1);
        match runtime_replay(prefix) {
            None => None,
            Some(before) => match score::checked_append(
                before, runtime_kind_image(entries.last().kind),
            ) {
                None => None,
                Some((after, _)) => Some(after),
            },
        }
    }
}

pub proof fn runtime_append_trace_replays_exact(entries: Seq<RuntimeScoreEntry>)
    requires runtime_append_trace_valid(entries),
    ensures match runtime_replay(entries) {
        None => false,
        Some(transcript) => runtime_projects_score(
            RuntimeScoreArena { entries }, transcript,
        ),
    },
    decreases entries.len(),
{
    if entries.len() == 0 {
        score::empty_arena_well_formed();
    } else {
        let prefix = entries.take(entries.len() - 1);
        assert(runtime_append_trace_valid(prefix)) by {
            assert forall|i: int| 0 <= i < prefix.len()
                implies #[trigger] runtime_append_trace_step(prefix, i) by {
                assert(runtime_append_trace_step(entries, i));
                runtime_take_of_take(entries, prefix.len() as int, i);
                runtime_take_of_take(entries, prefix.len() as int, i + 1);
                assert(prefix.take(i) == entries.take(i));
                assert(prefix.take(i + 1) == entries.take(i + 1));
                assert(prefix[i] == entries[i]);
            }
        }
        runtime_append_trace_replays_exact(prefix);
        let before_transcript = runtime_replay(prefix).unwrap();
        let before = RuntimeScoreArena { entries: prefix };
        let after = RuntimeScoreArena { entries };
        let kind = entries.last().kind;
        let handle = (entries.len() - 1) as usize;
        assert(runtime_append_trace_step(entries, entries.len() - 1));
        assert(entries.take(entries.len() as int) == entries);
        assert(entries[entries.len() - 1] == entries.last());
        assert(runtime_kind_valid(before.entries, kind));
        runtime_kind_valid_projects(before, before_transcript, kind);
        assert(runtime_push_success(before, after, kind, handle));
        runtime_push_refines_score_append(before, before_transcript, after, kind, handle);
    }
}

pub proof fn runtime_append_trace_handle_exact(
    entries: Seq<RuntimeScoreEntry>, handle: usize,
)
    requires
        runtime_append_trace_valid(entries),
        (handle as int) < entries.len(),
    ensures match runtime_replay(entries) {
        None => false,
        Some(transcript) => {
            &&& score::handle_contents(transcript, handle)
                == Some(runtime_kind_image(entries[handle as int].kind))
            &&& score::handle_encoded_len(transcript, handle)
                == Some(entries[handle as int].encoded_len)
            &&& score::handle_transcript(transcript, handle) == Some(
                score::kind_transcript(
                    transcript.entries.take(handle as int),
                    runtime_kind_image(entries[handle as int].kind),
                ),
            )
        },
    },
{
    runtime_append_trace_replays_exact(entries);
    let transcript = runtime_replay(entries).unwrap();
    assert(score::entry_well_formed(transcript.entries, handle as int));
}

pub proof fn runtime_operational_attempt_preserves_replay_or_appends(
    before: RuntimeScoreArena, transcript: score::ScoreArena,
    kind: RuntimeScoreKind, reservation_granted: bool,
)
    requires
        runtime_append_trace_valid(before.entries),
        runtime_projects_score(before, transcript),
        score::kind_valid(transcript.entries, runtime_kind_image(kind)),
    ensures match runtime_append_attempt(before, kind, reservation_granted) {
        RuntimeAppendOutcome::LengthOverflow(after) => {
            &&& after.entries == before.entries
            &&& runtime_append_trace_valid(after.entries)
            &&& runtime_replay(after.entries) == runtime_replay(before.entries)
        },
        RuntimeAppendOutcome::ReservationFailed(after) => {
            &&& after.entries == before.entries
            &&& runtime_append_trace_valid(after.entries)
            &&& runtime_projects_score(after, transcript)
            &&& runtime_replay(after.entries) == runtime_replay(before.entries)
        },
        RuntimeAppendOutcome::Appended(after, handle) => {
            &&& runtime_append_trace_valid(after.entries)
            &&& match runtime_replay(after.entries) {
                Some(replayed) => {
                    &&& runtime_projects_score(after, replayed)
                    &&& score::handle_contents(replayed, handle)
                        == Some(runtime_kind_image(kind))
                    &&& score::handle_encoded_len(replayed, handle)
                        == Some(runtime_score_len(before.entries, kind) as usize)
                    &&& score::handle_transcript(replayed, handle) == Some(
                        score::kind_transcript(
                            replayed.entries.take(handle as int), runtime_kind_image(kind),
                        ),
                    )
                },
                None => false,
            }
        },
    },
{
    runtime_append_attempt_refines_score_append(
        before, transcript, kind, reservation_granted,
    );
    runtime_failed_attempt_preserves_append_trace(before, kind, reservation_granted);
    match runtime_append_attempt(before, kind, reservation_granted) {
        RuntimeAppendOutcome::Appended(after, handle) => {
            runtime_kind_valid_projects(before, transcript, kind);
            assert(runtime_push_success(before, after, kind, handle));
            assert(handle as int == before.entries.len());
            assert(after.entries[handle as int].kind == kind);
            assert(after.entries[handle as int].encoded_len
                == runtime_score_len(before.entries, kind) as usize);
            runtime_success_preserves_append_trace(before, after, kind, handle);
            runtime_append_trace_replays_exact(after.entries);
            runtime_append_trace_handle_exact(after.entries, handle);
        },
        _ => {},
    }
}

pub proof fn indexed_bytes_and_node_witness()
    ensures {
        let source = Seq::<RuntimeScoreEntry>::empty();
        let bytes = seq![0u8, 1u8];
        let atom = RuntimeScoreKind::BytesIndexed(bytes);
        let one = source.push(RuntimeScoreEntry { kind: atom, encoded_len: 7 });
        let two = one.push(RuntimeScoreEntry {
            kind: RuntimeScoreKind::Node(seq![0usize]), encoded_len: 9,
        });
        let initial = score::empty_arena();
        match score::checked_append(initial, score::ScoreNode::BytesAtom(bytes)) {
            Some((after_atom, atom_handle)) => match score::checked_append(
                after_atom, score::ScoreNode::Node(seq![atom_handle]),
            ) {
                Some((after_node, node_handle)) => {
                    &&& atom_handle == 0
                    &&& node_handle == 1
                    &&& runtime_projects_score(
                        RuntimeScoreArena { entries: two }, after_node,
                    )
                },
                None => false,
            },
            None => false,
        }
    },
{
    let bytes = seq![0u8, 1u8];
    let initial_runtime = RuntimeScoreArena { entries: Seq::empty() };
    let initial = score::empty_arena();
    score::empty_arena_well_formed();
    let atom = RuntimeScoreKind::BytesIndexed(bytes);
    let after_atom_runtime = RuntimeScoreArena {
        entries: initial_runtime.entries.push(RuntimeScoreEntry {
            kind: atom, encoded_len: 7,
        }),
    };
    assert(score::zero_count(bytes.drop_first().drop_first()) == 0);
    assert(score::zero_count(bytes.drop_first()) == 0);
    assert(score::zero_count(bytes) == 1);
    assert(runtime_score_len(initial_runtime.entries, atom) == 7);
    assert(runtime_push_success(initial_runtime, after_atom_runtime, atom, 0));
    runtime_push_refines_score_append(
        initial_runtime, initial, after_atom_runtime, atom, 0,
    );
    if let Some((after_atom, atom_handle)) = score::checked_append(
        initial, score::ScoreNode::BytesAtom(bytes),
    ) {
        assert(atom_handle == 0);
        let node = RuntimeScoreKind::Node(seq![atom_handle]);
        let after_node_runtime = RuntimeScoreArena {
            entries: after_atom_runtime.entries.push(RuntimeScoreEntry {
                kind: node, encoded_len: 9,
            }),
        };
        assert(after_atom_runtime.entries[0].encoded_len == 7);
        assert(seq![atom_handle] == seq![0usize]);
        assert(seq![atom_handle].drop_first() == Seq::<usize>::empty());
        assert(runtime_children_len(after_atom_runtime.entries, Seq::<usize>::empty()) == 0);
        assert(runtime_children_len(after_atom_runtime.entries, seq![atom_handle])
            == after_atom_runtime.entries[0].encoded_len as int
                + runtime_children_len(after_atom_runtime.entries, Seq::<usize>::empty()));
        assert(runtime_children_len(after_atom_runtime.entries, seq![atom_handle]) == 7);
        assert(runtime_score_len(after_atom_runtime.entries, node) == 9);
        assert(runtime_push_success(after_atom_runtime, after_node_runtime, node, 1));
        runtime_push_refines_score_append(
            after_atom_runtime, after_atom, after_node_runtime, node, 1,
        );
    }
}

}

fn main() {}
