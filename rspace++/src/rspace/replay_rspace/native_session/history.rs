use serde::de::DeserializeOwned;
use shared::rust::clone_backing::CloneBacking;

use super::*;
use crate::rspace::hashing::native_source::{SourceMeter, channels_hash, hash};
use crate::rspace::history::native_reader::{
    NativeLeafKind, NativeReadCharge, NativeReadError, NativeReadFault, NativeReadMeter,
    decode_record,
};

struct Meter<F>(F);

impl<F: Fn(usize, usize, usize) -> Result<(), RSpaceError>> NativeReadMeter for Meter<F> {
    type Error = RSpaceError;

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error> {
        (self.0)(charge.operations, charge.scanned_bytes, charge.backing_bytes)
    }
}

fn read_error(error: NativeReadError<RSpaceError>) -> RSpaceError {
    match error {
        NativeReadError::Host(error) | NativeReadError::Consumer(error) => error,
        NativeReadError::Store(error) => error.into(),
        NativeReadError::Invalid(
            NativeReadFault::Depth | NativeReadFault::Allocation | NativeReadFault::Overflow,
        ) => RSpaceError::HostWorkRejected,
        NativeReadError::Invalid(error) => {
            RSpaceError::InterpreterError(format!("native history: {error:?}"))
        }
    }
}

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    E: NativeReplayEpoch,
{
    pub(super) fn read_records<T: DeserializeOwned>(
        &self,
        kind: NativeLeafKind,
        projection: Blake2b256Hash,
        reserve: &impl SourceMeter,
    ) -> Result<Vec<T>, RSpaceError> {
        let history = self.space.get_history_repository();
        let projection = projection.0.as_slice().try_into().map_err(|_| {
            RSpaceError::InterpreterError("native history projection length".to_owned())
        })?;
        let meter =
            Meter(|operations, scanned, backing| reserve.reserve(operations, scanned, backing));
        let decode_meter = Meter(|operations: usize, scanned: usize, backing| {
            reserve.reserve(
                operations
                    .checked_mul(2)
                    .ok_or(RSpaceError::HostWorkRejected)?,
                scanned
                    .checked_mul(2)
                    .ok_or(RSpaceError::HostWorkRejected)?,
                backing,
            )
        });
        history
            .native_history_reader(self.root)
            .with_records(kind, &projection, &meter, |rows| {
                reserve.reserve(
                    rows.len()
                        .checked_mul(2)
                        .and_then(|count| count.checked_add(1))
                        .ok_or(RSpaceError::HostWorkRejected)?,
                    0,
                    backing::<T>(rows.len())?,
                )?;
                let mut values = Vec::new();
                values
                    .try_reserve_exact(rows.len())
                    .map_err(|_| RSpaceError::HostWorkRejected)?;
                for row in rows.iter() {
                    values.push(decode_record(row, &decode_meter).map_err(read_error)?);
                }
                Ok(values)
            })
            .map_err(read_error)
            .map(Option::unwrap_or_default)
    }

    pub(super) fn history_reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), RSpaceError> {
        self.epoch.reserve_comparison(operations, scanned)?;
        self.epoch.reserve_work(0, backing)
    }

    pub(super) fn read_data_with(
        &self,
        channel: &C,
        reserve: &impl SourceMeter,
    ) -> Result<Vec<Datum<A>>, RSpaceError> {
        self.space.get_store().get_data_with_reader(
            channel,
            &|| self.read_records(NativeLeafKind::Data, hash(channel, reserve)?, reserve),
            reserve,
        )
    }

    pub(super) fn read_joins(&self, channel: &C) -> Result<Vec<Vec<C>>, RSpaceError> {
        let reserve =
            |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        self.space.get_store().get_joins_with_reader(
            channel,
            &|| self.read_records(NativeLeafKind::Joins, hash(channel, &reserve)?, &reserve),
            &reserve,
        )
    }

    pub(super) fn read_continuations(
        &self,
        channels: &[C],
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        let reserve =
            |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        self.space.get_store().get_continuations_with_reader(
            channels,
            &|| {
                self.read_records(
                    NativeLeafKind::Continuations,
                    channels_hash(channels, &reserve)?,
                    &reserve,
                )
            },
            &reserve,
        )
    }

    /// The continuations of `channels` as shared views (C1, DR-81).
    pub(super) fn read_continuation_views(
        &self,
        channels: &[C],
    ) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, RSpaceError> {
        let reserve =
            |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        self.space.get_store().get_continuation_views_with_reader(
            channels,
            &|| {
                self.read_records(
                    NativeLeafKind::Continuations,
                    channels_hash(channels, &reserve)?,
                    &reserve,
                )
            },
            &reserve,
        )
    }

    /// Fills the continuation cache of `channels` without copying a
    /// continuation (C1, DR-81).
    pub(super) fn prefetch_continuations(&self, channels: &[C]) -> Result<(), RSpaceError> {
        self.read_continuation_views(channels).map(drop)
    }

    pub(super) fn prepare_data(&self, channel: &C) -> Result<(), RSpaceError> {
        self.read_data_with(channel, &|operations, scanned, backing| {
            self.history_reserve(operations, scanned, backing)
        })
        .map(|_| ())
    }
}
