use bincode::Options;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::KvStoreError;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingRequestPolicy {
    pub revision: u64,
    pub initial_timestamp: u64,
    pub last_request_timestamp: u64,
    pub requested_as_dependency: bool,
    pub retry_attempts: u32,
    pub peer_requery_attempts: u32,
    pub peer_requery_cursor: u32,
    pub retry_budget_quarantine_until: Option<u64>,
    pub dependency_recovery_last_request: Option<u64>,
    pub broadcast_retry_last_request: Option<u64>,
    pub peer_requery_last_request: Option<u64>,
}

impl PendingRequestPolicy {
    pub const SCHEMA_VERSION: u16 = 1;
    pub const MAX_ENCODED_BYTES: usize = 75;

    fn options() -> impl Options {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .reject_trailing_bytes()
            .with_limit(Self::MAX_ENCODED_BYTES as u64)
    }

    fn validate(&self) -> Result<(), KvStoreError> {
        if self.revision == 0 {
            return Err(KvStoreError::InvalidArgument(
                "pending request policy revision must be nonzero".into(),
            ));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, KvStoreError> {
        self.validate()?;
        Ok(Self::options().serialize(&(Self::SCHEMA_VERSION, self))?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, KvStoreError> {
        if bytes.len() > Self::MAX_ENCODED_BYTES {
            return Err(KvStoreError::InvalidArgument(
                "pending request policy exceeds its encoded size bound".into(),
            ));
        }
        let (version, policy): (u16, Self) = Self::options().deserialize(bytes)?;
        if version != Self::SCHEMA_VERSION {
            return Err(KvStoreError::InvalidArgument(format!(
                "unsupported pending request policy version {version}"
            )));
        }
        policy.validate()?;
        Ok(policy)
    }

    pub fn advance_revision(&mut self) -> Result<(), KvStoreError> {
        self.validate()?;
        self.revision = self.revision.checked_add(1).ok_or_else(|| {
            KvStoreError::InvalidArgument("pending request policy revision exhausted".into())
        })?;
        Ok(())
    }

    pub fn renewed_retry_budget(&self, sweep_time: u64) -> Result<Option<Self>, KvStoreError> {
        self.validate()?;
        if self
            .retry_budget_quarantine_until
            .is_none_or(|deadline| deadline > sweep_time)
        {
            return Ok(None);
        }
        let mut renewed = self.clone();
        renewed.retry_attempts = 0;
        renewed.retry_budget_quarantine_until = None;
        Ok(Some(renewed))
    }

    pub fn quarantined_retry_budget(
        &self,
        budget: u32,
        deadline: u64,
    ) -> Result<Self, KvStoreError> {
        self.validate()?;
        if budget == 0 || self.retry_attempts < budget {
            return Err(KvStoreError::InvalidArgument(
                "retry quarantine requires an exhausted positive budget".into(),
            ));
        }
        let mut quarantined = self.clone();
        quarantined.retry_budget_quarantine_until = Some(deadline);
        quarantined.peer_requery_attempts = 0;
        quarantined.peer_requery_cursor = 0;
        quarantined.dependency_recovery_last_request = None;
        quarantined.broadcast_retry_last_request = None;
        quarantined.peer_requery_last_request = None;
        Ok(quarantined)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn policy() -> PendingRequestPolicy {
        PendingRequestPolicy {
            revision: 1,
            initial_timestamp: 1,
            last_request_timestamp: 2,
            requested_as_dependency: true,
            retry_attempts: 7,
            peer_requery_attempts: 3,
            peer_requery_cursor: 2,
            retry_budget_quarantine_until: Some(10),
            dependency_recovery_last_request: Some(4),
            broadcast_retry_last_request: Some(5),
            peer_requery_last_request: Some(6),
        }
    }

    proptest! {
        #[test]
        fn quarantine_entry_resets_only_the_upstream_peer_schedule(
            revision in 1u64..=u64::MAX,
            clocks in any::<[u64; 7]>(),
            counters in any::<[u32; 3]>(),
            budget in any::<u32>(),
            dependency in any::<bool>(),
        ) {
            let original = PendingRequestPolicy {
                revision,
                initial_timestamp: clocks[0],
                last_request_timestamp: clocks[1],
                requested_as_dependency: dependency,
                retry_attempts: counters[0],
                peer_requery_attempts: counters[1],
                peer_requery_cursor: counters[2],
                retry_budget_quarantine_until: Some(clocks[2]),
                dependency_recovery_last_request: Some(clocks[3]),
                broadcast_retry_last_request: Some(clocks[4]),
                peer_requery_last_request: Some(clocks[5]),
            };
            let result = original.quarantined_retry_budget(budget, clocks[6]);
            prop_assert_eq!(result.is_ok(), budget != 0 && original.retry_attempts >= budget);
            if let Ok(mut quarantined) = result {
                prop_assert_eq!(quarantined.retry_budget_quarantine_until, Some(clocks[6]));
                prop_assert_eq!(quarantined.peer_requery_attempts, 0);
                prop_assert_eq!(quarantined.peer_requery_cursor, 0);
                prop_assert_eq!(quarantined.dependency_recovery_last_request, None);
                prop_assert_eq!(quarantined.broadcast_retry_last_request, None);
                prop_assert_eq!(quarantined.peer_requery_last_request, None);
                quarantined.retry_budget_quarantine_until = original.retry_budget_quarantine_until;
                quarantined.peer_requery_attempts = original.peer_requery_attempts;
                quarantined.peer_requery_cursor = original.peer_requery_cursor;
                quarantined.dependency_recovery_last_request = original.dependency_recovery_last_request;
                quarantined.broadcast_retry_last_request = original.broadcast_retry_last_request;
                quarantined.peer_requery_last_request = original.peer_requery_last_request;
                prop_assert_eq!(quarantined, original);
            }
        }

        #[test]
        fn renewal_preserves_every_field_except_spent_total_and_deadline(
            revision in 1u64..=u64::MAX,
            clocks in any::<[u64; 5]>(),
            counters in any::<[u32; 3]>(),
            deadline in prop::option::of(any::<u64>()),
            sweep_time in any::<u64>(),
            dependency in any::<bool>(),
        ) {
            let original = PendingRequestPolicy {
                revision,
                initial_timestamp: clocks[0],
                last_request_timestamp: clocks[1],
                requested_as_dependency: dependency,
                retry_attempts: counters[0],
                peer_requery_attempts: counters[1],
                peer_requery_cursor: counters[2],
                retry_budget_quarantine_until: deadline,
                dependency_recovery_last_request: Some(clocks[2]),
                broadcast_retry_last_request: Some(clocks[3]),
                peer_requery_last_request: Some(clocks[4]),
            };
            let result = original.renewed_retry_budget(sweep_time).unwrap();
            let due = deadline.is_some_and(|until| until <= sweep_time);
            prop_assert_eq!(result.is_some(), due);
            if let Some(mut renewed) = result {
                prop_assert_eq!(renewed.retry_attempts, 0);
                prop_assert_eq!(renewed.retry_budget_quarantine_until, None);
                renewed.retry_attempts = original.retry_attempts;
                renewed.retry_budget_quarantine_until = original.retry_budget_quarantine_until;
                prop_assert_eq!(renewed, original);
            }
        }
    }

    #[test]
    fn rejects_unknown_version_zero_revision_and_noncanonical_bytes() {
        let original = policy();
        let bytes = original.encode().unwrap();
        assert_eq!(bytes.len(), PendingRequestPolicy::MAX_ENCODED_BYTES);
        let mut unknown = bytes.clone();
        unknown[..2].copy_from_slice(&2u16.to_le_bytes());
        assert!(PendingRequestPolicy::decode(&unknown).is_err());
        let mut zero_revision = bytes.clone();
        zero_revision[2..10].fill(0);
        assert!(PendingRequestPolicy::decode(&zero_revision).is_err());
        for length in 0..bytes.len() {
            assert!(PendingRequestPolicy::decode(&bytes[..length]).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(PendingRequestPolicy::decode(&trailing).is_err());
        let mut shorter = original;
        shorter.retry_budget_quarantine_until = None;
        let mut trailing = shorter.encode().unwrap();
        trailing.push(0);
        assert!(PendingRequestPolicy::decode(&trailing).is_err());
    }

    #[test]
    fn exhausted_revision_does_not_mutate_policy() {
        let mut policy = policy();
        policy.revision = u64::MAX;
        let before = policy.clone();
        assert!(policy.advance_revision().is_err());
        assert_eq!(policy, before);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn bounded_policy_encoding_preserves_every_field(
            revision in 1u64..=u64::MAX,
            timestamps in any::<[u64; 2]>(),
            authority in any::<bool>(),
            counters in any::<[u32; 3]>(),
            deadlines in any::<[Option<u64>; 4]>(),
        ) {
            let expected = PendingRequestPolicy {
                revision,
                initial_timestamp: timestamps[0],
                last_request_timestamp: timestamps[1],
                requested_as_dependency: authority,
                retry_attempts: counters[0],
                peer_requery_attempts: counters[1],
                peer_requery_cursor: counters[2],
                retry_budget_quarantine_until: deadlines[0],
                dependency_recovery_last_request: deadlines[1],
                broadcast_retry_last_request: deadlines[2],
                peer_requery_last_request: deadlines[3],
            };
            let encoded = expected.encode().unwrap();
            prop_assert!(encoded.len() <= PendingRequestPolicy::MAX_ENCODED_BYTES);
            prop_assert_eq!(PendingRequestPolicy::decode(&encoded).unwrap(), expected.clone());
            if revision < u64::MAX {
                let mut next = expected.clone();
                next.advance_revision().unwrap();
                prop_assert_eq!(next.revision, revision + 1);
                next.revision = revision;
                prop_assert_eq!(next, expected);
            }
        }

        #[test]
        fn arbitrary_policy_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..129)) {
            if let Ok(policy) = PendingRequestPolicy::decode(&bytes) {
                prop_assert_eq!(policy.encode().unwrap(), bytes);
                prop_assert_ne!(policy.revision, 0);
            }
        }
    }
}
