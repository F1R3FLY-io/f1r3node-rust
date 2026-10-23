use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::g_unforgeable::UnfInstance;
// Changed by G3 (DR-135): the single-private-name test reads a `Par`.
// use models::rhoapi::{CostSignature, GPrivate};
use models::rhoapi::{CostSignature, GPrivate, Par};
use rholang::rust::interpreter::accounting::authority::{
    canonical_cost_signature, cost_signature_to_sig, AuthorityError,
};
// Added by G3 (DR-135): the owner purses of a signature (R2).
use rholang::rust::interpreter::accounting::owner_funding::{OwnerPurse, OwnerPurseSet};
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultPayer {
    pub signature: CostSignature,
    pub lane_key: [u8; 32],
    pub custody_key: [u8; 32],
    pub address: VaultAddress,
    /// Added by G3 (DR-135): how `vault_payer` located the vault (R2).
    pub kind: VaultPayerKind,
}

/// Added by G3 (DR-135): the kind of vault that `vault_payer` locates (R2). A
/// key wallet and a private-name vault belong to an unforgeable signature,
/// so they can fund an owner. A lane vault belongs to any other signature,
/// which anyone can spell, so its owner has budget 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultPayerKind {
    /// The vault of a valid secp256k1 key, or of a principal of key family 1.
    KeyWallet,
    /// The vault of a name or a quote of exactly one `GPrivate`.
    PrivateName,
    /// The vault keyed by the lane of the signature.
    LaneVault,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum VaultPayerError {
    #[error("unit cost authority has no payable vault")]
    UnitAuthority,
    #[error("invalid cost authority: {0}")]
    InvalidAuthority(#[from] AuthorityError),
}

fn ground_custody_public_key(bytes: &[u8]) -> Option<&[u8]> {
    if PublicKey::validate_secp256k1_bytes(bytes).is_ok() {
        return Some(bytes);
    }
    let (family, encoded) = bytes.split_at_checked(2)?;
    if u16::from_be_bytes(family.try_into().ok()?) != 1 {
        return None;
    }
    let (length, public_key) = encoded.split_at_checked(4)?;
    if usize::try_from(u32::from_be_bytes(length.try_into().ok()?)).ok()? != public_key.len() {
        return None;
    }
    PublicKey::validate_secp256k1_bytes(public_key)
        .is_ok()
        .then_some(public_key)
}

pub fn vault_payer(signature: &CostSignature) -> Result<VaultPayer, VaultPayerError> {
    let signature = canonical_cost_signature(signature)?;
    let runtime_signature = cost_signature_to_sig(&signature)?;
    if runtime_signature == rholang::rust::interpreter::accounting::Sig::Unit {
        return Err(VaultPayerError::UnitAuthority);
    }
    let lane_key = runtime_signature.lane_hash();
    let ground_public_key = signature.value.as_ref().and_then(|value| match value {
        CostSignatureValue::Ground(bytes) => ground_custody_public_key(bytes),
        _ => None,
    });
    // Changed by G3 (DR-135): the match also names the kind of the vault, and
    // the single-private-name test moved to `single_private_name` (R2).
    // let address = match (signature.value.as_ref(), ground_public_key) {
    //     (Some(CostSignatureValue::Ground(_)), Some(public_key)) => {
    //         VaultAddress::from_public_key(&PublicKey::from_bytes(public_key))
    //             .expect("validated secp256k1 public keys have the native vault key length")
    //     }
    //     (Some(CostSignatureValue::Quote(par)), _) | (Some(CostSignatureValue::Name(par)), _) => par
    //         .unforgeables
    //         .first()
    //         .and_then(|unforgeable| unforgeable.unf_instance.as_ref())
    //         .and_then(|unforgeable| match unforgeable {
    //             UnfInstance::GPrivateBody(gprivate)
    //                 if par.sends.is_empty()
    //                     && par.receives.is_empty()
    //                     && par.news.is_empty()
    //                     && par.exprs.is_empty()
    //                     && par.matches.is_empty()
    //                     && par.unforgeables.len() == 1
    //                     && par.bundles.is_empty()
    //                     && par.connectives.is_empty()
    //                     && par.conditionals.is_empty()
    //                     && par.cost_signed_terms.is_empty()
    //                     && par.cost_stacks.is_empty() =>
    //             {
    //                 Some(VaultAddress::from_unforgeable(gprivate))
    //             }
    //             _ => None,
    //         })
    //         .unwrap_or_else(|| {
    //             VaultAddress::from_unforgeable(&GPrivate {
    //                 id: lane_key.to_vec(),
    //             })
    //         }),
    //     _ => VaultAddress::from_unforgeable(&GPrivate {
    //         id: lane_key.to_vec(),
    //     }),
    // };
    let lane_vault = || {
        VaultAddress::from_unforgeable(&GPrivate {
            id: lane_key.to_vec(),
        })
    };
    let (address, kind) = match (signature.value.as_ref(), ground_public_key) {
        (Some(CostSignatureValue::Ground(_)), Some(public_key)) => (
            VaultAddress::from_public_key(&PublicKey::from_bytes(public_key))
                .expect("validated secp256k1 public keys have the native vault key length"),
            VaultPayerKind::KeyWallet,
        ),
        (Some(CostSignatureValue::Quote(par)), _) | (Some(CostSignatureValue::Name(par)), _) => {
            match single_private_name(par) {
                Some(gprivate) => (
                    VaultAddress::from_unforgeable(gprivate),
                    VaultPayerKind::PrivateName,
                ),
                None => (lane_vault(), VaultPayerKind::LaneVault),
            }
        }
        _ => (lane_vault(), VaultPayerKind::LaneVault),
    };
    let mut custody_identity = b"f1r3node:system-vault-custody:v1".to_vec();
    custody_identity.extend_from_slice(address.to_base58().as_bytes());
    let custody_key = Blake2b256::hash(custody_identity)
        .try_into()
        .expect("Blake2b-256 digest length");
    Ok(VaultPayer {
        signature,
        lane_key,
        custody_key,
        address,
        // Added by G3 (DR-135).
        kind,
    })
}

/// Added by G3 (DR-135): the private name of a `Par` that holds exactly one
/// `GPrivate` and nothing else, the structural test that `vault_payer` and
/// `private_name_payer` share (R2). Only the holder of the name can write it,
/// so a signature of it is unforgeable.
pub(crate) fn single_private_name(par: &Par) -> Option<&GPrivate> {
    match par.unforgeables.as_slice() {
        [unforgeable]
            if par.sends.is_empty()
                && par.receives.is_empty()
                && par.news.is_empty()
                && par.exprs.is_empty()
                && par.matches.is_empty()
                && par.bundles.is_empty()
                && par.connectives.is_empty()
                && par.conditionals.is_empty()
                && par.cost_signed_terms.is_empty()
                && par.cost_stacks.is_empty() =>
        {
            match unforgeable.unf_instance.as_ref() {
                Some(UnfInstance::GPrivateBody(gprivate)) => Some(gprivate),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Added by G3 (DR-135): the purses that can fund an owner with `signature`
/// (R2), or `None` when anyone can spell the signature. A key wallet or a
/// private-name vault is one purse. A canonical compound is flat, so its
/// elements are its atoms. When every atom is a key wallet or a private
/// name, the compound has its lane vault for Rule 3 and the atom purses for
/// Rule 2. Any other signature, a compound with a forgeable atom included,
/// has no purse and so budget 0.
pub fn purse_of(signature: &CostSignature) -> Option<OwnerPurseSet> {
    let payer = vault_payer(signature).ok()?;
    match payer.kind {
        VaultPayerKind::KeyWallet | VaultPayerKind::PrivateName => {
            Some(OwnerPurseSet::Single(OwnerPurse {
                custody: payer.custody_key,
                signature: payer.signature,
            }))
        }
        VaultPayerKind::LaneVault => {
            let Some(CostSignatureValue::Compound(compound)) = payer.signature.value.as_ref()
            else {
                return None;
            };
            let mut atoms = Vec::with_capacity(compound.elements.len());
            for element in &compound.elements {
                let atom = vault_payer(element).ok()?;
                match atom.kind {
                    VaultPayerKind::KeyWallet | VaultPayerKind::PrivateName => {
                        atoms.push(OwnerPurse {
                            custody: atom.custody_key,
                            signature: atom.signature,
                        })
                    }
                    VaultPayerKind::LaneVault => return None,
                }
            }
            Some(OwnerPurseSet::Compound {
                vault: OwnerPurse {
                    custody: payer.custody_key,
                    signature: payer.signature,
                },
                atoms: atoms.into(),
            })
        }
    }
}

pub fn balance_query_source(address: &VaultAddress) -> String {
    format!(
        r#"
        new return, rl(`rho:registry:lookup`), systemVaultCh, vaultCh in {{
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {{
            @systemVault!("find", "{}", *vaultCh) |
            for (@result <- vaultCh) {{
              match result {{
                (true, vault) => {{ @vault!("balance", *return) }}
                _ => {{ return!(0) }}
              }}
            }}
          }}
        }}
        "#,
        address.to_base58()
    )
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{CostSignature, GPrivate, GUnforgeable, Par};
    use proptest::prelude::*;
    use rholang::rust::interpreter::accounting::authority::compound_cost_signatures;

    use super::*;

    fn ground(bytes: Vec<u8>) -> CostSignature {
        CostSignature {
            value: Some(CostSignatureValue::Ground(bytes)),
        }
    }

    fn private_name(id: Vec<u8>) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }])
    }

    fn principal_ground(key_family: u16, public_key: &[u8]) -> CostSignature {
        let mut bytes = Vec::with_capacity(6 + public_key.len());
        bytes.extend_from_slice(&key_family.to_be_bytes());
        bytes.extend_from_slice(&(public_key.len() as u32).to_be_bytes());
        bytes.extend_from_slice(public_key);
        ground(bytes)
    }

    fn valid_public_key() -> Vec<u8> {
        hex::decode(
            "04fa70d7be5eb750e0915c0f6d19e7085d18bb1c22d030feb2a877ca2cd226d04438aa819359c56c720142fbc66e9da03a5ab960a3d8b75363a226b7c800f60420",
        )
        .unwrap()
    }

    #[test]
    fn signer_ground_uses_the_existing_public_key_vault() {
        let bytes = valid_public_key();
        let payer = vault_payer(&ground(bytes.clone())).unwrap();
        let expected = VaultAddress::from_public_key(&PublicKey::from_bytes(&bytes)).unwrap();
        assert_eq!(payer.address, expected);
    }

    #[test]
    fn protocol_principal_uses_the_existing_public_key_vault() {
        let bytes = valid_public_key();
        let legacy = vault_payer(&ground(bytes.clone())).unwrap();
        let principal = vault_payer(&principal_ground(1, &bytes)).unwrap();
        assert_eq!(principal.address, legacy.address);
        assert_eq!(principal.custody_key, legacy.custody_key);
        assert_ne!(principal.lane_key, legacy.lane_key);
    }

    #[test]
    fn malformed_protocol_principals_do_not_alias_public_key_custody() {
        let public_key = valid_public_key();
        let expected = vault_payer(&ground(public_key.clone())).unwrap();

        let mut trailing = principal_ground(1, &public_key);
        match trailing.value.as_mut().unwrap() {
            CostSignatureValue::Ground(bytes) => bytes.push(0),
            _ => unreachable!(),
        }
        let mut invalid_key = vec![0; 65];
        invalid_key[0] = 4;

        for signature in [
            principal_ground(2, &public_key),
            principal_ground(1, &invalid_key),
            trailing,
        ] {
            let actual = vault_payer(&signature).unwrap();
            assert_ne!(actual.address, expected.address);
            assert_ne!(actual.custody_key, expected.custody_key);
        }
    }

    proptest! {
        #[test]
        fn unsupported_principal_families_never_alias_native_custody(
            key_family in any::<u16>().prop_filter("family 1 is native custody", |value| *value != 1)
        ) {
            let public_key = valid_public_key();
            let expected = vault_payer(&ground(public_key.clone())).unwrap();
            let actual = vault_payer(&principal_ground(key_family, &public_key)).unwrap();
            prop_assert_ne!(actual.address, expected.address);
            prop_assert_ne!(actual.custody_key, expected.custody_key);
        }

        #[test]
        fn incorrect_principal_lengths_never_alias_native_custody(
            declared_length in (0u32..=130).prop_filter("65 is the canonical key length", |value| *value != 65)
        ) {
            let public_key = valid_public_key();
            let expected = vault_payer(&ground(public_key.clone())).unwrap();
            let mut encoded = Vec::with_capacity(6 + public_key.len());
            encoded.extend_from_slice(&1u16.to_be_bytes());
            encoded.extend_from_slice(&declared_length.to_be_bytes());
            encoded.extend_from_slice(&public_key);
            let actual = vault_payer(&ground(encoded)).unwrap();
            prop_assert_ne!(actual.address, expected.address);
            prop_assert_ne!(actual.custody_key, expected.custody_key);
        }
    }

    #[test]
    fn resolved_funding_slot_uses_its_unforgeable_vault() {
        let gprivate = GPrivate { id: vec![7; 32] };
        let signature = CostSignature {
            value: Some(CostSignatureValue::Name(private_name(gprivate.id.clone()))),
        };
        let payer = vault_payer(&signature).unwrap();
        assert_eq!(payer.address, VaultAddress::from_unforgeable(&gprivate));
    }

    #[test]
    fn unresolved_process_authority_uses_its_lane_vault() {
        let signature = CostSignature {
            value: Some(CostSignatureValue::Quote(Par::default().with_exprs(vec![
                models::rhoapi::Expr {
                    expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(7)),
                },
            ]))),
        };
        let payer = vault_payer(&signature).unwrap();
        assert_eq!(
            payer.address,
            VaultAddress::from_unforgeable(&GPrivate {
                id: payer.lane_key.to_vec(),
            })
        );
    }

    #[test]
    fn compound_payer_is_canonical_and_distinct_from_components() {
        let left = ground(vec![1; 32]);
        let right = ground(vec![2; 32]);
        let compound = compound_cost_signatures(&left, &right).unwrap();
        let payer = vault_payer(&compound).unwrap();
        assert_ne!(payer.address, vault_payer(&left).unwrap().address);
        assert_ne!(payer.address, vault_payer(&right).unwrap().address);
    }

    #[test]
    fn unit_authority_cannot_be_used_as_a_payer() {
        let unit = CostSignature {
            value: Some(CostSignatureValue::Unit(true)),
        };
        assert_eq!(vault_payer(&unit), Err(VaultPayerError::UnitAuthority));
    }

    /// Added by G3 (DR-135): only an unforgeable signature locates an owner
    /// purse (R2). A key wallet and a private name are one purse. A compound
    /// of such atoms has its lane vault and one purse for each atom, in the
    /// order and with the multiplicity of the canonical compound. A signature
    /// that anyone can spell has none: an invalid key, a quoted process, a
    /// name of a `Par` with more than a private name, a compound with such an
    /// atom, and Unit.
    #[test]
    fn owner_purse_of_forgeable_signature_is_none() {
        let key = ground(valid_public_key());
        let private = CostSignature {
            value: Some(CostSignatureValue::Name(private_name(vec![7; 32]))),
        };
        let forged_key = ground(vec![1; 32]);
        let quoted_process = CostSignature {
            value: Some(CostSignatureValue::Quote(Par::default().with_exprs(vec![
                models::rhoapi::Expr {
                    expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(7)),
                },
            ]))),
        };
        let crowded_name = CostSignature {
            value: Some(CostSignatureValue::Name(
                private_name(vec![7; 32]).with_exprs(vec![models::rhoapi::Expr {
                    expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(1)),
                }]),
            )),
        };
        let unit = CostSignature {
            value: Some(CostSignatureValue::Unit(true)),
        };

        let single = |signature: &CostSignature| {
            let payer = vault_payer(signature).unwrap();
            Some(OwnerPurseSet::Single(OwnerPurse {
                custody: payer.custody_key,
                signature: payer.signature,
            }))
        };
        assert_eq!(vault_payer(&key).unwrap().kind, VaultPayerKind::KeyWallet);
        assert_eq!(
            vault_payer(&private).unwrap().kind,
            VaultPayerKind::PrivateName
        );
        assert_eq!(purse_of(&key), single(&key));
        assert_eq!(purse_of(&private), single(&private));

        let compound = compound_cost_signatures(&key, &private).unwrap();
        let Some(OwnerPurseSet::Compound { vault, atoms }) = purse_of(&compound) else {
            panic!("a compound of unforgeable atoms has compound purses");
        };
        let vault_of_compound = vault_payer(&compound).unwrap();
        assert_eq!(vault_of_compound.kind, VaultPayerKind::LaneVault);
        assert_eq!(vault.custody, vault_of_compound.custody_key);
        assert_eq!(vault.signature, vault_of_compound.signature);
        let Some(CostSignatureValue::Compound(elements)) = vault_of_compound.signature.value else {
            panic!("a canonical compound");
        };
        assert_eq!(atoms.len(), elements.elements.len());
        for (atom, element) in atoms.iter().zip(&elements.elements) {
            assert_eq!(Some(OwnerPurseSet::Single(atom.clone())), single(element));
        }
        let doubled = compound_cost_signatures(&key, &key).unwrap();
        let Some(OwnerPurseSet::Compound { atoms, .. }) = purse_of(&doubled) else {
            panic!("a compound of one key twice has compound purses");
        };
        assert_eq!(atoms.len(), 2, "the atoms keep their multiplicity");
        assert_eq!(atoms[0], atoms[1]);

        for (name, forgeable) in [
            ("an invalid key", forged_key.clone()),
            ("a quoted process", quoted_process.clone()),
            ("a name of more than a private name", crowded_name),
            (
                "a compound with an invalid key",
                compound_cost_signatures(&key, &forged_key).unwrap(),
            ),
            (
                "a compound with a quoted process",
                compound_cost_signatures(&private, &quoted_process).unwrap(),
            ),
        ] {
            assert_eq!(
                vault_payer(&forgeable).unwrap().kind,
                VaultPayerKind::LaneVault,
                "{name} has a lane vault"
            );
            assert_eq!(purse_of(&forgeable), None, "{name} has no owner purse");
        }
        assert_eq!(purse_of(&unit), None, "Unit has no owner purse");
    }

    #[test]
    fn balance_query_is_valid_rholang() {
        let payer = vault_payer(&ground(vec![1; 32])).unwrap();
        rholang::rust::interpreter::compiler::compiler::Compiler::source_to_adt(
            &balance_query_source(&payer.address),
        )
        .unwrap();
    }
}
