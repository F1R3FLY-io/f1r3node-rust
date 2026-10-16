// See models/src/main/scala/coop/rchain/models/rholang/sorter/UnforgeableSortMatcher.scala
//
// A `GUnforgeable` carries bytes, never a `Par`, so it is a LEAF and needs no
// driver. The implementation lives in `sort_combine`.

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sort_combine::sort_unforgeable;
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
        let mut children = meter.vec(count)?;
        match &unf.unf_instance {
            Some(UnfInstance::GPrivateBody(value)) => children.push(
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.id)?),
            ),
            Some(UnfInstance::GDeployerIdBody(value)) => children.push(
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.public_key)?),
            ),
            Some(UnfInstance::GDeployIdBody(value)) => children.push(
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.sig)?),
            ),
            Some(UnfInstance::GAuthorityIdBody(value)) => children.push(
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(&value.id)?),
            ),
            Some(UnfInstance::GPrincipalIdBody(value)) => {
                children.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
                    value.key_family,
                )));
                children.push(Tree::<ScoreAtom>::create_leaf_from_bytes(
                    meter.clone(&value.public_key)?,
                ));
            }
            Some(UnfInstance::GSysAuthTokenBody(_)) | None => {}
        }
        Ok(ScoredTerm {
            term: meter.clone(unf)?,
            score: Tree::<ScoreAtom>::create_node_from_i32_metered(tag, children, meter)?,
        })
    }
}

impl Sortable<GUnforgeable> for UnforgeableSortMatcher {
    fn sort_match(unf: &GUnforgeable) -> ScoredTerm<GUnforgeable> { sort_unforgeable(unf) }
}
