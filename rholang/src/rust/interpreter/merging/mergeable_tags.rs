// Source of truth for the genesis-defined mergeable-channel tag identities.
//
// A mergeable tag is an unforgeable name (`Par`) derived deterministically
// from a (deployer-pubkey, timestamp) seed. The same seed values are also
// used to sign the corresponding genesis Rholang contract for those tags
// that are tied to a contract (e.g. NonNegativeNumber.rho), so casper's
// genesis-deploy code re-exports the constants from this module.
//
// Tags:
//   - NonNegativeNumber tag → IntegerAdd merge strategy. Used for vault
//     balance counters and gas accumulators.
//   - BitmaskOr tag → BitmaskOr merge strategy. Used for Registry.rho's
//     TreeHashMap interior-node bitmaps so concurrent registry inserts
//     into the same interior node don't conflict at multi-parent merge.

use std::collections::{BTreeMap, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::casper::DeployDataProto;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use prost::Message;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

pub const NON_NEGATIVE_NUMBER_PK: &str =
    "e33c9f1e925819d04733db4ec8539a84507c9e9abd32822059349449fe03997d";
pub const NON_NEGATIVE_NUMBER_TIMESTAMP: i64 = 1559156251792;

// Dedicated key for deriving the bitmask-OR mergeable tag's unforgeable
// name. Not used to sign any deploy; only seeds the RNG so the tag has
// an identity independent of any specific genesis contract.
pub const BITMASK_OR_TAG_PK: &str =
    "4d76b8e3f29a51c8d05e7b4f9a23c6e1d8b5f0a7c4e91b6d3a8f5c2e9b6d4a1c";
pub const BITMASK_OR_TAG_TIMESTAMP: i64 = 1762000000000;
pub const INTEGER_ADD_MERGEABLE_TAG_URI: &str = "rho:system:integerAddMergeableTag";
pub const BITMASK_OR_MERGEABLE_TAG_URI: &str = "rho:system:bitmaskMergeableTag";

pub fn pub_key_from_hex(priv_key_hex: &str) -> PublicKey {
    let private_key =
        PrivateKey::from_bytes(&hex::decode(priv_key_hex).expect("invalid private key hex"));
    Secp256k1.to_public(&private_key)
}

fn unforgeable_name_rng(deployer: &PublicKey, timestamp: i64) -> Blake2b512Random {
    let seed = DeployDataProto {
        deployer: deployer.bytes.clone(),
        timestamp,
        ..Default::default()
    };
    Blake2b512Random::create_from_bytes(&seed.encode_to_vec())
}

fn tag_name(deployer_pk_hex: &str, timestamp: i64) -> Par {
    let pubkey = pub_key_from_hex(deployer_pk_hex);
    let mut rng = unforgeable_name_rng(&pubkey, timestamp);
    rng.next();
    let unforgeable_byte = rng.next();
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: unforgeable_byte.into_iter().map(|b| b as u8).collect(),
        })),
    }])
}

pub fn non_negative_mergeable_tag_name() -> Par {
    tag_name(NON_NEGATIVE_NUMBER_PK, NON_NEGATIVE_NUMBER_TIMESTAMP)
}

pub fn bitmask_or_mergeable_tag_name() -> Par {
    tag_name(BITMASK_OR_TAG_PK, BITMASK_OR_TAG_TIMESTAMP)
}

/// Standard mergeable-tag registry installed at runtime startup. Maps each
/// genesis-defined tag `Par` to its merge strategy. Use this everywhere a
/// mergeable-tag table is needed unless a test specifically wants a custom
/// configuration.
pub fn default_mergeable_tags() -> HashMap<Par, MergeType> {
    let mut tags = HashMap::new();
    tags.insert(non_negative_mergeable_tag_name(), MergeType::IntegerAdd);
    tags.insert(bitmask_or_mergeable_tag_name(), MergeType::BitmaskOr);
    tags
}

pub fn mergeable_tag_uri_bindings(
    tags: &HashMap<Par, MergeType>,
) -> Result<BTreeMap<&'static str, Par>, &'static str> {
    let mut bindings = BTreeMap::new();
    for (tag, merge_type) in tags {
        let uri = match merge_type {
            MergeType::IntegerAdd => INTEGER_ADD_MERGEABLE_TAG_URI,
            MergeType::BitmaskOr => BITMASK_OR_MERGEABLE_TAG_URI,
        };
        if bindings.insert(uri, tag.clone()).is_some() {
            return Err(uri);
        }
    }
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use proptest::prelude::*;
    use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

    use super::{
        bitmask_or_mergeable_tag_name, default_mergeable_tags, mergeable_tag_uri_bindings,
        non_negative_mergeable_tag_name, BITMASK_OR_MERGEABLE_TAG_URI,
        INTEGER_ADD_MERGEABLE_TAG_URI,
    };

    #[test]
    fn system_uri_bindings_authenticate_exact_merge_types() {
        let tags = default_mergeable_tags();
        let bindings = mergeable_tag_uri_bindings(&tags).expect("valid system merge tags");

        assert_eq!(
            bindings.get(INTEGER_ADD_MERGEABLE_TAG_URI),
            Some(&non_negative_mergeable_tag_name())
        );
        assert_eq!(
            bindings.get(BITMASK_OR_MERGEABLE_TAG_URI),
            Some(&bitmask_or_mergeable_tag_name())
        );
        assert_eq!(bindings.len(), 2);
    }

    #[test]
    fn duplicate_merge_type_binding_is_rejected() {
        let mut tags = HashMap::new();
        tags.insert(non_negative_mergeable_tag_name(), MergeType::IntegerAdd);
        tags.insert(bitmask_or_mergeable_tag_name(), MergeType::IntegerAdd);

        assert_eq!(
            mergeable_tag_uri_bindings(&tags),
            Err(INTEGER_ADD_MERGEABLE_TAG_URI)
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn system_uri_bindings_ignore_registry_insertion_order(integer_first in any::<bool>()) {
            let integer = (non_negative_mergeable_tag_name(), MergeType::IntegerAdd);
            let bitmask = (bitmask_or_mergeable_tag_name(), MergeType::BitmaskOr);
            let mut tags = HashMap::new();
            if integer_first {
                tags.insert(integer.0, integer.1);
                tags.insert(bitmask.0, bitmask.1);
            } else {
                tags.insert(bitmask.0, bitmask.1);
                tags.insert(integer.0, integer.1);
            }

            let bindings = mergeable_tag_uri_bindings(&tags).unwrap();
            prop_assert_eq!(
                bindings.get(INTEGER_ADD_MERGEABLE_TAG_URI),
                Some(&non_negative_mergeable_tag_name())
            );
            prop_assert_eq!(
                bindings.get(BITMASK_OR_MERGEABLE_TAG_URI),
                Some(&bitmask_or_mergeable_tag_name())
            );
        }
    }
}
