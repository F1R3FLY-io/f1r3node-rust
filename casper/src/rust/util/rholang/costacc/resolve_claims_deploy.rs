use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::expr::ExprInstance::EMapBody;
use models::rhoapi::{Expr, Par};
use models::rust::casper::protocol::casper_message::{
    ClaimFamily, ClaimId, ClaimOutcome, ClaimResolution, SystemDeployData,
};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::utils::{new_elist_par, new_gbool_par, new_gbytearray_par, new_gint_par};
use prost::bytes::Bytes;
use rholang::rust::interpreter::rho_type::{Extractor, RhoBoolean, RhoNil, RhoString};
use rspace_plus_plus::rspace::history::Either;

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;

/// Applies the outcomes of the claims whose tally crossed at the block's
/// floor, slashes the ejected validators whose grace the floor has passed,
/// and expires the claims whose deadline the floor has reached.
#[derive(Clone)]
pub struct ResolveClaimsDeploy {
    pub resolutions: Vec<ClaimResolution>,
    pub grace_expiries: Vec<Bytes>,
    pub expiries: Vec<ClaimId>,
    pub floor_hash: Bytes,
    pub floor_number: i64,
    pub pk: PublicKey,
    pub initial_rand: Blake2b512Random,
}

fn string_par(value: &str) -> Par {
    models::rust::utils::new_gstring_par(value.to_string(), Vec::new(), false)
}

fn map_par(entries: HashMap<Par, Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(EMapBody(ParMapTypeMapper::par_map_to_emap(
            ParMap::create_from_sorted_par_map(SortedParMap::create_from_map(entries)),
        ))),
    }])
}

/// The fields that name `claim` to the contract.
fn claim_fields(claim: &ClaimId) -> HashMap<Par, Par> {
    let family = match claim.family {
        ClaimFamily::Eject => "eject",
        ClaimFamily::Rejoin => "rejoin",
        ClaimFamily::Overflow => "overflow",
    };
    HashMap::from([
        (string_par("family"), string_par(family)),
        (
            string_par("target"),
            new_gbytearray_par(claim.target.to_vec(), Vec::new(), false),
        ),
        (
            string_par("openedAt"),
            new_gint_par(claim.opened_at, Vec::new(), false),
        ),
        (
            string_par("proposer"),
            new_gbytearray_par(claim.proposer.to_vec(), Vec::new(), false),
        ),
    ])
}

impl ResolveClaimsDeploy {
    fn resolutions_par(&self) -> Par {
        let resolutions = self
            .resolutions
            .iter()
            .map(|resolution| {
                let mut fields = claim_fields(&resolution.claim);
                fields.insert(
                    string_par("passed"),
                    new_gbool_par(resolution.outcome == ClaimOutcome::Pass, Vec::new(), false),
                );
                // A rejoin's topup is debited from its target's vault.
                // Only a system deploy can mint this identity, and validation
                // admits only the resolutions the block's floor requires.
                if resolution.claim.family == ClaimFamily::Rejoin {
                    let (_, payer) =
                        self.mk_deployer_id(&PublicKey::from_bytes(&resolution.claim.target));
                    fields.insert(string_par("payer"), payer);
                }
                map_par(fields)
            })
            .collect();
        new_elist_par(resolutions, Vec::new(), false, None, Vec::new(), false)
    }

    fn expiries_par(&self) -> Par {
        let expiries = self
            .expiries
            .iter()
            .map(|claim| map_par(claim_fields(claim)))
            .collect();
        new_elist_par(expiries, Vec::new(), false, None, Vec::new(), false)
    }
}

fn validators_par(validators: &[Bytes]) -> Par {
    let validators = validators
        .iter()
        .map(|validator| new_gbytearray_par(validator.to_vec(), Vec::new(), false))
        .collect();
    new_elist_par(validators, Vec::new(), false, None, Vec::new(), false)
}

impl SystemDeployTrait for ResolveClaimsDeploy {
    type Output = (RhoBoolean, Either<RhoString, RhoNil>);
    type Result = ();

    fn source() -> &'static str {
        r#"
          new rl(`rho:registry:lookup`),
          poSCh,
          resolutions(`sys:casper:claimResolutions`),
          graceExpiries(`sys:casper:graceExpiries`),
          claimExpiries(`sys:casper:claimExpiries`),
          floorHash(`sys:casper:claimFloorHash`),
          floorNumber(`sys:casper:claimFloorNumber`),
          sysAuthToken(`sys:casper:authToken`),
          return(`sys:casper:return`)
          in {
            rl!(`rho:system:pos`, *poSCh) |
            for(@(_, PoS) <- poSCh) {
              @PoS!("resolveClaims", *resolutions, *graceExpiries, *claimExpiries,
                    *floorHash, *floorNumber, *sysAuthToken, *return)
            }
        }"#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result> {
        match value {
            (true, _) => Either::Right(()),
            (false, Either::Left(error_msg)) => Either::Left(SystemDeployUserError::new(error_msg)),
            _ => Either::Left(SystemDeployUserError::new(
                "Claim resolution failed unexpectedly".to_string(),
            )),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn recorded_form(&self) -> SystemDeployData {
        SystemDeployData::ResolveClaims {
            resolutions: self.resolutions.clone(),
            grace_expiries: self.grace_expiries.clone(),
            expiries: self.expiries.clone(),
            floor_hash: self.floor_hash.clone(),
            floor_number: self.floor_number,
        }
    }

    fn rand(&self) -> Blake2b512Random { self.initial_rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();

        let (d_key, d_value) = self.mk_deployer_id(&self.pk);
        env.insert(d_key, d_value);

        env.insert(
            "sys:casper:claimResolutions".to_string(),
            self.resolutions_par(),
        );
        env.insert(
            "sys:casper:graceExpiries".to_string(),
            validators_par(&self.grace_expiries),
        );
        env.insert("sys:casper:claimExpiries".to_string(), self.expiries_par());
        env.insert(
            "sys:casper:claimFloorHash".to_string(),
            new_gbytearray_par(self.floor_hash.to_vec(), Vec::new(), false),
        );
        env.insert(
            "sys:casper:claimFloorNumber".to_string(),
            new_gint_par(self.floor_number, Vec::new(), false),
        );

        let (sys_key, sys_value) = self.mk_sys_auth_token();
        env.insert(sys_key, sys_value);

        let (ret_key, ret_value) = self.mk_return_channel();
        env.insert(ret_key, ret_value);

        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> {
        match self.env().get("sys:casper:return") {
            Some(par) => Ok(par.clone()),
            None => Err(CasperError::RuntimeError(
                "Return channel not found. This is a compile time error.".to_string(),
            )),
        }
    }
}
