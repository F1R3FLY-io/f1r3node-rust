use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};

use super::kind::TokenKind;
use super::owner::WalletOwner;

pub const WALLET_CHANNEL_DOMAIN: &[u8] = b"f1r3fly/rev-stack/wallet/v1";

const TAG_KIND: u8 = 0x10;

pub fn wallet_channel_id(owner: &WalletOwner) -> Vec<u8> {
    let mut input = Vec::with_capacity(WALLET_CHANNEL_DOMAIN.len() + 64);
    input.extend_from_slice(WALLET_CHANNEL_DOMAIN);
    input.extend_from_slice(&owner.canonical_bytes());
    Blake2b256::hash(input)
}

pub fn wallet_channel_id_of(owner: &WalletOwner, kind: &TokenKind) -> Vec<u8> {
    if kind.is_rev() {
        return wallet_channel_id(owner);
    }
    let name = kind.name();
    let mut input = Vec::with_capacity(WALLET_CHANNEL_DOMAIN.len() + 5 + name.len() + 64);
    input.extend_from_slice(WALLET_CHANNEL_DOMAIN);
    input.push(TAG_KIND);
    input.extend_from_slice(&(name.len() as u32).to_be_bytes());
    input.extend_from_slice(name);
    input.extend_from_slice(&owner.canonical_bytes());
    Blake2b256::hash(input)
}

pub fn wallet_channel(owner: &WalletOwner) -> Par { channel_of(wallet_channel_id(owner)) }

pub fn wallet_channel_of(owner: &WalletOwner, kind: &TokenKind) -> Par {
    channel_of(wallet_channel_id_of(owner, kind))
}

fn channel_of(id: Vec<u8>) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
    }])
}
