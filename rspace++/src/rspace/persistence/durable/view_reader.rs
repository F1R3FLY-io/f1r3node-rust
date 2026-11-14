use super::{
    DurableObjectReadErrorV1, DurableViewReadHandleV1, continuations_for_channels_at,
    data_for_channel_at, joins_for_channel_at,
};
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::hot_store::DurableHotStoreBootstrapLimits;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::semantic::SemanticProfile;
use crate::rspace::persistence::{ClosureError, SemanticRoots};

const UNBOUNDED_DATUM_OCCURRENCES: u64 = u64::MAX;

pub(crate) struct DurableViewReaderV1<P: SemanticProfile> {
    handle: DurableViewReadHandleV1<P>,
    roots: SemanticRoots,
    limits: DurableHotStoreBootstrapLimits,
}

impl<P: SemanticProfile> DurableViewReaderV1<P> {
    pub(crate) fn new(handle: DurableViewReadHandleV1<P>, roots: SemanticRoots) -> Self {
        Self {
            handle,
            roots,
            limits: DurableHotStoreBootstrapLimits::default(),
        }
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "only unit tests set a durable view reader's limits; the durable \
                      state-authority backend reads with the default limits, which impose no \
                      ceiling"
        )
    )]
    pub(crate) fn with_limits(self, limits: DurableHotStoreBootstrapLimits) -> Self {
        Self { limits, ..self }
    }
}

impl<P: SemanticProfile> DurableViewReaderV1<P>
where P::Channel: Clone
{
    pub(crate) fn try_get_data_canonical(
        &self,
        key: &P::Channel,
    ) -> Result<Vec<Datum<P::Datum>>, ClosureError<DurableObjectReadErrorV1>> {
        data_for_channel_at(
            self.handle.core.verified_state(),
            &self.roots,
            key,
            self.limits.max_steps,
            self.limits.max_endpoints,
            UNBOUNDED_DATUM_OCCURRENCES,
        )
    }

    pub(crate) fn try_get_continuations_canonical(
        &self,
        key: &[P::Channel],
    ) -> Result<
        Vec<WaitingContinuation<P::Pattern, P::Continuation>>,
        ClosureError<DurableObjectReadErrorV1>,
    > {
        continuations_for_channels_at(
            self.handle.core.verified_state(),
            &self.roots,
            key,
            self.limits.max_steps,
            self.limits.max_endpoints,
            self.limits.max_payload_bytes,
        )
    }

    pub(crate) fn try_get_joins_canonical(
        &self,
        key: &P::Channel,
    ) -> Result<Vec<Vec<P::Channel>>, ClosureError<DurableObjectReadErrorV1>> {
        joins_for_channel_at(
            self.handle.core.verified_state(),
            &self.roots,
            key,
            self.limits.max_steps,
            self.limits.max_endpoints,
            self.limits.max_join_occurrences,
        )
    }
}

impl<P: SemanticProfile> HistoryReaderBase<P::Channel, P::Pattern, P::Datum, P::Continuation>
    for DurableViewReaderV1<P>
where P::Channel: Clone
{
    fn get_data_canonical(&self, key: &P::Channel) -> Vec<Datum<P::Datum>> {
        self.try_get_data_canonical(key)
            .expect("durable view reader: the authenticated data read of one channel failed")
    }

    fn get_continuations_canonical(
        &self,
        key: &[P::Channel],
    ) -> Vec<WaitingContinuation<P::Pattern, P::Continuation>> {
        self.try_get_continuations_canonical(key).expect(
            "durable view reader: the authenticated continuation read of one ordered channel \
             group failed",
        )
    }

    fn get_joins_canonical(&self, key: &P::Channel) -> Vec<Vec<P::Channel>> {
        self.try_get_joins_canonical(key)
            .expect("durable view reader: the authenticated join read of one channel failed")
    }
}
