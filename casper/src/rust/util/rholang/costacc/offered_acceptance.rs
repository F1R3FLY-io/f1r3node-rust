//! DR-102: test-only records of offered deploy attempts. Changed by DR-131:
//! the records no longer carry host-work usage.

#[cfg(any(test, feature = "test-utils"))]
pub use recorder::{
    OfferedBudgetRecorder, OfferedBudgetUsage, OfferedCheckRecord, OfferedCheckRole,
    OfferedUsageKind,
};

#[cfg(any(test, feature = "test-utils"))]
mod recorder {
    use std::sync::{Arc, Mutex};

    /// The step that a record marks.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum OfferedUsageKind {
        ProducerSelfReplay,
        ValidatorReplay,
        /// Added by G1-6 (DR-124): an offered play enters its runtime. The
        /// play records it just before execution.
        PlayRuntimeEntry,
        /// Added by G1-6 (DR-124): an offered replay enters its runtime. The
        /// replay records it just before evaluation.
        ReplayRuntimeEntry,
        /// Added by G1-6 (DR-124): the decision of one check before execution.
        Check(OfferedCheckRole, OfferedCheckRecord),
        /// Added by G1-6 (DR-124): a correctness failure under the bound
        /// source `Certificate` (design §9). The producer also logs it.
        CertificateCorrectnessFailure,
        /// Added by G1-6 (DR-124): whether the URN map of the native replay
        /// environment equals the URN map that the replay check read.
        ReplayUrnMap {
            matches_check: bool,
        },
        /// Added by G1-6 (DR-124), a test hook of one node: the node skips the
        /// check for the deploy and binds the signed limit, as before G1-6.
        SkipCheckHook,
        /// Added by G1-6 (DR-124), a test hook of one node: the check of the
        /// deploy sees an estimate without counts. A complete offer then runs
        /// under the certified bound 0 and passes it.
        ZeroEstimateHook,
        /// Added by G1-7 (DR-65 item 5): the outcome of one held-capacity
        /// guard on the success path. Play records its own guard, and replay
        /// records the guard that it recomputes for a committed rollback.
        HeldCapacityGuard {
            role: OfferedCheckRole,
            holds: bool,
        },
        /// Added by G1-7 (DR-65 item 5), a test hook of one node: the guard of
        /// the deploy fails on that node, in play and in replay. A producer
        /// with the hook claims a guard failure that other nodes reject.
        ForceGuardFailureHook,
        /// Added by DR-134 (DR-65 O-13), a test hook of one node: the guard of
        /// the deploy ignores its later holds on that node, in play and in
        /// replay. A producer with the hook commits a success that other
        /// nodes, which count the later holds, reject.
        IgnoreLaterHoldsHook,
    }

    /// Added by G1-6 (DR-124): the path that ran a check before execution.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum OfferedCheckRole {
        Play,
        Replay,
    }

    /// Added by G1-6 (DR-124): the decision of one check. An accepted offer
    /// keeps the kind of its estimate, B, the resource bound, the bound source
    /// and the total hold.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum OfferedCheckRecord {
        Accepted {
            complete: bool,
            estimate: u64,
            resource_bound: u64,
            bound_source:
                rholang::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource,
            total_hold: u128,
        },
        Rejected,
    }

    /// One step of one completed attempt.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct OfferedBudgetUsage {
        pub kind: OfferedUsageKind,
        pub deploy_id: Vec<u8>,
        pub pre_state_root: Vec<u8>,
        pub attempt: usize,
    }

    /// Test-only records of offered deploy attempts, shared by every clone of
    /// a runtime manager.
    #[derive(Clone, Default)]
    pub struct OfferedBudgetRecorder(Arc<Mutex<Vec<OfferedBudgetUsage>>>);

    impl OfferedBudgetRecorder {
        pub fn record(&self, kind: OfferedUsageKind, deploy_id: &[u8], pre_state_root: &[u8]) {
            let mut records = self.0.lock().expect("offered usage records lock");
            let attempt = records
                .iter()
                .filter(|record| {
                    record.kind == kind
                        && record.deploy_id == deploy_id
                        && record.pre_state_root == pre_state_root
                })
                .count();
            records.push(OfferedBudgetUsage {
                kind,
                deploy_id: deploy_id.to_vec(),
                pre_state_root: pre_state_root.to_vec(),
                attempt,
            });
        }

        pub fn records(&self) -> Vec<OfferedBudgetUsage> {
            self.0.lock().expect("offered usage records lock").clone()
        }
    }
}
