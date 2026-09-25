use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeRef};

use super::*;

#[derive(Clone, Copy)]
pub(super) enum RuntimeDeployRef<'a> {
    Body(&'a Cosigned<DeployData>),
    Retained(&'a DeployEnvelope),
}

impl<'a> RuntimeDeployRef<'a> {
    pub(super) fn body(self) -> &'a DeployData {
        match self {
            Self::Body(deploy) => &deploy.data,
            Self::Retained(deploy) => deploy.body(),
        }
    }

    pub(super) fn metadata(self) -> SystemProcessDeployData {
        match self {
            Self::Body(deploy) => SystemProcessDeployData::from_cosigned(deploy),
            Self::Retained(deploy) => SystemProcessDeployData::from_envelope(deploy),
        }
    }

    pub(super) fn normalizer_env(self) -> HashMap<String, Par> {
        match self {
            Self::Body(deploy) => {
                models::rust::normalizer_env::normalizer_env_from_cosigned_deploy(deploy)
            }
            Self::Retained(deploy) => {
                models::rust::normalizer_env::normalizer_env_from_envelope(deploy)
            }
        }
    }

    pub(super) fn rng(self) -> Blake2b512Random {
        match self {
            Self::Body(deploy) => Tools::user_deploy_rng(deploy),
            Self::Retained(deploy) => Tools::user_envelope_rng(deploy),
        }
    }

    pub(super) fn install_authority(self, cost: &accounting::RuntimeBudget) {
        match self {
            Self::Body(deploy) => install_body_authority(deploy, cost),
            Self::Retained(deploy) => {
                let funding = match deploy.view() {
                    DeployEnvelopeRef::Legacy(body) => {
                        install_body_authority(body, cost);
                        return;
                    }
                    DeployEnvelopeRef::BodyV61(body) => accounting::funding_sig(body),
                    DeployEnvelopeRef::Funded(funded) => accounting::funding_sig(funded),
                    DeployEnvelopeRef::OfferedFunded(offered) => accounting::funding_sig(offered),
                };
                let identity = deploy
                    .identity()
                    .as_bytes()
                    .try_into()
                    .expect("checked protocol-v6 deploy identity width");
                cost.set_deploy_id_funded(identity, funding);
            }
        }
    }
}

fn install_body_authority(deploy: &Cosigned<DeployData>, cost: &accounting::RuntimeBudget) {
    let funding = accounting::funding_sig(deploy);
    if deploy.is_envelope_bound() {
        let identity = deploy
            .envelope_commitment()
            .expect("validated protocol-v6 envelope identity")
            .as_ref()
            .try_into()
            .expect("protocol-v6 deploy identity width");
        cost.set_deploy_id_funded(identity, funding);
    } else if deploy.is_compound() {
        let signatures: Vec<&[u8]> = deploy
            .signers()
            .iter()
            .map(|signer| signer.sig.as_ref())
            .collect();
        cost.set_deploy_signatures_funded(&signatures, funding);
    } else {
        cost.set_deploy_signature_funded(&deploy.primary().sig, funding);
    }
}
