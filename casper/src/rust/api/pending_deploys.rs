// Changed by DR-134 (DR-66): a pending deploy is a legacy deploy or an offered
// envelope, and on protocol 6 the pool holds only offered envelopes.
// use crypto::rust::signatures::signed::Signed;
// use models::rust::casper::protocol::casper_message::DeployData;
use block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate;

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
    // Changed by DR-134 (DR-66): legacy deploys and offered envelopes.
    // pub deploys: Vec<(Signed<DeployData>, bool)>,
    pub deploys: Vec<(PendingDeployCandidate, bool)>,
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

/// Added by DR-134 (DR-66): the public key of the deployer: the signer of a
/// legacy deploy, or the primary signer of an offered envelope.
pub fn deployer_key(deploy: &PendingDeployCandidate) -> &[u8] {
    match deploy {
        PendingDeployCandidate::Legacy(deploy) => &deploy.pk.bytes,
        PendingDeployCandidate::Envelope(envelope) => &envelope.primary().pk.bytes,
    }
}

/// Added by DR-134 (DR-66): the deploy timestamp.
pub fn timestamp(deploy: &PendingDeployCandidate) -> i64 {
    match deploy {
        PendingDeployCandidate::Legacy(deploy) => deploy.data.time_stamp,
        PendingDeployCandidate::Envelope(envelope) => envelope.body().time_stamp,
    }
}

/// Added by DR-134 (DR-66): the lookup identity: the signature of a legacy
/// deploy, or the 32-byte deploy id of an offered envelope.
pub fn lookup_id(deploy: &PendingDeployCandidate) -> &[u8] {
    match deploy {
        PendingDeployCandidate::Legacy(deploy) => &deploy.sig,
        PendingDeployCandidate::Envelope(envelope) => envelope.identity().as_bytes(),
    }
}
