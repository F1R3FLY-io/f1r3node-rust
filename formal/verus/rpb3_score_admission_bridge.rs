use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
mod rpb3_admission_cursor;
#[cfg(rpb3_integrated)]
use crate::rpb3_admission_cursor;
#[cfg(not(rpb3_integrated))]
mod rpb3_score_transcript;
use {rpb3_admission_cursor as admission, rpb3_score_transcript as score};

#[cfg(rpb3_integrated)]
use crate::rpb3_score_transcript;

verus! {

pub struct ScoreAdmissionBridge {
    pub ghost admission: admission::AdmissionState,
    pub ghost transcript: score::ScoreArena,
}

pub open spec fn bridge_well_formed(bridge: ScoreAdmissionBridge) -> bool {
    &&& score::arena_well_formed(bridge.transcript)
    &&& bridge.admission.arena.len() == bridge.transcript.entries.len()
    &&& forall|i: int| 0 <= i < bridge.admission.arena.len()
        ==> bridge.admission.arena[i].encoded_len
            == bridge.transcript.entries[i].encoded_len
            && bridge.admission.arena[i].transcript_bytes
                == bridge.transcript.entries[i].transcript
}

/// The next insertion handle is shared by the transcript and admission arena.
pub open spec fn bridge_append_certificate(
    bridge: ScoreAdmissionBridge, next_handle: usize,
) -> bool {
    &&& bridge_well_formed(bridge)
    &&& score::append_certificate(bridge.transcript, next_handle)
    &&& bridge.admission.arena.len() == next_handle as int
}

pub open spec fn bridge_certified_interval(
    bridge: ScoreAdmissionBridge, checkpoint: usize, next_handle: usize,
) -> bool {
    &&& bridge_append_certificate(bridge, next_handle)
    &&& checkpoint <= next_handle
}

pub proof fn bridge_certified_interval_entries_exact(
    bridge: ScoreAdmissionBridge, checkpoint: usize, next_handle: usize,
)
    requires bridge_certified_interval(bridge, checkpoint, next_handle),
    ensures forall|i: int| checkpoint as int <= i < next_handle as int ==> {
        &&& score::entry_well_formed(bridge.transcript.entries, i)
        &&& bridge.admission.arena[i].encoded_len
            == bridge.transcript.entries[i].encoded_len
        &&& bridge.admission.arena[i].transcript_bytes
            == bridge.transcript.entries[i].transcript
    },
{
    score::certified_interval_entries_valid(bridge.transcript, checkpoint, next_handle);
}

pub open spec fn bridge_binding_bytes_exact(
    bridge: ScoreAdmissionBridge, handle: usize, bytes: Seq<u8>,
) -> bool {
    bridge_well_formed(bridge)
        && (handle as int) < bridge.admission.arena.len()
        && bridge.admission.arena[handle as int].encoded_len as int == bytes.len()
        && bridge.admission.arena[handle as int].transcript_bytes == bytes
        && score::handle_bound_to_bytes(bridge.transcript, handle, bytes)
}

pub proof fn bridge_existing_binding_has_exact_transcript(
    bridge: ScoreAdmissionBridge, handle: usize,
)
    requires
        bridge_well_formed(bridge),
        (handle as int) < bridge.admission.arena.len(),
    ensures bridge_binding_bytes_exact(
        bridge, handle, bridge.transcript.entries[handle as int].transcript,
    ),
{
    assert(score::entry_well_formed(bridge.transcript.entries, handle as int));
}

pub proof fn bridge_binding_excludes_generic_zero_transcript(
    bridge: ScoreAdmissionBridge, handle: usize,
)
    requires
        bridge_well_formed(bridge),
        (handle as int) < bridge.admission.arena.len(),
    ensures
        bridge.admission.arena[handle as int].transcript_bytes.len() > 0,
        bridge.admission.arena[handle as int].transcript_bytes[0] != 0u8,
        bridge.admission.arena[handle as int].transcript_bytes
            != Seq::new(
                bridge.admission.arena[handle as int].encoded_len as nat,
                |i: int| 0u8,
            ),
{
    assert(score::entry_well_formed(bridge.transcript.entries, handle as int));
    let kind = bridge.transcript.entries[handle as int].kind;
    match kind {
        score::ScoreNode::Integer(_) => {
            assert(bridge.transcript.entries[handle as int].transcript[0] == 1u8);
        },
        score::ScoreNode::StringAtom(_) => {
            assert(bridge.transcript.entries[handle as int].transcript[0] == 1u8);
        },
        score::ScoreNode::BytesAtom(_) => {
            assert(bridge.transcript.entries[handle as int].transcript[0] == 1u8);
        },
        score::ScoreNode::Node(_) => {
            assert(bridge.transcript.entries[handle as int].transcript[0] == 2u8);
        },
    }
    let count = bridge.admission.arena[handle as int].encoded_len as nat;
    let zeros = Seq::new(count, |i: int| 0u8);
    assert(zeros[0] == 0u8);
    assert(bridge.admission.arena[handle as int].transcript_bytes[0] != zeros[0]);
}

pub open spec fn bridge_initial(root: admission::AdmissionFrameState)
    -> ScoreAdmissionBridge
{
    ScoreAdmissionBridge {
        admission: admission::admission_initial_state(root),
        transcript: score::empty_arena(),
    }
}

pub proof fn bridge_initial_exact(root: admission::AdmissionFrameState)
    ensures bridge_well_formed(bridge_initial(root)),
{
    score::empty_arena_well_formed();
}

pub proof fn bridge_initial_append_certificate(root: admission::AdmissionFrameState)
    ensures bridge_append_certificate(bridge_initial(root), 0usize),
{
    bridge_initial_exact(root);
    score::empty_append_certificate();
}

pub open spec fn bridge_typed_record_step(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode, body_id: u64,
) -> Option<admission::AdmissionState> {
    match score::checked_append(bridge.transcript, kind) {
        None => None,
        Some((next_transcript, handle)) => {
            if bridge.admission.frames.len() == 0 {
                None
            } else {
                let top = bridge.admission.frames.last();
                let binding = admission::AdmissionScoreBinding {
                    body_id,
                    semantic: admission::admission_family_complete(top),
                    encoded_len: next_transcript.entries[handle as int].encoded_len,
                    transcript_bytes: next_transcript.entries[handle as int].transcript,
                };
                match admission::admission_append_score_entry_step(
                    bridge.admission, binding,
                ) {
                    None => None,
                    Some(next) => admission::admission_attach_score_step(
                        next, handle, body_id,
                    ),
                }
            }
        },
    }
}

pub proof fn bridge_zero_count_matches(bytes: Seq<u8>)
    ensures
        score::zero_count(bytes) == admission::admission_zero_bytes(bytes),
        0 <= score::zero_count(bytes),
    decreases bytes.len(),
{
    if bytes.len() > 0 {
        assert(bytes.drop_first() == bytes.subrange(1, bytes.len() as int));
        bridge_zero_count_matches(bytes.drop_first());
    }
}

pub proof fn bridge_node_children_lengths_match(
    bridge: ScoreAdmissionBridge, children: Seq<usize>,
)
    requires
        bridge_well_formed(bridge),
        score::child_handles_valid(bridge.transcript.entries, children),
    ensures
        admission::admission_node_children_valid(bridge.admission.arena, children),
        0 <= admission::admission_node_children_length(bridge.admission.arena, children),
        admission::admission_node_children_length(bridge.admission.arena, children)
            == score::children_encoded_len(bridge.transcript.entries, children),
    decreases children.len(),
{
    assert forall|i: int| 0 <= i < children.len()
        implies (children[i] as int) < bridge.admission.arena.len() by {
        assert((children[i] as int) < bridge.transcript.entries.len());
    }
    if children.len() > 0 {
        let tail = children.drop_first();
        assert(tail == children.subrange(1, children.len() as int));
        assert(score::child_handles_valid(bridge.transcript.entries, tail)) by {
            assert forall|i: int| 0 <= i < tail.len()
                implies (tail[i] as int) < bridge.transcript.entries.len() by {
                assert(tail[i] == children[i + 1]);
            }
        }
        bridge_node_children_lengths_match(bridge, tail);
        assert(bridge.admission.arena[children[0] as int].encoded_len
            == bridge.transcript.entries[children[0] as int].encoded_len);
        assert(0 <= bridge.admission.arena[children[0] as int].encoded_len as int);
    }
}

pub proof fn bridge_typed_record_matches(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode, body_id: u64,
)
    requires
        bridge_well_formed(bridge),
        score::checked_append(bridge.transcript, kind).is_some(),
    ensures match bridge_typed_record_step(bridge, kind, body_id) {
        None => true,
        Some(next) => {
            let handle = bridge.admission.arena.len();
            &&& next.arena.len() == handle + 1
            &&& next.arena[handle as int].transcript_bytes
                == score::kind_transcript(bridge.transcript.entries, kind)
            &&& next.arena[handle as int].encoded_len as int
                == score::kind_transcript(bridge.transcript.entries, kind).len()
            &&& next.frames.last().score_handle == Some(handle as usize)
        },
    },
{
    match kind {
        score::ScoreNode::Integer(value) => {
            score::integer_transcript_exact(value);
            assert(score::kind_transcript(
                bridge.transcript.entries, score::ScoreNode::Integer(value),
            ).len() == 10);
        },
        score::ScoreNode::StringAtom(bytes) => {
            score::string_and_bytes_transcripts_exact(bytes);
            bridge_zero_count_matches(bytes);
            let encoded_len = 4 + bytes.len() + admission::admission_zero_bytes(bytes);
            assert(4 <= encoded_len <= usize::MAX as int);
            assert(score::kind_transcript(
                bridge.transcript.entries, score::ScoreNode::StringAtom(bytes),
            ).len() == encoded_len);
        },
        score::ScoreNode::BytesAtom(bytes) => {
            score::string_and_bytes_transcripts_exact(bytes);
            bridge_zero_count_matches(bytes);
            let encoded_len = 4 + bytes.len() + admission::admission_zero_bytes(bytes);
            assert(4 <= encoded_len <= usize::MAX as int);
            assert(score::kind_transcript(
                bridge.transcript.entries, score::ScoreNode::BytesAtom(bytes),
            ).len() == encoded_len);
        },
        score::ScoreNode::Node(children) => {
            assert(score::child_handles_valid(bridge.transcript.entries, children));
            bridge_node_children_lengths_match(bridge, children);
            assert forall|i: int| 0 <= i < bridge.transcript.entries.len()
                implies bridge.transcript.entries[i].encoded_len as int
                    == bridge.transcript.entries[i].transcript.len() by {
                assert(score::entry_well_formed(bridge.transcript.entries, i));
            }
            score::node_transcript_exact(bridge.transcript.entries, children);
            let child_len = admission::admission_node_children_length(
                bridge.admission.arena, children,
            );
            assert(0 <= child_len);
            assert(score::kind_transcript(
                bridge.transcript.entries, score::ScoreNode::Node(children),
            ).len() == 2 + child_len);
            assert(child_len <= usize::MAX as int - 2);
        },
    }
}

pub open spec fn bridge_append(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode, body_id: u64,
) -> Option<(ScoreAdmissionBridge, usize)> {
    match score::checked_append(bridge.transcript, kind) {
        None => None,
        Some((next_transcript, handle)) => {
            match bridge_typed_record_step(bridge, kind, body_id) {
                None => None,
                Some(next_admission) => {
                    Some((ScoreAdmissionBridge {
                        admission: next_admission,
                        transcript: next_transcript,
                    }, handle))
                },
            }
        },
    }
}

pub proof fn bridge_append_exact(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode, body_id: u64,
)
    requires bridge_well_formed(bridge),
    ensures match bridge_append(bridge, kind, body_id) {
        None => true,
        Some((next, handle)) => {
            &&& bridge_well_formed(next)
            &&& handle as int == bridge.admission.arena.len()
            &&& next.admission.arena.len() == bridge.admission.arena.len() + 1
            &&& next.transcript.entries.len() == bridge.transcript.entries.len() + 1
            &&& next.admission.arena[handle as int].body_id == body_id
            &&& next.admission.arena[handle as int].semantic
                == admission::admission_family_complete(next.admission.frames.last())
            &&& next.admission.arena[handle as int].encoded_len
                == next.transcript.entries[handle as int].encoded_len
            &&& score::handle_contents(next.transcript, handle) == Some(kind)
            &&& score::handle_bound_to_bytes(
                next.transcript, handle,
                score::kind_transcript(bridge.transcript.entries, kind)
            )
            &&& bridge_binding_bytes_exact(
                next, handle, score::kind_transcript(bridge.transcript.entries, kind)
            )
            &&& next.admission.frames.last().score_handle == Some(handle)
            &&& admission::admission_score_body_agrees(
                next.admission.frames.last(), next.admission.arena,
            )
        },
    },
{
    score::checked_append_exact(bridge.transcript, kind);
    if let Some((next, handle)) = bridge_append(bridge, kind, body_id) {
        bridge_typed_record_matches(bridge, kind, body_id);
        assert(admission::admission_score_body_agrees(
            next.admission.frames.last(), next.admission.arena,
        ));
        assert forall|i: int| 0 <= i < next.admission.arena.len()
            implies next.admission.arena[i].encoded_len
                == next.transcript.entries[i].encoded_len
                && next.admission.arena[i].transcript_bytes
                    == next.transcript.entries[i].transcript by {
            if i < bridge.admission.arena.len() {
                assert(next.admission.arena[i] == bridge.admission.arena[i]);
                assert(next.transcript.entries[i] == bridge.transcript.entries[i]);
            } else {
                assert(i == handle as int);
                assert(next.admission.arena[i].transcript_bytes
                    == score::kind_transcript(bridge.transcript.entries, kind));
                assert(next.transcript.entries[i].transcript
                    == score::kind_transcript(bridge.transcript.entries, kind));
            }
        }
        bridge_existing_binding_has_exact_transcript(next, handle);
    }
}

pub open spec fn bridge_append_integer(
    bridge: ScoreAdmissionBridge, value: i64, body_id: u64,
) -> Option<(ScoreAdmissionBridge, usize)> {
    bridge_append(bridge, score::ScoreNode::Integer(value), body_id)
}

pub open spec fn bridge_append_string(
    bridge: ScoreAdmissionBridge, bytes: Seq<u8>, body_id: u64,
) -> Option<(ScoreAdmissionBridge, usize)> {
    bridge_append(bridge, score::ScoreNode::StringAtom(bytes), body_id)
}

pub open spec fn bridge_append_bytes(
    bridge: ScoreAdmissionBridge, bytes: Seq<u8>, body_id: u64,
) -> Option<(ScoreAdmissionBridge, usize)> {
    bridge_append(bridge, score::ScoreNode::BytesAtom(bytes), body_id)
}

pub open spec fn bridge_append_node(
    bridge: ScoreAdmissionBridge, children: Seq<usize>, body_id: u64,
) -> Option<(ScoreAdmissionBridge, usize)> {
    bridge_append(bridge, score::ScoreNode::Node(children), body_id)
}

pub proof fn bridge_integer_exact(
    bridge: ScoreAdmissionBridge, value: i64, body_id: u64,
)
    requires bridge_well_formed(bridge),
    ensures match bridge_append_integer(bridge, value, body_id) {
        None => true,
        Some((next, handle)) => {
            &&& bridge_well_formed(next)
            &&& next.admission.arena[handle as int].encoded_len == 10
            &&& score::handle_transcript(next.transcript, handle)
                == Some(score::integer_transcript(value))
            &&& next.admission.arena[handle as int].transcript_bytes
                == score::integer_transcript(value)
        },
    },
{
    score::append_integer_exact(bridge.transcript, value);
    bridge_append_exact(bridge, score::ScoreNode::Integer(value), body_id);
}

pub proof fn bridge_string_exact(
    bridge: ScoreAdmissionBridge, bytes: Seq<u8>, body_id: u64,
)
    requires bridge_well_formed(bridge),
    ensures match bridge_append_string(bridge, bytes, body_id) {
        None => true,
        Some((next, handle)) => {
            &&& bridge_well_formed(next)
            &&& next.admission.arena[handle as int].encoded_len as int
                == 4 + bytes.len() + score::zero_count(bytes)
            &&& score::handle_transcript(next.transcript, handle)
                == Some(score::string_transcript(bytes))
            &&& next.admission.arena[handle as int].transcript_bytes
                == score::string_transcript(bytes)
        },
    },
{
    score::append_string_exact(bridge.transcript, bytes);
    bridge_append_exact(bridge, score::ScoreNode::StringAtom(bytes), body_id);
}

pub proof fn bridge_bytes_exact(
    bridge: ScoreAdmissionBridge, bytes: Seq<u8>, body_id: u64,
)
    requires bridge_well_formed(bridge),
    ensures match bridge_append_bytes(bridge, bytes, body_id) {
        None => true,
        Some((next, handle)) => {
            &&& bridge_well_formed(next)
            &&& next.admission.arena[handle as int].encoded_len as int
                == 4 + bytes.len() + score::zero_count(bytes)
            &&& score::handle_transcript(next.transcript, handle)
                == Some(score::bytes_transcript(bytes))
            &&& next.admission.arena[handle as int].transcript_bytes
                == score::bytes_transcript(bytes)
        },
    },
{
    score::append_bytes_exact(bridge.transcript, bytes);
    bridge_append_exact(bridge, score::ScoreNode::BytesAtom(bytes), body_id);
}

pub proof fn bridge_node_exact(
    bridge: ScoreAdmissionBridge, children: Seq<usize>, body_id: u64,
)
    requires
        bridge_well_formed(bridge),
        score::child_handles_valid(bridge.transcript.entries, children),
    ensures match bridge_append_node(bridge, children, body_id) {
        None => true,
        Some((next, handle)) => {
            &&& bridge_well_formed(next)
            &&& next.admission.arena[handle as int].encoded_len as int
                == 2 + score::children_encoded_len(bridge.transcript.entries, children)
            &&& score::handle_transcript(next.transcript, handle) == Some(
                seq![2u8] + score::children_transcript(bridge.transcript.entries, children)
                    + seq![0u8]
            )
            &&& next.admission.arena[handle as int].transcript_bytes
                == seq![2u8]
                    + score::children_transcript(bridge.transcript.entries, children)
                    + seq![0u8]
        },
    },
{
    score::append_node_exact(bridge.transcript, children);
    bridge_append_exact(bridge, score::ScoreNode::Node(children), body_id);
}

pub open spec fn bridge_append_unattached(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode,
) -> Option<(ScoreAdmissionBridge, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match score::checked_append(bridge.transcript, kind) {
            None => None,
            Some((next_transcript, handle)) => {
                let top = bridge.admission.frames.last();
                let binding = admission::AdmissionScoreBinding {
                    body_id: top.body_id,
                    semantic: admission::admission_family_complete(top),
                    encoded_len: next_transcript.entries[handle as int].encoded_len,
                    transcript_bytes: next_transcript.entries[handle as int].transcript,
                };
                match admission::admission_append_score_entry_step(bridge.admission, binding) {
                    None => None,
                    Some(next_admission) => Some((ScoreAdmissionBridge {
                        admission: next_admission,
                        transcript: next_transcript,
                    }, handle)),
                }
            },
        }
    }
}

pub proof fn bridge_append_unattached_exact(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode,
)
    requires bridge_well_formed(bridge),
    ensures match bridge_append_unattached(bridge, kind) {
        None => true,
        Some((next, handle)) => {
            &&& bridge_well_formed(next)
            &&& next.admission.frames == bridge.admission.frames
            &&& handle as int == bridge.admission.arena.len()
            &&& next.admission.arena.len() == bridge.admission.arena.len() + 1
            &&& next.admission.arena.take(bridge.admission.arena.len() as int)
                == bridge.admission.arena
            &&& next.transcript.entries.take(bridge.transcript.entries.len() as int)
                == bridge.transcript.entries
            &&& next.admission.arena[handle as int].body_id
                == bridge.admission.frames.last().body_id
            &&& next.admission.arena[handle as int].semantic
                == admission::admission_family_complete(bridge.admission.frames.last())
            &&& next.admission.arena[handle as int].encoded_len
                == next.transcript.entries[handle as int].encoded_len
            &&& score::handle_contents(next.transcript, handle) == Some(kind)
            &&& score::handle_bound_to_bytes(
                next.transcript, handle,
                score::kind_transcript(bridge.transcript.entries, kind)
            )
            &&& bridge_binding_bytes_exact(
                next, handle, score::kind_transcript(bridge.transcript.entries, kind)
            )
        },
    },
{
    score::checked_append_exact(bridge.transcript, kind);
    if let Some((next, handle)) = bridge_append_unattached(bridge, kind) {
        assert forall|i: int| 0 <= i < next.admission.arena.len()
            implies next.admission.arena[i].encoded_len
                == next.transcript.entries[i].encoded_len
                && next.admission.arena[i].transcript_bytes
                    == next.transcript.entries[i].transcript by {
            if i < bridge.admission.arena.len() {
                assert(next.admission.arena[i] == bridge.admission.arena[i]);
                assert(next.transcript.entries[i] == bridge.transcript.entries[i]);
            }
        }
        bridge_existing_binding_has_exact_transcript(next, handle);
    }
}

pub proof fn bridge_append_preserves_certificate(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode, body_id: u64,
    next_handle: usize,
)
    requires bridge_append_certificate(bridge, next_handle),
    ensures match bridge_append(bridge, kind, body_id) {
        Some((next, handle)) => {
            &&& handle == next_handle
            &&& bridge_append_certificate(
                next, next.transcript.entries.len() as usize,
            )
            &&& next.transcript.entries.take(next_handle as int)
                == bridge.transcript.entries
            &&& score::entry_well_formed(next.transcript.entries, next_handle as int)
            &&& bridge_binding_bytes_exact(
                next, handle,
                score::kind_transcript(bridge.transcript.entries, kind),
            )
        },
        None => true,
    },
{
    score::checked_append_preserves_certificate(bridge.transcript, kind, next_handle);
    bridge_append_exact(bridge, kind, body_id);
    if let Some((next, handle)) = bridge_append(bridge, kind, body_id) {
        assert(score::checked_append(bridge.transcript, kind)
            == Some((next.transcript, handle)));
    }
}

pub proof fn bridge_append_unattached_preserves_certificate(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode, next_handle: usize,
)
    requires bridge_append_certificate(bridge, next_handle),
    ensures match bridge_append_unattached(bridge, kind) {
        Some((next, handle)) => {
            &&& handle == next_handle
            &&& bridge_append_certificate(
                next, next.transcript.entries.len() as usize,
            )
            &&& next.transcript.entries.take(next_handle as int)
                == bridge.transcript.entries
            &&& next.admission.arena.take(next_handle as int)
                == bridge.admission.arena
            &&& score::entry_well_formed(next.transcript.entries, next_handle as int)
            &&& bridge_binding_bytes_exact(
                next, handle,
                score::kind_transcript(bridge.transcript.entries, kind),
            )
        },
        None => true,
    },
{
    score::checked_append_preserves_certificate(bridge.transcript, kind, next_handle);
    bridge_append_unattached_exact(bridge, kind);
    if let Some((next, handle)) = bridge_append_unattached(bridge, kind) {
        assert(score::checked_append(bridge.transcript, kind)
            == Some((next.transcript, handle)));
    }
}

/// A reserve failure, score overflow, or rejected admission append does not
/// commit either arena, so the same next handle remains available.
pub open spec fn bridge_append_unattached_attempt(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode,
    reservation_granted: bool,
) -> (ScoreAdmissionBridge, Option<usize>) {
    if !reservation_granted {
        (bridge, None)
    } else {
        match bridge_append_unattached(bridge, kind) {
            None => (bridge, None),
            Some((next, handle)) => (next, Some(handle)),
        }
    }
}

pub proof fn bridge_append_unattached_attempt_preserves_certificate(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode,
    next_handle: usize, reservation_granted: bool,
)
    requires bridge_append_certificate(bridge, next_handle),
    ensures match bridge_append_unattached_attempt(
        bridge, kind, reservation_granted,
    ) {
        (next, Some(handle)) => {
            &&& reservation_granted
            &&& handle == next_handle
            &&& bridge_append_certificate(
                next, next.transcript.entries.len() as usize,
            )
            &&& next.transcript.entries.take(next_handle as int)
                == bridge.transcript.entries
            &&& next.admission.arena.take(next_handle as int)
                == bridge.admission.arena
            &&& score::entry_well_formed(next.transcript.entries, next_handle as int)
        },
        (unchanged, None) => {
            &&& unchanged.transcript.entries == bridge.transcript.entries
            &&& unchanged.admission.arena == bridge.admission.arena
            &&& unchanged.transcript.entries.len() == next_handle as int
            &&& unchanged.admission.arena.len() == next_handle as int
            &&& bridge_append_certificate(unchanged, next_handle)
        },
    },
{
    if reservation_granted {
        bridge_append_unattached_preserves_certificate(bridge, kind, next_handle);
    }
}

pub proof fn bridge_append_unattached_attempt_preserves_certified_interval(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode,
    checkpoint: usize, next_handle: usize, reservation_granted: bool,
)
    requires bridge_certified_interval(bridge, checkpoint, next_handle),
    ensures match bridge_append_unattached_attempt(
        bridge, kind, reservation_granted,
    ) {
        (next, Some(_)) => bridge_certified_interval(
            next, checkpoint, next.transcript.entries.len() as usize,
        ),
        (unchanged, None) => bridge_certified_interval(
            unchanged, checkpoint, next_handle,
        ),
    },
{
    bridge_append_unattached_attempt_preserves_certificate(
        bridge, kind, next_handle, reservation_granted,
    );
}

pub proof fn bridge_append_unattached_preserves_admission_wf(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode,
)
    requires admission::admission_state_wf(bridge.admission),
    ensures match bridge_append_unattached(bridge, kind) {
        None => true,
        Some((next, _)) => admission::admission_state_wf(next.admission),
    },
{
    assert(bridge.admission.frames.len() > 0);
    if let Some((next_transcript, handle)) = score::checked_append(bridge.transcript, kind) {
        let top = bridge.admission.frames.last();
        let binding = admission::AdmissionScoreBinding {
            body_id: top.body_id,
            semantic: admission::admission_family_complete(top),
            encoded_len: next_transcript.entries[handle as int].encoded_len,
            transcript_bytes: next_transcript.entries[handle as int].transcript,
        };
        admission::admission_append_score_entry_preserves_wf(bridge.admission, binding);
    }
}

pub proof fn bridge_score_prefix_transitive(
    old: Seq<score::ScoreEntry>, middle: Seq<score::ScoreEntry>,
    next: Seq<score::ScoreEntry>,
)
    requires
        old.len() <= middle.len(),
        middle.len() <= next.len(),
        middle.take(old.len() as int) == old,
        next.take(middle.len() as int) == middle,
    ensures next.take(old.len() as int) == old,
{
    assert forall|i: int| 0 <= i < old.len()
        implies next[i] == old[i] by {
        assert(next[i] == middle[i]);
        assert(middle[i] == old[i]);
    }
    assert(next.take(old.len() as int).len() == old.len());
}

pub proof fn bridge_binding_prefix_transitive(
    old: Seq<admission::AdmissionScoreBinding>,
    middle: Seq<admission::AdmissionScoreBinding>,
    next: Seq<admission::AdmissionScoreBinding>,
)
    requires
        old.len() <= middle.len(),
        middle.len() <= next.len(),
        middle.take(old.len() as int) == old,
        next.take(middle.len() as int) == middle,
    ensures next.take(old.len() as int) == old,
{
    assert forall|i: int| 0 <= i < old.len()
        implies next[i] == old[i] by {
        assert(next[i] == middle[i]);
        assert(middle[i] == old[i]);
    }
    assert(next.take(old.len() as int).len() == old.len());
}

pub proof fn bridge_score_handle_preserved(
    old: score::ScoreArena, next: score::ScoreArena, handle: usize,
)
    requires
        (handle as int) < old.entries.len(),
        old.entries.len() <= next.entries.len(),
        next.entries.take(old.entries.len() as int) == old.entries,
    ensures score::handle_contents(next, handle) == score::handle_contents(old, handle),
{
    assert(next.entries[handle as int] == old.entries[handle as int]);
}

pub open spec fn bridge_score_step_shape(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
) -> bool {
    &&& before.admission.frames.len() > 0
    &&& after.admission.pushed == before.admission.pushed
    &&& after.admission.completed == before.admission.completed
    &&& after.admission.frames.len() == before.admission.frames.len()
    &&& forall|i: int| 0 <= i < before.admission.frames.len() - 1
        ==> after.admission.frames[i] == before.admission.frames[i]
    &&& after.admission.frames.last().source_bytes
        == before.admission.frames.last().source_bytes
    &&& after.admission.frames.last().region_start
        == before.admission.frames.last().region_start
    &&& after.admission.frames.last().region_end
        == before.admission.frames.last().region_end
    &&& after.admission.frames.last().cursor
        == before.admission.frames.last().cursor
    &&& after.admission.frames.last().last_slot
        == before.admission.frames.last().last_slot
    &&& after.admission.frames.last().returns
        == before.admission.frames.last().returns
    &&& after.admission.frames.last().physical_source
        == before.admission.frames.last().physical_source
    &&& after.admission.frames.last().physical_start
        == before.admission.frames.last().physical_start
    &&& after.admission.frames.last().frame_kind
        == before.admission.frames.last().frame_kind
    &&& after.admission.frames.last().program_type_id
        == before.admission.frames.last().program_type_id
    &&& after.admission.frames.last().family
        == before.admission.frames.last().family
    &&& after.admission.frames.last().return_id
        == before.admission.frames.last().return_id
    &&& before.admission.arena.len() <= after.admission.arena.len()
    &&& after.admission.arena.take(before.admission.arena.len() as int)
        == before.admission.arena
    &&& before.transcript.entries.len() <= after.transcript.entries.len()
    &&& after.transcript.entries.take(before.transcript.entries.len() as int)
        == before.transcript.entries
}

pub proof fn bridge_score_step_shape_reflexive(bridge: ScoreAdmissionBridge)
    requires bridge.admission.frames.len() > 0,
    ensures bridge_score_step_shape(bridge, bridge),
{
    assert forall|i: int| 0 <= i < bridge.admission.frames.len() - 1
        implies bridge.admission.frames[i] == bridge.admission.frames[i] by {}
    assert(bridge.admission.arena.take(bridge.admission.arena.len() as int)
        == bridge.admission.arena);
    assert(bridge.transcript.entries.take(bridge.transcript.entries.len() as int)
        == bridge.transcript.entries);
    assert(bridge.admission.frames.last().physical_source
        == bridge.admission.frames.last().physical_source);
    assert(bridge.admission.frames.last().physical_start
        == bridge.admission.frames.last().physical_start);
}

pub proof fn bridge_score_step_shape_preserves_par_source_trace(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        admission::admission_par_source_trace_invariant(before.admission),
        bridge_score_step_shape(before, after),
    ensures admission::admission_par_source_trace_invariant(after.admission),
{
    let previous = before.admission.frames.last();
    let current = after.admission.frames.last();
    if previous.program_type_id == 1 && previous.frame_kind == 1
        && previous.family == admission::AdmissionFamily::Other
    {
        let fields = choose|fields: Seq<admission::IndexedCanonicalFieldModel>|
            admission::admission_par_source_prefix_fields(previous, fields);
        assert(admission::admission_par_source_prefix_fields(current, fields));
    }
    assert forall|i: int| 0 <= i < after.admission.frames.len()
        implies admission::admission_par_source_prefix(after.admission.frames[i]) by {
        if i < after.admission.frames.len() - 1 {
            assert(after.admission.frames[i] == before.admission.frames[i]);
        }
    }
    assert forall|i: int| 0 <= i < after.admission.frames.len()
        implies admission::admission_source_identity_in_returns(after.admission.frames[i]) by {
        if i < after.admission.frames.len() - 1 {
            assert(after.admission.frames[i] == before.admission.frames[i]);
        } else {
            assert(current.returns == previous.returns);
        }
    }
}

pub proof fn bridge_score_step_shape_transitive(
    before: ScoreAdmissionBridge, middle: ScoreAdmissionBridge,
    after: ScoreAdmissionBridge,
)
    requires
        bridge_score_step_shape(before, middle),
        bridge_score_step_shape(middle, after),
    ensures bridge_score_step_shape(before, after),
{
    assert forall|i: int| 0 <= i < before.admission.frames.len() - 1
        implies after.admission.frames[i] == before.admission.frames[i] by {
        assert(after.admission.frames[i] == middle.admission.frames[i]);
        assert(middle.admission.frames[i] == before.admission.frames[i]);
    }
    bridge_score_prefix_transitive(
        before.transcript.entries,
        middle.transcript.entries,
        after.transcript.entries,
    );
    bridge_binding_prefix_transitive(
        before.admission.arena,
        middle.admission.arena,
        after.admission.arena,
    );
    assert(after.admission.frames.last().physical_source
        == before.admission.frames.last().physical_source);
    assert(after.admission.frames.last().physical_start
        == before.admission.frames.last().physical_start);
}

pub proof fn bridge_append_unattached_shape(
    bridge: ScoreAdmissionBridge, kind: score::ScoreNode,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_append_unattached(bridge, kind) {
        None => true,
        Some((next, _)) => bridge_score_step_shape(bridge, next),
    },
{
    bridge_append_unattached_exact(bridge, kind);
    if let Some((next, _)) = bridge_append_unattached(bridge, kind) {
        assert(next.admission.frames == bridge.admission.frames);
        assert(bridge.admission.frames.len() > 0);
    }
}

pub proof fn bridge_attach_score_shape(
    bridge: ScoreAdmissionBridge, handle: usize, body_id: u64,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match admission::admission_attach_score_step(
        bridge.admission, handle, body_id,
    ) {
        None => true,
        Some(admission) => bridge_score_step_shape(
            bridge, ScoreAdmissionBridge {
                admission, transcript: bridge.transcript,
            },
        ),
    },
{
    if let Some(admission) = admission::admission_attach_score_step(
        bridge.admission, handle, body_id,
    ) {
        assert(admission.arena == bridge.admission.arena);
        assert(admission.frames.len() == bridge.admission.frames.len());
        assert forall|i: int| 0 <= i < bridge.admission.frames.len() - 1
            implies admission.frames[i] == bridge.admission.frames[i] by {}
        assert(admission.frames.last().source_bytes
            == bridge.admission.frames.last().source_bytes);
        assert(admission.frames.last().physical_source
            == bridge.admission.frames.last().physical_source);
        assert(admission.frames.last().physical_start
            == bridge.admission.frames.last().physical_start);
        assert(admission.frames.last().frame_kind
            == bridge.admission.frames.last().frame_kind);
        assert(admission.frames.last().program_type_id
            == bridge.admission.frames.last().program_type_id);
        assert(admission.frames.last().family
            == bridge.admission.frames.last().family);
        assert(admission.frames.last().return_id
            == bridge.admission.frames.last().return_id);
        assert(admission.arena.take(bridge.admission.arena.len() as int)
            == bridge.admission.arena);
        assert(bridge.transcript.entries.take(bridge.transcript.entries.len() as int)
            == bridge.transcript.entries);
        assert(bridge_score_step_shape(
            bridge, ScoreAdmissionBridge {
                admission, transcript: bridge.transcript,
            },
        ));
    }
}

pub open spec fn bridge_cost_layers(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<Seq<usize>>
    decreases summaries.len(),
{
    if summaries.len() == 0 {
        Some(Seq::empty())
    } else {
        let returned = summaries[0];
        match returned.score_handle {
            None => None,
            Some(handle) => {
                if returned.slot != 0 || returned.tag != 1
                    || returned.kind != admission::AdmissionReturnedKind::CostScore
                    || handle as int >= arena.len()
                    || returned.score_origin_body_id
                        != Some(arena[handle as int].body_id)
                    || !arena[handle as int].semantic
                {
                    None
                } else {
                    match bridge_cost_layers(summaries.drop_first(), arena) {
                        None => None,
                        Some(tail) => Some(seq![handle] + tail),
                    }
                }
            },
        }
    }
}

pub proof fn bridge_cost_layers_valid(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>,
)
    ensures match bridge_cost_layers(summaries, arena) {
        None => true,
        Some(layers) => {
            &&& layers.len() == summaries.len()
            &&& admission::admission_node_children_valid(arena, layers)
            &&& forall|i: int| 0 <= i < layers.len() ==> {
                let returned = #[trigger] summaries[i];
                &&& returned.slot == 0
                &&& returned.tag == 1
                &&& returned.kind == admission::AdmissionReturnedKind::CostScore
                &&& returned.score_handle == Some(layers[i])
                &&& returned.score_origin_body_id
                    == Some(arena[layers[i] as int].body_id)
                &&& arena[layers[i] as int].semantic
            }
        },
    },
    decreases summaries.len(),
{
    if summaries.len() > 0 {
        bridge_cost_layers_valid(summaries.drop_first(), arena);
        if let Some(layers) = bridge_cost_layers(summaries, arena) {
            assert forall|i: int| 0 <= i < layers.len()
                implies (layers[i] as int) < arena.len() by {
                if i > 0 {
                    assert(layers[i] == layers.drop_first()[i - 1]);
                }
            }
            assert forall|i: int| 0 <= i < layers.len()
                implies {
                    let returned = #[trigger] summaries[i];
                    returned.slot == 0
                        && returned.tag == 1
                        && returned.kind == admission::AdmissionReturnedKind::CostScore
                        && returned.score_handle == Some(layers[i])
                        && returned.score_origin_body_id
                            == Some(arena[layers[i] as int].body_id)
                        && arena[layers[i] as int].semantic
                } by {
                if i > 0 {
                    assert(summaries[i] == summaries.drop_first()[i - 1]);
                    assert(layers[i] == layers.drop_first()[i - 1]);
                }
            }
        }
    }
}

pub open spec fn bridge_constructor_step(
    bridge: ScoreAdmissionBridge, constructor: i32, children: Seq<usize>,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0
        || !score::child_handles_valid(bridge.transcript.entries, children)
    {
        None
    } else {
        let body_id = bridge.admission.frames.last().body_id;
        match bridge_append_unattached(
            bridge, score::ScoreNode::Integer(constructor as i64),
        ) {
            None => None,
            Some((after_constructor, constructor_handle)) => {
                let all_children = seq![constructor_handle] + children;
                match bridge_append_unattached(
                    after_constructor, score::ScoreNode::Node(all_children),
                ) {
                    None => None,
                    Some((after_node, score_handle)) => {
                        match admission::admission_attach_score_step(
                            after_node.admission, score_handle, body_id,
                        ) {
                            None => None,
                            Some(admission) => Some((ScoreAdmissionBridge {
                                admission,
                                transcript: after_node.transcript,
                            }, constructor_handle, score_handle)),
                        }
                    },
                }
            },
        }
    }
}

pub open spec fn bridge_constructor_transcript(
    bridge: ScoreAdmissionBridge, constructor: i32, children: Seq<usize>,
) -> Seq<u8> {
    seq![2u8] + score::integer_transcript(constructor as i64)
        + score::children_transcript(bridge.transcript.entries, children) + seq![0u8]
}

pub open spec fn bridge_cost_token_stack_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0
        || bridge.admission.frames.last().family != admission::AdmissionFamily::CostTokenStack
    {
        None
    } else {
        match bridge_cost_layers(
            bridge.admission.frames.last().returns, bridge.admission.arena,
        ) {
            None => None,
            Some(layers) => bridge_constructor_step(bridge, 508i32, layers),
        }
    }
}

pub open spec fn bridge_cost_signature_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0
        || bridge.admission.frames.last().family != admission::AdmissionFamily::CostSignatureFrame
        || bridge.admission.frames.last().program_type_id != 9
    {
        None
    } else {
        match bridge_cost_layers(
            bridge.admission.frames.last().returns, bridge.admission.arena,
        ) {
            None => None,
            Some(atoms) => bridge_constructor_step(bridge, 501i32, atoms),
        }
    }
}

pub open spec fn bridge_cost_signature_transcript(
    bridge: ScoreAdmissionBridge, atoms: Seq<usize>,
) -> Seq<u8> {
    bridge_constructor_transcript(bridge, 501i32, atoms)
}

pub open spec fn bridge_cost_atom_absent_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if top.program_type_id != 10
            || top.seen.contains(0)
            || top.scalar_slots.contains(0)
            || top.scalar_occurrences.len() != 0
            || top.returns.len() != 0
        {
            None
        } else {
            bridge_constructor_step(bridge, 505i32, Seq::empty())
        }
    }
}

pub open spec fn bridge_cost_atom_bytes_code(tag: u32) -> Option<i32> {
    match tag {
        1 => Some(502i32),
        2 => Some(503i32),
        4 => Some(507i32),
        _ => None,
    }
}

pub open spec fn bridge_cost_atom_bytes_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if top.program_type_id != 10
            || !top.seen.contains(0)
            || !top.scalar_slots.contains(0)
            || top.scalar_occurrences.len() != 1
            || top.returns.len() != 0
        {
            None
        } else {
            let occurrence = top.scalar_occurrences[0];
            if occurrence.slot != 0 || occurrence.scalar_kind != 16 {
                None
            } else {
                match bridge_cost_atom_bytes_code(occurrence.tag) {
                    None => None,
                    Some(constructor) => {
                        match bridge_append_unattached(
                            bridge, score::ScoreNode::BytesAtom(occurrence.bytes),
                        ) {
                            None => None,
                            Some((after_bytes, bytes_handle)) => bridge_constructor_step(
                                after_bytes, constructor, seq![bytes_handle],
                            ),
                        }
                    },
                }
            }
        }
    }
}

pub open spec fn bridge_sint32_value(encoded: u64) -> i32 {
    let word = encoded as u32;
    let shifted = (word >> 1) as i32;
    if word & 1 == 0 { shifted } else { (-1int - shifted as int) as i32 }
}

pub open spec fn bridge_cost_bound_value(
    frame: admission::AdmissionFrameState,
) -> Option<i32> {
    if frame.program_type_id != 11 || frame.returns.len() != 0 {
        None
    } else if frame.seen.contains(0) {
        if !frame.scalar_slots.contains(0) || frame.scalar_occurrences.len() != 1 {
            None
        } else {
            let occurrence = frame.scalar_occurrences[0];
            if occurrence.slot != 0 || occurrence.tag != 1
                || occurrence.scalar_kind != 3
                || occurrence.varint_value > u32::MAX as u64
                || occurrence.varint_value == 0
            {
                None
            } else {
                Some(bridge_sint32_value(occurrence.varint_value))
            }
        }
    } else if frame.scalar_occurrences.len() == 0
        && !frame.scalar_slots.contains(0)
    {
        Some(0)
    } else {
        None
    }
}

pub open spec fn bridge_cost_bound_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_cost_bound_value(bridge.admission.frames.last()) {
            None => None,
            Some(index) => match bridge_append_unattached(
                bridge, score::ScoreNode::Integer(index as i64),
            ) {
                None => None,
                Some((after_index, index_handle)) => bridge_constructor_step(
                    after_index, 504i32, seq![index_handle],
                ),
            },
        }
    }
}

pub open spec fn bridge_cost_atom_bound_forward_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if top.program_type_id != 10
            || !top.seen.contains(0)
            || top.scalar_slots.contains(0)
            || top.scalar_occurrences.len() != 0
            || top.returns.len() != 1
        {
            None
        } else {
            let returned = top.returns[0];
            match returned.score_handle {
                None => None,
                Some(handle) => {
                    if returned.slot != 0 || returned.tag != 3
                        || returned.kind != admission::AdmissionReturnedKind::CostScore
                        || handle as int >= bridge.admission.arena.len()
                        || returned.score_origin_body_id != Some(
                            bridge.admission.arena[handle as int].body_id,
                        )
                        || !bridge.admission.arena[handle as int].semantic
                    {
                        None
                    } else {
                        match admission::admission_forward_bound_score_step(
                            bridge.admission,
                        ) {
                            None => None,
                            Some(admission) => Some((ScoreAdmissionBridge {
                                admission,
                                transcript: bridge.transcript,
                            }, handle)),
                        }
                    }
                },
            }
        }
    }
}

pub open spec fn bridge_scalar_slot(
    occurrences: Seq<admission::AdmissionScalarOccurrence>,
    slot: usize, tag: u32, scalar_kind: u8,
) -> Option<Option<u64>>
    decreases occurrences.len(),
{
    if occurrences.len() == 0 {
        Some(None)
    } else {
        let first = occurrences[0];
        let tail = bridge_scalar_slot(occurrences.drop_first(), slot, tag, scalar_kind);
        if first.slot != slot {
            tail
        } else if first.tag != tag || first.scalar_kind != scalar_kind {
            None
        } else {
            match tail {
                Some(None) => Some(Some(first.varint_value)),
                _ => None,
            }
        }
    }
}

pub open spec fn bridge_bool_slot(
    frame: admission::AdmissionFrameState, slot: usize, tag: u32,
) -> Option<bool> {
    match bridge_scalar_slot(frame.scalar_occurrences, slot, tag, 1u8) {
        None => None,
        Some(None) => if frame.seen.contains(slot as int)
            || frame.scalar_slots.contains(slot as int)
        { None } else { Some(false) },
        Some(Some(value)) => if value == 1
            && frame.seen.contains(slot as int)
            && frame.scalar_slots.contains(slot as int)
        { Some(true) } else { None },
    }
}

pub open spec fn bridge_par_return_handle(
    returned: admission::AdmissionReturnedSummary,
    arena: Seq<admission::AdmissionScoreBinding>, slot: usize, tag: u32,
) -> Option<usize> {
    if returned.slot != slot || returned.tag != tag {
        None
    } else {
        match (returned.kind, returned.score_handle) {
            (admission::AdmissionReturnedKind::Par { .. }, Some(handle)) => {
                if handle as int >= arena.len()
                    || returned.score_origin_body_id
                        != Some(arena[handle as int].body_id)
                    || !arena[handle as int].semantic
                { None } else { Some(handle) }
            },
            _ => None,
        }
    }
}

pub open spec fn bridge_bundle_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(usize, bool, bool)> {
    if frame.family != admission::AdmissionFamily::Bundle
        || frame.program_type_id != 8 || frame.returns.len() != 1
        || !admission::admission_family_complete(frame)
    { None } else {
        match (
            bridge_par_return_handle(frame.returns[0], arena, 0, 1),
            bridge_bool_slot(frame, 2, 3),
            bridge_bool_slot(frame, 1, 2),
        ) {
            (Some(body), Some(read), Some(write)) => Some((body, read, write)),
            _ => None,
        }
    }
}

pub open spec fn bridge_bundle_constructor(read: bool, write: bool) -> i32 {
    match (read, write) {
        (true, true) => 308i32,
        (true, false) => 306i32,
        (false, true) => 307i32,
        (false, false) => 305i32,
    }
}

pub open spec fn bridge_bundle_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_bundle_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((body, read, write)) => bridge_constructor_step(
                bridge, bridge_bundle_constructor(read, write), seq![body],
            ),
        }
    }
}

pub open spec fn bridge_if_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(usize, usize, usize, bool)> {
    if frame.family != admission::AdmissionFamily::If
        || frame.program_type_id != 23 || frame.returns.len() != 3
        || !admission::admission_family_complete(frame)
    { None } else {
        match (
            bridge_par_return_handle(frame.returns[0], arena, 0, 1),
            bridge_par_return_handle(frame.returns[1], arena, 1, 2),
            bridge_par_return_handle(frame.returns[2], arena, 2, 3),
            bridge_bool_slot(frame, 4, 5),
        ) {
            (Some(condition), Some(if_true), Some(if_false), Some(connective_used)) =>
                Some((condition, if_true, if_false, connective_used)),
            _ => None,
        }
    }
}

pub open spec fn bridge_if_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_if_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((condition, if_true, if_false, connective_used)) => {
                let flag = if connective_used { 1i64 } else { 0i64 };
                match bridge_append_unattached(bridge, score::ScoreNode::Integer(flag)) {
                    None => None,
                    Some((after_flag, flag_handle)) => match bridge_constructor_step(
                        after_flag, 309i32,
                        seq![condition, if_true, if_false, flag_handle],
                    ) {
                        None => None,
                        Some((next, constructor_handle, score_handle)) =>
                            Some((next, flag_handle, constructor_handle, score_handle)),
                    },
                }
            },
        }
    }
}

pub open spec fn bridge_par_handles_for_slot(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>, slot: usize, tag: u32,
) -> Option<Seq<usize>>
    decreases summaries.len(),
{
    if summaries.len() == 0 {
        Some(Seq::empty())
    } else {
        let first = summaries[0];
        match bridge_par_handles_for_slot(summaries.drop_first(), arena, slot, tag) {
            None => None,
            Some(rest) => if first.slot != slot {
                Some(rest)
            } else {
                match bridge_par_return_handle(first, arena, slot, tag) {
                    None => None,
                    Some(handle) => Some(seq![handle] + rest),
                }
            },
        }
    }
}

pub open spec fn bridge_cost_handles_for_slot(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>, slot: usize, tag: u32,
) -> Option<Seq<usize>>
    decreases summaries.len(),
{
    if summaries.len() == 0 {
        Some(Seq::empty())
    } else {
        let first = summaries[0];
        match bridge_cost_handles_for_slot(summaries.drop_first(), arena, slot, tag) {
            None => None,
            Some(rest) => if first.slot != slot {
                Some(rest)
            } else {
                match (first.kind, first.score_handle) {
                    (admission::AdmissionReturnedKind::CostScore, Some(handle))
                        if first.tag == tag && (handle as int) < arena.len()
                            && first.score_origin_body_id
                                == Some(arena[handle as int].body_id)
                            && arena[handle as int].semantic => Some(seq![handle] + rest),
                    _ => None,
                }
            },
        }
    }
}

pub open spec fn bridge_match_case_handles(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<Seq<usize>>
    decreases summaries.len(),
{
    if summaries.len() == 0 {
        Some(Seq::empty())
    } else {
        let first = summaries[0];
        match bridge_match_case_handles(summaries.drop_first(), arena) {
            None => None,
            Some(rest) => if first.slot != 1 {
                Some(rest)
            } else {
                match (first.kind, first.score_handle) {
                    (
                        admission::AdmissionReturnedKind::MatchCase {
                            required_present: true, canonical_fixed_point: true,
                        }, Some(handle),
                    ) if first.tag == 2 && (handle as int) < arena.len()
                        && first.score_origin_body_id
                            == Some(arena[handle as int].body_id)
                        && arena[handle as int].semantic => Some(seq![handle] + rest),
                    _ => None,
                }
            },
        }
    }
}

pub open spec fn bridge_var_handles_for_slot(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>, slot: usize, tag: u32,
) -> Option<Seq<usize>>
    decreases summaries.len(),
{
    if summaries.len() == 0 {
        Some(Seq::empty())
    } else {
        let first = summaries[0];
        match bridge_var_handles_for_slot(summaries.drop_first(), arena, slot, tag) {
            None => None,
            Some(rest) => if first.slot != slot {
                Some(rest)
            } else {
                match (first.kind, first.score_handle) {
                    (admission::AdmissionReturnedKind::VarScore, Some(handle))
                        if first.tag == tag && (handle as int) < arena.len()
                            && first.score_origin_body_id
                                == Some(arena[handle as int].body_id)
                            && arena[handle as int].semantic => Some(seq![handle] + rest),
                    _ => None,
                }
            },
        }
    }
}

pub open spec fn bridge_receive_bind_handles(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<Seq<usize>>
    decreases summaries.len(),
{
    if summaries.len() == 0 {
        Some(Seq::empty())
    } else {
        let first = summaries[0];
        match bridge_receive_bind_handles(summaries.drop_first(), arena) {
            None => None,
            Some(rest) => if first.slot != 0 {
                Some(rest)
            } else {
                match (first.kind, first.score_handle) {
                    (
                        admission::AdmissionReturnedKind::ReceiveBind {
                            source_present: true,
                        }, Some(handle),
                    ) if first.tag == 1 && (handle as int) < arena.len()
                        && first.score_origin_body_id
                            == Some(arena[handle as int].body_id)
                        && arena[handle as int].semantic => Some(seq![handle] + rest),
                    _ => None,
                }
            },
        }
    }
}

pub open spec fn bridge_receive_bind_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(usize, Seq<usize>, Option<usize>, Seq<usize>)> {
    if frame.family != admission::AdmissionFamily::ReceiveBindFrame
        || frame.program_type_id != 16
        || !admission::admission_family_complete(frame)
    { None } else {
        match (
            bridge_par_handles_for_slot(frame.returns, arena, 0, 1),
            bridge_par_handles_for_slot(frame.returns, arena, 1, 2),
            bridge_var_handles_for_slot(frame.returns, arena, 2, 3),
            bridge_cost_handles_for_slot(frame.returns, arena, 4, 5),
        ) {
            (Some(patterns), Some(source), Some(remainder), Some(cost))
                if source.len() == 1 && remainder.len() <= 1 && cost.len() <= 1
                    && frame.returns.len()
                        == patterns.len() + source.len() + remainder.len() + cost.len() =>
                    Some((source[0], patterns,
                        if remainder.len() == 1 { Some(remainder[0]) } else { None }, cost)),
            _ => None,
        }
    }
}

pub open spec fn bridge_receive_bind_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        match bridge_receive_bind_parts(top, bridge.admission.arena) {
            None => None,
            Some((source, patterns, remainder, cost)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(0i64),
                ) {
                    None => None,
                    Some((after_default, default_handle)) => {
                        let selected_remainder = match remainder {
                            Some(handle) => handle,
                            None => default_handle,
                        };
                        let children = seq![source] + patterns
                            + seq![selected_remainder] + cost;
                        match bridge_append_unattached(
                            after_default, score::ScoreNode::Node(children),
                        ) {
                            None => None,
                            Some((after_node, score_handle)) =>
                                match admission::admission_attach_score_step(
                                    after_node.admission, score_handle, top.body_id,
                                ) {
                                    None => None,
                                    Some(admission) => Some((ScoreAdmissionBridge {
                                        admission, transcript: after_node.transcript,
                                    }, default_handle, score_handle)),
                                },
                        }
                    },
                },
        }
    }
}

pub open spec fn bridge_int32_slot(
    frame: admission::AdmissionFrameState, slot: usize, tag: u32,
) -> Option<i64> {
    match bridge_scalar_slot(frame.scalar_occurrences, slot, tag, 2u8) {
        None => None,
        Some(None) => if frame.seen.contains(slot as int)
            || frame.scalar_slots.contains(slot as int)
        { None } else { Some(0i64) },
        Some(Some(value)) => if value != 0
            && ((value as u32 as i32) as u64) == value
            && frame.seen.contains(slot as int)
            && frame.scalar_slots.contains(slot as int)
        { Some((value as u32 as i32) as i64) } else { None },
    }
}

pub open spec fn bridge_sint32_slot(
    frame: admission::AdmissionFrameState, slot: usize, tag: u32,
) -> Option<i64> {
    match bridge_scalar_slot(frame.scalar_occurrences, slot, tag, 3u8) {
        None => None,
        Some(None) => if frame.seen.contains(slot as int)
            || frame.scalar_slots.contains(slot as int)
        { None } else { Some(0i64) },
        Some(Some(value)) => if value != 0 && value <= u32::MAX as u64
            && frame.seen.contains(slot as int)
            && frame.scalar_slots.contains(slot as int)
        {
            let encoded = value as u32;
            Some(((encoded >> 1) ^ (if encoded & 1 == 1 {
                u32::MAX
            } else { 0u32 })) as i32 as i64)
        } else { None },
    }
}

pub open spec fn bridge_sint64_value(encoded: u64) -> i64 {
    let shifted = (encoded >> 1) as i64;
    if encoded & 1 == 0 { shifted } else { (-1int - shifted as int) as i64 }
}

pub open spec fn bridge_fixed64_word(bytes: Seq<u8>) -> u64
    recommends bytes.len() == 8,
{
    (bytes[0] as u64)
        | ((bytes[1] as u64) << 8)
        | ((bytes[2] as u64) << 16)
        | ((bytes[3] as u64) << 24)
        | ((bytes[4] as u64) << 32)
        | ((bytes[5] as u64) << 40)
        | ((bytes[6] as u64) << 48)
        | ((bytes[7] as u64) << 56)
}

pub open spec fn bridge_string_bytes_for_slot(
    occurrences: Seq<admission::AdmissionScalarOccurrence>,
    slot: usize, tag: u32, scalar_kind: u8,
) -> Option<Seq<Seq<u8>>>
    decreases occurrences.len(),
{
    if occurrences.len() == 0 {
        Some(Seq::empty())
    } else {
        let first = occurrences[0];
        match bridge_string_bytes_for_slot(
            occurrences.drop_first(), slot, tag, scalar_kind,
        ) {
            None => None,
            Some(rest) => if first.slot != slot {
                Some(rest)
            } else if first.tag == tag && first.scalar_kind == scalar_kind {
                Some(seq![first.bytes] + rest)
            } else { None },
        }
    }
}

pub open spec fn bridge_match_case_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(usize, usize, i64, Option<usize>)> {
    if frame.family != admission::AdmissionFamily::MatchCaseFrame
        || frame.program_type_id != 21
        || !admission::admission_family_complete(frame)
    { None } else {
        match (
            bridge_par_handles_for_slot(frame.returns, arena, 0, 1),
            bridge_par_handles_for_slot(frame.returns, arena, 1, 2),
            bridge_par_handles_for_slot(frame.returns, arena, 3, 4),
            bridge_int32_slot(frame, 2, 3),
        ) {
            (Some(pattern), Some(source), Some(guard), Some(free_count))
                if pattern.len() == 1 && source.len() == 1 && guard.len() <= 1
                    && frame.returns.len() == pattern.len() + source.len() + guard.len() =>
                    Some((pattern[0], source[0], free_count,
                        if guard.len() == 1 { Some(guard[0]) } else { None })),
            _ => None,
        }
    }
}

pub open spec fn bridge_match_case_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        match bridge_match_case_parts(top, bridge.admission.arena) {
            None => None,
            Some((pattern, source, free_count, guard)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(0i64),
                ) {
                    None => None,
                    Some((after_flag, flag_handle)) =>
                        match bridge_append_unattached(
                            after_flag, score::ScoreNode::Integer(999i64),
                        ) {
                            None => None,
                            Some((after_constructor, constructor_handle)) =>
                                match bridge_append_unattached(
                                    after_constructor,
                                    score::ScoreNode::Node(seq![constructor_handle, flag_handle]),
                                ) {
                                    None => None,
                                    Some((after_empty, empty_handle)) => {
                                        let selected_guard = match guard {
                                            Some(handle) => handle,
                                            None => empty_handle,
                                        };
                                        match bridge_append_unattached(
                                            after_empty, score::ScoreNode::Integer(free_count),
                                        ) {
                                            None => None,
                                            Some((after_count, count_handle)) =>
                                                match bridge_append_unattached(
                                                    after_count, score::ScoreNode::Node(seq![
                                                        pattern, source, count_handle,
                                                        selected_guard,
                                                    ]),
                                                ) {
                                                    None => None,
                                                    Some((after_node, score_handle)) =>
                                                        match admission::admission_attach_score_step(
                                                            after_node.admission, score_handle,
                                                            top.body_id,
                                                        ) {
                                                            None => None,
                                                            Some(admission) => Some((
                                                                ScoreAdmissionBridge {
                                                                    admission,
                                                                    transcript: after_node.transcript,
                                                                }, flag_handle,
                                                                constructor_handle, empty_handle,
                                                                count_handle, score_handle,
                                                            )),
                                                        },
                                                },
                                        }
                                    },
                                },
                        },
                },
        }
    }
}

pub open spec fn bridge_bool_oneof_parts(
    frame: admission::AdmissionFrameState,
) -> Option<(i32, i64)> {
    if frame.scalar_occurrences.len() != 1
        || frame.returns.len() != 0
        || !frame.seen.contains(0)
        || !frame.scalar_slots.contains(0)
        || !admission::admission_family_complete(frame)
    { None } else {
        let occurrence = frame.scalar_occurrences[0];
        if occurrence.slot != 0 || occurrence.scalar_kind != 1
            || occurrence.varint_value > 1
        { None } else if frame.family == admission::AdmissionFamily::Expr
            && frame.program_type_id == 24 && occurrence.tag == 1
        {
            Some((1i32, if occurrence.varint_value == 1 { 0i64 } else { 1i64 }))
        } else if frame.family == admission::AdmissionFamily::Connective
            && frame.program_type_id == 55
        {
            let constructor = match occurrence.tag {
                5 => Some(404i32), 6 => Some(405i32), 7 => Some(406i32),
                8 => Some(407i32), 9 => Some(408i32), _ => None,
            };
            match constructor {
                Some(tag) => Some((tag, occurrence.varint_value as i64)),
                None => None,
            }
        } else { None }
    }
}

pub open spec fn bridge_bool_oneof_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_bool_oneof_parts(bridge.admission.frames.last()) {
            None => None,
            Some((constructor, value)) =>
                match bridge_append_unattached(bridge, score::ScoreNode::Integer(value)) {
                    None => None,
                    Some((after_value, value_handle)) =>
                        match bridge_constructor_step(
                            after_value, constructor, seq![value_handle],
                        ) {
                            None => None,
                            Some((next, constructor_handle, score_handle)) =>
                                Some((next, value_handle, constructor_handle, score_handle)),
                        },
                },
        }
    }
}

pub open spec fn bridge_expr_atom_parts(
    frame: admission::AdmissionFrameState,
) -> Option<(i32, score::ScoreNode)> {
    if frame.family != admission::AdmissionFamily::Expr
        || frame.program_type_id != 24
        || frame.scalar_occurrences.len() != 1
        || frame.returns.len() != 0
        || !frame.seen.contains(0)
        || !frame.scalar_slots.contains(0)
        || !admission::admission_family_complete(frame)
        || !admission::admission_expr_fixed(frame)
    { None } else {
        let occurrence = frame.scalar_occurrences[0];
        if occurrence.slot != 0 { None } else {
            match (occurrence.tag, occurrence.scalar_kind) {
                (3, 14) => Some((3i32, score::ScoreNode::StringAtom(occurrence.bytes))),
                (4, 14) => Some((4i32, score::ScoreNode::StringAtom(occurrence.bytes))),
                (25, 16) => Some((116i32, score::ScoreNode::BytesAtom(occurrence.bytes))),
                (35, 16) => Some((15i32, score::ScoreNode::BytesAtom(occurrence.bytes))),
                _ => None,
            }
        }
    }
}

pub open spec fn bridge_expr_atom_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_expr_atom_parts(bridge.admission.frames.last()) {
            None => None,
            Some((constructor, atom_kind)) =>
                match bridge_append_unattached(bridge, atom_kind) {
                    None => None,
                    Some((after_atom, atom_handle)) =>
                        match bridge_constructor_step(
                            after_atom, constructor, seq![atom_handle],
                        ) {
                            None => None,
                            Some((next, constructor_handle, score_handle)) =>
                                Some((next, atom_handle, constructor_handle, score_handle)),
                        },
                },
        }
    }
}

pub open spec fn bridge_expr_numeric_parts(
    frame: admission::AdmissionFrameState,
) -> Option<(i32, i64)> {
    if frame.family != admission::AdmissionFamily::Expr
        || frame.program_type_id != 24
        || frame.scalar_occurrences.len() != 1
        || frame.returns.len() != 0
        || !frame.seen.contains(0)
        || !frame.scalar_slots.contains(0)
        || !admission::admission_family_complete(frame)
        || !admission::admission_expr_fixed(frame)
    { None } else {
        let occurrence = frame.scalar_occurrences[0];
        if occurrence.slot != 0 { None } else {
            match (occurrence.tag, occurrence.scalar_kind) {
                (2, 6) => Some((2i32, bridge_sint64_value(occurrence.varint_value))),
                (34, 12) if occurrence.bytes.len() == 8 =>
                    Some((14i32, bridge_fixed64_word(occurrence.bytes) as i64)),
                _ => None,
            }
        }
    }
}

pub open spec fn bridge_expr_numeric_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_expr_numeric_parts(bridge.admission.frames.last()) {
            None => None,
            Some((constructor, value)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(value),
                ) {
                    None => None,
                    Some((after_value, value_handle)) =>
                        match bridge_constructor_step(
                            after_value, constructor, seq![value_handle],
                        ) {
                            None => None,
                            Some((next, constructor_handle, score_handle)) =>
                                Some((next, value_handle, constructor_handle, score_handle)),
                        },
                },
        }
    }
}

pub open spec fn bridge_expr_big_rational_parts(
    frame: admission::AdmissionFrameState,
) -> Option<(Seq<u8>, Seq<u8>)> {
    if frame.family != admission::AdmissionFamily::Expr
        || frame.program_type_id != 24
        || frame.returns.len() != 1
        || frame.scalar_occurrences.len() != 0
        || !frame.seen.contains(0)
        || !admission::admission_family_complete(frame)
        || !admission::admission_expr_fixed(frame)
    { None } else {
        let returned = frame.returns[0];
        if returned.slot != 0 || returned.tag != 36
            || returned.score_handle.is_some()
        { None } else {
            match returned.kind {
                admission::AdmissionReturnedKind::BigRational {
                    numerator, denominator,
                } => Some((numerator, denominator)),
                _ => None,
            }
        }
    }
}

pub open spec fn bridge_expr_big_rational_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_expr_big_rational_parts(bridge.admission.frames.last()) {
            None => None,
            Some((numerator, denominator)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::BytesAtom(numerator),
                ) {
                    None => None,
                    Some((after_numerator, numerator_handle)) =>
                        match bridge_append_unattached(
                            after_numerator, score::ScoreNode::BytesAtom(denominator),
                        ) {
                            None => None,
                            Some((after_denominator, denominator_handle)) =>
                                match bridge_constructor_step(
                                    after_denominator, 16i32,
                                    seq![numerator_handle, denominator_handle],
                                ) {
                                    None => None,
                                    Some((next, constructor_handle, score_handle)) =>
                                        Some((next, numerator_handle, denominator_handle,
                                            constructor_handle, score_handle)),
                                },
                        },
                },
        }
    }
}

pub open spec fn bridge_expr_fixed_point_parts(
    frame: admission::AdmissionFrameState,
) -> Option<(Seq<u8>, u32)> {
    if frame.family != admission::AdmissionFamily::Expr
        || frame.program_type_id != 24
        || frame.returns.len() != 1
        || frame.scalar_occurrences.len() != 0
        || !frame.seen.contains(0)
        || !admission::admission_family_complete(frame)
        || !admission::admission_expr_fixed(frame)
    { None } else {
        let returned = frame.returns[0];
        if returned.slot != 0 || returned.tag != 37
            || returned.score_handle.is_some()
        { None } else {
            match returned.kind {
                admission::AdmissionReturnedKind::FixedPoint { unscaled, scale } =>
                    Some((unscaled, scale)),
                _ => None,
            }
        }
    }
}

pub open spec fn bridge_expr_fixed_point_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_expr_fixed_point_parts(bridge.admission.frames.last()) {
            None => None,
            Some((unscaled, scale)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::BytesAtom(unscaled),
                ) {
                    None => None,
                    Some((after_unscaled, unscaled_handle)) =>
                        match bridge_append_unattached(
                            after_unscaled, score::ScoreNode::Integer(scale as i64),
                        ) {
                            None => None,
                            Some((after_scale, scale_handle)) =>
                                match bridge_append_unattached(
                                    after_scale, score::ScoreNode::Node(seq![scale_handle]),
                                ) {
                                    None => None,
                                    Some((after_scale_node, scale_node_handle)) =>
                                        match bridge_constructor_step(
                                            after_scale_node, 17i32,
                                            seq![unscaled_handle, scale_node_handle],
                                        ) {
                                            None => None,
                                            Some((next, constructor_handle, score_handle)) =>
                                                Some((next, unscaled_handle, scale_handle,
                                                    scale_node_handle, constructor_handle,
                                                    score_handle)),
                                        },
                                },
                        },
                },
        }
    }
}

pub open spec fn bridge_sys_auth_token_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if top.family == admission::AdmissionFamily::GUnforgeable
            && top.program_type_id == 60
            && top.returns.len() == 1
            && top.returns[0].slot == 0
            && top.returns[0].tag == 4
            && top.returns[0].kind == admission::AdmissionReturnedKind::Unit
            && top.returns[0].score_handle.is_none()
            && top.scalar_occurrences.len() == 0
            && admission::admission_family_complete(top)
        {
            bridge_constructor_step(bridge, 12i32, Seq::empty())
        } else { None }
    }
}

pub open spec fn bridge_new_empty_collections_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(i64, usize)> {
    if frame.family != admission::AdmissionFamily::New
        || frame.program_type_id != 20
        || !admission::admission_family_complete(frame)
        || frame.seen.contains(2) || frame.seen.contains(3)
        || frame.returns.len() != 1
    { None } else {
        match (
            bridge_sint32_slot(frame, 0, 1),
            bridge_par_return_handle(frame.returns[0], arena, 1, 2),
        ) {
            (Some(bind_count), Some(body)) => Some((bind_count, body)),
            _ => None,
        }
    }
}

pub open spec fn bridge_new_empty_collections_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_new_empty_collections_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((bind_count, body)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(0i64),
                ) {
                    None => None,
                    Some((after_uri, uri_handle)) =>
                        match bridge_append_unattached(
                            after_uri, score::ScoreNode::Integer(0i64),
                        ) {
                            None => None,
                            Some((after_injection, injection_handle)) =>
                                match bridge_append_unattached(
                                    after_injection, score::ScoreNode::Integer(bind_count),
                                ) {
                                    None => None,
                                    Some((after_count, count_handle)) =>
                                        match bridge_constructor_step(
                                            after_count, 303i32,
                                            seq![count_handle, uri_handle,
                                                injection_handle, body],
                                        ) {
                                            None => None,
                                            Some((next, constructor_handle, score_handle)) =>
                                                Some((next, count_handle, uri_handle,
                                                    injection_handle, constructor_handle,
                                                    score_handle)),
                                        },
                                },
                        },
                },
        }
    }
}

pub open spec fn bridge_expr_payload_forward_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if top.family != admission::AdmissionFamily::Expr
            || top.program_type_id != 24
            || top.returns.len() != 1
            || top.returns[0].slot != 0
            || !admission::admission_field_tag_valid(
                admission::AdmissionFamily::Expr, 0, top.returns[0].tag, true, false,
            )
            || top.scalar_occurrences.len() != 0
            || !top.seen.contains(0)
            || !admission::admission_family_complete(top)
            || !admission::admission_expr_fixed(top)
        { None } else {
            let returned = top.returns[0];
            match (returned.kind, returned.score_handle) {
                (
                    admission::AdmissionReturnedKind::ExprPayload {
                        score_present: true, canonical_fixed_point: true, ..
                    },
                    Some(handle),
                ) if (handle as int) < bridge.admission.arena.len()
                    && returned.score_origin_body_id
                        == Some(bridge.admission.arena[handle as int].body_id)
                    && bridge.admission.arena[handle as int].semantic =>
                    match admission::admission_attach_score_step(
                        bridge.admission, handle, top.body_id,
                    ) {
                        None => None,
                        Some(admission) => Some((ScoreAdmissionBridge {
                            admission, transcript: bridge.transcript,
                        }, handle)),
                    },
                _ => None,
            }
        }
    }
}

pub open spec fn bridge_expr_structural_constructor(
    program_type_id: u16,
) -> Option<(i32, bool)> {
    match program_type_id {
        36 => Some((112i32, false)),
        37 => Some((101i32, false)),
        38 => Some((102i32, true)),
        39 => Some((103i32, true)),
        40 => Some((122i32, true)),
        41 => Some((104i32, true)),
        42 => Some((105i32, true)),
        43 => Some((106i32, true)),
        44 => Some((107i32, true)),
        45 => Some((108i32, true)),
        46 => Some((109i32, true)),
        47 => Some((110i32, true)),
        48 => Some((111i32, true)),
        49 => Some((113i32, true)),
        50 => Some((114i32, true)),
        51 => Some((118i32, true)),
        52 => Some((119i32, true)),
        53 => Some((120i32, true)),
        54 => Some((121i32, true)),
        _ => None,
    }
}

pub open spec fn bridge_expr_structural_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(i32, Seq<usize>)> {
    if frame.family != admission::AdmissionFamily::Other
        || frame.frame_kind != 1
        || frame.scalar_occurrences.len() != 0
    {
        None
    } else {
        match bridge_expr_structural_constructor(frame.program_type_id) {
            None => None,
            Some((constructor, binary)) => match (
                bridge_par_handles_for_slot(frame.returns, arena, 0, 1),
                bridge_par_handles_for_slot(frame.returns, arena, 1, 2),
            ) {
                (Some(first), Some(second)) if first.len() == 1
                    && second.len() == (if binary { 1nat } else { 0nat })
                    && frame.returns.len() == (if binary { 2nat } else { 1nat }) =>
                    Some((constructor, first + second)),
                _ => None,
            },
        }
    }
}

pub open spec fn bridge_expr_structural_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, i32, Seq<usize>, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_expr_structural_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((constructor, children)) =>
                match bridge_constructor_step(bridge, constructor, children) {
                    None => None,
                    Some((next, constructor_handle, score_handle)) =>
                        Some((next, constructor, children, constructor_handle, score_handle)),
                },
        }
    }
}

pub proof fn bridge_expr_structural_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_structural_step(bridge) {
        None => true,
        Some((next, constructor, children, constructor_handle, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(constructor as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        if let Some((constructor, children)) = bridge_expr_structural_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            bridge_constructor_exact(bridge, constructor, children);
            bridge_constructor_shape(bridge, constructor, children);
        }
    }
}

pub open spec fn bridge_evar_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<usize> {
    if frame.family != admission::AdmissionFamily::EVarFrame
        || frame.program_type_id != 35
        || frame.frame_kind != 1
        || frame.scalar_occurrences.len() != 0
        || frame.returns.len() != 1
        || !frame.seen.contains(0)
    {
        None
    } else {
        match bridge_var_handles_for_slot(frame.returns, arena, 0, 1) {
            Some(handles) if handles.len() == 1 => Some(handles[0]),
            _ => None,
        }
    }
}

pub open spec fn bridge_evar_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_evar_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some(var_handle) => match bridge_constructor_step(
                bridge, 100i32, seq![var_handle],
            ) {
                None => None,
                Some((next, constructor_handle, score_handle)) =>
                    Some((next, var_handle, constructor_handle, score_handle)),
            },
        }
    }
}

pub proof fn bridge_evar_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_evar_step(bridge) {
        None => true,
        Some((next, var_handle, constructor_handle, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(100i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle, var_handle]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        if let Some(var_handle) = bridge_evar_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            bridge_constructor_exact(bridge, 100i32, seq![var_handle]);
            bridge_constructor_shape(bridge, 100i32, seq![var_handle]);
            if let Some((_, constructor_handle, _)) =
                bridge_constructor_step(bridge, 100i32, seq![var_handle])
            {
                assert(seq![constructor_handle] + seq![var_handle]
                    == seq![constructor_handle, var_handle]);
            }
        }
    }
}

pub open spec fn bridge_ordered_sequence_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(i32, Seq<usize>, Option<usize>, bool)> {
    if frame.family != admission::AdmissionFamily::Other
        || frame.frame_kind != 1
        || (frame.program_type_id != 27 && frame.program_type_id != 28
            && frame.program_type_id != 29)
        || frame.slot_count != (if frame.program_type_id == 28 { 3usize } else { 4usize })
    {
        None
    } else {
        match (
            bridge_par_handles_for_slot(frame.returns, arena, 0, 1),
            bridge_var_handles_for_slot(frame.returns, arena, 3, 5),
            bridge_bool_slot(frame, 2, 4),
        ) {
            (Some(elements), Some(remainders), Some(connective_used))
                if frame.seen.contains(0) == (elements.len() > 0)
                    && frame.returns.len() == elements.len() + remainders.len()
                    && (if frame.program_type_id != 28 {
                        remainders.len() <= 1
                            && frame.seen.contains(3) == (remainders.len() == 1)
                    } else {
                        remainders.len() == 0 && !frame.seen.contains(3)
                    }) =>
                Some((if frame.program_type_id == 27 { 6i32 }
                    else if frame.program_type_id == 28 { 7i32 }
                    else { 8i32 },
                    elements,
                    if remainders.len() == 0 { None } else { Some(remainders[0]) },
                    connective_used)),
            _ => None,
        }
    }
}

pub open spec fn bridge_ordered_sequence_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, Option<usize>, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_ordered_sequence_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((constructor, elements, remainder, connective_used)) => {
                let after_remainder = if constructor != 7 {
                    match bridge_append_unattached(
                        bridge, score::ScoreNode::Integer(-1i64),
                    ) {
                        None => None,
                        Some((next, default_handle)) =>
                            Some((next, Some(match remainder {
                                Some(handle) => handle,
                                None => default_handle,
                            }))),
                    }
                } else {
                    Some((bridge, None))
                };
                match after_remainder {
                    None => None,
                    Some((after_rem, rem_handle)) =>
                        match bridge_append_unattached(
                            after_rem, score::ScoreNode::Integer(
                                if connective_used { 1i64 } else { 0i64 },
                            ),
                        ) {
                            None => None,
                            Some((after_flag, flag_handle)) => {
                                let children = match rem_handle {
                                    Some(handle) => seq![handle] + elements + seq![flag_handle],
                                    None => elements + seq![flag_handle],
                                };
                                match bridge_constructor_step(
                                    after_flag, constructor, children,
                                ) {
                                    None => None,
                                    Some((next, constructor_handle, score_handle)) =>
                                        Some((next, rem_handle, flag_handle,
                                            constructor_handle, score_handle)),
                                }
                            },
                        },
                }
            },
        }
    }
}

pub proof fn bridge_ordered_sequence_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_ordered_sequence_step(bridge) {
        None => true,
        Some((next, rem_handle, flag_handle, constructor_handle, score_handle)) => {
            let (constructor, elements, _, connective_used) =
                bridge_ordered_sequence_parts(
                    bridge.admission.frames.last(), bridge.admission.arena,
                ).unwrap();
            let children = match rem_handle {
                Some(handle) => seq![handle] + elements + seq![flag_handle],
                None => elements + seq![flag_handle],
            };
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& score::handle_contents(next.transcript, flag_handle)
                == Some(score::ScoreNode::Integer(
                    if connective_used { 1i64 } else { 0i64 },
                ))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(constructor as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        if let Some((constructor, elements, remainder, connective_used)) =
            bridge_ordered_sequence_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            )
        {
            if constructor != 7 {
                let default_kind = score::ScoreNode::Integer(-1i64);
                bridge_append_unattached_exact(bridge, default_kind);
                bridge_append_unattached_preserves_admission_wf(bridge, default_kind);
                bridge_append_unattached_shape(bridge, default_kind);
            } else {
                bridge_score_step_shape_reflexive(bridge);
            }
            let after_remainder = if constructor != 7 {
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(-1i64),
                ) {
                    None => None,
                    Some((next, default_handle)) =>
                        Some((next, Some(match remainder {
                            Some(handle) => handle,
                            None => default_handle,
                        }))),
                }
            } else {
                Some((bridge, None))
            };
            if let Some((after_rem, rem_handle)) = after_remainder {
                let flag_kind = score::ScoreNode::Integer(
                    if connective_used { 1i64 } else { 0i64 },
                );
                bridge_append_unattached_exact(after_rem, flag_kind);
                bridge_append_unattached_preserves_admission_wf(after_rem, flag_kind);
                bridge_append_unattached_shape(after_rem, flag_kind);
                if let Some((after_flag, flag_handle)) =
                    bridge_append_unattached(after_rem, flag_kind)
                {
                    let children = match rem_handle {
                        Some(handle) => seq![handle] + elements + seq![flag_handle],
                        None => elements + seq![flag_handle],
                    };
                    bridge_constructor_exact(after_flag, constructor, children);
                    bridge_constructor_shape(after_flag, constructor, children);
                    if let Some((next, _, _)) =
                        bridge_constructor_step(after_flag, constructor, children)
                    {
                        bridge_score_handle_preserved(
                            after_flag.transcript, next.transcript, flag_handle,
                        );
                        bridge_score_step_shape_transitive(bridge, after_rem, after_flag);
                        bridge_score_step_shape_transitive(bridge, after_flag, next);
                    }
                }
            }
        }
    }
}

pub open spec fn bridge_emethod_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(Seq<u8>, usize, Seq<usize>, bool)> {
    if frame.family != admission::AdmissionFamily::Other
        || frame.frame_kind != 1
        || frame.program_type_id != 33
    {
        None
    } else {
        match (
            bridge_string_bytes_for_slot(frame.scalar_occurrences, 0, 1, 15u8),
            bridge_par_handles_for_slot(frame.returns, arena, 1, 2),
            bridge_par_handles_for_slot(frame.returns, arena, 2, 3),
            bridge_bool_slot(frame, 4, 6),
        ) {
            (Some(names), Some(targets), Some(arguments), Some(connective_used))
                if names.len() <= 1
                    && frame.seen.contains(0) == (names.len() == 1)
                    && targets.len() == 1
                    && frame.returns.len() == 1 + arguments.len() =>
                Some((if names.len() == 0 { Seq::empty() } else { names[0] },
                    targets[0], arguments, connective_used)),
            _ => None,
        }
    }
}

pub open spec fn bridge_emethod_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_emethod_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((method, target, arguments, connective_used)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::StringAtom(method),
                ) {
                    None => None,
                    Some((after_method, method_handle)) =>
                        match bridge_append_unattached(
                            after_method, score::ScoreNode::Integer(
                                if connective_used { 1i64 } else { 0i64 },
                            ),
                        ) {
                            None => None,
                            Some((after_flag, flag_handle)) =>
                                match bridge_constructor_step(
                                    after_flag, 115i32,
                                    seq![method_handle, target] + arguments
                                        + seq![flag_handle],
                                ) {
                                    None => None,
                                    Some((next, constructor_handle, score_handle)) =>
                                        Some((next, method_handle, flag_handle,
                                            target, constructor_handle, score_handle)),
                                },
                        },
                },
        }
    }
}

pub proof fn bridge_emethod_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_emethod_step(bridge) {
        None => true,
        Some((next, method_handle, flag_handle, target,
            constructor_handle, score_handle)) => {
            let (method, _, arguments, connective_used) = bridge_emethod_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& score::handle_contents(next.transcript, method_handle)
                == Some(score::ScoreNode::StringAtom(method))
            &&& score::handle_contents(next.transcript, flag_handle)
                == Some(score::ScoreNode::Integer(
                    if connective_used { 1i64 } else { 0i64 },
                ))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(
                    seq![constructor_handle, method_handle, target] + arguments
                        + seq![flag_handle],
                ))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        if let Some((method, target, arguments, connective_used)) =
            bridge_emethod_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            )
        {
            let method_kind = score::ScoreNode::StringAtom(method);
            bridge_append_unattached_exact(bridge, method_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, method_kind);
            bridge_append_unattached_shape(bridge, method_kind);
            if let Some((after_method, method_handle)) =
                bridge_append_unattached(bridge, method_kind)
            {
                let flag_kind = score::ScoreNode::Integer(
                    if connective_used { 1i64 } else { 0i64 },
                );
                bridge_append_unattached_exact(after_method, flag_kind);
                bridge_append_unattached_preserves_admission_wf(after_method, flag_kind);
                bridge_append_unattached_shape(after_method, flag_kind);
                if let Some((after_flag, flag_handle)) =
                    bridge_append_unattached(after_method, flag_kind)
                {
                    let children = seq![method_handle, target] + arguments
                        + seq![flag_handle];
                    bridge_constructor_exact(after_flag, 115i32, children);
                    bridge_constructor_shape(after_flag, 115i32, children);
                    if let Some((next, constructor_handle, _)) =
                        bridge_constructor_step(after_flag, 115i32, children)
                    {
                        bridge_score_prefix_transitive(
                            bridge.transcript.entries,
                            after_method.transcript.entries,
                            after_flag.transcript.entries,
                        );
                        bridge_score_prefix_transitive(
                            after_method.transcript.entries,
                            after_flag.transcript.entries,
                            next.transcript.entries,
                        );
                        bridge_score_handle_preserved(
                            after_method.transcript, next.transcript, method_handle,
                        );
                        bridge_score_handle_preserved(
                            after_flag.transcript, next.transcript, flag_handle,
                        );
                        bridge_score_step_shape_transitive(
                            bridge, after_method, after_flag,
                        );
                        bridge_score_step_shape_transitive(
                            bridge, after_flag, next,
                        );
                        assert(seq![constructor_handle] + children
                            == seq![constructor_handle, method_handle, target]
                                + arguments + seq![flag_handle]);
                    }
                }
            }
        }
    }
}

pub open spec fn bridge_connective_not_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if top.family != admission::AdmissionFamily::Connective
            || top.program_type_id != 55
            || top.returns.len() != 1
            || top.scalar_occurrences.len() != 0
            || !admission::admission_family_complete(top)
        { None } else {
            match bridge_par_return_handle(
                top.returns[0], bridge.admission.arena, 0, 3,
            ) {
                None => None,
                Some(child) => bridge_constructor_step(bridge, 400i32, seq![child]),
            }
        }
    }
}

pub open spec fn bridge_connective_varref_parts(
    frame: admission::AdmissionFrameState,
) -> Option<(i64, i64)> {
    if frame.family != admission::AdmissionFamily::Connective
        || frame.program_type_id != 55
        || frame.returns.len() != 1
        || frame.scalar_occurrences.len() != 0
        || !frame.seen.contains(0)
        || !admission::admission_family_complete(frame)
    { None } else {
        let returned = frame.returns[0];
        if returned.slot != 0 || returned.tag != 4
            || returned.score_handle.is_some()
        { None } else {
            match returned.kind {
                admission::AdmissionReturnedKind::VarRef { index, depth } =>
                    Some((index as i64, depth as i64)),
                _ => None,
            }
        }
    }
}

pub open spec fn bridge_connective_varref_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_connective_varref_parts(bridge.admission.frames.last()) {
            None => None,
            Some((index, depth)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(index),
                ) {
                    None => None,
                    Some((after_index, index_handle)) =>
                        match bridge_append_unattached(
                            after_index, score::ScoreNode::Integer(depth),
                        ) {
                            None => None,
                            Some((after_depth, depth_handle)) =>
                                match bridge_constructor_step(
                                    after_depth, 403i32,
                                    seq![index_handle, depth_handle],
                                ) {
                                    None => None,
                                    Some((next, constructor_handle, score_handle)) =>
                                        Some((next, index_handle, depth_handle,
                                            constructor_handle, score_handle)),
                                },
                        },
                },
        }
    }
}

pub open spec fn bridge_connective_collection_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(i32, Seq<usize>)> {
    if frame.family != admission::AdmissionFamily::Connective
        || frame.program_type_id != 55
        || frame.returns.len() != 1
        || frame.scalar_occurrences.len() != 0
        || !frame.seen.contains(0)
        || !admission::admission_family_complete(frame)
    { None } else {
        let returned = frame.returns[0];
        let constructor = match returned.tag {
            1 => Some(401i32), 2 => Some(402i32), _ => None,
        };
        match (constructor, returned.kind) {
            (
                Some(constructor),
                admission::AdmissionReturnedKind::ConnectiveBody { par_handles },
            ) if returned.slot == 0 && returned.score_handle.is_none()
                && (forall|i: int| 0 <= i < par_handles.len()
                    ==> (par_handles[i] as int) < arena.len()
                        && arena[par_handles[i] as int].semantic) =>
                Some((constructor, par_handles)),
            _ => None,
        }
    }
}

pub open spec fn bridge_connective_collection_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_connective_collection_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((constructor, children)) =>
                bridge_constructor_step(bridge, constructor, children),
        }
    }
}

pub open spec fn bridge_unforgeable_bytes_parts(
    frame: admission::AdmissionFrameState,
) -> Option<(i32, Seq<u8>)> {
    if frame.family != admission::AdmissionFamily::GUnforgeable
        || frame.program_type_id != 60
        || frame.returns.len() != 1
        || frame.scalar_occurrences.len() != 0
        || !frame.seen.contains(0)
        || !admission::admission_family_complete(frame)
    { None } else {
        let returned = frame.returns[0];
        if returned.slot != 0 || returned.score_handle.is_some() {
            None
        } else {
            let constructor = match returned.tag {
                1 => Some(5i32), 2 => Some(11i32), 3 => Some(10i32), _ => None,
            };
            match (constructor, returned.kind) {
                (Some(constructor), admission::AdmissionReturnedKind::Bytes { value }) =>
                    Some((constructor, value)),
                _ => None,
            }
        }
    }
}

pub open spec fn bridge_unforgeable_bytes_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_unforgeable_bytes_parts(bridge.admission.frames.last()) {
            None => None,
            Some((constructor, value)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::BytesAtom(value),
                ) {
                    None => None,
                    Some((after_bytes, bytes_handle)) =>
                        match bridge_constructor_step(
                            after_bytes, constructor, seq![bytes_handle],
                        ) {
                            None => None,
                            Some((next, constructor_handle, score_handle)) =>
                                Some((next, bytes_handle, constructor_handle, score_handle)),
                        },
                },
        }
    }
}

pub open spec fn bridge_receive_present_condition_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(bool, bool, Seq<usize>, usize, i64, bool, usize, Seq<usize>)> {
    if frame.family != admission::AdmissionFamily::Receive
        || frame.program_type_id != 19
        || !admission::admission_family_complete(frame)
        || !admission::admission_receive_guard_fixed(frame)
        || admission::admission_returned_default_par(frame, 7)
    { None } else {
        match (
            bridge_receive_bind_handles(frame.returns, arena),
            bridge_par_handles_for_slot(frame.returns, arena, 1, 2),
            bridge_par_handles_for_slot(frame.returns, arena, 7, 8),
            bridge_cost_handles_for_slot(frame.returns, arena, 8, 9),
            bridge_bool_slot(frame, 2, 3),
            bridge_bool_slot(frame, 3, 4),
            bridge_int32_slot(frame, 4, 5),
            bridge_bool_slot(frame, 6, 7),
        ) {
            (
                Some(binds), Some(body), Some(condition), Some(cost),
                Some(persistent), Some(peek), Some(bind_count), Some(connective_used),
            ) if body.len() == 1 && condition.len() == 1 && cost.len() <= 1
                && frame.returns.len()
                    == binds.len() + body.len() + condition.len() + cost.len() =>
                Some((persistent, peek, binds, body[0], bind_count,
                    connective_used, condition[0], cost)),
            _ => None,
        }
    }
}

pub open spec fn bridge_receive_present_condition_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_receive_present_condition_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((persistent, peek, binds, body, bind_count,
                connective_used, condition, cost)) => {
                let persistent_value = if persistent { 1i64 } else { 0i64 };
                let peek_value = if peek { 1i64 } else { 0i64 };
                let connective_value = if connective_used { 1i64 } else { 0i64 };
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(persistent_value),
                ) {
                    None => None,
                    Some((after_persistent, persistent_handle)) =>
                        match bridge_append_unattached(
                            after_persistent, score::ScoreNode::Integer(peek_value),
                        ) {
                            None => None,
                            Some((after_peek, peek_handle)) =>
                                match bridge_append_unattached(
                                    after_peek, score::ScoreNode::Integer(bind_count),
                                ) {
                                    None => None,
                                    Some((after_count, count_handle)) =>
                                        match bridge_append_unattached(
                                            after_count,
                                            score::ScoreNode::Integer(connective_value),
                                        ) {
                                            None => None,
                                            Some((after_connective, connective_handle)) => {
                                                let children = seq![
                                                    persistent_handle, peek_handle,
                                                ] + binds + seq![
                                                    body, count_handle, connective_handle,
                                                    condition,
                                                ] + cost;
                                                match bridge_constructor_step(
                                                    after_connective, 301i32, children,
                                                ) {
                                                    None => None,
                                                    Some((next, constructor_handle, score_handle)) =>
                                                        Some((next, persistent_handle,
                                                            peek_handle, count_handle,
                                                            connective_handle,
                                                            constructor_handle, score_handle)),
                                                }
                                            },
                                        },
                                },
                        },
                }
            },
        }
    }
}

pub open spec fn bridge_empty_par_unattached_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize)> {
    match bridge_append_unattached(bridge, score::ScoreNode::Integer(0i64)) {
        None => None,
        Some((after_flag, flag_handle)) =>
            match bridge_append_unattached(
                after_flag, score::ScoreNode::Integer(999i64),
            ) {
                None => None,
                Some((after_constructor, constructor_handle)) =>
                    match bridge_append_unattached(
                        after_constructor,
                        score::ScoreNode::Node(seq![constructor_handle, flag_handle]),
                    ) {
                        None => None,
                        Some((after_empty, empty_handle)) =>
                            Some((after_empty, flag_handle, constructor_handle, empty_handle)),
                    },
            },
    }
}

pub open spec fn bridge_receive_absent_condition_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(bool, bool, Seq<usize>, usize, i64, bool, Seq<usize>)> {
    if frame.family != admission::AdmissionFamily::Receive
        || frame.program_type_id != 19
        || !admission::admission_family_complete(frame)
        || frame.seen.contains(7)
    { None } else {
        match (
            bridge_receive_bind_handles(frame.returns, arena),
            bridge_par_handles_for_slot(frame.returns, arena, 1, 2),
            bridge_cost_handles_for_slot(frame.returns, arena, 8, 9),
            bridge_bool_slot(frame, 2, 3),
            bridge_bool_slot(frame, 3, 4),
            bridge_int32_slot(frame, 4, 5),
            bridge_bool_slot(frame, 6, 7),
        ) {
            (
                Some(binds), Some(body), Some(cost), Some(persistent),
                Some(peek), Some(bind_count), Some(connective_used),
            ) if body.len() == 1 && cost.len() <= 1
                && frame.returns.len() == binds.len() + body.len() + cost.len() =>
                Some((persistent, peek, binds, body[0], bind_count,
                    connective_used, cost)),
            _ => None,
        }
    }
}

pub open spec fn bridge_receive_absent_condition_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_receive_absent_condition_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((persistent, peek, binds, body, bind_count, connective_used, cost)) =>
                match bridge_empty_par_unattached_step(bridge) {
                    None => None,
                    Some((after_empty, _, _, empty_handle)) => {
                        let persistent_value = if persistent { 1i64 } else { 0i64 };
                        let peek_value = if peek { 1i64 } else { 0i64 };
                        let connective_value = if connective_used { 1i64 } else { 0i64 };
                        match bridge_append_unattached(
                            after_empty, score::ScoreNode::Integer(persistent_value),
                        ) {
                            None => None,
                            Some((after_persistent, persistent_handle)) =>
                                match bridge_append_unattached(
                                    after_persistent, score::ScoreNode::Integer(peek_value),
                                ) {
                                    None => None,
                                    Some((after_peek, peek_handle)) =>
                                        match bridge_append_unattached(
                                            after_peek, score::ScoreNode::Integer(bind_count),
                                        ) {
                                            None => None,
                                            Some((after_count, count_handle)) =>
                                                match bridge_append_unattached(
                                                    after_count,
                                                    score::ScoreNode::Integer(connective_value),
                                                ) {
                                                    None => None,
                                                    Some((after_connective, connective_handle)) => {
                                                        let children = seq![
                                                            persistent_handle, peek_handle,
                                                        ] + binds + seq![
                                                            body, count_handle,
                                                            connective_handle, empty_handle,
                                                        ] + cost;
                                                        match bridge_constructor_step(
                                                            after_connective, 301i32, children,
                                                        ) {
                                                            None => None,
                                                            Some((next, constructor_handle,
                                                                score_handle)) => Some((next,
                                                                empty_handle,
                                                                persistent_handle, peek_handle,
                                                                count_handle, connective_handle,
                                                                constructor_handle,
                                                                score_handle)),
                                                        }
                                                    },
                                                },
                                        },
                                },
                        }
                    },
                },
        }
    }
}

pub open spec fn bridge_new_one_injection_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(i64, usize, Seq<u8>, usize)> {
    if frame.family != admission::AdmissionFamily::New
        || frame.program_type_id != 20
        || !admission::admission_family_complete(frame)
        || frame.seen.contains(2)
        || frame.returns.len() != 2
    { None } else {
        match (
            bridge_sint32_slot(frame, 0, 1),
            bridge_par_return_handle(frame.returns[0], arena, 1, 2),
            frame.returns[1].kind,
        ) {
            (
                Some(bind_count), Some(body),
                admission::AdmissionReturnedKind::Map {
                    key_bytes, value_score_handle: Some(value_handle),
                },
            ) if frame.returns[1].slot == 3
                && frame.returns[1].tag == 4
                && (value_handle as int) < arena.len()
                && arena[value_handle as int].semantic =>
                Some((bind_count, body, key_bytes, value_handle)),
            _ => None,
        }
    }
}

pub open spec fn bridge_new_one_injection_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_new_one_injection_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((bind_count, body, key_bytes, value_handle)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(0i64),
                ) {
                    None => None,
                    Some((after_uri, uri_handle)) =>
                        match bridge_append_unattached(
                            after_uri, score::ScoreNode::StringAtom(key_bytes),
                        ) {
                            None => None,
                            Some((after_key, key_handle)) =>
                                match bridge_append_unattached(
                                    after_key, score::ScoreNode::Node(
                                        seq![key_handle, value_handle],
                                    ),
                                ) {
                                    None => None,
                                    Some((after_injection, injection_handle)) =>
                                        match bridge_append_unattached(
                                            after_injection,
                                            score::ScoreNode::Integer(bind_count),
                                        ) {
                                            None => None,
                                            Some((after_count, count_handle)) =>
                                                match bridge_constructor_step(
                                                    after_count, 303i32,
                                                    seq![count_handle, uri_handle,
                                                        injection_handle, body],
                                                ) {
                                                    None => None,
                                                    Some((next, constructor_handle, score_handle)) =>
                                                        Some((next, count_handle, uri_handle,
                                                            key_handle, injection_handle,
                                                            constructor_handle, score_handle)),
                                                },
                                        },
                                },
                        },
                },
        }
    }
}

pub open spec fn bridge_new_one_absent_injection_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(i64, usize, Seq<u8>)> {
    if frame.family != admission::AdmissionFamily::New
        || frame.program_type_id != 20
        || !admission::admission_family_complete(frame)
        || frame.seen.contains(2)
        || frame.returns.len() != 2
    { None } else {
        match (
            bridge_sint32_slot(frame, 0, 1),
            bridge_par_return_handle(frame.returns[0], arena, 1, 2),
            frame.returns[1].kind,
        ) {
            (
                Some(bind_count), Some(body),
                admission::AdmissionReturnedKind::Map {
                    key_bytes, value_score_handle: None,
                },
            ) if frame.returns[1].slot == 3
                && frame.returns[1].tag == 4 =>
                Some((bind_count, body, key_bytes)),
            _ => None,
        }
    }
}

pub open spec fn bridge_new_one_absent_injection_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_new_one_absent_injection_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((bind_count, body, key_bytes)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(0i64),
                ) {
                    None => None,
                    Some((after_uri, uri_handle)) =>
                        match bridge_empty_par_unattached_step(after_uri) {
                            None => None,
                            Some((after_empty, _, _, empty_handle)) =>
                                match bridge_append_unattached(
                                    after_empty, score::ScoreNode::StringAtom(key_bytes),
                                ) {
                                    None => None,
                                    Some((after_key, key_handle)) =>
                                        match bridge_append_unattached(
                                            after_key, score::ScoreNode::Node(
                                                seq![key_handle, empty_handle],
                                            ),
                                        ) {
                                            None => None,
                                            Some((after_injection, injection_handle)) =>
                                                match bridge_append_unattached(
                                                    after_injection,
                                                    score::ScoreNode::Integer(bind_count),
                                                ) {
                                                    None => None,
                                                    Some((after_count, count_handle)) =>
                                                        match bridge_constructor_step(
                                                            after_count, 303i32,
                                                            seq![count_handle, uri_handle,
                                                                injection_handle, body],
                                                        ) {
                                                            None => None,
                                                            Some((next, constructor_handle,
                                                                score_handle)) => Some((next,
                                                                uri_handle, empty_handle,
                                                                key_handle, injection_handle,
                                                                count_handle,
                                                                constructor_handle,
                                                                score_handle)),
                                                        },
                                                },
                                        },
                                },
                        },
                },
        }
    }
}

pub open spec fn bridge_new_one_uri_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(i64, usize, Seq<u8>)> {
    if frame.family != admission::AdmissionFamily::New
        || frame.program_type_id != 20
        || !admission::admission_family_complete(frame)
        || !admission::admission_new_uris_sorted(frame)
        || frame.seen.contains(3)
        || frame.returns.len() != 1
    { None } else {
        match (
            bridge_sint32_slot(frame, 0, 1),
            bridge_par_return_handle(frame.returns[0], arena, 1, 2),
            bridge_string_bytes_for_slot(frame.scalar_occurrences, 2, 3, 15u8),
        ) {
            (Some(bind_count), Some(body), Some(uris))
                if uris.len() == 1 && frame.seen.contains(2) =>
                Some((bind_count, body, uris[0])),
            _ => None,
        }
    }
}

pub open spec fn bridge_new_one_uri_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_new_one_uri_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((bind_count, body, uri_bytes)) =>
                match bridge_append_unattached(
                    bridge, score::ScoreNode::StringAtom(uri_bytes),
                ) {
                    None => None,
                    Some((after_uri, uri_handle)) =>
                        match bridge_append_unattached(
                            after_uri, score::ScoreNode::Integer(0i64),
                        ) {
                            None => None,
                            Some((after_injection, injection_handle)) =>
                                match bridge_append_unattached(
                                    after_injection, score::ScoreNode::Integer(bind_count),
                                ) {
                                    None => None,
                                    Some((after_count, count_handle)) =>
                                        match bridge_constructor_step(
                                            after_count, 303i32,
                                            seq![count_handle, uri_handle,
                                                injection_handle, body],
                                        ) {
                                            None => None,
                                            Some((next, constructor_handle, score_handle)) =>
                                                Some((next, count_handle, uri_handle,
                                                    injection_handle, constructor_handle,
                                                    score_handle)),
                                        },
                                },
                        },
                },
        }
    }
}

pub open spec fn bridge_new_uri_fold(
    bridge: ScoreAdmissionBridge, uris: Seq<Seq<u8>>,
) -> Option<(ScoreAdmissionBridge, Seq<usize>)>
    decreases uris.len(),
{
    if uris.len() == 0 {
        Some((bridge, Seq::empty()))
    } else {
        match bridge_append_unattached(bridge, score::ScoreNode::StringAtom(uris[0])) {
            None => None,
            Some((after_first, first)) => match bridge_new_uri_fold(
                after_first, uris.drop_first(),
            ) {
                None => None,
                Some((after_rest, rest)) => Some((after_rest, seq![first] + rest)),
            },
        }
    }
}

pub open spec fn bridge_new_uris_or_absent(
    bridge: ScoreAdmissionBridge, uris: Seq<Seq<u8>>,
) -> Option<(ScoreAdmissionBridge, Seq<usize>)> {
    if uris.len() == 0 {
        match bridge_append_unattached(bridge, score::ScoreNode::Integer(0i64)) {
            None => None,
            Some((next, handle)) => Some((next, seq![handle])),
        }
    } else {
        bridge_new_uri_fold(bridge, uris)
    }
}

pub open spec fn bridge_new_map_value(
    returned: admission::AdmissionReturnedSummary,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(Seq<u8>, Option<usize>)> {
    if returned.slot != 3 || returned.tag != 4 {
        None
    } else {
        match returned.kind {
            admission::AdmissionReturnedKind::Map {
                key_bytes, value_score_handle,
            } => match value_score_handle {
                Some(handle) if (handle as int) < arena.len()
                    && arena[handle as int].semantic =>
                    Some((key_bytes, Some(handle))),
                None => Some((key_bytes, None)),
                _ => None,
            },
            _ => None,
        }
    }
}

pub open spec fn bridge_new_injection_fold(
    bridge: ScoreAdmissionBridge,
    maps: Seq<admission::AdmissionReturnedSummary>,
) -> Option<(ScoreAdmissionBridge, Seq<usize>)>
    decreases maps.len(),
{
    if maps.len() == 0 {
        Some((bridge, Seq::empty()))
    } else {
        match bridge_new_map_value(maps[0], bridge.admission.arena) {
            None => None,
            Some((key_bytes, value)) => {
                let with_value = match value {
                    Some(value_handle) => Some((bridge, value_handle)),
                    None => match bridge_empty_par_unattached_step(bridge) {
                        None => None,
                        Some((next, _, _, value_handle)) => Some((next, value_handle)),
                    },
                };
                match with_value {
                    None => None,
                    Some((after_value, value_handle)) =>
                        match bridge_append_unattached(
                            after_value, score::ScoreNode::StringAtom(key_bytes),
                        ) {
                            None => None,
                            Some((after_key, key_handle)) =>
                                match bridge_append_unattached(
                                    after_key,
                                    score::ScoreNode::Node(seq![key_handle, value_handle]),
                                ) {
                                    None => None,
                                    Some((after_pair, pair_handle)) =>
                                        match bridge_new_injection_fold(
                                            after_pair, maps.drop_first(),
                                        ) {
                                            None => None,
                                            Some((after_rest, rest)) =>
                                                Some((after_rest, seq![pair_handle] + rest)),
                                        },
                                },
                        },
                }
            },
        }
    }
}

pub open spec fn bridge_new_injections_or_absent(
    bridge: ScoreAdmissionBridge,
    maps: Seq<admission::AdmissionReturnedSummary>,
) -> Option<(ScoreAdmissionBridge, Seq<usize>)> {
    if maps.len() == 0 {
        match bridge_append_unattached(bridge, score::ScoreNode::Integer(0i64)) {
            None => None,
            Some((next, handle)) => Some((next, seq![handle])),
        }
    } else {
        bridge_new_injection_fold(bridge, maps)
    }
}

pub open spec fn bridge_new_collections_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(i64, usize, Seq<Seq<u8>>, Seq<admission::AdmissionReturnedSummary>)> {
    if frame.family != admission::AdmissionFamily::New
        || frame.program_type_id != 20
        || !admission::admission_family_complete(frame)
        || !admission::admission_new_uris_sorted(frame)
        || frame.returns.len() == 0
    { None } else {
        match (
            bridge_sint32_slot(frame, 0, 1),
            bridge_par_return_handle(frame.returns[0], arena, 1, 2),
            bridge_string_bytes_for_slot(frame.scalar_occurrences, 2, 3, 15u8),
        ) {
            (Some(bind_count), Some(body), Some(uris))
                if frame.seen.contains(2) == (uris.len() > 0)
                    && frame.seen.contains(3) == (frame.returns.len() > 1)
                    && (uris.len() > 1 || frame.returns.len() > 2
                        || (uris.len() > 0 && frame.returns.len() > 1)) =>
                Some((bind_count, body, uris, frame.returns.drop_first())),
            _ => None,
        }
    }
}

pub open spec fn bridge_new_collections_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, Seq<usize>, Seq<usize>, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_new_collections_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((bind_count, body, uris, maps)) =>
                match bridge_new_uris_or_absent(bridge, uris) {
                    None => None,
                    Some((after_uris, uri_handles)) =>
                        match bridge_new_injections_or_absent(after_uris, maps) {
                            None => None,
                            Some((after_maps, injection_handles)) =>
                                match bridge_append_unattached(
                                    after_maps, score::ScoreNode::Integer(bind_count),
                                ) {
                                    None => None,
                                    Some((after_count, count_handle)) =>
                                        match bridge_constructor_step(
                                            after_count, 303i32,
                                            seq![count_handle] + uri_handles
                                                + injection_handles + seq![body],
                                        ) {
                                            None => None,
                                            Some((next, constructor_handle, score_handle)) =>
                                                Some((next, uri_handles, injection_handles,
                                                    count_handle, constructor_handle,
                                                    score_handle)),
                                        },
                                },
                        },
                },
        }
    }
}

pub proof fn bridge_new_uri_fold_preserves(
    bridge: ScoreAdmissionBridge, uris: Seq<Seq<u8>>,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
        bridge.admission.frames.len() > 0,
    ensures match bridge_new_uri_fold(bridge, uris) {
        None => true,
        Some((next, handles)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& next.admission.frames == bridge.admission.frames
            &&& handles.len() == uris.len()
        },
    },
    decreases uris.len(),
{
    if uris.len() == 0 {
        bridge_score_step_shape_reflexive(bridge);
    } else {
        let kind = score::ScoreNode::StringAtom(uris[0]);
        bridge_append_unattached_exact(bridge, kind);
        bridge_append_unattached_preserves_admission_wf(bridge, kind);
        bridge_append_unattached_shape(bridge, kind);
        if let Some((after_first, _)) = bridge_append_unattached(bridge, kind) {
            bridge_new_uri_fold_preserves(after_first, uris.drop_first());
            if let Some((after_rest, _)) =
                bridge_new_uri_fold(after_first, uris.drop_first())
            {
                bridge_score_step_shape_transitive(bridge, after_first, after_rest);
                assert(after_rest.admission.frames == bridge.admission.frames);
            }
        }
    }
}

pub proof fn bridge_new_uris_or_absent_preserves(
    bridge: ScoreAdmissionBridge, uris: Seq<Seq<u8>>,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
        bridge.admission.frames.len() > 0,
    ensures match bridge_new_uris_or_absent(bridge, uris) {
        None => true,
        Some((next, handles)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& next.admission.frames == bridge.admission.frames
            &&& handles.len() == if uris.len() == 0 { 1 } else { uris.len() }
        },
    },
{
    if uris.len() == 0 {
        let kind = score::ScoreNode::Integer(0i64);
        bridge_append_unattached_exact(bridge, kind);
        bridge_append_unattached_preserves_admission_wf(bridge, kind);
        bridge_append_unattached_shape(bridge, kind);
    } else {
        bridge_new_uri_fold_preserves(bridge, uris);
    }
}

pub proof fn bridge_new_injection_fold_preserves(
    bridge: ScoreAdmissionBridge,
    maps: Seq<admission::AdmissionReturnedSummary>,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
        bridge.admission.frames.len() > 0,
    ensures match bridge_new_injection_fold(bridge, maps) {
        None => true,
        Some((next, handles)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& next.admission.frames == bridge.admission.frames
            &&& handles.len() == maps.len()
        },
    },
    decreases maps.len(),
{
    if maps.len() == 0 {
        bridge_score_step_shape_reflexive(bridge);
    } else if let Some((key_bytes, value)) =
        bridge_new_map_value(maps[0], bridge.admission.arena)
    {
        if value.is_none() {
            bridge_empty_par_unattached_exact(bridge);
            bridge_empty_par_unattached_shape(bridge);
        }
        let with_value = match value {
            Some(value_handle) => Some((bridge, value_handle)),
            None => match bridge_empty_par_unattached_step(bridge) {
                None => None,
                Some((next, _, _, value_handle)) => Some((next, value_handle)),
            },
        };
        if let Some((after_value, value_handle)) = with_value {
            if value.is_some() {
                bridge_score_step_shape_reflexive(bridge);
            }
            let key_kind = score::ScoreNode::StringAtom(key_bytes);
            bridge_append_unattached_exact(after_value, key_kind);
            bridge_append_unattached_preserves_admission_wf(after_value, key_kind);
            bridge_append_unattached_shape(after_value, key_kind);
            if let Some((after_key, key_handle)) =
                bridge_append_unattached(after_value, key_kind)
            {
                let pair_kind = score::ScoreNode::Node(seq![key_handle, value_handle]);
                bridge_append_unattached_exact(after_key, pair_kind);
                bridge_append_unattached_preserves_admission_wf(after_key, pair_kind);
                bridge_append_unattached_shape(after_key, pair_kind);
                if let Some((after_pair, _)) =
                    bridge_append_unattached(after_key, pair_kind)
                {
                    bridge_new_injection_fold_preserves(
                        after_pair, maps.drop_first(),
                    );
                    if let Some((after_rest, _)) =
                        bridge_new_injection_fold(after_pair, maps.drop_first())
                    {
                        bridge_score_step_shape_transitive(
                            bridge, after_value, after_key,
                        );
                        bridge_score_step_shape_transitive(
                            bridge, after_key, after_pair,
                        );
                        bridge_score_step_shape_transitive(
                            bridge, after_pair, after_rest,
                        );
                        assert(after_rest.admission.frames == bridge.admission.frames);
                    }
                }
            }
        }
    }
}

pub proof fn bridge_new_injections_or_absent_preserves(
    bridge: ScoreAdmissionBridge,
    maps: Seq<admission::AdmissionReturnedSummary>,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
        bridge.admission.frames.len() > 0,
    ensures match bridge_new_injections_or_absent(bridge, maps) {
        None => true,
        Some((next, handles)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& next.admission.frames == bridge.admission.frames
            &&& handles.len() == if maps.len() == 0 { 1 } else { maps.len() }
        },
    },
{
    if maps.len() == 0 {
        let kind = score::ScoreNode::Integer(0i64);
        bridge_append_unattached_exact(bridge, kind);
        bridge_append_unattached_preserves_admission_wf(bridge, kind);
        bridge_append_unattached_shape(bridge, kind);
    } else {
        bridge_new_injection_fold_preserves(bridge, maps);
    }
}

pub proof fn bridge_new_collections_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_new_collections_step(bridge) {
        None => true,
        Some((next, uri_handles, injection_handles, count_handle,
            constructor_handle, score_handle)) => {
            let (bind_count, body, uris, maps) = bridge_new_collections_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& uri_handles.len() == if uris.len() == 0 { 1 } else { uris.len() }
            &&& injection_handles.len() == if maps.len() == 0 { 1 } else { maps.len() }
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, count_handle)
                == Some(score::ScoreNode::Integer(bind_count))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(303i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(
                    seq![constructor_handle, count_handle] + uri_handles
                        + injection_handles + seq![body],
                ))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        if let Some((bind_count, body, uris, maps)) = bridge_new_collections_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            bridge_new_uris_or_absent_preserves(bridge, uris);
            if let Some((after_uris, uri_handles)) = bridge_new_uris_or_absent(
                bridge, uris,
            ) {
                bridge_new_injections_or_absent_preserves(after_uris, maps);
                if let Some((after_maps, injection_handles)) =
                    bridge_new_injections_or_absent(after_uris, maps)
                {
                    let count_kind = score::ScoreNode::Integer(bind_count);
                    bridge_append_unattached_exact(after_maps, count_kind);
                    bridge_append_unattached_preserves_admission_wf(after_maps, count_kind);
                    bridge_append_unattached_shape(after_maps, count_kind);
                    if let Some((after_count, count_handle)) =
                        bridge_append_unattached(after_maps, count_kind)
                    {
                        let children = seq![count_handle] + uri_handles
                            + injection_handles + seq![body];
                        bridge_constructor_exact(after_count, 303i32, children);
                        bridge_constructor_shape(after_count, 303i32, children);
                        if let Some((next, constructor_handle, score_handle)) =
                            bridge_constructor_step(after_count, 303i32, children)
                        {
                            assert(after_count.admission.frames
                                == bridge.admission.frames);
                            bridge_score_step_shape_transitive(
                                bridge, after_uris, after_maps,
                            );
                            bridge_score_step_shape_transitive(
                                bridge, after_maps, after_count,
                            );
                            bridge_score_step_shape_transitive(
                                bridge, after_count, next,
                            );
                            assert(seq![constructor_handle] + children
                                == seq![constructor_handle, count_handle] + uri_handles
                                    + injection_handles + seq![body]);
                            assert(admission::admission_family_complete(
                                bridge.admission.frames.last(),
                            ));
                        }
                    }
                }
            }
        }
    }
}

pub open spec fn bridge_send_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(bool, usize, Seq<usize>, bool, Seq<usize>)> {
    if frame.family != admission::AdmissionFamily::Send
        || frame.program_type_id != 15
        || !admission::admission_family_complete(frame)
    { None } else {
        match (
            bridge_par_handles_for_slot(frame.returns, arena, 0, 1),
            bridge_par_handles_for_slot(frame.returns, arena, 1, 2),
            bridge_cost_handles_for_slot(frame.returns, arena, 5, 7),
            bridge_bool_slot(frame, 2, 3),
            bridge_bool_slot(frame, 4, 6),
        ) {
            (
                Some(channel), Some(data), Some(cost),
                Some(persistent), Some(connective_used),
            ) if channel.len() == 1 && cost.len() <= 1
                && frame.returns.len() == channel.len() + data.len() + cost.len() =>
                    Some((persistent, channel[0], data, connective_used, cost)),
            _ => None,
        }
    }
}

pub open spec fn bridge_send_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_send_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((persistent, channel, data, connective_used, cost)) => {
                let persistent_flag = if persistent { 1i64 } else { 0i64 };
                let connective_flag = if connective_used { 1i64 } else { 0i64 };
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(persistent_flag),
                ) {
                    None => None,
                    Some((after_persistent, persistent_handle)) =>
                        match bridge_append_unattached(
                            after_persistent, score::ScoreNode::Integer(connective_flag),
                        ) {
                            None => None,
                            Some((after_connective, connective_handle)) => {
                                let children = seq![persistent_handle, channel]
                                    + data + seq![connective_handle] + cost;
                                match bridge_constructor_step(
                                    after_connective, 300i32, children,
                                ) {
                                    None => None,
                                    Some((next, constructor_handle, score_handle)) =>
                                        Some((next, persistent_handle, connective_handle,
                                            constructor_handle, score_handle)),
                                }
                            },
                        },
                }
            },
        }
    }
}

pub open spec fn bridge_match_parts(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(usize, Seq<usize>, bool)> {
    if frame.family != admission::AdmissionFamily::Match
        || frame.program_type_id != 22
        || !admission::admission_family_complete(frame)
        || !admission::admission_family_admissible(frame)
    { None } else {
        match (
            bridge_par_handles_for_slot(frame.returns, arena, 0, 1),
            bridge_match_case_handles(frame.returns, arena),
            bridge_bool_slot(frame, 3, 5),
        ) {
            (Some(target), Some(cases), Some(connective_used))
                if target.len() == 1
                    && frame.returns.len() == target.len() + cases.len() =>
                        Some((target[0], cases, connective_used)),
            _ => None,
        }
    }
}

pub open spec fn bridge_match_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        match bridge_match_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            None => None,
            Some((target, cases, connective_used)) => {
                let flag = if connective_used { 1i64 } else { 0i64 };
                match bridge_append_unattached(bridge, score::ScoreNode::Integer(flag)) {
                    None => None,
                    Some((after_flag, flag_handle)) => {
                        let children = seq![target] + cases + seq![flag_handle];
                        match bridge_constructor_step(after_flag, 304i32, children) {
                            None => None,
                            Some((next, constructor_handle, score_handle)) =>
                                Some((next, flag_handle, constructor_handle, score_handle)),
                        }
                    },
                }
            },
        }
    }
}

pub open spec fn bridge_absent_specialized_constructor_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if !((top.family == admission::AdmissionFamily::Expr
            && top.program_type_id == 24)
            || (top.family == admission::AdmissionFamily::GUnforgeable
                && top.program_type_id == 60))
            || top.seen.contains(0)
            || top.scalar_slots.contains(0)
            || top.scalar_occurrences.len() != 0
            || top.returns.len() != 0
        {
            None
        } else {
            bridge_constructor_step(bridge, 0i32, Seq::empty())
        }
    }
}

pub open spec fn bridge_absent_connective_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if top.family != admission::AdmissionFamily::Connective
            || top.program_type_id != 55
            || top.seen.contains(0)
            || top.scalar_slots.contains(0)
            || top.scalar_occurrences.len() != 0
            || top.returns.len() != 0
        {
            None
        } else {
            match bridge_append_unattached(bridge, score::ScoreNode::Integer(0i64)) {
                None => None,
                Some((after_integer, score_handle)) =>
                    match admission::admission_attach_score_step(
                        after_integer.admission, score_handle, top.body_id,
                    ) {
                        None => None,
                        Some(admission) => Some((ScoreAdmissionBridge {
                            admission, transcript: after_integer.transcript,
                        }, score_handle)),
                    },
            }
        }
    }
}

pub open spec fn bridge_region_scalar(
    frame: admission::AdmissionFrameState, slot: usize,
) -> Option<u64> {
    let scalar_kind = if slot == 0 || slot == 6 { 4u8 } else { 7u8 };
    if slot > 6 || slot == 5 {
        None
    } else {
        match bridge_scalar_slot(
            frame.scalar_occurrences, slot, (slot as int + 1) as u32, scalar_kind,
        ) {
            None => None,
            Some(None) => if frame.seen.contains(slot as int)
                || frame.scalar_slots.contains(slot as int)
            {
                None
            } else {
                Some(0)
            },
            Some(Some(value)) => {
                if !frame.seen.contains(slot as int)
                    || !frame.scalar_slots.contains(slot as int)
                    || value == 0
                    || ((slot == 0 || slot == 6) && value > u32::MAX as u64)
                {
                    None
                } else {
                    Some(value)
                }
            },
        }
    }
}

pub open spec fn bridge_region_signer(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<Option<usize>> {
    if frame.returns.len() == 0 {
        if frame.seen.contains(5) { None } else { Some(None) }
    } else if frame.returns.len() == 1 {
        let returned = frame.returns[0];
        match returned.score_handle {
            None => None,
            Some(handle) => {
                if returned.slot != 5 || returned.tag != 6
                    || returned.kind != admission::AdmissionReturnedKind::CostScore
                    || !frame.seen.contains(5)
                    || handle as int >= arena.len()
                    || returned.score_origin_body_id
                        != Some(arena[handle as int].body_id)
                    || !arena[handle as int].semantic
                {
                    None
                } else {
                    Some(Some(handle))
                }
            },
        }
    } else {
        None
    }
}

pub open spec fn bridge_region_fields(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<(u32, Seq<u64>, Option<usize>, u32)> {
    match bridge_region_scalar(frame, 0) {
        None => None,
        Some(version) => match bridge_region_scalar(frame, 1) {
            None => None,
            Some(start_line) => match bridge_region_scalar(frame, 2) {
                None => None,
                Some(start_column) => match bridge_region_scalar(frame, 3) {
                    None => None,
                    Some(end_line) => match bridge_region_scalar(frame, 4) {
                        None => None,
                        Some(end_column) => match bridge_region_signer(frame, arena) {
                            None => None,
                            Some(signer) => match bridge_region_scalar(frame, 6) {
                                None => None,
                                Some(ordinal) => Some((
                                    version as u32,
                                    seq![start_line, start_column, end_line, end_column],
                                    signer,
                                    ordinal as u32,
                                )),
                            },
                        },
                    },
                },
            },
        },
    }
}

pub open spec fn bridge_append_coordinate_bytes(
    bridge: ScoreAdmissionBridge, coordinates: Seq<u64>,
) -> Option<(ScoreAdmissionBridge, Seq<usize>)>
    decreases coordinates.len(),
{
    if coordinates.len() == 0 {
        Some((bridge, Seq::empty()))
    } else {
        match bridge_append_unattached(
            bridge,
            score::ScoreNode::BytesAtom(score::big_endian_eight(coordinates[0])),
        ) {
            None => None,
            Some((after_first, first_handle)) => {
                match bridge_append_coordinate_bytes(
                    after_first, coordinates.drop_first(),
                ) {
                    None => None,
                    Some((after_tail, tail_handles)) =>
                        Some((after_tail, seq![first_handle] + tail_handles)),
                }
            },
        }
    }
}

pub proof fn bridge_append_coordinate_bytes_exact(
    bridge: ScoreAdmissionBridge, coordinates: Seq<u64>,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_append_coordinate_bytes(bridge, coordinates) {
        None => true,
        Some((next, handles)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.transcript.entries.take(bridge.transcript.entries.len() as int)
                == bridge.transcript.entries
            &&& next.admission.arena.take(bridge.admission.arena.len() as int)
                == bridge.admission.arena
            &&& handles.len() == coordinates.len()
            &&& next.transcript.entries.len()
                == bridge.transcript.entries.len() + coordinates.len()
            &&& next.admission.arena.len()
                == bridge.admission.arena.len() + coordinates.len()
            &&& forall|i: int| 0 <= i < handles.len() ==> {
                &&& handles[i] as int == bridge.transcript.entries.len() + i
                &&& score::handle_contents(next.transcript, handles[i])
                    == Some(score::ScoreNode::BytesAtom(
                        score::big_endian_eight(coordinates[i]),
                    ))
            }
        },
    },
    decreases coordinates.len(),
{
    if coordinates.len() > 0 {
        let kind = score::ScoreNode::BytesAtom(
            score::big_endian_eight(coordinates[0]),
        );
        bridge_append_unattached_exact(bridge, kind);
        bridge_append_unattached_preserves_admission_wf(bridge, kind);
        if let Some((after_first, first_handle)) = bridge_append_unattached(bridge, kind) {
            bridge_append_coordinate_bytes_exact(
                after_first, coordinates.drop_first(),
            );
            if let Some((next, tail_handles)) = bridge_append_coordinate_bytes(
                after_first, coordinates.drop_first(),
            ) {
                assert(next.admission.arena.len()
                    == after_first.admission.arena.len()
                        + coordinates.drop_first().len());
                assert(next.transcript.entries.take(
                    after_first.transcript.entries.len() as int,
                ) == after_first.transcript.entries);
                assert(first_handle as int == bridge.transcript.entries.len());
                assert(after_first.transcript.entries.len()
                    == bridge.transcript.entries.len() + 1);
                assert((first_handle as int) < after_first.transcript.entries.len());
                assert(next.transcript.entries.len()
                    == after_first.transcript.entries.len()
                        + coordinates.drop_first().len());
                assert(after_first.transcript.entries.len() <= next.transcript.entries.len());
                assert(next.transcript.entries[first_handle as int]
                    == after_first.transcript.entries[first_handle as int]);
                assert(score::handle_contents(next.transcript, first_handle)
                    == Some(kind));
                assert(next.transcript.entries.len()
                    == bridge.transcript.entries.len() + coordinates.len());
                assert(next.admission.arena.len()
                    == bridge.admission.arena.len() + coordinates.len());
                bridge_score_prefix_transitive(
                    bridge.transcript.entries,
                    after_first.transcript.entries,
                    next.transcript.entries,
                );
                bridge_binding_prefix_transitive(
                    bridge.admission.arena,
                    after_first.admission.arena,
                    next.admission.arena,
                );
                let handles = seq![first_handle] + tail_handles;
                assert(handles.len() == coordinates.len());
                assert forall|i: int| 0 <= i < handles.len()
                    implies handles[i] as int == bridge.transcript.entries.len() + i
                        && score::handle_contents(next.transcript, handles[i])
                            == Some(score::ScoreNode::BytesAtom(
                                score::big_endian_eight(coordinates[i]),
                            )) by {
                    if i > 0 {
                        assert(handles[i] == tail_handles[i - 1]);
                        assert(coordinates[i] == coordinates.drop_first()[i - 1]);
                        assert(tail_handles[i - 1] as int
                            == after_first.transcript.entries.len() + (i - 1));
                        assert(score::handle_contents(next.transcript, tail_handles[i - 1])
                            == Some(score::ScoreNode::BytesAtom(
                                score::big_endian_eight(coordinates.drop_first()[i - 1]),
                            )));
                    }
                }
            }
        }
    }
}

pub proof fn bridge_append_coordinate_bytes_shape(
    bridge: ScoreAdmissionBridge, coordinates: Seq<u64>,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_append_coordinate_bytes(bridge, coordinates) {
        None => true,
        Some((next, _)) => bridge_score_step_shape(bridge, next),
    },
    decreases coordinates.len(),
{
    if coordinates.len() == 0 {
        bridge_score_step_shape_reflexive(bridge);
    } else {
        let kind = score::ScoreNode::BytesAtom(
            score::big_endian_eight(coordinates[0]),
        );
        bridge_append_unattached_shape(bridge, kind);
        bridge_append_unattached_exact(bridge, kind);
        bridge_append_unattached_preserves_admission_wf(bridge, kind);
        if let Some((after_first, _)) = bridge_append_unattached(bridge, kind) {
            bridge_append_coordinate_bytes_shape(
                after_first, coordinates.drop_first(),
            );
            if let Some((next, _)) = bridge_append_coordinate_bytes(
                after_first, coordinates.drop_first(),
            ) {
                bridge_score_step_shape_transitive(bridge, after_first, next);
            }
        }
    }
}

pub open spec fn bridge_absent_signer_unattached(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize)> {
    match bridge_append_unattached(bridge, score::ScoreNode::Integer(506i64)) {
        None => None,
        Some((after_constructor, constructor_handle)) => bridge_append_unattached(
            after_constructor, score::ScoreNode::Node(seq![constructor_handle]),
        ),
    }
}

pub proof fn bridge_absent_signer_unattached_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_absent_signer_unattached(bridge) {
        None => true,
        Some((next, signer_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& signer_handle as int == bridge.transcript.entries.len() + 1
            &&& next.transcript.entries.len() == bridge.transcript.entries.len() + 2
            &&& next.admission.arena.len() == bridge.admission.arena.len() + 2
            &&& next.transcript.entries.take(bridge.transcript.entries.len() as int)
                == bridge.transcript.entries
            &&& next.admission.arena.take(bridge.admission.arena.len() as int)
                == bridge.admission.arena
            &&& score::handle_contents(next.transcript, signer_handle)
                == Some(score::ScoreNode::Node(seq![
                    bridge.transcript.entries.len() as usize,
                ]))
            &&& score::handle_contents(
                next.transcript, bridge.transcript.entries.len() as usize,
            ) == Some(score::ScoreNode::Integer(506i64))
        },
    },
{
    bridge_append_unattached_exact(bridge, score::ScoreNode::Integer(506i64));
    bridge_append_unattached_preserves_admission_wf(
        bridge, score::ScoreNode::Integer(506i64),
    );
    if let Some((after_constructor, constructor_handle)) = bridge_append_unattached(
        bridge, score::ScoreNode::Integer(506i64),
    ) {
        bridge_append_unattached_exact(
            after_constructor, score::ScoreNode::Node(seq![constructor_handle]),
        );
        bridge_append_unattached_preserves_admission_wf(
            after_constructor, score::ScoreNode::Node(seq![constructor_handle]),
        );
        if let Some((next, _)) = bridge_append_unattached(
            after_constructor, score::ScoreNode::Node(seq![constructor_handle]),
        ) {
            bridge_score_prefix_transitive(
                bridge.transcript.entries,
                after_constructor.transcript.entries,
                next.transcript.entries,
            );
            bridge_binding_prefix_transitive(
                bridge.admission.arena,
                after_constructor.admission.arena,
                next.admission.arena,
            );
        }
    }
}

pub proof fn bridge_absent_signer_unattached_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_absent_signer_unattached(bridge) {
        None => true,
        Some((next, _)) => bridge_score_step_shape(bridge, next),
    },
{
    let first_kind = score::ScoreNode::Integer(506i64);
    bridge_append_unattached_shape(bridge, first_kind);
    bridge_append_unattached_exact(bridge, first_kind);
    bridge_append_unattached_preserves_admission_wf(bridge, first_kind);
    if let Some((after_constructor, constructor_handle)) = bridge_append_unattached(
        bridge, first_kind,
    ) {
        let second_kind = score::ScoreNode::Node(seq![constructor_handle]);
        bridge_append_unattached_shape(after_constructor, second_kind);
        if let Some((next, _)) = bridge_append_unattached(
            after_constructor, second_kind,
        ) {
            bridge_score_step_shape_transitive(
                bridge, after_constructor, next,
            );
        }
    }
}

pub open spec fn bridge_cost_region_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, Seq<usize>, usize, usize)> {
    if bridge.admission.frames.len() == 0
        || bridge.admission.frames.last().family != admission::AdmissionFamily::CostRegionFrame
        || bridge.admission.frames.last().program_type_id != 12
    {
        None
    } else {
        let top = bridge.admission.frames.last();
        match bridge_region_fields(top, bridge.admission.arena) {
            None => None,
            Some((version, coordinates, signer, ordinal)) => {
                match bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(version as i64),
                ) {
                    None => None,
                    Some((after_version, version_handle)) => {
                        match bridge_append_coordinate_bytes(after_version, coordinates) {
                            None => None,
                            Some((after_coordinates, coordinate_handles)) => {
                                let signer_result = match signer {
                                    Some(handle) => Some((after_coordinates, handle)),
                                    None => bridge_absent_signer_unattached(after_coordinates),
                                };
                                match signer_result {
                                    None => None,
                                    Some((after_signer, signer_handle)) => {
                                        match bridge_append_unattached(
                                            after_signer,
                                            score::ScoreNode::Integer(ordinal as i64),
                                        ) {
                                            None => None,
                                            Some((after_ordinal, ordinal_handle)) => {
                                                let children = seq![version_handle]
                                                    + coordinate_handles
                                                    + seq![signer_handle, ordinal_handle];
                                                match bridge_constructor_step(
                                                    after_ordinal, 500i32, children,
                                                ) {
                                                    None => None,
                                                    Some((next, constructor_handle, score_handle)) =>
                                                        Some((next, children, constructor_handle, score_handle)),
                                                }
                                            },
                                        }
                                    },
                                }
                            },
                        }
                    },
                }
            },
        }
    }
}

pub proof fn bridge_cost_region_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_region_step(bridge) {
        None => true,
        Some((next, children, constructor_handle, score_handle)) => {
            let (version, coordinates, signer, ordinal) = bridge_region_fields(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& children.len() == 7
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(500i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
            &&& score::handle_contents(next.transcript, children[0])
                == Some(score::ScoreNode::Integer(version as i64))
            &&& forall|i: int| 0 <= i < 4 ==>
                score::handle_contents(next.transcript, children[i + 1])
                    == Some(score::ScoreNode::BytesAtom(
                        score::big_endian_eight(coordinates[i]),
                    ))
            &&& (match signer {
                Some(handle) => children[5] == handle,
                None => exists|tag_handle: usize|
                    score::handle_contents(next.transcript, tag_handle)
                        == Some(score::ScoreNode::Integer(506i64))
                        && score::handle_contents(next.transcript, children[5])
                            == Some(score::ScoreNode::Node(seq![tag_handle])),
            })
            &&& score::handle_contents(next.transcript, children[6])
                == Some(score::ScoreNode::Integer(ordinal as i64))
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].encoded_len
                == next.transcript.entries[score_handle as int].encoded_len
        },
    },
{
    if bridge.admission.frames.len() > 0
        && bridge.admission.frames.last().family == admission::AdmissionFamily::CostRegionFrame
        && bridge.admission.frames.last().program_type_id == 12
    {
        let top = bridge.admission.frames.last();
        if let Some((version, coordinates, signer, ordinal)) = bridge_region_fields(
            top, bridge.admission.arena,
        ) {
            let version_kind = score::ScoreNode::Integer(version as i64);
            bridge_append_unattached_exact(bridge, version_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, version_kind);
            if let Some((after_version, version_handle)) = bridge_append_unattached(
                bridge, version_kind,
            ) {
                bridge_append_coordinate_bytes_exact(after_version, coordinates);
                if let Some((after_coordinates, coordinate_handles)) =
                    bridge_append_coordinate_bytes(after_version, coordinates)
                {
                    if signer.is_none() {
                        bridge_absent_signer_unattached_exact(after_coordinates);
                    }
                    let signer_result = match signer {
                        Some(handle) => Some((after_coordinates, handle)),
                        None => bridge_absent_signer_unattached(after_coordinates),
                    };
                    if let Some((after_signer, signer_handle)) = signer_result {
                        let ordinal_kind = score::ScoreNode::Integer(ordinal as i64);
                        bridge_append_unattached_exact(after_signer, ordinal_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_signer, ordinal_kind,
                        );
                        if let Some((after_ordinal, ordinal_handle)) =
                            bridge_append_unattached(after_signer, ordinal_kind)
                        {
                            let children = seq![version_handle] + coordinate_handles
                                + seq![signer_handle, ordinal_handle];
                            bridge_constructor_exact(after_ordinal, 500i32, children);
                            assert(coordinates.len() == 4);
                            assert(coordinate_handles.len() == 4);
                            assert(children.len() == 7);
                            assert(children[0] == version_handle);
                            assert(children[5] == signer_handle);
                            assert(children[6] == ordinal_handle);
                            assert forall|i: int| 0 <= i < 4
                                implies children[i + 1] == coordinate_handles[i] by {}
                            if let Some((next, constructor_handle, score_handle)) =
                                bridge_constructor_step(after_ordinal, 500i32, children)
                            {
                                assert(after_ordinal.transcript.entries.take(
                                    after_signer.transcript.entries.len() as int,
                                ) == after_signer.transcript.entries);
                                if signer.is_some() {
                                    assert(after_signer.transcript.entries
                                        == after_coordinates.transcript.entries);
                                } else {
                                    assert(after_signer.transcript.entries.take(
                                        after_coordinates.transcript.entries.len() as int,
                                    ) == after_coordinates.transcript.entries);
                                }
                                assert(after_signer.transcript.entries.take(
                                    after_coordinates.transcript.entries.len() as int,
                                ) == after_coordinates.transcript.entries);
                                bridge_score_prefix_transitive(
                                    after_coordinates.transcript.entries,
                                    after_signer.transcript.entries,
                                    after_ordinal.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_version.transcript.entries,
                                    after_coordinates.transcript.entries,
                                    after_ordinal.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_version.transcript.entries,
                                    after_ordinal.transcript.entries,
                                    next.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_coordinates.transcript.entries,
                                    after_ordinal.transcript.entries,
                                    next.transcript.entries,
                                );
                                bridge_score_handle_preserved(
                                    after_version.transcript, next.transcript, version_handle,
                                );
                                assert(score::handle_contents(next.transcript, children[0])
                                    == Some(version_kind));
                                assert forall|i: int| 0 <= i < 4
                                    implies score::handle_contents(next.transcript, children[i + 1])
                                        == Some(score::ScoreNode::BytesAtom(
                                            score::big_endian_eight(coordinates[i]),
                                        )) by {
                                    bridge_score_handle_preserved(
                                        after_coordinates.transcript,
                                        next.transcript,
                                        coordinate_handles[i],
                                    );
                                }
                                bridge_score_prefix_transitive(
                                    after_signer.transcript.entries,
                                    after_ordinal.transcript.entries,
                                    next.transcript.entries,
                                );
                                bridge_score_handle_preserved(
                                    after_ordinal.transcript, next.transcript, ordinal_handle,
                                );
                                if signer.is_none() {
                                    bridge_score_handle_preserved(
                                        after_signer.transcript, next.transcript, signer_handle,
                                    );
                                    let tag_handle =
                                        after_coordinates.transcript.entries.len() as usize;
                                    bridge_score_handle_preserved(
                                        after_signer.transcript, next.transcript, tag_handle,
                                    );
                                    assert(score::handle_contents(next.transcript, tag_handle)
                                        == Some(score::ScoreNode::Integer(506i64)));
                                    assert(score::handle_contents(next.transcript, children[5])
                                        == Some(score::ScoreNode::Node(seq![tag_handle])));
                                }
                                assert(score::handle_contents(next.transcript, children[6])
                                    == Some(ordinal_kind));
                                assert(next.admission.arena[score_handle as int].encoded_len
                                    == next.transcript.entries[score_handle as int].encoded_len);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_cost_region_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_region_step(bridge) {
        None => true,
        Some((next, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if bridge.admission.frames.len() > 0
        && bridge.admission.frames.last().family
            == admission::AdmissionFamily::CostRegionFrame
        && bridge.admission.frames.last().program_type_id == 12
    {
        let top = bridge.admission.frames.last();
        if let Some((version, coordinates, signer, ordinal)) = bridge_region_fields(
            top, bridge.admission.arena,
        ) {
            let version_kind = score::ScoreNode::Integer(version as i64);
            bridge_append_unattached_shape(bridge, version_kind);
            bridge_append_unattached_exact(bridge, version_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, version_kind);
            if let Some((after_version, version_handle)) = bridge_append_unattached(
                bridge, version_kind,
            ) {
                bridge_append_coordinate_bytes_shape(after_version, coordinates);
                bridge_append_coordinate_bytes_exact(after_version, coordinates);
                if let Some((after_coordinates, coordinate_handles)) =
                    bridge_append_coordinate_bytes(after_version, coordinates)
                {
                    if signer.is_none() {
                        bridge_absent_signer_unattached_shape(after_coordinates);
                        bridge_absent_signer_unattached_exact(after_coordinates);
                    }
                    let signer_result = match signer {
                        Some(handle) => Some((after_coordinates, handle)),
                        None => bridge_absent_signer_unattached(after_coordinates),
                    };
                    if let Some((after_signer, signer_handle)) = signer_result {
                        if signer.is_some() {
                            bridge_score_step_shape_reflexive(after_coordinates);
                        }
                        bridge_score_step_shape_transitive(
                            after_version, after_coordinates, after_signer,
                        );
                        let ordinal_kind = score::ScoreNode::Integer(ordinal as i64);
                        bridge_append_unattached_shape(after_signer, ordinal_kind);
                        bridge_append_unattached_exact(after_signer, ordinal_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_signer, ordinal_kind,
                        );
                        if let Some((after_ordinal, ordinal_handle)) =
                            bridge_append_unattached(after_signer, ordinal_kind)
                        {
                            let children = seq![version_handle] + coordinate_handles
                                + seq![signer_handle, ordinal_handle];
                            bridge_constructor_shape(after_ordinal, 500i32, children);
                            if let Some((next, _, _)) = bridge_constructor_step(
                                after_ordinal, 500i32, children,
                            ) {
                                bridge_score_step_shape_transitive(
                                    after_version, after_signer, after_ordinal,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_version, after_ordinal,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_ordinal, next,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

pub open spec fn bridge_cost_token_stack_transcript(
    bridge: ScoreAdmissionBridge, layers: Seq<usize>,
) -> Seq<u8> {
    bridge_constructor_transcript(bridge, 508i32, layers)
}

pub open spec fn bridge_raw_par_score_tags() -> Seq<u32> {
    seq![1u32, 2u32, 5u32, 4u32, 6u32, 11u32, 8u32, 7u32, 12u32, 13u32]
}

pub open spec fn bridge_raw_par_tag_slot(tag: u32) -> Option<usize> {
    match tag {
        1 => Some(0), 2 => Some(1), 4 => Some(2), 5 => Some(3),
        6 => Some(4), 7 => Some(5), 8 => Some(6), 11 => Some(9),
        12 => Some(10), 13 => Some(11), _ => None,
    }
}

pub open spec fn bridge_raw_par_returns_source(
    frame: admission::AdmissionFrameState,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> bool {
    &&& frame.frame_kind == 1
    &&& frame.program_type_id == 1
    &&& frame.family == admission::AdmissionFamily::Other
    &&& frame.cursor == frame.region_end
    &&& frame.waiting_id.is_none()
    &&& frame.score_handle.is_none()
    &&& forall|i: int| 0 <= i < frame.returns.len() ==> {
        let returned = #[trigger] frame.returns[i];
        match (
            bridge_raw_par_tag_slot(returned.tag), returned.kind,
            returned.score_handle, returned.score_origin_body_id,
        ) {
            (
                Some(slot),
                admission::AdmissionReturnedKind::Component { family_tag, .. },
                Some(handle), Some(origin),
            ) => {
                &&& returned.slot == slot
                &&& family_tag == returned.tag
                &&& (handle as int) < arena.len()
                &&& origin == arena[handle as int].body_id
            },
            _ => false,
        }
    }
}

pub open spec fn bridge_raw_par_handles_for_tag(
    summaries: Seq<admission::AdmissionReturnedSummary>, tag: u32,
) -> Option<Seq<usize>>
    decreases summaries.len(),
{
    if summaries.len() == 0 {
        Some(Seq::empty())
    } else {
        let first = summaries[0];
        match bridge_raw_par_handles_for_tag(summaries.drop_first(), tag) {
            None => None,
            Some(rest) => if first.tag != tag {
                Some(rest)
            } else {
                match (first.kind, first.score_handle) {
                    (admission::AdmissionReturnedKind::Component { family_tag, .. }, Some(handle))
                        if family_tag == tag => Some(seq![handle] + rest),
                    _ => None,
                }
            },
        }
    }
}

pub open spec fn bridge_raw_par_ordered_handles(
    summaries: Seq<admission::AdmissionReturnedSummary>, tags: Seq<u32>,
) -> Option<Seq<usize>>
    decreases tags.len(),
{
    if tags.len() == 0 {
        Some(Seq::empty())
    } else {
        match (
            bridge_raw_par_handles_for_tag(summaries, tags[0]),
            bridge_raw_par_ordered_handles(summaries, tags.drop_first()),
        ) {
            (Some(first), Some(rest)) => Some(first + rest),
            _ => None,
        }
    }
}

pub open spec fn bridge_raw_par_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if !bridge_raw_par_returns_source(top, bridge.admission.arena) {
            None
        } else {
            match bridge_raw_par_ordered_handles(
                top.returns, bridge_raw_par_score_tags(),
            ) {
                None => None,
                Some(components) => {
                    let flag = if admission::admission_raw_par_connective_used(top) {
                        1i64
                    } else { 0i64 };
                    match bridge_append_unattached(
                        bridge, score::ScoreNode::Integer(flag),
                    ) {
                        None => None,
                        Some((after_flag, flag_handle)) => match bridge_constructor_step(
                            after_flag, 999i32, components.push(flag_handle),
                        ) {
                            None => None,
                            Some((next, constructor_handle, score_handle)) =>
                                Some((next, flag_handle, constructor_handle, score_handle)),
                        },
                    }
                },
            }
        }
    }
}

pub open spec fn bridge_nonsemantic_family(family: admission::AdmissionFamily) -> bool {
    match family {
        admission::AdmissionFamily::Send
        | admission::AdmissionFamily::Receive
        | admission::AdmissionFamily::New
        | admission::AdmissionFamily::Match
        | admission::AdmissionFamily::Bundle
        | admission::AdmissionFamily::If
        | admission::AdmissionFamily::Expr => true,
        _ => false,
    }
}

pub open spec fn bridge_nonsemantic_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize)> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let top = bridge.admission.frames.last();
        if !bridge_nonsemantic_family(top.family)
            || admission::admission_family_complete(top)
        {
            None
        } else {
            bridge_constructor_step(bridge, i32::MAX, Seq::empty())
        }
    }
}

pub proof fn bridge_old_children_transcript_preserved(
    old: Seq<score::ScoreEntry>, next: Seq<score::ScoreEntry>, children: Seq<usize>,
)
    requires
        old.len() <= next.len(),
        next.take(old.len() as int) == old,
        score::child_handles_valid(old, children),
    ensures score::children_transcript(next, children)
        == score::children_transcript(old, children),
    decreases children.len(),
{
    if children.len() > 0 {
        let tail = children.drop_first();
        assert(score::child_handles_valid(old, tail)) by {
            assert forall|i: int| 0 <= i < tail.len()
                implies (tail[i] as int) < old.len() by {
                assert(tail[i] == children[i + 1]);
            }
        }
        bridge_old_children_transcript_preserved(old, next, tail);
        assert(next[children[0] as int] == old[children[0] as int]);
    }
}

pub proof fn bridge_constructor_exact(
    bridge: ScoreAdmissionBridge, constructor: i32, children: Seq<usize>,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_constructor_step(bridge, constructor, children) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& constructor_handle as int == bridge.transcript.entries.len()
            &&& score_handle as int == bridge.transcript.entries.len() + 1
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(constructor as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
            &&& next.transcript.entries.take(bridge.transcript.entries.len() as int)
                == bridge.transcript.entries
            &&& next.admission.arena.take(bridge.admission.arena.len() as int)
                == bridge.admission.arena
            &&& score::handle_transcript(next.transcript, score_handle)
                == Some(bridge_constructor_transcript(bridge, constructor, children))
            &&& next.admission.arena[score_handle as int].encoded_len as int
                == bridge_constructor_transcript(bridge, constructor, children).len()
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == bridge_constructor_transcript(bridge, constructor, children)
            &&& next.admission.arena[score_handle as int].body_id
                == bridge.admission.frames.last().body_id
            &&& next.admission.arena[score_handle as int].semantic
                == admission::admission_family_complete(bridge.admission.frames.last())
        },
    },
{
    if bridge.admission.frames.len() > 0
        && score::child_handles_valid(bridge.transcript.entries, children)
    {
        let top = bridge.admission.frames.last();
            bridge_append_unattached_exact(
                bridge, score::ScoreNode::Integer(constructor as i64),
            );
            bridge_append_unattached_preserves_admission_wf(
                bridge, score::ScoreNode::Integer(constructor as i64),
            );
            if let Some((after_constructor, constructor_handle)) =
                bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(constructor as i64),
                )
            {
                let all_children = seq![constructor_handle] + children;
                assert(score::child_handles_valid(after_constructor.transcript.entries, all_children)) by {
                    assert forall|i: int| 0 <= i < all_children.len()
                        implies (all_children[i] as int) < after_constructor.transcript.entries.len() by {
                        if i > 0 {
                            assert(all_children[i] == children[i - 1]);
                        }
                    }
                }
                score::append_node_exact(after_constructor.transcript, all_children);
                bridge_append_unattached_exact(
                    after_constructor, score::ScoreNode::Node(all_children),
                );
                bridge_append_unattached_preserves_admission_wf(
                    after_constructor, score::ScoreNode::Node(all_children),
                );
                if let Some((after_node, score_handle)) = bridge_append_unattached(
                    after_constructor, score::ScoreNode::Node(all_children),
                ) {
                    assert(after_node.transcript.entries.take(
                        after_constructor.transcript.entries.len() as int,
                    ) == after_constructor.transcript.entries);
                    assert(after_constructor.transcript.entries.take(
                        bridge.transcript.entries.len() as int,
                    ) == bridge.transcript.entries);
                    assert(after_node.admission.arena.take(
                        after_constructor.admission.arena.len() as int,
                    ) == after_constructor.admission.arena);
                    assert(after_constructor.admission.arena.take(
                        bridge.admission.arena.len() as int,
                    ) == bridge.admission.arena);
                    bridge_score_prefix_transitive(
                        bridge.transcript.entries,
                        after_constructor.transcript.entries,
                        after_node.transcript.entries,
                    );
                    bridge_binding_prefix_transitive(
                        bridge.admission.arena,
                        after_constructor.admission.arena,
                        after_node.admission.arena,
                    );
                    bridge_old_children_transcript_preserved(
                        bridge.transcript.entries,
                        after_constructor.transcript.entries,
                        children,
                    );
                    assert(all_children.len() > 0);
                    assert(all_children[0] == constructor_handle);
                    assert(all_children.drop_first() == children);
                    assert((constructor_handle as int)
                        < after_constructor.transcript.entries.len());
                    assert(after_constructor.transcript.entries[constructor_handle as int].transcript
                        == score::integer_transcript(constructor as i64));
                    assert(score::children_transcript(
                        after_constructor.transcript.entries, all_children,
                    ) == after_constructor.transcript.entries[constructor_handle as int].transcript
                        + score::children_transcript(
                            after_constructor.transcript.entries, children,
                        ));
                    assert(score::children_transcript(
                        after_constructor.transcript.entries, all_children,
                    ) == score::integer_transcript(constructor as i64)
                        + score::children_transcript(bridge.transcript.entries, children));
                    if let Some(admission) = admission::admission_attach_score_step(
                        after_node.admission, score_handle, top.body_id,
                    ) {
                        admission::admission_attach_score_preserves_wf(
                            after_node.admission, score_handle, top.body_id,
                        );
                        assert(admission.frames.last().score_handle == Some(score_handle));
                        assert(admission.arena == after_node.admission.arena);
                    }
                }
            }
    }
}

pub proof fn bridge_constructor_shape(
    bridge: ScoreAdmissionBridge, constructor: i32, children: Seq<usize>,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_constructor_step(bridge, constructor, children) {
        None => true,
        Some((next, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    let first_kind = score::ScoreNode::Integer(constructor as i64);
    bridge_append_unattached_shape(bridge, first_kind);
    bridge_append_unattached_preserves_admission_wf(bridge, first_kind);
    if let Some((after_constructor, constructor_handle)) = bridge_append_unattached(
        bridge, first_kind,
    ) {
        bridge_append_unattached_exact(bridge, first_kind);
        let second_kind = score::ScoreNode::Node(seq![constructor_handle] + children);
        bridge_append_unattached_shape(after_constructor, second_kind);
        bridge_append_unattached_preserves_admission_wf(after_constructor, second_kind);
        if let Some((after_node, score_handle)) = bridge_append_unattached(
            after_constructor, second_kind,
        ) {
            bridge_append_unattached_exact(after_constructor, second_kind);
            let body_id = bridge.admission.frames.last().body_id;
            bridge_attach_score_shape(after_node, score_handle, body_id);
            if let Some(admission) = admission::admission_attach_score_step(
                after_node.admission, score_handle, body_id,
            ) {
                let next = ScoreAdmissionBridge {
                    admission, transcript: after_node.transcript,
                };
                bridge_score_step_shape_transitive(
                    bridge, after_constructor, after_node,
                );
                bridge_score_step_shape_transitive(
                    bridge, after_node, next,
                );
            }
        }
    }
}

pub proof fn bridge_bundle_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_bundle_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            let (body, read, write) = bridge_bundle_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            let constructor = bridge_bundle_constructor(read, write);
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(constructor as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle, body]))
            &&& score::handle_transcript(next.transcript, score_handle)
                == Some(bridge_constructor_transcript(bridge, constructor, seq![body]))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == bridge_constructor_transcript(bridge, constructor, seq![body])
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((body, read, write)) = bridge_bundle_parts(top, bridge.admission.arena) {
            let constructor = bridge_bundle_constructor(read, write);
            bridge_constructor_exact(bridge, constructor, seq![body]);
            assert(admission::admission_family_complete(top));
            if let Some((next, constructor_handle, score_handle)) = bridge_constructor_step(
                bridge, constructor, seq![body],
            ) {
                assert(seq![constructor_handle] + seq![body]
                    == seq![constructor_handle, body]);
                assert(next.admission.arena[score_handle as int].semantic);
            }
        }
    }
}

pub proof fn bridge_bundle_source_finish(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_bundle_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            &&& bridge_score_source_step(bridge, next)
            &&& next.admission.frames.len() == bridge.admission.frames.len()
            &&& next.admission.pushed == bridge.admission.pushed
            &&& next.admission.completed == bridge.admission.completed
            &&& next.admission.next_obligation_id
                == bridge.admission.next_obligation_id
            &&& next.admission.root_end == bridge.admission.root_end
            &&& constructor_handle as int == bridge.transcript.entries.len()
            &&& score_handle as int == bridge.transcript.entries.len() + 1
        },
    },
{
    if let Some((next, constructor_handle, score_handle)) = bridge_bundle_step(bridge) {
        let (body, read, write) = bridge_bundle_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ).unwrap();
        let constructor = bridge_bundle_constructor(read, write);
        bridge_constructor_exact(bridge, constructor, seq![body]);
        assert(bridge_same(next, next));
        assert(bridge_score_source_step(bridge, next));
    }
}

pub proof fn bridge_if_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_if_step(bridge) {
        None => true,
        Some((next, flag_handle, constructor_handle, score_handle)) => {
            let (condition, if_true, if_false, connective_used) = bridge_if_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            let flag = if connective_used { 1i64 } else { 0i64 };
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& flag_handle as int == bridge.transcript.entries.len()
            &&& score::handle_contents(next.transcript, flag_handle)
                == Some(score::ScoreNode::Integer(flag))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(309i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![
                    constructor_handle, condition, if_true, if_false, flag_handle,
                ]))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == next.transcript.entries[score_handle as int].transcript
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((condition, if_true, if_false, connective_used)) =
            bridge_if_parts(top, bridge.admission.arena)
        {
            let flag = if connective_used { 1i64 } else { 0i64 };
            let flag_kind = score::ScoreNode::Integer(flag);
            bridge_append_unattached_exact(bridge, flag_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, flag_kind);
            if let Some((after_flag, flag_handle)) = bridge_append_unattached(
                bridge, flag_kind,
            ) {
                let children = seq![condition, if_true, if_false, flag_handle];
                bridge_constructor_exact(after_flag, 309i32, children);
                assert(admission::admission_family_complete(top));
                if let Some((next, constructor_handle, score_handle)) =
                    bridge_constructor_step(after_flag, 309i32, children)
                {
                    assert(flag_handle as int == bridge.transcript.entries.len());
                    bridge_score_handle_preserved(
                        after_flag.transcript, next.transcript, flag_handle,
                    );
                    assert(seq![constructor_handle] + children == seq![
                        constructor_handle, condition, if_true, if_false, flag_handle,
                    ]);
                    assert(after_flag.admission.frames == bridge.admission.frames);
                    assert(next.admission.arena[score_handle as int].semantic);
                }
            }
        }
    }
}

pub proof fn bridge_send_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_send_step(bridge) {
        None => true,
        Some((next, persistent_handle, connective_handle,
            constructor_handle, score_handle)) => {
            let (persistent, channel, data, connective_used, cost) = bridge_send_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            let children = seq![persistent_handle, channel]
                + data + seq![connective_handle] + cost;
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, persistent_handle)
                == Some(score::ScoreNode::Integer(if persistent { 1i64 } else { 0i64 }))
            &&& score::handle_contents(next.transcript, connective_handle)
                == Some(score::ScoreNode::Integer(if connective_used { 1i64 } else { 0i64 }))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(300i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == next.transcript.entries[score_handle as int].transcript
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((persistent, channel, data, connective_used, cost)) =
            bridge_send_parts(top, bridge.admission.arena)
        {
            let persistent_kind = score::ScoreNode::Integer(
                if persistent { 1i64 } else { 0i64 },
            );
            let connective_kind = score::ScoreNode::Integer(
                if connective_used { 1i64 } else { 0i64 },
            );
            bridge_append_unattached_exact(bridge, persistent_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, persistent_kind);
            if let Some((after_persistent, persistent_handle)) =
                bridge_append_unattached(bridge, persistent_kind)
            {
                bridge_append_unattached_exact(after_persistent, connective_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_persistent, connective_kind,
                );
                if let Some((after_connective, connective_handle)) =
                    bridge_append_unattached(after_persistent, connective_kind)
                {
                    let children = seq![persistent_handle, channel]
                        + data + seq![connective_handle] + cost;
                    bridge_constructor_exact(after_connective, 300i32, children);
                    if let Some((next, _, score_handle)) = bridge_constructor_step(
                        after_connective, 300i32, children,
                    ) {
                        bridge_score_prefix_transitive(
                            after_persistent.transcript.entries,
                            after_connective.transcript.entries,
                            next.transcript.entries,
                        );
                        bridge_score_handle_preserved(
                            after_persistent.transcript, next.transcript,
                            persistent_handle,
                        );
                        bridge_score_handle_preserved(
                            after_connective.transcript, next.transcript,
                            connective_handle,
                        );
                        assert(admission::admission_family_complete(top));
                        assert(next.admission.arena[score_handle as int].semantic);
                    }
                }
            }
        }
    }
}

pub proof fn bridge_match_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_match_step(bridge) {
        None => true,
        Some((next, flag_handle, constructor_handle, score_handle)) => {
            let (target, cases, connective_used) = bridge_match_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            let children = seq![target] + cases + seq![flag_handle];
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, flag_handle)
                == Some(score::ScoreNode::Integer(
                    if connective_used { 1i64 } else { 0i64 },
                ))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(304i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == next.transcript.entries[score_handle as int].transcript
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((target, cases, connective_used)) =
            bridge_match_parts(top, bridge.admission.arena)
        {
            let flag_kind = score::ScoreNode::Integer(
                if connective_used { 1i64 } else { 0i64 },
            );
            bridge_append_unattached_exact(bridge, flag_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, flag_kind);
            if let Some((after_flag, flag_handle)) = bridge_append_unattached(
                bridge, flag_kind,
            ) {
                let children = seq![target] + cases + seq![flag_handle];
                bridge_constructor_exact(after_flag, 304i32, children);
                if let Some((next, _, score_handle)) = bridge_constructor_step(
                    after_flag, 304i32, children,
                ) {
                    bridge_score_handle_preserved(
                        after_flag.transcript, next.transcript, flag_handle,
                    );
                    assert(admission::admission_family_complete(top));
                    assert(next.admission.arena[score_handle as int].semantic);
                }
            }
        }
    }
}

pub proof fn bridge_absent_specialized_constructor_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_absent_specialized_constructor_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle]))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == bridge_constructor_transcript(bridge, 0i32, Seq::empty())
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if ((top.family == admission::AdmissionFamily::Expr
            && top.program_type_id == 24)
            || (top.family == admission::AdmissionFamily::GUnforgeable
                && top.program_type_id == 60))
            && !top.seen.contains(0)
            && !top.scalar_slots.contains(0)
            && top.scalar_occurrences.len() == 0
            && top.returns.len() == 0
        {
            bridge_constructor_exact(bridge, 0i32, Seq::empty());
            assert(admission::admission_family_complete(top));
        }
    }
}

pub proof fn bridge_absent_connective_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_absent_connective_step(bridge) {
        None => true,
        Some((next, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == score::integer_transcript(0i64)
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if top.family == admission::AdmissionFamily::Connective
            && top.program_type_id == 55
            && !top.seen.contains(0)
            && !top.scalar_slots.contains(0)
            && top.scalar_occurrences.len() == 0
            && top.returns.len() == 0
        {
            let kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_exact(bridge, kind);
            bridge_append_unattached_preserves_admission_wf(bridge, kind);
            if let Some((after_integer, score_handle)) = bridge_append_unattached(
                bridge, kind,
            ) {
                admission::admission_attach_score_preserves_wf(
                    after_integer.admission, score_handle, top.body_id,
                );
                if let Some(admission) = admission::admission_attach_score_step(
                    after_integer.admission, score_handle, top.body_id,
                ) {
                    assert(admission.frames.last().score_handle == Some(score_handle));
                    assert(admission.arena == after_integer.admission.arena);
                }
            }
        }
    }
}

pub proof fn bridge_receive_bind_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_receive_bind_step(bridge) {
        None => true,
        Some((next, default_handle, score_handle)) => {
            let (source, patterns, remainder, cost) = bridge_receive_bind_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            let selected_remainder = match remainder {
                Some(handle) => handle,
                None => default_handle,
            };
            let children = seq![source] + patterns + seq![selected_remainder] + cost;
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& default_handle as int == bridge.transcript.entries.len()
            &&& score::handle_contents(next.transcript, default_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(children))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == next.transcript.entries[score_handle as int].transcript
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((source, patterns, remainder, cost)) = bridge_receive_bind_parts(
            top, bridge.admission.arena,
        ) {
            let default_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_exact(bridge, default_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, default_kind);
            if let Some((after_default, default_handle)) = bridge_append_unattached(
                bridge, default_kind,
            ) {
                let selected_remainder = match remainder {
                    Some(handle) => handle,
                    None => default_handle,
                };
                let children = seq![source] + patterns
                    + seq![selected_remainder] + cost;
                let node_kind = score::ScoreNode::Node(children);
                bridge_append_unattached_exact(after_default, node_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_default, node_kind,
                );
                if let Some((after_node, score_handle)) = bridge_append_unattached(
                    after_default, node_kind,
                ) {
                    admission::admission_attach_score_preserves_wf(
                        after_node.admission, score_handle, top.body_id,
                    );
                    if let Some(admission) = admission::admission_attach_score_step(
                        after_node.admission, score_handle, top.body_id,
                    ) {
                        assert(admission.arena == after_node.admission.arena);
                        assert(admission.frames.last().score_handle == Some(score_handle));
                        assert(score::handle_contents(after_node.transcript, default_handle)
                            == Some(default_kind));
                        assert(admission::admission_family_complete(top));
                    }
                }
            }
        }
    }
}

pub proof fn bridge_match_case_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_match_case_step(bridge) {
        None => true,
        Some((next, flag_handle, constructor_handle, empty_handle,
            count_handle, score_handle)) => {
            let (pattern, source, free_count, guard) = bridge_match_case_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            let selected_guard = match guard {
                Some(handle) => handle,
                None => empty_handle,
            };
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& flag_handle as int == bridge.transcript.entries.len()
            &&& score::handle_contents(next.transcript, flag_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(999i64))
            &&& score::handle_contents(next.transcript, empty_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle, flag_handle]))
            &&& score::handle_contents(next.transcript, count_handle)
                == Some(score::ScoreNode::Integer(free_count))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![
                    pattern, source, count_handle, selected_guard,
                ]))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == next.transcript.entries[score_handle as int].transcript
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((pattern, source, free_count, guard)) = bridge_match_case_parts(
            top, bridge.admission.arena,
        ) {
            let flag_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_exact(bridge, flag_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, flag_kind);
            if let Some((after_flag, flag_handle)) = bridge_append_unattached(
                bridge, flag_kind,
            ) {
                let constructor_kind = score::ScoreNode::Integer(999i64);
                bridge_append_unattached_exact(after_flag, constructor_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_flag, constructor_kind,
                );
                if let Some((after_constructor, constructor_handle)) =
                    bridge_append_unattached(after_flag, constructor_kind)
                {
                    let empty_kind = score::ScoreNode::Node(
                        seq![constructor_handle, flag_handle],
                    );
                    bridge_append_unattached_exact(after_constructor, empty_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_constructor, empty_kind,
                    );
                    if let Some((after_empty, empty_handle)) =
                        bridge_append_unattached(after_constructor, empty_kind)
                    {
                        let selected_guard = match guard {
                            Some(handle) => handle,
                            None => empty_handle,
                        };
                        let count_kind = score::ScoreNode::Integer(free_count);
                        bridge_append_unattached_exact(after_empty, count_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_empty, count_kind,
                        );
                        if let Some((after_count, count_handle)) =
                            bridge_append_unattached(after_empty, count_kind)
                        {
                            let node_kind = score::ScoreNode::Node(
                                seq![pattern, source, count_handle, selected_guard],
                            );
                            bridge_append_unattached_exact(after_count, node_kind);
                            bridge_append_unattached_preserves_admission_wf(
                                after_count, node_kind,
                            );
                            if let Some((after_node, score_handle)) =
                                bridge_append_unattached(after_count, node_kind)
                            {
                                admission::admission_attach_score_preserves_wf(
                                    after_node.admission, score_handle, top.body_id,
                                );
                                if let Some(admission) = admission::admission_attach_score_step(
                                    after_node.admission, score_handle, top.body_id,
                                ) {
                                    assert(admission.arena == after_node.admission.arena);
                                    assert(admission.frames.last().score_handle == Some(score_handle));
                                    assert(admission::admission_family_complete(top));
                                    bridge_score_prefix_transitive(
                                        after_flag.transcript.entries,
                                        after_constructor.transcript.entries,
                                        after_empty.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_flag.transcript.entries,
                                        after_empty.transcript.entries,
                                        after_count.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_flag.transcript.entries,
                                        after_count.transcript.entries,
                                        after_node.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_constructor.transcript.entries,
                                        after_empty.transcript.entries,
                                        after_count.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_constructor.transcript.entries,
                                        after_count.transcript.entries,
                                        after_node.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_empty.transcript.entries,
                                        after_count.transcript.entries,
                                        after_node.transcript.entries,
                                    );
                                    bridge_score_handle_preserved(
                                        after_flag.transcript, after_node.transcript, flag_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_constructor.transcript, after_node.transcript,
                                        constructor_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_empty.transcript, after_node.transcript,
                                        empty_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_count.transcript, after_node.transcript,
                                        count_handle,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_bool_oneof_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_bool_oneof_step(bridge) {
        None => true,
        Some((next, value_handle, constructor_handle, score_handle)) => {
            let (constructor, value) = bridge_bool_oneof_parts(
                bridge.admission.frames.last(),
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, value_handle)
                == Some(score::ScoreNode::Integer(value))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(constructor as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle, value_handle]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((constructor, value)) = bridge_bool_oneof_parts(top) {
            let value_kind = score::ScoreNode::Integer(value);
            bridge_append_unattached_exact(bridge, value_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, value_kind);
            if let Some((after_value, value_handle)) = bridge_append_unattached(
                bridge, value_kind,
            ) {
                bridge_constructor_exact(after_value, constructor, seq![value_handle]);
                if let Some((next, constructor_handle, score_handle)) = bridge_constructor_step(
                    after_value, constructor, seq![value_handle],
                ) {
                    bridge_score_handle_preserved(
                        after_value.transcript, next.transcript, value_handle,
                    );
                    assert(admission::admission_family_complete(top));
                    assert(next.admission.arena[score_handle as int].semantic);
                    assert(seq![constructor_handle] + seq![value_handle]
                        == seq![constructor_handle, value_handle]);
                    assert(score::handle_contents(next.transcript, value_handle)
                        == Some(value_kind));
                    assert(score::handle_contents(next.transcript, constructor_handle)
                        == Some(score::ScoreNode::Integer(constructor as i64)));
                    assert(score::handle_contents(next.transcript, score_handle)
                        == Some(score::ScoreNode::Node(
                            seq![constructor_handle, value_handle],
                        )));
                }
            }
        }
    }
}

pub proof fn bridge_expr_atom_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_atom_step(bridge) {
        None => true,
        Some((next, atom_handle, constructor_handle, score_handle)) => {
            let (constructor, atom_kind) = bridge_expr_atom_parts(
                bridge.admission.frames.last(),
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, atom_handle)
                == Some(atom_kind)
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(constructor as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle, atom_handle]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((constructor, atom_kind)) = bridge_expr_atom_parts(top) {
            bridge_append_unattached_exact(bridge, atom_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, atom_kind);
            if let Some((after_atom, atom_handle)) = bridge_append_unattached(
                bridge, atom_kind,
            ) {
                bridge_constructor_exact(after_atom, constructor, seq![atom_handle]);
                if let Some((next, constructor_handle, score_handle)) =
                    bridge_constructor_step(after_atom, constructor, seq![atom_handle])
                {
                    bridge_score_handle_preserved(
                        after_atom.transcript, next.transcript, atom_handle,
                    );
                    assert(seq![constructor_handle] + seq![atom_handle]
                        == seq![constructor_handle, atom_handle]);
                    assert(admission::admission_family_complete(top));
                    assert(next.admission.arena[score_handle as int].semantic);
                }
            }
        }
    }
}

pub proof fn bridge_expr_numeric_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_numeric_step(bridge) {
        None => true,
        Some((next, value_handle, constructor_handle, score_handle)) => {
            let (constructor, value) = bridge_expr_numeric_parts(
                bridge.admission.frames.last(),
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, value_handle)
                == Some(score::ScoreNode::Integer(value))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(constructor as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle, value_handle]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((constructor, value)) = bridge_expr_numeric_parts(top) {
            let value_kind = score::ScoreNode::Integer(value);
            bridge_append_unattached_exact(bridge, value_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, value_kind);
            if let Some((after_value, value_handle)) = bridge_append_unattached(
                bridge, value_kind,
            ) {
                bridge_constructor_exact(after_value, constructor, seq![value_handle]);
                if let Some((next, constructor_handle, score_handle)) =
                    bridge_constructor_step(after_value, constructor, seq![value_handle])
                {
                    bridge_score_handle_preserved(
                        after_value.transcript, next.transcript, value_handle,
                    );
                    assert(seq![constructor_handle] + seq![value_handle]
                        == seq![constructor_handle, value_handle]);
                    assert(admission::admission_family_complete(top));
                    assert(next.admission.arena[score_handle as int].semantic);
                }
            }
        }
    }
}

pub proof fn bridge_expr_big_rational_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_big_rational_step(bridge) {
        None => true,
        Some((next, numerator_handle, denominator_handle,
            constructor_handle, score_handle)) => {
            let (numerator, denominator) = bridge_expr_big_rational_parts(
                bridge.admission.frames.last(),
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, numerator_handle)
                == Some(score::ScoreNode::BytesAtom(numerator))
            &&& score::handle_contents(next.transcript, denominator_handle)
                == Some(score::ScoreNode::BytesAtom(denominator))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(16i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![
                    constructor_handle, numerator_handle, denominator_handle,
                ]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((numerator, denominator)) = bridge_expr_big_rational_parts(top) {
            let numerator_kind = score::ScoreNode::BytesAtom(numerator);
            bridge_append_unattached_exact(bridge, numerator_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, numerator_kind);
            if let Some((after_numerator, numerator_handle)) = bridge_append_unattached(
                bridge, numerator_kind,
            ) {
                let denominator_kind = score::ScoreNode::BytesAtom(denominator);
                bridge_append_unattached_exact(after_numerator, denominator_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_numerator, denominator_kind,
                );
                if let Some((after_denominator, denominator_handle)) =
                    bridge_append_unattached(after_numerator, denominator_kind)
                {
                    bridge_constructor_exact(
                        after_denominator, 16i32,
                        seq![numerator_handle, denominator_handle],
                    );
                    if let Some((next, constructor_handle, score_handle)) =
                        bridge_constructor_step(
                            after_denominator, 16i32,
                            seq![numerator_handle, denominator_handle],
                        )
                    {
                        bridge_score_prefix_transitive(
                            after_numerator.transcript.entries,
                            after_denominator.transcript.entries,
                            next.transcript.entries,
                        );
                        bridge_score_handle_preserved(
                            after_numerator.transcript, next.transcript, numerator_handle,
                        );
                        bridge_score_handle_preserved(
                            after_denominator.transcript, next.transcript,
                            denominator_handle,
                        );
                        assert(seq![constructor_handle]
                            + seq![numerator_handle, denominator_handle]
                            == seq![constructor_handle, numerator_handle,
                                denominator_handle]);
                        assert(admission::admission_family_complete(top));
                        assert(next.admission.arena[score_handle as int].semantic);
                    }
                }
            }
        }
    }
}

pub proof fn bridge_expr_fixed_point_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_fixed_point_step(bridge) {
        None => true,
        Some((next, unscaled_handle, scale_handle, scale_node_handle,
            constructor_handle, score_handle)) => {
            let (unscaled, scale) = bridge_expr_fixed_point_parts(
                bridge.admission.frames.last(),
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, unscaled_handle)
                == Some(score::ScoreNode::BytesAtom(unscaled))
            &&& score::handle_contents(next.transcript, scale_handle)
                == Some(score::ScoreNode::Integer(scale as i64))
            &&& score::handle_contents(next.transcript, scale_node_handle)
                == Some(score::ScoreNode::Node(seq![scale_handle]))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(17i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![
                    constructor_handle, unscaled_handle, scale_node_handle,
                ]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((unscaled, scale)) = bridge_expr_fixed_point_parts(top) {
            let unscaled_kind = score::ScoreNode::BytesAtom(unscaled);
            bridge_append_unattached_exact(bridge, unscaled_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, unscaled_kind);
            if let Some((after_unscaled, unscaled_handle)) = bridge_append_unattached(
                bridge, unscaled_kind,
            ) {
                let scale_kind = score::ScoreNode::Integer(scale as i64);
                bridge_append_unattached_exact(after_unscaled, scale_kind);
                bridge_append_unattached_preserves_admission_wf(after_unscaled, scale_kind);
                if let Some((after_scale, scale_handle)) = bridge_append_unattached(
                    after_unscaled, scale_kind,
                ) {
                    let scale_node_kind = score::ScoreNode::Node(seq![scale_handle]);
                    bridge_append_unattached_exact(after_scale, scale_node_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_scale, scale_node_kind,
                    );
                    if let Some((after_scale_node, scale_node_handle)) =
                        bridge_append_unattached(after_scale, scale_node_kind)
                    {
                        bridge_constructor_exact(
                            after_scale_node, 17i32,
                            seq![unscaled_handle, scale_node_handle],
                        );
                        if let Some((next, constructor_handle, score_handle)) =
                            bridge_constructor_step(
                                after_scale_node, 17i32,
                                seq![unscaled_handle, scale_node_handle],
                            )
                        {
                            bridge_score_prefix_transitive(
                                after_unscaled.transcript.entries,
                                after_scale.transcript.entries,
                                after_scale_node.transcript.entries,
                            );
                            bridge_score_prefix_transitive(
                                after_unscaled.transcript.entries,
                                after_scale_node.transcript.entries,
                                next.transcript.entries,
                            );
                            bridge_score_prefix_transitive(
                                after_scale.transcript.entries,
                                after_scale_node.transcript.entries,
                                next.transcript.entries,
                            );
                            bridge_score_handle_preserved(
                                after_unscaled.transcript, next.transcript, unscaled_handle,
                            );
                            bridge_score_handle_preserved(
                                after_scale.transcript, next.transcript, scale_handle,
                            );
                            bridge_score_handle_preserved(
                                after_scale_node.transcript, next.transcript,
                                scale_node_handle,
                            );
                            assert(seq![constructor_handle]
                                + seq![unscaled_handle, scale_node_handle]
                                == seq![constructor_handle, unscaled_handle,
                                    scale_node_handle]);
                            assert(admission::admission_family_complete(top));
                            assert(next.admission.arena[score_handle as int].semantic);
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_sys_auth_token_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_sys_auth_token_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(12i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if top.family == admission::AdmissionFamily::GUnforgeable
            && top.program_type_id == 60
            && top.returns.len() == 1
            && top.returns[0].slot == 0
            && top.returns[0].tag == 4
            && top.returns[0].kind == admission::AdmissionReturnedKind::Unit
            && top.returns[0].score_handle.is_none()
            && top.scalar_occurrences.len() == 0
            && admission::admission_family_complete(top)
        {
            bridge_constructor_exact(bridge, 12i32, Seq::empty());
        }
    }
}

pub proof fn bridge_new_empty_collections_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_new_empty_collections_step(bridge) {
        None => true,
        Some((next, count_handle, uri_handle, injection_handle,
            constructor_handle, score_handle)) => {
            let (bind_count, body) = bridge_new_empty_collections_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, count_handle)
                == Some(score::ScoreNode::Integer(bind_count))
            &&& score::handle_contents(next.transcript, uri_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& score::handle_contents(next.transcript, injection_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(303i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![
                    constructor_handle, count_handle, uri_handle, injection_handle, body,
                ]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((bind_count, body)) = bridge_new_empty_collections_parts(
            top, bridge.admission.arena,
        ) {
            let absent_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_exact(bridge, absent_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, absent_kind);
            if let Some((after_uri, uri_handle)) = bridge_append_unattached(
                bridge, absent_kind,
            ) {
                bridge_append_unattached_exact(after_uri, absent_kind);
                bridge_append_unattached_preserves_admission_wf(after_uri, absent_kind);
                if let Some((after_injection, injection_handle)) = bridge_append_unattached(
                    after_uri, absent_kind,
                ) {
                    let count_kind = score::ScoreNode::Integer(bind_count);
                    bridge_append_unattached_exact(after_injection, count_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_injection, count_kind,
                    );
                    if let Some((after_count, count_handle)) =
                        bridge_append_unattached(after_injection, count_kind)
                    {
                        let children = seq![count_handle, uri_handle, injection_handle, body];
                        bridge_constructor_exact(after_count, 303i32, children);
                        if let Some((next, constructor_handle, score_handle)) =
                            bridge_constructor_step(after_count, 303i32, children)
                        {
                            bridge_score_prefix_transitive(
                                after_injection.transcript.entries,
                                after_count.transcript.entries,
                                next.transcript.entries,
                            );
                            bridge_score_prefix_transitive(
                                after_uri.transcript.entries,
                                after_injection.transcript.entries,
                                after_count.transcript.entries,
                            );
                            bridge_score_prefix_transitive(
                                after_uri.transcript.entries,
                                after_count.transcript.entries,
                                next.transcript.entries,
                            );
                            bridge_score_handle_preserved(
                                after_uri.transcript, next.transcript, uri_handle,
                            );
                            bridge_score_handle_preserved(
                                after_injection.transcript, next.transcript,
                                injection_handle,
                            );
                            bridge_score_handle_preserved(
                                after_count.transcript, next.transcript, count_handle,
                            );
                            assert(seq![constructor_handle] + children == seq![
                                constructor_handle, count_handle, uri_handle,
                                injection_handle, body,
                            ]);
                            assert(admission::admission_family_complete(top));
                            assert(next.admission.arena[score_handle as int].semantic);
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_new_one_injection_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_new_one_injection_step(bridge) {
        None => true,
        Some((next, count_handle, uri_handle, key_handle, injection_handle,
            constructor_handle, score_handle)) => {
            let (bind_count, body, key_bytes, value_handle) =
                bridge_new_one_injection_parts(
                    bridge.admission.frames.last(), bridge.admission.arena,
                ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, uri_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& score::handle_contents(next.transcript, key_handle)
                == Some(score::ScoreNode::StringAtom(key_bytes))
            &&& score::handle_contents(next.transcript, injection_handle)
                == Some(score::ScoreNode::Node(seq![key_handle, value_handle]))
            &&& score::handle_contents(next.transcript, count_handle)
                == Some(score::ScoreNode::Integer(bind_count))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(303i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![
                    constructor_handle, count_handle, uri_handle, injection_handle, body,
                ]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((bind_count, body, key_bytes, value_handle)) =
            bridge_new_one_injection_parts(top, bridge.admission.arena)
        {
            let uri_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_exact(bridge, uri_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, uri_kind);
            if let Some((after_uri, uri_handle)) = bridge_append_unattached(
                bridge, uri_kind,
            ) {
                let key_kind = score::ScoreNode::StringAtom(key_bytes);
                bridge_append_unattached_exact(after_uri, key_kind);
                bridge_append_unattached_preserves_admission_wf(after_uri, key_kind);
                if let Some((after_key, key_handle)) = bridge_append_unattached(
                    after_uri, key_kind,
                ) {
                    let injection_kind = score::ScoreNode::Node(
                        seq![key_handle, value_handle],
                    );
                    bridge_append_unattached_exact(after_key, injection_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_key, injection_kind,
                    );
                    if let Some((after_injection, injection_handle)) =
                        bridge_append_unattached(after_key, injection_kind)
                    {
                        let count_kind = score::ScoreNode::Integer(bind_count);
                        bridge_append_unattached_exact(after_injection, count_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_injection, count_kind,
                        );
                        if let Some((after_count, count_handle)) =
                            bridge_append_unattached(after_injection, count_kind)
                        {
                            let children = seq![count_handle, uri_handle, injection_handle, body];
                            bridge_constructor_exact(after_count, 303i32, children);
                            if let Some((next, constructor_handle, score_handle)) =
                                bridge_constructor_step(after_count, 303i32, children)
                            {
                                bridge_score_prefix_transitive(
                                    after_uri.transcript.entries,
                                    after_key.transcript.entries,
                                    after_injection.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_uri.transcript.entries,
                                    after_injection.transcript.entries,
                                    after_count.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_uri.transcript.entries,
                                    after_count.transcript.entries,
                                    next.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_key.transcript.entries,
                                    after_injection.transcript.entries,
                                    after_count.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_key.transcript.entries,
                                    after_count.transcript.entries,
                                    next.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_injection.transcript.entries,
                                    after_count.transcript.entries,
                                    next.transcript.entries,
                                );
                                bridge_score_handle_preserved(
                                    after_uri.transcript, next.transcript, uri_handle,
                                );
                                bridge_score_handle_preserved(
                                    after_key.transcript, next.transcript, key_handle,
                                );
                                bridge_score_handle_preserved(
                                    after_injection.transcript, next.transcript,
                                    injection_handle,
                                );
                                bridge_score_handle_preserved(
                                    after_count.transcript, next.transcript, count_handle,
                                );
                                assert(seq![constructor_handle] + children == seq![
                                    constructor_handle, count_handle, uri_handle,
                                    injection_handle, body,
                                ]);
                                assert(admission::admission_family_complete(top));
                                assert(next.admission.arena[score_handle as int].semantic);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_new_one_absent_injection_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_new_one_absent_injection_step(bridge) {
        None => true,
        Some((next, uri_handle, empty_handle, key_handle, injection_handle,
            count_handle, constructor_handle, score_handle)) => {
            let (bind_count, body, key_bytes) = bridge_new_one_absent_injection_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, uri_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& score::handle_contents(next.transcript, key_handle)
                == Some(score::ScoreNode::StringAtom(key_bytes))
            &&& score::handle_contents(next.transcript, injection_handle)
                == Some(score::ScoreNode::Node(seq![key_handle, empty_handle]))
            &&& score::handle_contents(next.transcript, count_handle)
                == Some(score::ScoreNode::Integer(bind_count))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(303i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![
                    constructor_handle, count_handle, uri_handle, injection_handle, body,
                ]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((bind_count, body, key_bytes)) =
            bridge_new_one_absent_injection_parts(top, bridge.admission.arena)
        {
            let uri_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_exact(bridge, uri_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, uri_kind);
            if let Some((after_uri, uri_handle)) = bridge_append_unattached(
                bridge, uri_kind,
            ) {
                bridge_empty_par_unattached_exact(after_uri);
                if let Some((after_empty, _, _, empty_handle)) =
                    bridge_empty_par_unattached_step(after_uri)
                {
                    let key_kind = score::ScoreNode::StringAtom(key_bytes);
                    bridge_append_unattached_exact(after_empty, key_kind);
                    bridge_append_unattached_preserves_admission_wf(after_empty, key_kind);
                    if let Some((after_key, key_handle)) = bridge_append_unattached(
                        after_empty, key_kind,
                    ) {
                        let injection_kind = score::ScoreNode::Node(
                            seq![key_handle, empty_handle],
                        );
                        bridge_append_unattached_exact(after_key, injection_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_key, injection_kind,
                        );
                        if let Some((after_injection, injection_handle)) =
                            bridge_append_unattached(after_key, injection_kind)
                        {
                            let count_kind = score::ScoreNode::Integer(bind_count);
                            bridge_append_unattached_exact(after_injection, count_kind);
                            bridge_append_unattached_preserves_admission_wf(
                                after_injection, count_kind,
                            );
                            if let Some((after_count, count_handle)) =
                                bridge_append_unattached(after_injection, count_kind)
                            {
                                let children = seq![count_handle, uri_handle,
                                    injection_handle, body];
                                bridge_constructor_exact(after_count, 303i32, children);
                                if let Some((next, constructor_handle, score_handle)) =
                                    bridge_constructor_step(after_count, 303i32, children)
                                {
                                    bridge_score_prefix_transitive(
                                        after_uri.transcript.entries,
                                        after_empty.transcript.entries,
                                        after_key.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_uri.transcript.entries,
                                        after_key.transcript.entries,
                                        after_injection.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_uri.transcript.entries,
                                        after_injection.transcript.entries,
                                        after_count.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_uri.transcript.entries,
                                        after_count.transcript.entries,
                                        next.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_empty.transcript.entries,
                                        after_key.transcript.entries,
                                        after_injection.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_empty.transcript.entries,
                                        after_injection.transcript.entries,
                                        after_count.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_empty.transcript.entries,
                                        after_count.transcript.entries,
                                        next.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_key.transcript.entries,
                                        after_injection.transcript.entries,
                                        after_count.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_key.transcript.entries,
                                        after_count.transcript.entries,
                                        next.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_injection.transcript.entries,
                                        after_count.transcript.entries,
                                        next.transcript.entries,
                                    );
                                    bridge_score_handle_preserved(
                                        after_uri.transcript, next.transcript, uri_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_key.transcript, next.transcript, key_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_injection.transcript, next.transcript,
                                        injection_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_count.transcript, next.transcript, count_handle,
                                    );
                                    assert(seq![constructor_handle] + children == seq![
                                        constructor_handle, count_handle, uri_handle,
                                        injection_handle, body,
                                    ]);
                                    assert(admission::admission_family_complete(top));
                                    assert(next.admission.arena[score_handle as int].semantic);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_new_one_uri_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_new_one_uri_step(bridge) {
        None => true,
        Some((next, count_handle, uri_handle, injection_handle,
            constructor_handle, score_handle)) => {
            let (bind_count, body, uri_bytes) = bridge_new_one_uri_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, uri_handle)
                == Some(score::ScoreNode::StringAtom(uri_bytes))
            &&& score::handle_contents(next.transcript, injection_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& score::handle_contents(next.transcript, count_handle)
                == Some(score::ScoreNode::Integer(bind_count))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(303i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![
                    constructor_handle, count_handle, uri_handle, injection_handle, body,
                ]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((bind_count, body, uri_bytes)) = bridge_new_one_uri_parts(
            top, bridge.admission.arena,
        ) {
            let uri_kind = score::ScoreNode::StringAtom(uri_bytes);
            bridge_append_unattached_exact(bridge, uri_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, uri_kind);
            if let Some((after_uri, uri_handle)) = bridge_append_unattached(
                bridge, uri_kind,
            ) {
                let absent_kind = score::ScoreNode::Integer(0i64);
                bridge_append_unattached_exact(after_uri, absent_kind);
                bridge_append_unattached_preserves_admission_wf(after_uri, absent_kind);
                if let Some((after_injection, injection_handle)) = bridge_append_unattached(
                    after_uri, absent_kind,
                ) {
                    let count_kind = score::ScoreNode::Integer(bind_count);
                    bridge_append_unattached_exact(after_injection, count_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_injection, count_kind,
                    );
                    if let Some((after_count, count_handle)) =
                        bridge_append_unattached(after_injection, count_kind)
                    {
                        let children = seq![count_handle, uri_handle, injection_handle, body];
                        bridge_constructor_exact(after_count, 303i32, children);
                        if let Some((next, constructor_handle, score_handle)) =
                            bridge_constructor_step(after_count, 303i32, children)
                        {
                            bridge_score_prefix_transitive(
                                after_injection.transcript.entries,
                                after_count.transcript.entries,
                                next.transcript.entries,
                            );
                            bridge_score_prefix_transitive(
                                after_uri.transcript.entries,
                                after_injection.transcript.entries,
                                after_count.transcript.entries,
                            );
                            bridge_score_prefix_transitive(
                                after_uri.transcript.entries,
                                after_count.transcript.entries,
                                next.transcript.entries,
                            );
                            bridge_score_handle_preserved(
                                after_uri.transcript, next.transcript, uri_handle,
                            );
                            bridge_score_handle_preserved(
                                after_injection.transcript, next.transcript,
                                injection_handle,
                            );
                            bridge_score_handle_preserved(
                                after_count.transcript, next.transcript, count_handle,
                            );
                            assert(seq![constructor_handle] + children == seq![
                                constructor_handle, count_handle, uri_handle,
                                injection_handle, body,
                            ]);
                            assert(admission::admission_family_complete(top));
                            assert(next.admission.arena[score_handle as int].semantic);
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_expr_payload_forward_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_payload_forward_step(bridge) {
        None => true,
        Some((next, handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.transcript == bridge.transcript
            &&& next.admission.arena == bridge.admission.arena
            &&& next.admission.frames.last().score_handle == Some(handle)
            &&& next.admission.arena[handle as int].semantic
            &&& next.admission.arena[handle as int].transcript_bytes
                == bridge.transcript.entries[handle as int].transcript
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if top.family == admission::AdmissionFamily::Expr
            && top.program_type_id == 24
            && top.returns.len() == 1
            && top.returns[0].slot == 0
            && admission::admission_field_tag_valid(
                admission::AdmissionFamily::Expr, 0, top.returns[0].tag, true, false,
            )
            && top.scalar_occurrences.len() == 0
            && top.seen.contains(0)
            && admission::admission_family_complete(top)
            && admission::admission_expr_fixed(top)
        {
            let returned = top.returns[0];
            if let (
                admission::AdmissionReturnedKind::ExprPayload {
                    score_present: true, canonical_fixed_point: true, ..
                },
                Some(handle),
            ) = (returned.kind, returned.score_handle) {
                if (handle as int) < bridge.admission.arena.len()
                    && returned.score_origin_body_id
                        == Some(bridge.admission.arena[handle as int].body_id)
                    && bridge.admission.arena[handle as int].semantic
                {
                    admission::admission_attach_score_preserves_wf(
                        bridge.admission, handle, top.body_id,
                    );
                    if let Some(admission) = admission::admission_attach_score_step(
                        bridge.admission, handle, top.body_id,
                    ) {
                        assert(admission.arena == bridge.admission.arena);
                        assert(admission.frames.last().score_handle == Some(handle));
                    }
                }
            }
        }
    }
}

pub proof fn bridge_connective_not_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_connective_not_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            let child = bridge_par_return_handle(
                bridge.admission.frames.last().returns[0], bridge.admission.arena, 0, 3,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(400i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle, child]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if top.family == admission::AdmissionFamily::Connective
            && top.program_type_id == 55
            && top.returns.len() == 1
            && top.scalar_occurrences.len() == 0
            && admission::admission_family_complete(top)
        {
            if let Some(child) = bridge_par_return_handle(
                top.returns[0], bridge.admission.arena, 0, 3,
            ) {
                bridge_constructor_exact(bridge, 400i32, seq![child]);
                if let Some((next, constructor_handle, _)) =
                    bridge_constructor_step(bridge, 400i32, seq![child])
                {
                    assert(seq![constructor_handle] + seq![child]
                        == seq![constructor_handle, child]);
                    assert(next.admission.arena[next.admission.frames.last().score_handle.unwrap() as int].semantic);
                }
            }
        }
    }
}

pub proof fn bridge_connective_varref_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_connective_varref_step(bridge) {
        None => true,
        Some((next, index_handle, depth_handle, constructor_handle, score_handle)) => {
            let (index, depth) = bridge_connective_varref_parts(
                bridge.admission.frames.last(),
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, index_handle)
                == Some(score::ScoreNode::Integer(index))
            &&& score::handle_contents(next.transcript, depth_handle)
                == Some(score::ScoreNode::Integer(depth))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(403i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![
                    constructor_handle, index_handle, depth_handle,
                ]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((index, depth)) = bridge_connective_varref_parts(top) {
            let index_kind = score::ScoreNode::Integer(index);
            bridge_append_unattached_exact(bridge, index_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, index_kind);
            if let Some((after_index, index_handle)) = bridge_append_unattached(
                bridge, index_kind,
            ) {
                let depth_kind = score::ScoreNode::Integer(depth);
                bridge_append_unattached_exact(after_index, depth_kind);
                bridge_append_unattached_preserves_admission_wf(after_index, depth_kind);
                if let Some((after_depth, depth_handle)) = bridge_append_unattached(
                    after_index, depth_kind,
                ) {
                    bridge_constructor_exact(
                        after_depth, 403i32, seq![index_handle, depth_handle],
                    );
                    if let Some((next, constructor_handle, score_handle)) =
                        bridge_constructor_step(
                            after_depth, 403i32, seq![index_handle, depth_handle],
                        )
                    {
                        bridge_score_prefix_transitive(
                            after_index.transcript.entries,
                            after_depth.transcript.entries,
                            next.transcript.entries,
                        );
                        bridge_score_handle_preserved(
                            after_index.transcript, next.transcript, index_handle,
                        );
                        bridge_score_handle_preserved(
                            after_depth.transcript, next.transcript, depth_handle,
                        );
                        assert(seq![constructor_handle]
                            + seq![index_handle, depth_handle]
                            == seq![constructor_handle, index_handle, depth_handle]);
                        assert(admission::admission_family_complete(top));
                        assert(next.admission.arena[score_handle as int].semantic);
                    }
                }
            }
        }
    }
}

pub proof fn bridge_connective_collection_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_connective_collection_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            let (constructor, children) = bridge_connective_collection_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(constructor as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((constructor, children)) = bridge_connective_collection_parts(
            top, bridge.admission.arena,
        ) {
            bridge_constructor_exact(bridge, constructor, children);
            assert(admission::admission_family_complete(top));
        }
    }
}

pub proof fn bridge_unforgeable_bytes_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_unforgeable_bytes_step(bridge) {
        None => true,
        Some((next, bytes_handle, constructor_handle, score_handle)) => {
            let (constructor, value) = bridge_unforgeable_bytes_parts(
                bridge.admission.frames.last(),
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, bytes_handle)
                == Some(score::ScoreNode::BytesAtom(value))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(constructor as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle, bytes_handle]))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((constructor, value)) = bridge_unforgeable_bytes_parts(top) {
            let bytes_kind = score::ScoreNode::BytesAtom(value);
            bridge_append_unattached_exact(bridge, bytes_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, bytes_kind);
            if let Some((after_bytes, bytes_handle)) = bridge_append_unattached(
                bridge, bytes_kind,
            ) {
                bridge_constructor_exact(after_bytes, constructor, seq![bytes_handle]);
                if let Some((next, constructor_handle, score_handle)) =
                    bridge_constructor_step(after_bytes, constructor, seq![bytes_handle])
                {
                    bridge_score_handle_preserved(
                        after_bytes.transcript, next.transcript, bytes_handle,
                    );
                    assert(seq![constructor_handle] + seq![bytes_handle]
                        == seq![constructor_handle, bytes_handle]);
                    assert(admission::admission_family_complete(top));
                    assert(next.admission.arena[score_handle as int].semantic);
                }
            }
        }
    }
}

pub proof fn bridge_empty_par_unattached_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_empty_par_unattached_step(bridge) {
        None => true,
        Some((after_empty, flag_handle, constructor_handle, empty_handle)) => {
            &&& bridge_well_formed(after_empty)
            &&& admission::admission_state_wf(after_empty.admission)
            &&& after_empty.admission.frames == bridge.admission.frames
            &&& after_empty.transcript.entries.take(bridge.transcript.entries.len() as int)
                == bridge.transcript.entries
            &&& score::handle_contents(after_empty.transcript, flag_handle)
                == Some(score::ScoreNode::Integer(0i64))
            &&& score::handle_contents(after_empty.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(999i64))
            &&& score::handle_contents(after_empty.transcript, empty_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle, flag_handle]))
        },
    },
{
    let flag_kind = score::ScoreNode::Integer(0i64);
    bridge_append_unattached_exact(bridge, flag_kind);
    bridge_append_unattached_preserves_admission_wf(bridge, flag_kind);
    if let Some((after_flag, flag_handle)) = bridge_append_unattached(bridge, flag_kind) {
        let constructor_kind = score::ScoreNode::Integer(999i64);
        bridge_append_unattached_exact(after_flag, constructor_kind);
        bridge_append_unattached_preserves_admission_wf(after_flag, constructor_kind);
        if let Some((after_constructor, constructor_handle)) =
            bridge_append_unattached(after_flag, constructor_kind)
        {
            let empty_kind = score::ScoreNode::Node(seq![constructor_handle, flag_handle]);
            bridge_append_unattached_exact(after_constructor, empty_kind);
            bridge_append_unattached_preserves_admission_wf(after_constructor, empty_kind);
            if let Some((after_empty, empty_handle)) =
                bridge_append_unattached(after_constructor, empty_kind)
            {
                bridge_score_prefix_transitive(
                    bridge.transcript.entries,
                    after_flag.transcript.entries,
                    after_constructor.transcript.entries,
                );
                bridge_score_prefix_transitive(
                    bridge.transcript.entries,
                    after_constructor.transcript.entries,
                    after_empty.transcript.entries,
                );
                bridge_score_prefix_transitive(
                    after_flag.transcript.entries,
                    after_constructor.transcript.entries,
                    after_empty.transcript.entries,
                );
                bridge_score_handle_preserved(
                    after_flag.transcript, after_empty.transcript, flag_handle,
                );
                bridge_score_handle_preserved(
                    after_constructor.transcript, after_empty.transcript,
                    constructor_handle,
                );
            }
        }
    }
}

pub proof fn bridge_receive_present_condition_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_receive_present_condition_step(bridge) {
        None => true,
        Some((next, persistent_handle, peek_handle, count_handle,
            connective_handle, constructor_handle, score_handle)) => {
            let (persistent, peek, binds, body, bind_count,
                connective_used, condition, cost) =
                bridge_receive_present_condition_parts(
                    bridge.admission.frames.last(), bridge.admission.arena,
                ).unwrap();
            let children = seq![persistent_handle, peek_handle] + binds
                + seq![body, count_handle, connective_handle, condition] + cost;
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, persistent_handle)
                == Some(score::ScoreNode::Integer(if persistent { 1i64 } else { 0i64 }))
            &&& score::handle_contents(next.transcript, peek_handle)
                == Some(score::ScoreNode::Integer(if peek { 1i64 } else { 0i64 }))
            &&& score::handle_contents(next.transcript, count_handle)
                == Some(score::ScoreNode::Integer(bind_count))
            &&& score::handle_contents(next.transcript, connective_handle)
                == Some(score::ScoreNode::Integer(
                    if connective_used { 1i64 } else { 0i64 },
                ))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(301i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((persistent, peek, binds, body, bind_count,
            connective_used, condition, cost)) =
            bridge_receive_present_condition_parts(top, bridge.admission.arena)
        {
            let persistent_kind = score::ScoreNode::Integer(
                if persistent { 1i64 } else { 0i64 },
            );
            bridge_append_unattached_exact(bridge, persistent_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, persistent_kind);
            if let Some((after_persistent, persistent_handle)) =
                bridge_append_unattached(bridge, persistent_kind)
            {
                let peek_kind = score::ScoreNode::Integer(if peek { 1i64 } else { 0i64 });
                bridge_append_unattached_exact(after_persistent, peek_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_persistent, peek_kind,
                );
                if let Some((after_peek, peek_handle)) =
                    bridge_append_unattached(after_persistent, peek_kind)
                {
                    let count_kind = score::ScoreNode::Integer(bind_count);
                    bridge_append_unattached_exact(after_peek, count_kind);
                    bridge_append_unattached_preserves_admission_wf(after_peek, count_kind);
                    if let Some((after_count, count_handle)) =
                        bridge_append_unattached(after_peek, count_kind)
                    {
                        let connective_kind = score::ScoreNode::Integer(
                            if connective_used { 1i64 } else { 0i64 },
                        );
                        bridge_append_unattached_exact(after_count, connective_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_count, connective_kind,
                        );
                        if let Some((after_connective, connective_handle)) =
                            bridge_append_unattached(after_count, connective_kind)
                        {
                            let children = seq![persistent_handle, peek_handle] + binds
                                + seq![body, count_handle, connective_handle, condition] + cost;
                            bridge_constructor_exact(after_connective, 301i32, children);
                            if let Some((next, constructor_handle, score_handle)) =
                                bridge_constructor_step(after_connective, 301i32, children)
                            {
                                bridge_score_prefix_transitive(
                                    after_persistent.transcript.entries,
                                    after_peek.transcript.entries,
                                    after_count.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_persistent.transcript.entries,
                                    after_count.transcript.entries,
                                    after_connective.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_persistent.transcript.entries,
                                    after_connective.transcript.entries,
                                    next.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_peek.transcript.entries,
                                    after_count.transcript.entries,
                                    after_connective.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_peek.transcript.entries,
                                    after_connective.transcript.entries,
                                    next.transcript.entries,
                                );
                                bridge_score_prefix_transitive(
                                    after_count.transcript.entries,
                                    after_connective.transcript.entries,
                                    next.transcript.entries,
                                );
                                bridge_score_handle_preserved(
                                    after_persistent.transcript, next.transcript,
                                    persistent_handle,
                                );
                                bridge_score_handle_preserved(
                                    after_peek.transcript, next.transcript, peek_handle,
                                );
                                bridge_score_handle_preserved(
                                    after_count.transcript, next.transcript, count_handle,
                                );
                                bridge_score_handle_preserved(
                                    after_connective.transcript, next.transcript,
                                    connective_handle,
                                );
                                assert(admission::admission_family_complete(top));
                                assert(next.admission.arena[score_handle as int].semantic);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_receive_absent_condition_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_receive_absent_condition_step(bridge) {
        None => true,
        Some((next, empty_handle, persistent_handle, peek_handle, count_handle,
            connective_handle, constructor_handle, score_handle)) => {
            let (persistent, peek, binds, body, bind_count, connective_used, cost) =
                bridge_receive_absent_condition_parts(
                    bridge.admission.frames.last(), bridge.admission.arena,
                ).unwrap();
            let children = seq![persistent_handle, peek_handle] + binds
                + seq![body, count_handle, connective_handle, empty_handle] + cost;
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, persistent_handle)
                == Some(score::ScoreNode::Integer(if persistent { 1i64 } else { 0i64 }))
            &&& score::handle_contents(next.transcript, peek_handle)
                == Some(score::ScoreNode::Integer(if peek { 1i64 } else { 0i64 }))
            &&& score::handle_contents(next.transcript, count_handle)
                == Some(score::ScoreNode::Integer(bind_count))
            &&& score::handle_contents(next.transcript, connective_handle)
                == Some(score::ScoreNode::Integer(
                    if connective_used { 1i64 } else { 0i64 },
                ))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(301i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if let Some((persistent, peek, binds, body, bind_count,
            connective_used, cost)) =
            bridge_receive_absent_condition_parts(top, bridge.admission.arena)
        {
            bridge_empty_par_unattached_exact(bridge);
            if let Some((after_empty, _, _, empty_handle)) =
                bridge_empty_par_unattached_step(bridge)
            {
                let persistent_kind = score::ScoreNode::Integer(
                    if persistent { 1i64 } else { 0i64 },
                );
                bridge_append_unattached_exact(after_empty, persistent_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_empty, persistent_kind,
                );
                if let Some((after_persistent, persistent_handle)) =
                    bridge_append_unattached(after_empty, persistent_kind)
                {
                    let peek_kind = score::ScoreNode::Integer(if peek { 1i64 } else { 0i64 });
                    bridge_append_unattached_exact(after_persistent, peek_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_persistent, peek_kind,
                    );
                    if let Some((after_peek, peek_handle)) =
                        bridge_append_unattached(after_persistent, peek_kind)
                    {
                        let count_kind = score::ScoreNode::Integer(bind_count);
                        bridge_append_unattached_exact(after_peek, count_kind);
                        bridge_append_unattached_preserves_admission_wf(after_peek, count_kind);
                        if let Some((after_count, count_handle)) =
                            bridge_append_unattached(after_peek, count_kind)
                        {
                            let connective_kind = score::ScoreNode::Integer(
                                if connective_used { 1i64 } else { 0i64 },
                            );
                            bridge_append_unattached_exact(after_count, connective_kind);
                            bridge_append_unattached_preserves_admission_wf(
                                after_count, connective_kind,
                            );
                            if let Some((after_connective, connective_handle)) =
                                bridge_append_unattached(after_count, connective_kind)
                            {
                                let children = seq![persistent_handle, peek_handle] + binds
                                    + seq![body, count_handle, connective_handle, empty_handle]
                                    + cost;
                                bridge_constructor_exact(after_connective, 301i32, children);
                                if let Some((next, _, score_handle)) =
                                    bridge_constructor_step(after_connective, 301i32, children)
                                {
                                    bridge_score_prefix_transitive(
                                        after_empty.transcript.entries,
                                        after_persistent.transcript.entries,
                                        after_peek.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_empty.transcript.entries,
                                        after_peek.transcript.entries,
                                        after_count.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_empty.transcript.entries,
                                        after_count.transcript.entries,
                                        after_connective.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_empty.transcript.entries,
                                        after_connective.transcript.entries,
                                        next.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_persistent.transcript.entries,
                                        after_peek.transcript.entries,
                                        after_count.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_persistent.transcript.entries,
                                        after_count.transcript.entries,
                                        after_connective.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_persistent.transcript.entries,
                                        after_connective.transcript.entries,
                                        next.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_peek.transcript.entries,
                                        after_count.transcript.entries,
                                        after_connective.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_peek.transcript.entries,
                                        after_connective.transcript.entries,
                                        next.transcript.entries,
                                    );
                                    bridge_score_prefix_transitive(
                                        after_count.transcript.entries,
                                        after_connective.transcript.entries,
                                        next.transcript.entries,
                                    );
                                    bridge_score_handle_preserved(
                                        after_empty.transcript, next.transcript, empty_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_persistent.transcript, next.transcript,
                                        persistent_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_peek.transcript, next.transcript, peek_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_count.transcript, next.transcript, count_handle,
                                    );
                                    bridge_score_handle_preserved(
                                        after_connective.transcript, next.transcript,
                                        connective_handle,
                                    );
                                    assert(admission::admission_family_complete(top));
                                    assert(next.admission.arena[score_handle as int].semantic);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_cost_token_stack_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_token_stack_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            let layers = bridge_cost_layers(
                bridge.admission.frames.last().returns, bridge.admission.arena,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& constructor_handle as int == bridge.transcript.entries.len()
            &&& score_handle as int == bridge.transcript.entries.len() + 1
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(508i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + layers))
            &&& score::handle_transcript(next.transcript, score_handle)
                == Some(bridge_cost_token_stack_transcript(bridge, layers))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == bridge_cost_token_stack_transcript(bridge, layers)
            &&& next.admission.arena[score_handle as int].encoded_len as int
                == bridge_cost_token_stack_transcript(bridge, layers).len()
            &&& next.admission.arena[score_handle as int].body_id
                == bridge.admission.frames.last().body_id
            &&& next.admission.arena[score_handle as int].semantic
        },
    },
{
    if bridge.admission.frames.len() > 0
        && bridge.admission.frames.last().family == admission::AdmissionFamily::CostTokenStack
    {
        let top = bridge.admission.frames.last();
        bridge_cost_layers_valid(top.returns, bridge.admission.arena);
        if let Some(layers) = bridge_cost_layers(top.returns, bridge.admission.arena) {
            assert(score::child_handles_valid(bridge.transcript.entries, layers)) by {
                assert forall|i: int| 0 <= i < layers.len()
                    implies (layers[i] as int) < bridge.transcript.entries.len() by {
                    assert((layers[i] as int) < bridge.admission.arena.len());
                }
            }
            bridge_constructor_exact(bridge, 508i32, layers);
            assert(admission::admission_family_complete(top));
        }
    }
}

pub proof fn bridge_cost_signature_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_signature_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            let atoms = bridge_cost_layers(
                bridge.admission.frames.last().returns, bridge.admission.arena,
            ).unwrap();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& constructor_handle as int == bridge.transcript.entries.len()
            &&& score_handle as int == bridge.transcript.entries.len() + 1
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(501i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + atoms))
            &&& score::handle_transcript(next.transcript, score_handle)
                == Some(bridge_cost_signature_transcript(bridge, atoms))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == bridge_cost_signature_transcript(bridge, atoms)
            &&& next.admission.arena[score_handle as int].encoded_len as int
                == bridge_cost_signature_transcript(bridge, atoms).len()
            &&& next.admission.arena[score_handle as int].body_id
                == bridge.admission.frames.last().body_id
            &&& next.admission.arena[score_handle as int].semantic
        },
    },
{
    if bridge.admission.frames.len() > 0
        && bridge.admission.frames.last().family == admission::AdmissionFamily::CostSignatureFrame
        && bridge.admission.frames.last().program_type_id == 9
    {
        let top = bridge.admission.frames.last();
        bridge_cost_layers_valid(top.returns, bridge.admission.arena);
        if let Some(atoms) = bridge_cost_layers(top.returns, bridge.admission.arena) {
            assert(score::child_handles_valid(bridge.transcript.entries, atoms)) by {
                assert forall|i: int| 0 <= i < atoms.len()
                    implies (atoms[i] as int) < bridge.transcript.entries.len() by {
                    assert((atoms[i] as int) < bridge.admission.arena.len());
                }
            }
            bridge_constructor_exact(bridge, 501i32, atoms);
            assert(admission::admission_family_complete(top));
        }
    }
}

pub proof fn bridge_cost_atom_absent_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_atom_absent_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(505i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle]))
            &&& next.admission.frames.last().score_handle == Some(score_handle)
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if top.program_type_id == 10
            && !top.seen.contains(0)
            && !top.scalar_slots.contains(0)
            && top.scalar_occurrences.len() == 0
            && top.returns.len() == 0
        {
            bridge_constructor_exact(bridge, 505i32, Seq::empty());
        }
    }
}

pub proof fn bridge_cost_atom_bytes_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_atom_bytes_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            let occurrence = bridge.admission.frames.last().scalar_occurrences[0];
            let bytes_handle = bridge.transcript.entries.len() as usize;
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& score::handle_contents(next.transcript, bytes_handle)
                == Some(score::ScoreNode::BytesAtom(occurrence.bytes))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(
                    bridge_cost_atom_bytes_code(occurrence.tag).unwrap() as i64,
                ))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(
                    seq![constructor_handle, bytes_handle],
                ))
            &&& next.admission.frames.last().score_handle == Some(score_handle)
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if top.program_type_id == 10
            && top.seen.contains(0)
            && top.scalar_slots.contains(0)
            && top.scalar_occurrences.len() == 1
            && top.returns.len() == 0
        {
            let occurrence = top.scalar_occurrences[0];
            if occurrence.slot == 0 && occurrence.scalar_kind == 16 {
                if let Some(constructor) = bridge_cost_atom_bytes_code(occurrence.tag) {
                    bridge_append_unattached_exact(
                        bridge, score::ScoreNode::BytesAtom(occurrence.bytes),
                    );
                    bridge_append_unattached_preserves_admission_wf(
                        bridge, score::ScoreNode::BytesAtom(occurrence.bytes),
                    );
                    if let Some((after_bytes, bytes_handle)) = bridge_append_unattached(
                        bridge, score::ScoreNode::BytesAtom(occurrence.bytes),
                    ) {
                        bridge_constructor_exact(
                            after_bytes, constructor, seq![bytes_handle],
                        );
                        if let Some((next, constructor_handle, score_handle)) = bridge_constructor_step(
                            after_bytes, constructor, seq![bytes_handle],
                        ) {
                            assert(bytes_handle as int == bridge.transcript.entries.len());
                            assert(next.transcript.entries.take(
                                after_bytes.transcript.entries.len() as int,
                            ) == after_bytes.transcript.entries);
                            assert(score::handle_contents(next.transcript, bytes_handle)
                                == Some(score::ScoreNode::BytesAtom(occurrence.bytes)));
                            assert(seq![constructor_handle] + seq![bytes_handle]
                                == seq![constructor_handle, bytes_handle]);
                            assert(score::handle_contents(next.transcript, score_handle)
                                == Some(score::ScoreNode::Node(
                                    seq![constructor_handle, bytes_handle],
                                )));
                            assert(bridge_cost_atom_bytes_step(bridge)
                                == Some((next, constructor_handle, score_handle)));
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_cost_bound_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_bound_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            let index = bridge_cost_bound_value(
                bridge.admission.frames.last(),
            ).unwrap();
            let index_handle = bridge.transcript.entries.len() as usize;
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& score::handle_contents(next.transcript, index_handle)
                == Some(score::ScoreNode::Integer(index as i64))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(504i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(
                    seq![constructor_handle, index_handle],
                ))
            &&& next.admission.frames.last().score_handle == Some(score_handle)
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        if let Some(index) = bridge_cost_bound_value(bridge.admission.frames.last()) {
            bridge_append_unattached_exact(
                bridge, score::ScoreNode::Integer(index as i64),
            );
            bridge_append_unattached_preserves_admission_wf(
                bridge, score::ScoreNode::Integer(index as i64),
            );
            if let Some((after_index, index_handle)) = bridge_append_unattached(
                bridge, score::ScoreNode::Integer(index as i64),
            ) {
                bridge_constructor_exact(
                    after_index, 504i32, seq![index_handle],
                );
                if let Some((next, constructor_handle, score_handle)) = bridge_constructor_step(
                    after_index, 504i32, seq![index_handle],
                ) {
                    assert(index_handle as int == bridge.transcript.entries.len());
                    assert(next.transcript.entries.take(
                        after_index.transcript.entries.len() as int,
                    ) == after_index.transcript.entries);
                    assert(score::handle_contents(next.transcript, index_handle)
                        == Some(score::ScoreNode::Integer(index as i64)));
                    assert(seq![constructor_handle] + seq![index_handle]
                        == seq![constructor_handle, index_handle]);
                    assert(score::handle_contents(next.transcript, score_handle)
                        == Some(score::ScoreNode::Node(
                            seq![constructor_handle, index_handle],
                        )));
                    assert(bridge_cost_bound_step(bridge)
                        == Some((next, constructor_handle, score_handle)));
                }
            }
        }
    }
}

pub proof fn bridge_cost_atom_bound_forward_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_atom_bound_forward_step(bridge) {
        None => true,
        Some((next, handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.transcript == bridge.transcript
            &&& next.admission.arena == bridge.admission.arena
            &&& next.admission.frames.last().score_handle == Some(handle)
            &&& score::handle_transcript(next.transcript, handle)
                == score::handle_transcript(bridge.transcript, handle)
            &&& bridge_binding_bytes_exact(
                next, handle, bridge.transcript.entries[handle as int].transcript,
            )
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if top.program_type_id == 10 && top.returns.len() == 1 {
            if let Some(handle) = top.returns[0].score_handle {
                admission::admission_forward_bound_score_refines(bridge.admission);
                admission::admission_forward_bound_score_preserves_wf(bridge.admission);
            }
        }
    }
    if let Some((next, handle)) = bridge_cost_atom_bound_forward_step(bridge) {
        bridge_existing_binding_has_exact_transcript(next, handle);
        assert(next.transcript.entries[handle as int]
            == bridge.transcript.entries[handle as int]);
    }
}

pub proof fn bridge_nonsemantic_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_nonsemantic_step(bridge) {
        None => true,
        Some((next, constructor_handle, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(i32::MAX as i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle]))
            &&& score::handle_transcript(next.transcript, score_handle)
                == Some(bridge_constructor_transcript(
                    bridge, i32::MAX, Seq::empty(),
                ))
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == bridge_constructor_transcript(bridge, i32::MAX, Seq::empty())
            &&& !next.admission.arena[score_handle as int].semantic
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if bridge_nonsemantic_family(top.family)
            && !admission::admission_family_complete(top)
        {
            bridge_constructor_exact(bridge, i32::MAX, Seq::empty());
        }
    }
}

pub proof fn bridge_raw_par_exact(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_raw_par_step(bridge) {
        None => true,
        Some((next, flag_handle, constructor_handle, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& next.admission.arena[score_handle as int].semantic
            &&& score::handle_contents(next.transcript, flag_handle)
                == Some(score::ScoreNode::Integer(
                    if admission::admission_raw_par_connective_used(
                        bridge.admission.frames.last(),
                    ) { 1i64 } else { 0i64 },
                ))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(999i64))
            &&& match bridge_raw_par_ordered_handles(
                bridge.admission.frames.last().returns, bridge_raw_par_score_tags(),
            ) {
                Some(components) => score::handle_contents(next.transcript, score_handle)
                    == Some(score::ScoreNode::Node(
                        seq![constructor_handle] + components.push(flag_handle),
                    )),
                None => false,
            }
        },
    },
{
    if bridge.admission.frames.len() > 0 {
        let top = bridge.admission.frames.last();
        if bridge_raw_par_returns_source(top, bridge.admission.arena) {
            if let Some(components) = bridge_raw_par_ordered_handles(
                top.returns, bridge_raw_par_score_tags(),
            ) {
                let flag = if admission::admission_raw_par_connective_used(top) {
                    1i64
                } else { 0i64 };
                bridge_append_unattached_exact(bridge, score::ScoreNode::Integer(flag));
                bridge_append_unattached_preserves_admission_wf(
                    bridge, score::ScoreNode::Integer(flag),
                );
                if let Some((after_flag, flag_handle)) = bridge_append_unattached(
                    bridge, score::ScoreNode::Integer(flag),
                ) {
                    bridge_constructor_exact(
                        after_flag, 999i32, components.push(flag_handle),
                    );
                }
            }
        }
    }
}

pub open spec fn bridge_same(
    left: ScoreAdmissionBridge, right: ScoreAdmissionBridge,
) -> bool {
    left.admission == right.admission && left.transcript == right.transcript
}

pub open spec fn bridge_admission_only_step(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
    next_admission: Option<admission::AdmissionState>,
) -> bool {
    match next_admission {
        None => false,
        Some(state) => after.admission == state && after.transcript == before.transcript,
    }
}

pub open spec fn bridge_score_source_step(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
) -> bool {
    ||| match bridge_raw_par_step(before) {
        None => false,
        Some((next, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_cost_atom_absent_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_cost_atom_bytes_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_cost_bound_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_cost_atom_bound_forward_step(before) {
        None => false,
        Some((next, _)) => bridge_same(next, after),
    }
    ||| match bridge_cost_signature_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_cost_region_step(before) {
        None => false,
        Some((next, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_cost_token_stack_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_bundle_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_if_step(before) {
        None => false,
        Some((next, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_send_step(before) {
        None => false,
        Some((next, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_match_step(before) {
        None => false,
        Some((next, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_absent_specialized_constructor_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_absent_connective_step(before) {
        None => false,
        Some((next, _)) => bridge_same(next, after),
    }
    ||| match bridge_receive_bind_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_match_case_step(before) {
        None => false,
        Some((next, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_bool_oneof_step(before) {
        None => false,
        Some((next, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_expr_atom_step(before) {
        None => false,
        Some((next, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_expr_numeric_step(before) {
        None => false,
        Some((next, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_expr_big_rational_step(before) {
        None => false,
        Some((next, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_expr_fixed_point_step(before) {
        None => false,
        Some((next, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_sys_auth_token_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_new_empty_collections_step(before) {
        None => false,
        Some((next, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_new_one_injection_step(before) {
        None => false,
        Some((next, _, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_new_one_absent_injection_step(before) {
        None => false,
        Some((next, _, _, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_new_one_uri_step(before) {
        None => false,
        Some((next, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_new_collections_step(before) {
        None => false,
        Some((next, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_expr_payload_forward_step(before) {
        None => false,
        Some((next, _)) => bridge_same(next, after),
    }
    ||| match bridge_expr_structural_step(before) {
        None => false,
        Some((next, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_evar_step(before) {
        None => false,
        Some((next, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_ordered_sequence_step(before) {
        None => false,
        Some((next, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_emethod_step(before) {
        None => false,
        Some((next, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_emap_finish_step(before) {
        None => false,
        Some((next, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_connective_not_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_connective_varref_step(before) {
        None => false,
        Some((next, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_connective_collection_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_unforgeable_bytes_step(before) {
        None => false,
        Some((next, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_receive_present_condition_step(before) {
        None => false,
        Some((next, _, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_receive_absent_condition_step(before) {
        None => false,
        Some((next, _, _, _, _, _, _, _)) => bridge_same(next, after),
    }
    ||| match bridge_nonsemantic_step(before) {
        None => false,
        Some((next, _, _)) => bridge_same(next, after),
    }
}

pub enum BridgeScoreFinishSelection {
    NoCandidate,
    Unique(ScoreAdmissionBridge),
    Ambiguous,
}

pub open spec fn bridge_add_score_finish_candidate(
    selection: BridgeScoreFinishSelection,
    candidate: Option<ScoreAdmissionBridge>,
) -> BridgeScoreFinishSelection {
    match selection {
        BridgeScoreFinishSelection::NoCandidate => match candidate {
            None => BridgeScoreFinishSelection::NoCandidate,
            Some(next) => BridgeScoreFinishSelection::Unique(next),
        },
        BridgeScoreFinishSelection::Unique(prior) => match candidate {
            None => BridgeScoreFinishSelection::Unique(prior),
            Some(next) => if bridge_same(prior, next) {
                BridgeScoreFinishSelection::Unique(prior)
            } else {
                BridgeScoreFinishSelection::Ambiguous
            },
        },
        BridgeScoreFinishSelection::Ambiguous => BridgeScoreFinishSelection::Ambiguous,
    }
}

pub open spec fn bridge_score_finish_selection_sound(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
) -> bool {
    match selection {
        BridgeScoreFinishSelection::Unique(next) => bridge_score_source_step(before, next),
        _ => true,
    }
}

pub proof fn bridge_add_score_finish_candidate_sound(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
    candidate: Option<ScoreAdmissionBridge>,
)
    requires
        bridge_score_finish_selection_sound(before, selection),
        match candidate {
            None => true,
            Some(next) => bridge_score_source_step(before, next),
        },
    ensures bridge_score_finish_selection_sound(
        before, bridge_add_score_finish_candidate(selection, candidate),
    ),
{
    match selection {
        BridgeScoreFinishSelection::NoCandidate => {},
        BridgeScoreFinishSelection::Unique(_) => {},
        BridgeScoreFinishSelection::Ambiguous => {},
    }
}

#[verifier::opaque]
pub open spec fn bridge_score_finish_selection_group_1(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
) -> BridgeScoreFinishSelection {
    let candidate_1 = match bridge_raw_par_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_cost_atom_absent_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_cost_atom_bytes_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_cost_bound_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_cost_atom_bound_forward_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    };
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_cost_signature_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_cost_region_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    let candidate_8 = match bridge_cost_token_stack_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_8 = bridge_add_score_finish_candidate(
        selection_7, candidate_8,
    );
    selection_8
}

pub proof fn bridge_score_finish_selection_group_1_sound(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
)
    requires bridge_score_finish_selection_sound(before, selection_0),
    ensures bridge_score_finish_selection_sound(before, bridge_score_finish_selection_group_1(before, selection_0)),
{
    reveal(bridge_score_finish_selection_group_1);
    let candidate_1 = match bridge_raw_par_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_1 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_0, candidate_1);
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_cost_atom_absent_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_2 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_1, candidate_2);
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_cost_atom_bytes_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_3 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_2, candidate_3);
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_cost_bound_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_4 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_3, candidate_4);
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_cost_atom_bound_forward_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    };
    if let Some(next) = candidate_5 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_4, candidate_5);
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_cost_signature_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_6 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_5, candidate_6);
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_cost_region_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_7 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_6, candidate_7);
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    let candidate_8 = match bridge_cost_token_stack_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_8 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_7, candidate_8);
    let selection_8 = bridge_add_score_finish_candidate(
        selection_7, candidate_8,
    );
    assert(bridge_score_finish_selection_sound(before, selection_8));
}

#[verifier::opaque]
pub open spec fn bridge_score_finish_selection_group_2(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
) -> BridgeScoreFinishSelection {
    let candidate_1 = match bridge_bundle_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_if_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_send_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_match_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_absent_specialized_constructor_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_absent_connective_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    };
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_receive_bind_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    let candidate_8 = match bridge_match_case_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    let selection_8 = bridge_add_score_finish_candidate(
        selection_7, candidate_8,
    );
    selection_8
}

pub proof fn bridge_score_finish_selection_group_2_sound(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
)
    requires bridge_score_finish_selection_sound(before, selection_0),
    ensures bridge_score_finish_selection_sound(before, bridge_score_finish_selection_group_2(before, selection_0)),
{
    reveal(bridge_score_finish_selection_group_2);
    let candidate_1 = match bridge_bundle_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_1 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_0, candidate_1);
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_if_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_2 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_1, candidate_2);
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_send_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_3 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_2, candidate_3);
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_match_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_4 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_3, candidate_4);
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_absent_specialized_constructor_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_5 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_4, candidate_5);
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_absent_connective_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    };
    if let Some(next) = candidate_6 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_5, candidate_6);
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_receive_bind_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_7 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_6, candidate_7);
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    let candidate_8 = match bridge_match_case_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_8 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_7, candidate_8);
    let selection_8 = bridge_add_score_finish_candidate(
        selection_7, candidate_8,
    );
    assert(bridge_score_finish_selection_sound(before, selection_8));
}

#[verifier::opaque]
pub open spec fn bridge_score_finish_selection_group_3(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
) -> BridgeScoreFinishSelection {
    let candidate_1 = match bridge_bool_oneof_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_expr_atom_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_expr_numeric_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_expr_big_rational_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_expr_fixed_point_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_sys_auth_token_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_new_empty_collections_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    let candidate_8 = match bridge_new_one_injection_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _)) => Some(next),
    };
    let selection_8 = bridge_add_score_finish_candidate(
        selection_7, candidate_8,
    );
    selection_8
}

pub proof fn bridge_score_finish_selection_group_3_sound(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
)
    requires bridge_score_finish_selection_sound(before, selection_0),
    ensures bridge_score_finish_selection_sound(before, bridge_score_finish_selection_group_3(before, selection_0)),
{
    reveal(bridge_score_finish_selection_group_3);
    let candidate_1 = match bridge_bool_oneof_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_1 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_0, candidate_1);
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_expr_atom_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_2 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_1, candidate_2);
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_expr_numeric_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_3 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_2, candidate_3);
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_expr_big_rational_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_4 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_3, candidate_4);
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_expr_fixed_point_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_5 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_4, candidate_5);
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_sys_auth_token_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_6 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_5, candidate_6);
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_new_empty_collections_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_7 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_6, candidate_7);
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    let candidate_8 = match bridge_new_one_injection_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_8 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_7, candidate_8);
    let selection_8 = bridge_add_score_finish_candidate(
        selection_7, candidate_8,
    );
    assert(bridge_score_finish_selection_sound(before, selection_8));
}

#[verifier::opaque]
pub open spec fn bridge_score_finish_selection_group_4(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
) -> BridgeScoreFinishSelection {
    let candidate_1 = match bridge_new_one_absent_injection_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _, _)) => Some(next),
    };
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_new_one_uri_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_new_collections_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_expr_payload_forward_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    };
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_expr_structural_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_evar_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_ordered_sequence_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    let candidate_8 = match bridge_emethod_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    let selection_8 = bridge_add_score_finish_candidate(
        selection_7, candidate_8,
    );
    let candidate_9 = match bridge_emap_finish_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    let selection_9 = bridge_add_score_finish_candidate(
        selection_8, candidate_9,
    );
    selection_9
}

pub proof fn bridge_score_finish_selection_group_4_sound(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
)
    requires bridge_score_finish_selection_sound(before, selection_0),
    ensures bridge_score_finish_selection_sound(before, bridge_score_finish_selection_group_4(before, selection_0)),
{
    reveal(bridge_score_finish_selection_group_4);
    let candidate_1 = match bridge_new_one_absent_injection_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_1 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_0, candidate_1);
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_new_one_uri_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_2 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_1, candidate_2);
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_new_collections_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_3 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_2, candidate_3);
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_expr_payload_forward_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    };
    if let Some(next) = candidate_4 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_3, candidate_4);
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_expr_structural_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_5 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_4, candidate_5);
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_evar_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_6 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_5, candidate_6);
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_ordered_sequence_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_7 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_6, candidate_7);
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    let candidate_8 = match bridge_emethod_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_8 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_7, candidate_8);
    let selection_8 = bridge_add_score_finish_candidate(
        selection_7, candidate_8,
    );
    let candidate_9 = match bridge_emap_finish_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_9 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_8, candidate_9);
    let selection_9 = bridge_add_score_finish_candidate(
        selection_8, candidate_9,
    );
    assert(bridge_score_finish_selection_sound(before, selection_9));
}

#[verifier::opaque]
pub open spec fn bridge_score_finish_selection_group_5(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
) -> BridgeScoreFinishSelection {
    let candidate_1 = match bridge_connective_not_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_connective_varref_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_connective_collection_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_unforgeable_bytes_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_receive_present_condition_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _)) => Some(next),
    };
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_receive_absent_condition_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _, _)) => Some(next),
    };
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_nonsemantic_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    selection_7
}

pub proof fn bridge_score_finish_selection_group_5_sound(
    before: ScoreAdmissionBridge, selection_0: BridgeScoreFinishSelection,
)
    requires bridge_score_finish_selection_sound(before, selection_0),
    ensures bridge_score_finish_selection_sound(before, bridge_score_finish_selection_group_5(before, selection_0)),
{
    reveal(bridge_score_finish_selection_group_5);
    let candidate_1 = match bridge_connective_not_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_1 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_0, candidate_1);
    let selection_1 = bridge_add_score_finish_candidate(
        selection_0, candidate_1,
    );
    let candidate_2 = match bridge_connective_varref_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_2 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_1, candidate_2);
    let selection_2 = bridge_add_score_finish_candidate(
        selection_1, candidate_2,
    );
    let candidate_3 = match bridge_connective_collection_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_3 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_2, candidate_3);
    let selection_3 = bridge_add_score_finish_candidate(
        selection_2, candidate_3,
    );
    let candidate_4 = match bridge_unforgeable_bytes_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_4 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_3, candidate_4);
    let selection_4 = bridge_add_score_finish_candidate(
        selection_3, candidate_4,
    );
    let candidate_5 = match bridge_receive_present_condition_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_5 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_4, candidate_5);
    let selection_5 = bridge_add_score_finish_candidate(
        selection_4, candidate_5,
    );
    let candidate_6 = match bridge_receive_absent_condition_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _, _)) => Some(next),
    };
    if let Some(next) = candidate_6 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_5, candidate_6);
    let selection_6 = bridge_add_score_finish_candidate(
        selection_5, candidate_6,
    );
    let candidate_7 = match bridge_nonsemantic_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if let Some(next) = candidate_7 {
        assert(bridge_score_source_step(before, next));
    }
    bridge_add_score_finish_candidate_sound(before, selection_6, candidate_7);
    let selection_7 = bridge_add_score_finish_candidate(
        selection_6, candidate_7,
    );
    assert(bridge_score_finish_selection_sound(before, selection_7));
}

#[verifier::opaque]
pub open spec fn bridge_score_finish_selected(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    let selection_0 = BridgeScoreFinishSelection::NoCandidate;
    let selection_1 = bridge_score_finish_selection_group_1(before, selection_0);
    let selection_2 = bridge_score_finish_selection_group_2(before, selection_1);
    let selection_3 = bridge_score_finish_selection_group_3(before, selection_2);
    let selection_4 = bridge_score_finish_selection_group_4(before, selection_3);
    let selection_5 = bridge_score_finish_selection_group_5(before, selection_4);
    match selection_5 {
        BridgeScoreFinishSelection::Unique(next) => Some(next),
        _ => None,
    }
}

pub proof fn bridge_score_finish_selected_is_source_step(
    before: ScoreAdmissionBridge,
)
    ensures match bridge_score_finish_selected(before) {
        None => true,
        Some(after) => bridge_score_source_step(before, after),
    },
{
    reveal(bridge_score_finish_selected);
    let selection_0 = BridgeScoreFinishSelection::NoCandidate;
    assert(bridge_score_finish_selection_sound(before, selection_0));
    bridge_score_finish_selection_group_1_sound(before, selection_0);
    let selection_1 = bridge_score_finish_selection_group_1(before, selection_0);
    bridge_score_finish_selection_group_2_sound(before, selection_1);
    let selection_2 = bridge_score_finish_selection_group_2(before, selection_1);
    bridge_score_finish_selection_group_3_sound(before, selection_2);
    let selection_3 = bridge_score_finish_selection_group_3(before, selection_2);
    bridge_score_finish_selection_group_4_sound(before, selection_3);
    let selection_4 = bridge_score_finish_selection_group_4(before, selection_3);
    bridge_score_finish_selection_group_5_sound(before, selection_4);
    let selection_5 = bridge_score_finish_selection_group_5(before, selection_4);
    assert(bridge_score_finish_selection_sound(before, selection_5));
}

pub proof fn bridge_score_finish_selected_preserves_checked(
    before: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match bridge_score_finish_selected(before) {
        None => true,
        Some(after) => {
            &&& bridge_score_source_step(before, after)
            &&& bridge_well_formed(after)
            &&& admission::admission_state_wf(after.admission)
            &&& bridge_score_step_shape(before, after)
        },
    },
{
    bridge_score_finish_selected_is_source_step(before);
    if let Some(after) = bridge_score_finish_selected(before) {
        bridge_score_source_step_preserves_exact(before, after);
        bridge_score_source_step_preserves_shape(before, after);
    }
}

pub open spec fn bridge_primary_component_frame(before: ScoreAdmissionBridge) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && match before.admission.frames.last().family {
            admission::AdmissionFamily::Send
            | admission::AdmissionFamily::Bundle
            | admission::AdmissionFamily::If
            | admission::AdmissionFamily::Match => true,
            _ => false,
        }
}

pub proof fn bridge_primary_component_group_1_empty(before: ScoreAdmissionBridge)
    requires bridge_primary_component_frame(before),
    ensures bridge_score_finish_selection_group_1(
        before, BridgeScoreFinishSelection::NoCandidate,
    ) == BridgeScoreFinishSelection::NoCandidate,
{
    let top = before.admission.frames.last();
    assert(top.program_type_id == admission::admission_family_program_id(top.family).unwrap());
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    reveal(bridge_score_finish_selection_group_1);
}

pub open spec fn bridge_primary_component_finish(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    if before.admission.frames.len() == 0 {
        None
    } else {
        match before.admission.frames.last().family {
            admission::AdmissionFamily::Send => match bridge_send_step(before) {
                None => None,
                Some((next, _, _, _, _)) => Some(next),
            },
            admission::AdmissionFamily::Bundle => match bridge_bundle_step(before) {
                None => None,
                Some((next, _, _)) => Some(next),
            },
            admission::AdmissionFamily::If => match bridge_if_step(before) {
                None => None,
                Some((next, _, _, _)) => Some(next),
            },
            admission::AdmissionFamily::Match => match bridge_match_step(before) {
                None => None,
                Some((next, _, _, _)) => Some(next),
            },
            _ => None,
        }
    }
}

pub proof fn bridge_primary_component_group_2_exact(before: ScoreAdmissionBridge)
    requires bridge_primary_component_frame(before),
    ensures match bridge_primary_component_finish(before) {
        None => bridge_score_finish_selection_group_2(
            before, BridgeScoreFinishSelection::NoCandidate,
        ) == BridgeScoreFinishSelection::NoCandidate,
        Some(next) => bridge_score_finish_selection_group_2(
            before, BridgeScoreFinishSelection::NoCandidate,
        ) == BridgeScoreFinishSelection::Unique(next),
    },
{
    let top = before.admission.frames.last();
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    match top.family {
        admission::AdmissionFamily::Send => {
            assert(bridge_bundle_step(before).is_none());
            assert(bridge_if_step(before).is_none());
            assert(bridge_match_step(before).is_none());
        },
        admission::AdmissionFamily::Bundle => {
            assert(bridge_if_step(before).is_none());
            assert(bridge_send_step(before).is_none());
            assert(bridge_match_step(before).is_none());
        },
        admission::AdmissionFamily::If => {
            assert(bridge_bundle_step(before).is_none());
            assert(bridge_send_step(before).is_none());
            assert(bridge_match_step(before).is_none());
        },
        admission::AdmissionFamily::Match => {
            assert(bridge_bundle_step(before).is_none());
            assert(bridge_if_step(before).is_none());
            assert(bridge_send_step(before).is_none());
        },
        _ => {},
    }
    reveal(bridge_score_finish_selection_group_2);
}

pub proof fn bridge_primary_component_group_3_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_primary_component_frame(before),
    ensures bridge_score_finish_selection_group_3(before, selection) == selection,
{
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    reveal(bridge_score_finish_selection_group_3);
}

pub proof fn bridge_primary_component_group_4_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_primary_component_frame(before),
    ensures bridge_score_finish_selection_group_4(before, selection) == selection,
{
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    reveal(bridge_score_finish_selection_group_4);
}

pub open spec fn bridge_nonsemantic_finish_candidate(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    match bridge_nonsemantic_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    }
}

pub proof fn bridge_primary_component_group_5_nonsemantic(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_primary_component_frame(before),
    ensures bridge_score_finish_selection_group_5(before, selection)
        == bridge_add_score_finish_candidate(
            selection, bridge_nonsemantic_finish_candidate(before),
        ),
{
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    reveal(bridge_score_finish_selection_group_5);
}

pub proof fn bridge_primary_component_finish_excludes_nonsemantic(
    before: ScoreAdmissionBridge,
)
    requires bridge_primary_component_frame(before),
    ensures bridge_primary_component_finish(before).is_some()
        ==> bridge_nonsemantic_finish_candidate(before).is_none(),
{
    let top = before.admission.frames.last();
    if bridge_primary_component_finish(before).is_some() {
        match top.family {
            admission::AdmissionFamily::Send => {
                assert(bridge_send_step(before).is_some());
                assert(admission::admission_family_complete(top));
            },
            admission::AdmissionFamily::Bundle => {
                assert(bridge_bundle_step(before).is_some());
                assert(admission::admission_family_complete(top));
            },
            admission::AdmissionFamily::If => {
                assert(bridge_if_step(before).is_some());
                assert(admission::admission_family_complete(top));
            },
            admission::AdmissionFamily::Match => {
                assert(bridge_match_step(before).is_some());
                assert(admission::admission_family_complete(top));
            },
            _ => {},
        }
    }
}

pub proof fn bridge_primary_component_finish_selected_exact(
    before: ScoreAdmissionBridge,
)
    requires bridge_primary_component_frame(before),
    ensures
        bridge_primary_component_finish(before).is_some()
            ==> bridge_nonsemantic_finish_candidate(before).is_none(),
        bridge_score_finish_selected(before) == if bridge_primary_component_finish(before).is_some() {
            bridge_primary_component_finish(before)
        } else {
            bridge_nonsemantic_finish_candidate(before)
        },
{
    bridge_primary_component_group_1_empty(before);
    bridge_primary_component_group_2_exact(before);
    bridge_primary_component_finish_excludes_nonsemantic(before);
    reveal(bridge_score_finish_selected);
    let empty = BridgeScoreFinishSelection::NoCandidate;
    let first = bridge_score_finish_selection_group_1(before, empty);
    let second = bridge_score_finish_selection_group_2(before, first);
    bridge_primary_component_group_3_empty(before, second);
    let third = bridge_score_finish_selection_group_3(before, second);
    bridge_primary_component_group_4_empty(before, third);
    let fourth = bridge_score_finish_selection_group_4(before, third);
    bridge_primary_component_group_5_nonsemantic(before, fourth);
    if let Some(next) = bridge_primary_component_finish(before) {
        assert(second == BridgeScoreFinishSelection::Unique(next));
        assert(bridge_nonsemantic_finish_candidate(before).is_none());
    } else {
        assert(second == BridgeScoreFinishSelection::NoCandidate);
    }
}

pub proof fn bridge_primary_component_source_step_has_unique_finish(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_primary_component_frame(before),
        admission::admission_family_complete(before.admission.frames.last()),
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
        forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) ==> bridge_same(after, other),
{
    let top = before.admission.frames.last();
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    match top.family {
        admission::AdmissionFamily::Send => {
            assert(bridge_bundle_step(before).is_none());
            assert(bridge_if_step(before).is_none());
            assert(bridge_match_step(before).is_none());
        },
        admission::AdmissionFamily::Bundle => {
            assert(bridge_send_step(before).is_none());
            assert(bridge_if_step(before).is_none());
            assert(bridge_match_step(before).is_none());
        },
        admission::AdmissionFamily::If => {
            assert(bridge_send_step(before).is_none());
            assert(bridge_bundle_step(before).is_none());
            assert(bridge_match_step(before).is_none());
        },
        admission::AdmissionFamily::Match => {
            assert(bridge_send_step(before).is_none());
            assert(bridge_bundle_step(before).is_none());
            assert(bridge_if_step(before).is_none());
        },
        _ => {},
    }
    bridge_primary_component_finish_selected_exact(before);
    assert(bridge_nonsemantic_finish_candidate(before).is_none());
    assert forall|other: ScoreAdmissionBridge|
        bridge_score_source_step(before, other)
            implies match bridge_primary_component_finish(before) {
                Some(next) => bridge_same(next, other),
                None => false,
            } by {
        assert(bridge_score_source_step(before, other));
    }
    assert(bridge_primary_component_finish(before).is_some());
    let next = bridge_primary_component_finish(before).unwrap();
    assert(bridge_score_finish_selected(before) == Some(next));
    assert(bridge_same(next, after));
    assert forall|other: ScoreAdmissionBridge|
        bridge_score_source_step(before, other) implies bridge_same(after, other) by {
        assert(bridge_same(next, other));
    }
}

pub proof fn bridge_cost_token_stack_source_step_has_unique_finish(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::CostTokenStack,
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
        forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) ==> bridge_same(after, other),
{
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_some());
    let (next, _, _) = bridge_cost_token_stack_step(before).unwrap();
    let empty = BridgeScoreFinishSelection::NoCandidate;
    reveal(bridge_score_finish_selection_group_1);
    assert(bridge_score_finish_selection_group_1(before, empty)
        == BridgeScoreFinishSelection::Unique(next));
    let first = bridge_score_finish_selection_group_1(before, empty);
    reveal(bridge_score_finish_selection_group_2);
    let second = bridge_score_finish_selection_group_2(before, first);
    assert(second == BridgeScoreFinishSelection::Unique(next));
    reveal(bridge_score_finish_selection_group_3);
    let third = bridge_score_finish_selection_group_3(before, second);
    assert(third == BridgeScoreFinishSelection::Unique(next));
    reveal(bridge_score_finish_selection_group_4);
    let fourth = bridge_score_finish_selection_group_4(before, third);
    assert(fourth == BridgeScoreFinishSelection::Unique(next));
    reveal(bridge_score_finish_selection_group_5);
    let fifth = bridge_score_finish_selection_group_5(before, fourth);
    assert(fifth == BridgeScoreFinishSelection::Unique(next));
    reveal(bridge_score_finish_selected);
    assert(bridge_score_finish_selected(before) == Some(next));
    assert(bridge_same(next, after));
    assert forall|other: ScoreAdmissionBridge|
        bridge_score_source_step(before, other) implies bridge_same(after, other) by {
        assert(bridge_score_source_step(before, other));
        assert(bridge_same(next, other));
    }
}

pub proof fn bridge_gunforgeable_source_step_matches_selected(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::GUnforgeable,
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
{
    let top = before.admission.frames.last();
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    let absent = match bridge_absent_specialized_constructor_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let token = match bridge_sys_auth_token_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let bytes = match bridge_unforgeable_bytes_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    if absent.is_some() {
        assert(top.returns.len() == 0);
        assert(token.is_none());
        assert(bytes.is_none());
    }
    if token.is_some() {
        assert(top.returns.len() == 1);
        assert(top.returns[0].tag == 4);
        assert(absent.is_none());
        assert(bytes.is_none());
    }
    if bytes.is_some() {
        assert(top.returns.len() == 1);
        assert(top.returns[0].tag != 4);
        assert(absent.is_none());
        assert(token.is_none());
    }
    assert(absent.is_some() || token.is_some() || bytes.is_some());
    let empty = BridgeScoreFinishSelection::NoCandidate;
    reveal(bridge_score_finish_selection_group_1);
    let first = bridge_score_finish_selection_group_1(before, empty);
    assert(first == empty);
    reveal(bridge_score_finish_selection_group_2);
    let second = bridge_score_finish_selection_group_2(before, first);
    assert(second == match absent {
        Some(next) => BridgeScoreFinishSelection::Unique(next),
        None => empty,
    });
    reveal(bridge_score_finish_selection_group_3);
    let third = bridge_score_finish_selection_group_3(before, second);
    assert(third == if absent.is_some() {
        second
    } else {
        match token {
            Some(next) => BridgeScoreFinishSelection::Unique(next),
            None => empty,
        }
    });
    reveal(bridge_score_finish_selection_group_4);
    let fourth = bridge_score_finish_selection_group_4(before, third);
    assert(fourth == third);
    reveal(bridge_score_finish_selection_group_5);
    let fifth = bridge_score_finish_selection_group_5(before, fourth);
    assert(fifth == if absent.is_some() || token.is_some() {
        fourth
    } else {
        match bytes {
            Some(next) => BridgeScoreFinishSelection::Unique(next),
            None => empty,
        }
    });
    reveal(bridge_score_finish_selected);
    if absent.is_some() {
        assert(bridge_score_finish_selected(before) == absent);
        assert(bridge_same(absent.unwrap(), after));
    } else if token.is_some() {
        assert(bridge_score_finish_selected(before) == token);
        assert(bridge_same(token.unwrap(), after));
    } else {
        assert(bytes.is_some());
        assert(bridge_score_finish_selected(before) == bytes);
        assert(bridge_same(bytes.unwrap(), after));
    }
}

pub proof fn bridge_gunforgeable_source_step_has_unique_finish(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::GUnforgeable,
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
        forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) ==> bridge_same(after, other),
{
    bridge_gunforgeable_source_step_matches_selected(before, after);
    assert forall|other: ScoreAdmissionBridge|
        bridge_score_source_step(before, other) implies bridge_same(after, other) by {
        bridge_gunforgeable_source_step_matches_selected(before, other);
    }
}

pub proof fn bridge_connective_source_step_matches_selected(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::Connective,
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
{
    let top = before.admission.frames.last();
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    let absent = match bridge_absent_connective_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    };
    let boolean = match bridge_bool_oneof_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let not = match bridge_connective_not_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let varref = match bridge_connective_varref_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    let collection = match bridge_connective_collection_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    if absent.is_some() {
        assert(top.returns.len() == 0);
        assert(top.scalar_occurrences.len() == 0);
        assert(boolean.is_none());
        assert(not.is_none());
        assert(varref.is_none());
        assert(collection.is_none());
    }
    if boolean.is_some() {
        assert(top.returns.len() == 0);
        assert(top.scalar_occurrences.len() == 1);
        assert(absent.is_none());
        assert(not.is_none());
        assert(varref.is_none());
        assert(collection.is_none());
    }
    if not.is_some() {
        assert(top.returns.len() == 1);
        assert(top.returns[0].tag == 3);
        assert(absent.is_none());
        assert(boolean.is_none());
        assert(varref.is_none());
        assert(collection.is_none());
    }
    if varref.is_some() {
        assert(top.returns.len() == 1);
        assert(top.returns[0].tag == 4);
        assert(absent.is_none());
        assert(boolean.is_none());
        assert(not.is_none());
        assert(collection.is_none());
    }
    if collection.is_some() {
        assert(top.returns.len() == 1);
        assert(top.returns[0].tag == 1 || top.returns[0].tag == 2);
        assert(absent.is_none());
        assert(boolean.is_none());
        assert(not.is_none());
        assert(varref.is_none());
    }
    assert(absent.is_some() || boolean.is_some() || not.is_some()
        || varref.is_some() || collection.is_some());
    let empty = BridgeScoreFinishSelection::NoCandidate;
    reveal(bridge_score_finish_selection_group_1);
    let first = bridge_score_finish_selection_group_1(before, empty);
    assert(first == empty);
    reveal(bridge_score_finish_selection_group_2);
    let second = bridge_score_finish_selection_group_2(before, first);
    assert(second == match absent {
        Some(next) => BridgeScoreFinishSelection::Unique(next),
        None => empty,
    });
    reveal(bridge_score_finish_selection_group_3);
    let third = bridge_score_finish_selection_group_3(before, second);
    assert(third == if absent.is_some() {
        second
    } else {
        match boolean {
            Some(next) => BridgeScoreFinishSelection::Unique(next),
            None => empty,
        }
    });
    reveal(bridge_score_finish_selection_group_4);
    let fourth = bridge_score_finish_selection_group_4(before, third);
    assert(fourth == third);
    reveal(bridge_score_finish_selection_group_5);
    let fifth = bridge_score_finish_selection_group_5(before, fourth);
    assert(fifth == if absent.is_some() || boolean.is_some() {
        fourth
    } else if not.is_some() {
        BridgeScoreFinishSelection::Unique(not.unwrap())
    } else if varref.is_some() {
        BridgeScoreFinishSelection::Unique(varref.unwrap())
    } else {
        match collection {
            Some(next) => BridgeScoreFinishSelection::Unique(next),
            None => empty,
        }
    });
    reveal(bridge_score_finish_selected);
    if absent.is_some() {
        assert(bridge_score_finish_selected(before) == absent);
        assert(bridge_same(absent.unwrap(), after));
    } else if boolean.is_some() {
        assert(bridge_score_finish_selected(before) == boolean);
        assert(bridge_same(boolean.unwrap(), after));
    } else if not.is_some() {
        assert(bridge_score_finish_selected(before) == not);
        assert(bridge_same(not.unwrap(), after));
    } else if varref.is_some() {
        assert(bridge_score_finish_selected(before) == varref);
        assert(bridge_same(varref.unwrap(), after));
    } else {
        assert(collection.is_some());
        assert(bridge_score_finish_selected(before) == collection);
        assert(bridge_same(collection.unwrap(), after));
    }
}

pub proof fn bridge_connective_source_step_has_unique_finish(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::Connective,
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
        forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) ==> bridge_same(after, other),
{
    bridge_connective_source_step_matches_selected(before, after);
    assert forall|other: ScoreAdmissionBridge|
        bridge_score_source_step(before, other) implies bridge_same(after, other) by {
        bridge_connective_source_step_matches_selected(before, other);
    }
}

pub proof fn bridge_expr_source_step_matches_selected(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::Expr,
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
{
    let top = before.admission.frames.last();
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    let absent = match bridge_absent_specialized_constructor_step(before) {
        None => None,
        Some((next, _, _)) => Some(next),
    };
    let boolean = match bridge_bool_oneof_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let atom = match bridge_expr_atom_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let numeric = match bridge_expr_numeric_step(before) {
        None => None,
        Some((next, _, _, _)) => Some(next),
    };
    let rational = match bridge_expr_big_rational_step(before) {
        None => None,
        Some((next, _, _, _, _)) => Some(next),
    };
    let fixed = match bridge_expr_fixed_point_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    };
    let forwarded = match bridge_expr_payload_forward_step(before) {
        None => None,
        Some((next, _)) => Some(next),
    };
    let nonsemantic = bridge_nonsemantic_finish_candidate(before);
    if absent.is_some() {
        assert(!top.seen.contains(0));
        assert(top.returns.len() == 0);
        assert(top.scalar_occurrences.len() == 0);
        assert(admission::admission_family_complete(top));
        assert(boolean.is_none() && atom.is_none() && numeric.is_none());
        assert(rational.is_none() && fixed.is_none() && forwarded.is_none());
        assert(nonsemantic.is_none());
    }
    if boolean.is_some() {
        assert(top.scalar_occurrences.len() == 1);
        assert(top.scalar_occurrences[0].tag == 1);
        assert(top.returns.len() == 0);
        assert(admission::admission_family_complete(top));
        assert(absent.is_none() && atom.is_none() && numeric.is_none());
        assert(rational.is_none() && fixed.is_none() && forwarded.is_none());
        assert(nonsemantic.is_none());
    }
    if atom.is_some() {
        assert(top.scalar_occurrences.len() == 1);
        assert(top.scalar_occurrences[0].tag == 3
            || top.scalar_occurrences[0].tag == 4
            || top.scalar_occurrences[0].tag == 25
            || top.scalar_occurrences[0].tag == 35);
        assert(top.returns.len() == 0);
        assert(admission::admission_family_complete(top));
        assert(absent.is_none() && boolean.is_none() && numeric.is_none());
        assert(rational.is_none() && fixed.is_none() && forwarded.is_none());
        assert(nonsemantic.is_none());
    }
    if numeric.is_some() {
        assert(top.scalar_occurrences.len() == 1);
        assert(top.scalar_occurrences[0].tag == 2
            || top.scalar_occurrences[0].tag == 34);
        assert(top.returns.len() == 0);
        assert(admission::admission_family_complete(top));
        assert(absent.is_none() && boolean.is_none() && atom.is_none());
        assert(rational.is_none() && fixed.is_none() && forwarded.is_none());
        assert(nonsemantic.is_none());
    }
    if rational.is_some() {
        assert(top.returns.len() == 1);
        assert(top.returns[0].tag == 36);
        assert(top.returns[0].score_handle.is_none());
        assert(top.scalar_occurrences.len() == 0);
        assert(admission::admission_family_complete(top));
        assert(absent.is_none() && boolean.is_none() && atom.is_none());
        assert(numeric.is_none() && fixed.is_none() && forwarded.is_none());
        assert(nonsemantic.is_none());
    }
    if fixed.is_some() {
        assert(top.returns.len() == 1);
        assert(top.returns[0].tag == 37);
        assert(top.returns[0].score_handle.is_none());
        assert(top.scalar_occurrences.len() == 0);
        assert(admission::admission_family_complete(top));
        assert(absent.is_none() && boolean.is_none() && atom.is_none());
        assert(numeric.is_none() && rational.is_none() && forwarded.is_none());
        assert(nonsemantic.is_none());
    }
    if forwarded.is_some() {
        assert(top.returns.len() == 1);
        assert(top.returns[0].score_handle.is_some());
        assert(top.scalar_occurrences.len() == 0);
        assert(admission::admission_family_complete(top));
        assert(absent.is_none() && boolean.is_none() && atom.is_none());
        assert(numeric.is_none() && rational.is_none() && fixed.is_none());
        assert(nonsemantic.is_none());
    }
    if nonsemantic.is_some() {
        assert(!admission::admission_family_complete(top));
        assert(absent.is_none() && boolean.is_none() && atom.is_none());
        assert(numeric.is_none() && rational.is_none() && fixed.is_none());
        assert(forwarded.is_none());
    }
    assert(absent.is_some() || boolean.is_some() || atom.is_some()
        || numeric.is_some() || rational.is_some() || fixed.is_some()
        || forwarded.is_some() || nonsemantic.is_some());
    let empty = BridgeScoreFinishSelection::NoCandidate;
    reveal(bridge_score_finish_selection_group_1);
    let first = bridge_score_finish_selection_group_1(before, empty);
    assert(first == empty);
    reveal(bridge_score_finish_selection_group_2);
    let second = bridge_score_finish_selection_group_2(before, first);
    assert(second == match absent {
        Some(next) => BridgeScoreFinishSelection::Unique(next),
        None => empty,
    });
    reveal(bridge_score_finish_selection_group_3);
    let third = bridge_score_finish_selection_group_3(before, second);
    assert(third == if absent.is_some() {
        second
    } else if boolean.is_some() {
        BridgeScoreFinishSelection::Unique(boolean.unwrap())
    } else if atom.is_some() {
        BridgeScoreFinishSelection::Unique(atom.unwrap())
    } else if numeric.is_some() {
        BridgeScoreFinishSelection::Unique(numeric.unwrap())
    } else if rational.is_some() {
        BridgeScoreFinishSelection::Unique(rational.unwrap())
    } else {
        match fixed {
            Some(next) => BridgeScoreFinishSelection::Unique(next),
            None => empty,
        }
    });
    reveal(bridge_score_finish_selection_group_4);
    let fourth = bridge_score_finish_selection_group_4(before, third);
    assert(fourth == if absent.is_some() || boolean.is_some()
        || atom.is_some() || numeric.is_some() || rational.is_some()
        || fixed.is_some()
    {
        third
    } else {
        match forwarded {
            Some(next) => BridgeScoreFinishSelection::Unique(next),
            None => empty,
        }
    });
    reveal(bridge_score_finish_selection_group_5);
    let fifth = bridge_score_finish_selection_group_5(before, fourth);
    assert(fifth == if absent.is_some() || boolean.is_some()
        || atom.is_some() || numeric.is_some() || rational.is_some()
        || fixed.is_some() || forwarded.is_some()
    {
        fourth
    } else {
        match nonsemantic {
            Some(next) => BridgeScoreFinishSelection::Unique(next),
            None => empty,
        }
    });
    reveal(bridge_score_finish_selected);
    if absent.is_some() {
        assert(bridge_score_finish_selected(before) == absent);
        assert(bridge_same(absent.unwrap(), after));
    } else if boolean.is_some() {
        assert(bridge_score_finish_selected(before) == boolean);
        assert(bridge_same(boolean.unwrap(), after));
    } else if atom.is_some() {
        assert(bridge_score_finish_selected(before) == atom);
        assert(bridge_same(atom.unwrap(), after));
    } else if numeric.is_some() {
        assert(bridge_score_finish_selected(before) == numeric);
        assert(bridge_same(numeric.unwrap(), after));
    } else if rational.is_some() {
        assert(bridge_score_finish_selected(before) == rational);
        assert(bridge_same(rational.unwrap(), after));
    } else if fixed.is_some() {
        assert(bridge_score_finish_selected(before) == fixed);
        assert(bridge_same(fixed.unwrap(), after));
    } else if forwarded.is_some() {
        assert(bridge_score_finish_selected(before) == forwarded);
        assert(bridge_same(forwarded.unwrap(), after));
    } else {
        assert(nonsemantic.is_some());
        assert(bridge_score_finish_selected(before) == nonsemantic);
        assert(bridge_same(nonsemantic.unwrap(), after));
    }
}

pub proof fn bridge_expr_source_step_has_unique_finish(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::Expr,
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
        forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) ==> bridge_same(after, other),
{
    bridge_expr_source_step_matches_selected(before, after);
    assert forall|other: ScoreAdmissionBridge|
        bridge_score_source_step(before, other) implies bridge_same(after, other) by {
        bridge_expr_source_step_matches_selected(before, other);
    }
}

pub open spec fn bridge_other_incomplete_frame(before: ScoreAdmissionBridge) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && !admission::admission_family_complete(before.admission.frames.last())
        && match before.admission.frames.last().family {
            admission::AdmissionFamily::Receive
            | admission::AdmissionFamily::New
            | admission::AdmissionFamily::Expr => true,
            _ => false,
        }
}

pub proof fn bridge_other_incomplete_group_1_empty(before: ScoreAdmissionBridge)
    requires bridge_other_incomplete_frame(before),
    ensures bridge_score_finish_selection_group_1(
        before, BridgeScoreFinishSelection::NoCandidate,
    ) == BridgeScoreFinishSelection::NoCandidate,
{
    let top = before.admission.frames.last();
    assert(top.program_type_id == admission::admission_family_program_id(top.family).unwrap());
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    reveal(bridge_score_finish_selection_group_1);
}

pub proof fn bridge_other_incomplete_group_2_empty(before: ScoreAdmissionBridge)
    requires bridge_other_incomplete_frame(before),
    ensures bridge_score_finish_selection_group_2(
        before, BridgeScoreFinishSelection::NoCandidate,
    ) == BridgeScoreFinishSelection::NoCandidate,
{
    let top = before.admission.frames.last();
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    reveal(bridge_score_finish_selection_group_2);
}

pub proof fn bridge_other_incomplete_group_3_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_other_incomplete_frame(before),
    ensures bridge_score_finish_selection_group_3(before, selection) == selection,
{
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    reveal(bridge_score_finish_selection_group_3);
}

pub proof fn bridge_other_incomplete_group_4_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_other_incomplete_frame(before),
    ensures bridge_score_finish_selection_group_4(before, selection) == selection,
{
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    reveal(bridge_score_finish_selection_group_4);
}

pub proof fn bridge_other_incomplete_group_5_nonsemantic(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_other_incomplete_frame(before),
    ensures bridge_score_finish_selection_group_5(before, selection)
        == bridge_add_score_finish_candidate(
            selection, bridge_nonsemantic_finish_candidate(before),
        ),
{
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    reveal(bridge_score_finish_selection_group_5);
}

pub proof fn bridge_other_incomplete_finish_selected_exact(
    before: ScoreAdmissionBridge,
)
    requires bridge_other_incomplete_frame(before),
    ensures bridge_score_finish_selected(before)
        == bridge_nonsemantic_finish_candidate(before),
{
    bridge_other_incomplete_group_1_empty(before);
    bridge_other_incomplete_group_2_empty(before);
    reveal(bridge_score_finish_selected);
    let empty = BridgeScoreFinishSelection::NoCandidate;
    let first = bridge_score_finish_selection_group_1(before, empty);
    let second = bridge_score_finish_selection_group_2(before, first);
    bridge_other_incomplete_group_3_empty(before, second);
    let third = bridge_score_finish_selection_group_3(before, second);
    bridge_other_incomplete_group_4_empty(before, third);
    let fourth = bridge_score_finish_selection_group_4(before, third);
    bridge_other_incomplete_group_5_nonsemantic(before, fourth);
}

pub open spec fn bridge_nonsemantic_incomplete_frame(before: ScoreAdmissionBridge) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && bridge_nonsemantic_family(before.admission.frames.last().family)
        && !admission::admission_family_complete(before.admission.frames.last())
}

pub proof fn bridge_nonsemantic_incomplete_finish_selected_exact(
    before: ScoreAdmissionBridge,
)
    requires bridge_nonsemantic_incomplete_frame(before),
    ensures
        bridge_score_finish_selected(before)
            == bridge_nonsemantic_finish_candidate(before),
        bridge_nonsemantic_finish_candidate(before).is_some()
            ==> bridge_score_finish_selected(before).is_some(),
{
    let top = before.admission.frames.last();
    match top.family {
        admission::AdmissionFamily::Send
        | admission::AdmissionFamily::Bundle
        | admission::AdmissionFamily::If
        | admission::AdmissionFamily::Match => {
            assert(bridge_primary_component_frame(before));
            bridge_primary_component_finish_selected_exact(before);
            assert(bridge_primary_component_finish(before).is_none());
        },
        admission::AdmissionFamily::Receive
        | admission::AdmissionFamily::New
        | admission::AdmissionFamily::Expr => {
            assert(bridge_other_incomplete_frame(before));
            bridge_other_incomplete_finish_selected_exact(before);
        },
        _ => {},
    }
}

pub proof fn bridge_nonsemantic_incomplete_source_step_matches_selected(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_nonsemantic_incomplete_frame(before),
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
{
    let top = before.admission.frames.last();
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_some());
    bridge_nonsemantic_incomplete_finish_selected_exact(before);
    let (next, _, _) = bridge_nonsemantic_step(before).unwrap();
    assert(bridge_score_finish_selected(before) == Some(next));
    assert(bridge_same(next, after));
}

pub proof fn bridge_nonsemantic_incomplete_source_step_has_unique_finish(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_nonsemantic_incomplete_frame(before),
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
        forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) ==> bridge_same(after, other),
{
    bridge_nonsemantic_incomplete_source_step_matches_selected(before, after);
    assert forall|other: ScoreAdmissionBridge|
        bridge_score_source_step(before, other) implies bridge_same(after, other) by {
        bridge_nonsemantic_incomplete_source_step_matches_selected(before, other);
    }
}

pub open spec fn bridge_no_score_child_frame(before: ScoreAdmissionBridge) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && !admission::admission_family_complete(before.admission.frames.last())
        && match before.admission.frames.last().family {
            admission::AdmissionFamily::ReceiveBindFrame
            | admission::AdmissionFamily::MatchCaseFrame => true,
            _ => false,
        }
}

pub proof fn bridge_no_score_child_group_1_empty(before: ScoreAdmissionBridge)
    requires bridge_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_1(
        before, BridgeScoreFinishSelection::NoCandidate,
    ) == BridgeScoreFinishSelection::NoCandidate,
{
    let top = before.admission.frames.last();
    assert(top.program_type_id == admission::admission_family_program_id(top.family).unwrap());
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    reveal(bridge_score_finish_selection_group_1);
}

pub proof fn bridge_no_score_child_group_2_empty(before: ScoreAdmissionBridge)
    requires bridge_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_2(
        before, BridgeScoreFinishSelection::NoCandidate,
    ) == BridgeScoreFinishSelection::NoCandidate,
{
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    reveal(bridge_score_finish_selection_group_2);
}

pub proof fn bridge_no_score_child_group_3_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_3(before, selection) == selection,
{
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    reveal(bridge_score_finish_selection_group_3);
}

pub proof fn bridge_no_score_child_group_4_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_4(before, selection) == selection,
{
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    reveal(bridge_score_finish_selection_group_4);
}

pub proof fn bridge_no_score_child_group_5_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_5(before, selection) == selection,
{
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    reveal(bridge_score_finish_selection_group_5);
}

pub proof fn bridge_no_score_child_finish_selected_none(before: ScoreAdmissionBridge)
    requires bridge_no_score_child_frame(before),
    ensures bridge_score_finish_selected(before).is_none(),
{
    bridge_no_score_child_group_1_empty(before);
    bridge_no_score_child_group_2_empty(before);
    reveal(bridge_score_finish_selected);
    let empty = BridgeScoreFinishSelection::NoCandidate;
    let first = bridge_score_finish_selection_group_1(before, empty);
    let second = bridge_score_finish_selection_group_2(before, first);
    bridge_no_score_child_group_3_empty(before, second);
    let third = bridge_score_finish_selection_group_3(before, second);
    bridge_no_score_child_group_4_empty(before, third);
    let fourth = bridge_score_finish_selection_group_4(before, third);
    bridge_no_score_child_group_5_empty(before, fourth);
}

pub proof fn bridge_receive_bind_source_absent_selected_none(before: ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::ReceiveBindFrame,
        !admission::admission_returned_par(before.admission.frames.last(), 1),
    ensures bridge_score_finish_selected(before).is_none(),
{
    assert(bridge_no_score_child_frame(before));
    bridge_no_score_child_finish_selected_none(before);
}

pub proof fn bridge_match_case_required_absent_selected_none(before: ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::MatchCaseFrame,
        !admission::admission_returned_par(before.admission.frames.last(), 0)
            || !admission::admission_returned_par(before.admission.frames.last(), 1),
    ensures bridge_score_finish_selected(before).is_none(),
{
    assert(bridge_no_score_child_frame(before));
    bridge_no_score_child_finish_selected_none(before);
}

pub proof fn bridge_receive_bind_absent_source_has_derived_no_score_return(
    before: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 1,
        admission::admission_frame_wf(before.admission.frames[before.admission.frames.len() - 2]),
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames[before.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Receive,
        before.admission.frames.last().family
            == admission::AdmissionFamily::ReceiveBindFrame,
        !admission::admission_returned_par(before.admission.frames.last(), 1),
    ensures
        bridge_score_finish_selected(before).is_none(),
        admission::concrete_derived_summary_with_arena(
            before.admission.frames[before.admission.frames.len() - 2],
            before.admission.frames.last(), before.admission.arena,
        ) == Some(admission::AdmissionReturnedKind::ReceiveBind {
            source_present: false,
        }),
{
    bridge_receive_bind_source_absent_selected_none(before);
    let parent = before.admission.frames[before.admission.frames.len() - 2];
    let child = before.admission.frames.last();
    assert(parent.program_type_id == 19);
    assert(child.program_type_id == 16);
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::ReceiveBind {
            source_present: false,
        }));
}

pub proof fn bridge_match_case_missing_required_has_derived_no_score_return(
    before: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 1,
        admission::admission_frame_wf(before.admission.frames[before.admission.frames.len() - 2]),
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames[before.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Match,
        before.admission.frames.last().family
            == admission::AdmissionFamily::MatchCaseFrame,
        !admission::admission_returned_par(before.admission.frames.last(), 0)
            || !admission::admission_returned_par(before.admission.frames.last(), 1),
    ensures
        bridge_score_finish_selected(before).is_none(),
        admission::concrete_derived_summary_with_arena(
            before.admission.frames[before.admission.frames.len() - 2],
            before.admission.frames.last(), before.admission.arena,
        ) == Some(admission::AdmissionReturnedKind::MatchCase {
            required_present: false,
            canonical_fixed_point: !admission::admission_returned_default_par(
                before.admission.frames.last(), 3,
            ),
        }),
{
    bridge_match_case_required_absent_selected_none(before);
    let parent = before.admission.frames[before.admission.frames.len() - 2];
    let child = before.admission.frames.last();
    assert(parent.program_type_id == 22);
    assert(child.program_type_id == 21);
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::MatchCase {
            required_present: false,
            canonical_fixed_point: !admission::admission_returned_default_par(child, 3),
        }));
}

pub proof fn bridge_unforgeable_unit_child_has_derived_no_score_return(
    before: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 1,
        admission::admission_frame_wf(before.admission.frames[before.admission.frames.len() - 2]),
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames[before.admission.frames.len() - 2].family
            == admission::AdmissionFamily::GUnforgeable,
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().program_type_id == 64,
    ensures
        bridge_score_finish_selected(before).is_none(),
        admission::concrete_derived_summary_with_arena(
            before.admission.frames[before.admission.frames.len() - 2],
            before.admission.frames.last(), before.admission.arena,
        ) == Some(admission::AdmissionReturnedKind::Unit),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
    let parent = before.admission.frames[before.admission.frames.len() - 2];
    let child = before.admission.frames.last();
    assert(parent.program_type_id == 60);
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::Unit));
}

pub proof fn bridge_connective_varref_child_has_derived_no_score_return(
    before: ScoreAdmissionBridge, index: i32, depth: i32,
)
    requires
        before.admission.frames.len() > 1,
        admission::admission_frame_wf(before.admission.frames[before.admission.frames.len() - 2]),
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames[before.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Connective,
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().program_type_id == 56,
        admission::admission_var_ref_fields(before.admission.frames.last())
            == Some((index, depth)),
    ensures
        bridge_score_finish_selected(before).is_none(),
        admission::concrete_derived_summary_with_arena(
            before.admission.frames[before.admission.frames.len() - 2],
            before.admission.frames.last(), before.admission.arena,
        ) == Some(admission::AdmissionReturnedKind::VarRef { index, depth }),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
    let parent = before.admission.frames[before.admission.frames.len() - 2];
    let child = before.admission.frames.last();
    assert(parent.program_type_id == 55);
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::VarRef { index, depth }));
}

pub proof fn bridge_connective_body_child_has_derived_no_score_return(
    before: ScoreAdmissionBridge, par_handles: Seq<usize>,
)
    requires
        before.admission.frames.len() > 1,
        admission::admission_frame_wf(before.admission.frames[before.admission.frames.len() - 2]),
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames[before.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Connective,
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().program_type_id == 57,
        admission::admission_connective_body_handles(before.admission.frames.last())
            == Some(par_handles),
    ensures
        bridge_score_finish_selected(before).is_none(),
        admission::concrete_derived_summary_with_arena(
            before.admission.frames[before.admission.frames.len() - 2],
            before.admission.frames.last(), before.admission.arena,
        ) == Some(admission::AdmissionReturnedKind::ConnectiveBody { par_handles }),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
    let parent = before.admission.frames[before.admission.frames.len() - 2];
    let child = before.admission.frames.last();
    assert(parent.program_type_id == 55);
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::ConnectiveBody { par_handles }));
}

pub proof fn bridge_unforgeable_bytes_child_has_derived_no_score_return(
    before: ScoreAdmissionBridge, value: Seq<u8>,
)
    requires
        before.admission.frames.len() > 1,
        admission::admission_frame_wf(before.admission.frames[before.admission.frames.len() - 2]),
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames[before.admission.frames.len() - 2].family
            == admission::AdmissionFamily::GUnforgeable,
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().program_type_id == 61
            || before.admission.frames.last().program_type_id == 62
            || before.admission.frames.last().program_type_id == 63,
        admission::admission_bytes_leaf_payload(before.admission.frames.last())
            == Some(value),
    ensures
        bridge_score_finish_selected(before).is_none(),
        admission::concrete_derived_summary_with_arena(
            before.admission.frames[before.admission.frames.len() - 2],
            before.admission.frames.last(), before.admission.arena,
        ) == Some(admission::AdmissionReturnedKind::Bytes { value }),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
    let parent = before.admission.frames[before.admission.frames.len() - 2];
    let child = before.admission.frames.last();
    assert(parent.program_type_id == 60);
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::Bytes { value }));
}

pub proof fn bridge_big_rational_child_has_derived_no_score_return(
    before: ScoreAdmissionBridge, numerator: Seq<u8>, denominator: Seq<u8>,
)
    requires
        before.admission.frames.len() > 1,
        admission::admission_frame_wf(before.admission.frames[before.admission.frames.len() - 2]),
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames[before.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Expr,
        before.admission.frames[before.admission.frames.len() - 2].waiting_tag == Some(36),
        before.admission.frames.last().family
            == admission::AdmissionFamily::BigRationalFrame,
        admission::admission_big_rational_fields(before.admission.frames.last())
            == Some((numerator, denominator)),
    ensures
        bridge_score_finish_selected(before).is_none(),
        admission::concrete_derived_summary_with_arena(
            before.admission.frames[before.admission.frames.len() - 2],
            before.admission.frames.last(), before.admission.arena,
        ) == Some(admission::AdmissionReturnedKind::BigRational {
            numerator, denominator,
        }),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
    let parent = before.admission.frames[before.admission.frames.len() - 2];
    let child = before.admission.frames.last();
    assert(parent.program_type_id == 24);
    assert(child.program_type_id == 25);
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::BigRational {
            numerator, denominator,
        }));
}

pub proof fn bridge_fixed_point_child_has_derived_no_score_return(
    before: ScoreAdmissionBridge, unscaled: Seq<u8>, scale: u32,
)
    requires
        before.admission.frames.len() > 1,
        admission::admission_frame_wf(before.admission.frames[before.admission.frames.len() - 2]),
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames[before.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Expr,
        before.admission.frames[before.admission.frames.len() - 2].waiting_tag == Some(37),
        before.admission.frames.last().family
            == admission::AdmissionFamily::FixedPointFrame,
        admission::admission_fixed_point_fields(before.admission.frames.last())
            == Some((unscaled, scale)),
    ensures
        bridge_score_finish_selected(before).is_none(),
        admission::concrete_derived_summary_with_arena(
            before.admission.frames[before.admission.frames.len() - 2],
            before.admission.frames.last(), before.admission.arena,
        ) == Some(admission::AdmissionReturnedKind::FixedPoint { unscaled, scale }),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
    let parent = before.admission.frames[before.admission.frames.len() - 2];
    let child = before.admission.frames.last();
    assert(parent.program_type_id == 24);
    assert(child.program_type_id == 26);
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::FixedPoint { unscaled, scale }));
}

pub proof fn bridge_evar_absent_child_has_derived_no_score_return(
    before: ScoreAdmissionBridge,
)
    requires
        before.admission.frames.len() > 1,
        admission::admission_frame_wf(before.admission.frames[before.admission.frames.len() - 2]),
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames[before.admission.frames.len() - 2].family
            == admission::AdmissionFamily::Expr,
        before.admission.frames[before.admission.frames.len() - 2].waiting_tag == Some(19),
        before.admission.frames.last().family == admission::AdmissionFamily::EVarFrame,
        !admission::admission_returned_var(before.admission.frames.last(), 0),
    ensures
        bridge_score_finish_selected(before).is_none(),
        admission::concrete_derived_summary_with_arena(
            before.admission.frames[before.admission.frames.len() - 2],
            before.admission.frames.last(), before.admission.arena,
        ) == Some(admission::AdmissionReturnedKind::ExprPayload {
            score_present: false,
            canonical_fixed_point: true,
            canonical_path_stable: false,
        }),
{
    bridge_evar_missing_var_finish_selected_none(before);
    let parent = before.admission.frames[before.admission.frames.len() - 2];
    let child = before.admission.frames.last();
    assert(parent.program_type_id == 24);
    assert(child.program_type_id == 35);
    assert(admission::concrete_derived_summary(parent, child)
        == Some(admission::AdmissionReturnedKind::ExprPayload {
            score_present: false,
            canonical_fixed_point: true,
            canonical_path_stable: false,
        }));
}

pub proof fn bridge_par_handles_for_slot_absent(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>, slot: usize, tag: u32,
)
    requires forall|i: int| 0 <= i < summaries.len() ==> summaries[i].slot != slot,
    ensures bridge_par_handles_for_slot(summaries, arena, slot, tag)
        == Some(Seq::empty()),
    decreases summaries.len(),
{
    if summaries.len() > 0 {
        let rest = summaries.drop_first();
        assert forall|i: int| 0 <= i < rest.len()
            implies rest[i].slot != slot by {
            assert(rest[i] == summaries[i + 1]);
        }
        bridge_par_handles_for_slot_absent(rest, arena, slot, tag);
    }
}

pub open spec fn bridge_receive_complete_frame(before: ScoreAdmissionBridge) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && before.admission.frames.last().family == admission::AdmissionFamily::Receive
        && admission::admission_family_complete(before.admission.frames.last())
}

pub open spec fn bridge_receive_present_finish_candidate(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    match bridge_receive_present_condition_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _)) => Some(next),
    }
}

pub open spec fn bridge_receive_absent_finish_candidate(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    match bridge_receive_absent_condition_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _, _)) => Some(next),
    }
}

pub proof fn bridge_receive_complete_groups_1_to_4_empty(
    before: ScoreAdmissionBridge,
)
    requires bridge_receive_complete_frame(before),
    ensures
        bridge_score_finish_selection_group_1(
            before, BridgeScoreFinishSelection::NoCandidate,
        ) == BridgeScoreFinishSelection::NoCandidate,
        bridge_score_finish_selection_group_2(
            before, BridgeScoreFinishSelection::NoCandidate,
        ) == BridgeScoreFinishSelection::NoCandidate,
        forall|selection: BridgeScoreFinishSelection|
            bridge_score_finish_selection_group_3(before, selection) == selection,
        forall|selection: BridgeScoreFinishSelection|
            bridge_score_finish_selection_group_4(before, selection) == selection,
{
    let top = before.admission.frames.last();
    assert(top.program_type_id == admission::admission_family_program_id(top.family).unwrap());
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    reveal(bridge_score_finish_selection_group_1);

    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    reveal(bridge_score_finish_selection_group_2);

    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    reveal(bridge_score_finish_selection_group_3);

    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    reveal(bridge_score_finish_selection_group_4);
}

pub proof fn bridge_receive_absent_condition_excludes_present(
    before: ScoreAdmissionBridge,
)
    requires
        bridge_receive_complete_frame(before),
        !before.admission.frames.last().seen.contains(7),
    ensures bridge_receive_present_finish_candidate(before).is_none(),
{
    let top = before.admission.frames.last();
    assert forall|i: int| 0 <= i < top.returns.len()
        implies top.returns[i].slot != 7 by {
        assert(top.seen.contains(top.returns[i].slot as int));
    }
    bridge_par_handles_for_slot_absent(top.returns, before.admission.arena, 7, 8);
    assert(bridge_receive_present_condition_parts(top, before.admission.arena).is_none());
}

pub proof fn bridge_receive_complete_finish_selected_exact(
    before: ScoreAdmissionBridge,
)
    requires bridge_receive_complete_frame(before),
    ensures bridge_score_finish_selected(before)
        == if before.admission.frames.last().seen.contains(7) {
            bridge_receive_present_finish_candidate(before)
        } else {
            bridge_receive_absent_finish_candidate(before)
        },
{
    bridge_receive_complete_groups_1_to_4_empty(before);
    let top = before.admission.frames.last();
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    if top.seen.contains(7) {
        assert(bridge_receive_absent_finish_candidate(before).is_none());
    } else {
        bridge_receive_absent_condition_excludes_present(before);
    }
    reveal(bridge_score_finish_selection_group_5);
    reveal(bridge_score_finish_selected);
}

pub proof fn bridge_receive_complete_source_step_has_unique_finish(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_receive_complete_frame(before),
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
        forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) ==> bridge_same(after, other),
{
    let top = before.admission.frames.last();
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    if top.seen.contains(7) {
        assert(bridge_receive_absent_finish_candidate(before).is_none());
    } else {
        bridge_receive_absent_condition_excludes_present(before);
    }
    bridge_receive_complete_finish_selected_exact(before);
    assert forall|other: ScoreAdmissionBridge|
        bridge_score_source_step(before, other)
            implies if top.seen.contains(7) {
                match bridge_receive_present_finish_candidate(before) {
                    Some(next) => bridge_same(next, other),
                    None => false,
                }
            } else {
                match bridge_receive_absent_finish_candidate(before) {
                    Some(next) => bridge_same(next, other),
                    None => false,
                }
            } by {
        assert(bridge_score_source_step(before, other));
    }
    if top.seen.contains(7) {
        assert(bridge_receive_present_finish_candidate(before).is_some());
        let next = bridge_receive_present_finish_candidate(before).unwrap();
        assert(bridge_score_finish_selected(before) == Some(next));
        assert(bridge_same(next, after));
        assert forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) implies bridge_same(after, other) by {
            assert(bridge_same(next, other));
        }
    } else {
        assert(bridge_receive_absent_finish_candidate(before).is_some());
        let next = bridge_receive_absent_finish_candidate(before).unwrap();
        assert(bridge_score_finish_selected(before) == Some(next));
        assert(bridge_same(next, after));
        assert forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) implies bridge_same(after, other) by {
            assert(bridge_same(next, other));
        }
    }
}

pub open spec fn bridge_new_complete_frame(before: ScoreAdmissionBridge) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && before.admission.frames.last().family == admission::AdmissionFamily::New
        && admission::admission_family_complete(before.admission.frames.last())
}

pub open spec fn bridge_new_empty_finish_candidate(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    match bridge_new_empty_collections_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

pub open spec fn bridge_new_one_injection_finish_candidate(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    match bridge_new_one_injection_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _)) => Some(next),
    }
}

pub open spec fn bridge_new_one_absent_injection_finish_candidate(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    match bridge_new_one_absent_injection_step(before) {
        None => None,
        Some((next, _, _, _, _, _, _, _)) => Some(next),
    }
}

pub open spec fn bridge_new_one_uri_finish_candidate(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    match bridge_new_one_uri_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

pub open spec fn bridge_new_collections_finish_candidate(
    before: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    match bridge_new_collections_step(before) {
        None => None,
        Some((next, _, _, _, _, _)) => Some(next),
    }
}

pub proof fn bridge_new_complete_groups_1_and_2_empty(before: ScoreAdmissionBridge)
    requires bridge_new_complete_frame(before),
    ensures
        bridge_score_finish_selection_group_1(
            before, BridgeScoreFinishSelection::NoCandidate,
        ) == BridgeScoreFinishSelection::NoCandidate,
        bridge_score_finish_selection_group_2(
            before, BridgeScoreFinishSelection::NoCandidate,
        ) == BridgeScoreFinishSelection::NoCandidate,
{
    let top = before.admission.frames.last();
    assert(top.program_type_id == admission::admission_family_program_id(top.family).unwrap());
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    reveal(bridge_score_finish_selection_group_1);

    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    reveal(bridge_score_finish_selection_group_2);
}

pub proof fn bridge_new_complete_group_5_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_new_complete_frame(before),
    ensures bridge_score_finish_selection_group_5(before, selection) == selection,
{
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    reveal(bridge_score_finish_selection_group_5);
}

pub proof fn bridge_string_bytes_for_slot_absent(
    occurrences: Seq<admission::AdmissionScalarOccurrence>,
    slot: usize, tag: u32, scalar_kind: u8,
)
    requires forall|i: int| 0 <= i < occurrences.len()
        ==> occurrences[i].slot != slot,
    ensures bridge_string_bytes_for_slot(occurrences, slot, tag, scalar_kind)
        == Some(Seq::empty()),
    decreases occurrences.len(),
{
    if occurrences.len() > 0 {
        let rest = occurrences.drop_first();
        assert forall|i: int| 0 <= i < rest.len()
            implies rest[i].slot != slot by {
            assert(rest[i] == occurrences[i + 1]);
        }
        bridge_string_bytes_for_slot_absent(rest, slot, tag, scalar_kind);
    }
}

pub proof fn bridge_new_no_uris_have_empty_uri_summary(
    before: ScoreAdmissionBridge,
)
    requires
        bridge_new_complete_frame(before),
        !before.admission.frames.last().seen.contains(2),
    ensures bridge_string_bytes_for_slot(
        before.admission.frames.last().scalar_occurrences, 2, 3, 15u8,
    ) == Some(Seq::empty()),
{
    let top = before.admission.frames.last();
    assert forall|i: int| 0 <= i < top.scalar_occurrences.len()
        implies top.scalar_occurrences[i].slot != 2 by {
        assert(top.scalar_slots.contains(top.scalar_occurrences[i].slot as int));
        assert(top.seen.contains(top.scalar_occurrences[i].slot as int));
    }
    bridge_string_bytes_for_slot_absent(top.scalar_occurrences, 2, 3, 15u8);
}

pub proof fn bridge_new_specialized_parts_disjoint(
    before: ScoreAdmissionBridge,
)
    requires bridge_new_complete_frame(before),
    ensures
        bridge_new_empty_finish_candidate(before).is_some() ==> {
            &&& bridge_new_one_injection_finish_candidate(before).is_none()
            &&& bridge_new_one_absent_injection_finish_candidate(before).is_none()
            &&& bridge_new_one_uri_finish_candidate(before).is_none()
            &&& bridge_new_collections_finish_candidate(before).is_none()
        },
        bridge_new_one_injection_finish_candidate(before).is_some() ==> {
            &&& bridge_new_one_absent_injection_finish_candidate(before).is_none()
            &&& bridge_new_one_uri_finish_candidate(before).is_none()
            &&& bridge_new_collections_finish_candidate(before).is_none()
        },
        bridge_new_one_absent_injection_finish_candidate(before).is_some() ==> {
            &&& bridge_new_one_uri_finish_candidate(before).is_none()
            &&& bridge_new_collections_finish_candidate(before).is_none()
        },
        bridge_new_one_uri_finish_candidate(before).is_some()
            ==> bridge_new_collections_finish_candidate(before).is_none(),
{
    let top = before.admission.frames.last();
    let arena = before.admission.arena;
    if bridge_new_empty_finish_candidate(before).is_some() {
        assert(bridge_new_empty_collections_parts(top, arena).is_some());
        assert(!top.seen.contains(2));
        assert(!top.seen.contains(3));
        assert(top.returns.len() == 1);
        bridge_new_no_uris_have_empty_uri_summary(before);
        assert(bridge_new_one_injection_parts(top, arena).is_none());
        assert(bridge_new_one_absent_injection_parts(top, arena).is_none());
        assert(bridge_new_one_uri_parts(top, arena).is_none());
        assert(bridge_new_collections_parts(top, arena).is_none());
    }
    if bridge_new_one_injection_finish_candidate(before).is_some() {
        assert(bridge_new_one_injection_parts(top, arena).is_some());
        assert(!top.seen.contains(2));
        assert(top.returns.len() == 2);
        bridge_new_no_uris_have_empty_uri_summary(before);
        assert(bridge_new_one_absent_injection_parts(top, arena).is_none());
        assert(bridge_new_one_uri_parts(top, arena).is_none());
        assert(bridge_new_collections_parts(top, arena).is_none());
    }
    if bridge_new_one_absent_injection_finish_candidate(before).is_some() {
        assert(bridge_new_one_absent_injection_parts(top, arena).is_some());
        assert(!top.seen.contains(2));
        assert(top.returns.len() == 2);
        bridge_new_no_uris_have_empty_uri_summary(before);
        assert(bridge_new_one_uri_parts(top, arena).is_none());
        assert(bridge_new_collections_parts(top, arena).is_none());
    }
    if bridge_new_one_uri_finish_candidate(before).is_some() {
        assert(bridge_new_one_uri_parts(top, arena).is_some());
        assert(top.seen.contains(2));
        assert(!top.seen.contains(3));
        assert(top.returns.len() == 1);
        assert(bridge_new_collections_parts(top, arena).is_none());
    }
}

pub proof fn bridge_new_complete_finish_selected_exact(
    before: ScoreAdmissionBridge,
)
    requires bridge_new_complete_frame(before),
    ensures bridge_score_finish_selected(before)
        == if bridge_new_empty_finish_candidate(before).is_some() {
            bridge_new_empty_finish_candidate(before)
        } else if bridge_new_one_injection_finish_candidate(before).is_some() {
            bridge_new_one_injection_finish_candidate(before)
        } else if bridge_new_one_absent_injection_finish_candidate(before).is_some() {
            bridge_new_one_absent_injection_finish_candidate(before)
        } else if bridge_new_one_uri_finish_candidate(before).is_some() {
            bridge_new_one_uri_finish_candidate(before)
        } else {
            bridge_new_collections_finish_candidate(before)
        },
{
    bridge_new_complete_groups_1_and_2_empty(before);
    bridge_new_specialized_parts_disjoint(before);
    let e = bridge_new_empty_finish_candidate(before);
    let i = bridge_new_one_injection_finish_candidate(before);
    let a = bridge_new_one_absent_injection_finish_candidate(before);
    let u = bridge_new_one_uri_finish_candidate(before);
    let g = bridge_new_collections_finish_candidate(before);
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    reveal(bridge_score_finish_selection_group_3);
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    reveal(bridge_score_finish_selection_group_4);
    reveal(bridge_score_finish_selected);
    let empty = BridgeScoreFinishSelection::NoCandidate;
    let first = bridge_score_finish_selection_group_1(before, empty);
    let second = bridge_score_finish_selection_group_2(before, first);
    let third = bridge_score_finish_selection_group_3(before, second);
    let fourth = bridge_score_finish_selection_group_4(before, third);
    bridge_new_complete_group_5_empty(before, fourth);
    if e.is_some() {
        assert(i.is_none() && a.is_none() && u.is_none() && g.is_none());
    } else if i.is_some() {
        assert(a.is_none() && u.is_none() && g.is_none());
    } else if a.is_some() {
        assert(u.is_none() && g.is_none());
    } else if u.is_some() {
        assert(g.is_none());
    }
}

pub proof fn bridge_new_complete_source_step_matches_selected(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_new_complete_frame(before),
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
{
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    assert(bridge_expr_structural_step(before).is_none());
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    assert(bridge_emethod_step(before).is_none());
    assert(bridge_emap_finish_step(before).is_none());
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    let e = bridge_new_empty_finish_candidate(before);
    let i = bridge_new_one_injection_finish_candidate(before);
    let a = bridge_new_one_absent_injection_finish_candidate(before);
    let u = bridge_new_one_uri_finish_candidate(before);
    let g = bridge_new_collections_finish_candidate(before);
    bridge_new_specialized_parts_disjoint(before);
    bridge_new_complete_finish_selected_exact(before);
    assert(e.is_some() || i.is_some() || a.is_some() || u.is_some() || g.is_some());
    if e.is_some() {
        assert(bridge_same(e.unwrap(), after));
    } else if i.is_some() {
        assert(bridge_same(i.unwrap(), after));
    } else if a.is_some() {
        assert(bridge_same(a.unwrap(), after));
    } else if u.is_some() {
        assert(bridge_same(u.unwrap(), after));
    } else {
        assert(g.is_some());
        assert(bridge_same(g.unwrap(), after));
    }
}

pub proof fn bridge_new_complete_source_step_has_unique_finish(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_new_complete_frame(before),
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
        forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) ==> bridge_same(after, other),
{
    bridge_new_complete_source_step_matches_selected(before, after);
    assert forall|other: ScoreAdmissionBridge|
        bridge_score_source_step(before, other) implies bridge_same(after, other) by {
        bridge_new_complete_source_step_matches_selected(before, other);
    }
}

pub open spec fn bridge_structural_no_score_child_frame(
    before: ScoreAdmissionBridge,
) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && match before.admission.frames.last().family {
            admission::AdmissionFamily::BigRationalFrame
            | admission::AdmissionFamily::FixedPointFrame => true,
            admission::AdmissionFamily::EVarFrame =>
                !admission::admission_returned_var(before.admission.frames.last(), 0),
            admission::AdmissionFamily::Other => match before.admission.frames.last().program_type_id {
                3 | 7 | 14 | 17 | 34 | 56 | 57 | 61 | 62 | 63 | 64 => true,
                30 => match bridge_emap_finish_source(before) {
                    Some(source) => !source.complete,
                    None => false,
                },
                33 => !admission::admission_returned_par(before.admission.frames.last(), 1),
                program_type_id => match bridge_expr_structural_constructor(program_type_id) {
                    Some((_, binary)) =>
                        !admission::admission_returned_par(before.admission.frames.last(), 0)
                        || binary && !admission::admission_returned_par(
                            before.admission.frames.last(), 1,
                        ),
                    None => false,
                },
            },
            _ => false,
        }
}

pub proof fn bridge_par_handles_for_slot_without_par(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>, slot: usize, tag: u32,
)
    requires forall|i: int| 0 <= i < summaries.len()
        ==> summaries[i].slot != slot || !admission::admission_is_par(summaries[i].kind),
    ensures match bridge_par_handles_for_slot(summaries, arena, slot, tag) {
        Some(handles) => handles.len() == 0,
        None => true,
    },
    decreases summaries.len(),
{
    if summaries.len() > 0 {
        let rest = summaries.drop_first();
        assert forall|i: int| 0 <= i < rest.len()
            implies rest[i].slot != slot
                || !admission::admission_is_par(rest[i].kind) by {
            assert(rest[i] == summaries[i + 1]);
        }
        bridge_par_handles_for_slot_without_par(rest, arena, slot, tag);
        let first = summaries[0];
        if first.slot == slot {
            assert(!admission::admission_is_par(first.kind));
            assert(bridge_par_return_handle(first, arena, slot, tag).is_none());
        }
    }
}

pub proof fn bridge_expr_structural_missing_par_step_none(before: ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().frame_kind == 1,
        match bridge_expr_structural_constructor(before.admission.frames.last().program_type_id) {
            Some((_, binary)) =>
                !admission::admission_returned_par(before.admission.frames.last(), 0)
                || binary && !admission::admission_returned_par(
                    before.admission.frames.last(), 1,
                ),
            None => false,
        },
    ensures bridge_expr_structural_step(before).is_none(),
{
    let top = before.admission.frames.last();
    if let Some((_, binary)) = bridge_expr_structural_constructor(top.program_type_id) {
        if !admission::admission_returned_par(top, 0) {
            assert forall|i: int| 0 <= i < top.returns.len()
                implies top.returns[i].slot != 0
                    || !admission::admission_is_par(top.returns[i].kind) by {}
            bridge_par_handles_for_slot_without_par(top.returns, before.admission.arena, 0, 1);
        } else {
            assert(binary);
            assert(!admission::admission_returned_par(top, 1));
            assert forall|i: int| 0 <= i < top.returns.len()
                implies top.returns[i].slot != 1
                    || !admission::admission_is_par(top.returns[i].kind) by {}
            bridge_par_handles_for_slot_without_par(top.returns, before.admission.arena, 1, 2);
        }
    } else {
        assert(false);
    }
    assert(bridge_expr_structural_parts(top, before.admission.arena).is_none());
}

pub proof fn bridge_emethod_missing_target_step_none(before: ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().program_type_id == 33,
        !admission::admission_returned_par(before.admission.frames.last(), 1),
    ensures bridge_emethod_step(before).is_none(),
{
    let top = before.admission.frames.last();
    assert forall|i: int| 0 <= i < top.returns.len()
        implies top.returns[i].slot != 1
            || !admission::admission_is_par(top.returns[i].kind) by {}
    bridge_par_handles_for_slot_without_par(top.returns, before.admission.arena, 1, 2);
    assert(bridge_emethod_parts(top, before.admission.arena).is_none());
}

pub proof fn bridge_structural_no_score_group_1_empty(before: ScoreAdmissionBridge)
    requires bridge_structural_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_1(
        before, BridgeScoreFinishSelection::NoCandidate,
    ) == BridgeScoreFinishSelection::NoCandidate,
{
    let top = before.admission.frames.last();
    match top.family {
        admission::AdmissionFamily::BigRationalFrame
        | admission::AdmissionFamily::FixedPointFrame
        | admission::AdmissionFamily::EVarFrame => {
            assert(top.program_type_id == admission::admission_family_program_id(top.family).unwrap());
        },
        _ => {},
    }
    assert(bridge_raw_par_step(before).is_none());
    assert(bridge_cost_atom_absent_step(before).is_none());
    assert(bridge_cost_atom_bytes_step(before).is_none());
    assert(bridge_cost_bound_step(before).is_none());
    assert(bridge_cost_atom_bound_forward_step(before).is_none());
    assert(bridge_cost_signature_step(before).is_none());
    assert(bridge_cost_region_step(before).is_none());
    assert(bridge_cost_token_stack_step(before).is_none());
    reveal(bridge_score_finish_selection_group_1);
}

pub proof fn bridge_structural_no_score_group_2_empty(before: ScoreAdmissionBridge)
    requires bridge_structural_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_2(
        before, BridgeScoreFinishSelection::NoCandidate,
    ) == BridgeScoreFinishSelection::NoCandidate,
{
    assert(bridge_bundle_step(before).is_none());
    assert(bridge_if_step(before).is_none());
    assert(bridge_send_step(before).is_none());
    assert(bridge_match_step(before).is_none());
    assert(bridge_absent_specialized_constructor_step(before).is_none());
    assert(bridge_absent_connective_step(before).is_none());
    assert(bridge_receive_bind_step(before).is_none());
    assert(bridge_match_case_step(before).is_none());
    reveal(bridge_score_finish_selection_group_2);
}

pub proof fn bridge_structural_no_score_group_3_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_structural_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_3(before, selection) == selection,
{
    assert(bridge_bool_oneof_step(before).is_none());
    assert(bridge_expr_atom_step(before).is_none());
    assert(bridge_expr_numeric_step(before).is_none());
    assert(bridge_expr_big_rational_step(before).is_none());
    assert(bridge_expr_fixed_point_step(before).is_none());
    assert(bridge_sys_auth_token_step(before).is_none());
    assert(bridge_new_empty_collections_step(before).is_none());
    assert(bridge_new_one_injection_step(before).is_none());
    reveal(bridge_score_finish_selection_group_3);
}

pub proof fn bridge_structural_no_score_group_4_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_structural_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_4(before, selection) == selection,
{
    assert(bridge_new_one_absent_injection_step(before).is_none());
    assert(bridge_new_one_uri_step(before).is_none());
    assert(bridge_new_collections_step(before).is_none());
    assert(bridge_expr_payload_forward_step(before).is_none());
    if before.admission.frames.last().family == admission::AdmissionFamily::Other {
        if before.admission.frames.last().frame_kind == 1
            && bridge_expr_structural_constructor(
            before.admission.frames.last().program_type_id,
        ).is_some() {
            bridge_expr_structural_missing_par_step_none(before);
        }
    }
    assert(bridge_expr_structural_step(before).is_none());
    if before.admission.frames.last().family == admission::AdmissionFamily::EVarFrame {
        bridge_evar_missing_var_step_none(before);
    }
    assert(bridge_evar_step(before).is_none());
    assert(bridge_ordered_sequence_step(before).is_none());
    if before.admission.frames.last().family == admission::AdmissionFamily::Other
        && before.admission.frames.last().frame_kind == 1
        && before.admission.frames.last().program_type_id == 33 {
        bridge_emethod_missing_target_step_none(before);
    }
    assert(bridge_emethod_step(before).is_none());
    if before.admission.frames.last().family == admission::AdmissionFamily::Other
        && before.admission.frames.last().program_type_id == 30
    {
        bridge_emap_incomplete_finish_has_no_score_step(before);
    }
    assert(bridge_emap_finish_step(before).is_none());
    reveal(bridge_score_finish_selection_group_4);
}

pub proof fn bridge_structural_no_score_group_5_empty(
    before: ScoreAdmissionBridge, selection: BridgeScoreFinishSelection,
)
    requires bridge_structural_no_score_child_frame(before),
    ensures bridge_score_finish_selection_group_5(before, selection) == selection,
{
    assert(bridge_connective_not_step(before).is_none());
    assert(bridge_connective_varref_step(before).is_none());
    assert(bridge_connective_collection_step(before).is_none());
    assert(bridge_unforgeable_bytes_step(before).is_none());
    assert(bridge_receive_present_condition_step(before).is_none());
    assert(bridge_receive_absent_condition_step(before).is_none());
    assert(bridge_nonsemantic_step(before).is_none());
    reveal(bridge_score_finish_selection_group_5);
}

pub proof fn bridge_structural_no_score_child_finish_selected_none(
    before: ScoreAdmissionBridge,
)
    requires bridge_structural_no_score_child_frame(before),
    ensures bridge_score_finish_selected(before).is_none(),
{
    bridge_structural_no_score_group_1_empty(before);
    bridge_structural_no_score_group_2_empty(before);
    reveal(bridge_score_finish_selected);
    let empty = BridgeScoreFinishSelection::NoCandidate;
    let first = bridge_score_finish_selection_group_1(before, empty);
    let second = bridge_score_finish_selection_group_2(before, first);
    bridge_structural_no_score_group_3_empty(before, second);
    let third = bridge_score_finish_selection_group_3(before, second);
    bridge_structural_no_score_group_4_empty(before, third);
    let fourth = bridge_score_finish_selection_group_4(before, third);
    bridge_structural_no_score_group_5_empty(before, fourth);
}

pub proof fn bridge_evar_missing_var_step_none(before: ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::EVarFrame,
        !admission::admission_returned_var(before.admission.frames.last(), 0),
    ensures bridge_evar_step(before).is_none(),
{
    let top = before.admission.frames.last();
    if let Some(_) = bridge_evar_parts(top, before.admission.arena) {
        assert(top.returns.len() == 1);
        assert(top.returns[0].slot == 0);
        assert(top.returns[0].kind == admission::AdmissionReturnedKind::VarScore);
        assert(admission::admission_returned_var(top, 0));
    }
}

pub proof fn bridge_evar_missing_var_finish_selected_none(before: ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().family == admission::AdmissionFamily::EVarFrame,
        !admission::admission_returned_var(before.admission.frames.last(), 0),
    ensures bridge_score_finish_selected(before).is_none(),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
}

pub proof fn bridge_other_summary_child_finish_selected_none(before: ScoreAdmissionBridge)
    requires
        before.admission.frames.len() > 0,
        admission::admission_frame_wf(before.admission.frames.last()),
        before.admission.frames.last().frame_kind == 1,
        before.admission.frames.last().family == admission::AdmissionFamily::Other,
        before.admission.frames.last().score_handle.is_none(),
        match before.admission.frames.last().program_type_id {
            3 | 14 | 17 | 34 => true,
            _ => false,
        },
    ensures bridge_score_finish_selected(before).is_none(),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
}

pub struct BridgeEMapPairImage {
    pub key: Option<admission::AdmissionParReturnImage>,
    pub value_par: Option<admission::AdmissionParReturnImage>,
    pub ghost body_bytes: Seq<u8>,
}

pub open spec fn bridge_emap_pair_from_return(
    returned: admission::AdmissionReturnedSummary,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<BridgeEMapPairImage> {
    if returned.slot != 0 || returned.tag != 1
        || returned.score_handle.is_some() || returned.score_origin_body_id.is_some()
    {
        None
    } else {
        match returned.kind {
            admission::AdmissionReturnedKind::KeyValuePair { key, value_par } => {
                if admission::admission_key_value_pair_bindings(key, value_par, arena) {
                    Some(BridgeEMapPairImage {
                        key,
                        value_par,
                        body_bytes: returned.body_bytes,
                    })
                } else { None }
            },
            _ => None,
        }
    }
}

pub open spec fn bridge_emap_pairs_from_returns(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> Option<Seq<BridgeEMapPairImage>>
    decreases summaries.len(),
{
    if summaries.len() == 0 {
        Some(Seq::empty())
    } else if summaries[0].slot == 3 {
        bridge_emap_pairs_from_returns(summaries.drop_first(), arena)
    } else {
        match bridge_emap_pair_from_return(summaries[0], arena) {
            None => None,
            Some(pair) => match bridge_emap_pairs_from_returns(summaries.drop_first(), arena) {
                None => None,
                Some(rest) => Some(seq![pair] + rest),
            },
        }
    }
}

pub open spec fn bridge_emap_pair_complete(pair: BridgeEMapPairImage) -> bool {
    pair.key.is_some() && pair.value_par.is_some()
}

pub open spec fn bridge_emap_complete(pairs: Seq<BridgeEMapPairImage>) -> bool
    decreases pairs.len(),
{
    pairs.len() == 0
        || (bridge_emap_pair_complete(pairs[0])
            && bridge_emap_complete(pairs.drop_first()))
}

pub open spec fn bridge_emap_complete_key_handles(
    pairs: Seq<BridgeEMapPairImage>,
) -> Seq<usize>
    decreases pairs.len(),
{
    if pairs.len() == 0 {
        Seq::empty()
    } else {
        let rest = bridge_emap_complete_key_handles(pairs.drop_first());
        if bridge_emap_pair_complete(pairs[0]) {
            seq![pairs[0].key.unwrap().score_handle] + rest
        } else {
            rest
        }
    }
}

pub proof fn bridge_emap_complete_key_handles_exact(
    pairs: Seq<BridgeEMapPairImage>,
)
    ensures
        bridge_emap_complete(pairs)
            == (bridge_emap_complete_key_handles(pairs).len() == pairs.len()),
    decreases pairs.len(),
{
    if pairs.len() > 0 {
        let rest = pairs.drop_first();
        bridge_emap_complete_key_handles_exact(rest);
        if !bridge_emap_pair_complete(pairs[0]) {
            bridge_emap_complete_key_handles_bounded(rest);
            assert(bridge_emap_complete_key_handles(pairs).len() <= rest.len());
        }
    }
}

pub proof fn bridge_emap_complete_key_handles_bounded(
    pairs: Seq<BridgeEMapPairImage>,
)
    ensures bridge_emap_complete_key_handles(pairs).len() <= pairs.len(),
    decreases pairs.len(),
{
    if pairs.len() > 0 {
        bridge_emap_complete_key_handles_bounded(pairs.drop_first());
    }
}

pub proof fn bridge_canonical_varint_end_unique(
    bytes: Seq<u8>, start: int, first_end: int, second_end: int,
    first_value: u64, second_value: u64,
)
    requires
        0 <= start < first_end <= bytes.len(),
        0 <= start < second_end <= bytes.len(),
        admission::canonical_varint_bytes(
            bytes.subrange(start, first_end), first_value,
        ),
        admission::canonical_varint_bytes(
            bytes.subrange(start, second_end), second_value,
        ),
    ensures first_end == second_end && first_value == second_value,
{
    if first_end < second_end {
        let at = first_end - start - 1;
        assert(0 <= at < second_end - start - 1);
        assert(bytes.subrange(start, first_end)[at] == bytes[first_end - 1]);
        assert(bytes.subrange(start, second_end)[at] == bytes[first_end - 1]);
        assert(bytes.subrange(start, first_end)[at] < 128);
        assert(bytes.subrange(start, second_end)[at] >= 128);
    } else if second_end < first_end {
        let at = second_end - start - 1;
        assert(0 <= at < first_end - start - 1);
        assert(bytes.subrange(start, second_end)[at] == bytes[second_end - 1]);
        assert(bytes.subrange(start, first_end)[at] == bytes[second_end - 1]);
        assert(bytes.subrange(start, second_end)[at] < 128);
        assert(bytes.subrange(start, first_end)[at] >= 128);
    }
}

pub proof fn bridge_canonical_field_tag_and_end_unique(
    bytes: Seq<u8>, cursor: usize,
    first: admission::IndexedCanonicalFieldModel,
    second: admission::IndexedCanonicalFieldModel,
)
    requires
        bytes.len() <= usize::MAX as int,
        admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: 0, region_end: bytes.len() as usize,
                next: cursor, source_bytes: bytes,
            }, first,
        ),
        admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: 0, region_end: bytes.len() as usize,
                next: cursor, source_bytes: bytes,
            }, second,
        ),
    ensures
        first.number == second.number,
        first.wire == second.wire,
        first.encoded_end == second.encoded_end,
{
    let first_key_end = first.encoded_start as int + first.key_width as int;
    let second_key_end = second.encoded_start as int + second.key_width as int;
    bridge_canonical_varint_end_unique(
        bytes, cursor as int, first_key_end, second_key_end,
        (first.number as int * 8 + first.wire as int) as u64,
        (second.number as int * 8 + second.wire as int) as u64,
    );
    assert(first.wire == 0 || first.wire == 1 || first.wire == 2 || first.wire == 5);
    assert(second.wire == 0 || second.wire == 1 || second.wire == 2 || second.wire == 5);
    assert(first.number == second.number && first.wire == second.wire);
    if first.wire == 2 {
        bridge_canonical_varint_end_unique(
            bytes, first_key_end, first.payload_start as int,
            second.payload_start as int,
            first.payload_length, second.payload_length,
        );
    } else if first.wire == 0 {
        bridge_canonical_varint_end_unique(
            bytes, first.payload_start as int, first.encoded_end as int,
            second.encoded_end as int,
            first.decoded_varint, second.decoded_varint,
        );
    }
}

pub open spec fn bridge_par_wire_field_sequence_at(
    bytes: Seq<u8>, fields: Seq<admission::IndexedCanonicalFieldModel>,
    cursor: usize, previous_tag: u32,
) -> bool
{
    admission::admission_par_wire_field_sequence_at(bytes, fields, cursor, previous_tag)
}

pub open spec fn bridge_par_wire_field_sequence(
    bytes: Seq<u8>, fields: Seq<admission::IndexedCanonicalFieldModel>,
) -> bool { admission::admission_par_wire_field_sequence(bytes, fields) }

pub open spec fn bridge_par_locally_free_range(
    fields: Seq<admission::IndexedCanonicalFieldModel>,
) -> Option<(usize, usize)> { admission::admission_par_locally_free_range(fields) }

pub open spec fn bridge_par_identity_from_fields(
    bytes: Seq<u8>, fields: Seq<admission::IndexedCanonicalFieldModel>,
) -> Seq<u8> { admission::admission_par_identity_from_fields(bytes, fields) }

pub proof fn bridge_par_locally_free_range_is_source_bounded(
    bytes: Seq<u8>, fields: Seq<admission::IndexedCanonicalFieldModel>,
    cursor: usize, previous_tag: u32,
)
    requires bridge_par_wire_field_sequence_at(bytes, fields, cursor, previous_tag),
    ensures match bridge_par_locally_free_range(fields) {
        None => true,
        Some((start, end)) => cursor <= start && start < end
            && end as int <= bytes.len(),
    },
    decreases fields.len(),
{
    if fields.len() > 0 {
        let field = fields[0];
        if field.number != 9 {
            bridge_par_locally_free_range_is_source_bounded(
                bytes, fields.drop_first(), field.encoded_end, field.number,
            );
        }
    }
}

pub proof fn bridge_par_identity_is_source_slice(
    bytes: Seq<u8>, fields: Seq<admission::IndexedCanonicalFieldModel>,
)
    requires bridge_par_wire_field_sequence(bytes, fields),
    ensures match bridge_par_locally_free_range(fields) {
        None => bridge_par_identity_from_fields(bytes, fields) == bytes,
        Some((start, end)) => {
            &&& start < end
            &&& end as int <= bytes.len()
            &&& bridge_par_identity_from_fields(bytes, fields)
                == bytes.subrange(0, start as int)
                    + bytes.subrange(end as int, bytes.len() as int)
        },
    },
{
    bridge_par_locally_free_range_is_source_bounded(bytes, fields, 0, 0);
}

pub proof fn bridge_par_locally_free_range_unique_at(
    bytes: Seq<u8>,
    first: Seq<admission::IndexedCanonicalFieldModel>,
    second: Seq<admission::IndexedCanonicalFieldModel>,
    cursor: usize, previous_tag: u32,
)
    requires
        bridge_par_wire_field_sequence_at(bytes, first, cursor, previous_tag),
        bridge_par_wire_field_sequence_at(bytes, second, cursor, previous_tag),
    ensures bridge_par_locally_free_range(first)
        == bridge_par_locally_free_range(second),
    decreases bytes.len() - cursor as int,
{
    if first.len() == 0 {
        assert(cursor as int == bytes.len());
        assert(second.len() == 0);
    } else if second.len() == 0 {
        assert(cursor as int == bytes.len());
        assert(first.len() == 0);
    } else {
        let left = first[0];
        let right = second[0];
        bridge_canonical_field_tag_and_end_unique(bytes, cursor, left, right);
        if left.number != 9 {
            bridge_par_locally_free_range_unique_at(
                bytes, first.drop_first(), second.drop_first(),
                left.encoded_end, left.number,
            );
        } else {
            assert(left.encoded_start == cursor && right.encoded_start == cursor);
        }
    }
}

pub proof fn bridge_par_identity_from_source_is_unique(
    bytes: Seq<u8>, fields: Seq<admission::IndexedCanonicalFieldModel>,
)
    requires bridge_par_wire_field_sequence(bytes, fields),
    ensures bridge_par_identity_from_source(bytes)
        == Some(bridge_par_identity_from_fields(bytes, fields)),
{
    assert(exists|candidate: Seq<admission::IndexedCanonicalFieldModel>|
        admission::admission_par_wire_field_sequence(bytes, candidate)) by {
        assert(admission::admission_par_wire_field_sequence(bytes, fields));
    }
    let selected = choose|selected: Seq<admission::IndexedCanonicalFieldModel>|
        admission::admission_par_wire_field_sequence(bytes, selected);
    assert(bridge_par_wire_field_sequence(bytes, selected));
    bridge_par_locally_free_range_unique_at(bytes, fields, selected, 0, 0);
    assert(admission::admission_par_locally_free_range(fields)
        == admission::admission_par_locally_free_range(selected));
    assert(admission::admission_par_identity_from_fields(bytes, fields)
        == admission::admission_par_identity_from_fields(bytes, selected));
    assert(admission::admission_par_identity_from_source(bytes)
        == Some(admission::admission_par_identity_from_fields(bytes, selected)));
}

pub open spec fn bridge_par_identity_from_source(bytes: Seq<u8>) -> Option<Seq<u8>> {
    admission::admission_par_identity_from_source(bytes)
}

pub proof fn bridge_empty_par_body_has_empty_identity()
    ensures bridge_par_identity_from_source(Seq::<u8>::empty())
        == Some(Seq::<u8>::empty()),
{
    let bytes = Seq::<u8>::empty();
    let fields = Seq::<admission::IndexedCanonicalFieldModel>::empty();
    assert(bridge_par_wire_field_sequence(bytes, fields));
    bridge_par_identity_from_source_is_unique(bytes, fields);
}

pub proof fn bridge_locally_free_only_par_has_empty_identity(
    bytes: Seq<u8>, field: admission::IndexedCanonicalFieldModel,
)
    requires
        field.number == 9,
        bridge_par_wire_field_sequence(bytes, seq![field]),
    ensures bridge_par_identity_from_source(bytes) == Some(Seq::<u8>::empty()),
{
    bridge_par_identity_from_source_is_unique(bytes, seq![field]);
    bridge_par_identity_is_source_slice(bytes, seq![field]);
    assert(bridge_par_wire_field_sequence_at(bytes, seq![field], 0, 0));
    assert(seq![field].drop_first() == Seq::<admission::IndexedCanonicalFieldModel>::empty());
    assert(bridge_par_wire_field_sequence_at(
        bytes, Seq::empty(), field.encoded_end, field.number,
    ));
    assert(field.encoded_start == 0);
    assert(field.encoded_end as int == bytes.len());
}

pub open spec fn bridge_emap_pair_score_order_lt(
    left: BridgeEMapPairImage,
    right: BridgeEMapPairImage,
    arena: Seq<admission::AdmissionScoreBinding>,
) -> bool {
    admission::admission_emap_pair_score_order_lt(
        bridge_emap_pair_as_admission(left),
        bridge_emap_pair_as_admission(right), arena,
    )
}

pub open spec fn bridge_emap_pair_as_admission(
    pair: BridgeEMapPairImage,
) -> admission::AdmissionEMapPairImage {
    admission::AdmissionEMapPairImage {
        key: pair.key, value_par: pair.value_par, body_bytes: pair.body_bytes,
    }
}

pub open spec fn bridge_emap_pairs_as_admission(
    pairs: Seq<BridgeEMapPairImage>,
) -> Seq<admission::AdmissionEMapPairImage> {
    Seq::new(pairs.len() as nat, |i: int| bridge_emap_pair_as_admission(pairs[i]))
}

pub proof fn bridge_emap_pairs_match_admission_source(
    summaries: Seq<admission::AdmissionReturnedSummary>,
    arena: Seq<admission::AdmissionScoreBinding>,
)
    requires
        bridge_emap_pairs_from_returns(summaries, arena).is_some(),
        admission::admission_emap_pairs_from_returns(summaries, arena).is_some(),
    ensures admission::admission_emap_pairs_from_returns(summaries, arena).unwrap()
        == bridge_emap_pairs_as_admission(
            bridge_emap_pairs_from_returns(summaries, arena).unwrap(),
        ),
    decreases summaries.len(),
{
    if summaries.len() > 0 {
        bridge_emap_pairs_match_admission_source(summaries.drop_first(), arena);
        let bridge_pairs = bridge_emap_pairs_from_returns(summaries, arena).unwrap();
        let admission_pairs = admission::admission_emap_pairs_from_returns(
            summaries, arena,
        ).unwrap();
        assert(bridge_pairs.len() == admission_pairs.len());
        assert forall|i: int| 0 <= i < bridge_pairs.len()
            implies admission_pairs[i]
                == bridge_emap_pair_as_admission(bridge_pairs[i]) by {
            if summaries[0].slot != 3 && i > 0 {
                assert(bridge_pairs[i]
                    == bridge_emap_pairs_from_returns(
                        summaries.drop_first(), arena,
                    ).unwrap()[i - 1]);
            }
        }
    }
}

pub open spec fn bridge_emap_source_fixed_point(
    pairs: Seq<BridgeEMapPairImage>, arena: Seq<admission::AdmissionScoreBinding>,
) -> bool {
    admission::admission_emap_source_fixed_point(
        bridge_emap_pairs_as_admission(pairs), arena,
    )
}

pub proof fn bridge_emap_complete_matches_admission(
    pairs: Seq<BridgeEMapPairImage>,
)
    ensures bridge_emap_complete(pairs)
        == admission::admission_emap_complete(
            bridge_emap_pairs_as_admission(pairs),
        ),
    decreases pairs.len(),
{
    if pairs.len() > 0 {
        bridge_emap_complete_matches_admission(pairs.drop_first());
        let converted = bridge_emap_pairs_as_admission(pairs);
        assert(converted.len() == pairs.len());
        assert(converted[0] == bridge_emap_pair_as_admission(pairs[0]));
        assert(converted.drop_first()
            == bridge_emap_pairs_as_admission(pairs.drop_first()));
    }
}

pub struct BridgeEMapFinishSource {
    pub ghost pairs: Seq<BridgeEMapPairImage>,
    pub ghost pair_scores: Seq<usize>,
    pub remainder: Option<usize>,
    pub connective_used: bool,
    pub complete: bool,
    pub canonical_fixed_point: bool,
}

pub open spec fn bridge_emap_finish_source(
    bridge: ScoreAdmissionBridge,
) -> Option<BridgeEMapFinishSource> {
    if bridge.admission.frames.len() == 0 {
        None
    } else {
        let frame = bridge.admission.frames.last();
        if frame.family != admission::AdmissionFamily::Other
            || frame.frame_kind != 1 || frame.program_type_id != 30
            || frame.cursor != frame.region_end || frame.waiting_id.is_some()
        {
            None
        } else {
            match (
                bridge_emap_pairs_from_returns(frame.returns, bridge.admission.arena),
                bridge_var_handles_for_slot(frame.returns, bridge.admission.arena, 3, 5),
                bridge_bool_slot(frame, 2, 4),
                admission::admission_emap_source_summary(
                    frame, bridge.admission.arena,
                ),
            ) {
                (Some(pairs), Some(remainders), Some(connective_used),
                    Some((complete, canonical_fixed_point)))
                    if remainders.len() <= 1
                        && frame.returns.len() == pairs.len() + remainders.len()
                        && frame.seen.contains(0) == (pairs.len() > 0)
                        && frame.seen.contains(3) == (remainders.len() == 1) =>
                    Some(BridgeEMapFinishSource {
                        pair_scores: bridge_emap_complete_key_handles(pairs),
                        complete,
                        canonical_fixed_point,
                        pairs,
                        remainder: if remainders.len() == 0 {
                            None
                        } else { Some(remainders[0]) },
                        connective_used,
                    }),
                _ => None,
            }
        }
    }
}

pub proof fn bridge_emap_finish_source_complete_and_incomplete(
    bridge: ScoreAdmissionBridge,
)
    ensures match bridge_emap_finish_source(bridge) {
        None => true,
        Some(source) => {
            &&& admission::admission_emap_source_summary(
                bridge.admission.frames.last(), bridge.admission.arena,
            ) == Some((source.complete, source.canonical_fixed_point))
            &&& source.complete == bridge_emap_complete(source.pairs)
            &&& source.pair_scores == bridge_emap_complete_key_handles(source.pairs)
            &&& source.canonical_fixed_point
                == bridge_emap_source_fixed_point(source.pairs, bridge.admission.arena)
            &&& (source.complete
                == (source.pair_scores.len() == source.pairs.len()))
            &&& (!source.complete ==> source.pair_scores.len() < source.pairs.len())
        },
    },
{
    if let Some(source) = bridge_emap_finish_source(bridge) {
        let frame = bridge.admission.frames.last();
        bridge_emap_pairs_match_admission_source(
            frame.returns, bridge.admission.arena,
        );
        bridge_emap_complete_matches_admission(source.pairs);
        let admitted_pairs = admission::admission_emap_pairs_from_returns(
            frame.returns, bridge.admission.arena,
        ).unwrap();
        assert(admitted_pairs == bridge_emap_pairs_as_admission(source.pairs));
        assert(admission::admission_emap_source_summary(
            frame, bridge.admission.arena,
        ) == Some((source.complete, source.canonical_fixed_point)));
        bridge_emap_complete_key_handles_exact(source.pairs);
        bridge_emap_complete_key_handles_bounded(source.pairs);
    }
}

pub open spec fn bridge_emap_finish_step(
    bridge: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize, usize, usize, usize, usize)> {
    match bridge_emap_finish_source(bridge) {
        Some(source) if source.complete => {
            match bridge_append_unattached(bridge, score::ScoreNode::Integer(-1i64)) {
                None => None,
                Some((after_default, default_handle)) => {
                    let remainder = match source.remainder {
                        Some(handle) => handle,
                        None => default_handle,
                    };
                    match bridge_append_unattached(
                        after_default,
                        score::ScoreNode::Integer(
                            if source.connective_used { 1i64 } else { 0i64 },
                        ),
                    ) {
                        None => None,
                        Some((after_flag, flag_handle)) => match bridge_constructor_step(
                            after_flag, 9i32,
                            seq![remainder] + source.pair_scores + seq![flag_handle],
                        ) {
                            None => None,
                            Some((next, constructor_handle, score_handle)) => Some((
                                next, default_handle, remainder, flag_handle,
                                constructor_handle, score_handle,
                            )),
                        },
                    }
                },
            }
        },
        _ => None,
    }
}

pub proof fn bridge_emap_incomplete_finish_has_no_score_step(
    bridge: ScoreAdmissionBridge,
)
    requires
        bridge_emap_finish_source(bridge).is_some(),
        !bridge_emap_finish_source(bridge).unwrap().complete,
    ensures bridge_emap_finish_step(bridge).is_none(),
{}

pub proof fn bridge_emap_complete_finish_source_exact(
    bridge: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_emap_finish_step(bridge) {
        None => true,
        Some((next, default_handle, remainder, flag_handle,
            constructor_handle, score_handle)) => {
            let source = bridge_emap_finish_source(bridge).unwrap();
            let children = seq![remainder] + source.pair_scores + seq![flag_handle];
            &&& source.complete
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(bridge, next)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& score::handle_contents(next.transcript, default_handle)
                == Some(score::ScoreNode::Integer(-1i64))
            &&& score::handle_contents(next.transcript, flag_handle)
                == Some(score::ScoreNode::Integer(
                    if source.connective_used { 1i64 } else { 0i64 },
                ))
            &&& score::handle_contents(next.transcript, constructor_handle)
                == Some(score::ScoreNode::Integer(9i64))
            &&& score::handle_contents(next.transcript, score_handle)
                == Some(score::ScoreNode::Node(seq![constructor_handle] + children))
        },
    },
{
    if let Some(source) = bridge_emap_finish_source(bridge) {
        if source.complete {
            let default_kind = score::ScoreNode::Integer(-1i64);
            bridge_append_unattached_exact(bridge, default_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, default_kind);
            bridge_append_unattached_shape(bridge, default_kind);
            if let Some((after_default, default_handle)) =
                bridge_append_unattached(bridge, default_kind)
            {
                let remainder = match source.remainder {
                    Some(handle) => handle,
                    None => default_handle,
                };
                let flag_kind = score::ScoreNode::Integer(
                    if source.connective_used { 1i64 } else { 0i64 },
                );
                bridge_append_unattached_exact(after_default, flag_kind);
                bridge_append_unattached_preserves_admission_wf(after_default, flag_kind);
                bridge_append_unattached_shape(after_default, flag_kind);
                if let Some((after_flag, flag_handle)) =
                    bridge_append_unattached(after_default, flag_kind)
                {
                    let children = seq![remainder] + source.pair_scores + seq![flag_handle];
                    bridge_constructor_exact(after_flag, 9i32, children);
                    bridge_constructor_shape(after_flag, 9i32, children);
                    if let Some((next, _, _)) =
                        bridge_constructor_step(after_flag, 9i32, children)
                    {
                        bridge_score_step_shape_transitive(
                            after_default, after_flag, next,
                        );
                        bridge_score_step_shape_transitive(bridge, after_default, after_flag);
                        bridge_score_step_shape_transitive(bridge, after_flag, next);
                        bridge_score_handle_preserved(
                            after_flag.transcript, next.transcript, flag_handle,
                        );
                        bridge_score_handle_preserved(
                            after_default.transcript, next.transcript, default_handle,
                        );
                    }
                }
            }
        }
    }
}

pub proof fn bridge_emap_incomplete_pair_has_no_key_score(
    pair: BridgeEMapPairImage,
)
    requires !bridge_emap_pair_complete(pair),
    ensures bridge_emap_complete_key_handles(seq![pair]).len() == 0,
{
    let one = seq![pair];
    assert(one.len() == 1 && one[0] == pair);
    assert(one.drop_first() == Seq::<BridgeEMapPairImage>::empty());
    assert(bridge_emap_complete_key_handles(one.drop_first()).len() == 0);
    assert(bridge_emap_complete_key_handles(one) == Seq::<usize>::empty());
}

pub proof fn bridge_emap_complete_pair_has_exact_key_score(
    pair: BridgeEMapPairImage,
)
    requires bridge_emap_pair_complete(pair),
    ensures bridge_emap_complete_key_handles(seq![pair])
        == seq![pair.key.unwrap().score_handle],
{
    let one = seq![pair];
    assert(one.len() == 1 && one[0] == pair);
    assert(one.drop_first() == Seq::<BridgeEMapPairImage>::empty());
    assert(bridge_emap_complete_key_handles(one.drop_first()) == Seq::<usize>::empty());
}

pub proof fn bridge_emap_duplicate_identity_rejects_fixed_point(
    pairs: Seq<BridgeEMapPairImage>, arena: Seq<admission::AdmissionScoreBinding>,
    first: int, second: int,
)
    requires
        0 <= first < second < pairs.len(),
        bridge_emap_pair_complete(pairs[first]),
        bridge_emap_pair_complete(pairs[second]),
        bridge_par_identity_from_source(pairs[first].key.unwrap().body_bytes).is_some(),
        bridge_par_identity_from_source(pairs[first].key.unwrap().body_bytes)
            == bridge_par_identity_from_source(pairs[second].key.unwrap().body_bytes),
    ensures !bridge_emap_source_fixed_point(pairs, arena),
{
    let converted = bridge_emap_pairs_as_admission(pairs);
    assert(converted[first] == bridge_emap_pair_as_admission(pairs[first]));
    assert(converted[second] == bridge_emap_pair_as_admission(pairs[second]));
    assert(!admission::admission_emap_source_fixed_point(converted, arena));
}

pub proof fn bridge_emap_pair_out_of_order_rejects_fixed_point(
    pairs: Seq<BridgeEMapPairImage>, arena: Seq<admission::AdmissionScoreBinding>,
    first: int, second: int,
)
    requires
        0 <= first < second < pairs.len(),
        bridge_emap_pair_complete(pairs[first]),
        bridge_emap_pair_complete(pairs[second]),
        forall|k: int| first < k < second ==> !bridge_emap_pair_complete(pairs[k]),
        !bridge_emap_pair_score_order_lt(pairs[first], pairs[second], arena),
    ensures !bridge_emap_source_fixed_point(pairs, arena),
{
    let converted = bridge_emap_pairs_as_admission(pairs);
    assert(converted[first] == bridge_emap_pair_as_admission(pairs[first]));
    assert(converted[second] == bridge_emap_pair_as_admission(pairs[second]));
    assert forall|k: int| first < k < second
        implies !admission::admission_emap_pair_complete(converted[k]) by {
        assert(converted[k] == bridge_emap_pair_as_admission(pairs[k]));
    }
    assert(!admission::admission_emap_source_fixed_point(converted, arena));
}

pub proof fn bridge_key_value_pair_source_eof_selected_none(
    bridge: ScoreAdmissionBridge,
)
    requires
        bridge.admission.frames.len() > 0,
        admission::admission_frame_wf(bridge.admission.frames.last()),
        admission::admission_key_value_pair_fields(
            bridge.admission.frames.last(),
        ).is_some(),
    ensures
        bridge.admission.frames.last().program_type_id == 34,
        bridge.admission.frames.last().cursor
            == bridge.admission.frames.last().region_end,
        bridge.admission.frames.last().score_handle.is_none(),
        bridge_score_finish_selected(bridge).is_none(),
{
    let child = bridge.admission.frames.last();
    assert(child.family == admission::AdmissionFamily::Other);
    assert(child.frame_kind == 1);
    assert(child.program_type_id == 34);
    assert(child.score_handle.is_none());
    bridge_other_summary_child_finish_selected_none(bridge);
}

pub open spec fn bridge_key_value_pair_parent_pop_step(
    bridge: ScoreAdmissionBridge,
) -> Option<ScoreAdmissionBridge> {
    if bridge.admission.frames.len() <= 1 {
        None
    } else {
        let child = bridge.admission.frames.last();
        let parent = bridge.admission.frames[bridge.admission.frames.len() - 2];
        if parent.family != admission::AdmissionFamily::Other
            || parent.frame_kind != 1 || parent.program_type_id != 30
            || parent.waiting_slot != Some(0) || parent.waiting_tag != Some(1)
            || child.return_id.is_none()
        {
            None
        } else {
            match admission::admission_key_value_pair_fields(child) {
                None => None,
                Some((key, value_par)) => {
                    let summary = admission::AdmissionReturnedKind::KeyValuePair {
                        key, value_par,
                    };
                    match admission::admission_complete_child_step(
                        bridge.admission, child.return_id.unwrap(), child.frame_kind,
                        summary,
                    ) {
                        None => None,
                        Some(next_admission) => Some(ScoreAdmissionBridge {
                            admission: next_admission,
                            transcript: bridge.transcript,
                        }),
                    }
                },
            }
        }
    }
}

pub proof fn bridge_key_value_pair_parent_pop_preserves_source(
    bridge: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_key_value_pair_parent_pop_step(bridge) {
        None => true,
        Some(next) => {
            let child = bridge.admission.frames.last();
            let prior_parent = bridge.admission.frames[bridge.admission.frames.len() - 2];
            let returned = next.admission.frames.last().returns.last();
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& next.transcript == bridge.transcript
            &&& next.admission.arena == bridge.admission.arena
            &&& next.admission.frames.len() + 1 == bridge.admission.frames.len()
            &&& returned.body_bytes == child.source_bytes
            &&& returned.body_id == child.body_id
            &&& returned.score_handle.is_none()
            &&& returned.kind == admission::AdmissionReturnedKind::KeyValuePair {
                key: admission::admission_key_value_pair_fields(child).unwrap().0,
                value_par: admission::admission_key_value_pair_fields(child).unwrap().1,
            }
            &&& next.admission.frames.last().physical_source == prior_parent.physical_source
            &&& next.admission.frames.last().physical_start == prior_parent.physical_start
        },
    },
{
    if bridge.admission.frames.len() > 1 {
        let child = bridge.admission.frames.last();
        let parent = bridge.admission.frames[bridge.admission.frames.len() - 2];
        if parent.family == admission::AdmissionFamily::Other
            && parent.frame_kind == 1 && parent.program_type_id == 30
            && parent.waiting_slot == Some(0) && parent.waiting_tag == Some(1)
            && child.return_id.is_some()
        {
            if let Some((key, value_par)) =
                admission::admission_key_value_pair_fields(child)
            {
                let summary = admission::AdmissionReturnedKind::KeyValuePair {
                    key, value_par,
                };
                admission::admission_complete_child_refines(
                    bridge.admission, child.return_id.unwrap(), child.frame_kind,
                    summary,
                );
                admission::admission_complete_child_preserves_wf(
                    bridge.admission, child.return_id.unwrap(), child.frame_kind,
                    summary,
                );
                if let Some(next) = bridge_key_value_pair_parent_pop_step(bridge) {
                    assert(next.admission.arena == bridge.admission.arena);
                    assert(next.transcript == bridge.transcript);
                    assert(bridge_well_formed(next));
                }
            }
        }
    }
}

pub open spec fn bridge_expr_structural_missing_par_frame(
    before: ScoreAdmissionBridge,
) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && before.admission.frames.last().family == admission::AdmissionFamily::Other
        && before.admission.frames.last().frame_kind == 1
        && match bridge_expr_structural_constructor(
            before.admission.frames.last().program_type_id,
        ) {
            Some((_, binary)) =>
                !admission::admission_returned_par(before.admission.frames.last(), 0)
                || binary && !admission::admission_returned_par(
                    before.admission.frames.last(), 1,
                ),
            None => false,
        }
}

pub proof fn bridge_expr_structural_missing_par_finish_selected_none(
    before: ScoreAdmissionBridge,
)
    requires bridge_expr_structural_missing_par_frame(before),
    ensures bridge_score_finish_selected(before).is_none(),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
}

pub open spec fn bridge_emethod_missing_target_frame(before: ScoreAdmissionBridge) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && before.admission.frames.last().family == admission::AdmissionFamily::Other
        && before.admission.frames.last().frame_kind == 1
        && before.admission.frames.last().program_type_id == 33
        && !admission::admission_returned_par(before.admission.frames.last(), 1)
}

pub proof fn bridge_emethod_missing_target_finish_selected_none(before: ScoreAdmissionBridge)
    requires bridge_emethod_missing_target_frame(before),
    ensures bridge_score_finish_selected(before).is_none(),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
}

pub open spec fn bridge_emap_incomplete_frame(before: ScoreAdmissionBridge) -> bool {
    before.admission.frames.len() > 0
        && admission::admission_frame_wf(before.admission.frames.last())
        && before.admission.frames.last().family == admission::AdmissionFamily::Other
        && before.admission.frames.last().program_type_id == 30
        && match bridge_emap_finish_source(before) {
            Some(source) => !source.complete,
            None => false,
        }
}

pub proof fn bridge_emap_incomplete_finish_selected_none(before: ScoreAdmissionBridge)
    requires bridge_emap_incomplete_frame(before),
    ensures bridge_score_finish_selected(before).is_none(),
{
    assert(bridge_structural_no_score_child_frame(before));
    bridge_structural_no_score_child_finish_selected_none(before);
}

pub open spec fn bridge_message_driver_step(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
) -> bool {
    exists|action: admission::ConcreteFrameActionModel|
        match #[trigger] admission::concrete_message_driver_step(before.admission, action) {
            Some(admission::ConcreteFrameOutcomeModel::Continued { state })
            | Some(admission::ConcreteFrameOutcomeModel::Pushed { state })
            | Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) =>
                bridge_admission_only_step(before, after, Some(state)),
            _ => false,
        }
}

pub open spec fn bridge_source_step(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
) -> bool {
    bridge_score_source_step(before, after)
        || bridge_message_driver_step(before, after)
}

pub proof fn bridge_score_source_step_preserves_exact(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        bridge_score_source_step(before, after),
    ensures
        bridge_well_formed(after),
        admission::admission_state_wf(after.admission),
{
    if let Some((next, _, _, _)) = bridge_raw_par_step(before) {
        bridge_raw_par_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_cost_atom_absent_step(before) {
        bridge_cost_atom_absent_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_cost_atom_bytes_step(before) {
        bridge_cost_atom_bytes_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_cost_bound_step(before) {
        bridge_cost_bound_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _)) = bridge_cost_atom_bound_forward_step(before) {
        bridge_cost_atom_bound_forward_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_cost_signature_step(before) {
        bridge_cost_signature_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _)) = bridge_cost_region_step(before) {
        bridge_cost_region_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_cost_token_stack_step(before) {
        bridge_cost_token_stack_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_bundle_step(before) {
        bridge_bundle_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _)) = bridge_if_step(before) {
        bridge_if_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _)) = bridge_send_step(before) {
        bridge_send_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _)) = bridge_match_step(before) {
        bridge_match_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_absent_specialized_constructor_step(before) {
        bridge_absent_specialized_constructor_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _)) = bridge_absent_connective_step(before) {
        bridge_absent_connective_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_receive_bind_step(before) {
        bridge_receive_bind_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _)) = bridge_match_case_step(before) {
        bridge_match_case_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _)) = bridge_bool_oneof_step(before) {
        bridge_bool_oneof_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _)) = bridge_expr_atom_step(before) {
        bridge_expr_atom_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _)) = bridge_expr_numeric_step(before) {
        bridge_expr_numeric_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _)) = bridge_expr_big_rational_step(before) {
        bridge_expr_big_rational_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _)) = bridge_expr_fixed_point_step(before) {
        bridge_expr_fixed_point_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_sys_auth_token_step(before) {
        bridge_sys_auth_token_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _)) = bridge_new_empty_collections_step(before) {
        bridge_new_empty_collections_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _, _)) = bridge_new_one_injection_step(before) {
        bridge_new_one_injection_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _, _, _)) = bridge_new_one_absent_injection_step(before) {
        bridge_new_one_absent_injection_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _)) = bridge_new_one_uri_step(before) {
        bridge_new_one_uri_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _)) = bridge_new_collections_step(before) {
        bridge_new_collections_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _)) = bridge_expr_payload_forward_step(before) {
        bridge_expr_payload_forward_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _)) = bridge_expr_structural_step(before) {
        bridge_expr_structural_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _)) = bridge_evar_step(before) {
        bridge_evar_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _)) = bridge_ordered_sequence_step(before) {
        bridge_ordered_sequence_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _)) = bridge_emethod_step(before) {
        bridge_emethod_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _)) = bridge_emap_finish_step(before) {
        bridge_emap_complete_finish_source_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_connective_not_step(before) {
        bridge_connective_not_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _)) = bridge_connective_varref_step(before) {
        bridge_connective_varref_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_connective_collection_step(before) {
        bridge_connective_collection_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _)) = bridge_unforgeable_bytes_step(before) {
        bridge_unforgeable_bytes_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _, _)) = bridge_receive_present_condition_step(before) {
        bridge_receive_present_condition_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _, _, _, _, _, _)) = bridge_receive_absent_condition_step(before) {
        bridge_receive_absent_condition_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    if let Some((next, _, _)) = bridge_nonsemantic_step(before) {
        bridge_nonsemantic_exact(before);
        if bridge_same(next, after) {
            assert(bridge_well_formed(after));
            assert(admission::admission_state_wf(after.admission));
            return;
        }
    }
    assert(false);
}

pub proof fn bridge_raw_par_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_raw_par_step(bridge) {
        None => true,
        Some((next, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _)) = bridge_raw_par_step(bridge) {
        let top = bridge.admission.frames.last();
        if let Some(components) = bridge_raw_par_ordered_handles(
            top.returns, bridge_raw_par_score_tags(),
        ) {
            let flag = if admission::admission_raw_par_connective_used(top) {
                1i64
            } else { 0i64 };
            let kind = score::ScoreNode::Integer(flag);
            bridge_append_unattached_shape(bridge, kind);
            bridge_append_unattached_exact(bridge, kind);
            bridge_append_unattached_preserves_admission_wf(bridge, kind);
            if let Some((after_flag, flag_handle)) = bridge_append_unattached(bridge, kind) {
                bridge_constructor_shape(
                    after_flag, 999i32, components.push(flag_handle),
                );
                bridge_score_step_shape_transitive(bridge, after_flag, next);
            }
        }
    }
}

pub proof fn bridge_cost_atom_bytes_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_atom_bytes_step(bridge) {
        None => true,
        Some((next, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _)) = bridge_cost_atom_bytes_step(bridge) {
        let occurrence = bridge.admission.frames.last().scalar_occurrences[0];
        if let Some(constructor) = bridge_cost_atom_bytes_code(occurrence.tag) {
            let kind = score::ScoreNode::BytesAtom(occurrence.bytes);
            bridge_append_unattached_shape(bridge, kind);
            bridge_append_unattached_exact(bridge, kind);
            bridge_append_unattached_preserves_admission_wf(bridge, kind);
            if let Some((after_bytes, bytes_handle)) = bridge_append_unattached(bridge, kind) {
                bridge_constructor_shape(after_bytes, constructor, seq![bytes_handle]);
                bridge_score_step_shape_transitive(bridge, after_bytes, next);
            }
        }
    }
}

pub proof fn bridge_cost_bound_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_bound_step(bridge) {
        None => true,
        Some((next, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _)) = bridge_cost_bound_step(bridge) {
        if let Some(index) = bridge_cost_bound_value(bridge.admission.frames.last()) {
            let kind = score::ScoreNode::Integer(index as i64);
            bridge_append_unattached_shape(bridge, kind);
            bridge_append_unattached_exact(bridge, kind);
            bridge_append_unattached_preserves_admission_wf(bridge, kind);
            if let Some((after_index, index_handle)) = bridge_append_unattached(bridge, kind) {
                bridge_constructor_shape(after_index, 504i32, seq![index_handle]);
                bridge_score_step_shape_transitive(bridge, after_index, next);
            }
        }
    }
}

pub proof fn bridge_cost_atom_bound_forward_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_cost_atom_bound_forward_step(bridge) {
        None => true,
        Some((next, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((_, handle)) = bridge_cost_atom_bound_forward_step(bridge) {
        bridge_attach_score_shape(
            bridge, handle, bridge.admission.frames.last().body_id,
        );
    }
}

pub proof fn bridge_if_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_if_step(bridge) {
        None => true,
        Some((next, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _)) = bridge_if_step(bridge) {
        if let Some((condition, if_true, if_false, connective_used)) = bridge_if_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            let flag = if connective_used { 1i64 } else { 0i64 };
            let kind = score::ScoreNode::Integer(flag);
            bridge_append_unattached_shape(bridge, kind);
            bridge_append_unattached_exact(bridge, kind);
            bridge_append_unattached_preserves_admission_wf(bridge, kind);
            if let Some((after_flag, flag_handle)) = bridge_append_unattached(bridge, kind) {
                bridge_constructor_shape(
                    after_flag, 309i32,
                    seq![condition, if_true, if_false, flag_handle],
                );
                bridge_score_step_shape_transitive(bridge, after_flag, next);
            }
        }
    }
}

pub proof fn bridge_send_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_send_step(bridge) {
        None => true,
        Some((next, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _)) = bridge_send_step(bridge) {
        if let Some((persistent, channel, data, connective_used, cost)) =
            bridge_send_parts(bridge.admission.frames.last(), bridge.admission.arena)
        {
            let persistent_kind = score::ScoreNode::Integer(
                if persistent { 1i64 } else { 0i64 },
            );
            let connective_kind = score::ScoreNode::Integer(
                if connective_used { 1i64 } else { 0i64 },
            );
            bridge_append_unattached_shape(bridge, persistent_kind);
            bridge_append_unattached_exact(bridge, persistent_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, persistent_kind);
            if let Some((after_persistent, persistent_handle)) =
                bridge_append_unattached(bridge, persistent_kind)
            {
                bridge_append_unattached_shape(after_persistent, connective_kind);
                bridge_append_unattached_exact(after_persistent, connective_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_persistent, connective_kind,
                );
                if let Some((after_connective, connective_handle)) =
                    bridge_append_unattached(after_persistent, connective_kind)
                {
                    let children = seq![persistent_handle, channel]
                        + data + seq![connective_handle] + cost;
                    bridge_constructor_shape(after_connective, 300i32, children);
                    bridge_score_step_shape_transitive(
                        bridge, after_persistent, after_connective,
                    );
                    bridge_score_step_shape_transitive(
                        bridge, after_connective, next,
                    );
                }
            }
        }
    }
}

pub proof fn bridge_match_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_match_step(bridge) {
        None => true,
        Some((next, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _)) = bridge_match_step(bridge) {
        if let Some((target, cases, connective_used)) = bridge_match_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            let flag_kind = score::ScoreNode::Integer(
                if connective_used { 1i64 } else { 0i64 },
            );
            bridge_append_unattached_shape(bridge, flag_kind);
            bridge_append_unattached_exact(bridge, flag_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, flag_kind);
            if let Some((after_flag, flag_handle)) = bridge_append_unattached(
                bridge, flag_kind,
            ) {
                bridge_constructor_shape(
                    after_flag, 304i32, seq![target] + cases + seq![flag_handle],
                );
                bridge_score_step_shape_transitive(bridge, after_flag, next);
            }
        }
    }
}

pub proof fn bridge_absent_specialized_constructor_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_absent_specialized_constructor_step(bridge) {
        None => true,
        Some((next, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((_, _, _)) = bridge_absent_specialized_constructor_step(bridge) {
        bridge_constructor_shape(bridge, 0i32, Seq::empty());
    }
}

pub proof fn bridge_absent_connective_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_absent_connective_step(bridge) {
        None => true,
        Some((next, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _)) = bridge_absent_connective_step(bridge) {
        let kind = score::ScoreNode::Integer(0i64);
        bridge_append_unattached_shape(bridge, kind);
        bridge_append_unattached_exact(bridge, kind);
        bridge_append_unattached_preserves_admission_wf(bridge, kind);
        if let Some((after_integer, score_handle)) = bridge_append_unattached(bridge, kind) {
            bridge_attach_score_shape(
                after_integer, score_handle, bridge.admission.frames.last().body_id,
            );
            bridge_score_step_shape_transitive(bridge, after_integer, next);
        }
    }
}

pub proof fn bridge_receive_bind_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_receive_bind_step(bridge) {
        None => true,
        Some((next, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _)) = bridge_receive_bind_step(bridge) {
        let top = bridge.admission.frames.last();
        if let Some((source, patterns, remainder, cost)) = bridge_receive_bind_parts(
            top, bridge.admission.arena,
        ) {
            let default_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_shape(bridge, default_kind);
            bridge_append_unattached_exact(bridge, default_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, default_kind);
            if let Some((after_default, default_handle)) = bridge_append_unattached(
                bridge, default_kind,
            ) {
                let selected_remainder = match remainder {
                    Some(handle) => handle,
                    None => default_handle,
                };
                let node_kind = score::ScoreNode::Node(
                    seq![source] + patterns + seq![selected_remainder] + cost,
                );
                bridge_append_unattached_shape(after_default, node_kind);
                bridge_append_unattached_exact(after_default, node_kind);
                bridge_append_unattached_preserves_admission_wf(after_default, node_kind);
                if let Some((after_node, score_handle)) = bridge_append_unattached(
                    after_default, node_kind,
                ) {
                    bridge_attach_score_shape(after_node, score_handle, top.body_id);
                    bridge_score_step_shape_transitive(bridge, after_default, after_node);
                    bridge_score_step_shape_transitive(bridge, after_node, next);
                }
            }
        }
    }
}

pub proof fn bridge_match_case_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_match_case_step(bridge) {
        None => true,
        Some((next, _, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _, _)) = bridge_match_case_step(bridge) {
        let top = bridge.admission.frames.last();
        if let Some((pattern, source, free_count, guard)) = bridge_match_case_parts(
            top, bridge.admission.arena,
        ) {
            let flag_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_shape(bridge, flag_kind);
            bridge_append_unattached_exact(bridge, flag_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, flag_kind);
            if let Some((after_flag, flag_handle)) = bridge_append_unattached(
                bridge, flag_kind,
            ) {
                let constructor_kind = score::ScoreNode::Integer(999i64);
                bridge_append_unattached_shape(after_flag, constructor_kind);
                bridge_append_unattached_exact(after_flag, constructor_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_flag, constructor_kind,
                );
                if let Some((after_constructor, constructor_handle)) =
                    bridge_append_unattached(after_flag, constructor_kind)
                {
                    let empty_kind = score::ScoreNode::Node(
                        seq![constructor_handle, flag_handle],
                    );
                    bridge_append_unattached_shape(after_constructor, empty_kind);
                    bridge_append_unattached_exact(after_constructor, empty_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_constructor, empty_kind,
                    );
                    if let Some((after_empty, empty_handle)) =
                        bridge_append_unattached(after_constructor, empty_kind)
                    {
                        let selected_guard = match guard {
                            Some(handle) => handle,
                            None => empty_handle,
                        };
                        let count_kind = score::ScoreNode::Integer(free_count);
                        bridge_append_unattached_shape(after_empty, count_kind);
                        bridge_append_unattached_exact(after_empty, count_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_empty, count_kind,
                        );
                        if let Some((after_count, count_handle)) =
                            bridge_append_unattached(after_empty, count_kind)
                        {
                            let node_kind = score::ScoreNode::Node(
                                seq![pattern, source, count_handle, selected_guard],
                            );
                            bridge_append_unattached_shape(after_count, node_kind);
                            bridge_append_unattached_exact(after_count, node_kind);
                            bridge_append_unattached_preserves_admission_wf(
                                after_count, node_kind,
                            );
                            if let Some((after_node, score_handle)) =
                                bridge_append_unattached(after_count, node_kind)
                            {
                                bridge_attach_score_shape(after_node, score_handle, top.body_id);
                                bridge_score_step_shape_transitive(
                                    bridge, after_flag, after_constructor,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_constructor, after_empty,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_empty, after_count,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_count, after_node,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_node, next,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_bool_oneof_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_bool_oneof_step(bridge) {
        None => true,
        Some((next, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _)) = bridge_bool_oneof_step(bridge) {
        if let Some((constructor, value)) = bridge_bool_oneof_parts(
            bridge.admission.frames.last(),
        ) {
            let value_kind = score::ScoreNode::Integer(value);
            bridge_append_unattached_shape(bridge, value_kind);
            bridge_append_unattached_exact(bridge, value_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, value_kind);
            if let Some((after_value, value_handle)) = bridge_append_unattached(
                bridge, value_kind,
            ) {
                bridge_constructor_shape(after_value, constructor, seq![value_handle]);
                bridge_score_step_shape_transitive(bridge, after_value, next);
            }
        }
    }
}

pub proof fn bridge_expr_atom_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_atom_step(bridge) {
        None => true,
        Some((next, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _)) = bridge_expr_atom_step(bridge) {
        if let Some((constructor, atom_kind)) = bridge_expr_atom_parts(
            bridge.admission.frames.last(),
        ) {
            bridge_append_unattached_shape(bridge, atom_kind);
            bridge_append_unattached_exact(bridge, atom_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, atom_kind);
            if let Some((after_atom, atom_handle)) = bridge_append_unattached(
                bridge, atom_kind,
            ) {
                bridge_constructor_shape(after_atom, constructor, seq![atom_handle]);
                bridge_score_step_shape_transitive(bridge, after_atom, next);
            }
        }
    }
}

pub proof fn bridge_expr_numeric_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_numeric_step(bridge) {
        None => true,
        Some((next, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _)) = bridge_expr_numeric_step(bridge) {
        if let Some((constructor, value)) = bridge_expr_numeric_parts(
            bridge.admission.frames.last(),
        ) {
            let value_kind = score::ScoreNode::Integer(value);
            bridge_append_unattached_shape(bridge, value_kind);
            bridge_append_unattached_exact(bridge, value_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, value_kind);
            if let Some((after_value, value_handle)) = bridge_append_unattached(
                bridge, value_kind,
            ) {
                bridge_constructor_shape(after_value, constructor, seq![value_handle]);
                bridge_score_step_shape_transitive(bridge, after_value, next);
            }
        }
    }
}

pub proof fn bridge_expr_big_rational_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_big_rational_step(bridge) {
        None => true,
        Some((next, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _)) = bridge_expr_big_rational_step(bridge) {
        if let Some((numerator, denominator)) = bridge_expr_big_rational_parts(
            bridge.admission.frames.last(),
        ) {
            let numerator_kind = score::ScoreNode::BytesAtom(numerator);
            bridge_append_unattached_shape(bridge, numerator_kind);
            bridge_append_unattached_exact(bridge, numerator_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, numerator_kind);
            if let Some((after_numerator, numerator_handle)) = bridge_append_unattached(
                bridge, numerator_kind,
            ) {
                let denominator_kind = score::ScoreNode::BytesAtom(denominator);
                bridge_append_unattached_shape(after_numerator, denominator_kind);
                bridge_append_unattached_exact(after_numerator, denominator_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_numerator, denominator_kind,
                );
                if let Some((after_denominator, denominator_handle)) =
                    bridge_append_unattached(after_numerator, denominator_kind)
                {
                    bridge_constructor_shape(
                        after_denominator, 16i32,
                        seq![numerator_handle, denominator_handle],
                    );
                    bridge_score_step_shape_transitive(
                        bridge, after_numerator, after_denominator,
                    );
                    bridge_score_step_shape_transitive(bridge, after_denominator, next);
                }
            }
        }
    }
}

pub proof fn bridge_expr_fixed_point_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_fixed_point_step(bridge) {
        None => true,
        Some((next, _, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _, _)) = bridge_expr_fixed_point_step(bridge) {
        if let Some((unscaled, scale)) = bridge_expr_fixed_point_parts(
            bridge.admission.frames.last(),
        ) {
            let unscaled_kind = score::ScoreNode::BytesAtom(unscaled);
            bridge_append_unattached_shape(bridge, unscaled_kind);
            bridge_append_unattached_exact(bridge, unscaled_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, unscaled_kind);
            if let Some((after_unscaled, unscaled_handle)) = bridge_append_unattached(
                bridge, unscaled_kind,
            ) {
                let scale_kind = score::ScoreNode::Integer(scale as i64);
                bridge_append_unattached_shape(after_unscaled, scale_kind);
                bridge_append_unattached_exact(after_unscaled, scale_kind);
                bridge_append_unattached_preserves_admission_wf(after_unscaled, scale_kind);
                if let Some((after_scale, scale_handle)) = bridge_append_unattached(
                    after_unscaled, scale_kind,
                ) {
                    let scale_node_kind = score::ScoreNode::Node(seq![scale_handle]);
                    bridge_append_unattached_shape(after_scale, scale_node_kind);
                    bridge_append_unattached_exact(after_scale, scale_node_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_scale, scale_node_kind,
                    );
                    if let Some((after_scale_node, scale_node_handle)) =
                        bridge_append_unattached(after_scale, scale_node_kind)
                    {
                        bridge_constructor_shape(
                            after_scale_node, 17i32,
                            seq![unscaled_handle, scale_node_handle],
                        );
                        bridge_score_step_shape_transitive(
                            bridge, after_unscaled, after_scale,
                        );
                        bridge_score_step_shape_transitive(
                            bridge, after_scale, after_scale_node,
                        );
                        bridge_score_step_shape_transitive(bridge, after_scale_node, next);
                    }
                }
            }
        }
    }
}

pub proof fn bridge_new_empty_collections_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_new_empty_collections_step(bridge) {
        None => true,
        Some((next, _, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _, _)) = bridge_new_empty_collections_step(bridge) {
        if let Some((bind_count, body)) = bridge_new_empty_collections_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            let absent_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_shape(bridge, absent_kind);
            bridge_append_unattached_exact(bridge, absent_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, absent_kind);
            if let Some((after_uri, uri_handle)) = bridge_append_unattached(
                bridge, absent_kind,
            ) {
                bridge_append_unattached_shape(after_uri, absent_kind);
                bridge_append_unattached_exact(after_uri, absent_kind);
                bridge_append_unattached_preserves_admission_wf(after_uri, absent_kind);
                if let Some((after_injection, injection_handle)) = bridge_append_unattached(
                    after_uri, absent_kind,
                ) {
                    let count_kind = score::ScoreNode::Integer(bind_count);
                    bridge_append_unattached_shape(after_injection, count_kind);
                    bridge_append_unattached_exact(after_injection, count_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_injection, count_kind,
                    );
                    if let Some((after_count, count_handle)) =
                        bridge_append_unattached(after_injection, count_kind)
                    {
                        bridge_constructor_shape(
                            after_count, 303i32,
                            seq![count_handle, uri_handle, injection_handle, body],
                        );
                        bridge_score_step_shape_transitive(bridge, after_uri, after_injection);
                        bridge_score_step_shape_transitive(bridge, after_injection, after_count);
                        bridge_score_step_shape_transitive(bridge, after_count, next);
                    }
                }
            }
        }
    }
}

pub proof fn bridge_new_one_injection_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_new_one_injection_step(bridge) {
        None => true,
        Some((next, _, _, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _, _, _)) = bridge_new_one_injection_step(bridge) {
        if let Some((bind_count, body, key_bytes, value_handle)) =
            bridge_new_one_injection_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            )
        {
            let uri_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_shape(bridge, uri_kind);
            bridge_append_unattached_exact(bridge, uri_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, uri_kind);
            if let Some((after_uri, uri_handle)) = bridge_append_unattached(
                bridge, uri_kind,
            ) {
                let key_kind = score::ScoreNode::StringAtom(key_bytes);
                bridge_append_unattached_shape(after_uri, key_kind);
                bridge_append_unattached_exact(after_uri, key_kind);
                bridge_append_unattached_preserves_admission_wf(after_uri, key_kind);
                if let Some((after_key, key_handle)) = bridge_append_unattached(
                    after_uri, key_kind,
                ) {
                    let injection_kind = score::ScoreNode::Node(
                        seq![key_handle, value_handle],
                    );
                    bridge_append_unattached_shape(after_key, injection_kind);
                    bridge_append_unattached_exact(after_key, injection_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_key, injection_kind,
                    );
                    if let Some((after_injection, injection_handle)) =
                        bridge_append_unattached(after_key, injection_kind)
                    {
                        let count_kind = score::ScoreNode::Integer(bind_count);
                        bridge_append_unattached_shape(after_injection, count_kind);
                        bridge_append_unattached_exact(after_injection, count_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_injection, count_kind,
                        );
                        if let Some((after_count, count_handle)) =
                            bridge_append_unattached(after_injection, count_kind)
                        {
                            bridge_constructor_shape(
                                after_count, 303i32,
                                seq![count_handle, uri_handle, injection_handle, body],
                            );
                            bridge_score_step_shape_transitive(bridge, after_uri, after_key);
                            bridge_score_step_shape_transitive(bridge, after_key, after_injection);
                            bridge_score_step_shape_transitive(bridge, after_injection, after_count);
                            bridge_score_step_shape_transitive(bridge, after_count, next);
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_new_one_absent_injection_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_new_one_absent_injection_step(bridge) {
        None => true,
        Some((next, _, _, _, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _, _, _, _)) =
        bridge_new_one_absent_injection_step(bridge)
    {
        if let Some((bind_count, body, key_bytes)) =
            bridge_new_one_absent_injection_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            )
        {
            let uri_kind = score::ScoreNode::Integer(0i64);
            bridge_append_unattached_shape(bridge, uri_kind);
            bridge_append_unattached_exact(bridge, uri_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, uri_kind);
            if let Some((after_uri, uri_handle)) = bridge_append_unattached(
                bridge, uri_kind,
            ) {
                bridge_empty_par_unattached_shape(after_uri);
                bridge_empty_par_unattached_exact(after_uri);
                if let Some((after_empty, _, _, empty_handle)) =
                    bridge_empty_par_unattached_step(after_uri)
                {
                    let key_kind = score::ScoreNode::StringAtom(key_bytes);
                    bridge_append_unattached_shape(after_empty, key_kind);
                    bridge_append_unattached_exact(after_empty, key_kind);
                    bridge_append_unattached_preserves_admission_wf(after_empty, key_kind);
                    if let Some((after_key, key_handle)) = bridge_append_unattached(
                        after_empty, key_kind,
                    ) {
                        let injection_kind = score::ScoreNode::Node(
                            seq![key_handle, empty_handle],
                        );
                        bridge_append_unattached_shape(after_key, injection_kind);
                        bridge_append_unattached_exact(after_key, injection_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_key, injection_kind,
                        );
                        if let Some((after_injection, injection_handle)) =
                            bridge_append_unattached(after_key, injection_kind)
                        {
                            let count_kind = score::ScoreNode::Integer(bind_count);
                            bridge_append_unattached_shape(after_injection, count_kind);
                            bridge_append_unattached_exact(after_injection, count_kind);
                            bridge_append_unattached_preserves_admission_wf(
                                after_injection, count_kind,
                            );
                            if let Some((after_count, count_handle)) =
                                bridge_append_unattached(after_injection, count_kind)
                            {
                                bridge_constructor_shape(
                                    after_count, 303i32,
                                    seq![count_handle, uri_handle, injection_handle, body],
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_uri, after_empty,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_empty, after_key,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_key, after_injection,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_injection, after_count,
                                );
                                bridge_score_step_shape_transitive(bridge, after_count, next);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_new_one_uri_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_new_one_uri_step(bridge) {
        None => true,
        Some((next, _, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _, _)) = bridge_new_one_uri_step(bridge) {
        if let Some((bind_count, body, uri_bytes)) = bridge_new_one_uri_parts(
            bridge.admission.frames.last(), bridge.admission.arena,
        ) {
            let uri_kind = score::ScoreNode::StringAtom(uri_bytes);
            bridge_append_unattached_shape(bridge, uri_kind);
            bridge_append_unattached_exact(bridge, uri_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, uri_kind);
            if let Some((after_uri, uri_handle)) = bridge_append_unattached(
                bridge, uri_kind,
            ) {
                let absent_kind = score::ScoreNode::Integer(0i64);
                bridge_append_unattached_shape(after_uri, absent_kind);
                bridge_append_unattached_exact(after_uri, absent_kind);
                bridge_append_unattached_preserves_admission_wf(after_uri, absent_kind);
                if let Some((after_injection, injection_handle)) = bridge_append_unattached(
                    after_uri, absent_kind,
                ) {
                    let count_kind = score::ScoreNode::Integer(bind_count);
                    bridge_append_unattached_shape(after_injection, count_kind);
                    bridge_append_unattached_exact(after_injection, count_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_injection, count_kind,
                    );
                    if let Some((after_count, count_handle)) =
                        bridge_append_unattached(after_injection, count_kind)
                    {
                        bridge_constructor_shape(
                            after_count, 303i32,
                            seq![count_handle, uri_handle, injection_handle, body],
                        );
                        bridge_score_step_shape_transitive(bridge, after_uri, after_injection);
                        bridge_score_step_shape_transitive(bridge, after_injection, after_count);
                        bridge_score_step_shape_transitive(bridge, after_count, next);
                    }
                }
            }
        }
    }
}

pub proof fn bridge_expr_payload_forward_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_expr_payload_forward_step(bridge) {
        None => true,
        Some((next, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((_, handle)) = bridge_expr_payload_forward_step(bridge) {
        bridge_attach_score_shape(
            bridge, handle, bridge.admission.frames.last().body_id,
        );
    }
}

pub proof fn bridge_connective_varref_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_connective_varref_step(bridge) {
        None => true,
        Some((next, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _)) = bridge_connective_varref_step(bridge) {
        if let Some((index, depth)) = bridge_connective_varref_parts(
            bridge.admission.frames.last(),
        ) {
            let index_kind = score::ScoreNode::Integer(index);
            bridge_append_unattached_shape(bridge, index_kind);
            bridge_append_unattached_exact(bridge, index_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, index_kind);
            if let Some((after_index, index_handle)) = bridge_append_unattached(
                bridge, index_kind,
            ) {
                let depth_kind = score::ScoreNode::Integer(depth);
                bridge_append_unattached_shape(after_index, depth_kind);
                bridge_append_unattached_exact(after_index, depth_kind);
                bridge_append_unattached_preserves_admission_wf(after_index, depth_kind);
                if let Some((after_depth, depth_handle)) = bridge_append_unattached(
                    after_index, depth_kind,
                ) {
                    bridge_constructor_shape(
                        after_depth, 403i32, seq![index_handle, depth_handle],
                    );
                    bridge_score_step_shape_transitive(bridge, after_index, after_depth);
                    bridge_score_step_shape_transitive(bridge, after_depth, next);
                }
            }
        }
    }
}

pub proof fn bridge_unforgeable_bytes_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_unforgeable_bytes_step(bridge) {
        None => true,
        Some((next, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _)) = bridge_unforgeable_bytes_step(bridge) {
        if let Some((constructor, value)) = bridge_unforgeable_bytes_parts(
            bridge.admission.frames.last(),
        ) {
            let bytes_kind = score::ScoreNode::BytesAtom(value);
            bridge_append_unattached_shape(bridge, bytes_kind);
            bridge_append_unattached_exact(bridge, bytes_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, bytes_kind);
            if let Some((after_bytes, bytes_handle)) = bridge_append_unattached(
                bridge, bytes_kind,
            ) {
                bridge_constructor_shape(after_bytes, constructor, seq![bytes_handle]);
                bridge_score_step_shape_transitive(bridge, after_bytes, next);
            }
        }
    }
}

pub proof fn bridge_empty_par_unattached_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_empty_par_unattached_step(bridge) {
        None => true,
        Some((after_empty, _, _, _)) => bridge_score_step_shape(bridge, after_empty),
    },
{
    let flag_kind = score::ScoreNode::Integer(0i64);
    bridge_append_unattached_shape(bridge, flag_kind);
    bridge_append_unattached_exact(bridge, flag_kind);
    bridge_append_unattached_preserves_admission_wf(bridge, flag_kind);
    if let Some((after_flag, flag_handle)) = bridge_append_unattached(bridge, flag_kind) {
        let constructor_kind = score::ScoreNode::Integer(999i64);
        bridge_append_unattached_shape(after_flag, constructor_kind);
        bridge_append_unattached_exact(after_flag, constructor_kind);
        bridge_append_unattached_preserves_admission_wf(after_flag, constructor_kind);
        if let Some((after_constructor, constructor_handle)) =
            bridge_append_unattached(after_flag, constructor_kind)
        {
            let empty_kind = score::ScoreNode::Node(seq![constructor_handle, flag_handle]);
            bridge_append_unattached_shape(after_constructor, empty_kind);
            bridge_append_unattached_exact(after_constructor, empty_kind);
            bridge_append_unattached_preserves_admission_wf(after_constructor, empty_kind);
            if let Some((after_empty, _)) =
                bridge_append_unattached(after_constructor, empty_kind)
            {
                bridge_score_step_shape_transitive(bridge, after_flag, after_constructor);
                bridge_score_step_shape_transitive(bridge, after_constructor, after_empty);
            }
        }
    }
}

pub proof fn bridge_receive_present_condition_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_receive_present_condition_step(bridge) {
        None => true,
        Some((next, _, _, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _, _, _)) = bridge_receive_present_condition_step(bridge) {
        if let Some((persistent, peek, binds, body, bind_count,
            connective_used, condition, cost)) =
            bridge_receive_present_condition_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            )
        {
            let persistent_kind = score::ScoreNode::Integer(
                if persistent { 1i64 } else { 0i64 },
            );
            bridge_append_unattached_shape(bridge, persistent_kind);
            bridge_append_unattached_exact(bridge, persistent_kind);
            bridge_append_unattached_preserves_admission_wf(bridge, persistent_kind);
            if let Some((after_persistent, persistent_handle)) =
                bridge_append_unattached(bridge, persistent_kind)
            {
                let peek_kind = score::ScoreNode::Integer(if peek { 1i64 } else { 0i64 });
                bridge_append_unattached_shape(after_persistent, peek_kind);
                bridge_append_unattached_exact(after_persistent, peek_kind);
                bridge_append_unattached_preserves_admission_wf(after_persistent, peek_kind);
                if let Some((after_peek, peek_handle)) =
                    bridge_append_unattached(after_persistent, peek_kind)
                {
                    let count_kind = score::ScoreNode::Integer(bind_count);
                    bridge_append_unattached_shape(after_peek, count_kind);
                    bridge_append_unattached_exact(after_peek, count_kind);
                    bridge_append_unattached_preserves_admission_wf(after_peek, count_kind);
                    if let Some((after_count, count_handle)) =
                        bridge_append_unattached(after_peek, count_kind)
                    {
                        let connective_kind = score::ScoreNode::Integer(
                            if connective_used { 1i64 } else { 0i64 },
                        );
                        bridge_append_unattached_shape(after_count, connective_kind);
                        bridge_append_unattached_exact(after_count, connective_kind);
                        bridge_append_unattached_preserves_admission_wf(
                            after_count, connective_kind,
                        );
                        if let Some((after_connective, connective_handle)) =
                            bridge_append_unattached(after_count, connective_kind)
                        {
                            let children = seq![persistent_handle, peek_handle] + binds
                                + seq![body, count_handle, connective_handle, condition] + cost;
                            bridge_constructor_shape(after_connective, 301i32, children);
                            bridge_score_step_shape_transitive(
                                bridge, after_persistent, after_peek,
                            );
                            bridge_score_step_shape_transitive(
                                bridge, after_peek, after_count,
                            );
                            bridge_score_step_shape_transitive(
                                bridge, after_count, after_connective,
                            );
                            bridge_score_step_shape_transitive(
                                bridge, after_connective, next,
                            );
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_receive_absent_condition_shape(bridge: ScoreAdmissionBridge)
    requires
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
    ensures match bridge_receive_absent_condition_step(bridge) {
        None => true,
        Some((next, _, _, _, _, _, _, _)) => bridge_score_step_shape(bridge, next),
    },
{
    if let Some((next, _, _, _, _, _, _, _)) =
        bridge_receive_absent_condition_step(bridge)
    {
        if let Some((persistent, peek, binds, body, bind_count,
            connective_used, cost)) =
            bridge_receive_absent_condition_parts(
                bridge.admission.frames.last(), bridge.admission.arena,
            )
        {
            bridge_empty_par_unattached_shape(bridge);
            bridge_empty_par_unattached_exact(bridge);
            if let Some((after_empty, _, _, empty_handle)) =
                bridge_empty_par_unattached_step(bridge)
            {
                let persistent_kind = score::ScoreNode::Integer(
                    if persistent { 1i64 } else { 0i64 },
                );
                bridge_append_unattached_shape(after_empty, persistent_kind);
                bridge_append_unattached_exact(after_empty, persistent_kind);
                bridge_append_unattached_preserves_admission_wf(
                    after_empty, persistent_kind,
                );
                if let Some((after_persistent, persistent_handle)) =
                    bridge_append_unattached(after_empty, persistent_kind)
                {
                    let peek_kind = score::ScoreNode::Integer(if peek { 1i64 } else { 0i64 });
                    bridge_append_unattached_shape(after_persistent, peek_kind);
                    bridge_append_unattached_exact(after_persistent, peek_kind);
                    bridge_append_unattached_preserves_admission_wf(
                        after_persistent, peek_kind,
                    );
                    if let Some((after_peek, peek_handle)) =
                        bridge_append_unattached(after_persistent, peek_kind)
                    {
                        let count_kind = score::ScoreNode::Integer(bind_count);
                        bridge_append_unattached_shape(after_peek, count_kind);
                        bridge_append_unattached_exact(after_peek, count_kind);
                        bridge_append_unattached_preserves_admission_wf(after_peek, count_kind);
                        if let Some((after_count, count_handle)) =
                            bridge_append_unattached(after_peek, count_kind)
                        {
                            let connective_kind = score::ScoreNode::Integer(
                                if connective_used { 1i64 } else { 0i64 },
                            );
                            bridge_append_unattached_shape(after_count, connective_kind);
                            bridge_append_unattached_exact(after_count, connective_kind);
                            bridge_append_unattached_preserves_admission_wf(
                                after_count, connective_kind,
                            );
                            if let Some((after_connective, connective_handle)) =
                                bridge_append_unattached(after_count, connective_kind)
                            {
                                let children = seq![persistent_handle, peek_handle] + binds
                                    + seq![body, count_handle, connective_handle, empty_handle]
                                    + cost;
                                bridge_constructor_shape(after_connective, 301i32, children);
                                bridge_score_step_shape_transitive(
                                    bridge, after_empty, after_persistent,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_persistent, after_peek,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_peek, after_count,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_count, after_connective,
                                );
                                bridge_score_step_shape_transitive(
                                    bridge, after_connective, next,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

pub proof fn bridge_score_source_step_preserves_shape(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        bridge_score_source_step(before, after),
    ensures bridge_score_step_shape(before, after),
{
    if let Some((next, _, _, _)) = bridge_raw_par_step(before) {
        bridge_raw_par_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_cost_atom_absent_step(before) {
        bridge_constructor_shape(before, 505i32, Seq::empty());
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_cost_atom_bytes_step(before) {
        bridge_cost_atom_bytes_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_cost_bound_step(before) {
        bridge_cost_bound_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _)) = bridge_cost_atom_bound_forward_step(before) {
        bridge_cost_atom_bound_forward_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_cost_signature_step(before) {
        if let Some(atoms) = bridge_cost_layers(
            before.admission.frames.last().returns, before.admission.arena,
        ) {
            bridge_constructor_shape(before, 501i32, atoms);
        }
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _)) = bridge_cost_region_step(before) {
        bridge_cost_region_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_cost_token_stack_step(before) {
        if let Some(layers) = bridge_cost_layers(
            before.admission.frames.last().returns, before.admission.arena,
        ) {
            bridge_constructor_shape(before, 508i32, layers);
        }
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_bundle_step(before) {
        if let Some((body, read, write)) = bridge_bundle_parts(
            before.admission.frames.last(), before.admission.arena,
        ) {
            bridge_constructor_shape(
                before, bridge_bundle_constructor(read, write), seq![body],
            );
        }
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _)) = bridge_if_step(before) {
        bridge_if_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _)) = bridge_send_step(before) {
        bridge_send_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _)) = bridge_match_step(before) {
        bridge_match_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_absent_specialized_constructor_step(before) {
        bridge_absent_specialized_constructor_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _)) = bridge_absent_connective_step(before) {
        bridge_absent_connective_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_receive_bind_step(before) {
        bridge_receive_bind_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _)) = bridge_match_case_step(before) {
        bridge_match_case_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _)) = bridge_bool_oneof_step(before) {
        bridge_bool_oneof_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _)) = bridge_expr_atom_step(before) {
        bridge_expr_atom_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _)) = bridge_expr_numeric_step(before) {
        bridge_expr_numeric_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _)) = bridge_expr_big_rational_step(before) {
        bridge_expr_big_rational_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _)) = bridge_expr_fixed_point_step(before) {
        bridge_expr_fixed_point_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_sys_auth_token_step(before) {
        bridge_constructor_shape(before, 12i32, Seq::empty());
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _)) = bridge_new_empty_collections_step(before) {
        bridge_new_empty_collections_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _, _)) = bridge_new_one_injection_step(before) {
        bridge_new_one_injection_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _, _, _)) = bridge_new_one_absent_injection_step(before) {
        bridge_new_one_absent_injection_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _)) = bridge_new_one_uri_step(before) {
        bridge_new_one_uri_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _)) = bridge_new_collections_step(before) {
        bridge_new_collections_exact(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _)) = bridge_expr_payload_forward_step(before) {
        bridge_expr_payload_forward_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _)) = bridge_expr_structural_step(before) {
        bridge_expr_structural_exact(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _)) = bridge_evar_step(before) {
        bridge_evar_exact(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _)) = bridge_ordered_sequence_step(before) {
        bridge_ordered_sequence_exact(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _)) = bridge_emethod_step(before) {
        bridge_emethod_exact(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _)) = bridge_emap_finish_step(before) {
        bridge_emap_complete_finish_source_exact(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_connective_not_step(before) {
        if let Some(child) = bridge_par_return_handle(
            before.admission.frames.last().returns[0], before.admission.arena, 0, 3,
        ) {
            bridge_constructor_shape(before, 400i32, seq![child]);
        }
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _)) = bridge_connective_varref_step(before) {
        bridge_connective_varref_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_connective_collection_step(before) {
        if let Some((constructor, children)) = bridge_connective_collection_parts(
            before.admission.frames.last(), before.admission.arena,
        ) {
            bridge_constructor_shape(before, constructor, children);
        }
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _)) = bridge_unforgeable_bytes_step(before) {
        bridge_unforgeable_bytes_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _, _)) = bridge_receive_present_condition_step(before) {
        bridge_receive_present_condition_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _, _, _, _, _, _)) = bridge_receive_absent_condition_step(before) {
        bridge_receive_absent_condition_shape(before);
        if bridge_same(next, after) { return; }
    }
    if let Some((next, _, _)) = bridge_nonsemantic_step(before) {
        bridge_constructor_shape(before, i32::MAX, Seq::empty());
        if bridge_same(next, after) { return; }
    }
    assert(false);
}

pub proof fn bridge_score_source_step_preserves_driver_counts(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        bridge_score_source_step(before, after),
    ensures
        after.admission.pushed == before.admission.pushed,
        after.admission.completed == before.admission.completed,
{
    bridge_score_source_step_preserves_shape(before, after);
}

pub proof fn bridge_score_source_step_preserves_par_source_trace(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        admission::admission_par_source_trace_invariant(before.admission),
        bridge_score_source_step(before, after),
    ensures admission::admission_par_source_trace_invariant(after.admission),
{
    bridge_score_source_step_preserves_shape(before, after);
    bridge_score_step_shape_preserves_par_source_trace(before, after);
}

pub proof fn bridge_message_driver_step_preserves_exact(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        bridge_message_driver_step(before, after),
    ensures
        bridge_well_formed(after),
        admission::admission_state_wf(after.admission),
{
    let action = choose|action: admission::ConcreteFrameActionModel|
        match #[trigger] admission::concrete_message_driver_step(before.admission, action) {
            Some(admission::ConcreteFrameOutcomeModel::Continued { state })
            | Some(admission::ConcreteFrameOutcomeModel::Pushed { state })
            | Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) =>
                bridge_admission_only_step(before, after, Some(state)),
            _ => false,
        };
    admission::concrete_message_driver_preserves_wf_and_arena(before.admission, action);
    match admission::concrete_message_driver_step(before.admission, action) {
        Some(admission::ConcreteFrameOutcomeModel::Continued { state })
        | Some(admission::ConcreteFrameOutcomeModel::Pushed { state })
        | Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => {
            assert(after.admission == state);
            assert(after.transcript == before.transcript);
            assert(after.admission.arena == before.admission.arena);
            assert(admission::admission_state_wf(after.admission));
            assert forall|i: int| 0 <= i < after.admission.arena.len()
                implies after.admission.arena[i].encoded_len
                    == after.transcript.entries[i].encoded_len
                    && after.admission.arena[i].transcript_bytes
                        == after.transcript.entries[i].transcript by {}
        },
        _ => assert(false),
    }
}

pub proof fn bridge_message_driver_step_preserves_par_source_trace(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        admission::admission_par_source_trace_invariant(before.admission),
        bridge_message_driver_step(before, after),
    ensures admission::admission_par_source_trace_invariant(after.admission),
{
    let action = choose|action: admission::ConcreteFrameActionModel|
        match #[trigger] admission::concrete_message_driver_step(before.admission, action) {
            Some(admission::ConcreteFrameOutcomeModel::Continued { state })
            | Some(admission::ConcreteFrameOutcomeModel::Pushed { state })
            | Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) =>
                bridge_admission_only_step(before, after, Some(state)),
            _ => false,
        };
    admission::concrete_message_driver_preserves_par_source_trace(
        before.admission, action,
    );
    assert(after.admission == match admission::concrete_message_driver_step(
        before.admission, action,
    ) {
        Some(admission::ConcreteFrameOutcomeModel::Continued { state })
        | Some(admission::ConcreteFrameOutcomeModel::Pushed { state })
        | Some(admission::ConcreteFrameOutcomeModel::ParentAccepted { state }) => state,
        _ => before.admission,
    });
}

pub proof fn bridge_source_step_preserves_exact(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        bridge_source_step(before, after),
    ensures
        bridge_well_formed(after),
        admission::admission_state_wf(after.admission),
{
    if bridge_score_source_step(before, after) {
        bridge_score_source_step_preserves_exact(before, after);
    } else {
        bridge_message_driver_step_preserves_exact(before, after);
    }
}

pub proof fn bridge_source_step_preserves_par_source_trace(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        admission::admission_par_source_trace_invariant(before.admission),
        bridge_source_step(before, after),
    ensures admission::admission_par_source_trace_invariant(after.admission),
{
    if bridge_score_source_step(before, after) {
        bridge_score_source_step_preserves_par_source_trace(before, after);
    } else {
        bridge_message_driver_step_preserves_par_source_trace(before, after);
    }
}

#[verifier::opaque]
pub open spec fn bridge_trace_edge(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
) -> bool {
    bridge_source_step(before, after)
}

pub proof fn bridge_trace_edge_is_source_step(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires bridge_trace_edge(before, after),
    ensures bridge_source_step(before, after),
{
    reveal(bridge_trace_edge);
}

pub open spec fn bridge_initial_trace(
    root: admission::AdmissionFrameState, trace: Seq<ScoreAdmissionBridge>,
) -> bool {
    &&& admission::admission_root_initial(root)
    &&& trace.len() > 0
    &&& bridge_same(trace[0], bridge_initial(root))
    &&& forall|i: int| 0 <= i < trace.len() - 1
        ==> #[trigger] bridge_trace_edge(trace[i], trace[i + 1])
}

pub open spec fn bridge_reducer_reachable(bridge: ScoreAdmissionBridge) -> bool {
    exists|root: admission::AdmissionFrameState, trace: Seq<ScoreAdmissionBridge>|
        bridge_initial_trace(root, trace)
            && bridge_same(trace[trace.len() - 1], bridge)
}

pub open spec fn bridge_trace_reaches(
    root: admission::AdmissionFrameState, bridge: ScoreAdmissionBridge,
) -> bool {
    exists|trace: Seq<ScoreAdmissionBridge>|
        #[trigger] bridge_initial_trace(root, trace)
            && bridge_same(trace[trace.len() - 1], bridge)
}

pub proof fn bridge_initial_reachable(root: admission::AdmissionFrameState)
    requires admission::admission_root_initial(root),
    ensures bridge_reducer_reachable(bridge_initial(root)),
{
    let trace = seq![bridge_initial(root)];
    assert(bridge_initial_trace(root, trace));
}

pub proof fn bridge_source_step_extends_reachability(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_reducer_reachable(before),
        bridge_source_step(before, after),
    ensures bridge_reducer_reachable(after),
{
    let (root, trace) = choose|root: admission::AdmissionFrameState,
        trace: Seq<ScoreAdmissionBridge>|
        bridge_initial_trace(root, trace)
            && bridge_same(trace[trace.len() - 1], before);
    assert(bridge_initial_trace(root, trace));
    assert(bridge_same(trace[trace.len() - 1], before));
    let extended = trace.push(after);
    assert forall|i: int| 0 <= i < extended.len() - 1
        implies #[trigger] bridge_trace_edge(extended[i], extended[i + 1]) by {
        if i < trace.len() - 1 {
            assert(extended[i] == trace[i]);
            assert(extended[i + 1] == trace[i + 1]);
        } else {
            assert(i == trace.len() - 1);
            assert(extended[i] == before);
            assert(extended[i + 1] == after);
            reveal(bridge_trace_edge);
        }
    }
    assert(bridge_initial_trace(root, extended));
}

pub proof fn bridge_trace_prefix_preserves_exact(
    root: admission::AdmissionFrameState,
    trace: Seq<ScoreAdmissionBridge>, index: int,
)
    requires
        bridge_initial_trace(root, trace),
        0 <= index < trace.len(),
    ensures
        bridge_well_formed(trace[index]),
        admission::admission_state_wf(trace[index].admission),
    decreases index,
{
    if index == 0 {
        bridge_initial_exact(root);
        admission::admission_initial_wf(root);
        assert(bridge_same(trace[0], bridge_initial(root)));
    } else {
        bridge_trace_prefix_preserves_exact(root, trace, index - 1);
        assert(bridge_initial_trace(root, trace));
        assert(forall|i: int| 0 <= i < trace.len() - 1
            ==> #[trigger] bridge_trace_edge(trace[i], trace[i + 1]));
        let previous: int = index - 1;
        assert(0 <= previous < trace.len() - 1);
        assert(bridge_trace_edge(trace[previous], trace[previous + 1]));
        assert(previous + 1 == index);
        assert(trace[previous + 1] == trace[index]);
        assert(bridge_trace_edge(trace[previous], trace[index]));
        bridge_trace_edge_is_source_step(trace[index - 1], trace[index]);
        bridge_source_step_preserves_exact(trace[index - 1], trace[index]);
    }
}

pub proof fn bridge_trace_prefix_preserves_par_source_trace(
    root: admission::AdmissionFrameState,
    trace: Seq<ScoreAdmissionBridge>, index: int,
)
    requires
        bridge_initial_trace(root, trace),
        0 <= index < trace.len(),
    ensures admission::admission_par_source_trace_invariant(
        trace[index].admission,
    ),
    decreases index,
{
    if index == 0 {
        admission::admission_par_initial_trace_invariant(root);
        assert(bridge_same(trace[0], bridge_initial(root)));
    } else {
        bridge_trace_prefix_preserves_par_source_trace(root, trace, index - 1);
        bridge_trace_prefix_preserves_exact(root, trace, index - 1);
        assert(forall|i: int| 0 <= i < trace.len() - 1
            ==> #[trigger] bridge_trace_edge(trace[i], trace[i + 1]));
        let previous: int = index - 1;
        assert(0 <= previous < trace.len() - 1);
        assert(bridge_trace_edge(trace[previous], trace[previous + 1]));
        assert(previous + 1 == index);
        assert(trace[previous + 1] == trace[index]);
        assert(bridge_trace_edge(trace[previous], trace[index]));
        bridge_trace_edge_is_source_step(trace[index - 1], trace[index]);
        bridge_source_step_preserves_par_source_trace(
            trace[index - 1], trace[index],
        );
    }
}

pub proof fn bridge_reachable_has_par_source_trace(
    bridge: ScoreAdmissionBridge,
)
    requires bridge_reducer_reachable(bridge),
    ensures admission::admission_par_source_trace_invariant(bridge.admission),
{
    let (root, trace) = choose|root: admission::AdmissionFrameState,
        trace: Seq<ScoreAdmissionBridge>|
        bridge_initial_trace(root, trace)
            && bridge_same(trace[trace.len() - 1], bridge);
    bridge_trace_prefix_preserves_par_source_trace(root, trace, trace.len() - 1);
}

pub proof fn bridge_reachable_emap_keys_have_source_identity(
    bridge: ScoreAdmissionBridge,
)
    requires
        bridge_reducer_reachable(bridge),
        bridge_emap_finish_source(bridge).is_some(),
    ensures forall|i: int|
        0 <= i < bridge_emap_finish_source(bridge).unwrap().pairs.len()
            && bridge_emap_finish_source(bridge).unwrap().pairs[i].key.is_some()
        ==> admission::admission_par_identity_from_source(
            bridge_emap_finish_source(bridge).unwrap().pairs[i]
                .key.unwrap().body_bytes,
        ).is_some(),
{
    bridge_reachable_has_par_source_trace(bridge);
    let frame = bridge.admission.frames.last();
    let source = bridge_emap_finish_source(bridge).unwrap();
    admission::admission_emap_returned_pairs_have_source_key_identity(
        frame.returns, bridge.admission.arena,
    );
    bridge_emap_pairs_match_admission_source(
        frame.returns, bridge.admission.arena,
    );
    let admitted = admission::admission_emap_pairs_from_returns(
        frame.returns, bridge.admission.arena,
    ).unwrap();
    assert(admitted == bridge_emap_pairs_as_admission(source.pairs));
    assert forall|i: int| 0 <= i < source.pairs.len()
        && source.pairs[i].key.is_some()
        implies admission::admission_par_identity_from_source(
            source.pairs[i].key.unwrap().body_bytes,
        ).is_some() by {
        assert(admitted[i] == bridge_emap_pair_as_admission(source.pairs[i]));
    }
}

pub proof fn bridge_reachable_has_exact_binding(
    bridge: ScoreAdmissionBridge,
)
    requires bridge_reducer_reachable(bridge),
    ensures
        bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
{
    let (root, trace) = choose|root: admission::AdmissionFrameState,
        trace: Seq<ScoreAdmissionBridge>|
        bridge_initial_trace(root, trace)
            && bridge_same(trace[trace.len() - 1], bridge);
    assert(bridge_initial_trace(root, trace));
    assert(bridge_same(trace[trace.len() - 1], bridge));
    bridge_trace_prefix_preserves_exact(root, trace, trace.len() - 1);
    assert(bridge_well_formed(trace[trace.len() - 1]));
    assert(admission::admission_state_wf(trace[trace.len() - 1].admission));
}

pub open spec fn bridge_component_score_eof_scope(bridge: ScoreAdmissionBridge) -> bool {
    bridge.admission.frames.len() > 0
        && bridge.admission.frames.last().cursor
            == bridge.admission.frames.last().region_end
        && bridge.admission.frames.last().waiting_id.is_none()
        && bridge.admission.frames.last().score_handle.is_none()
        && match bridge.admission.frames.last().family {
            admission::AdmissionFamily::Send
            | admission::AdmissionFamily::Receive
            | admission::AdmissionFamily::New
            | admission::AdmissionFamily::Match
            | admission::AdmissionFamily::Bundle
            | admission::AdmissionFamily::If
            | admission::AdmissionFamily::CostTokenStack
            | admission::AdmissionFamily::Expr
            | admission::AdmissionFamily::Connective
            | admission::AdmissionFamily::GUnforgeable => true,
            _ => false,
        }
}

pub proof fn bridge_reachable_component_score_eof_source_finish_unique(
    before: ScoreAdmissionBridge, after: ScoreAdmissionBridge,
)
    requires
        bridge_reducer_reachable(before),
        bridge_component_score_eof_scope(before),
        bridge_score_source_step(before, after),
    ensures
        bridge_score_finish_selected(before).is_some(),
        bridge_same(bridge_score_finish_selected(before).unwrap(), after),
        bridge_score_source_step(
            before, bridge_score_finish_selected(before).unwrap(),
        ),
        bridge_well_formed(bridge_score_finish_selected(before).unwrap()),
        admission::admission_state_wf(
            bridge_score_finish_selected(before).unwrap().admission,
        ),
        bridge_score_step_shape(
            before, bridge_score_finish_selected(before).unwrap(),
        ),
        forall|other: ScoreAdmissionBridge|
            bridge_score_source_step(before, other) ==> bridge_same(after, other),
{
    bridge_reachable_has_exact_binding(before);
    let index = before.admission.frames.len() - 1;
    assert(0 <= index < before.admission.frames.len());
    assert(admission::admission_frame_wf(before.admission.frames[index]));
    let top = before.admission.frames.last();
    assert(admission::admission_frame_wf(top));
    match top.family {
        admission::AdmissionFamily::Send
        | admission::AdmissionFamily::Bundle
        | admission::AdmissionFamily::If
        | admission::AdmissionFamily::Match => {
            if admission::admission_family_complete(top) {
                assert(bridge_primary_component_frame(before));
                bridge_primary_component_source_step_has_unique_finish(before, after);
            } else {
                assert(bridge_nonsemantic_incomplete_frame(before));
                bridge_nonsemantic_incomplete_source_step_has_unique_finish(before, after);
            }
        },
        admission::AdmissionFamily::Receive => {
            if admission::admission_family_complete(top) {
                assert(bridge_receive_complete_frame(before));
                bridge_receive_complete_source_step_has_unique_finish(before, after);
            } else {
                assert(bridge_nonsemantic_incomplete_frame(before));
                bridge_nonsemantic_incomplete_source_step_has_unique_finish(before, after);
            }
        },
        admission::AdmissionFamily::New => {
            if admission::admission_family_complete(top) {
                assert(bridge_new_complete_frame(before));
                bridge_new_complete_source_step_has_unique_finish(before, after);
            } else {
                assert(bridge_nonsemantic_incomplete_frame(before));
                bridge_nonsemantic_incomplete_source_step_has_unique_finish(before, after);
            }
        },
        admission::AdmissionFamily::Expr => {
            if admission::admission_family_complete(top) {
                bridge_expr_source_step_has_unique_finish(before, after);
            } else {
                assert(bridge_nonsemantic_incomplete_frame(before));
                bridge_nonsemantic_incomplete_source_step_has_unique_finish(before, after);
            }
        },
        admission::AdmissionFamily::CostTokenStack => {
            bridge_cost_token_stack_source_step_has_unique_finish(before, after);
        },
        admission::AdmissionFamily::Connective => {
            bridge_connective_source_step_has_unique_finish(before, after);
        },
        admission::AdmissionFamily::GUnforgeable => {
            bridge_gunforgeable_source_step_has_unique_finish(before, after);
        },
        _ => {},
    }
    bridge_score_finish_selected_preserves_checked(before);
}

pub open spec fn bridge_first_difference(
    expected: Seq<u8>, claimed: Seq<u8>,
) -> Option<int>
    decreases expected.len() + claimed.len(),
{
    if expected.len() == 0 || claimed.len() == 0 {
        if expected.len() == claimed.len() { None } else { Some(0) }
    } else if expected[0] != claimed[0] {
        Some(0)
    } else {
        match bridge_first_difference(expected.drop_first(), claimed.drop_first()) {
            None => None,
            Some(at) => Some(at + 1),
        }
    }
}

pub proof fn bridge_first_difference_exact(expected: Seq<u8>, claimed: Seq<u8>)
    ensures
        bridge_first_difference(expected, claimed).is_none() == (expected == claimed),
        match bridge_first_difference(expected, claimed) {
        None => expected == claimed,
        Some(at) => {
            &&& 0 <= at <= expected.len()
            &&& at <= claimed.len()
            &&& forall|i: int| 0 <= i < at ==> expected[i] == claimed[i]
            &&& (at == expected.len() || at == claimed.len()
                || expected[at] != claimed[at])
        },
    },
    decreases expected.len() + claimed.len(),
{
    if expected.len() == 0 || claimed.len() == 0 {
        if expected.len() == claimed.len() {
            assert(expected.len() == 0 && claimed.len() == 0);
            assert(expected == claimed);
        } else {
            assert(expected != claimed);
        }
    } else if expected[0] != claimed[0] {
        assert(expected != claimed);
    } else {
        bridge_first_difference_exact(expected.drop_first(), claimed.drop_first());
        if let Some(at) = bridge_first_difference(expected, claimed) {
            assert(expected != claimed) by {
                if expected == claimed {
                    assert(expected.drop_first() == claimed.drop_first());
                    assert(bridge_first_difference(
                        expected.drop_first(), claimed.drop_first(),
                    ).is_none());
                }
            }
            assert forall|i: int| 0 <= i < at
                implies expected[i] == claimed[i] by {
                if i > 0 {
                    assert(expected[i] == expected.drop_first()[i - 1]);
                    assert(claimed[i] == claimed.drop_first()[i - 1]);
                }
            }
        } else {
            assert(expected.drop_first() == claimed.drop_first());
            assert(expected.len() == claimed.len());
            assert forall|i: int| 0 <= i < expected.len()
                implies expected[i] == claimed[i] by {
                if i > 0 {
                    assert(expected[i] == expected.drop_first()[i - 1]);
                    assert(claimed[i] == claimed.drop_first()[i - 1]);
                }
            }
            assert(expected == claimed);
        }
    }
}

pub open spec fn bridge_root_transcript(bridge: ScoreAdmissionBridge) -> Option<Seq<u8>> {
    match bridge_root_handle(bridge) {
        None => None,
        Some(handle) => score::handle_transcript(bridge.transcript, handle),
    }
}

pub open spec fn bridge_root_first_difference(
    bridge: ScoreAdmissionBridge, claimed: Seq<u8>,
) -> Option<int> {
    match bridge_root_transcript(bridge) {
        None => None,
        Some(expected) => bridge_first_difference(expected, claimed),
    }
}

pub proof fn bridge_root_first_difference_exact(
    bridge: ScoreAdmissionBridge, claimed: Seq<u8>,
)
    requires bridge_well_formed(bridge), admission::admission_complete_root(bridge.admission),
    ensures
        bridge_root_first_difference(bridge, claimed).is_none()
            == score::claimed_score_matches(
                bridge.transcript,
                bridge.admission.frames[0].score_handle.unwrap(), claimed,
            ),
        match bridge_root_first_difference(bridge, claimed) {
        None => score::claimed_score_matches(
            bridge.transcript,
            bridge.admission.frames[0].score_handle.unwrap(), claimed,
        ),
        Some(at) => {
            let expected = bridge_root_transcript(bridge).unwrap();
            &&& 0 <= at <= expected.len()
            &&& at <= claimed.len()
            &&& forall|i: int| 0 <= i < at ==> expected[i] == claimed[i]
            &&& (at == expected.len() || at == claimed.len()
                || expected[at] != claimed[at])
        },
    },
{
    bridge_root_exact(bridge);
    bridge_first_difference_exact(bridge_root_transcript(bridge).unwrap(), claimed);
}

pub proof fn bridge_cost_token_stack_root_claim_exact(
    bridge: ScoreAdmissionBridge, claimed: Seq<u8>,
)
    requires
        bridge_reducer_reachable(bridge),
        bridge.admission.frames.len() == 1,
    ensures match bridge_cost_token_stack_step(bridge) {
        None => true,
        Some((next, _, score_handle)) => {
            let layers = bridge_cost_layers(
                bridge.admission.frames[0].returns, bridge.admission.arena,
            ).unwrap();
            &&& admission::admission_complete_root(next.admission)
            &&& bridge_reducer_reachable(next)
            &&& bridge_root_handle(next) == Some(score_handle)
            &&& bridge_root_transcript(next)
                == Some(bridge_cost_token_stack_transcript(bridge, layers))
            &&& bridge_root_first_difference(next, claimed).is_none()
                == (bridge_cost_token_stack_transcript(bridge, layers) == claimed)
            &&& bridge_source_checked_root_accepts(
                next, admission::AdmissionFamily::CostTokenStack, claimed,
            )
                == (bridge_cost_token_stack_transcript(bridge, layers) == claimed)
        },
    },
{
    bridge_reachable_has_exact_binding(bridge);
    bridge_cost_token_stack_exact(bridge);
    if let Some((next, _, score_handle)) = bridge_cost_token_stack_step(bridge) {
        assert(bridge_source_step(bridge, next));
        bridge_source_step_extends_reachability(bridge, next);
        assert(next.admission.frames.len() == 1);
        assert(next.admission.root_end == next.admission.frames[0].region_end);
        assert(next.admission.pushed as int == next.admission.completed as int + 1);
        assert(next.admission.frames[0].score_handle == Some(score_handle));
        assert(admission::admission_score_body_agrees(
            next.admission.frames[0], next.admission.arena,
        ));
        assert(admission::admission_complete_root(next.admission));
        bridge_root_exact(next);
        bridge_root_first_difference_exact(next, claimed);
    }
}

pub open spec fn bridge_root_handle(bridge: ScoreAdmissionBridge) -> Option<usize> {
    if admission::admission_complete_root(bridge.admission) {
        bridge.admission.frames[0].score_handle
    } else {
        None
    }
}

pub open spec fn bridge_source_checked_root_accepts(
    bridge: ScoreAdmissionBridge,
    claimed_family: admission::AdmissionFamily,
    claimed: Seq<u8>,
) -> bool {
    bridge_reducer_reachable(bridge)
        && admission::admission_complete_root(bridge.admission)
        && bridge.admission.frames[0].family == claimed_family
        && admission::admission_family_program_id(claimed_family)
            == Some(bridge.admission.frames[0].program_type_id)
        && (claimed_family == admission::AdmissionFamily::Send
            || claimed_family == admission::AdmissionFamily::Receive
            || claimed_family == admission::AdmissionFamily::New
            || claimed_family == admission::AdmissionFamily::Match
            || claimed_family == admission::AdmissionFamily::Bundle
            || claimed_family == admission::AdmissionFamily::If
            || claimed_family == admission::AdmissionFamily::CostTokenStack
            || claimed_family == admission::AdmissionFamily::Expr
            || claimed_family == admission::AdmissionFamily::Connective
            || claimed_family == admission::AdmissionFamily::GUnforgeable)
        && bridge_root_first_difference(bridge, claimed).is_none()
}

pub proof fn bridge_source_checked_root_has_exact_claim(
    bridge: ScoreAdmissionBridge,
    claimed_family: admission::AdmissionFamily,
    claimed: Seq<u8>,
)
    requires bridge_source_checked_root_accepts(bridge, claimed_family, claimed),
    ensures {
        let handle = bridge.admission.frames[0].score_handle.unwrap();
        &&& bridge_root_handle(bridge) == Some(handle)
        &&& bridge.admission.frames[0].family == claimed_family
        &&& admission::admission_family_program_id(claimed_family)
            == Some(bridge.admission.frames[0].program_type_id)
        &&& score::claimed_score_matches(bridge.transcript, handle, claimed)
        &&& bridge.admission.arena[handle as int].transcript_bytes == claimed
        &&& bridge.admission.arena[handle as int].transcript_bytes.len() > 0
        &&& bridge.admission.arena[handle as int].transcript_bytes[0] != 0u8
    },
{
    bridge_reachable_has_exact_binding(bridge);
    bridge_root_exact(bridge);
    bridge_root_first_difference_exact(bridge, claimed);
    let handle = bridge.admission.frames[0].score_handle.unwrap();
    bridge_binding_excludes_generic_zero_transcript(bridge, handle);
}

pub proof fn bridge_root_exact(bridge: ScoreAdmissionBridge)
    requires bridge_well_formed(bridge), admission::admission_complete_root(bridge.admission),
    ensures match bridge_root_handle(bridge) {
        None => false,
        Some(handle) => {
            &&& (handle as int) < bridge.transcript.entries.len()
            &&& admission::admission_score_origin_matches(
                bridge.admission.frames[0], bridge.admission.arena, handle,
            )
            &&& bridge.admission.arena[handle as int].semantic
                == admission::admission_family_complete(bridge.admission.frames[0])
            &&& bridge.admission.arena[handle as int].encoded_len
                == bridge.transcript.entries[handle as int].encoded_len
            &&& bridge.admission.arena[handle as int].transcript_bytes
                == bridge.transcript.entries[handle as int].transcript
            &&& score::entry_well_formed(bridge.transcript.entries, handle as int)
            &&& score::handle_bound_to_bytes(
                bridge.transcript, handle,
                bridge.transcript.entries[handle as int].transcript
            )
        },
    },
{
    admission::admission_root_refines(bridge.admission);
    let handle = bridge.admission.frames[0].score_handle.unwrap();
    assert(admission::admission_score_body_agrees(
        bridge.admission.frames[0], bridge.admission.arena,
    ));
    assert((handle as int) < bridge.admission.arena.len());
    assert(admission::admission_score_origin_matches(
        bridge.admission.frames[0], bridge.admission.arena, handle,
    ));
    assert(bridge.admission.arena[handle as int].semantic
        == admission::admission_family_complete(bridge.admission.frames[0]));
    assert(score::entry_well_formed(bridge.transcript.entries, handle as int));
    let bytes = bridge.transcript.entries[handle as int].transcript;
    assert(bytes.len() == bridge.transcript.entries[handle as int].encoded_len as int);
    assert(bytes.len() <= usize::MAX as int);
    assert(score::handle_transcript(bridge.transcript, handle) == Some(bytes));
    assert(score::handle_encoded_len(bridge.transcript, handle)
        == Some(bytes.len() as usize));
}

pub fn root_first_difference(
    expected: &[u8], claimed: &[u8], handle: usize,
    bridge: Ghost<ScoreAdmissionBridge>,
) -> (difference: Option<usize>)
    requires
        bridge_well_formed(bridge@),
        admission::admission_complete_root(bridge@.admission),
        handle == bridge@.admission.frames[0].score_handle.unwrap(),
        expected@ == bridge@.transcript.entries[
            bridge@.admission.frames[0].score_handle.unwrap() as int
        ].transcript,
    ensures
        difference.is_none() == score::claimed_score_matches(
            bridge@.transcript,
            bridge@.admission.frames[0].score_handle.unwrap(), claimed@
        ),
        match difference {
            Some(at) => {
                &&& at <= expected.len() && at <= claimed.len()
                &&& forall|i: int| 0 <= i < at as int
                    ==> expected@[i] == claimed@[i]
                &&& (at == expected.len() || at == claimed.len()
                    || expected@[at as int] != claimed@[at as int])
            },
            None => true,
        },
{
    proof {
        bridge_root_exact(bridge@);
    }
    score::first_difference_for_handle(
        expected, claimed, Ghost(bridge@.transcript), handle
    )
}

pub enum BridgeVarCase {
    Absent,
    Bound { level: i32 },
    Free { level: i32 },
    Wildcard,
}

pub open spec fn bridge_var_case(
    top: admission::AdmissionFrameState,
) -> Option<BridgeVarCase> {
    if top.family != admission::AdmissionFamily::Other
        || top.frame_kind != 1
        || top.program_type_id != 6
        || top.waiting_id.is_some()
        || top.cursor != top.region_end
        || top.score_handle.is_some()
    {
        None
    } else if !top.seen.contains(0) {
        if top.returns.len() == 0 && top.scalar_occurrences.len() == 0
            && !top.scalar_slots.contains(0)
        {
            Some(BridgeVarCase::Absent)
        } else {
            None
        }
    } else if top.scalar_slots.contains(0) {
        if top.returns.len() != 0 || top.scalar_occurrences.len() != 1 {
            None
        } else {
            let occurrence = top.scalar_occurrences[0];
            if occurrence.slot != 0 || occurrence.scalar_kind != 3
                || occurrence.varint_value > u32::MAX as u64
            {
                None
            } else if occurrence.tag == 1 {
                Some(BridgeVarCase::Bound {
                    level: bridge_sint32_value(occurrence.varint_value),
                })
            } else if occurrence.tag == 2 {
                Some(BridgeVarCase::Free {
                    level: bridge_sint32_value(occurrence.varint_value),
                })
            } else {
                None
            }
        }
    } else if top.scalar_occurrences.len() == 0
        && top.returns.len() == 1
        && top.returns[0].slot == 0
        && top.returns[0].tag == 3
        && top.returns[0].kind == admission::AdmissionReturnedKind::Unit
    {
        Some(BridgeVarCase::Wildcard)
    } else {
        None
    }
}

pub proof fn bridge_var_instance_descriptor_rows_pinned(
    field: admission::IndexedCanonicalFieldModel, slot: usize, repeated: bool,
    scalar_kind: u8,
)
    ensures
        (field.number == 1 || field.number == 2) ==> (
            admission::admission_scalar_descriptor_valid(6, field, slot, repeated, scalar_kind)
                == (slot == 0 && !repeated && scalar_kind == 3 && field.wire == 0
                    && field.decoded_varint <= u32::MAX as u64)
        ),
        field.number == 3 ==> !admission::admission_scalar_descriptor_valid(
            6, field, slot, repeated, scalar_kind,
        ),
{
}

pub open spec fn bridge_var_level_score_step(
    before: ScoreAdmissionBridge, constructor: i32, level: i32,
) -> Option<(ScoreAdmissionBridge, usize)> {
    match bridge_append_unattached(before, score::ScoreNode::Integer(level as i64)) {
        None => None,
        Some((after_level, level_handle)) => match bridge_constructor_step(
            after_level, constructor, seq![level_handle],
        ) {
            None => None,
            Some((next, _, score_handle)) => Some((next, score_handle)),
        },
    }
}

pub open spec fn bridge_var_score_step(
    before: ScoreAdmissionBridge,
) -> Option<(ScoreAdmissionBridge, usize)> {
    if before.admission.frames.len() == 0 {
        None
    } else {
        let top = before.admission.frames.last();
        match bridge_var_case(top) {
            None => None,
            Some(BridgeVarCase::Absent) => match bridge_append_unattached(
                before, score::ScoreNode::Integer(0i64),
            ) {
                None => None,
                Some((after_leaf, leaf_handle)) =>
                    match admission::admission_attach_score_step(
                        after_leaf.admission, leaf_handle, top.body_id,
                    ) {
                        None => None,
                        Some(admission) => Some((ScoreAdmissionBridge {
                            admission, transcript: after_leaf.transcript,
                        }, leaf_handle)),
                    },
            },
            Some(BridgeVarCase::Bound { level }) =>
                bridge_var_level_score_step(before, 50i32, level),
            Some(BridgeVarCase::Free { level }) =>
                bridge_var_level_score_step(before, 51i32, level),
            Some(BridgeVarCase::Wildcard) => match bridge_constructor_step(
                before, 52i32, Seq::empty(),
            ) {
                None => None,
                Some((next, _, score_handle)) => Some((next, score_handle)),
            },
        }
    }
}

pub open spec fn bridge_transcript_suffix_kinds(
    transcript: score::ScoreArena, base: int,
) -> Seq<score::ScoreNode> {
    transcript.entries.subrange(base, transcript.entries.len() as int)
        .map_values(|entry: score::ScoreEntry| entry.kind)
}

pub open spec fn production_score_constructor_nodes(
    constructor: i32, children: Seq<usize>, constructor_handle: usize,
) -> Seq<score::ScoreNode> {
    seq![
        score::ScoreNode::Integer(constructor as i64),
        score::ScoreNode::Node(seq![constructor_handle] + children),
    ]
}

pub open spec fn production_var_score_nodes(
    var_case: BridgeVarCase, base: usize,
) -> Seq<score::ScoreNode> {
    match var_case {
        BridgeVarCase::Absent => seq![score::ScoreNode::Integer(0i64)],
        BridgeVarCase::Bound { level } => seq![score::ScoreNode::Integer(level as i64)]
            + production_score_constructor_nodes(50i32, seq![base], (base + 1) as usize),
        BridgeVarCase::Free { level } => seq![score::ScoreNode::Integer(level as i64)]
            + production_score_constructor_nodes(51i32, seq![base], (base + 1) as usize),
        BridgeVarCase::Wildcard => production_score_constructor_nodes(
            52i32, Seq::empty(), base,
        ),
    }
}

pub proof fn bridge_var_level_score_step_exact(
    before: ScoreAdmissionBridge, constructor: i32, level: i32,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match bridge_var_level_score_step(before, constructor, level) {
        None => true,
        Some((next, score_handle)) => {
            let base = before.transcript.entries.len() as int;
            let handle_base = before.transcript.entries.len() as usize;
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(before, next)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& score_handle as int == base + 2
            &&& next.transcript.entries.len() == base + 3
            &&& next.admission.arena[score_handle as int].body_id
                == before.admission.frames.last().body_id
            &&& next.admission.arena[score_handle as int].semantic
                == admission::admission_family_complete(before.admission.frames.last())
            &&& next.admission.root_end == before.admission.root_end
            &&& next.admission.next_obligation_id == before.admission.next_obligation_id
            &&& bridge_transcript_suffix_kinds(next.transcript, base)
                == seq![score::ScoreNode::Integer(level as i64)]
                    + production_score_constructor_nodes(
                        constructor, seq![handle_base], (handle_base + 1) as usize,
                    )
        },
    },
{
    let level_kind = score::ScoreNode::Integer(level as i64);
    bridge_append_unattached_exact(before, level_kind);
    bridge_append_unattached_preserves_admission_wf(before, level_kind);
    bridge_append_unattached_shape(before, level_kind);
    if let Some((after_level, level_handle)) = bridge_append_unattached(before, level_kind) {
        let children = seq![level_handle];
        bridge_constructor_exact(after_level, constructor, children);
        bridge_constructor_shape(after_level, constructor, children);
        if let Some((next, constructor_handle, score_handle)) = bridge_constructor_step(
            after_level, constructor, children,
        ) {
            bridge_score_step_shape_transitive(before, after_level, next);
            let base = before.transcript.entries.len() as int;
            let handle_base = before.transcript.entries.len() as usize;
            let constructor_kind = score::ScoreNode::Integer(constructor as i64);
            bridge_append_unattached_exact(after_level, constructor_kind);
            let (after_constructor, appended_constructor) = bridge_append_unattached(
                after_level, constructor_kind,
            ).unwrap();
            let node_kind = score::ScoreNode::Node(seq![appended_constructor] + children);
            bridge_append_unattached_exact(after_constructor, node_kind);
            let (after_node, appended_node) = bridge_append_unattached(
                after_constructor, node_kind,
            ).unwrap();
            assert(appended_constructor == constructor_handle);
            assert(appended_node == score_handle);
            assert(next.transcript == after_node.transcript);
            assert(level_handle as int == base);
            assert(constructor_handle as int == base + 1);
            assert(score_handle as int == base + 2);
            assert(after_node.transcript.entries.len() == base + 3);
            assert(after_constructor.transcript.entries[base]
                == after_level.transcript.entries[base]);
            assert(after_node.transcript.entries[base]
                == after_constructor.transcript.entries[base]);
            assert(after_node.transcript.entries[base + 1]
                == after_constructor.transcript.entries[base + 1]);
            let suffix = bridge_transcript_suffix_kinds(next.transcript, base);
            assert(suffix.len() == 3);
            assert(suffix[0] == level_kind);
            assert(suffix[1] == constructor_kind);
            assert(suffix[2] == node_kind);
            assert(seq![appended_constructor] + children
                =~= seq![(handle_base + 1) as usize] + seq![handle_base]);
            assert(suffix =~= seq![level_kind] + production_score_constructor_nodes(
                constructor, seq![handle_base], (handle_base + 1) as usize,
            ));
        }
    }
}

pub proof fn bridge_var_score_step_exact(before: ScoreAdmissionBridge)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match bridge_var_score_step(before) {
        None => true,
        Some((next, score_handle)) => {
            &&& bridge_well_formed(next)
            &&& admission::admission_state_wf(next.admission)
            &&& bridge_score_step_shape(before, next)
            &&& next.admission.frames.last().score_handle == Some(score_handle)
            &&& (score_handle as int) < next.admission.arena.len()
            &&& next.admission.arena[score_handle as int].semantic
            &&& next.admission.arena[score_handle as int].body_id
                == before.admission.frames.last().body_id
            &&& next.admission.arena[score_handle as int].transcript_bytes
                == next.transcript.entries[score_handle as int].transcript
            &&& next.admission.root_end == before.admission.root_end
            &&& next.admission.next_obligation_id == before.admission.next_obligation_id
        },
    },
{
    if before.admission.frames.len() > 0 {
        let top = before.admission.frames.last();
        match bridge_var_case(top) {
            Some(BridgeVarCase::Absent) => {
                let kind = score::ScoreNode::Integer(0i64);
                bridge_append_unattached_exact(before, kind);
                bridge_append_unattached_preserves_admission_wf(before, kind);
                bridge_append_unattached_shape(before, kind);
                if let Some((after_leaf, leaf_handle)) = bridge_append_unattached(before, kind) {
                    admission::admission_attach_score_preserves_wf(
                        after_leaf.admission, leaf_handle, top.body_id,
                    );
                    bridge_attach_score_shape(after_leaf, leaf_handle, top.body_id);
                    if let Some(admission) = admission::admission_attach_score_step(
                        after_leaf.admission, leaf_handle, top.body_id,
                    ) {
                        let next = ScoreAdmissionBridge {
                            admission, transcript: after_leaf.transcript,
                        };
                        bridge_score_step_shape_transitive(before, after_leaf, next);
                        assert(admission::admission_family_complete(top));
                        assert(bridge_well_formed(next));
                    }
                }
            },
            Some(BridgeVarCase::Bound { level }) => {
                bridge_var_level_score_step_exact(before, 50i32, level);
                assert(admission::admission_family_complete(top));
            },
            Some(BridgeVarCase::Free { level }) => {
                bridge_var_level_score_step_exact(before, 51i32, level);
                assert(admission::admission_family_complete(top));
            },
            Some(BridgeVarCase::Wildcard) => {
                bridge_constructor_exact(before, 52i32, Seq::empty());
                bridge_constructor_shape(before, 52i32, Seq::empty());
                assert(admission::admission_family_complete(top));
            },
            None => {},
        }
    }
}

pub proof fn bridge_var_score_step_is_production_var_score(before: ScoreAdmissionBridge)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
    ensures match bridge_var_score_step(before) {
        None => true,
        Some((next, score_handle)) => match bridge_var_case(before.admission.frames.last()) {
            None => false,
            Some(var_case) => {
                let base = before.transcript.entries.len() as int;
                let nodes = production_var_score_nodes(var_case, base as usize);
                &&& next.transcript.entries.len() == base + nodes.len()
                &&& next.transcript.entries.take(base) == before.transcript.entries
                &&& bridge_transcript_suffix_kinds(next.transcript, base) == nodes
                &&& score_handle as int == base + nodes.len() - 1
                &&& next.admission.frames.last().score_handle == Some(score_handle)
            },
        },
    },
{
    bridge_var_score_step_exact(before);
    if before.admission.frames.len() > 0 {
        let top = before.admission.frames.last();
        let base = before.transcript.entries.len() as int;
        let handle_base = before.transcript.entries.len() as usize;
        match bridge_var_case(top) {
            Some(BridgeVarCase::Absent) => {
                let kind = score::ScoreNode::Integer(0i64);
                bridge_append_unattached_exact(before, kind);
                if let Some((after_leaf, leaf_handle)) = bridge_append_unattached(before, kind) {
                    assert(leaf_handle as int == base);
                    assert(after_leaf.transcript.entries.len() == base + 1);
                    let suffix = bridge_transcript_suffix_kinds(after_leaf.transcript, base);
                    assert(suffix[0] == kind);
                    assert(suffix =~= production_var_score_nodes(
                        BridgeVarCase::Absent, handle_base,
                    ));
                }
            },
            Some(BridgeVarCase::Bound { level }) => {
                bridge_var_level_score_step_exact(before, 50i32, level);
            },
            Some(BridgeVarCase::Free { level }) => {
                bridge_var_level_score_step_exact(before, 51i32, level);
            },
            Some(BridgeVarCase::Wildcard) => {
                bridge_constructor_exact(before, 52i32, Seq::empty());
                if let Some((next, constructor_handle, score_handle)) = bridge_constructor_step(
                    before, 52i32, Seq::empty(),
                ) {
                    let constructor_kind = score::ScoreNode::Integer(52i64);
                    bridge_append_unattached_exact(before, constructor_kind);
                    let (after_constructor, appended_constructor) = bridge_append_unattached(
                        before, constructor_kind,
                    ).unwrap();
                    let node_kind = score::ScoreNode::Node(
                        seq![appended_constructor] + Seq::<usize>::empty(),
                    );
                    bridge_append_unattached_exact(after_constructor, node_kind);
                    let (after_node, appended_node) = bridge_append_unattached(
                        after_constructor, node_kind,
                    ).unwrap();
                    assert(appended_constructor == constructor_handle);
                    assert(appended_node == score_handle);
                    assert(next.transcript == after_node.transcript);
                    assert(constructor_handle as int == base);
                    assert(after_node.transcript.entries.len() == base + 2);
                    assert(after_node.transcript.entries[base]
                        == after_constructor.transcript.entries[base]);
                    let suffix = bridge_transcript_suffix_kinds(next.transcript, base);
                    assert(suffix[0] == constructor_kind);
                    assert(suffix[1] == node_kind);
                    assert(seq![appended_constructor] + Seq::<usize>::empty()
                        =~= seq![handle_base] + Seq::<usize>::empty());
                    assert(suffix =~= production_var_score_nodes(
                        BridgeVarCase::Wildcard, handle_base,
                    ));
                }
            },
            None => {},
        }
    }
}

pub proof fn bridge_var_frame_required_slots_generated(
    frame: admission::AdmissionFrameState,
)
    requires
        frame.program_type_id == 6,
        frame.slot_count == 1,
        frame.required == Set::<int>::empty(),
    ensures admission::admission_required_slots_generated(frame),
{
    assert forall|tag: u32| !admission::admission_required_descriptor_slot(6, tag, 0) by {}
    assert forall|slot: int| 0 <= slot < frame.slot_count as int implies
        frame.required.contains(slot) == (exists|tag: u32|
            admission::admission_required_descriptor_slot(
                frame.program_type_id, tag, slot,
            )) by {
        assert(slot == 0);
        assert(!frame.required.contains(slot));
    }
}

pub proof fn bridge_var_level_score_step_succeeds_from_empty(
    before: ScoreAdmissionBridge, constructor: i32, level: i32,
)
    requires
        bridge_well_formed(before),
        admission::admission_state_wf(before.admission),
        before.transcript == score::empty_arena(),
        bridge_var_case(before.admission.frames.last()).is_some(),
        admission::admission_slots_complete(before.admission.frames.last()),
        before.admission.frames.last().nested_started
            == before.admission.frames.last().nested_resolved,
    ensures bridge_var_level_score_step(before, constructor, level).is_some(),
{
    let top = before.admission.frames.last();
    let level_kind = score::ScoreNode::Integer(level as i64);
    score::integer_transcript_exact(level as i64);
    assert(bridge_append_unattached(before, level_kind).is_some());
    bridge_append_unattached_exact(before, level_kind);
    bridge_append_unattached_preserves_admission_wf(before, level_kind);
    let (after_level, level_handle) = bridge_append_unattached(before, level_kind).unwrap();
    let constructor_kind = score::ScoreNode::Integer(constructor as i64);
    score::integer_transcript_exact(constructor as i64);
    assert(bridge_append_unattached(after_level, constructor_kind).is_some());
    bridge_append_unattached_exact(after_level, constructor_kind);
    bridge_append_unattached_preserves_admission_wf(after_level, constructor_kind);
    let (after_constructor, constructor_handle) = bridge_append_unattached(
        after_level, constructor_kind,
    ).unwrap();
    assert(level_handle == 0usize);
    assert(constructor_handle == 1usize);
    let entries = after_constructor.transcript.entries;
    assert(entries.len() == 2);
    assert(entries[0] == after_level.transcript.entries[0]);
    assert(entries[0].encoded_len == 10);
    assert(entries[1].encoded_len == 10);
    let children = seq![constructor_handle] + seq![level_handle];
    assert(children =~= seq![1usize, 0usize]);
    assert(score::child_handles_valid(entries, children));
    assert forall|i: int| 0 <= i < entries.len()
        implies entries[i].encoded_len as int == entries[i].transcript.len() by {
        assert(score::entry_well_formed(entries, i));
    }
    score::node_transcript_exact(entries, children);
    assert(children.drop_first() =~= seq![0usize]);
    assert(seq![0usize].drop_first() =~= Seq::<usize>::empty());
    assert(score::children_encoded_len(entries, Seq::<usize>::empty()) == 0);
    assert(score::children_encoded_len(entries, seq![0usize]) == 10);
    assert(score::children_encoded_len(entries, children) == 20);
    let node_kind = score::ScoreNode::Node(children);
    assert(bridge_append_unattached(after_constructor, node_kind).is_some());
    bridge_append_unattached_exact(after_constructor, node_kind);
    let (after_node, node_handle) = bridge_append_unattached(
        after_constructor, node_kind,
    ).unwrap();
    assert(after_node.admission.frames == before.admission.frames);
    assert(admission::admission_family_complete(top));
    assert(admission::admission_attach_score_step(
        after_node.admission, node_handle, top.body_id,
    ).is_some());
    assert(bridge_constructor_step(after_level, constructor, seq![level_handle]).is_some());
}

pub open spec fn bridge_var_witness_frame(
    source_bytes: Seq<u8>, seen: Set<int>, scalar_slots: Set<int>,
    scalar_occurrences: Seq<admission::AdmissionScalarOccurrence>,
    returns: Seq<admission::AdmissionReturnedSummary>,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: 0,
        physical_start: 0,
        program_type_id: 6,
        region_start: 0,
        region_end: source_bytes.len() as usize,
        source_bytes,
        cursor: source_bytes.len() as usize,
        slot_count: 1,
        last_slot: if seen.contains(0) { Some(0usize) } else { None },
        seen,
        required: Set::empty(),
        scalar_slots,
        scalar_occurrences,
        returns,
        family: admission::AdmissionFamily::Other,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: None,
        frame_kind: 1,
        body_id: 0,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    }
}

pub open spec fn bridge_var_witness_scalar(
    tag: u32, varint_value: u64,
) -> admission::AdmissionScalarOccurrence {
    admission::AdmissionScalarOccurrence {
        slot: 0,
        tag,
        scalar_kind: 3,
        varint_value,
        physical_source: 0,
        physical_start: 1,
        bytes: seq![varint_value as u8],
    }
}

pub open spec fn bridge_var_witness_wildcard_return() -> admission::AdmissionReturnedSummary {
    admission::AdmissionReturnedSummary {
        slot: 0,
        tag: 3,
        body_id: 1,
        body_bytes: Seq::empty(),
        score_handle: None,
        score_origin_body_id: None,
        kind: admission::AdmissionReturnedKind::Unit,
    }
}

pub proof fn bridge_var_score_absent_witness()
    ensures {
        let before = bridge_initial(bridge_var_witness_frame(
            Seq::empty(), Set::empty(), Set::empty(), Seq::empty(), Seq::empty(),
        ));
        &&& bridge_well_formed(before)
        &&& admission::admission_state_wf(before.admission)
        &&& bridge_var_case(before.admission.frames.last()) == Some(BridgeVarCase::Absent)
        &&& match bridge_var_score_step(before) {
            None => false,
            Some((next, score_handle)) => {
                &&& score_handle == 0usize
                &&& bridge_transcript_suffix_kinds(next.transcript, 0)
                    == seq![score::ScoreNode::Integer(0i64)]
                &&& next.admission.frames.last().score_handle == Some(0usize)
            },
        }
    },
{
    let frame = bridge_var_witness_frame(
        Seq::empty(), Set::empty(), Set::empty(), Seq::empty(), Seq::empty(),
    );
    let before = bridge_initial(frame);
    bridge_initial_exact(frame);
    bridge_var_frame_required_slots_generated(frame);
    assert(admission::admission_frame_wf(frame));
    assert(before.admission.frames.last() == frame);
    assert(admission::admission_state_wf(before.admission));
    assert(bridge_var_case(frame) == Some(BridgeVarCase::Absent));
    let kind = score::ScoreNode::Integer(0i64);
    score::integer_transcript_exact(0i64);
    assert(bridge_append_unattached(before, kind).is_some());
    bridge_append_unattached_exact(before, kind);
    let (after_leaf, leaf_handle) = bridge_append_unattached(before, kind).unwrap();
    assert(leaf_handle == 0usize);
    assert(after_leaf.admission.frames == before.admission.frames);
    assert(admission::admission_slots_complete(frame));
    assert(admission::admission_attach_score_step(
        after_leaf.admission, leaf_handle, 0,
    ).is_some());
    assert(bridge_var_score_step(before).is_some());
    bridge_var_score_step_is_production_var_score(before);
}

pub proof fn bridge_var_score_bound_zero_witness()
    ensures {
        let before = bridge_initial(bridge_var_witness_frame(
            seq![0x08u8, 0x00u8], Set::empty().insert(0), Set::empty().insert(0),
            seq![bridge_var_witness_scalar(1, 0)], Seq::empty(),
        ));
        &&& bridge_well_formed(before)
        &&& admission::admission_state_wf(before.admission)
        &&& bridge_var_case(before.admission.frames.last())
            == Some(BridgeVarCase::Bound { level: 0i32 })
        &&& match bridge_var_score_step(before) {
            None => false,
            Some((next, score_handle)) => {
                &&& score_handle == 2usize
                &&& bridge_transcript_suffix_kinds(next.transcript, 0) == seq![
                    score::ScoreNode::Integer(0i64),
                    score::ScoreNode::Integer(50i64),
                    score::ScoreNode::Node(seq![1usize, 0usize]),
                ]
                &&& next.admission.frames.last().score_handle == Some(2usize)
            },
        }
    },
{
    let frame = bridge_var_witness_frame(
        seq![0x08u8, 0x00u8], Set::empty().insert(0), Set::empty().insert(0),
        seq![bridge_var_witness_scalar(1, 0)], Seq::empty(),
    );
    let before = bridge_initial(frame);
    bridge_initial_exact(frame);
    bridge_var_frame_required_slots_generated(frame);
    assert(admission::admission_frame_wf(frame));
    assert(before.admission.frames.last() == frame);
    assert(admission::admission_state_wf(before.admission));
    assert(((0u64 as u32) >> 1) == 0u32) by (bit_vector);
    assert(((0u64 as u32) & 1) == 0u32) by (bit_vector);
    assert(bridge_sint32_value(0) == 0i32);
    assert(bridge_var_case(frame) == Some(BridgeVarCase::Bound { level: 0i32 }));
    assert(admission::admission_slots_complete(frame));
    bridge_var_level_score_step_succeeds_from_empty(before, 50i32, 0i32);
    assert(bridge_var_score_step(before).is_some());
    bridge_var_score_step_is_production_var_score(before);
    assert(seq![1usize] + seq![0usize] =~= seq![1usize, 0usize]);
    assert(production_var_score_nodes(BridgeVarCase::Bound { level: 0i32 }, 0usize)
        =~= seq![
            score::ScoreNode::Integer(0i64),
            score::ScoreNode::Integer(50i64),
            score::ScoreNode::Node(seq![1usize, 0usize]),
        ]);
}

pub proof fn bridge_var_score_free_minus_one_witness()
    ensures {
        let before = bridge_initial(bridge_var_witness_frame(
            seq![0x10u8, 0x01u8], Set::empty().insert(0), Set::empty().insert(0),
            seq![bridge_var_witness_scalar(2, 1)], Seq::empty(),
        ));
        &&& bridge_well_formed(before)
        &&& admission::admission_state_wf(before.admission)
        &&& bridge_var_case(before.admission.frames.last())
            == Some(BridgeVarCase::Free { level: -1i32 })
        &&& match bridge_var_score_step(before) {
            None => false,
            Some((next, score_handle)) => {
                &&& score_handle == 2usize
                &&& bridge_transcript_suffix_kinds(next.transcript, 0) == seq![
                    score::ScoreNode::Integer(-1i64),
                    score::ScoreNode::Integer(51i64),
                    score::ScoreNode::Node(seq![1usize, 0usize]),
                ]
                &&& next.admission.frames.last().score_handle == Some(2usize)
            },
        }
    },
{
    let frame = bridge_var_witness_frame(
        seq![0x10u8, 0x01u8], Set::empty().insert(0), Set::empty().insert(0),
        seq![bridge_var_witness_scalar(2, 1)], Seq::empty(),
    );
    let before = bridge_initial(frame);
    bridge_initial_exact(frame);
    bridge_var_frame_required_slots_generated(frame);
    assert(admission::admission_frame_wf(frame));
    assert(before.admission.frames.last() == frame);
    assert(admission::admission_state_wf(before.admission));
    assert(((1u64 as u32) >> 1) == 0u32) by (bit_vector);
    assert(((1u64 as u32) & 1) == 1u32) by (bit_vector);
    assert(bridge_sint32_value(1) == -1i32);
    assert(bridge_var_case(frame) == Some(BridgeVarCase::Free { level: -1i32 }));
    assert(admission::admission_slots_complete(frame));
    bridge_var_level_score_step_succeeds_from_empty(before, 51i32, -1i32);
    assert(bridge_var_score_step(before).is_some());
    bridge_var_score_step_is_production_var_score(before);
    assert(seq![1usize] + seq![0usize] =~= seq![1usize, 0usize]);
    assert(production_var_score_nodes(BridgeVarCase::Free { level: -1i32 }, 0usize)
        =~= seq![
            score::ScoreNode::Integer(-1i64),
            score::ScoreNode::Integer(51i64),
            score::ScoreNode::Node(seq![1usize, 0usize]),
        ]);
}

pub proof fn bridge_var_score_wildcard_witness()
    ensures {
        let before = bridge_initial(bridge_var_witness_frame(
            seq![0x1au8, 0x00u8], Set::empty().insert(0), Set::empty(),
            Seq::empty(), seq![bridge_var_witness_wildcard_return()],
        ));
        &&& bridge_well_formed(before)
        &&& admission::admission_state_wf(before.admission)
        &&& bridge_var_case(before.admission.frames.last()) == Some(BridgeVarCase::Wildcard)
        &&& match bridge_var_score_step(before) {
            None => false,
            Some((next, score_handle)) => {
                &&& score_handle == 1usize
                &&& bridge_transcript_suffix_kinds(next.transcript, 0) == seq![
                    score::ScoreNode::Integer(52i64),
                    score::ScoreNode::Node(seq![0usize]),
                ]
                &&& next.admission.frames.last().score_handle == Some(1usize)
            },
        }
    },
{
    let frame = bridge_var_witness_frame(
        seq![0x1au8, 0x00u8], Set::empty().insert(0), Set::empty(),
        Seq::empty(), seq![bridge_var_witness_wildcard_return()],
    );
    let before = bridge_initial(frame);
    bridge_initial_exact(frame);
    bridge_var_frame_required_slots_generated(frame);
    assert(admission::admission_frame_wf(frame));
    assert(before.admission.frames.last() == frame);
    assert(admission::admission_state_wf(before.admission));
    assert(bridge_var_case(frame) == Some(BridgeVarCase::Wildcard));
    let constructor_kind = score::ScoreNode::Integer(52i64);
    score::integer_transcript_exact(52i64);
    assert(bridge_append_unattached(before, constructor_kind).is_some());
    bridge_append_unattached_exact(before, constructor_kind);
    let (after_constructor, constructor_handle) = bridge_append_unattached(
        before, constructor_kind,
    ).unwrap();
    assert(constructor_handle == 0usize);
    let entries = after_constructor.transcript.entries;
    assert(entries.len() == 1);
    assert(entries[0].encoded_len == 10);
    let children = seq![constructor_handle] + Seq::<usize>::empty();
    assert(children =~= seq![0usize]);
    assert(score::child_handles_valid(entries, children));
    assert forall|i: int| 0 <= i < entries.len()
        implies entries[i].encoded_len as int == entries[i].transcript.len() by {
        assert(score::entry_well_formed(entries, i));
    }
    score::node_transcript_exact(entries, children);
    assert(children.drop_first() =~= Seq::<usize>::empty());
    assert(score::children_encoded_len(entries, Seq::<usize>::empty()) == 0);
    assert(score::children_encoded_len(entries, children) == 10);
    let node_kind = score::ScoreNode::Node(children);
    assert(bridge_append_unattached(after_constructor, node_kind).is_some());
    bridge_append_unattached_exact(after_constructor, node_kind);
    let (after_node, node_handle) = bridge_append_unattached(
        after_constructor, node_kind,
    ).unwrap();
    assert(after_node.admission.frames == before.admission.frames);
    assert(admission::admission_slots_complete(frame));
    assert(admission::admission_attach_score_step(
        after_node.admission, node_handle, 0,
    ).is_some());
    assert(bridge_constructor_step(before, 52i32, Seq::empty()).is_some());
    assert(bridge_var_score_step(before).is_some());
    bridge_var_score_step_is_production_var_score(before);
    assert(seq![0usize] + Seq::<usize>::empty() =~= seq![0usize]);
    assert(production_var_score_nodes(BridgeVarCase::Wildcard, 0usize)
        =~= seq![
            score::ScoreNode::Integer(52i64),
            score::ScoreNode::Node(seq![0usize]),
        ]);
}

}

fn main() {}
