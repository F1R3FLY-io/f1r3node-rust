use std::collections::BTreeSet;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, ListParWithRandom, Par};
use prost::Message;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::ops::CellStore;
use super::owner::WalletOwner;
use super::RevStackError;
use crate::rust::interpreter::rho_type::{RhoByteArray, RhoList, RhoNumber, RhoString, RhoTuple2};

pub const AUTHORITY_CHANNEL_DOMAIN: &[u8] = b"f1r3fly/rev-stack/hook-authority/v1";
pub const REV_HOOK_CHANNEL_DOMAIN: &[u8] = b"f1r3fly/rev-stack/rev-hook/v1";

const AUTHORITY_TAG: &str = "hook-authority";
const REV_HOOK_TAG: &str = "rev-hook";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookAuthority {
    members: Vec<WalletOwner>,
    threshold: u32,
}

impl HookAuthority {
    pub fn new(members: Vec<WalletOwner>, threshold: u32) -> Result<Self, RevStackError> {
        let mut seen = BTreeSet::new();
        let mut sorted = Vec::with_capacity(members.len());
        for m in members {
            if matches!(m, WalletOwner::Compound(_)) {
                return Err(RevStackError::InvalidOwner(
                    "a hook authority member must be a single signature".into(),
                ));
            }
            if seen.insert(m.canonical_bytes()) {
                sorted.push(m);
            }
        }
        sorted.sort_by_cached_key(|m| m.canonical_bytes());
        if threshold == 0 || threshold as usize > sorted.len() {
            return Err(RevStackError::InvalidOwner(format!(
                "a hook authority threshold must be in 1..={}, not {threshold}",
                sorted.len()
            )));
        }
        Ok(HookAuthority {
            members: sorted,
            threshold,
        })
    }

    pub fn members(&self) -> &[WalletOwner] { &self.members }

    pub fn threshold(&self) -> u32 { self.threshold }

    pub fn authorizes(&self, signers: &[WalletOwner]) -> bool {
        let members: BTreeSet<Vec<u8>> = self.members.iter().map(|m| m.canonical_bytes()).collect();
        let mut distinct = BTreeSet::new();
        for s in signers {
            let key = s.canonical_bytes();
            if !members.contains(&key) {
                return false;
            }
            distinct.insert(key);
        }
        distinct.len() >= self.threshold as usize
    }

    fn to_par(&self) -> Par {
        RhoTuple2::create_par((
            RhoNumber::create_par(self.threshold as i64),
            RhoList::create_par(
                self.members
                    .iter()
                    .map(|m| RhoByteArray::create_par(m.canonical_bytes()))
                    .collect(),
            ),
        ))
    }

    fn from_par(par: &Par) -> Option<Self> {
        let (threshold, members) = RhoTuple2::unapply(par)?;
        let threshold = u32::try_from(RhoNumber::unapply(&threshold)?).ok()?;
        let members = RhoList::unapply(&members)?
            .iter()
            .map(|p| RhoByteArray::unapply(p).and_then(|b| WalletOwner::from_canonical_bytes(&b)))
            .collect::<Option<Vec<_>>>()?;
        HookAuthority::new(members, threshold).ok()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RevHook {
    Noop,
    Contract(Par),
}

impl RevHook {
    fn to_par(&self) -> Par {
        match self {
            RevHook::Noop => Par::default(),
            RevHook::Contract(p) => p.clone(),
        }
    }

    fn from_par(par: Par) -> Self {
        if par == Par::default() {
            RevHook::Noop
        } else {
            RevHook::Contract(par)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookPoint {
    AfterSettlement,
    EpochBoundary,
}

pub fn authority_channel() -> Par { domain_channel(AUTHORITY_CHANNEL_DOMAIN) }

pub fn rev_hook_channel() -> Par { domain_channel(REV_HOOK_CHANNEL_DOMAIN) }

pub fn install_genesis_hooks(
    store: &dyn CellStore,
    authority: &HookAuthority,
) -> Result<(), RevStackError> {
    if !store.get_data(&authority_channel()).is_empty()
        || !store.get_data(&rev_hook_channel()).is_empty()
    {
        return Err(RevStackError::CorruptWallet(
            "the hook authority is already installed".into(),
        ));
    }
    write_single(
        store,
        &authority_channel(),
        AUTHORITY_TAG,
        authority.to_par(),
    )?;
    write_single(
        store,
        &rev_hook_channel(),
        REV_HOOK_TAG,
        RevHook::Noop.to_par(),
    )
}

pub fn hook_authority(store: &dyn CellStore) -> Result<Option<HookAuthority>, RevStackError> {
    read_single(store, &authority_channel(), AUTHORITY_TAG)?
        .map(|p| {
            HookAuthority::from_par(&p).ok_or_else(|| {
                RevStackError::CorruptWallet("the hook authority datum is malformed".into())
            })
        })
        .transpose()
}

pub fn rev_hook(store: &dyn CellStore) -> Result<Option<RevHook>, RevStackError> {
    Ok(read_single(store, &rev_hook_channel(), REV_HOOK_TAG)?.map(RevHook::from_par))
}

pub fn replace_rev_hook(
    store: &dyn CellStore,
    signers: &[WalletOwner],
    hook: RevHook,
) -> Result<(), RevStackError> {
    let authority = hook_authority(store)?
        .ok_or_else(|| RevStackError::CorruptWallet("no hook authority at genesis".into()))?;
    if !authority.authorizes(signers) {
        return Err(RevStackError::InvalidOwner(format!(
            "replacing a hook needs at least {} of the {} genesis hook addresses",
            authority.threshold(),
            authority.members().len()
        )));
    }
    let channel = rev_hook_channel();
    clear(store, &channel)?;
    write_single(store, &channel, REV_HOOK_TAG, hook.to_par())
}

fn domain_channel(domain: &[u8]) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: Blake2b256::hash(domain.to_vec()),
        })),
    }])
}

fn write_single(
    store: &dyn CellStore,
    channel: &Par,
    tag: &str,
    value: Par,
) -> Result<(), RevStackError> {
    let a = ListParWithRandom {
        pars: vec![RhoTuple2::create_par((
            RhoString::create_par(tag.to_string()),
            RhoByteArray::create_par(value.encode_to_vec()),
        ))],
        random_state: Vec::new(),
    };
    let source = Produce::create(channel, &a, false);
    store.put_datum(
        channel,
        Datum {
            a,
            persist: false,
            source,
        },
    );
    Ok(())
}

fn read_single(
    store: &dyn CellStore,
    channel: &Par,
    tag: &str,
) -> Result<Option<Par>, RevStackError> {
    let data = store.get_data(channel);
    let malformed = || RevStackError::CorruptWallet(format!("the {tag} datum is malformed"));
    match data.as_slice() {
        [] => Ok(None),
        [datum] => {
            let [par] = datum.a.pars.as_slice() else {
                return Err(malformed());
            };
            let (t, value) = RhoTuple2::unapply(par).ok_or_else(malformed)?;
            if RhoString::unapply(&t).as_deref() != Some(tag) || datum.persist {
                return Err(malformed());
            }
            let bytes = RhoByteArray::unapply(&value).ok_or_else(malformed)?;
            Par::decode(bytes.as_slice())
                .map(Some)
                .map_err(|_| malformed())
        }
        _ => Err(RevStackError::CorruptWallet(format!(
            "more than one {tag} datum"
        ))),
    }
}

fn clear(store: &dyn CellStore, channel: &Par) -> Result<(), RevStackError> {
    let n = store.get_data(channel).len();
    for index in (0..n).rev() {
        store.remove_datum(channel, index as i32)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

    use super::*;
    use crate::rust::interpreter::rho_type::RhoNumber;

    #[derive(Default)]
    struct Mem(Mutex<HashMap<Par, Vec<Datum<ListParWithRandom>>>>);

    impl CellStore for Mem {
        fn get_data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>> {
            self.0
                .lock()
                .unwrap()
                .get(channel)
                .cloned()
                .unwrap_or_default()
        }

        fn put_datum(&self, channel: &Par, datum: Datum<ListParWithRandom>) {
            self.0
                .lock()
                .unwrap()
                .entry(channel.clone())
                .or_default()
                .push(datum);
        }

        fn remove_datum(&self, channel: &Par, index: i32) -> Result<(), RevStackError> {
            self.0
                .lock()
                .unwrap()
                .get_mut(channel)
                .filter(|d| (index as usize) < d.len())
                .map(|d| {
                    d.remove(index as usize);
                })
                .ok_or_else(|| RevStackError::Store("no such datum".into()))
        }
    }

    fn keys(n: usize) -> Vec<WalletOwner> {
        (0..n)
            .map(|_| WalletOwner::from_public_key(&Secp256k1.new_key_pair().1).unwrap())
            .collect()
    }

    #[test]
    fn the_authority_needs_a_valid_threshold_and_single_members() {
        let ks = keys(3);
        assert!(HookAuthority::new(ks.clone(), 0).is_err());
        assert!(HookAuthority::new(ks.clone(), 4).is_err());
        let dup = vec![ks[0].clone(), ks[0].clone(), ks[1].clone()];
        assert_eq!(HookAuthority::new(dup, 2).unwrap().members().len(), 2);
        let compound = WalletOwner::compound(ks[..2].to_vec()).unwrap();
        assert!(HookAuthority::new(vec![compound], 1).is_err());
    }

    #[test]
    fn genesis_installs_the_authority_and_a_noop_rev_hook_once() {
        let store = Mem::default();
        let ks = keys(3);
        let authority = HookAuthority::new(ks.clone(), 2).unwrap();
        assert_eq!(hook_authority(&store).unwrap(), None);
        install_genesis_hooks(&store, &authority).unwrap();
        assert_eq!(hook_authority(&store).unwrap(), Some(authority.clone()));
        assert_eq!(rev_hook(&store).unwrap(), Some(RevHook::Noop));
        assert!(install_genesis_hooks(&store, &authority).is_err());
    }

    #[test]
    fn replacing_the_rev_hook_needs_the_threshold_of_genesis_addresses() {
        let store = Mem::default();
        let ks = keys(3);
        let outsider = keys(1).remove(0);
        install_genesis_hooks(&store, &HookAuthority::new(ks.clone(), 2).unwrap()).unwrap();
        let hook = RevHook::Contract(RhoNumber::create_par(7));
        assert!(replace_rev_hook(&store, &ks[..1], hook.clone()).is_err());
        assert!(replace_rev_hook(&store, &[ks[0].clone(), ks[0].clone()], hook.clone()).is_err());
        assert!(replace_rev_hook(&store, &[ks[0].clone(), outsider], hook.clone()).is_err());
        assert_eq!(rev_hook(&store).unwrap(), Some(RevHook::Noop));
        replace_rev_hook(&store, &[ks[2].clone(), ks[0].clone()], hook.clone()).unwrap();
        assert_eq!(rev_hook(&store).unwrap(), Some(hook));
        assert_eq!(store.get_data(&rev_hook_channel()).len(), 1);
        replace_rev_hook(&store, &ks, RevHook::Noop).unwrap();
        assert_eq!(rev_hook(&store).unwrap(), Some(RevHook::Noop));
    }

    #[test]
    fn the_compute_wallet_starts_empty_and_spends_only_under_the_authority() {
        use std::sync::Arc;

        use super::super::compute::{
            compute_balance, compute_wallet_owner, deposit_compute, spend_compute, ComputeSpender,
        };
        use super::super::ops::RevStacks;

        let store: Arc<Mem> = Arc::new(Mem::default());
        let ks = keys(3);
        let payee = keys(1).remove(0);
        install_genesis_hooks(store.as_ref(), &HookAuthority::new(ks.clone(), 2).unwrap()).unwrap();
        assert_eq!(compute_balance(store.clone()).unwrap(), 0);
        deposit_compute(store.clone(), 70).unwrap();
        deposit_compute(store.clone(), 30).unwrap();
        assert_eq!(compute_balance(store.clone()).unwrap(), 100);

        let one = ComputeSpender::Authority(ks[..1].to_vec());
        assert!(spend_compute(store.clone(), &one, &payee, 10).is_err());
        let outsider = ComputeSpender::Authority(vec![ks[0].clone(), payee.clone()]);
        assert!(spend_compute(store.clone(), &outsider, &payee, 10).is_err());
        let two = ComputeSpender::Authority(ks[1..].to_vec());
        assert!(spend_compute(store.clone(), &two, &compute_wallet_owner(), 10).is_err());
        assert!(spend_compute(store.clone(), &two, &payee, 101).is_err());
        assert_eq!(spend_compute(store.clone(), &two, &payee, 40).unwrap(), 60);
        assert_eq!(
            spend_compute(store.clone(), &ComputeSpender::InstalledHook, &payee, 60).unwrap(),
            0
        );
        assert_eq!(RevStacks::new(store.clone()).balance(&payee).unwrap(), 100);
        assert_eq!(compute_balance(store).unwrap(), 0);
    }
}
