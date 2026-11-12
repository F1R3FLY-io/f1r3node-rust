use super::{
    DurableViewReadHandleV1, continuations_for_channels_at, data_for_channel_at,
    joins_for_channel_at,
};
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::hot_store::DurableHotStoreBootstrapLimits;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::SemanticRoots;
use crate::rspace::persistence::semantic::SemanticProfile;

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

impl<P: SemanticProfile> HistoryReaderBase<P::Channel, P::Pattern, P::Datum, P::Continuation>
    for DurableViewReaderV1<P>
where P::Channel: Clone
{
    fn get_data_canonical(&self, key: &P::Channel) -> Vec<Datum<P::Datum>> {
        data_for_channel_at(
            self.handle.core.verified_state(),
            &self.roots,
            key,
            self.limits.max_steps,
            self.limits.max_endpoints,
            UNBOUNDED_DATUM_OCCURRENCES,
        )
        .expect("durable view reader: the authenticated data read of one channel failed")
    }

    fn get_continuations_canonical(
        &self,
        key: &[P::Channel],
    ) -> Vec<WaitingContinuation<P::Pattern, P::Continuation>> {
        continuations_for_channels_at(
            self.handle.core.verified_state(),
            &self.roots,
            key,
            self.limits.max_steps,
            self.limits.max_endpoints,
            self.limits.max_payload_bytes,
        )
        .expect(
            "durable view reader: the authenticated continuation read of one ordered channel \
             group failed",
        )
    }

    fn get_joins_canonical(&self, key: &P::Channel) -> Vec<Vec<P::Channel>> {
        joins_for_channel_at(
            self.handle.core.verified_state(),
            &self.roots,
            key,
            self.limits.max_steps,
            self.limits.max_endpoints,
            self.limits.max_join_occurrences,
        )
        .expect("durable view reader: the authenticated join read of one channel failed")
    }
}
