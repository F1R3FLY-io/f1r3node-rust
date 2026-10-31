use crypto::rust::public_key::PublicKey;
use models::rhoapi::GPrivate;

use super::RevStackError;
use crate::rust::interpreter::util::vault_address::VaultAddress;

const TAG_ADDRESS: u8 = 0x01;
const TAG_UNFORGEABLE: u8 = 0x02;
const TAG_COMPOUND: u8 = 0x03;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum WalletOwner {
    Address(VaultAddress),
    Unforgeable(Vec<u8>),
    Compound(Vec<WalletOwner>),
}

impl WalletOwner {
    pub fn from_public_key(pk: &PublicKey) -> Result<Self, RevStackError> {
        VaultAddress::from_public_key(pk)
            .map(WalletOwner::Address)
            .ok_or_else(|| RevStackError::InvalidOwner("public key has an invalid length".into()))
    }

    pub fn from_deployer_id(deployer_id: &[u8]) -> Result<Self, RevStackError> {
        Self::from_public_key(&PublicKey::from_bytes(deployer_id))
    }

    pub fn from_address(address: &str) -> Result<Self, RevStackError> {
        VaultAddress::parse(address)
            .map(WalletOwner::Address)
            .map_err(RevStackError::InvalidOwner)
    }

    pub fn from_vault_address(address: VaultAddress) -> Self { WalletOwner::Address(address) }

    pub fn from_unforgeable(name: &GPrivate) -> Self { WalletOwner::Unforgeable(name.id.clone()) }

    pub fn compound(components: Vec<WalletOwner>) -> Result<Self, RevStackError> {
        let mut flat = Vec::with_capacity(components.len());
        for c in components {
            match c {
                WalletOwner::Compound(inner) => flat.extend(inner),
                other => flat.push(other),
            }
        }
        match flat.len() {
            0 => Err(RevStackError::InvalidOwner(
                "compound signature with no components".into(),
            )),
            1 => Ok(flat.pop().expect("one component")),
            _ => {
                flat.sort_by_cached_key(|c| c.canonical_bytes());
                Ok(WalletOwner::Compound(flat))
            }
        }
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Option<Self> {
        let (tag, rest) = bytes.split_first()?;
        let owner = match *tag {
            TAG_ADDRESS | TAG_UNFORGEABLE => {
                let len = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?) as usize;
                let body = rest.get(4..4 + len)?;
                if rest.len() != 4 + len {
                    return None;
                }
                if *tag == TAG_ADDRESS {
                    WalletOwner::from_address(std::str::from_utf8(body).ok()?).ok()?
                } else {
                    WalletOwner::Unforgeable(body.to_vec())
                }
            }
            TAG_COMPOUND => {
                let count = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?) as usize;
                let mut at = 4;
                let mut components = Vec::new();
                for _ in 0..count {
                    let len =
                        u32::from_be_bytes(rest.get(at..at + 4)?.try_into().ok()?) as usize;
                    components.push(Self::from_canonical_bytes(
                        rest.get(at + 4..at + 4 + len)?,
                    )?);
                    at += 4 + len;
                }
                if at != rest.len() {
                    return None;
                }
                WalletOwner::compound(components).ok()?
            }
            _ => return None,
        };
        (owner.canonical_bytes() == bytes).then_some(owner)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        match self {
            WalletOwner::Address(address) => {
                let s = address.to_base58();
                let mut out = Vec::with_capacity(1 + 4 + s.len());
                out.push(TAG_ADDRESS);
                push_len_prefixed(&mut out, s.as_bytes());
                out
            }
            WalletOwner::Unforgeable(id) => {
                let mut out = Vec::with_capacity(1 + 4 + id.len());
                out.push(TAG_UNFORGEABLE);
                push_len_prefixed(&mut out, id);
                out
            }
            WalletOwner::Compound(components) => {
                let mut out = vec![TAG_COMPOUND];
                out.extend_from_slice(&(components.len() as u32).to_be_bytes());
                for c in components {
                    push_len_prefixed(&mut out, &c.canonical_bytes());
                }
                out
            }
        }
    }
}

fn push_len_prefixed(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::WalletOwner;

    #[test]
    fn a_compound_owner_survives_its_canonical_bytes() {
        let a = WalletOwner::Unforgeable(vec![1; 32]);
        let b = WalletOwner::Unforgeable(vec![2; 32]);
        let joint = WalletOwner::compound(vec![b.clone(), a.clone()]).unwrap();
        let bytes = joint.canonical_bytes();
        assert_eq!(WalletOwner::from_canonical_bytes(&bytes), Some(joint));
        assert_eq!(WalletOwner::from_canonical_bytes(&bytes[..bytes.len() - 1]), None);
        assert_eq!(
            WalletOwner::from_canonical_bytes(&a.canonical_bytes()),
            Some(a)
        );
    }
}
