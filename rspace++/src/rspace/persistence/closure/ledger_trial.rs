use pathmap::PathMap;

use super::{
    ClosureDomainV1, ClosureError, ClosureInventoryV1, DigestState, StructuralClosureLedgerV1,
    format_error, load_reference,
};
use crate::rspace::persistence::canonical::{CanonicalFormatError, ObjectRefV1, checked_len};
use crate::rspace::persistence::object::{
    CanonicalDagNodeView, CanonicalObjectKind, CanonicalObjectView, EndpointValue,
};
use crate::rspace::persistence::retention::v2_format::{
    RETENTION_RADIX_DOMAIN, RetentionAnchorV2, RetentionReader,
};
use crate::rspace::persistence::store::ImmutableObjectStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LedgerTrialAxisV1 {
    Count,
    UnionObjects,
    UnionBytes,
    Scratch,
    References,
    Expansions,
    QuotedBytes,
    FinalObjects,
    FinalBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LedgerTrialLimitsV1(pub(crate) [u64; 9]);

impl LedgerTrialLimitsV1 {
    pub(crate) const UNBOUNDED: Self = Self([u64::MAX; 9]);

    /// Replace one axis's budget, so a caller names every budget it sets.
    pub(crate) const fn with(mut self, axis: LedgerTrialAxisV1, limit: u64) -> Self {
        self.0[axis as usize] = limit;
        self
    }

    const fn get(self, axis: LedgerTrialAxisV1) -> u64 { self.0[axis as usize] }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LedgerTrialUsageV1 {
    pub(crate) peak_count: u64,
    pub(crate) peak_objects: u64,
    pub(crate) peak_bytes: u64,
    pub(crate) peak_scratch: u64,
    pub(crate) references: u64,
    pub(crate) expansions: u64,
    pub(crate) quoted_bytes: u64,
    pub(crate) consumed_bytes: u64,
}

#[derive(Debug)]
pub(crate) enum LedgerTrialRefusalV1<E> {
    Budget(LedgerTrialAxisV1),
    UnauthorizedRemoval,
    ZeroCountDecrement,
    InventoryUnderflow,
    Closure(ClosureError<E>),
}

pub(crate) struct PreparedLedgerTrialV1 {
    pub(crate) successor: StructuralClosureLedgerV1,
    pub(crate) usage: LedgerTrialUsageV1,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RootDeltaV1<'a> {
    pub(crate) added: &'a [ObjectRefV1],
    pub(crate) removed: &'a [ObjectRefV1],
}

pub(crate) struct PreparedDomainTrialsV1 {
    pub(crate) successor: StructuralClosureLedgerV1,
    pub(crate) usage: [LedgerTrialUsageV1; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Activation,
    Deactivation,
}

#[derive(Clone, Copy, Debug)]
struct TrialCursor {
    start: usize,
    next: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug)]
struct TrialCharge {
    count: u128,
    objects: u128,
    bytes: u128,
    live_scratch: u128,
    references: u64,
    expansions: u64,
    quoted_bytes: u64,
    consumed_bytes: u64,
}

struct LedgerTrial<'a, S> {
    store: &'a S,
    domain: ClosureDomainV1,
    limits: LedgerTrialLimitsV1,
    states: PathMap<DigestState>,
    live: ClosureInventoryV1,
    usage: LedgerTrialUsageV1,
    events: Vec<ObjectRefV1>,
    cursors: Vec<TrialCursor>,
    references: Vec<ObjectRefV1>,
}

pub(crate) fn run_ledger_trial<S>(
    store: &S,
    predecessor: &StructuralClosureLedgerV1,
    domain: ClosureDomainV1,
    added: &[ObjectRefV1],
    removed: &[ObjectRefV1],
    limits: LedgerTrialLimitsV1,
) -> Result<PreparedLedgerTrialV1, LedgerTrialRefusalV1<S::Error>>
where
    S: ImmutableObjectStore,
{
    let index = domain.index();
    let roots = authorized_roots(&predecessor.roots, index, added, removed)?;
    let mut trial = LedgerTrial {
        store,
        domain,
        limits,
        states: predecessor.states.clone(),
        live: predecessor.inventories[index],
        usage: LedgerTrialUsageV1::default(),
        events: Vec::new(),
        cursors: Vec::new(),
        references: Vec::new(),
    };
    trial.run_phase(Phase::Activation, added)?;
    trial.run_phase(Phase::Deactivation, removed)?;
    if trial.live.distinct_objects > limits.get(LedgerTrialAxisV1::FinalObjects) {
        return Err(LedgerTrialRefusalV1::Budget(LedgerTrialAxisV1::FinalObjects));
    }
    if trial.live.canonical_bytes > limits.get(LedgerTrialAxisV1::FinalBytes) {
        return Err(LedgerTrialRefusalV1::Budget(LedgerTrialAxisV1::FinalBytes));
    }
    let mut inventories = predecessor.inventories;
    inventories[index] = trial.live;
    Ok(PreparedLedgerTrialV1 {
        successor: StructuralClosureLedgerV1 {
            states: trial.states,
            roots,
            next_epoch: predecessor.next_epoch,
            inventories,
        },
        usage: trial.usage,
    })
}

pub(crate) fn run_domain_trials<S>(
    store: &S,
    predecessor: &StructuralClosureLedgerV1,
    current: RootDeltaV1<'_>,
    retained: RootDeltaV1<'_>,
    limits: [LedgerTrialLimitsV1; 2],
) -> Result<PreparedDomainTrialsV1, (ClosureDomainV1, LedgerTrialRefusalV1<S::Error>)>
where
    S: ImmutableObjectStore,
{
    let current_trial = run_ledger_trial(
        store,
        predecessor,
        ClosureDomainV1::Current,
        current.added,
        current.removed,
        limits[ClosureDomainV1::Current.index()],
    )
    .map_err(|refusal| (ClosureDomainV1::Current, refusal))?;
    let retained_trial = run_ledger_trial(
        store,
        &current_trial.successor,
        ClosureDomainV1::Retained,
        retained.added,
        retained.removed,
        limits[ClosureDomainV1::Retained.index()],
    )
    .map_err(|refusal| (ClosureDomainV1::Retained, refusal))?;
    Ok(PreparedDomainTrialsV1 {
        successor: retained_trial.successor,
        usage: [current_trial.usage, retained_trial.usage],
    })
}

fn authorized_roots<E>(
    predecessor: &PathMap<[u64; 3]>,
    domain: usize,
    added: &[ObjectRefV1],
    removed: &[ObjectRefV1],
) -> Result<PathMap<[u64; 3]>, LedgerTrialRefusalV1<E>> {
    let mut roots = predecessor.clone();
    for reference in added {
        let key = reference.digest();
        let mut counts = roots.get_val_at(key.as_bytes()).copied().unwrap_or([0; 3]);
        counts[domain] = counts[domain].checked_add(1).ok_or_else(|| {
            LedgerTrialRefusalV1::Closure(format_error(CanonicalFormatError::CounterOverflow(
                "closure root multiplicities",
            )))
        })?;
        roots.insert(key.as_bytes(), counts);
    }
    for reference in removed {
        let key = reference.digest();
        let mut counts = roots.get_val_at(key.as_bytes()).copied().unwrap_or([0; 3]);
        counts[domain] = match counts[domain] {
            0 => return Err(LedgerTrialRefusalV1::UnauthorizedRemoval),
            positive => positive - 1,
        };
        roots.insert(key.as_bytes(), counts);
    }
    Ok(roots)
}

impl<S> LedgerTrial<'_, S>
where S: ImmutableObjectStore
{
    fn run_phase(
        &mut self,
        phase: Phase,
        roots: &[ObjectRefV1],
    ) -> Result<(), LedgerTrialRefusalV1<S::Error>> {
        self.events.clear();
        self.events
            .try_reserve(roots.len())
            .map_err(|_| allocation_failure(roots.len()))?;
        self.events.extend_from_slice(roots);
        let start = TrialCharge {
            count: 0,
            objects: u128::from(self.live.distinct_objects),
            bytes: u128::from(self.live.canonical_bytes),
            live_scratch: self.live_scratch(),
            references: checked_len(roots.len(), "ledger trial root events")
                .map_err(|error| LedgerTrialRefusalV1::Closure(format_error(error)))?,
            expansions: 0,
            quoted_bytes: 0,
            consumed_bytes: 0,
        };
        self.reserve(&start)?;
        loop {
            match (self.events.last().copied(), self.cursors.last().copied()) {
                (Some(reference), _) => self.consume_event(phase, reference)?,
                (None, Some(cursor)) if cursor.next < cursor.end => self.transfer()?,
                (None, Some(cursor)) => self.discard(cursor)?,
                (None, None) => return Ok(()),
            }
        }
    }

    fn live_scratch(&self) -> u128 { self.events.len() as u128 + self.cursors.len() as u128 }

    fn steady_charge(&self, live_scratch: u128) -> TrialCharge {
        TrialCharge {
            count: 0,
            objects: u128::from(self.live.distinct_objects),
            bytes: u128::from(self.live.canonical_bytes),
            live_scratch,
            references: 0,
            expansions: 0,
            quoted_bytes: 0,
            consumed_bytes: 0,
        }
    }

    fn consume_event(
        &mut self,
        phase: Phase,
        reference: ObjectRefV1,
    ) -> Result<(), LedgerTrialRefusalV1<S::Error>> {
        let key = reference.digest();
        let mut state = match self.states.get_val_at(key.as_bytes()).copied() {
            Some(state) if state.declared_len != reference.encoded_len() => {
                return Err(LedgerTrialRefusalV1::Closure(format_error(
                    CanonicalFormatError::ObjectLengthMismatch {
                        expected: state.declared_len,
                        actual: reference.encoded_len(),
                    },
                )));
            }
            Some(state) => state,
            None => DigestState::fresh(reference.encoded_len()),
        };
        let before = state.counts[self.domain.index()];
        let (after, charged_count, crossing) = match (phase, before) {
            (Phase::Activation, _) => {
                let after = u128::from(before) + 1;
                (after, after, before == 0)
            }
            (Phase::Deactivation, 0) => return Err(LedgerTrialRefusalV1::ZeroCountDecrement),
            (Phase::Deactivation, positive) => (u128::from(positive - 1), 0, positive == 1),
        };
        let mut charge = self.steady_charge(self.live_scratch() - 1);
        charge.count = charged_count;
        if crossing {
            let weight = reference.encoded_len();
            charge.live_scratch += 1;
            charge.expansions = 1;
            charge.quoted_bytes = weight;
            charge.consumed_bytes = weight;
            match phase {
                Phase::Activation => {
                    charge.objects += 1;
                    charge.bytes += u128::from(weight);
                }
                Phase::Deactivation => {
                    charge.objects = charge
                        .objects
                        .checked_sub(1)
                        .ok_or(LedgerTrialRefusalV1::InventoryUnderflow)?;
                    charge.bytes = charge
                        .bytes
                        .checked_sub(u128::from(weight))
                        .ok_or(LedgerTrialRefusalV1::InventoryUnderflow)?;
                }
            }
        }
        self.reserve(&charge)?;
        if crossing {
            self.push_cursor(reference)?;
        }
        self.events.pop();
        state.counts[self.domain.index()] =
            u64::try_from(after).expect("a reserved reference count is within its u64 limit");
        self.states.insert(key.as_bytes(), state);
        self.live = ClosureInventoryV1 {
            distinct_objects: u64::try_from(charge.objects)
                .expect("reserved live objects are within their peak"),
            canonical_bytes: u64::try_from(charge.bytes)
                .expect("reserved live bytes are within their peak"),
        };
        Ok(())
    }

    fn transfer(&mut self) -> Result<(), LedgerTrialRefusalV1<S::Error>> {
        let mut charge = self.steady_charge(self.live_scratch() + 1);
        charge.references = 1;
        self.reserve(&charge)?;
        self.events
            .try_reserve(1)
            .map_err(|_| allocation_failure(self.events.len() + 1))?;
        let top = self
            .cursors
            .last_mut()
            .expect("a scheduled transfer has a top cursor");
        let reference = self.references[top.next];
        top.next += 1;
        self.events.push(reference);
        Ok(())
    }

    fn discard(&mut self, cursor: TrialCursor) -> Result<(), LedgerTrialRefusalV1<S::Error>> {
        let charge = self.steady_charge(self.live_scratch() - 1);
        self.reserve(&charge)?;
        self.references.truncate(cursor.start);
        self.cursors.pop();
        Ok(())
    }

    fn push_cursor(
        &mut self,
        reference: ObjectRefV1,
    ) -> Result<(), LedgerTrialRefusalV1<S::Error>> {
        let bytes = load_reference(self.store, reference).map_err(LedgerTrialRefusalV1::Closure)?;
        let start = self.references.len();
        match self.domain {
            ClosureDomainV1::Metadata => {
                self.push_metadata_references(reference, bytes.as_ref())?
            }
            ClosureDomainV1::Current | ClosureDomainV1::Retained => {
                self.push_semantic_references(reference, bytes.as_ref())?
            }
        }
        self.cursors
            .try_reserve(1)
            .map_err(|_| allocation_failure(self.cursors.len() + 1))?;
        self.cursors.push(TrialCursor {
            start,
            next: start,
            end: self.references.len(),
        });
        Ok(())
    }

    /// A metadata object is a retention anchor, which references its radix
    /// root, or a radix node, which references its children. Every other
    /// domain is refused, so no state-DAG object enters the metadata domain.
    fn push_metadata_references(
        &mut self,
        reference: ObjectRefV1,
        bytes: &[u8],
    ) -> Result<(), LedgerTrialRefusalV1<S::Error>> {
        let closure = |error| LedgerTrialRefusalV1::Closure(format_error(error));
        match CanonicalObjectKind::from_domain_prefix_for(bytes, RetentionReader::Dual)
            .map_err(closure)?
        {
            CanonicalObjectKind::RetentionAnchor => {
                let anchor = RetentionAnchorV2::parse(bytes).map_err(closure)?;
                self.reserve_references(1)?;
                self.references.push(anchor.historical_root());
            }
            CanonicalObjectKind::RetentionRadixNode => {
                let node = CanonicalDagNodeView::parse_in_domain(
                    bytes,
                    RETENTION_RADIX_DOMAIN,
                    "RSpace retention radix node",
                )
                .map_err(closure)?;
                let children = node.children().iter();
                self.reserve_references(children.len())?;
                self.references.extend(children.map(|child| child.target));
            }
            kind => {
                return Err(closure(CanonicalFormatError::UnexpectedObjectKind {
                    digest: reference.digest(),
                    expected: "retention anchor or retention radix node",
                    actual: kind.name(),
                }));
            }
        }
        Ok(())
    }

    fn push_semantic_references(
        &mut self,
        reference: ObjectRefV1,
        bytes: &[u8],
    ) -> Result<(), LedgerTrialRefusalV1<S::Error>> {
        let object = CanonicalObjectView::parse(bytes)
            .map_err(|error| LedgerTrialRefusalV1::Closure(format_error(error)))?;
        match object {
            CanonicalObjectView::SemanticManifest(manifest) => {
                let roots = manifest.roots().iter();
                self.reserve_references(roots.len())?;
                self.references
                    .extend(roots.into_iter().map(|(_, root)| root));
            }
            CanonicalObjectView::DagNode(node) => {
                let content = match node.endpoint() {
                    EndpointValue::Content(content) => Some(content),
                    _ => None,
                };
                let children = node.children().iter();
                self.reserve_references(usize::from(content.is_some()) + children.len())?;
                self.references.extend(content);
                self.references.extend(children.map(|child| child.target));
            }
            CanonicalObjectView::Blob(_) => {}
            CanonicalObjectView::AcceleratorManifest(_) |
            CanonicalObjectView::RetentionManifest(_) => {
                return Err(LedgerTrialRefusalV1::Closure(format_error(
                    CanonicalFormatError::UnexpectedObjectKind {
                        digest: reference.digest(),
                        expected: "semantic manifest, DAG node or blob",
                        actual: object.kind().name(),
                    },
                )));
            }
        }
        Ok(())
    }

    fn reserve_references(
        &mut self,
        additional: usize,
    ) -> Result<(), LedgerTrialRefusalV1<S::Error>> {
        self.references
            .try_reserve(additional)
            .map_err(|_| allocation_failure(self.references.len().saturating_add(additional)))
    }

    fn reserve(&mut self, charge: &TrialCharge) -> Result<(), LedgerTrialRefusalV1<S::Error>> {
        let limits = self.limits;
        let usage = self.usage;
        let peak = |used: u64, live: u128, axis: LedgerTrialAxisV1| {
            u64::try_from(u128::from(used).max(live))
                .ok()
                .filter(|raised| *raised <= limits.get(axis))
                .ok_or(LedgerTrialRefusalV1::Budget(axis))
        };
        let sum = |used: u64, amount: u64, axis: LedgerTrialAxisV1| match limits
            .get(axis)
            .checked_sub(amount)
        {
            Some(room) if used <= room => Ok(used + amount),
            _ => Err(LedgerTrialRefusalV1::Budget(axis)),
        };
        let peak_count = peak(usage.peak_count, charge.count, LedgerTrialAxisV1::Count)?;
        let peak_objects =
            peak(usage.peak_objects, charge.objects, LedgerTrialAxisV1::UnionObjects)?;
        let peak_bytes = peak(usage.peak_bytes, charge.bytes, LedgerTrialAxisV1::UnionBytes)?;
        let peak_scratch =
            peak(usage.peak_scratch, charge.live_scratch, LedgerTrialAxisV1::Scratch)?;
        let references = sum(usage.references, charge.references, LedgerTrialAxisV1::References)?;
        let expansions = sum(usage.expansions, charge.expansions, LedgerTrialAxisV1::Expansions)?;
        let quoted_bytes =
            sum(usage.quoted_bytes, charge.quoted_bytes, LedgerTrialAxisV1::QuotedBytes)?;
        let consumed_bytes = usage
            .consumed_bytes
            .checked_add(charge.consumed_bytes)
            .ok_or_else(|| {
                LedgerTrialRefusalV1::Closure(format_error(CanonicalFormatError::CounterOverflow(
                    "ledger trial consumed bytes",
                )))
            })?;
        self.usage = LedgerTrialUsageV1 {
            peak_count,
            peak_objects,
            peak_bytes,
            peak_scratch,
            references,
            expansions,
            quoted_bytes,
            consumed_bytes,
        };
        Ok(())
    }
}

fn allocation_failure<E>(requested: usize) -> LedgerTrialRefusalV1<E> {
    LedgerTrialRefusalV1::Closure(format_error(CanonicalFormatError::AllocationFailure {
        requested: u64::try_from(requested).unwrap_or(u64::MAX),
    }))
}
