/// System deploy marker constants.
/// System deploy IDs are 33 bytes: [32-byte blockHash][1-byte marker].
/// Consensus-bearing: they identify system deploys in blocks' rejected sets.

/// Marker for slash system deploys
pub const SLASH_MARKER: u8 = 0x01;
/// Marker for close block system deploys
pub const CLOSE_BLOCK_MARKER: u8 = 0x02;
/// Marker for empty/heartbeat system deploys
pub const HEARTBEAT_MARKER: u8 = 0x03;
/// Marker for claim-resolution system deploys
pub const RESOLVE_CLAIMS_MARKER: u8 = 0x05;
/// Marker for dishonest-vote evidence system deploys
pub const DISHONEST_VOTE_MARKER: u8 = 0x06;

/// System deploy ID length (32-byte hash + 1-byte marker)
pub const SYSTEM_DEPLOY_ID_LEN: usize = 33;

use models::rust::casper::protocol::casper_message::{ProcessedSystemDeploy, SystemDeployData};
use prost::bytes::Bytes;

/// Detect if a deploy ID is a system deploy ID.
/// System deploy IDs are 33 bytes: [32-byte blockHash][1-byte marker]
/// Markers: 0x01 (slash), 0x02 (close block), 0x03 (empty/heartbeat),
/// 0x05 (resolve claims), 0x06 (dishonest vote)
pub fn is_system_deploy_id(id: &Bytes) -> bool {
    id.len() == SYSTEM_DEPLOY_ID_LEN && {
        let last_byte = id[32];
        last_byte == SLASH_MARKER
            || last_byte == CLOSE_BLOCK_MARKER
            || last_byte == HEARTBEAT_MARKER
            || last_byte == RESOLVE_CLAIMS_MARKER
            || last_byte == DISHONEST_VOTE_MARKER
    }
}

/// Detect if a deploy ID is specifically a slash system deploy.
pub fn is_slash_deploy_id(id: &Bytes) -> bool {
    id.len() == SYSTEM_DEPLOY_ID_LEN && id[32] == SLASH_MARKER
}

/// Whether a system deploy carries an action the network needs to land, as
/// opposed to the per-block bookkeeping every proposal appends.
///
/// Exhaustive on purpose. A new system deploy has to answer this question,
/// and the empty-block progress rule reads the answer instead of testing for
/// one variant and counting the rest — a `matches!` there would compile
/// unchanged and silently put the new deploy on the wrong side of the rule.
pub fn is_consensus_action(processed: &ProcessedSystemDeploy) -> bool {
    match processed {
        ProcessedSystemDeploy::Succeeded { system_deploy, .. } => match system_deploy {
            SystemDeployData::Slash { .. }
            | SystemDeployData::ResolveClaims { .. }
            | SystemDeployData::DishonestVote { .. } => true,
            SystemDeployData::CloseBlockSystemDeployData | SystemDeployData::Empty => false,
        },
        ProcessedSystemDeploy::Failed { .. } => false,
    }
}

/// Whether a system deploy is the close-block bookkeeping that carries no
/// action of its own.
pub fn is_bookkeeping(processed: &ProcessedSystemDeploy) -> bool {
    match processed {
        ProcessedSystemDeploy::Succeeded { system_deploy, .. } => match system_deploy {
            SystemDeployData::CloseBlockSystemDeployData => true,
            SystemDeployData::Slash { .. }
            | SystemDeployData::ResolveClaims { .. }
            | SystemDeployData::DishonestVote { .. }
            | SystemDeployData::Empty => false,
        },
        ProcessedSystemDeploy::Failed { .. } => false,
    }
}

/// Whether a block's deploy content settles the empty-block progress rule on
/// its own, without reference to its parents: it serves paying users, it
/// lands a consensus action, or it is the well-formed empty block the
/// heartbeat relies on. Anything else has to show a parent its proposer had
/// not already seen.
///
/// The single-deploy shape is deliberate: a block with no system deploys at
/// all, or with the close-block deploy repeated, is not what a proposal
/// builds and does not earn the liveness exemption.
pub fn deploy_content_shows_progress(
    has_user_deploys: bool,
    system_deploys: &[ProcessedSystemDeploy],
) -> bool {
    if has_user_deploys {
        return true;
    }
    system_deploys.iter().any(is_consensus_action)
        || (system_deploys.len() == 1 && is_bookkeeping(&system_deploys[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close() -> ProcessedSystemDeploy {
        ProcessedSystemDeploy::Succeeded {
            event_list: Vec::new(),
            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
        }
    }

    fn slash() -> ProcessedSystemDeploy {
        let commitment = crate::rust::util::proto_util::block_commitment(
            &models::rust::block_implicits::get_random_block_default(),
        );
        ProcessedSystemDeploy::Succeeded {
            event_list: Vec::new(),
            system_deploy: SystemDeployData::create_slash(
                models::rust::casper::protocol::casper_message::EquivocationEvidence {
                    first: commitment.clone(),
                    second: commitment,
                },
                crypto::rust::public_key::PublicKey::from_bytes(b"issuer"),
            ),
        }
    }

    fn failed() -> ProcessedSystemDeploy {
        ProcessedSystemDeploy::Failed {
            event_list: Vec::new(),
            error_msg: "boom".to_string(),
        }
    }

    #[test]
    fn a_slash_is_an_action_and_a_close_block_is_not() {
        assert!(is_consensus_action(&slash()));
        assert!(!is_bookkeeping(&slash()));
        assert!(is_bookkeeping(&close()));
        assert!(!is_consensus_action(&close()));
    }

    #[test]
    fn a_failed_system_deploy_is_neither() {
        assert!(!is_consensus_action(&failed()));
        assert!(!is_bookkeeping(&failed()));
    }

    #[test]
    fn user_deploys_and_consensus_actions_show_progress_on_their_own() {
        assert!(deploy_content_shows_progress(true, &[]));
        assert!(deploy_content_shows_progress(false, &[slash(), close()]));
    }

    #[test]
    fn the_well_formed_empty_block_shows_progress_and_other_empty_shapes_do_not() {
        assert!(deploy_content_shows_progress(false, &[close()]));
        assert!(
            !deploy_content_shows_progress(false, &[]),
            "a block with no system deploys at all is not what a proposal builds"
        );
        // The shape any additional non-action system deploy produces: the
        // block is still empty of work, and the single-deploy exemption no
        // longer applies, so it has to show a new parent.
        assert!(!deploy_content_shows_progress(false, &[close(), close()]));
        assert!(!deploy_content_shows_progress(false, &[failed(), close()]));
    }
}
