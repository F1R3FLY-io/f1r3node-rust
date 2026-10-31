use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crypto::rust::signatures::signed::Signed;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::{Cosignature, DeployData};
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::rev_stack::region::{admissible, paying_owners};
use rholang::rust::interpreter::accounting::rev_stack::{
    wallet_channel_id, CellStore, HotStoreCells, RevAmount, RevStacks, WalletOwner,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub type WalletId = Vec<u8>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateInput {
    pub envelope_sig: Bytes,
    pub deployer: WalletOwner,
    pub named: Vec<WalletOwner>,
    pub cosigners: Vec<WalletOwner>,
    pub cosignatures_valid: bool,
    pub phlo_limit: i64,
    pub phlo_price: i64,
}

impl GateInput {
    pub fn from_deploy(
        deploy: &Signed<DeployData>,
        named: Vec<WalletOwner>,
    ) -> Result<Self, CasperError> {
        let deployer = WalletOwner::from_public_key(&deploy.pk)
            .map_err(|e| CasperError::RuntimeError(e.to_string()))?;
        let verified = Cosignature::verified_cosigners(&deploy.data);
        let cosigners = verified
            .iter()
            .flatten()
            .filter_map(|pk| WalletOwner::from_public_key(pk).ok())
            .collect::<Vec<_>>();
        let cosignatures_valid = verified
            .as_ref()
            .is_some_and(|keys| keys.len() == cosigners.len());
        Ok(GateInput {
            envelope_sig: deploy.sig.clone(),
            deployer,
            named,
            cosigners,
            cosignatures_valid,
            phlo_limit: deploy.data.phlo_limit,
            phlo_price: deploy.data.phlo_price,
        })
    }

    pub fn signers(&self) -> Vec<WalletOwner> {
        std::iter::once(self.deployer.clone())
            .chain(self.cosigners.iter().cloned())
            .collect()
    }

    pub fn paying_wallets(&self) -> Vec<WalletOwner> { paying_owners(&self.deployer, &self.named) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    ForeignSignature,
    InvalidTerms,
    Overflow,
    InsufficientFunds {
        wallet: WalletId,
        need: RevAmount,
        have: RevAmount,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Admitted,
    Refused(Refusal),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commitment {
    pub wallet: WalletId,
    pub amount: RevAmount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateEntry {
    pub envelope_sig: Bytes,
    pub delta: u64,
    pub verdict: Verdict,
    pub commitments: Vec<Commitment>,
}

impl GateEntry {
    pub fn admitted(&self) -> bool { self.verdict == Verdict::Admitted }

    pub fn to_proto(&self) -> GateEntryProto {
        let (verdict, wallet, need, have) = match &self.verdict {
            Verdict::Admitted => (VERDICT_ADMITTED, Bytes::new(), 0, 0),
            Verdict::Refused(Refusal::ForeignSignature) => {
                (VERDICT_FOREIGN_SIGNATURE, Bytes::new(), 0, 0)
            }
            Verdict::Refused(Refusal::InvalidTerms) => (VERDICT_INVALID_TERMS, Bytes::new(), 0, 0),
            Verdict::Refused(Refusal::Overflow) => (VERDICT_OVERFLOW, Bytes::new(), 0, 0),
            Verdict::Refused(Refusal::InsufficientFunds { wallet, need, have }) => (
                VERDICT_INSUFFICIENT_FUNDS,
                Bytes::copy_from_slice(wallet),
                *need,
                *have,
            ),
        };
        GateEntryProto {
            envelope_sig: self.envelope_sig.clone(),
            delta: self.delta,
            verdict,
            wallet,
            need,
            have,
            commitments: self
                .commitments
                .iter()
                .map(|c| CommitmentProto {
                    wallet: Bytes::copy_from_slice(&c.wallet),
                    amount: c.amount,
                })
                .collect(),
        }
    }

    pub fn from_proto(proto: GateEntryProto) -> Result<Self, String> {
        let verdict = match proto.verdict {
            VERDICT_ADMITTED => Verdict::Admitted,
            VERDICT_FOREIGN_SIGNATURE => Verdict::Refused(Refusal::ForeignSignature),
            VERDICT_INVALID_TERMS => Verdict::Refused(Refusal::InvalidTerms),
            VERDICT_OVERFLOW => Verdict::Refused(Refusal::Overflow),
            VERDICT_INSUFFICIENT_FUNDS => Verdict::Refused(Refusal::InsufficientFunds {
                wallet: proto.wallet.to_vec(),
                need: proto.need,
                have: proto.have,
            }),
            other => return Err(format!("unknown gate verdict {other}")),
        };
        if verdict != Verdict::Admitted && !proto.commitments.is_empty() {
            return Err("a refused envelope carries no commitment".to_string());
        }
        Ok(GateEntry {
            envelope_sig: proto.envelope_sig,
            delta: proto.delta,
            verdict,
            commitments: proto
                .commitments
                .into_iter()
                .map(|c| Commitment {
                    wallet: c.wallet.to_vec(),
                    amount: c.amount,
                })
                .collect(),
        })
    }
}

const VERDICT_ADMITTED: u32 = 0;
const VERDICT_FOREIGN_SIGNATURE: u32 = 1;
const VERDICT_INVALID_TERMS: u32 = 2;
const VERDICT_OVERFLOW: u32 = 3;
const VERDICT_INSUFFICIENT_FUNDS: u32 = 4;

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CommitmentProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub wallet: Bytes,
    #[prost(uint64, tag = "2")]
    pub amount: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct GateEntryProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub envelope_sig: Bytes,
    #[prost(uint64, tag = "2")]
    pub delta: u64,
    #[prost(uint32, tag = "3")]
    pub verdict: u32,
    #[prost(bytes = "bytes", tag = "4")]
    pub wallet: Bytes,
    #[prost(uint64, tag = "5")]
    pub need: u64,
    #[prost(uint64, tag = "6")]
    pub have: u64,
    #[prost(message, repeated, tag = "7")]
    pub commitments: Vec<CommitmentProto>,
}

pub trait Demand {
    fn delta(&self, input: &GateInput) -> Option<u64>;
}

pub struct PhloLimitDemand;

impl Demand for PhloLimitDemand {
    fn delta(&self, input: &GateInput) -> Option<u64> { u64::try_from(input.phlo_limit).ok() }
}

pub trait WalletReader {
    fn balance(&self, owner: &WalletOwner) -> Result<RevAmount, CasperError>;
}

pub struct StoreReader {
    stacks: RevStacks,
}

impl StoreReader {
    pub fn new(store: Arc<dyn CellStore>) -> Self {
        StoreReader {
            stacks: RevStacks::new(store),
        }
    }

    pub async fn at(
        runtime_manager: &RuntimeManager,
        pre_root: &StateHash,
    ) -> Result<Self, CasperError> {
        let mut runtime = runtime_manager.spawn_runtime().await?;
        runtime
            .reset(&Blake2b256Hash::from_bytes_prost(pre_root))
            .await?;
        let store = runtime.reducer.space.hot_store().ok_or_else(|| {
            CasperError::RuntimeError("the runtime's space has no native REV layer".to_string())
        })?;
        Ok(Self::new(Arc::new(HotStoreCells(store))))
    }
}

impl WalletReader for StoreReader {
    fn balance(&self, owner: &WalletOwner) -> Result<RevAmount, CasperError> {
        self.stacks
            .balance(owner)
            .map_err(|e| CasperError::RuntimeError(e.to_string()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitmentTable {
    committed: BTreeMap<WalletId, RevAmount>,
}

impl CommitmentTable {
    pub fn committed(&self, wallet: &[u8]) -> RevAmount {
        self.committed.get(wallet).copied().unwrap_or(0)
    }

    pub fn wallets(&self) -> impl Iterator<Item = (&WalletId, &RevAmount)> { self.committed.iter() }

    fn add(&mut self, commitments: &[Commitment]) {
        for c in commitments {
            *self.committed.entry(c.wallet.clone()).or_insert(0) += c.amount;
        }
    }
}

pub struct Gate<'a, R: WalletReader, D: Demand> {
    reader: &'a R,
    demand: &'a D,
    balances: HashMap<WalletId, RevAmount>,
    table: CommitmentTable,
    entries: Vec<GateEntry>,
}

impl<'a, R: WalletReader, D: Demand> Gate<'a, R, D> {
    pub fn new(reader: &'a R, demand: &'a D) -> Self {
        Gate {
            reader,
            demand,
            balances: HashMap::new(),
            table: CommitmentTable::default(),
            entries: Vec::new(),
        }
    }

    pub fn admit(&mut self, input: &GateInput) -> Result<&GateEntry, CasperError> {
        let entry = self.decide(input)?;
        if entry.admitted() {
            self.table.add(&entry.commitments);
        }
        self.entries.push(entry);
        Ok(self.entries.last().expect("an entry was pushed"))
    }

    pub fn finish(self) -> GateOutcome {
        GateOutcome {
            entries: self.entries,
            table: self.table,
        }
    }

    fn decide(&mut self, input: &GateInput) -> Result<GateEntry, CasperError> {
        let refuse = |delta: u64, refusal: Refusal| GateEntry {
            envelope_sig: input.envelope_sig.clone(),
            delta,
            verdict: Verdict::Refused(refusal),
            commitments: Vec::new(),
        };
        if !input.cosignatures_valid {
            return Ok(refuse(0, Refusal::InvalidTerms));
        }
        let signers = input.signers();
        if input.named.iter().any(|s| !admissible(s, &signers)) {
            return Ok(refuse(0, Refusal::ForeignSignature));
        }
        let Ok(price) = u64::try_from(input.phlo_price) else {
            return Ok(refuse(0, Refusal::InvalidTerms));
        };
        let Some(delta) = self.demand.delta(input) else {
            return Ok(refuse(0, Refusal::InvalidTerms));
        };
        if i64::try_from(delta).map_or(true, |d| d > input.phlo_limit) {
            return Ok(refuse(delta, Refusal::InvalidTerms));
        }
        let Some(need) = delta.checked_mul(price) else {
            return Ok(refuse(delta, Refusal::Overflow));
        };
        let mut commitments = Vec::new();
        for owner in input.paying_wallets() {
            let wallet = wallet_channel_id(&owner);
            let balance = self.balance(&wallet, &owner)?;
            let have = balance.saturating_sub(self.table.committed(&wallet));
            if have < need {
                return Ok(refuse(
                    delta,
                    Refusal::InsufficientFunds { wallet, need, have },
                ));
            }
            if need > 0 {
                commitments.push(Commitment {
                    wallet,
                    amount: need,
                });
            }
        }
        Ok(GateEntry {
            envelope_sig: input.envelope_sig.clone(),
            delta,
            verdict: Verdict::Admitted,
            commitments,
        })
    }

    fn balance(
        &mut self,
        wallet: &WalletId,
        owner: &WalletOwner,
    ) -> Result<RevAmount, CasperError> {
        if let Some(b) = self.balances.get(wallet) {
            return Ok(*b);
        }
        let b = self.reader.balance(owner)?;
        self.balances.insert(wallet.clone(), b);
        Ok(b)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GateOutcome {
    pub entries: Vec<GateEntry>,
    pub table: CommitmentTable,
}

impl GateOutcome {
    pub fn admitted(&self) -> Vec<bool> { self.entries.iter().map(GateEntry::admitted).collect() }

    pub fn committed_through(&self, position: usize, wallet: &[u8]) -> RevAmount {
        self.entries
            .iter()
            .take(position.saturating_add(1))
            .flat_map(|e| e.commitments.iter())
            .filter(|c| c.wallet == wallet)
            .map(|c| c.amount)
            .sum()
    }

    pub fn visible_balance(
        &self,
        pre_balance: RevAmount,
        position: usize,
        wallet: &[u8],
    ) -> RevAmount {
        pre_balance.saturating_sub(self.committed_through(position, wallet))
    }
}

pub fn gate_block<R: WalletReader, D: Demand>(
    reader: &R,
    demand: &D,
    inputs: &[GateInput],
) -> Result<GateOutcome, CasperError> {
    let mut gate = Gate::new(reader, demand);
    for input in inputs {
        gate.admit(input)?;
    }
    Ok(gate.finish())
}

pub fn check_recorded<R: WalletReader, D: Demand>(
    reader: &R,
    demand: &D,
    inputs: &[GateInput],
    recorded: &[GateEntry],
) -> Result<GateOutcome, String> {
    let outcome = gate_block(reader, demand, inputs).map_err(|e| e.to_string())?;
    if outcome.entries.len() != recorded.len() {
        return Err(format!(
            "the record holds {} gate entries for {} envelopes",
            recorded.len(),
            outcome.entries.len()
        ));
    }
    for (i, (got, want)) in outcome.entries.iter().zip(recorded).enumerate() {
        if got != want {
            return Err(format!("gate entry {i} differs from the record"));
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use proptest::prelude::*;

    use super::*;

    struct MapReader(HashMap<WalletId, RevAmount>);

    impl WalletReader for MapReader {
        fn balance(&self, owner: &WalletOwner) -> Result<RevAmount, CasperError> {
            Ok(self.0.get(&wallet_channel_id(owner)).copied().unwrap_or(0))
        }
    }

    fn owners(n: usize) -> Vec<WalletOwner> {
        (0..n)
            .map(|_| WalletOwner::from_public_key(&Secp256k1.new_key_pair().1).unwrap())
            .collect()
    }

    fn input(deployer: &WalletOwner, limit: i64, price: i64, i: usize) -> GateInput {
        GateInput {
            envelope_sig: Bytes::from(i.to_be_bytes().to_vec()),
            deployer: deployer.clone(),
            named: Vec::new(),
            phlo_limit: limit,
            phlo_price: price,
            cosignatures_valid: true,
            cosigners: Vec::new(),
        }
    }

    fn reader(owners: &[WalletOwner], balances: &[RevAmount]) -> MapReader {
        MapReader(
            owners
                .iter()
                .zip(balances)
                .map(|(o, b)| (wallet_channel_id(o), *b))
                .collect(),
        )
    }

    #[test]
    fn later_envelopes_see_earlier_commitments() {
        let os = owners(1);
        let r = reader(&os, &[1_000]);
        let inputs: Vec<_> = (0..3).map(|i| input(&os[0], 100, 4, i)).collect();
        let out = gate_block(&r, &PhloLimitDemand, &inputs).unwrap();
        assert_eq!(out.admitted(), vec![true, true, false]);
        let w = wallet_channel_id(&os[0]);
        assert_eq!(out.table.committed(&w), 800);
        assert_eq!(
            out.entries[2].verdict,
            Verdict::Refused(Refusal::InsufficientFunds {
                wallet: w.clone(),
                need: 400,
                have: 200
            })
        );
        assert_eq!(out.visible_balance(1_000, 0, &w), 600);
        assert_eq!(out.visible_balance(1_000, 2, &w), 200);
    }

    #[test]
    fn a_term_naming_another_signature_is_refused() {
        let os = owners(2);
        let r = reader(&os, &[1_000, 1_000]);
        let mut foreign = input(&os[0], 10, 1, 0);
        foreign.named = vec![os[0].clone(), os[1].clone()];
        let mut own = input(&os[0], 10, 1, 1);
        own.named = vec![os[0].clone()];
        let out = gate_block(&r, &PhloLimitDemand, &[foreign, own]).unwrap();
        assert_eq!(
            out.entries[0].verdict,
            Verdict::Refused(Refusal::ForeignSignature)
        );
        assert!(out.entries[0].commitments.is_empty());
        assert!(out.entries[1].admitted());
    }

    #[test]
    fn invalid_terms_and_overflow_are_refused() {
        let os = owners(1);
        let r = reader(&os, &[u64::MAX >> 1]);
        let out = gate_block(
            &r,
            &PhloLimitDemand,
            &[
                input(&os[0], -1, 1, 0),
                input(&os[0], 1, -1, 1),
                input(&os[0], i64::MAX, i64::MAX, 2),
                input(&os[0], 5, 0, 3),
            ],
        )
        .unwrap();
        assert_eq!(
            out.entries[0].verdict,
            Verdict::Refused(Refusal::InvalidTerms)
        );
        assert_eq!(
            out.entries[1].verdict,
            Verdict::Refused(Refusal::InvalidTerms)
        );
        assert_eq!(out.entries[2].verdict, Verdict::Refused(Refusal::Overflow));
        assert!(out.entries[3].admitted());
        assert!(out.entries[3].commitments.is_empty());
    }

    struct Fixed(u64);

    impl Demand for Fixed {
        fn delta(&self, _: &GateInput) -> Option<u64> { Some(self.0) }
    }

    #[test]
    fn a_demand_above_the_signed_limit_is_refused() {
        let os = owners(1);
        let r = reader(&os, &[1_000]);
        let out = gate_block(&r, &Fixed(11), &[input(&os[0], 10, 1, 0)]).unwrap();
        assert_eq!(
            out.entries[0].verdict,
            Verdict::Refused(Refusal::InvalidTerms)
        );
        let out = gate_block(&r, &Fixed(3), &[input(&os[0], 10, 2, 0)]).unwrap();
        assert_eq!(out.entries[0].commitments[0].amount, 6);
    }

    #[test]
    fn replay_checks_the_recorded_entries() {
        let os = owners(1);
        let r = reader(&os, &[500]);
        let inputs: Vec<_> = (0..2).map(|i| input(&os[0], 100, 3, i)).collect();
        let out = gate_block(&r, &PhloLimitDemand, &inputs).unwrap();
        assert!(check_recorded(&r, &PhloLimitDemand, &inputs, &out.entries).is_ok());
        let mut forged = out.entries.clone();
        forged[1].verdict = Verdict::Admitted;
        forged[1].commitments = vec![Commitment {
            wallet: wallet_channel_id(&os[0]),
            amount: 300,
        }];
        assert!(check_recorded(&r, &PhloLimitDemand, &inputs, &forged).is_err());
        assert!(check_recorded(&r, &PhloLimitDemand, &inputs, &out.entries[..1]).is_err());
    }

    #[test]
    fn a_refused_entry_with_a_commitment_does_not_decode() {
        let os = owners(1);
        let r = reader(&os, &[500]);
        let inputs: Vec<_> = (0..2).map(|i| input(&os[0], 100, 3, i)).collect();
        let out = gate_block(&r, &PhloLimitDemand, &inputs).unwrap();
        for e in &out.entries {
            assert_eq!(GateEntry::from_proto(e.to_proto()).unwrap(), *e);
        }
        let mut bad = out.entries[1].to_proto();
        bad.commitments = out.entries[0].to_proto().commitments;
        assert!(GateEntry::from_proto(bad).is_err());
    }

    proptest! {
        #[test]
        fn commitments_never_exceed_a_balance(
            balances in proptest::collection::vec(0u64..10_000, 1..5),
            envs in proptest::collection::vec((0usize..5, -5i64..300, -2i64..50), 0..40),
        ) {
            let os = owners(balances.len());
            let r = reader(&os, &balances);
            let inputs: Vec<_> = envs
                .iter()
                .enumerate()
                .map(|(i, (w, l, p))| input(&os[w % os.len()], *l, *p, i))
                .collect();
            let out = gate_block(&r, &PhloLimitDemand, &inputs).unwrap();
            prop_assert_eq!(out.entries.len(), inputs.len());
            for (o, b) in os.iter().zip(&balances) {
                let w = wallet_channel_id(o);
                prop_assert!(out.table.committed(&w) <= *b);
                let mut running = 0u64;
                for (i, e) in out.entries.iter().enumerate() {
                    running += e.commitments.iter().filter(|c| c.wallet == w).map(|c| c.amount).sum::<u64>();
                    prop_assert!(running <= *b);
                    prop_assert_eq!(out.committed_through(i, &w), running);
                    prop_assert_eq!(out.visible_balance(*b, i, &w), b - running);
                }
                prop_assert_eq!(running, out.table.committed(&w));
            }
            for (e, inp) in out.entries.iter().zip(&inputs) {
                if e.admitted() {
                    let need = (inp.phlo_limit as u64) * (inp.phlo_price as u64);
                    let total: u64 = e.commitments.iter().map(|c| c.amount).sum();
                    prop_assert_eq!(total, need);
                } else {
                    prop_assert!(e.commitments.is_empty());
                }
            }
            prop_assert_eq!(gate_block(&r, &PhloLimitDemand, &inputs).unwrap(), out.clone());
            prop_assert!(check_recorded(&r, &PhloLimitDemand, &inputs, &out.entries).is_ok());
        }
    }
}
