// See models/src/main/scala/coop/rchain/models/NormalizerEnv.scala

use std::collections::HashMap;

use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::{Cosigned, Signed};

use super::casper::protocol::casper_message::DeployData;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{
    EList, ETuple, Expr, GAuthorityId, GDeployId, GDeployerId, GPrincipalId, GUnforgeable, Par,
};

const SYSTEM_DEPLOY_ID_URI: &str = "rho:system:deployId";
const SYSTEM_DEPLOYER_ID_URI: &str = "rho:system:deployerId";
/// Multi-signer introspection channel. Always a `List[DeployerId]` (one or
/// more elements). For legacy single-signer deploys the list contains exactly
/// the primary signer's `DeployerId`. For multi-sig deploys (decoded from
/// `DeployDataProto.cosigners`), the list contains every signer's `DeployerId`
/// in canonical ascending `pk.bytes` order.
const SYSTEM_COSIGNERS_URI: &str = "rho:system:cosigners";
const SYSTEM_AUTHORITY_ID_URI: &str = "rho:system:authorityId";
const SYSTEM_SIGNERS_URI: &str = "rho:system:signers";
const SYSTEM_POLICY_MEMBERS_URI: &str = "rho:system:policyMembers";
const SYSTEM_AUTHORITY_THRESHOLD_URI: &str = "rho:system:authorityThreshold";
const LEGACY_DEPLOY_ID_URI: &str = "rho:rchain:deployId";
const LEGACY_DEPLOYER_ID_URI: &str = "rho:rchain:deployerId";

fn insert_legacy_alias(
    env: &mut HashMap<String, Par>,
    legacy_uri: &'static str,
    canonical_uri: &'static str,
    value: Par,
) {
    tracing::debug!(
        target: "f1r3fly.models.legacy_uri",
        "Resolved legacy URI alias `{}` via canonical `{}`",
        legacy_uri,
        canonical_uri
    );
    env.insert(legacy_uri.to_string(), value);
}

pub fn with_deployer_id(deployer_pk: &PublicKey) -> HashMap<String, Par> {
    let mut env = HashMap::new();
    let deployer_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: deployer_pk.bytes.to_vec(),
        })),
    }]);

    env.insert(SYSTEM_DEPLOYER_ID_URI.to_string(), deployer_id_par.clone());
    // Backward-compatible alias used by external clients.
    insert_legacy_alias(
        &mut env,
        LEGACY_DEPLOYER_ID_URI,
        SYSTEM_DEPLOYER_ID_URI,
        deployer_id_par,
    );
    env
}

/// Build a Rholang `List[DeployerId]` Par from a slice of public keys.
/// Used to populate `rho:system:cosigners` for both legacy single-sig and
/// multi-sig deploys. The list order is the input order; callers are
/// responsible for canonicalization (Cosigned::from_signed_data sorts).
fn build_cosigners_list_par(pks: &[&PublicKey]) -> Par {
    let elements: Vec<Par> = pks
        .iter()
        .map(|pk| {
            Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
                    public_key: pk.bytes.to_vec(),
                })),
            }])
        })
        .collect();
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: elements,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

pub fn normalizer_env_from_deploy(deploy: &Signed<DeployData>) -> HashMap<String, Par> {
    let mut env = HashMap::new();

    let deploy_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
            sig: deploy.sig.to_vec(),
        })),
    }]);

    let deployer_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
            public_key: deploy.pk.bytes.to_vec(),
        })),
    }]);

    env.insert(SYSTEM_DEPLOY_ID_URI.to_string(), deploy_id_par.clone());
    // Backward-compatible alias used by external clients.
    insert_legacy_alias(
        &mut env,
        LEGACY_DEPLOY_ID_URI,
        SYSTEM_DEPLOY_ID_URI,
        deploy_id_par,
    );

    env.insert(SYSTEM_DEPLOYER_ID_URI.to_string(), deployer_id_par.clone());
    // Backward-compatible alias used by external clients.
    insert_legacy_alias(
        &mut env,
        LEGACY_DEPLOYER_ID_URI,
        SYSTEM_DEPLOYER_ID_URI,
        deployer_id_par,
    );

    // For legacy single-sig deploys the cosigners list contains exactly the
    // primary signer. In-deploy Rholang code reading `rho:system:cosigners`
    // gets a uniform `List[DeployerId]` shape across single and multi-sig
    // deploys — for legacy deploys the list has one element.
    let cosigners_par = build_cosigners_list_par(&[&deploy.pk]);
    env.insert(SYSTEM_COSIGNERS_URI.to_string(), cosigners_par);

    env
}

/// Multi-signer variant of [`normalizer_env_from_deploy`] taking a fully
/// validated [`Cosigned<DeployData>`] envelope (one or more cosigners,
/// canonically ordered by pk.bytes). Exposes:
///
/// - `rho:system:deployId` / `rho:rchain:deployId` — built from the
///   PRIMARY signer's signature (back-compat).
/// - `rho:system:deployerId` / `rho:rchain:deployerId` — built from the
///   PRIMARY signer's public key (back-compat — Rholang code that only
///   knows about a single deployer sees the primary).
/// - `rho:system:cosigners` — full `List[DeployerId]` of every signer in
///   canonical order. This is the channel Rholang programs use to introspect
///   the full cosigner set.
pub fn normalizer_env_from_cosigned_deploy(deploy: &Cosigned<DeployData>) -> HashMap<String, Par> {
    if !deploy.is_envelope_bound() {
        let mut env = normalizer_env_from_deploy(&deploy.as_legacy_signed_ref());
        let signers = deploy
            .signers()
            .iter()
            .map(|signer| &signer.pk)
            .collect::<Vec<_>>();
        env.insert(
            SYSTEM_COSIGNERS_URI.to_string(),
            build_cosigners_list_par(&signers),
        );
        return env;
    }

    let mut env = HashMap::new();
    let deploy_id = deploy
        .envelope_commitment()
        .expect("validated protocol-v6 envelope identity");
    let selected = deploy
        .selected_signers_v61()
        .expect("validated protocol-v6 selected signers");

    let deploy_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
            sig: deploy_id.to_vec(),
        })),
    }]);

    env.insert(SYSTEM_DEPLOY_ID_URI.to_string(), deploy_id_par.clone());
    insert_legacy_alias(
        &mut env,
        LEGACY_DEPLOY_ID_URI,
        SYSTEM_DEPLOY_ID_URI,
        deploy_id_par,
    );

    let authority_id = authority_id_v61(&selected);
    env.insert(
        SYSTEM_AUTHORITY_ID_URI.to_string(),
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GAuthorityIdBody(GAuthorityId {
                id: authority_id,
            })),
        }]),
    );

    env.insert(
        SYSTEM_SIGNERS_URI.to_string(),
        principal_descriptors(&selected),
    );
    let policy_members: Vec<_> = deploy.signers().iter().collect();
    env.insert(
        SYSTEM_POLICY_MEMBERS_URI.to_string(),
        principal_descriptors(&policy_members),
    );
    env.insert(
        SYSTEM_AUTHORITY_THRESHOLD_URI.to_string(),
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(i64::from(deploy.cosigner_threshold()))),
        }]),
    );

    if let [signer] = selected.as_slice() {
        let deployer_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrincipalIdBody(GPrincipalId {
                key_family: 1,
                public_key: signer.pk.bytes.to_vec(),
            })),
        }]);
        env.insert(SYSTEM_DEPLOYER_ID_URI.to_string(), deployer_id_par.clone());
        insert_legacy_alias(
            &mut env,
            LEGACY_DEPLOYER_ID_URI,
            SYSTEM_DEPLOYER_ID_URI,
            deployer_id_par,
        );
    }

    env
}

pub fn normalizer_env_from_v61_single_signer(
    deploy_id: &[u8],
    public_key: &PublicKey,
) -> HashMap<String, Par> {
    let mut env = HashMap::new();
    let deploy_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
            sig: deploy_id.to_vec(),
        })),
    }]);
    env.insert(SYSTEM_DEPLOY_ID_URI.to_string(), deploy_id_par.clone());
    insert_legacy_alias(
        &mut env,
        LEGACY_DEPLOY_ID_URI,
        SYSTEM_DEPLOY_ID_URI,
        deploy_id_par,
    );

    env.insert(
        SYSTEM_AUTHORITY_ID_URI.to_string(),
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GAuthorityIdBody(GAuthorityId {
                id: authority_id_v61_from_public_keys(&[public_key]),
            })),
        }]),
    );

    let principal = principal_descriptor(1, &public_key.bytes);
    let principals = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![principal],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }]);
    env.insert(SYSTEM_SIGNERS_URI.to_string(), principals.clone());
    env.insert(SYSTEM_POLICY_MEMBERS_URI.to_string(), principals);
    env.insert(
        SYSTEM_AUTHORITY_THRESHOLD_URI.to_string(),
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        }]),
    );

    let deployer_id_par = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrincipalIdBody(GPrincipalId {
            key_family: 1,
            public_key: public_key.bytes.to_vec(),
        })),
    }]);
    env.insert(SYSTEM_DEPLOYER_ID_URI.to_string(), deployer_id_par.clone());
    insert_legacy_alias(
        &mut env,
        LEGACY_DEPLOYER_ID_URI,
        SYSTEM_DEPLOYER_ID_URI,
        deployer_id_par,
    );
    env
}

fn principal_descriptor(scheme: u16, public_key: &[u8]) -> Par {
    let scheme = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(i64::from(scheme))),
    }]);
    let key = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(public_key.to_vec())),
    }]);
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::ETupleBody(ETuple {
            ps: vec![scheme, key],
            locally_free: Vec::new(),
            connective_used: false,
        })),
    }])
}

fn principal_descriptors(signers: &[&crypto::rust::signatures::signed::Cosigner]) -> Par {
    let ps = signers
        .iter()
        .map(|signer| {
            let scheme = signer
                .scheme_id_v61()
                .expect("validated protocol-v6 signer scheme");
            principal_descriptor(scheme, &signer.pk.bytes)
        })
        .collect();
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

pub fn authority_id_v61(signers: &[&crypto::rust::signatures::signed::Cosigner]) -> Vec<u8> {
    let public_keys = signers.iter().map(|signer| &signer.pk).collect::<Vec<_>>();
    authority_id_v61_from_public_keys(&public_keys)
}

fn authority_id_v61_from_public_keys(signers: &[&PublicKey]) -> Vec<u8> {
    let mut preimage = Vec::new();
    append_lp64(&mut preimage, b"f1r3fly:rholang:compound-authority:v6.1");
    preimage.extend_from_slice(&(signers.len() as u32).to_be_bytes());
    for signer in signers {
        let mut ground = Vec::new();
        ground.extend_from_slice(&1u16.to_be_bytes());
        ground.extend_from_slice(&(signer.bytes.len() as u32).to_be_bytes());
        ground.extend_from_slice(&signer.bytes);
        append_lp64(&mut preimage, &ground);
    }
    crypto::rust::hash::blake2b256::Blake2b256::hash(preimage)
}

fn append_lp64(target: &mut Vec<u8>, value: &[u8]) {
    target.extend_from_slice(&(value.len() as u64).to_be_bytes());
    target.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::public_key::PublicKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use crypto::rust::signatures::signed::Cosigner;
    use prost::bytes::Bytes;

    use super::*;

    fn signed_deploy_fixture() -> Signed<DeployData> {
        Signed {
            data: DeployData {
                term: "Nil".to_string(),
                language: "rholang".to_string(),
                time_stamp: 1,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                authority_presentations: Vec::new(),
            },
            pk: PublicKey::from_bytes(&[1, 2, 3, 4]),
            sig: Bytes::from(vec![5, 6, 7, 8]),
            sig_algorithm: Box::new(Secp256k1),
        }
    }

    fn deploy_data_fixture() -> DeployData {
        DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        }
    }

    fn threshold_envelope(selected: &[usize], threshold: u32) -> Cosigned<DeployData> {
        let secp = Secp256k1;
        let mut members = (0..3)
            .map(|_| {
                let (private_key, public_key) = secp.new_key_pair();
                (
                    Cosigner {
                        pk: public_key,
                        sig: Bytes::new(),
                        sig_algorithm: Box::new(secp.clone()),
                    },
                    private_key,
                )
            })
            .collect::<Vec<(Cosigner, PrivateKey)>>();
        members.sort_by_key(|(signer, _)| signer.principal_bytes_v61().unwrap());
        let mut bitmap = vec![0u8; members.len().div_ceil(8)];
        for index in selected {
            bitmap[index / 8] |= 1 << (index % 8);
        }
        let unsigned = members
            .iter()
            .map(|(signer, _)| signer.clone())
            .collect::<Vec<_>>();
        let data = deploy_data_fixture();
        for index in selected {
            let hash = Cosigned::<DeployData>::envelope_signing_hash_for_presence(
                &data,
                &unsigned,
                threshold,
                &bitmap,
                &members[*index].0.sig_algorithm.name(),
            )
            .unwrap();
            members[*index].0.sig = members[*index]
                .0
                .sig_algorithm
                .sign(&hash, &members[*index].1.bytes)
                .into();
        }
        Cosigned::from_envelope_signed_data_threshold(
            data,
            members.into_iter().map(|(signer, _)| signer).collect(),
            threshold,
        )
        .unwrap()
    }

    #[test]
    fn with_deployer_id_should_include_legacy_alias() {
        let deployer_pk = PublicKey::from_bytes(&[10, 11, 12, 13]);
        let env = with_deployer_id(&deployer_pk);

        let system = env
            .get(SYSTEM_DEPLOYER_ID_URI)
            .expect("Missing rho:system:deployerId");
        let legacy = env
            .get(LEGACY_DEPLOYER_ID_URI)
            .expect("Missing rho:rchain:deployerId");

        assert_eq!(system, legacy);
    }

    #[test]
    fn normalizer_env_from_deploy_should_include_legacy_aliases() {
        let deploy = signed_deploy_fixture();
        let env = normalizer_env_from_deploy(&deploy);

        let system_deploy_id = env
            .get(SYSTEM_DEPLOY_ID_URI)
            .expect("Missing rho:system:deployId");
        let legacy_deploy_id = env
            .get(LEGACY_DEPLOY_ID_URI)
            .expect("Missing rho:rchain:deployId");

        let system_deployer_id = env
            .get(SYSTEM_DEPLOYER_ID_URI)
            .expect("Missing rho:system:deployerId");
        let legacy_deployer_id = env
            .get(LEGACY_DEPLOYER_ID_URI)
            .expect("Missing rho:rchain:deployerId");

        assert_eq!(system_deploy_id, legacy_deploy_id);
        assert_eq!(system_deployer_id, legacy_deployer_id);
    }

    #[test]
    fn normalizer_env_from_deploy_includes_single_element_cosigners_list() {
        let deploy = signed_deploy_fixture();
        let env = normalizer_env_from_deploy(&deploy);

        let cosigners_par = env
            .get(SYSTEM_COSIGNERS_URI)
            .expect("Missing rho:system:cosigners");

        // Must be exactly one EList expr, with one Par element holding the
        // primary GDeployerId.
        assert_eq!(cosigners_par.exprs.len(), 1);
        let elist = match &cosigners_par.exprs[0].expr_instance {
            Some(ExprInstance::EListBody(elist)) => elist,
            other => panic!("expected EList expr_instance, got {:?}", other),
        };
        assert_eq!(elist.ps.len(), 1);
        let inner_par = &elist.ps[0];
        assert_eq!(inner_par.unforgeables.len(), 1);
        let deployer_id = match &inner_par.unforgeables[0].unf_instance {
            Some(UnfInstance::GDeployerIdBody(d)) => d,
            other => panic!("expected GDeployerIdBody, got {:?}", other),
        };
        assert_eq!(deployer_id.public_key, deploy.pk.bytes.to_vec());
    }

    #[test]
    fn normalizer_env_from_cosigned_deploy_exposes_full_cosigners_list() {
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        use crypto::rust::signatures::signed::{Cosigner, ToMessage};
        use prost::Message;

        // Two distinct keypairs. Sign the same canonical message hash.
        let secp = Secp256k1;
        let (sk1, pk1) = secp.new_key_pair();
        let (sk2, pk2) = secp.new_key_pair();
        let data = DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let serialized = data.to_message().encode_to_vec();
        let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
        let sig1 = secp.sign(&hash, &sk1.bytes);
        let sig2 = secp.sign(&hash, &sk2.bytes);
        let signer1 = Cosigner {
            pk: pk1.clone(),
            sig: Bytes::from(sig1),
            sig_algorithm: Box::new(secp.clone()),
        };
        let signer2 = Cosigner {
            pk: pk2.clone(),
            sig: Bytes::from(sig2),
            sig_algorithm: Box::new(secp.clone()),
        };
        let cosigned = Cosigned::from_signed_data(data, vec![signer1, signer2]).expect("valid");
        let env = normalizer_env_from_cosigned_deploy(&cosigned);

        let cosigners_par = env
            .get(SYSTEM_COSIGNERS_URI)
            .expect("Missing rho:system:cosigners");
        let elist = match &cosigners_par.exprs[0].expr_instance {
            Some(ExprInstance::EListBody(elist)) => elist,
            other => panic!("expected EList, got {:?}", other),
        };
        assert_eq!(elist.ps.len(), 2);

        // Canonical order: pk.bytes ascending. Extract the deployer ids and
        // verify the ordering matches Cosigned's internal canonicalization.
        let recovered_pks: Vec<Vec<u8>> = elist
            .ps
            .iter()
            .map(|p| match &p.unforgeables[0].unf_instance {
                Some(UnfInstance::GDeployerIdBody(d)) => d.public_key.clone(),
                _ => panic!("expected GDeployerIdBody"),
            })
            .collect();
        let expected_order: Vec<Vec<u8>> = cosigned
            .signers()
            .iter()
            .map(|c| c.pk.bytes.to_vec())
            .collect();
        assert_eq!(recovered_pks, expected_order);

        // Primary's id matches `rho:system:deployerId`.
        let system_deployer_id = env
            .get(SYSTEM_DEPLOYER_ID_URI)
            .expect("Missing rho:system:deployerId");
        let primary_pk_in_env = match &system_deployer_id.unforgeables[0].unf_instance {
            Some(UnfInstance::GDeployerIdBody(d)) => d.public_key.clone(),
            _ => panic!("expected GDeployerIdBody"),
        };
        assert_eq!(primary_pk_in_env, cosigned.primary().pk.bytes.to_vec());
    }

    #[test]
    fn cosigned_envelope_legacy_uplift_yields_single_element_cosigners() {
        // The single-signer collapse path should produce an env identical in
        // shape (single-element cosigners list, primary deployer == sole signer)
        // to what `normalizer_env_from_deploy` produces for the same Signed.
        let deploy = signed_deploy_fixture();
        let cosigned = Cosigned::from_single_signer(deploy.clone()).expect("legacy uplift");

        let env_legacy = normalizer_env_from_deploy(&deploy);
        let env_cosigned = normalizer_env_from_cosigned_deploy(&cosigned);

        for key in [
            SYSTEM_DEPLOY_ID_URI,
            SYSTEM_DEPLOYER_ID_URI,
            SYSTEM_COSIGNERS_URI,
        ] {
            assert_eq!(
                env_legacy.get(key),
                env_cosigned.get(key),
                "legacy uplift must preserve channel {}",
                key
            );
        }
    }

    #[test]
    fn v61_multi_signer_environment_exposes_only_collective_authority() {
        let envelope = threshold_envelope(&[1, 2], 2);
        let unsigned = envelope.signers()[0].pk.clone();
        let env = normalizer_env_from_cosigned_deploy(&envelope);

        assert!(env.contains_key(SYSTEM_DEPLOY_ID_URI));
        assert!(env.contains_key(SYSTEM_AUTHORITY_ID_URI));
        assert!(env.contains_key(SYSTEM_SIGNERS_URI));
        assert!(env.contains_key(SYSTEM_POLICY_MEMBERS_URI));
        assert!(!env.contains_key(SYSTEM_DEPLOYER_ID_URI));
        assert!(!env.contains_key(LEGACY_DEPLOYER_ID_URI));
        assert!(!env.contains_key(SYSTEM_COSIGNERS_URI));

        let selected = match &env[SYSTEM_SIGNERS_URI].exprs[0].expr_instance {
            Some(ExprInstance::EListBody(list)) => list,
            other => panic!("expected signer list, got {other:?}"),
        };
        assert_eq!(selected.ps.len(), 2);
        assert!(selected.ps.iter().all(|entry| {
            let Some(ExprInstance::ETupleBody(tuple)) = &entry.exprs[0].expr_instance else {
                return false;
            };
            let Some(ExprInstance::GByteArray(key)) = &tuple.ps[1].exprs[0].expr_instance else {
                return false;
            };
            key.as_slice() != unsigned.bytes.as_ref()
        }));
    }

    #[test]
    fn v61_single_signer_environment_exposes_principal_authority() {
        let secp = Secp256k1;
        let (private_key, _) = secp.new_key_pair();
        let envelope =
            Cosigned::create_single_envelope(deploy_data_fixture(), Box::new(secp), private_key)
                .unwrap();
        let env = normalizer_env_from_cosigned_deploy(&envelope);
        let principal = &env[SYSTEM_DEPLOYER_ID_URI].unforgeables[0].unf_instance;

        assert!(matches!(principal, Some(UnfInstance::GPrincipalIdBody(_))));
        assert_eq!(
            env[SYSTEM_DEPLOY_ID_URI].unforgeables[0].unf_instance,
            Some(UnfInstance::GDeployIdBody(GDeployId {
                sig: envelope.envelope_commitment().unwrap().to_vec(),
            }))
        );
        assert_eq!(
            env,
            normalizer_env_from_v61_single_signer(
                &envelope.envelope_commitment().unwrap(),
                &envelope.primary().pk,
            )
        );
    }
}
