//! Bridges a linked EPM1 terminal child to CanonicalPath's kind-4 return.
//!
//! The terminal `EpmFrame` supplies the mode and accumulated entry-stability
//! flag. Its indexed snapshot supplies every child byte and physical anchor.
//! This does not establish that an arbitrary terminal frame is reachable from
//! a production EPM parser run, nor extract the CanonicalPath reducer wait.

use vstd::prelude::*;

#[cfg(not(rpb3_integrated))]
#[path = "rpb3_epathmap_event_refinement.rs"]
pub mod epm;
#[cfg(not(rpb3_integrated))]
use epm::admission;

#[cfg(rpb3_integrated)]
use crate::rpb3_admission_cursor as admission;
#[cfg(rpb3_integrated)]
use crate::rpb3_epathmap_event_refinement as epm;

verus! {

pub open spec fn canonical_path_epm_mode_code(mode: epm::EpmMode) -> u8 {
    match mode {
        epm::EpmMode::Empty => 0,
        epm::EpmMode::Set => 1,
        epm::EpmMode::Map => 2,
    }
}

pub open spec fn canonical_path_epm_indexed_as_admission(
    image: epm::IndexedRegionImage,
) -> admission::AdmissionIndexedSourceImage {
    admission::AdmissionIndexedSourceImage {
        bytes: image.bytes,
        positions: Seq::new(image.positions.len() as nat, |i: int|
            admission::AdmissionSourceAnchor {
                source: image.positions[i].source,
                offset: image.positions[i].offset,
            },
        ),
        end: admission::AdmissionSourceAnchor {
            source: image.end.source,
            offset: image.end.offset,
        },
    }
}

/// The core return-sidecar sequence carries the one accumulated EPM flag.
/// Its singleton shape makes the core `forall` check exactly that flag; it
/// does not assert a per-entry trace that the EPM model does not expose.
pub open spec fn canonical_path_epm_derived_return_source(
    before: epm::LinkedEpmState,
    push: admission::AdmissionCanonicalPathPushSource,
) -> admission::AdmissionCanonicalPathReturnSource {
    admission::AdmissionCanonicalPathReturnSource {
        push,
        reducer_waiting_id: push.reducer_obligation_id,
        reducer_waiting_kind: push.kind,
        stack_return_id: push.stack_return_id,
        epm_entry_stability: seq![before.epm.entries_stable],
    }
}

pub open spec fn canonical_path_epm_derived_summary(
    before: epm::LinkedEpmState,
) -> admission::AdmissionReturnedKind {
    admission::AdmissionReturnedKind::EPathMapSnapshot {
        mode: canonical_path_epm_mode_code(before.epm.mode),
        entries_canonical_path_stable: before.epm.entries_stable,
    }
}

/// An EPM1 end event alone permits Empty/Set/unstable summaries; the
/// CanonicalPath reducer accepts only a stable Map. The linked completion
/// and frame layout bind this successful case to the actual indexed header.
pub open spec fn canonical_path_epm_successful_source_completion(
    before: epm::LinkedEpmState,
    finished: epm::LinkedEpmState,
    push: admission::AdmissionCanonicalPathPushSource,
) -> bool {
    &&& epm::linked_epm_complete(
        before, finished,
        epm::EpmAction::Complete {
            mode: before.epm.mode,
            entries_stable: before.epm.entries_stable,
        },
    )
    &&& epm::frame_layout_wf(before.epm)
    &&& before.epm.mode == epm::EpmMode::Map
    &&& before.epm.entries_stable
    &&& before.admission.frames.len() >= 2
    &&& before.admission.frames.last().score_handle.is_none()
    &&& push.kind == admission::AdmissionCanonicalPathObligationKind::NestedEpm1
    &&& push.child == canonical_path_epm_indexed_as_admission(before.epm.snapshot)
}

pub proof fn canonical_path_epm_completion_derives_return_summary(
    before: epm::LinkedEpmState,
    finished: epm::LinkedEpmState,
    push: admission::AdmissionCanonicalPathPushSource,
)
    requires canonical_path_epm_successful_source_completion(before, finished, push),
    ensures
        admission::admission_canonical_path_return_summary_from_source(
            before.admission,
            canonical_path_epm_derived_return_source(before, push),
            canonical_path_epm_derived_summary(before),
        ),
        admission::admission_canonical_path_return_sidecar_after(
            canonical_path_epm_derived_return_source(before, push),
            canonical_path_epm_derived_summary(before),
        ).is_some(),
        push.child.bytes == before.epm.snapshot.bytes,
        push.child.end == (admission::AdmissionSourceAnchor {
            source: before.epm.snapshot.end.source,
            offset: before.epm.snapshot.end.offset,
        }),
{
    assert(epm::frame_layout_wf(before.epm));
    assert(epm::epm_prefix_from_bytes(before.epm.snapshot.bytes).is_some());
    let prefix = epm::epm_prefix_from_bytes(before.epm.snapshot.bytes).unwrap();
    assert(prefix.mode == epm::EpmMode::Map);
    assert(before.epm.snapshot.bytes.len() >= 6);
    assert(before.epm.snapshot.bytes[5] == 2);
    assert(admission::admission_canonical_path_epm_mode_from_bytes(
        push.child.bytes,
    ) == Some(2));
    assert(canonical_path_epm_derived_return_source(
        before, push,
    ).epm_entry_stability == seq![true]);
    assert(admission::admission_canonical_path_return_summary_from_source(
        before.admission,
        canonical_path_epm_derived_return_source(before, push),
        canonical_path_epm_derived_summary(before),
    ));
}

}

fn main() {}
