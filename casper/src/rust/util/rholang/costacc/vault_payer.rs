use crypto::rust::public_key::PublicKey;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{CostSignature, GPrivate};
use rholang::rust::interpreter::accounting::authority::{
    canonical_cost_signature, cost_signature_to_sig, AuthorityError,
};
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultPayer {
    pub signature: CostSignature,
    pub lane_key: [u8; 32],
    pub address: VaultAddress,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum VaultPayerError {
    #[error("unit cost authority has no payable vault")]
    UnitAuthority,
    #[error("invalid cost authority: {0}")]
    InvalidAuthority(#[from] AuthorityError),
}

pub fn vault_payer(signature: &CostSignature) -> Result<VaultPayer, VaultPayerError> {
    let signature = canonical_cost_signature(signature)?;
    let runtime_signature = cost_signature_to_sig(&signature)?;
    if runtime_signature == rholang::rust::interpreter::accounting::Sig::Unit {
        return Err(VaultPayerError::UnitAuthority);
    }
    let lane_key = runtime_signature.lane_hash();
    let address = match signature.value.as_ref() {
        Some(CostSignatureValue::Ground(bytes))
            if PublicKey::validate_secp256k1_bytes(bytes).is_ok() =>
        {
            VaultAddress::from_public_key(&PublicKey::from_bytes(bytes))
                .expect("validated secp256k1 public keys have the native vault key length")
        }
        Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => par
            .unforgeables
            .first()
            .and_then(|unforgeable| unforgeable.unf_instance.as_ref())
            .and_then(|unforgeable| match unforgeable {
                UnfInstance::GPrivateBody(gprivate)
                    if par.sends.is_empty()
                        && par.receives.is_empty()
                        && par.news.is_empty()
                        && par.exprs.is_empty()
                        && par.matches.is_empty()
                        && par.unforgeables.len() == 1
                        && par.bundles.is_empty()
                        && par.connectives.is_empty()
                        && par.conditionals.is_empty()
                        && par.cost_signed_terms.is_empty()
                        && par.cost_stacks.is_empty() =>
                {
                    Some(VaultAddress::from_unforgeable(gprivate))
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                VaultAddress::from_unforgeable(&GPrivate {
                    id: lane_key.to_vec(),
                })
            }),
        _ => VaultAddress::from_unforgeable(&GPrivate {
            id: lane_key.to_vec(),
        }),
    };
    Ok(VaultPayer {
        signature,
        lane_key,
        address,
    })
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

    #[test]
    fn signer_ground_uses_the_existing_public_key_vault() {
        let bytes = hex::decode(
            "04fa70d7be5eb750e0915c0f6d19e7085d18bb1c22d030feb2a877ca2cd226d04438aa819359c56c720142fbc66e9da03a5ab960a3d8b75363a226b7c800f60420",
        )
        .unwrap();
        let payer = vault_payer(&ground(bytes.clone())).unwrap();
        let expected = VaultAddress::from_public_key(&PublicKey::from_bytes(&bytes)).unwrap();
        assert_eq!(payer.address, expected);
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

    #[test]
    fn balance_query_is_valid_rholang() {
        let payer = vault_payer(&ground(vec![1; 32])).unwrap();
        rholang::rust::interpreter::compiler::compiler::Compiler::source_to_adt(
            &balance_query_source(&payer.address),
        )
        .unwrap();
    }
}
