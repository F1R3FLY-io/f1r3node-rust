//! Path-local writer for the shared retention V2 metadata.
//!
//! A successor moves the predecessor's current state into the history and
//! appoints the target as current. Under V2 that is one compare-and-swap edit
//! of the history radix at the predecessor's 48-byte key and one new 136-byte
//! anchor: the radix editor reads and writes only the edited search path, so
//! the work does not grow with the history. The successor's history count is
//! derived from the successful compare-and-swap outcomes, never from the
//! number of newly stored objects, which undercounts an aliased leaf.
//!
//! A V1 source migrates once: its history followed by its current state is
//! inserted as one sorted batch into the empty radix, which yields the
//! canonical radix of that history.

use std::num::NonZeroU64;
use std::sync::Arc;

use super::super::canonical::{CanonicalEncode, CanonicalFormatError, ObjectRefV1};
use super::super::object::EndpointValue;
use super::super::rewrite::{
    DagRewriteLimits, DagRewriteReport, RewriteError, rewrite_retention_history,
};
use super::super::store::MutableObjectStore;
use super::v2_format::{RetentionAnchorV2, RetentionKeyV2, empty_retention_radix_ref};
use super::{RetainedStateV1, RetentionManifestView};

/// One retention V2 metadata step: the appointed successor anchor, staged
/// together with every radix node the edit wrote, and the editor's work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetentionStepV2 {
    anchor: RetentionAnchorV2,
    report: DagRewriteReport,
}

impl RetentionStepV2 {
    pub(crate) const fn anchor(self) -> RetentionAnchorV2 { self.anchor }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the editor work counters are read by the 7926 properties")
    )]
    pub(crate) const fn report(self) -> DagRewriteReport { self.report }
}

/// One compare-and-swap edit of the history radix.
type RetentionEditV2 = (RetentionKeyV2, EndpointValue, EndpointValue);

/// Commit a successor: insert the predecessor's current state into the
/// history and appoint `target` as current.
pub(crate) fn commit_retention_v2<S: MutableObjectStore>(
    store: &S,
    predecessor: RetentionAnchorV2,
    target: RetainedStateV1,
    limits: DagRewriteLimits,
) -> Result<RetentionStepV2, RewriteError<S::Error>> {
    let edits = [insertion(predecessor.current())];
    let (root, report) = edit_history(store, predecessor.historical_root(), &edits, limits)?;
    let count = cas_count(predecessor.historical_count(), &edits)?;
    stage_anchor(store, RetentionAnchorV2::new(target, count, root)?, report)
}

/// Evict one historical state, keeping the current state.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "task 7756 adds the retention policy that evicts history")
)]
pub(crate) fn evict_retention_v2<S: MutableObjectStore>(
    store: &S,
    predecessor: RetentionAnchorV2,
    victim: RetainedStateV1,
    limits: DagRewriteLimits,
) -> Result<RetentionStepV2, RewriteError<S::Error>> {
    let edits = [removal(victim)];
    let (root, report) = edit_history(store, predecessor.historical_root(), &edits, limits)?;
    let count = cas_count(predecessor.historical_count(), &edits)?;
    stage_anchor(store, RetentionAnchorV2::new(predecessor.current(), count, root)?, report)
}

/// Migrate a V1 source once, at a commit: its history followed by its
/// current state is inserted as one sorted batch into the empty radix, so the
/// successor anchor equals committing `target` on the migrated anchor.
pub(crate) fn migrate_retention_v1<S: MutableObjectStore>(
    store: &S,
    manifest: RetentionManifestView<'_>,
    target: RetainedStateV1,
    limits: DagRewriteLimits,
) -> Result<RetentionStepV2, RewriteError<S::Error>> {
    let historical = manifest.historical();
    let capacity = historical
        .len()
        .checked_add(1)
        .ok_or(CanonicalFormatError::CounterOverflow("retention migration entries"))?;
    let mut edits = Vec::new();
    edits
        .try_reserve_exact(capacity)
        .map_err(|_| CanonicalFormatError::AllocationFailure {
            requested: u64::try_from(capacity).unwrap_or(u64::MAX),
        })?;
    for retained in historical {
        edits.push(insertion(retained?));
    }
    edits.push(insertion(manifest.current()));
    let (root, report) = edit_history(store, empty_retention_radix_ref(), &edits, limits)?;
    let count = cas_count(0, &edits)?;
    stage_anchor(store, RetentionAnchorV2::new(target, count, root)?, report)
}

fn insertion(state: RetainedStateV1) -> RetentionEditV2 {
    (RetentionKeyV2::new(state.state()), EndpointValue::Absent, claim(state))
}

fn removal(state: RetainedStateV1) -> RetentionEditV2 {
    (RetentionKeyV2::new(state.state()), claim(state), EndpointValue::Absent)
}

fn claim(state: RetainedStateV1) -> EndpointValue {
    EndpointValue::Multiplicity(
        NonZeroU64::new(state.canonical_bytes())
            .expect("a retained state claims at least its manifest's encoded length"),
    )
}

fn edit_history<S: MutableObjectStore>(
    store: &S,
    root: ObjectRefV1,
    edits: &[RetentionEditV2],
    limits: DagRewriteLimits,
) -> Result<(ObjectRefV1, DagRewriteReport), RewriteError<S::Error>> {
    rewrite_retention_history(
        store,
        root,
        edits
            .iter()
            .map(|(key, before, after)| (&key.as_bytes()[..], *before, *after)),
        limits,
    )
}

/// The history count after a batch whose every compare-and-swap succeeded:
/// an insertion adds one, a removal removes one, and any other outcome keeps
/// the count.
fn cas_count(count: u64, edits: &[RetentionEditV2]) -> Result<u64, CanonicalFormatError> {
    edits.iter().try_fold(count, |count, (_, before, after)| {
        match (before, after) {
            (EndpointValue::Absent, EndpointValue::Multiplicity(_)) => count.checked_add(1),
            (EndpointValue::Multiplicity(_), EndpointValue::Absent) => count.checked_sub(1),
            _ => Some(count),
        }
        .ok_or(CanonicalFormatError::CounterOverflow("retention history count"))
    })
}

fn stage_anchor<S: MutableObjectStore>(
    store: &S,
    anchor: RetentionAnchorV2,
    report: DagRewriteReport,
) -> Result<RetentionStepV2, RewriteError<S::Error>> {
    let bytes: Arc<[u8]> = anchor.try_canonical_bytes()?.into();
    store
        .put_if_absent_or_equal(anchor.object_ref(), bytes)
        .map_err(|error| RewriteError::Store(Box::new(error)))?;
    Ok(RetentionStepV2 { anchor, report })
}
