use vstd::prelude::*;

verus! {

pub enum ScoreNode {
    Integer(i64),
    StringAtom(Seq<u8>),
    BytesAtom(Seq<u8>),
    Node(Seq<usize>),
}

pub struct ScoreEntry {
    pub kind: ScoreNode,
    pub transcript: Seq<u8>,
    pub encoded_len: usize,
}

pub struct ScoreArena {
    pub entries: Seq<ScoreEntry>,
}

pub open spec fn sign_flipped_word(value: i64) -> u64 {
    (value as u64) ^ 0x8000_0000_0000_0000u64
}

pub open spec fn big_endian_eight(word: u64) -> Seq<u8> {
    seq![
        (word >> 56) as u8,
        (word >> 48) as u8,
        (word >> 40) as u8,
        (word >> 32) as u8,
        (word >> 24) as u8,
        (word >> 16) as u8,
        (word >> 8) as u8,
        word as u8,
    ]
}

pub open spec fn integer_transcript(value: i64) -> Seq<u8> {
    seq![1u8, 1u8] + big_endian_eight(sign_flipped_word(value))
}

pub open spec fn zero_count(bytes: Seq<u8>) -> int
    decreases bytes.len(),
{
    if bytes.len() == 0 {
        0
    } else {
        (if bytes[0] == 0 { 1int } else { 0int }) + zero_count(bytes.drop_first())
    }
}

pub open spec fn escape_zeroes(bytes: Seq<u8>) -> Seq<u8>
    decreases bytes.len(),
{
    if bytes.len() == 0 {
        Seq::empty()
    } else if bytes[0] == 0 {
        seq![0u8, 0xffu8] + escape_zeroes(bytes.drop_first())
    } else {
        seq![bytes[0]] + escape_zeroes(bytes.drop_first())
    }
}

pub open spec fn string_transcript(bytes: Seq<u8>) -> Seq<u8> {
    seq![1u8, 2u8] + escape_zeroes(bytes) + seq![0u8, 0u8]
}

pub open spec fn bytes_transcript(bytes: Seq<u8>) -> Seq<u8> {
    seq![1u8, 3u8] + escape_zeroes(bytes) + seq![0u8, 0u8]
}

pub open spec fn child_handles_valid(arena: Seq<ScoreEntry>, children: Seq<usize>) -> bool {
    forall|i: int| 0 <= i < children.len() ==> (children[i] as int) < arena.len()
}

pub open spec fn children_transcript(
    arena: Seq<ScoreEntry>, children: Seq<usize>,
) -> Seq<u8>
    decreases children.len(),
{
    if children.len() == 0 {
        Seq::empty()
    } else if (children[0] as int) < arena.len() {
        arena[children[0] as int].transcript
            + children_transcript(arena, children.drop_first())
    } else {
        Seq::empty()
    }
}

pub open spec fn children_encoded_len(
    arena: Seq<ScoreEntry>, children: Seq<usize>,
) -> int
    decreases children.len(),
{
    if children.len() == 0 {
        0
    } else if (children[0] as int) < arena.len() {
        arena[children[0] as int].encoded_len as int
            + children_encoded_len(arena, children.drop_first())
    } else {
        0
    }
}

pub open spec fn kind_valid(arena: Seq<ScoreEntry>, kind: ScoreNode) -> bool {
    match kind {
        ScoreNode::Node(children) => child_handles_valid(arena, children),
        _ => true,
    }
}

pub open spec fn kind_transcript(arena: Seq<ScoreEntry>, kind: ScoreNode) -> Seq<u8> {
    match kind {
        ScoreNode::Integer(value) => integer_transcript(value),
        ScoreNode::StringAtom(bytes) => string_transcript(bytes),
        ScoreNode::BytesAtom(bytes) => bytes_transcript(bytes),
        ScoreNode::Node(children) => {
            seq![2u8] + children_transcript(arena, children) + seq![0u8]
        },
    }
}

/// The production prepare phase retains the atom or child handles and seals
/// the length computed from that source. Transcript bytes are used only to
/// state and prove the refinement of this length, not to construct the entry.
pub struct PreparedScoreEntry {
    pub kind: ScoreNode,
    pub encoded_len: usize,
}

pub open spec fn kind_encoded_len(arena: Seq<ScoreEntry>, kind: ScoreNode) -> int {
    match kind {
        ScoreNode::Integer(_) => 10,
        ScoreNode::StringAtom(bytes) | ScoreNode::BytesAtom(bytes) => {
            4 + bytes.len() + zero_count(bytes)
        },
        ScoreNode::Node(children) => 2 + children_encoded_len(arena, children),
    }
}

pub open spec fn prepare_score_entry(
    arena: ScoreArena, kind: ScoreNode,
) -> Option<PreparedScoreEntry> {
    let encoded_len = kind_encoded_len(arena.entries, kind);
    if !kind_valid(arena.entries, kind)
        || arena.entries.len() >= usize::MAX as int
        || encoded_len < 0
        || encoded_len > usize::MAX as int
    {
        None
    } else {
        Some(PreparedScoreEntry { kind, encoded_len: encoded_len as usize })
    }
}

pub open spec fn entry_well_formed(arena: Seq<ScoreEntry>, index: int) -> bool {
    &&& 0 <= index < arena.len()
    &&& kind_valid(arena.take(index), arena[index].kind)
    &&& arena[index].transcript == kind_transcript(arena.take(index), arena[index].kind)
    &&& arena[index].encoded_len as int == arena[index].transcript.len()
}

pub open spec fn arena_well_formed(arena: ScoreArena) -> bool {
    forall|index: int| 0 <= index < arena.entries.len()
        ==> entry_well_formed(arena.entries, index)
}

pub open spec fn empty_arena() -> ScoreArena {
    ScoreArena { entries: Seq::empty() }
}

/// A certificate covers exactly the entries whose handles have been allocated.
/// Its proof is maintained at insertion, so a consumer can check a suffix
/// without recomputing the transcript of any earlier entry.
pub open spec fn append_certificate(arena: ScoreArena, next_handle: usize) -> bool {
    &&& arena.entries.len() == next_handle as int
    &&& forall|i: int| 0 <= i < next_handle as int
        ==> entry_well_formed(arena.entries, i)
}

pub open spec fn certified_interval(
    arena: ScoreArena, checkpoint: usize, next_handle: usize,
) -> bool {
    &&& append_certificate(arena, next_handle)
    &&& checkpoint <= next_handle
}

/// A failed reservation or a failed checked append has no committed state.
pub open spec fn checked_append_attempt(
    arena: ScoreArena, kind: ScoreNode, reservation_granted: bool,
) -> (ScoreArena, Option<usize>) {
    match prepare_score_entry(arena, kind) {
        None => (arena, None),
        Some(_) => {
            if !reservation_granted {
                (arena, None)
            } else {
                match checked_append(arena, kind) {
                    None => (arena, None),
                    Some((next, handle)) => (next, Some(handle)),
                }
            }
        },
    }
}

/// This state mirrors the production arena's stored certified prefix length.
/// The append transition publishes the new entry and advances the prefix in
/// the same successful branch; every failure returns the original state.
pub struct CertifiedScoreArena {
    pub arena: ScoreArena,
    pub certified_prefix_len: usize,
}

pub open spec fn certified_score_arena_wf(state: CertifiedScoreArena) -> bool {
    append_certificate(state.arena, state.certified_prefix_len)
}

pub open spec fn certified_empty_arena() -> CertifiedScoreArena {
    CertifiedScoreArena {
        arena: empty_arena(),
        certified_prefix_len: 0usize,
    }
}

pub open spec fn certified_append_attempt(
    before: CertifiedScoreArena, kind: ScoreNode, reservation_granted: bool,
) -> (CertifiedScoreArena, Option<usize>) {
    match checked_append_attempt(before.arena, kind, reservation_granted) {
        (after, Some(handle)) => {
            (CertifiedScoreArena {
                arena: after,
                certified_prefix_len: after.entries.len() as usize,
            }, Some(handle))
        },
        (_, None) => (before, None),
    }
}

pub open spec fn checked_append(
    arena: ScoreArena, kind: ScoreNode,
) -> Option<(ScoreArena, usize)> {
    let transcript = kind_transcript(arena.entries, kind);
    if !arena_well_formed(arena)
        || !kind_valid(arena.entries, kind)
        || arena.entries.len() >= usize::MAX as int
        || transcript.len() > usize::MAX as int
    {
        None
    } else {
        let handle = arena.entries.len() as usize;
        let entry = ScoreEntry {
            kind,
            transcript,
            encoded_len: transcript.len() as usize,
        };
        Some((ScoreArena { entries: arena.entries.push(entry) }, handle))
    }
}

pub open spec fn append_integer(
    arena: ScoreArena, value: i64,
) -> Option<(ScoreArena, usize)> {
    checked_append(arena, ScoreNode::Integer(value))
}

pub open spec fn append_string(
    arena: ScoreArena, bytes: Seq<u8>,
) -> Option<(ScoreArena, usize)> {
    checked_append(arena, ScoreNode::StringAtom(bytes))
}

pub open spec fn append_bytes(
    arena: ScoreArena, bytes: Seq<u8>,
) -> Option<(ScoreArena, usize)> {
    checked_append(arena, ScoreNode::BytesAtom(bytes))
}

pub open spec fn append_node(
    arena: ScoreArena, children: Seq<usize>,
) -> Option<(ScoreArena, usize)> {
    checked_append(arena, ScoreNode::Node(children))
}

pub open spec fn handle_contents(arena: ScoreArena, handle: usize) -> Option<ScoreNode> {
    if (handle as int) < arena.entries.len() {
        Some(arena.entries[handle as int].kind)
    } else {
        None
    }
}

pub open spec fn handle_transcript(arena: ScoreArena, handle: usize) -> Option<Seq<u8>> {
    if (handle as int) < arena.entries.len() {
        Some(arena.entries[handle as int].transcript)
    } else {
        None
    }
}

pub open spec fn handle_encoded_len(arena: ScoreArena, handle: usize) -> Option<usize> {
    if (handle as int) < arena.entries.len() {
        Some(arena.entries[handle as int].encoded_len)
    } else {
        None
    }
}

pub open spec fn handle_bound_to_bytes(
    arena: ScoreArena, handle: usize, bytes: Seq<u8>,
) -> bool {
    arena_well_formed(arena)
        && handle_transcript(arena, handle) == Some(bytes)
        && bytes.len() <= usize::MAX as int
        && handle_encoded_len(arena, handle) == Some(bytes.len() as usize)
}

pub proof fn empty_arena_well_formed()
    ensures arena_well_formed(empty_arena()),
{}

pub proof fn empty_append_certificate()
    ensures append_certificate(empty_arena(), 0usize),
{}

pub proof fn certified_interval_entries_valid(
    arena: ScoreArena, checkpoint: usize, next_handle: usize,
)
    requires certified_interval(arena, checkpoint, next_handle),
    ensures forall|i: int| checkpoint as int <= i < next_handle as int
        ==> entry_well_formed(arena.entries, i),
{}

pub proof fn integer_transcript_exact(value: i64)
    ensures
        integer_transcript(value).len() == 10,
        integer_transcript(value)[0] == 1u8,
        integer_transcript(value)[1] == 1u8,
        integer_transcript(value).subrange(2, 10)
            == big_endian_eight(sign_flipped_word(value)),
{}

pub proof fn escape_zeroes_length(bytes: Seq<u8>)
    ensures escape_zeroes(bytes).len() == bytes.len() + zero_count(bytes),
    decreases bytes.len(),
{
    if bytes.len() > 0 {
        escape_zeroes_length(bytes.drop_first());
    }
}

pub proof fn string_and_bytes_transcripts_exact(bytes: Seq<u8>)
    ensures
        string_transcript(bytes).len() == 4 + bytes.len() + zero_count(bytes),
        bytes_transcript(bytes).len() == 4 + bytes.len() + zero_count(bytes),
        string_transcript(bytes)[0] == 1u8,
        string_transcript(bytes)[1] == 2u8,
        bytes_transcript(bytes)[0] == 1u8,
        bytes_transcript(bytes)[1] == 3u8,
        string_transcript(bytes).subrange(
            string_transcript(bytes).len() - 2, string_transcript(bytes).len() as int
        ) == seq![0u8, 0u8],
        bytes_transcript(bytes).subrange(
            bytes_transcript(bytes).len() - 2, bytes_transcript(bytes).len() as int
        ) == seq![0u8, 0u8],
        string_transcript(bytes) != bytes_transcript(bytes),
{
    escape_zeroes_length(bytes);
}

pub proof fn children_transcript_length(
    arena: Seq<ScoreEntry>, children: Seq<usize>,
)
    requires
        child_handles_valid(arena, children),
        forall|i: int| 0 <= i < arena.len()
            ==> arena[i].encoded_len as int == arena[i].transcript.len(),
    ensures children_transcript(arena, children).len()
        == children_encoded_len(arena, children),
    decreases children.len(),
{
    if children.len() > 0 {
        children_transcript_length(arena, children.drop_first());
    }
}

pub proof fn node_transcript_exact(
    arena: Seq<ScoreEntry>, children: Seq<usize>,
)
    requires
        child_handles_valid(arena, children),
        forall|i: int| 0 <= i < arena.len()
            ==> arena[i].encoded_len as int == arena[i].transcript.len(),
    ensures
        kind_transcript(arena, ScoreNode::Node(children))
            == seq![2u8] + children_transcript(arena, children) + seq![0u8],
        kind_transcript(arena, ScoreNode::Node(children)).len()
            == 2 + children_encoded_len(arena, children),
        kind_transcript(arena, ScoreNode::Node(children))[0] == 2u8,
        kind_transcript(arena, ScoreNode::Node(children))[
            kind_transcript(arena, ScoreNode::Node(children)).len() - 1
        ] == 0u8,
{
    children_transcript_length(arena, children);
}

pub proof fn kind_encoded_len_matches_transcript(
    arena: ScoreArena, kind: ScoreNode, next_handle: usize,
)
    requires
        append_certificate(arena, next_handle),
        kind_valid(arena.entries, kind),
    ensures
        kind_encoded_len(arena.entries, kind)
            == kind_transcript(arena.entries, kind).len(),
        0 <= kind_encoded_len(arena.entries, kind),
{
    match kind {
        ScoreNode::Integer(value) => {
            integer_transcript_exact(value);
        },
        ScoreNode::StringAtom(bytes) | ScoreNode::BytesAtom(bytes) => {
            string_and_bytes_transcripts_exact(bytes);
        },
        ScoreNode::Node(children) => {
            assert forall|i: int| 0 <= i < arena.entries.len()
                implies arena.entries[i].encoded_len as int
                    == arena.entries[i].transcript.len() by {
                assert(entry_well_formed(arena.entries, i));
            }
            node_transcript_exact(arena.entries, children);
        },
    }
}

pub proof fn prepare_score_entry_matches_checked_append(
    arena: ScoreArena, kind: ScoreNode, next_handle: usize,
)
    requires append_certificate(arena, next_handle),
    ensures
        prepare_score_entry(arena, kind).is_some()
            == checked_append(arena, kind).is_some(),
        match prepare_score_entry(arena, kind) {
            Some(prepared) => {
                &&& prepared.kind == kind
                &&& prepared.encoded_len as int
                    == kind_transcript(arena.entries, kind).len()
            },
            None => true,
        },
{
    assert(arena_well_formed(arena));
    if kind_valid(arena.entries, kind) {
        kind_encoded_len_matches_transcript(arena, kind, next_handle);
    }
}

pub proof fn checked_append_exact(arena: ScoreArena, kind: ScoreNode)
    ensures match checked_append(arena, kind) {
        Some((next, handle)) => {
            &&& handle as int == arena.entries.len()
            &&& next.entries.len() == arena.entries.len() + 1
            &&& next.entries.take(handle as int) == arena.entries
            &&& entry_well_formed(next.entries, handle as int)
            &&& next.entries[handle as int].transcript
                == kind_transcript(arena.entries, kind)
            &&& next.entries[handle as int].encoded_len as int
                == kind_transcript(arena.entries, kind).len()
            &&& arena_well_formed(next)
            &&& handle_contents(next, handle) == Some(kind)
            &&& handle_bound_to_bytes(next, handle, kind_transcript(arena.entries, kind))
        },
        None => {
            !arena_well_formed(arena)
                || !kind_valid(arena.entries, kind)
                || arena.entries.len() >= usize::MAX as int
                || kind_transcript(arena.entries, kind).len() > usize::MAX as int
        },
    },
{
    if let Some((next, handle)) = checked_append(arena, kind) {
        assert forall|index: int| 0 <= index < next.entries.len()
            implies entry_well_formed(next.entries, index) by {
            if index < arena.entries.len() {
                assert(next.entries[index] == arena.entries[index]);
                assert(next.entries.take(index) == arena.entries.take(index));
                assert(entry_well_formed(arena.entries, index));
            } else {
                assert(index == arena.entries.len());
                assert(next.entries.take(index) == arena.entries);
            }
        }
    }
}

pub proof fn checked_append_preserves_certificate(
    arena: ScoreArena, kind: ScoreNode, next_handle: usize,
)
    requires append_certificate(arena, next_handle),
    ensures match checked_append(arena, kind) {
        Some((next, handle)) => {
            &&& handle == next_handle
            &&& append_certificate(next, next.entries.len() as usize)
            &&& next.entries.take(next_handle as int) == arena.entries
            &&& entry_well_formed(next.entries, next_handle as int)
        },
        None => true,
    },
{
    checked_append_exact(arena, kind);
    if let Some((next, handle)) = checked_append(arena, kind) {
        assert(next.entries.len() <= usize::MAX as int);
        assert(arena_well_formed(next));
    }
}

pub proof fn checked_append_attempt_preserves_certificate(
    arena: ScoreArena, kind: ScoreNode, next_handle: usize,
    reservation_granted: bool,
)
    requires append_certificate(arena, next_handle),
    ensures match checked_append_attempt(arena, kind, reservation_granted) {
        (next, Some(handle)) => {
            &&& reservation_granted
            &&& handle == next_handle
            &&& append_certificate(next, next.entries.len() as usize)
            &&& next.entries.take(next_handle as int) == arena.entries
            &&& entry_well_formed(next.entries, next_handle as int)
        },
        (unchanged, None) => {
            &&& unchanged.entries == arena.entries
            &&& unchanged.entries.len() == next_handle as int
            &&& append_certificate(unchanged, next_handle)
        },
    },
{
    if reservation_granted {
        checked_append_preserves_certificate(arena, kind, next_handle);
    }
}

pub proof fn checked_append_attempt_preserves_certified_interval(
    arena: ScoreArena, kind: ScoreNode, checkpoint: usize,
    next_handle: usize, reservation_granted: bool,
)
    requires certified_interval(arena, checkpoint, next_handle),
    ensures match checked_append_attempt(arena, kind, reservation_granted) {
        (next, Some(_)) => certified_interval(
            next, checkpoint, next.entries.len() as usize,
        ),
        (unchanged, None) => certified_interval(
            unchanged, checkpoint, next_handle,
        ),
    },
{
    checked_append_attempt_preserves_certificate(
        arena, kind, next_handle, reservation_granted,
    );
}

pub proof fn certified_empty_arena_wf()
    ensures certified_score_arena_wf(certified_empty_arena()),
{
    empty_append_certificate();
}

pub proof fn certified_append_attempt_preserves_wf(
    before: CertifiedScoreArena, kind: ScoreNode, reservation_granted: bool,
)
    requires certified_score_arena_wf(before),
    ensures match certified_append_attempt(before, kind, reservation_granted) {
        (after, Some(handle)) => {
            &&& reservation_granted
            &&& handle == before.certified_prefix_len
            &&& certified_score_arena_wf(after)
            &&& after.certified_prefix_len as int == after.arena.entries.len()
            &&& after.arena.entries.take(handle as int) == before.arena.entries
            &&& entry_well_formed(after.arena.entries, handle as int)
        },
        (unchanged, None) => {
            &&& unchanged.arena.entries == before.arena.entries
            &&& unchanged.certified_prefix_len == before.certified_prefix_len
            &&& certified_score_arena_wf(unchanged)
        },
    },
{
    prepare_score_entry_matches_checked_append(
        before.arena, kind, before.certified_prefix_len,
    );
    checked_append_attempt_preserves_certificate(
        before.arena, kind, before.certified_prefix_len, reservation_granted,
    );
}

pub proof fn append_integer_exact(arena: ScoreArena, value: i64)
    ensures match append_integer(arena, value) {
        Some((next, handle)) => {
            &&& handle_transcript(next, handle) == Some(integer_transcript(value))
            &&& handle_encoded_len(next, handle) == Some(10usize)
            &&& arena_well_formed(next)
        },
        None => true,
    },
{
    integer_transcript_exact(value);
    checked_append_exact(arena, ScoreNode::Integer(value));
}

pub proof fn append_string_exact(arena: ScoreArena, bytes: Seq<u8>)
    ensures match append_string(arena, bytes) {
        Some((next, handle)) => {
            &&& handle_transcript(next, handle) == Some(string_transcript(bytes))
            &&& next.entries[handle as int].encoded_len as int
                == 4 + bytes.len() + zero_count(bytes)
            &&& arena_well_formed(next)
        },
        None => true,
    },
{
    string_and_bytes_transcripts_exact(bytes);
    checked_append_exact(arena, ScoreNode::StringAtom(bytes));
}

pub proof fn append_bytes_exact(arena: ScoreArena, bytes: Seq<u8>)
    ensures match append_bytes(arena, bytes) {
        Some((next, handle)) => {
            &&& handle_transcript(next, handle) == Some(bytes_transcript(bytes))
            &&& next.entries[handle as int].encoded_len as int
                == 4 + bytes.len() + zero_count(bytes)
            &&& arena_well_formed(next)
        },
        None => true,
    },
{
    string_and_bytes_transcripts_exact(bytes);
    checked_append_exact(arena, ScoreNode::BytesAtom(bytes));
}

pub proof fn append_node_exact(arena: ScoreArena, children: Seq<usize>)
    requires
        arena_well_formed(arena),
        child_handles_valid(arena.entries, children),
    ensures match append_node(arena, children) {
        Some((next, handle)) => {
            &&& handle_transcript(next, handle)
                == Some(seq![2u8] + children_transcript(arena.entries, children) + seq![0u8])
            &&& next.entries[handle as int].encoded_len as int
                == 2 + children_encoded_len(arena.entries, children)
            &&& arena_well_formed(next)
        },
        None => true,
    },
{
    assert forall|i: int| 0 <= i < arena.entries.len()
        implies arena.entries[i].encoded_len as int
            == arena.entries[i].transcript.len() by {
        assert(entry_well_formed(arena.entries, i));
    }
    node_transcript_exact(arena.entries, children);
    checked_append_exact(arena, ScoreNode::Node(children));
}

pub open spec fn claimed_score_matches(
    arena: ScoreArena, handle: usize, claimed: Seq<u8>,
) -> bool {
    (handle as int) < arena.entries.len()
        && arena.entries[handle as int].transcript == claimed
}

pub fn first_score_difference(expected: &[u8], claimed: &[u8])
    -> (difference: Option<usize>)
    ensures
        difference.is_none() == (expected@ == claimed@),
        match difference {
        None => {
            &&& expected.len() == claimed.len()
            &&& forall|i: int| 0 <= i < expected.len() as int
                ==> expected@[i] == claimed@[i]
        },
        Some(at) => {
            &&& at <= expected.len() && at <= claimed.len()
            &&& (at == expected.len() || at == claimed.len()
                || expected@[at as int] != claimed@[at as int])
            &&& forall|i: int| 0 <= i < at as int
                ==> expected@[i] == claimed@[i]
        },
    },
{
    let mut at: usize = 0;
    while at < expected.len() && at < claimed.len()
        invariant
            at <= expected.len(),
            at <= claimed.len(),
            forall|i: int| 0 <= i < at as int ==> expected@[i] == claimed@[i],
        decreases expected.len() - at,
    {
        if expected[at] != claimed[at] {
            assert(expected@ != claimed@);
            return Some(at);
        }
        at += 1;
    }
    if at == expected.len() && at == claimed.len() {
        assert forall|i: int| 0 <= i < expected@.len()
            implies expected@[i] == claimed@[i] by {}
        assert(expected@ == claimed@);
        None
    } else {
        assert(expected@ != claimed@);
        Some(at)
    }
}

pub fn first_difference_for_handle(
    expected: &[u8], claimed: &[u8], arena: Ghost<ScoreArena>, handle: usize,
) -> (difference: Option<usize>)
    requires handle_bound_to_bytes(arena@, handle, expected@),
    ensures
        difference.is_none() == claimed_score_matches(arena@, handle, claimed@),
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
    first_score_difference(expected, claimed)
}

}

fn main() {}
