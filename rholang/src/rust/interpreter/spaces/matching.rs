//! Metered matching for the native spaces (P4 design B.5, readiness review S5, decision 42).
//!
//! A space matches with the injected matcher of its host (`SpaceStoreHandle::matcher`): the
//! same `Match` implementation that host play and replay use, never a hard-wired one. With a
//! host-work budget it calls the metered methods `get_metered` and `check_commit_metered`, which
//! reserve every read that they make on the meter of the job (DR-88 of
//! `feature/cost-accounted-rho`). The caller adds no charge for a call, and it inspects nothing
//! before the call. Without a budget it calls `get` and `check_commit`, as the host does, and
//! the results are equal. A host-work rejection aborts the operation with `HostWorkRejected`.
//! Matching is synchronous, so a space job never awaits between `begin` and `commit`.
//!
//! A datum is matched without its envelope (`record::unwrap_datum`), with its random state,
//! its cost authority and its cost stack. The rholang matcher refuses a datum with a cost stack,
//! as it does for the host.

use std::sync::Arc;

use models::rhoapi::{BindPattern, ListParWithRandom, TaggedContinuation};
use rspace_plus_plus::rspace::r#match::Match;

use super::agent::SpaceEnv;
use super::errors::SpaceError;
use crate::rust::interpreter::reified::costs::ScanMeter;

/// The injected matcher of a host store.
pub type ParMatcher = Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>>;

/// The matcher of one space job and its host-work meter.
pub struct SpaceMatcher<'a> {
    matcher: ParMatcher,
    meter: &'a ScanMeter,
}

impl<'a> SpaceMatcher<'a> {
    /// The injected matcher of the host of `env` with the meter of the job.
    pub fn new(env: &'a SpaceEnv) -> Self {
        Self {
            matcher: env.store.matcher(),
            meter: &env.meter,
        }
    }

    /// [`SpaceMatcher::new`] with the copy of the shared matcher handle reserved.
    pub fn metered(env: &'a SpaceEnv) -> Result<Self, SpaceError> {
        env.meter.shared(1)?;
        Ok(Self::new(env))
    }

    /// The host-work meter of the job.
    pub fn meter(&self) -> &'a ScanMeter { self.meter }

    /// The spatial match of `pattern` against `data`: the bound values in binder order with the
    /// random state and the cost authority of `data`, or `None`.
    pub fn bind(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
    ) -> Result<Option<ListParWithRandom>, SpaceError> {
        match self.meter.is_metered() {
            true => Ok(self.matcher.get_metered(pattern, data, self.meter)?),
            false => Ok(self.matcher.get(pattern, data)),
        }
    }

    /// The `where` guard of `continuation` over the bound values `matched` of its one bind. A
    /// continuation without a guard admits every match. The matcher reads `matched` by
    /// reference, so neither path copies it.
    pub fn admits(
        &self,
        continuation: &TaggedContinuation,
        matched: &ListParWithRandom,
    ) -> Result<bool, SpaceError> {
        match self.meter.is_metered() {
            true => Ok(self
                .matcher
                .check_commit_metered(continuation, &[matched], self.meter)?),
            false => Ok(self
                .matcher
                .check_commit(continuation, std::slice::from_ref(matched))),
        }
    }

    /// The `where` guard of `continuation` over the bound values `matched` of all its binds, in
    /// the bind order of its receive (decision 37: one check, after every bind matched). A
    /// continuation without a guard admits every match. With a meter, the slice of references
    /// that the metered check reads is a reserved buffer, and no value is copied. Without one,
    /// the check reads `matched` as the host reads the matches of a join.
    pub fn admits_all(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> Result<bool, SpaceError> {
        match self.meter.is_metered() {
            true => {
                let mut references = self.meter.buffer::<&ListParWithRandom>(matched.len())?;
                references.extend(matched.iter());
                Ok(self
                    .matcher
                    .check_commit_metered(continuation, &references, self.meter)?)
            }
            false => Ok(self.matcher.check_commit(continuation, matched)),
        }
    }

    /// The match of one bind: the spatial match of `pattern` against `data`, then the guard of
    /// `continuation`. `None` when either refuses.
    pub fn matches(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        continuation: &TaggedContinuation,
    ) -> Result<Option<ListParWithRandom>, SpaceError> {
        let Some(matched) = self.bind(pattern, data)? else {
            return Ok(None);
        };
        match self.admits(continuation, &matched)? {
            true => Ok(Some(matched)),
            false => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{Par, ParWithRandom};
    use models::rust::host_work::{
        HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUsage,
    };
    use models::rust::utils::{new_freevar_par, new_gbool_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::errors::RSpaceError;
    use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;

    async fn env(host_work: Option<HostWorkBudget>) -> SpaceEnv {
        let mut stores = InMemoryStoreManager::new();
        let host = RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        SpaceEnv {
            store: SpaceStoreHandle::new(host),
            budget: RuntimeBudget::new(Cost::create(1_000, "space matching test")),
            meter: ScanMeter::new(host_work),
            volatile: Default::default(),
        }
    }

    fn budget(limit: u64) -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)))
    }

    fn any() -> BindPattern {
        BindPattern {
            patterns: vec![new_freevar_par(0, Vec::new())],
            remainder: None,
            free_count: 1,
        }
    }

    fn exactly(value: i64) -> BindPattern {
        BindPattern {
            patterns: vec![new_gint_par(value, Vec::new(), false)],
            remainder: None,
            free_count: 0,
        }
    }

    fn datum(value: i64) -> ListParWithRandom {
        ListParWithRandom {
            pars: vec![new_gint_par(value, Vec::new(), false)],
            random_state: vec![value as u8],
            cost_authority: None,
            cost_stack: None,
        }
    }

    fn continuation(guard: Option<Par>) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(Par::default()),
                random_state: Vec::new(),
            })),
            guard,
            cost_authority: None,
        }
    }

    #[tokio::test]
    async fn space_matching_binds_with_the_injected_matcher() {
        let env = env(None).await;
        let matcher = SpaceMatcher::new(&env);
        let bound = matcher
            .bind(&any(), &datum(5))
            .expect("the match fits the budget");
        assert_eq!(bound, Matcher.get(&any(), &datum(5)));
        let bound = bound.expect("a free variable binds the datum");
        assert_eq!(bound.pars, datum(5).pars);
        assert_eq!(bound.random_state, datum(5).random_state);
        assert_eq!(
            matcher.bind(&exactly(6), &datum(5)),
            Ok(None),
            "a mismatch binds nothing"
        );
        assert_eq!(
            matcher.matches(&exactly(5), &datum(5), &continuation(None)),
            Ok(Some(ListParWithRandom {
                pars: Vec::new(),
                ..datum(5)
            }))
        );
    }

    /// The usage of `host_work` in the dimensions that a space job charges.
    fn charged(host_work: &HostWorkBudget) -> [u64; 3] {
        [
            HostWorkDimension::VerificationOperations,
            HostWorkDimension::VerificationBytes,
            HostWorkDimension::SearchStateBytes,
        ]
        .map(|dimension| host_work.usage(dimension).get())
    }

    /// A continuation without a guard whose body has `bytes` bytes.
    fn continuation_with_body(bytes: usize) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(new_gstring_par("b".repeat(bytes), Vec::new(), false)),
                random_state: Vec::new(),
            })),
            guard: None,
            cost_authority: None,
        }
    }

    #[tokio::test]
    async fn space_matching_reserves_only_what_the_matcher_reads() {
        let host_work = budget(1_000_000);
        let env = env(Some(host_work.clone())).await;
        let matcher = SpaceMatcher::new(&env);
        matcher
            .bind(&any(), &datum(5))
            .expect("the match fits the budget");
        let direct = budget(1_000_000);
        Matcher
            .get_metered(&any(), &datum(5), &ScanMeter::new(Some(direct.clone())))
            .expect("the match fits the budget");
        assert_eq!(
            charged(&host_work),
            charged(&direct),
            "bind adds nothing to the reads of the matcher"
        );
        assert!(charged(&host_work).iter().all(|amount| *amount > 0));
        assert_eq!(
            host_work.usage(HostWorkDimension::SearchCandidates),
            HostWorkUsage::ZERO
        );
        let before = charged(&host_work);
        matcher
            .admits(&continuation(None), &datum(5))
            .expect("the guard fits the budget");
        let unguarded = charged(&host_work);
        assert_eq!(
            [
                unguarded[0] - before[0],
                unguarded[1] - before[1],
                unguarded[2] - before[2]
            ],
            [1, 0, 0],
            "an unguarded check is one operation"
        );
        matcher
            .admits(&continuation_with_body(4096), &datum(5))
            .expect("the guard fits the budget");
        let after = charged(&host_work);
        assert_eq!(
            [
                after[0] - unguarded[0],
                after[1] - unguarded[1],
                after[2] - unguarded[2]
            ],
            [1, 0, 0],
            "the guard check never reads the body"
        );
    }

    #[test]
    fn legacy_mapping_dropped_operations_and_backing() {
        let full = RecordingMeter::new(false);
        Matcher
            .get_metered(&any(), &datum(5), &full)
            .expect("the recording meter accepts every charge");
        let legacy = RecordingMeter::new(true);
        Matcher
            .get_metered(&any(), &datum(5), &legacy)
            .expect("the recording meter accepts every charge");
        let [operations, scanned, backing] = full.recorded();
        assert!(operations >= 1 && backing >= 1, "the matcher reserves both");
        assert_eq!(
            legacy.recorded(),
            [0, scanned, 0],
            "negative control: the earlier mapping kept only the scanned bytes"
        );
    }

    /// A meter that records each reservation. With `legacy`, it keeps only the scanned bytes,
    /// as the meter of the agents did before decision 42.
    struct RecordingMeter {
        recorded: [AtomicUsize; 3],
        legacy: bool,
    }

    impl RecordingMeter {
        fn new(legacy: bool) -> Self {
            Self {
                recorded: Default::default(),
                legacy,
            }
        }

        fn recorded(&self) -> [usize; 3] {
            self.recorded
                .each_ref()
                .map(|amount| amount.load(Ordering::Relaxed))
        }
    }

    impl SourceMeter for RecordingMeter {
        fn reserve(
            &self,
            operations: usize,
            scanned: usize,
            backing: usize,
        ) -> Result<(), RSpaceError> {
            let [recorded_operations, recorded_scanned, recorded_backing] = &self.recorded;
            recorded_scanned.fetch_add(scanned, Ordering::Relaxed);
            if !self.legacy {
                recorded_operations.fetch_add(operations, Ordering::Relaxed);
                recorded_backing.fetch_add(backing, Ordering::Relaxed);
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn space_matching_guard_vetoes_a_spatial_match() {
        let env = env(None).await;
        let matcher = SpaceMatcher::new(&env);
        let refusing = continuation(Some(new_gbool_par(false, Vec::new(), false)));
        let admitting = continuation(Some(new_gbool_par(true, Vec::new(), false)));
        assert!(matcher
            .bind(&any(), &datum(1))
            .expect("the match fits the budget")
            .is_some());
        assert_eq!(matcher.admits(&refusing, &datum(1)), Ok(false));
        assert_eq!(matcher.matches(&any(), &datum(1), &refusing), Ok(None));
        assert!(matcher
            .matches(&any(), &datum(1), &admitting)
            .expect("the match fits the budget")
            .is_some());
        assert!(matcher
            .matches(&any(), &datum(1), &continuation(None))
            .expect("the match fits the budget")
            .is_some());
    }

    #[tokio::test]
    async fn space_matching_fails_closed_when_the_host_work_budget_rejects() {
        let env = env(Some(budget(0))).await;
        let matcher = SpaceMatcher::new(&env);
        let rejected = Some(SpaceError::Host(RSpaceError::HostWorkRejected));
        assert_eq!(matcher.bind(&any(), &datum(1)).err(), rejected);
        assert_eq!(
            matcher.admits(&continuation(None), &datum(1)).err(),
            rejected
        );
        assert_eq!(
            matcher
                .matches(&any(), &datum(1), &continuation(None))
                .err(),
            rejected
        );
        assert_eq!(
            InterpreterError::from(SpaceError::Host(RSpaceError::HostWorkRejected)),
            InterpreterError::HostWorkRejected
        );
        let scanning = env_with_calls_only().await;
        let matcher = SpaceMatcher::new(&scanning);
        assert_eq!(matcher.bind(&any(), &datum(1)).err(), rejected);
    }

    /// The guard `x < y` over the values of two binds: the bound values form one environment in
    /// bind order, so the value of the second bind is the bound variable 0.
    fn ascending() -> Par {
        use models::rhoapi::expr::ExprInstance;
        use models::rhoapi::{ELt, Expr};
        use models::rust::utils::new_boundvar_par;
        Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ELtBody(ELt {
                    p1: Some(new_boundvar_par(1, Vec::new(), false)),
                    p2: Some(new_boundvar_par(0, Vec::new(), false)),
                })),
            }],
            ..Par::default()
        }
    }

    #[tokio::test]
    async fn space_matching_join_guard_reads_every_bind_in_bind_order() {
        let guarded = continuation(Some(ascending()));
        let ordered = [datum(1), datum(2)];
        let reversed = [datum(2), datum(1)];
        for host_work in [None, Some(budget(u64::MAX))] {
            let env = env(host_work).await;
            let matcher = SpaceMatcher::new(&env);
            assert_eq!(matcher.admits_all(&guarded, &ordered), Ok(true));
            assert_eq!(matcher.admits_all(&guarded, &reversed), Ok(false));
            assert_eq!(matcher.admits_all(&continuation(None), &reversed), Ok(true));
            // The host checks the guard of a join once, over the matches of every bind.
            assert_eq!(
                matcher.admits_all(&guarded, &ordered),
                Ok(Matcher.check_commit(&guarded, &ordered))
            );
            assert_eq!(
                matcher.admits_all(&guarded, &reversed),
                Ok(Matcher.check_commit(&guarded, &reversed))
            );
        }
    }

    #[tokio::test]
    async fn space_matching_join_guard_reserves_one_check_and_its_buffer() {
        let host_work = budget(1_000_000);
        let env = env(Some(host_work.clone())).await;
        let matcher = SpaceMatcher::new(&env);
        let values = [datum(1), datum(2), datum(3)];
        let before = charged(&host_work);
        matcher
            .admits_all(&continuation(None), &values)
            .expect("the guard fits the budget");
        let after = charged(&host_work);
        let buffer_bytes = (values.len() * std::mem::size_of::<&ListParWithRandom>()) as u64;
        assert_eq!(
            [
                after[0] - before[0],
                after[1] - before[1],
                after[2] - before[2]
            ],
            [values.len() as u64 + 2, buffer_bytes, buffer_bytes],
            "the buffer of the references, then one unguarded check"
        );
        let refused = env_with_calls_only().await;
        assert_eq!(
            SpaceMatcher::new(&refused)
                .admits_all(&continuation(Some(ascending())), &values)
                .err(),
            Some(SpaceError::Host(RSpaceError::HostWorkRejected))
        );
    }

    // A budget of one unit in every dimension: room for one operation, but not for the reads of
    // one match.
    async fn env_with_calls_only() -> SpaceEnv { env(Some(budget(1))).await }

    #[tokio::test]
    async fn space_matching_refuses_a_datum_with_a_cost_stack() {
        let env = env(None).await;
        let matcher = SpaceMatcher::new(&env);
        let stacked = ListParWithRandom {
            cost_stack: Some(Default::default()),
            ..datum(1)
        };
        assert_eq!(matcher.bind(&any(), &stacked), Ok(None));
        assert_eq!(Matcher.get(&any(), &stacked), None);
    }
}
