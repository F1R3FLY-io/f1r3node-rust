use std::fmt;

use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::internal::WaitingContinuation;
use crate::rspace::rho_path::{
    ReceiveChannelGroup, ReceiveChannelProfile, ReceiveChannelQuery, RhoPathChannelPositions,
    RhoPathError, RhoPathReceivePosition, RhoPathV1, RhoPathV2, frame_equals,
};
use crate::rspace::serializers::serializers::{
    encode_continuation_identity_observed_into, encode_continuation_source_into,
};

const HEADER_WIDTH: usize = 8;
const PEEK_WIDTH: usize = 4;
const PEEK_TAIL_WIDTH: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderedReceiveError {
    Path(RhoPathError),
    TruncatedRecord,
    MalformedFieldBounds,
    InvalidPersist(u8),
    PeekCountOverflow,
    PeekTailMismatch,
    NonCanonicalPeeks,
    ArityMismatch { channels: usize, patterns: usize },
    IdentityMismatch,
    ProfileMismatch,
}

impl fmt::Display for OrderedReceiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(error) => write!(f, "invalid receive channel group: {error}"),
            Self::TruncatedRecord => f.write_str("truncated continuation record"),
            Self::MalformedFieldBounds => {
                f.write_str("continuation field bounds do not match the record")
            }
            Self::InvalidPersist(byte) => {
                write!(f, "invalid continuation persist byte {byte:#04x}")
            }
            Self::PeekCountOverflow => {
                f.write_str("continuation peek tail exceeds the address space")
            }
            Self::PeekTailMismatch => {
                f.write_str("continuation peek tail does not end at the identity bound")
            }
            Self::NonCanonicalPeeks => f.write_str("continuation peeks are not strictly ascending"),
            Self::ArityMismatch { channels, patterns } => write!(
                f,
                "channel group has {channels} channels but the record has {patterns} patterns"
            ),
            Self::IdentityMismatch => {
                f.write_str("continuation key identity differs from the record identity")
            }
            Self::ProfileMismatch => {
                f.write_str("channel query profile differs from the view profile")
            }
        }
    }
}

impl std::error::Error for OrderedReceiveError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContinuationFieldBounds(Box<[usize]>);

impl ContinuationFieldBounds {
    pub fn from_ends(ends: Box<[usize]>) -> Self { Self(ends) }

    pub fn ends(&self) -> &[usize] { &self.0 }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedContinuationRecord {
    canonical: Vec<u8>,
    field_bounds: ContinuationFieldBounds,
}

impl PreparedContinuationRecord {
    pub fn encode_waiting<P, K>(continuation: &WaitingContinuation<P, K>) -> Self
    where
        P: Clone + StableHashSerialize,
        K: Clone + StableHashSerialize,
    {
        let field_count = continuation
            .patterns
            .len()
            .checked_add(2)
            .expect("a continuation field count fits usize");
        let mut ends = Vec::with_capacity(field_count);
        let mut canonical = Vec::new();
        encode_continuation_identity_observed_into(continuation, &mut canonical, &mut |end| {
            ends.push(end)
        });
        encode_continuation_source_into(continuation, &mut canonical);
        Self {
            canonical,
            field_bounds: ContinuationFieldBounds(ends.into_boxed_slice()),
        }
    }

    pub fn canonical(&self) -> &[u8] { &self.canonical }

    pub fn field_bounds(&self) -> &ContinuationFieldBounds { &self.field_bounds }

    pub fn record_view(&self) -> ContinuationRecordView<'_> {
        ContinuationRecordView::admit_record(&self.canonical, &self.field_bounds)
            .expect("production encoder bounds admit their own record")
    }

    pub(crate) fn into_parts(self) -> (Vec<u8>, ContinuationFieldBounds) {
        (self.canonical, self.field_bounds)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContinuationRecordView<'a> {
    record: &'a [u8],
    pattern_ends: &'a [usize],
    body_end: usize,
    identity_end: usize,
    persist: bool,
    peeks: &'a [[u8; PEEK_WIDTH]],
}

impl<'a> ContinuationRecordView<'a> {
    pub fn admit_record(
        record: &'a [u8],
        bounds: &'a ContinuationFieldBounds,
    ) -> Result<Self, OrderedReceiveError> {
        let header = record
            .first_chunk::<HEADER_WIDTH>()
            .ok_or(OrderedReceiveError::TruncatedRecord)?;
        let Some((pattern_ends, &[body_end, identity_end])) = bounds.ends().split_last_chunk::<2>()
        else {
            return Err(OrderedReceiveError::MalformedFieldBounds);
        };
        if u64::try_from(pattern_ends.len()).ok() != Some(u64::from_le_bytes(*header)) {
            return Err(OrderedReceiveError::MalformedFieldBounds);
        }
        bounds
            .ends()
            .iter()
            .try_fold(HEADER_WIDTH, |previous, &finish| (previous <= finish).then_some(finish))
            .ok_or(OrderedReceiveError::MalformedFieldBounds)?;
        let (persist, after_persist) = match record.get(body_end..) {
            Some([0, after_persist @ ..]) => (false, after_persist),
            Some([1, after_persist @ ..]) => (true, after_persist),
            Some([byte, ..]) => return Err(OrderedReceiveError::InvalidPersist(*byte)),
            Some([]) | None => return Err(OrderedReceiveError::TruncatedRecord),
        };
        let peek_count = after_persist
            .first_chunk::<HEADER_WIDTH>()
            .ok_or(OrderedReceiveError::TruncatedRecord)?;
        let peeks_start = body_end
            .checked_add(PEEK_TAIL_WIDTH)
            .ok_or(OrderedReceiveError::PeekCountOverflow)?;
        let expected_end = usize::try_from(u64::from_le_bytes(*peek_count))
            .ok()
            .and_then(|count| count.checked_mul(PEEK_WIDTH))
            .and_then(|width| width.checked_add(peeks_start))
            .ok_or(OrderedReceiveError::PeekCountOverflow)?;
        if expected_end != identity_end {
            return Err(OrderedReceiveError::PeekTailMismatch);
        }
        let (peeks, []) = record
            .get(peeks_start..identity_end)
            .ok_or(OrderedReceiveError::TruncatedRecord)?
            .as_chunks::<PEEK_WIDTH>()
        else {
            return Err(OrderedReceiveError::PeekTailMismatch);
        };
        if !peeks
            .windows(2)
            .all(|pair| matches!(pair, [left, right] if peek_value(left) < peek_value(right)))
        {
            return Err(OrderedReceiveError::NonCanonicalPeeks);
        }
        Ok(Self {
            record,
            pattern_ends,
            body_end,
            identity_end,
            persist,
            peeks,
        })
    }

    pub const fn arity(&self) -> usize { self.pattern_ends.len() }

    pub fn identity(&self) -> &'a [u8] { &self.record[..self.identity_end] }

    pub fn source_suffix(&self) -> &'a [u8] { &self.record[self.identity_end..] }

    pub fn pattern_at(&self, index: usize) -> Option<&'a [u8]> {
        let finish = *self.pattern_ends.get(index)?;
        Some(&self.record[self.field_start(index)..finish])
    }

    pub fn body(&self) -> &'a [u8] {
        &self.record[self.field_start(self.pattern_ends.len())..self.body_end]
    }

    pub const fn persist(&self) -> bool { self.persist }

    pub fn raw_peeks(&self) -> impl ExactSizeIterator<Item = i32> + 'a {
        self.peeks.iter().map(peek_value)
    }

    pub const fn coarse_peek(&self) -> bool { !self.peeks.is_empty() }

    pub fn effective_peek(&self, position: RhoPathReceivePosition) -> bool {
        match position.signed_peek_key() {
            Some(key) => self
                .peeks
                .binary_search_by(|word| peek_value(word).cmp(&key))
                .is_ok(),
            None => false,
        }
    }

    fn field_start(&self, index: usize) -> usize {
        match index.checked_sub(1) {
            Some(previous) => self.pattern_ends[previous],
            None => HEADER_WIDTH,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderedReceiveView<'a> {
    profile: ReceiveChannelProfile,
    group: ReceiveChannelGroup<'a>,
    record: ContinuationRecordView<'a>,
}

impl<'a> OrderedReceiveView<'a> {
    pub fn admit_legacy_v1(
        record_path: &'a [u8],
        prefix_len: usize,
        bounds: &'a ContinuationFieldBounds,
    ) -> Result<Self, OrderedReceiveError> {
        let (prefix, record) = record_path
            .split_at_checked(prefix_len)
            .ok_or(OrderedReceiveError::TruncatedRecord)?;
        let group =
            RhoPathV1::parse_continuation_group(prefix).map_err(OrderedReceiveError::Path)?;
        let record = ContinuationRecordView::admit_record(record, bounds)?;
        Self::aligned(group, record)
    }

    pub fn admit_rci2_v2(
        key: &'a [u8],
        record: &'a [u8],
        bounds: &'a ContinuationFieldBounds,
    ) -> Result<Self, OrderedReceiveError> {
        let parsed = RhoPathV2::parse_continuation(key).map_err(OrderedReceiveError::Path)?;
        let record = ContinuationRecordView::admit_record(record, bounds)?;
        if !parsed.identity_matches(record.identity()) {
            return Err(OrderedReceiveError::IdentityMismatch);
        }
        Self::aligned(parsed.channel_group(), record)
    }

    fn aligned(
        group: ReceiveChannelGroup<'a>,
        record: ContinuationRecordView<'a>,
    ) -> Result<Self, OrderedReceiveError> {
        if group.channel_count() != record.arity() {
            return Err(OrderedReceiveError::ArityMismatch {
                channels: group.channel_count(),
                patterns: record.arity(),
            });
        }
        Ok(Self {
            profile: group.profile(),
            group,
            record,
        })
    }

    pub const fn profile(&self) -> ReceiveChannelProfile { self.profile }

    pub const fn record(&self) -> ContinuationRecordView<'a> { self.record }

    pub fn positions<'q>(
        &self,
        query: ReceiveChannelQuery<'q>,
    ) -> Result<RhoPathChannelPositions<'a, 'q>, OrderedReceiveError> {
        self.group
            .channel_positions_for(query)
            .ok_or(OrderedReceiveError::ProfileMismatch)
    }

    pub fn bind_at(&self, position: RhoPathReceivePosition) -> Option<ReceiveBind<'a>> {
        let channel_frame = self.group.frame_at(position)?;
        let pattern = self.record.pattern_at(position.index())?;
        Some(ReceiveBind {
            position,
            channel_frame,
            pattern,
            effective_peek: self.record.effective_peek(position),
        })
    }

    pub fn binds(&self) -> impl ExactSizeIterator<Item = ReceiveBind<'a>> + 'a {
        let record = self.record;
        self.group
            .frames()
            .enumerate()
            .map(move |(index, channel_frame)| {
                let position = RhoPathReceivePosition::at_index(index);
                ReceiveBind {
                    position,
                    channel_frame,
                    pattern: record
                        .pattern_at(index)
                        .expect("an aligned view has one pattern per channel frame"),
                    effective_peek: record.effective_peek(position),
                }
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiveBind<'a> {
    position: RhoPathReceivePosition,
    channel_frame: &'a [u8],
    pattern: &'a [u8],
    effective_peek: bool,
}

impl<'a> ReceiveBind<'a> {
    pub const fn position(&self) -> RhoPathReceivePosition { self.position }

    pub const fn channel_frame(&self) -> &'a [u8] { self.channel_frame }

    pub fn channel_matches(&self, raw: &[u8]) -> bool { frame_equals(self.channel_frame, raw) }

    pub const fn pattern(&self) -> &'a [u8] { self.pattern }

    pub const fn effective_peek(&self) -> bool { self.effective_peek }

    pub fn free_count<L: ReceivePatternLayout>(&self) -> Option<i32> {
        L::trailing_free_count(self.pattern)
    }
}

pub trait ReceivePatternLayout {
    fn trailing_free_count(pattern: &[u8]) -> Option<i32>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestingReceiveVisit {
    Unloaded,
    Visited(usize),
}

fn peek_value(word: &[u8; PEEK_WIDTH]) -> i32 { i32::from_le_bytes(*word) }

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;

    fn continuation_with_peeks(peeks: BTreeSet<i32>) -> WaitingContinuation<String, String> {
        let channels = vec![b"left".to_vec(), b"right".to_vec(), b"left".to_vec()];
        let patterns = vec!["first".to_string(), "second".to_string(), "third".to_string()];
        WaitingContinuation::create(&channels, &patterns, &"body".to_string(), false, peeks)
    }

    fn signed_boundary_indexes() -> impl Strategy<Value = usize> {
        let pivot = usize::try_from(i32::MAX).expect("i32::MAX fits usize");
        (pivot - 2)..(pivot + 3)
    }

    #[test]
    fn wide_positions_convert_without_wrap() {
        let peeks = BTreeSet::from([2, i32::MAX]);
        let record = PreparedContinuationRecord::encode_waiting(&continuation_with_peeks(peeks));
        let view = record.record_view();
        let narrow = RhoPathReceivePosition::at_index(2);
        assert_eq!(narrow.signed_peek_key(), Some(2));
        assert!(view.effective_peek(narrow));
        let signed_max = RhoPathReceivePosition::at_index(
            usize::try_from(i32::MAX).expect("i32::MAX fits usize"),
        );
        assert_eq!(signed_max.signed_peek_key(), Some(i32::MAX));
        assert!(view.effective_peek(signed_max));
        let wide_indexes = [
            usize::try_from(1u64 << 31).expect("the target addresses 2^31"),
            usize::try_from((1u64 << 32) + 2).expect("the target addresses 2^32 + 2"),
            usize::MAX,
        ];
        for index in wide_indexes {
            let position = RhoPathReceivePosition::at_index(index);
            assert_eq!(position.index(), index);
            assert_eq!(position.get(), u64::try_from(index).expect("usize fits u64"));
            assert_eq!(position.signed_peek_key(), None);
            assert!(!view.effective_peek(position));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn effective_peek_search_matches_linear_membership(
            peeks in proptest::collection::btree_set(
                prop_oneof![-8i32..72, any::<i32>(), Just(i32::MAX), Just(i32::MIN)],
                0..48,
            ),
            indexes in proptest::collection::vec(
                prop_oneof![0usize..80, any::<usize>(), signed_boundary_indexes()],
                1..32,
            ),
        ) {
            let record =
                PreparedContinuationRecord::encode_waiting(&continuation_with_peeks(peeks.clone()));
            let view = record.record_view();
            prop_assert_eq!(
                view.raw_peeks().collect::<Vec<_>>(),
                peeks.iter().copied().collect::<Vec<_>>()
            );
            prop_assert_eq!(view.coarse_peek(), !peeks.is_empty());
            for index in indexes {
                let position = RhoPathReceivePosition::at_index(index);
                let linear = peeks
                    .iter()
                    .any(|&peek| u64::try_from(peek).ok() == Some(position.get()));
                prop_assert_eq!(view.effective_peek(position), linear);
            }
        }
    }
}
