//! Conditional source projection for RPB3 indexed protobuf regions.
//!
//! `BackingIdentity` includes the immutable byte, provenance, and end images.
//! Equal symbolic identities therefore have equal images by construction.
//! For Borrowed, anchors are the source/base arithmetic below. For Frozen and
//! Composed, the caller must still establish that these images are exactly the
//! results of the backing's `byte_at` and `provenance_at` APIs.
//! This file does not verify those Rust implementations, their constructors,
//! the field decoder, or the Rust-to-Verus extraction. In particular, the
//! Rust `same_view` implying equal symbolic identities remains a separate
//! extraction obligation; the model does not infer it from equal handles.
use vstd::prelude::*;

verus! {

pub struct SourceAnchor {
    pub source: int,
    pub offset: int,
}

pub enum BackingKind {
    Borrowed,
    Frozen,
    Composed,
}

/// The handle and its immutable observations form one symbolic identity. A
/// handle alone is insufficient to justify image equality across two Rust
/// views until source-tree immutability is connected to this model.
pub struct BackingIdentity {
    pub handle: usize,
    pub ghost bytes: Seq<u8>,
    pub ghost positions: Seq<SourceAnchor>,
    pub end: SourceAnchor,
}

pub struct BackingImage {
    pub kind: BackingKind,
    pub ghost identity: BackingIdentity,
}

pub struct IndexedRegionView {
    pub ghost backing: BackingImage,
    pub start: usize,
    pub len: usize,
    /// Borrowed `subview_with_end` overrides this boundary; Frozen/Composed
    /// rebase the backing and store the new boundary in `backing.identity.end`.
    pub end_override: Option<SourceAnchor>,
}

pub struct IndexedSourceImage {
    pub ghost bytes: Seq<u8>,
    pub ghost positions: Seq<SourceAnchor>,
    pub end: SourceAnchor,
}

/// Values independently obtained from the concrete Rust APIs are supplied as
/// ghost inputs. `rust_api_binds_view` is the unproved extraction obligation;
/// it must not be inferred merely from `view_wf` or this module's theorems.
pub struct RustRegionApiObservation {
    pub ghost byte_at: Seq<Option<u8>>,
    pub ghost position_at: Seq<Option<SourceAnchor>>,
    pub end_position: SourceAnchor,
}

pub open spec fn backing_wf(backing: BackingImage) -> bool {
    backing.identity.positions.len() == backing.identity.bytes.len()
        && backing.identity.bytes.len() <= usize::MAX as int
}

pub open spec fn view_wf(view: IndexedRegionView) -> bool {
    backing_wf(view.backing)
        && view.start as int + view.len as int <= view.backing.identity.bytes.len()
}

pub open spec fn root_view(backing: BackingImage) -> IndexedRegionView
    recommends backing_wf(backing),
{
    IndexedRegionView {
        start: 0,
        len: backing.identity.bytes.len() as usize,
        backing,
        end_override: None,
    }
}

pub open spec fn model_end_position(view: IndexedRegionView) -> SourceAnchor {
    if !view_wf(view) {
        view.backing.identity.end
    } else {
        match view.end_override {
            Some(end) => end,
            None => {
                let at = view.start as int + view.len as int;
                if at == view.backing.identity.bytes.len() {
                    view.backing.identity.end
                } else {
                    view.backing.identity.positions[at]
                }
            }
        }
    }
}

/// Symbolic counterparts of `IndexedByteRegion::{byte_at,position_at}`.
/// `position_at(len)` is the end boundary, including for an empty view.
pub open spec fn model_byte_at(view: IndexedRegionView, at: usize) -> Option<u8> {
    if view_wf(view) && at < view.len {
        Some(view.backing.identity.bytes[view.start as int + at as int])
    } else {
        None
    }
}

pub open spec fn model_position_at(
    view: IndexedRegionView, at: usize,
) -> Option<SourceAnchor> {
    if !view_wf(view) || at > view.len {
        None
    } else if at == view.len {
        Some(model_end_position(view))
    } else {
        Some(view.backing.identity.positions[view.start as int + at as int])
    }
}

pub open spec fn project(view: IndexedRegionView) -> IndexedSourceImage
    recommends view_wf(view),
{
    IndexedSourceImage {
        bytes: Seq::new(view.len as nat, |i: int|
            view.backing.identity.bytes[view.start as int + i]),
        positions: Seq::new(view.len as nat, |i: int|
            view.backing.identity.positions[view.start as int + i]),
        end: model_end_position(view),
    }
}

pub open spec fn rust_api_binds_view(
    view: IndexedRegionView, observed: RustRegionApiObservation,
) -> bool {
    &&& view_wf(view)
    &&& observed.byte_at.len() == view.len as int
    &&& observed.position_at.len() == view.len as int + 1
    &&& forall|i: int| 0 <= i < view.len as int ==>
        observed.byte_at[i] == model_byte_at(view, i as usize)
    &&& forall|i: int| 0 <= i <= view.len as int ==>
        observed.position_at[i] == model_position_at(view, i as usize)
    &&& observed.end_position == model_end_position(view)
}

pub proof fn bound_rust_api_projects_exactly(
    view: IndexedRegionView, observed: RustRegionApiObservation,
)
    requires rust_api_binds_view(view, observed),
    ensures
        forall|i: int| 0 <= i < view.len as int ==>
            observed.byte_at[i] == Some(project(view).bytes[i]),
        forall|i: int| 0 <= i < view.len as int ==>
            observed.position_at[i] == Some(project(view).positions[i]),
        observed.position_at[view.len as int] == Some(project(view).end),
        observed.end_position == project(view).end,
{
    indexed_view_projects_exactly(view);
}

pub proof fn indexed_view_projects_exactly(view: IndexedRegionView)
    requires view_wf(view),
    ensures
        project(view).bytes.len() == view.len as int,
        project(view).positions.len() == view.len as int,
        forall|i: usize| i < view.len ==>
            model_byte_at(view, i) == Some(project(view).bytes[i as int]),
        forall|i: usize| i < view.len ==>
            model_position_at(view, i) == Some(project(view).positions[i as int]),
        model_position_at(view, view.len) == Some(project(view).end),
        model_byte_at(view, view.len).is_none(),
{
    assert forall|i: usize| i < view.len implies
        model_byte_at(view, i) == Some(project(view).bytes[i as int]) by {};
    assert forall|i: usize| i < view.len implies
        model_position_at(view, i) == Some(project(view).positions[i as int]) by {};
}

/// Borrowed `position_at(i)` uses one source ID and base-plus-index until the
/// explicit end boundary. `base + bytes.len()` must fit the Rust offset type.
pub open spec fn borrowed_backing(
    identity: usize, bytes: Seq<u8>, source: u32, base: usize,
    end: SourceAnchor,
) -> BackingImage {
    BackingImage {
        kind: BackingKind::Borrowed,
        identity: BackingIdentity {
            handle: identity,
            positions: Seq::new(bytes.len(), |i: int| SourceAnchor {
                source: source as int,
                offset: base as int + i,
            }),
            bytes,
            end,
        },
    }
}

pub proof fn borrowed_root_projection_exact(
    identity: usize, bytes: Seq<u8>, source: u32, base: usize,
    end: SourceAnchor,
)
    requires base as int + bytes.len() <= usize::MAX as int,
    ensures
        project(root_view(borrowed_backing(identity, bytes, source, base, end))).bytes
            == bytes,
        project(root_view(borrowed_backing(identity, bytes, source, base, end))).end
            == end,
        forall|i: usize| (i as int) < bytes.len() ==>
            model_position_at(
                root_view(borrowed_backing(identity, bytes, source, base, end)), i,
            ) == Some(SourceAnchor {
                source: source as int,
                offset: base as int + i as int,
            }),
{
    let backing = borrowed_backing(identity, bytes, source, base, end);
    assert(backing_wf(backing));
    indexed_view_projects_exactly(root_view(backing));
    assert forall|i: int| 0 <= i < bytes.len() implies
        project(root_view(backing)).bytes[i] == bytes[i] by {};
    assert(project(root_view(backing)).bytes =~= bytes);
    assert forall|i: usize| (i as int) < bytes.len() implies
        model_position_at(root_view(backing), i) == Some(SourceAnchor {
            source: source as int,
            offset: base as int + i as int,
        }) by {};
}

/// Frozen and Composed roots permit arbitrary discontinuous physical anchors.
/// The supplied backing images are premises, not inferred from Rust pointers.
pub proof fn frozen_root_projection_exact(backing: BackingImage)
    requires backing_wf(backing), backing.kind == BackingKind::Frozen,
    ensures
        project(root_view(backing)).bytes == backing.identity.bytes,
        project(root_view(backing)).positions == backing.identity.positions,
        project(root_view(backing)).end == backing.identity.end,
{
    indexed_view_projects_exactly(root_view(backing));
    assert forall|i: int| 0 <= i < backing.identity.bytes.len() implies
        project(root_view(backing)).bytes[i] == backing.identity.bytes[i] by {};
    assert(project(root_view(backing)).bytes =~= backing.identity.bytes);
    assert forall|i: int| 0 <= i < backing.identity.positions.len() implies
        project(root_view(backing)).positions[i] == backing.identity.positions[i] by {};
    assert(project(root_view(backing)).positions =~= backing.identity.positions);
}

pub proof fn composed_root_projection_exact(backing: BackingImage)
    requires backing_wf(backing), backing.kind == BackingKind::Composed,
    ensures
        project(root_view(backing)).bytes == backing.identity.bytes,
        project(root_view(backing)).positions == backing.identity.positions,
        project(root_view(backing)).end == backing.identity.end,
{
    indexed_view_projects_exactly(root_view(backing));
    assert forall|i: int| 0 <= i < backing.identity.bytes.len() implies
        project(root_view(backing)).bytes[i] == backing.identity.bytes[i] by {};
    assert(project(root_view(backing)).bytes =~= backing.identity.bytes);
    assert forall|i: int| 0 <= i < backing.identity.positions.len() implies
        project(root_view(backing)).positions[i] == backing.identity.positions[i] by {};
    assert(project(root_view(backing)).positions =~= backing.identity.positions);
}

pub proof fn empty_root_projection_exact(backing: BackingImage)
    requires backing_wf(backing), backing.identity.bytes.len() == 0,
    ensures
        project(root_view(backing)).bytes.len() == 0,
        project(root_view(backing)).positions.len() == 0,
        model_position_at(root_view(backing), 0) == Some(backing.identity.end),
        model_byte_at(root_view(backing), 0).is_none(),
{
    indexed_view_projects_exactly(root_view(backing));
}

/// Describes a successful `subview`: the same backing, shifted logical start,
/// and the parent end override only when the child reaches the parent's end.
/// Rust may return `None` on checked-arithmetic or backing-slice failure;
/// symbolic range validity alone does not prove a Rust call succeeds.
pub open spec fn model_subview(
    view: IndexedRegionView, start: usize, end: usize,
) -> Option<IndexedRegionView> {
    if !view_wf(view) || start > end || end > view.len {
        None
    } else {
        Some(IndexedRegionView {
            backing: view.backing,
            start: (view.start as int + start as int) as usize,
            len: (end as int - start as int) as usize,
            end_override: if end == view.len { view.end_override } else { None },
        })
    }
}

pub open spec fn model_subview_with_end(
    view: IndexedRegionView, start: usize, end: usize,
    explicit_end: SourceAnchor,
    rebased_root_identity: usize,
) -> Option<IndexedRegionView> {
    match model_subview(view, start, end) {
        None => None,
        Some(child) => match child.backing.kind {
            // Borrowed changes only its terminal metadata.
            BackingKind::Borrowed => Some(IndexedRegionView {
                backing: child.backing,
                start: child.start,
                len: child.len,
                end_override: Some(explicit_end),
            }),
            // Frozen and Composed call `root.subview`, then reset `start` to
            // zero. The supplied handle and projected image together identify
            // that new symbolic root. Binding it to the returned Rust root is
            // still an extraction obligation.
            kind => Some(IndexedRegionView {
                backing: BackingImage {
                    kind,
                    identity: BackingIdentity {
                        handle: rebased_root_identity,
                        bytes: project(child).bytes,
                        positions: project(child).positions,
                        end: explicit_end,
                    },
                },
                start: 0,
                len: child.len,
                end_override: None,
            }),
        },
    }
}

pub proof fn subview_projection_exact(
    view: IndexedRegionView, start: usize, end: usize,
)
    requires view_wf(view), start <= end, end <= view.len,
    ensures
        model_subview(view, start, end).is_some(),
        view_wf(model_subview(view, start, end).unwrap()),
        project(model_subview(view, start, end).unwrap()).bytes
            == project(view).bytes.subrange(start as int, end as int),
        project(model_subview(view, start, end).unwrap()).positions
            == project(view).positions.subrange(start as int, end as int),
        project(model_subview(view, start, end).unwrap()).end
            == model_position_at(view, end).unwrap(),
{
    let child = model_subview(view, start, end).unwrap();
    assert(view_wf(child));
    indexed_view_projects_exactly(child);
    assert forall|i: int| 0 <= i < child.len as int implies
        project(child).bytes[i]
            == project(view).bytes.subrange(start as int, end as int)[i] by {};
    assert(project(child).bytes =~= project(view).bytes.subrange(start as int, end as int));
    assert forall|i: int| 0 <= i < child.len as int implies
        project(child).positions[i]
            == project(view).positions.subrange(start as int, end as int)[i] by {};
    assert(project(child).positions =~= project(view).positions.subrange(start as int, end as int));
}

pub proof fn empty_subview_projection_exact(view: IndexedRegionView, at: usize)
    requires view_wf(view), at <= view.len,
    ensures
        project(model_subview(view, at, at).unwrap()).bytes.len() == 0,
        project(model_subview(view, at, at).unwrap()).positions.len() == 0,
        model_position_at(model_subview(view, at, at).unwrap(), 0)
            == model_position_at(view, at),
{
    subview_projection_exact(view, at, at);
}

pub proof fn subview_with_end_projection_exact(
    view: IndexedRegionView, start: usize, end: usize,
    explicit_end: SourceAnchor,
    rebased_root_identity: usize,
)
    requires view_wf(view), start <= end, end <= view.len,
    ensures
        model_subview_with_end(view, start, end, explicit_end, rebased_root_identity).is_some(),
        view_wf(model_subview_with_end(view, start, end, explicit_end, rebased_root_identity).unwrap()),
        project(model_subview_with_end(view, start, end, explicit_end, rebased_root_identity).unwrap()).bytes
            == project(view).bytes.subrange(start as int, end as int),
        project(model_subview_with_end(view, start, end, explicit_end, rebased_root_identity).unwrap()).positions
            == project(view).positions.subrange(start as int, end as int),
        model_position_at(
            model_subview_with_end(view, start, end, explicit_end, rebased_root_identity).unwrap(),
            (end as int - start as int) as usize,
        ) == Some(explicit_end),
{
    subview_projection_exact(view, start, end);
    let child = model_subview_with_end(view, start, end, explicit_end, rebased_root_identity).unwrap();
    let ordinary = model_subview(view, start, end).unwrap();
    indexed_view_projects_exactly(ordinary);
    assert(view_wf(child));
    indexed_view_projects_exactly(child);
    assert forall|i: int| 0 <= i < child.len as int implies
        project(child).bytes[i] == project(ordinary).bytes[i] by {};
    assert(project(child).bytes =~= project(ordinary).bytes);
    assert forall|i: int| 0 <= i < child.len as int implies
        project(child).positions[i] == project(ordinary).positions[i] by {};
    assert(project(child).positions =~= project(ordinary).positions);
}

/// Equality here includes the entire immutable symbolic image, not merely a
/// Rust pointer or handle. Proving that concrete `same_view` implies this
/// predicate remains a separate Rust-to-model extraction obligation.
pub open spec fn model_same_view(left: IndexedRegionView, right: IndexedRegionView) -> bool {
    left.backing.kind == right.backing.kind
        && left.backing.identity == right.backing.identity
        && left.start == right.start
        && left.len == right.len
        && left.end_override == right.end_override
}

pub proof fn same_view_preserves_image(
    left: IndexedRegionView, right: IndexedRegionView,
)
    requires
        view_wf(left), view_wf(right),
        model_same_view(left, right),
    ensures
        project(left).bytes == project(right).bytes,
        project(left).positions == project(right).positions,
        project(left).end == project(right).end,
{
    assert(left.backing.identity.bytes == right.backing.identity.bytes);
    assert(left.backing.identity.positions == right.backing.identity.positions);
    assert(left.backing.identity.end == right.backing.identity.end);
    assert forall|i: int| 0 <= i < left.len as int implies
        project(left).bytes[i] == project(right).bytes[i] by {};
    assert(project(left).bytes =~= project(right).bytes);
    assert forall|i: int| 0 <= i < left.len as int implies
        project(left).positions[i] == project(right).positions[i] by {};
    assert(project(left).positions =~= project(right).positions);
}

} // verus!

fn main() {}
