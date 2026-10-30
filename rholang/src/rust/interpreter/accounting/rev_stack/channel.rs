use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};

use super::owner::WalletOwner;

pub const WALLET_CHANNEL_DOMAIN: &[u8] = b"f1r3fly/rev-stack/wallet/v1";

pub fn wallet_channel_id(owner: &WalletOwner) -> Vec<u8> {
    let mut input = Vec::with_capacity(WALLET_CHANNEL_DOMAIN.len() + 64);
    input.extend_from_slice(WALLET_CHANNEL_DOMAIN);
    input.extend_from_slice(&owner.canonical_bytes());
    Blake2b256::hash(input)
}

pub fn wallet_channel(owner: &WalletOwner) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: wallet_channel_id(owner),
        })),
    }])
}
