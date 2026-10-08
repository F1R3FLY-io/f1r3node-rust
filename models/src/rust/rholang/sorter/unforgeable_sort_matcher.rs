// See models/src/main/scala/coop/rchain/models/rholang/sorter/UnforgeableSortMatcher.scala

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::GUnforgeable;

pub struct UnforgeableSortMatcher;

impl UnforgeableSortMatcher {
    pub fn sort_match_metered(
        unf: &GUnforgeable,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<GUnforgeable>, BackingError> {
        let (tag, count) = match &unf.unf_instance {
            Some(UnfInstance::GPrivateBody(_)) => (Score::PRIVATE, 1),
            Some(UnfInstance::GDeployerIdBody(_)) => (Score::DEPLOYER_AUTH, 1),
            Some(UnfInstance::GDeployIdBody(_)) => (Score::DEPLOY_ID, 1),
            Some(UnfInstance::GSysAuthTokenBody(_)) => (Score::SYS_AUTH_TOKEN, 0),
            Some(UnfInstance::GAuthorityIdBody(_)) => (Score::AUTHORITY_ID, 1),
            Some(UnfInstance::GPrincipalIdBody(_)) => (Score::PRINCIPAL_ID, 2),
            None => (Score::ABSENT, 0),
        };
        // Changed by D-E4 (DR-111): Rule S, one read of each slot.
        // let mut children = meter.vec(count)?;
        let mut children = meter.score_vec(count)?;
        match &unf.unf_instance {
            Some(UnfInstance::GPrivateBody(value)) => children.push(
                // Changed by D-O1 (DR-111): block accounting.
                // Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.id)?),
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone_blocks(&value.id)?),
            ),
            Some(UnfInstance::GDeployerIdBody(value)) => children.push(
                // Changed by D-O1 (DR-111): block accounting.
                // Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.public_key)?),
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone_blocks(&value.public_key)?),
            ),
            Some(UnfInstance::GDeployIdBody(value)) => children.push(
                // Changed by D-O1 (DR-111): block accounting.
                // Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.sig)?),
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone_blocks(&value.sig)?),
            ),
            Some(UnfInstance::GAuthorityIdBody(value)) => children.push(
                // Changed by D-O1 (DR-111): block accounting.
                // Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.id)?),
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone_blocks(&value.id)?),
            ),
            Some(UnfInstance::GPrincipalIdBody(value)) => {
                children.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
                    value.key_family,
                )));
                children.push(Tree::<ScoreAtom>::create_leaf_from_bytes(
                    // Changed by D-O1 (DR-111): block accounting.
                    // meter.clone(&value.public_key)?,
                    meter.clone_blocks(&value.public_key)?,
                ));
            }
            Some(UnfInstance::GSysAuthTokenBody(_)) | None => {}
        }
        Ok(ScoredTerm {
            // Changed by D-O1 (DR-111): block accounting.
            // term: meter.clone(unf)?,
            term: meter.clone_blocks(unf)?,
            score: Tree::<ScoreAtom>::create_node_from_i32_metered(tag, children, meter)?,
        })
    }
}

impl Sortable<GUnforgeable> for UnforgeableSortMatcher {
    fn sort_match(unf: &GUnforgeable) -> ScoredTerm<GUnforgeable> {
        match &unf.unf_instance {
            Some(unf) => match unf {
                UnfInstance::GPrivateBody(gpriv) => ScoredTerm {
                    term: GUnforgeable {
                        unf_instance: Some(UnfInstance::GPrivateBody(gpriv.clone())),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::PRIVATE, vec![
                        Tree::<ScoreAtom>::create_leaf_from_bytes(gpriv.id.clone()),
                    ]),
                },

                UnfInstance::GDeployerIdBody(id) => ScoredTerm {
                    term: GUnforgeable {
                        unf_instance: Some(UnfInstance::GDeployerIdBody(id.clone())),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::DEPLOYER_AUTH, vec![
                        Tree::<ScoreAtom>::create_leaf_from_bytes(id.public_key.clone()),
                    ]),
                },

                UnfInstance::GDeployIdBody(deploy_id) => ScoredTerm {
                    term: GUnforgeable {
                        unf_instance: Some(UnfInstance::GDeployIdBody(deploy_id.clone())),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::DEPLOY_ID, vec![
                        Tree::<ScoreAtom>::create_leaf_from_bytes(deploy_id.sig.clone()),
                    ]),
                },

                UnfInstance::GSysAuthTokenBody(token) => ScoredTerm {
                    term: GUnforgeable {
                        unf_instance: Some(UnfInstance::GSysAuthTokenBody(token.clone())),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::SYS_AUTH_TOKEN, vec![]),
                },

                UnfInstance::GAuthorityIdBody(id) => ScoredTerm {
                    term: GUnforgeable {
                        unf_instance: Some(UnfInstance::GAuthorityIdBody(id.clone())),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::AUTHORITY_ID, vec![
                        Tree::<ScoreAtom>::create_leaf_from_bytes(id.id.clone()),
                    ]),
                },

                UnfInstance::GPrincipalIdBody(id) => ScoredTerm {
                    term: GUnforgeable {
                        unf_instance: Some(UnfInstance::GPrincipalIdBody(id.clone())),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(Score::PRINCIPAL_ID, vec![
                        Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(id.key_family)),
                        Tree::<ScoreAtom>::create_leaf_from_bytes(id.public_key.clone()),
                    ]),
                },
            },
            None => ScoredTerm {
                term: unf.clone(),
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::ABSENT, Vec::new()),
            },
        }
    }
}
