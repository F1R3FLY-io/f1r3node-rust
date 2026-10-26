use crate::rspace::internal::{Datum, WaitingContinuation};

// See rspace/src/main/scala/coop/rchain/rspace/HotStoreAction.scala
// PartialEq and Eq are needed for hot_store_spec tests
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotStoreAction<C: Clone, P: Clone, A: Clone, K: Clone> {
    Insert(InsertAction<C, P, A, K>),
    Delete(DeleteAction<C>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InsertAction<C: Clone, P: Clone, A: Clone, K: Clone> {
    InsertData(InsertData<C, A>),
    InsertJoins(InsertJoins<C>),
    InsertContinuations(InsertContinuations<C, P, K>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertData<C: Clone, A: Clone> {
    pub channel: C,
    pub data: Vec<Datum<A>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertJoins<C: Clone> {
    pub channel: C,
    pub joins: Vec<Vec<C>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertContinuations<C: Clone, P: Clone, K: Clone> {
    pub channels: Vec<C>,
    pub continuations: Vec<WaitingContinuation<P, K>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteAction<C: Clone> {
    DeleteData(DeleteData<C>),
    DeleteJoins(DeleteJoins<C>),
    DeleteContinuations(DeleteContinuations<C>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteData<C: Clone> {
    pub channel: C,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteJoins<C: Clone> {
    pub channel: C,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteContinuations<C: Clone> {
    pub channels: Vec<C>,
}

/// D-C3 (D-S3, DR-97): one change of the native export, borrowed from the
/// native store or from an owned [`HotStoreAction`]. `W` is
/// `WaitingContinuation<P, K>` for an owned action and
/// `Arc<WaitingContinuation<P, K>>` for a store entry. Serde's `rc` feature
/// serializes an `Arc<T>` as its `T`, so both encode the same bytes with the
/// same writes.
#[derive(Debug)]
pub enum NativeExportAction<'a, C, A: Clone, W> {
    InsertData {
        channel: &'a C,
        data: &'a [Datum<A>],
    },
    InsertContinuations {
        channels: &'a [C],
        continuations: &'a [W],
    },
    InsertJoins {
        channel: &'a C,
        joins: &'a [Vec<C>],
    },
    DeleteData {
        channel: &'a C,
    },
    DeleteContinuations {
        channels: &'a [C],
    },
    DeleteJoins {
        channel: &'a C,
    },
}

impl<C, A: Clone, W> Clone for NativeExportAction<'_, C, A, W> {
    fn clone(&self) -> Self { *self }
}

impl<C, A: Clone, W> Copy for NativeExportAction<'_, C, A, W> {}

impl<C, A: Clone, W> NativeExportAction<'_, C, A, W> {
    /// Whether the change writes a leaf.
    pub fn is_insert(&self) -> bool {
        matches!(
            self,
            Self::InsertData { .. } | Self::InsertContinuations { .. } | Self::InsertJoins { .. }
        )
    }
}

impl<'a, C: Clone, P: Clone, A: Clone, K: Clone> From<&'a HotStoreAction<C, P, A, K>>
    for NativeExportAction<'a, C, A, WaitingContinuation<P, K>>
{
    fn from(action: &'a HotStoreAction<C, P, A, K>) -> Self {
        match action {
            HotStoreAction::Insert(InsertAction::InsertData(insert)) => Self::InsertData {
                channel: &insert.channel,
                data: &insert.data,
            },
            HotStoreAction::Insert(InsertAction::InsertContinuations(insert)) => {
                Self::InsertContinuations {
                    channels: &insert.channels,
                    continuations: &insert.continuations,
                }
            }
            HotStoreAction::Insert(InsertAction::InsertJoins(insert)) => Self::InsertJoins {
                channel: &insert.channel,
                joins: &insert.joins,
            },
            HotStoreAction::Delete(DeleteAction::DeleteData(delete)) => Self::DeleteData {
                channel: &delete.channel,
            },
            HotStoreAction::Delete(DeleteAction::DeleteContinuations(delete)) => {
                Self::DeleteContinuations {
                    channels: &delete.channels,
                }
            }
            HotStoreAction::Delete(DeleteAction::DeleteJoins(delete)) => Self::DeleteJoins {
                channel: &delete.channel,
            },
        }
    }
}

/// D-C3 (D-S3, DR-97): an action that the native checkpoint reads through a
/// view, so the owned and the borrowed exports share one preparation.
pub trait NativeExportView<C, A: Clone, W> {
    fn view(&self) -> NativeExportAction<'_, C, A, W>;
}

impl<C: Clone, P: Clone, A: Clone, K: Clone> NativeExportView<C, A, WaitingContinuation<P, K>>
    for HotStoreAction<C, P, A, K>
{
    fn view(&self) -> NativeExportAction<'_, C, A, WaitingContinuation<P, K>> {
        NativeExportAction::from(self)
    }
}

impl<C, A: Clone, W> NativeExportView<C, A, W> for NativeExportAction<'_, C, A, W> {
    fn view(&self) -> NativeExportAction<'_, C, A, W> { *self }
}
