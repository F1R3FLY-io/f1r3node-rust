use vstd::prelude::*;

verus! {

/// Source extraction for `CanonicalConnectiveChildren::new`: `bytes` is the
/// exact `message: &'a [u8]` stored by the Rust cursor, and `children` records
/// the successive, completely validated field-1 length-delimited regions.
/// The table is constrained by the actual canonical header bytes, not by an
/// independently chosen body or arbitrary list of lengths.
pub struct ChildRegion {
    pub header_start: usize,
    pub payload_start: usize,
    pub payload_end: usize,
}

pub struct CursorPosition {
    pub at: usize,
    pub remaining: usize,
}

pub struct ChildRange {
    pub start: usize,
    pub end: usize,
}

pub open spec fn varint_value(bytes: Seq<u8>, at: int, width: nat) -> nat
    decreases width,
{
    if width == 0 {
        0
    } else {
        (bytes[at] as nat % 128) + 128 * varint_value(bytes, at + 1, (width - 1) as nat)
    }
}

pub open spec fn canonical_length_header(
    bytes: Seq<u8>,
    start: int,
    end: int,
    value: nat,
) -> bool {
    &&& 0 <= start < end <= bytes.len()
    &&& end - start <= 10
    &&& (forall|i: int| start <= i < end - 1 ==> bytes[i] >= 128)
    &&& bytes[end - 1] < 128
    &&& (end - start > 1 ==> bytes[end - 1] % 128 > 0)
    &&& (end - start == 10 ==> bytes[end - 1] <= 1)
    &&& varint_value(bytes, start, (end - start) as nat) == value
}

pub open spec fn canonical_child(bytes: Seq<u8>, child: ChildRegion) -> bool {
    &&& (child.header_start as int) + 2 <= child.payload_start as int
    &&& child.payload_start <= child.payload_end
    &&& child.payload_end as int <= bytes.len()
    &&& bytes[child.header_start as int] == 10
    &&& canonical_length_header(
        bytes,
        child.header_start as int + 1,
        child.payload_start as int,
        (child.payload_end - child.payload_start) as nat,
    )
}

pub open spec fn validated_source(bytes: Seq<u8>, children: Seq<ChildRegion>) -> bool {
    &&& bytes.len() <= usize::MAX as int
    &&& children.len() <= usize::MAX as int
    &&& (children.len() == 0 ==> bytes.len() == 0)
    &&& (children.len() > 0 ==> children[children.len() - 1].payload_end as int == bytes.len())
    &&& (forall|i: int| 0 <= i < children.len() ==> {
        &&& canonical_child(bytes, children[i])
        &&& (i == 0 ==> children[i].header_start == 0)
        &&& (i > 0 ==> children[i].header_start == children[i - 1].payload_end)
    })
}

pub open spec fn valid_position(
    bytes: Seq<u8>,
    children: Seq<ChildRegion>,
    position: CursorPosition,
) -> bool {
    &&& validated_source(bytes, children)
    &&& position.remaining as int <= children.len()
    &&& (position.remaining == 0 ==> position.at as int == bytes.len())
    &&& (position.remaining > 0 ==>
        position.at == children[children.len() - position.remaining as int].header_start)
}

pub open spec fn bind_spec(
    source_address: usize,
    source_length: usize,
    owner_address: usize,
    owner_length: usize,
    position: CursorPosition,
) -> Option<CursorPosition> {
    if source_address == owner_address && source_length == owner_length {
        Some(position)
    } else {
        None
    }
}

pub open spec fn length_only_mutant(
    source_length: usize,
    owner_length: usize,
) -> bool {
    source_length == owner_length
}

pub open spec fn pointer_only_mutant(
    source_address: usize,
    owner_address: usize,
) -> bool {
    source_address == owner_address
}

/// Executable image of the two comparisons and the state transfer in Rust
/// `bind_owned`. It does not infer immutable bytes from pointer equality.
pub fn checked_bind(
    source_address: usize,
    source_length: usize,
    owner_address: usize,
    owner_length: usize,
    position: CursorPosition,
) -> (result: Option<CursorPosition>)
    ensures result == bind_spec(
        source_address,
        source_length,
        owner_address,
        owner_length,
        position,
    ),
{
    if source_address != owner_address || source_length != owner_length {
        None
    } else {
        Some(position)
    }
}

/// This is the audited contract of `StableConnectiveBodyOwner`, not a fact
/// proved for an arbitrary `AsRef<[u8]>` implementor. Production proofs must
/// discharge it by inspecting `EncodedRegion` and `ProtobufRegion` and the
/// Rust borrow that keeps `CanonicalConnectiveChildren.message` immutable.
pub open spec fn stable_owner_view(
    source_address: usize,
    source_bytes: Seq<u8>,
    owner_address: usize,
    owner_bytes: Seq<u8>,
) -> bool {
    (source_address == owner_address && source_bytes.len() == owner_bytes.len())
        ==> source_bytes == owner_bytes
}

pub proof fn mismatched_owner_refused_before_position_transfer(
    source_address: usize,
    source_length: usize,
    owner_address: usize,
    owner_length: usize,
    position: CursorPosition,
)
    requires source_address != owner_address || source_length != owner_length,
    ensures bind_spec(
        source_address, source_length, owner_address, owner_length, position,
    ).is_none(),
{
}

pub proof fn exact_owner_preserves_validated_position(
    source_address: usize,
    source_bytes: Seq<u8>,
    owner_address: usize,
    owner_bytes: Seq<u8>,
    children: Seq<ChildRegion>,
    position: CursorPosition,
)
    requires
        valid_position(source_bytes, children, position),
        owner_bytes.len() <= usize::MAX as int,
        stable_owner_view(source_address, source_bytes, owner_address, owner_bytes),
        source_address == owner_address,
        source_bytes.len() == owner_bytes.len(),
    ensures
        owner_bytes == source_bytes,
        valid_position(owner_bytes, children, position),
        bind_spec(
            source_address,
            source_bytes.len() as usize,
            owner_address,
            owner_bytes.len() as usize,
            position,
        ).is_some(),
{
}

pub open spec fn modeled_next(
    children: Seq<ChildRegion>,
    position: CursorPosition,
) -> Option<(CursorPosition, ChildRange)> {
    if position.remaining == 0 {
        None
    } else {
        let child = children[children.len() - position.remaining as int];
        Some((
            CursorPosition {
                at: child.payload_end,
                remaining: (position.remaining - 1) as usize,
            },
            ChildRange {
                start: child.payload_start,
                end: child.payload_end,
            },
        ))
    }
}

pub proof fn next_returns_exact_validated_region(
    bytes: Seq<u8>,
    children: Seq<ChildRegion>,
    position: CursorPosition,
)
    requires
        valid_position(bytes, children, position),
        position.remaining > 0,
    ensures
        modeled_next(children, position).is_some(),
        match modeled_next(children, position) {
            Some((next, range)) => {
                &&& valid_position(bytes, children, next)
                &&& range.start == children[
                    children.len() - position.remaining as int
                ].payload_start
                &&& range.end == children[
                    children.len() - position.remaining as int
                ].payload_end
                &&& range.start <= range.end
                &&& range.end as int <= bytes.len()
                &&& next.remaining + 1 == position.remaining
            },
            None => false,
        },
{
    let index = children.len() - position.remaining as int;
    assert(0 <= index < children.len());
    assert(canonical_child(bytes, children[index]));
    if position.remaining > 1 {
        assert(index + 1 < children.len());
        assert(children[index + 1].header_start == children[index].payload_end);
    } else {
        assert(index == children.len() - 1);
        assert(children[index].payload_end as int == bytes.len());
    }
}

pub proof fn exhaustion_iff_all_validated_children_consumed(
    bytes: Seq<u8>,
    children: Seq<ChildRegion>,
    position: CursorPosition,
)
    requires valid_position(bytes, children, position),
    ensures
        modeled_next(children, position).is_none() == (position.remaining == 0),
        (position.remaining == 0) ==>
            position.at as int == bytes.len(),
        (position.remaining == 0) ==>
            children.len() - position.remaining as int == children.len(),
{
}

/// A length-only mutant accepts a same-width but different owner. This is a
/// concrete counterexample to replacing the Rust pointer-and-length check.
pub proof fn equal_length_wrong_owner_mutant_is_detected() {
    let source_address: usize = 4096;
    let other_address: usize = 8192;
    let length: usize = 17;
    let position = CursorPosition { at: 0, remaining: 2 };
    assert(source_address != other_address);
    assert(length_only_mutant(length, length));
    assert(bind_spec(source_address, length, other_address, length, position).is_none());
}

/// Identical copied contents do not authorize rebinding an unrelated owner.
pub proof fn copied_body_mutant_is_detected(body: Seq<u8>)
    requires body.len() <= usize::MAX as int,
    ensures
        length_only_mutant(body.len() as usize, body.len() as usize),
        bind_spec(
            4096, body.len() as usize, 8192, body.len() as usize,
            CursorPosition { at: 0, remaining: 0 },
        ).is_none(),
{
}

/// A stale source address mutant would accept a truncated owner by pointer
/// alone. The real binding refuses it before transferring `at` or `remaining`.
pub proof fn same_pointer_truncated_owner_mutant_is_detected() {
    let address: usize = 4096;
    let position = CursorPosition { at: 5, remaining: 1 };
    assert(pointer_only_mutant(address, address));
    assert(bind_spec(address, 17, address, 16, position).is_none());
}

pub proof fn exhausted_cursor_still_refuses_wrong_owner_mutant() {
    let exhausted = CursorPosition { at: 17, remaining: 0 };
    assert(length_only_mutant(17, 17));
    assert(bind_spec(4096, 17, 8192, 17, exhausted).is_none());
}

} // verus!

fn main() {}
