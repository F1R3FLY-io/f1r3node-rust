use std::num::NonZeroUsize;

use super::recovery_pages::{validate_cursor_bounds, ExclusiveGate, RecoveryStore};

pub(super) trait IntegrityStore: RecoveryStore {
    type Genesis: Clone + Eq;
    type Head: Clone + Eq;

    fn genesis(&self) -> Result<Option<Self::Genesis>, Self::Error>;
    fn head(&self) -> Result<Option<Self::Head>, Self::Error>;
    fn non_empty(&self) -> Result<bool, Self::Error>;
    fn genesis_head(genesis: &Self::Genesis) -> Self::Head;
    fn revision(head: &Self::Head) -> u64;
    fn validate_initialized_endpoints(
        &self,
        genesis: &Self::Genesis,
        head: &Self::Head,
    ) -> Result<(), Self::Error>;
    fn record_head(&self, revision: u64) -> Result<Option<Self::Head>, Self::Error>;
    fn validate_next(
        &self,
        expected: &Self::Head,
        revision: u64,
    ) -> Result<Self::Head, Self::Error>;
}

pub(super) struct Captured<G, H> {
    pub snapshot: Option<(G, H)>,
    pub validated: Option<H>,
    pub complete: bool,
}

pub(super) fn capture<S: IntegrityStore>(
    store: &S,
) -> Result<Captured<S::Genesis, S::Head>, S::Error> {
    let _guard = store.gate().lock();
    let genesis = store.genesis()?;
    let head = store.head()?;
    match (genesis, head) {
        (None, None) if !store.non_empty()? => Ok(Captured {
            snapshot: None,
            validated: None,
            complete: true,
        }),
        (None, None) => Err(S::serialization(
            "finalization ledger contains partial bootstrap data".to_string(),
        )),
        (None, Some(_)) => Err(S::serialization(
            "finalization ledger head exists without an immutable genesis anchor".to_string(),
        )),
        (Some(_), None) => Err(S::serialization(
            "finalization genesis anchor exists without a durable head".to_string(),
        )),
        (Some(genesis), Some(head)) => {
            store.validate_initialized_endpoints(&genesis, &head)?;
            let validated = Some(S::genesis_head(&genesis));
            Ok(Captured {
                snapshot: Some((genesis, head)),
                validated,
                complete: false,
            })
        }
    }
}

pub(super) struct Integrity<'a, G, H, E> {
    pub snapshot: &'a Option<(G, H)>,
    pub validated: &'a mut Option<H>,
    pub failure: &'a mut Option<E>,
    pub complete: &'a mut bool,
}

pub(super) fn validate_page<S: IntegrityStore>(
    store: &S,
    progress: Integrity<'_, S::Genesis, S::Head, S::Error>,
    budget: NonZeroUsize,
) -> Result<bool, S::Error> {
    if let Some(error) = &progress.failure {
        return Err(error.clone());
    }
    if *progress.complete {
        return Ok(true);
    }
    let result = (|| {
        let _guard = store.gate().lock();
        let Some((genesis, target)) = progress.snapshot else {
            return Ok(true);
        };
        if store.genesis()?.as_ref() != Some(genesis) {
            return Err(S::serialization(
                "finalization genesis changed during integrity audit".to_string(),
            ));
        }
        let current = store.head()?.ok_or_else(|| {
            S::serialization("finalization head disappeared during integrity audit".to_string())
        })?;
        if S::revision(&current) < S::revision(target)
            || (S::revision(&current) == S::revision(target) && current != *target)
        {
            return Err(S::serialization(
                "finalization head regressed or changed during integrity audit".to_string(),
            ));
        }
        validate_cursor_bounds(store, S::revision(&current))?;
        if S::revision(target) > 0 {
            let endpoint = store.record_head(S::revision(target))?.ok_or_else(|| {
                S::serialization("captured finalization audit endpoint disappeared".to_string())
            })?;
            if endpoint != *target {
                return Err(S::serialization(
                    "captured finalization audit endpoint changed".to_string(),
                ));
            }
        }
        let mut expected = progress.validated.clone().ok_or_else(|| {
            S::serialization("finalization audit has no validated genesis prefix".to_string())
        })?;
        for _ in 0..budget.get() {
            if S::revision(&expected) == S::revision(target) {
                break;
            }
            let revision = S::revision(&expected).checked_add(1).ok_or_else(|| {
                S::serialization("finalization integrity cursor exhausted".to_string())
            })?;
            expected = store.validate_next(&expected, revision)?;
        }
        let complete = S::revision(&expected) == S::revision(target);
        if complete && expected != *target {
            return Err(S::serialization(
                "finalization ledger chain does not terminate at its captured head".to_string(),
            ));
        }
        *progress.validated = Some(expected);
        Ok(complete)
    })();
    match result {
        Ok(complete) => {
            *progress.complete = complete;
            Ok(complete)
        }
        Err(error) => {
            *progress.failure = Some(error.clone());
            Err(error)
        }
    }
}
