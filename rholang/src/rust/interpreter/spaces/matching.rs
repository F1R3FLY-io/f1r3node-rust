//! Matching for the native spaces (P4 design B.5).
//!
//! A space matches with the injected matcher of its host (`SpaceStoreHandle::matcher`): the
//! same `Match` implementation that host play and replay use, never a hard-wired one. It calls
//! `get` and `check_commit`, as the host does. Matching meters nothing: a space operation meters
// Changed by decision 94: a job awaits the stripes of phase B after `begin`. The text before:
// ```
// //! only its COMMs and the bytes that it stores (DR-131). Matching is synchronous, so a space
// //! job never awaits between `begin` and `commit`.
// ```
//! only its COMMs and the bytes that it stores (DR-131). Matching is synchronous, so a space job
//! awaits between `begin` and `commit` only to take the stripes of phase B.
//!
//! A datum is matched without its envelope (`record::unwrap_datum`), with its random state,
//! its cost authority and its cost stack. The rholang matcher refuses a datum with a cost stack,
//! as it does for the host.

use std::sync::Arc;

use models::rhoapi::{BindPattern, ListParWithRandom, TaggedContinuation};
use rspace_plus_plus::rspace::r#match::Match;

use super::agent::SpaceEnv;

/// The injected matcher of a host store.
pub type ParMatcher = Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>>;

/// The matcher of one space job.
pub struct SpaceMatcher {
    matcher: ParMatcher,
}

impl SpaceMatcher {
    /// The injected matcher of the host of `env`.
    pub fn new(env: &SpaceEnv) -> Self {
        Self {
            matcher: env.store.matcher(),
        }
    }

    /// The spatial match of `pattern` against `data`: the bound values in binder order with the
    /// random state and the cost authority of `data`, or `None`.
    pub fn bind(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
    ) -> Option<ListParWithRandom> {
        self.matcher.get(pattern, data)
    }

    /// The `where` guard of `continuation` over the bound values `matched` of its one bind. A
    /// continuation without a guard admits every match. The check reads `matched` in place.
    pub fn admits(&self, continuation: &TaggedContinuation, matched: &ListParWithRandom) -> bool {
        self.matcher
            .check_commit(continuation, std::slice::from_ref(matched))
    }

    /// The `where` guard of `continuation` over the bound values `matched` of all its binds, in
    /// the bind order of its receive (decision 37: one check, after every bind matched). A
    /// continuation without a guard admits every match. The check reads `matched` as the host
    /// reads the matches of a join.
    pub fn admits_all(
        &self,
        continuation: &TaggedContinuation,
        matched: &[ListParWithRandom],
    ) -> bool {
        self.matcher.check_commit(continuation, matched)
    }

    /// The match of one bind: the spatial match of `pattern` against `data`, then the guard of
    /// `continuation`. `None` when either refuses.
    pub fn matches(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        continuation: &TaggedContinuation,
    ) -> Option<ListParWithRandom> {
        let matched = self.bind(pattern, data)?;
        match self.admits(continuation, &matched) {
            true => Some(matched),
            false => None,
        }
    }

    /// Added by decision 102 (plan v4 C4.1): [`may_veto`] with the matcher of the job.
    pub fn may_veto(&self, continuation: &TaggedContinuation, theory: bool) -> bool {
        may_veto(&**self.matcher, continuation, theory)
    }
}

/// Added by decision 102 (plan v4 C4.1, rules CR-7 and R15): true when the commit of
/// `continuation` may refuse a match. The matcher can veto its guard (DR-129 E3-a,
/// `Match::commit_may_veto`), or the space has a theory (`theory`), whose check can refuse any
/// stored datum (P7, TH-6). The pick helpers of a space and the row search of each discipline
/// share this one rule.
pub fn may_veto(
    matcher: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>,
    continuation: &TaggedContinuation,
    theory: bool,
) -> bool {
    theory || matcher.commit_may_veto(continuation)
}

#[cfg(test)]
mod tests {
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{Par, ParWithRandom};
    use models::rust::utils::{new_freevar_par, new_gbool_par, new_gint_par};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;

    async fn env() -> SpaceEnv {
        let mut stores = InMemoryStoreManager::new();
        let host = RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        SpaceEnv {
            store: SpaceStoreHandle::new(host),
            budget: RuntimeBudget::new(Cost::create(1_000, "space matching test")),
            volatile: Default::default(),
            origin: Default::default(),
        }
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
        let env = env().await;
        let matcher = SpaceMatcher::new(&env);
        let bound = matcher.bind(&any(), &datum(5));
        assert_eq!(bound, Matcher.get(&any(), &datum(5)));
        let bound = bound.expect("a free variable binds the datum");
        assert_eq!(bound.pars, datum(5).pars);
        assert_eq!(bound.random_state, datum(5).random_state);
        assert_eq!(
            matcher.bind(&exactly(6), &datum(5)),
            None,
            "a mismatch binds nothing"
        );
        assert_eq!(
            matcher.matches(&exactly(5), &datum(5), &continuation(None)),
            Some(ListParWithRandom {
                pars: Vec::new(),
                ..datum(5)
            })
        );
    }

    #[tokio::test]
    async fn space_matching_guard_vetoes_a_spatial_match() {
        let env = env().await;
        let matcher = SpaceMatcher::new(&env);
        let refusing = continuation(Some(new_gbool_par(false, Vec::new(), false)));
        let admitting = continuation(Some(new_gbool_par(true, Vec::new(), false)));
        assert!(matcher.bind(&any(), &datum(1)).is_some());
        assert!(!matcher.admits(&refusing, &datum(1)));
        assert_eq!(matcher.matches(&any(), &datum(1), &refusing), None);
        assert!(matcher.matches(&any(), &datum(1), &admitting).is_some());
        assert!(matcher
            .matches(&any(), &datum(1), &continuation(None))
            .is_some());
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
        let env = env().await;
        let matcher = SpaceMatcher::new(&env);
        assert!(matcher.admits_all(&guarded, &ordered));
        assert!(!matcher.admits_all(&guarded, &reversed));
        assert!(matcher.admits_all(&continuation(None), &reversed));
        // The host checks the guard of a join once, over the matches of every bind.
        assert_eq!(
            matcher.admits_all(&guarded, &ordered),
            Matcher.check_commit(&guarded, &ordered)
        );
        assert_eq!(
            matcher.admits_all(&guarded, &reversed),
            Matcher.check_commit(&guarded, &reversed)
        );
    }

    #[tokio::test]
    async fn space_matching_refuses_a_datum_with_a_cost_stack() {
        let env = env().await;
        let matcher = SpaceMatcher::new(&env);
        let stacked = ListParWithRandom {
            cost_stack: Some(Default::default()),
            ..datum(1)
        };
        assert_eq!(matcher.bind(&any(), &stacked), None);
        assert_eq!(Matcher.get(&any(), &stacked), None);
    }

    /// Added by decision 102 (plan v4 C4.1, rules CR-7 and R15): the commit of a continuation
    /// may veto when its guard can veto or when the space has a theory. Without a theory the
    /// rule is `commit_may_veto` of the matcher of the host (DR-129 E3-a). With a theory every
    /// continuation may veto.
    #[tokio::test]
    async fn space_matching_may_veto_follows_the_guard_and_the_theory() {
        let env = env().await;
        let matcher = SpaceMatcher::new(&env);
        let cases = [
            (continuation(None), false),
            (continuation(Some(Par::default())), false),
            (
                continuation(Some(new_gbool_par(true, Vec::new(), false))),
                true,
            ),
            (
                continuation(Some(new_gbool_par(false, Vec::new(), false))),
                true,
            ),
        ];
        for (continuation, vetoes) in &cases {
            assert_eq!(
                matcher.may_veto(continuation, false),
                *vetoes,
                "{continuation:?}"
            );
            assert_eq!(
                Matcher.commit_may_veto(continuation),
                *vetoes,
                "{continuation:?}"
            );
            assert!(
                matcher.may_veto(continuation, true),
                "a theory may refuse: {continuation:?}"
            );
        }
    }
}
