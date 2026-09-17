use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;

/// Maximum number of pending deploys returned by a single
/// `getPendingDeploys` call. The cap protects consumers from unbounded
/// responses when the pending queue grows abnormally large. Callers
/// detect truncation by comparing `deploys.len() < total_available`.
pub const PENDING_DEPLOYS_MAX_RESULTS: usize = 1000;

/// Result of a bulk pending-deploys snapshot. The deploy list pairs each
/// deploy with an `is_rejected` flag: `false` for fresh deploys in
/// `deploy_storage` (not yet proposed), `true` for deploys in the
/// `rejected_deploy_buffer` (recovering after a merge conflict).
#[derive(Clone, Debug)]
pub struct PendingDeploysSnapshot {
    pub deploys: Vec<(Signed<DeployData>, bool)>,
    /// Total count of pending deploys that matched the query before cap
    /// truncation was applied.
    pub total_available: u32,
}

impl PendingDeploysSnapshot {
    pub fn empty() -> Self {
        Self {
            deploys: Vec::new(),
            total_available: 0,
        }
    }
}
