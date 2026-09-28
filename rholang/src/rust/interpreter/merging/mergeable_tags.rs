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

use std::collections::HashMap;

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

// Seed for the bitmask-OR mergeable tag's unforgeable name. It has the form
// of a secp256k1 private key because the tag derivation goes through a public
// key, but it signs nothing. Its only use is to seed the RNG, so the tag has
// an identity independent of any specific genesis contract.
pub const BITMASK_OR_TAG_SEED: &str =
    "4d76b8e3f29a51c8d05e7b4f9a23c6e1d8b5f0a7c4e91b6d3a8f5c2e9b6d4a1c";
pub const BITMASK_OR_TAG_TIMESTAMP: i64 = 1762000000000;

pub fn pub_key_from_hex(priv_key_hex: &str) -> PublicKey {
    let private_key =
        PrivateKey::from_bytes(&hex::decode(priv_key_hex).expect("invalid private key hex"));
    Secp256k1.to_public(&private_key)
}

pub fn unforgeable_name_rng(deployer: &PublicKey, timestamp: i64) -> Blake2b512Random {
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
    // The tag must equal `MergeableTag` from NonNegativeNumber.rho, the second
    // name its `new` draws from this seed (the first is `NonNegativeNumber`).
    // The BitmaskOr tag uses the same derivation; changing it changes the tag
    // bytes pinned in the tests below.
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
    tag_name(BITMASK_OR_TAG_SEED, BITMASK_OR_TAG_TIMESTAMP)
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

/// The tag bound to `rho:system:bitmaskMergeableTag`. Registry.rho reads that single
/// URI, so a second BitmaskOr tag is refused rather than left to map iteration order.
pub fn bitmask_or_tag(tags: &HashMap<Par, MergeType>) -> Option<&Par> {
    let mut found = tags
        .iter()
        .filter(|(_, merge_type)| **merge_type == MergeType::BitmaskOr)
        .map(|(tag, _)| tag);
    let tag = found.next();
    assert!(
        found.next().is_none(),
        "at most one BitmaskOr mergeable tag is supported: rho:system:bitmaskMergeableTag binds a single tag"
    );
    tag
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_id_hex(tag: &Par) -> String {
        match tag.unforgeables.as_slice() {
            [GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id, .. })),
                ..
            }] => hex::encode(id),
            other => panic!("a merge tag must be exactly one GPrivate, got {other:?}"),
        }
    }

    #[test]
    fn merge_tag_identities_are_pinned() {
        assert_eq!(
            private_id_hex(&non_negative_mergeable_tag_name()),
            "78a2588671230884044c801f5f9675defb420460d6895b809ee8cd6f6cfff5d3"
        );
        assert_eq!(
            private_id_hex(&bitmask_or_mergeable_tag_name()),
            "9902f19c7886266265b763d2d9c648e62aa6c31fc25c685be3c8416c8b86e52a"
        );
    }

    #[test]
    fn default_registry_maps_each_tag_to_its_strategy() {
        let tags = default_mergeable_tags();
        assert_eq!(tags.len(), 2);
        assert_eq!(
            tags.get(&non_negative_mergeable_tag_name()),
            Some(&MergeType::IntegerAdd)
        );
        assert_eq!(
            tags.get(&bitmask_or_mergeable_tag_name()),
            Some(&MergeType::BitmaskOr)
        );
    }

    #[test]
    fn the_default_registry_binds_its_bitmask_tag() {
        assert_eq!(
            bitmask_or_tag(&default_mergeable_tags()),
            Some(&bitmask_or_mergeable_tag_name())
        );
    }

    #[test]
    #[should_panic(expected = "at most one BitmaskOr mergeable tag")]
    fn a_second_bitmask_tag_is_refused() {
        let mut tags = default_mergeable_tags();
        tags.insert(Par::default(), MergeType::BitmaskOr);
        bitmask_or_tag(&tags);
    }
}
