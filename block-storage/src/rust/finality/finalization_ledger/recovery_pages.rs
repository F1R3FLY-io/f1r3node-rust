use std::iter::Peekable;
use std::marker::PhantomData;
use std::num::NonZeroUsize;

pub(super) trait ExclusiveGate {
    type Guard<'a>
    where Self: 'a;

    fn lock(&self) -> Self::Guard<'_>;
}

pub(super) trait ReceiptFactory {
    type Block;
    type Key;
    const KINDS: usize;

    fn effect(revision: u64, block: &Self::Block, kind: usize) -> Self::Key;
    fn complete(revision: u64) -> Self::Key;
}

pub(super) struct ReceiptKeys<I: Iterator, F: ReceiptFactory<Block = I::Item>> {
    revision: u64,
    blocks: I,
    current: Option<I::Item>,
    kind_index: usize,
    marker_pending: bool,
    factory: PhantomData<F>,
}

impl<I: Iterator, F: ReceiptFactory<Block = I::Item>> ReceiptKeys<I, F> {
    pub(super) fn new(revision: u64, blocks: I) -> Self {
        assert!(F::KINDS > 0);
        Self {
            revision,
            blocks,
            current: None,
            kind_index: 0,
            marker_pending: true,
            factory: PhantomData,
        }
    }
}

impl<I: Iterator, F: ReceiptFactory<Block = I::Item>> Iterator for ReceiptKeys<I, F> {
    type Item = F::Key;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_none() {
            self.current = self.blocks.next();
            self.kind_index = 0;
        }
        if let Some(block) = &self.current {
            let key = F::effect(self.revision, block, self.kind_index);
            self.kind_index += 1;
            if self.kind_index == F::KINDS {
                self.current = None;
            }
            Some(key)
        } else if std::mem::take(&mut self.marker_pending) {
            Some(F::complete(self.revision))
        } else {
            None
        }
    }
}

pub(super) trait RecoveryStore {
    type Error: Clone;
    type Gate: ExclusiveGate;
    type Keys: Iterator;

    fn gate(&self) -> &Self::Gate;
    fn head_revision(&self) -> Result<Option<u64>, Self::Error>;
    fn projection_cursor(&self) -> Result<u64, Self::Error>;
    fn effects_cursor(&self) -> Result<u64, Self::Error>;
    fn compaction_cursor(&self) -> Result<u64, Self::Error>;
    fn round_complete(&self, revision: u64) -> Result<bool, Self::Error>;
    fn write_effects_cursor(&self, revision: u64) -> Result<(), Self::Error>;
    fn receipt_keys(&self, revision: u64) -> Result<Self::Keys, Self::Error>;
    fn delete_receipts(&self, keys: Vec<<Self::Keys as Iterator>::Item>)
        -> Result<(), Self::Error>;
    fn write_compaction_cursor(&self, revision: u64) -> Result<(), Self::Error>;
    fn serialization(message: String) -> Self::Error;
    fn invalid(message: String) -> Self::Error;
    fn projection_pending(revision: u64, projected_revision: u64) -> Self::Error;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Window {
    pub cursor: u64,
    pub target: u64,
}

pub(super) fn validate_cursor_bounds<S: RecoveryStore>(
    store: &S,
    head: u64,
) -> Result<(), S::Error> {
    let projection = store.projection_cursor()?;
    let effects = store.effects_cursor()?;
    let compaction = store.compaction_cursor()?;
    if projection > head || effects > projection || compaction > effects {
        return Err(S::serialization(format!(
            "finalization cursor bounds are invalid: projection={projection}, effects={effects}, compaction={compaction}, head={head}"
        )));
    }
    Ok(())
}

pub(super) fn select_effects<S: RecoveryStore>(store: &S) -> Result<Window, S::Error> {
    let _guard = store.gate().lock();
    let head = store.head_revision()?.ok_or_else(|| {
        S::invalid("finalization ledger has no initialized genesis head".to_string())
    })?;
    validate_cursor_bounds(store, head)?;
    Ok(Window {
        cursor: store.effects_cursor()?,
        target: store.projection_cursor()?,
    })
}

pub(super) fn require_projection_locked<S: RecoveryStore>(
    store: &S,
    revision: u64,
) -> Result<(), S::Error> {
    let head = store.head_revision()?.ok_or_else(|| {
        S::invalid("finalization ledger has no initialized genesis head".to_string())
    })?;
    validate_cursor_bounds(store, head)?;
    if revision == 0 || revision > head {
        return Err(S::invalid(format!(
            "finalization effect revision {revision} is outside the committed rounds"
        )));
    }
    let projection = store.projection_cursor()?;
    if revision > projection {
        return Err(S::projection_pending(revision, projection));
    }
    Ok(())
}

pub(super) fn require_projection<S: RecoveryStore>(
    store: &S,
    revision: u64,
) -> Result<(), S::Error> {
    let _guard = store.gate().lock();
    require_projection_locked(store, revision)
}

pub(super) fn advancement_snapshot_locked<S: RecoveryStore>(store: &S) -> Result<Window, S::Error> {
    let head = store.head_revision()?.ok_or_else(|| {
        S::serialization("finalization ledger has no initialized head".to_string())
    })?;
    let projection = store.projection_cursor()?;
    let effects = store.effects_cursor()?;
    let compaction = store.compaction_cursor()?;
    if projection > head || effects > projection || compaction > effects {
        return Err(S::serialization(
            "finalization cursor bounds are invalid during effects advancement".to_string(),
        ));
    }
    Ok(Window {
        cursor: effects,
        target: projection,
    })
}

pub(super) fn begin_advancement<S: RecoveryStore>(store: &S) -> Result<Window, S::Error> {
    let _guard = store.gate().lock();
    advancement_snapshot_locked(store)
}

pub(super) struct Advancement<'a, E> {
    pub target: u64,
    pub cursor: &'a mut u64,
    pub finished: &'a mut bool,
    pub failure: &'a mut Option<E>,
}

pub(super) fn advance_page<S: RecoveryStore>(
    store: &S,
    progress: Advancement<'_, S::Error>,
    budget: NonZeroUsize,
) -> Result<bool, S::Error> {
    if let Some(error) = &progress.failure {
        return Err(error.clone());
    }
    if *progress.finished {
        return Ok(true);
    }
    let result = (|| {
        let _guard = store.gate().lock();
        let current = advancement_snapshot_locked(store)?;
        if current.cursor < *progress.cursor || current.target < progress.target {
            return Err(S::serialization(
                "finalization effects or projection cursor regressed during advancement"
                    .to_string(),
            ));
        }
        let mut cursor = current.cursor;
        let mut finished = cursor >= progress.target;
        for _ in 0..budget.get() {
            if cursor >= progress.target {
                break;
            }
            let next = cursor.checked_add(1).ok_or_else(|| {
                S::serialization("finalization effects cursor exhausted".to_string())
            })?;
            if !store.round_complete(next)? {
                finished = true;
                break;
            }
            cursor = next;
        }
        if cursor != current.cursor {
            store.write_effects_cursor(cursor)?;
        }
        *progress.cursor = cursor;
        Ok(finished || cursor >= progress.target)
    })();
    match result {
        Ok(finished) => {
            *progress.finished = finished;
            Ok(finished)
        }
        Err(error) => {
            *progress.failure = Some(error.clone());
            Err(error)
        }
    }
}

pub(super) fn begin_compaction<S: RecoveryStore>(store: &S) -> Result<Window, S::Error> {
    let _guard = store.gate().lock();
    let compacted = store.compaction_cursor()?;
    let completed = store.effects_cursor()?;
    if compacted > completed {
        return Err(S::serialization(format!(
            "finalization effects compaction cursor {compacted} exceeds completed cursor {completed}"
        )));
    }
    Ok(Window {
        cursor: compacted,
        target: completed,
    })
}

pub(super) struct Compaction<'a, I: Iterator, E> {
    pub target: u64,
    pub cursor: &'a mut u64,
    pub pending: &'a mut Option<Peekable<I>>,
    pub failure: &'a mut Option<E>,
}

pub(super) fn compact_page<S: RecoveryStore>(
    store: &S,
    progress: Compaction<'_, S::Keys, S::Error>,
    budget: NonZeroUsize,
) -> Result<bool, S::Error> {
    if let Some(error) = &progress.failure {
        return Err(error.clone());
    }
    let result = (|| {
        let _guard = store.gate().lock();
        let completed = store.effects_cursor()?;
        let compacted = store.compaction_cursor()?;
        if completed < progress.target || compacted > completed || compacted < *progress.cursor {
            return Err(S::serialization(
                "finalization compaction cursors regressed or exceeded completed effects"
                    .to_string(),
            ));
        }
        if compacted >= progress.target {
            *progress.pending = None;
            *progress.cursor = compacted;
            return Ok(true);
        }
        if compacted != *progress.cursor {
            *progress.pending = None;
            *progress.cursor = compacted;
        }
        let revision = compacted.checked_add(1).ok_or_else(|| {
            S::serialization("finalization compaction cursor exhausted".to_string())
        })?;
        if progress.pending.is_none() {
            *progress.pending = Some(store.receipt_keys(revision)?.peekable());
        }
        let pending = progress.pending.as_mut().ok_or_else(|| {
            S::serialization("finalization compaction has no receipt iterator".to_string())
        })?;
        let keys = pending.by_ref().take(budget.get()).collect();
        let round_finished = pending.peek().is_none();
        store.delete_receipts(keys)?;
        if round_finished {
            store.write_compaction_cursor(revision)?;
            *progress.cursor = revision;
            *progress.pending = None;
        }
        Ok(*progress.cursor == progress.target)
    })();
    if let Err(error) = &result {
        *progress.failure = Some(error.clone());
    }
    result
}
