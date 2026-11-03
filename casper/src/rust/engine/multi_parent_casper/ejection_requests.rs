//! The ejections this node asks for.
//!
//! Ejecting a validator is one validator's accusation, so it travels as an
//! ordinary deploy the accuser signs and pays for, admitted to this node's own
//! pool like any other deploy.

use std::collections::HashSet;

use comm::rust::transport::transport_layer::TransportLayer;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::validator::Validator;
use num_bigint::BigInt;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use super::block_admission::admit_deploy;
use super::types::MultiParentCasperImpl;
use crate::rust::api::block_api::admission_refusal;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;

/// The target this node asked about last. The requests themselves are read
/// from the pool, which survives a restart.
#[derive(Default)]
pub struct EjectionRequests {
    /// The next request goes to the target after this one, so a target the
    /// contract keeps refusing does not hold the others back.
    last: Option<Validator>,
}

const EJECTION_TERM: &str = r#"
          new rl(`rho:registry:lookup`), poSCh, deployerId(`rho:system:deployerId`), ret in {
            rl!(`rho:system:pos`, *poSCh) |
            for (@(_, PoS) <- poSCh) {
              @PoS!("ejectValidator", *deployerId, "TARGET".hexToBytes(), *ret)
            }
          }
        "#;

fn ejection_term(target: &Validator) -> String {
    EJECTION_TERM.replacen("TARGET", &hex::encode(target), 1)
}

/// The target of a term `ejection_term` built, and nothing for any other
/// term.
fn ejection_target(term: &str) -> Option<Validator> {
    let (prefix, suffix) = EJECTION_TERM
        .split_once("TARGET")
        .expect("the ejection term names its target");
    let target = term.strip_prefix(prefix)?.strip_suffix(suffix)?;
    hex::decode(target).ok().map(Validator::from)
}

pub fn is_ejection_request(term: &str) -> bool { ejection_target(term).is_some() }

const BALANCE_TERM: &str = r#"
  new return, rl(`rho:registry:lookup`), deployerId(`rho:system:deployerId`),
      vaultAddressOps(`rho:vault:address`), addressCh, systemVaultCh in {
    rl!(`rho:vault:system`, *systemVaultCh) |
    vaultAddressOps!("fromDeployerId", *deployerId, *addressCh) |
    for (@address <- addressCh & @(_, systemVault) <- systemVaultCh) {
      new vaultCh in {
        @systemVault!("findOrCreate", address, *vaultCh) |
        for (@(true, vault) <- vaultCh) {
          @vault!("balance", *return)
        }
      }
    }
  }
"#;

fn balance_of(reply: &[Par]) -> Result<BigInt, CasperError> {
    match reply {
        [par] => match par.exprs.as_slice() {
            [models::rhoapi::Expr {
                expr_instance: Some(ExprInstance::GBigInt(bytes)),
            }] => Ok(rholang::rust::interpreter::rho_type::bytes_to_bigint(bytes)),
            [models::rhoapi::Expr {
                expr_instance: Some(ExprInstance::GInt(balance)),
            }] => Ok(BigInt::from(*balance)),
            other => Err(CasperError::RuntimeError(format!(
                "a vault balance is a number, got {other:?}"
            ))),
        },
        other => Err(CasperError::RuntimeError(format!(
            "a vault balance query replies once, got {} replies",
            other.len()
        ))),
    }
}

/// The phlo limit a deploy priced at `price` can carry from a vault holding
/// `balance`. A vault holding more than the largest limit costs covers every
/// limit.
fn covered_limit(balance: &BigInt, price: i64) -> i64 {
    i64::try_from(&(balance / BigInt::from(price))).unwrap_or(i64::MAX)
}

impl<T: TransportLayer + Send + Sync> MultiParentCasperImpl<T> {
    /// Asks for the ejection of the first of `targets` other than itself that
    /// this node has not already asked for and knows no open claim against,
    /// and withdraws a request still waiting in the pool once the target's
    /// claim is open.
    ///
    /// One request waits at a time, committing all this validator's vault
    /// holds as of the post-state of `snapshot`'s main parent, the state the
    /// next block builds on: the cost of an opening is not known in advance,
    /// and requests sharing the vault can each fall short of it and all fail.
    /// The request pays only what its opening costs, the rest is refunded, and
    /// a vault that has fallen since fails the precharge and is charged
    /// nothing.
    pub async fn request_ejections_of(
        &self,
        snapshot: &CasperSnapshot,
        targets: &[Validator],
    ) -> Result<(), CasperError> {
        let Some(identity) = self.validator_id.as_ref() else {
            return Ok(());
        };

        let target: Option<Validator> = {
            let requests = self.ejection_requests.lock();
            let pool = &self.deploy_storage;
            let own = identity.public_key.bytes.as_ref();
            let pooled: Vec<(Validator, Bytes)> = pool
                .read_all()?
                .into_iter()
                .filter(|deploy| deploy.pk.bytes.as_ref() == own)
                .filter_map(|deploy| {
                    ejection_target(&deploy.data.term).map(|target| (target, deploy.sig.clone()))
                })
                .collect();
            let mut unsettled = HashSet::new();
            let mut waiting = false;
            for (target, sig) in pooled {
                let included = self
                    .block_dag_storage
                    .deploy_canonical_appearance(&sig.to_vec())?
                    .is_some();
                if !included && self.bucket_view.ejection_claim_open(&target) {
                    pool.remove_by_sig(&sig)?;
                } else {
                    waiting |= !included;
                    unsettled.insert(target);
                }
            }
            if waiting {
                None
            } else {
                let mut eligible: Vec<&Validator> = targets
                    .iter()
                    .filter(|target| {
                        target.as_ref() != own
                            && !unsettled.contains(*target)
                            && !self.bucket_view.ejection_claim_open(target)
                    })
                    .collect();
                eligible.sort();
                eligible
                    .iter()
                    .find(|target| requests.last.as_ref().is_none_or(|last| **target > last))
                    .or(eligible.first())
                    .map(|target| (*target).clone())
            }
        };
        let Some(target) = target else {
            return Ok(());
        };
        let Some(main_parent) = snapshot.parents.first() else {
            return Ok(());
        };

        let (reply, _) = self
            .runtime_manager
            .play_exploratory_deploy(
                BALANCE_TERM.to_string(),
                &main_parent.body.state.post_state_hash,
                Some(identity.public_key.clone()),
                Vec::new(),
            )
            .await?;
        // The precharge refuses to move nothing, so no deploy runs at a price
        // of nothing.
        let price = self.chain_parameters.min_phlo_price.max(1);
        let phlo_limit = covered_limit(&balance_of(&reply)?, price);
        if phlo_limit <= 0 {
            tracing::debug!(
                target: "f1r3fly.casper.economics",
                "no ejection requested: the vault cannot pay for one"
            );
            return Ok(());
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as i64;
        let shard_id = &self.casper_shard_conf.shard_name;
        let deploy = identity.sign_deploy(DeployData {
            term: ejection_term(&target),
            time_stamp: now,
            phlo_price: price,
            phlo_limit,
            valid_after_block_number: snapshot.max_block_num,
            shard_id: shard_id.clone(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        })?;
        if let Some(refusal) = admission_refusal(
            &deploy,
            shard_id,
            &self.chain_parameters,
            snapshot.max_block_num + 1,
            now,
        )? {
            tracing::warn!(
                target: "f1r3fly.casper.economics",
                target_validator = %hex::encode(&target),
                %refusal,
                "ejection request refused admission"
            );
            return Ok(());
        }
        match admit_deploy(self, deploy)? {
            Either::Right(_) => {
                self.ejection_requests.lock().last = Some(target);
            }
            Either::Left(error) => tracing::warn!(
                target: "f1r3fly.casper.economics",
                target_validator = %hex::encode(&target),
                %error,
                "ejection request not admitted"
            ),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ejection_term_names_its_target_and_no_other_term_does() {
        let target = Validator::from(vec![7u8; 65]);
        assert_eq!(ejection_target(&ejection_term(&target)), Some(target));
        assert_eq!(ejection_target(BALANCE_TERM), None);
    }
}
