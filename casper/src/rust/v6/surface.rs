use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::comm_ledger::Surface;
use rholang::rust::interpreter::accounting::membrane::{Refusal, RefusalKind};
use rholang::rust::interpreter::accounting::rev_stack::channel::wallet_channel_id;
use rholang::rust::interpreter::accounting::rev_stack::seal::{purse_of, Funding, SealRecord};
use rholang::rust::interpreter::accounting::rev_stack::{RevAmount, WalletOwner};

use super::gate::{CommitmentTable, WalletReader};

pub type Surfaces = Vec<(Option<Surface>, u64)>;

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SurfaceCellsProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub owner: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub envelope: Bytes,
    #[prost(uint64, tag = "3")]
    pub phlo_price: u64,
    #[prost(uint64, tag = "4")]
    pub cells: u64,
}

pub fn surface_purse(surface: &Surface) -> Result<Vec<WalletOwner>, String> {
    let owner = WalletOwner::from_canonical_bytes(&surface.owner)
        .ok_or_else(|| "a surface names no wallet".to_string())?;
    Ok(purse_of(&owner))
}

pub fn is_own(surface: &Option<Surface>, envelope: &[u8]) -> bool {
    surface
        .as_ref()
        .is_none_or(|s| s.envelope.as_slice() == envelope)
}

pub fn own_cells(surfaces: &Surfaces, envelope: &[u8]) -> u64 {
    surfaces
        .iter()
        .filter(|(s, _)| is_own(s, envelope))
        .map(|(_, cells)| *cells)
        .sum()
}

pub fn foreign<'a>(
    surfaces: &'a Surfaces,
    envelope: &'a [u8],
) -> impl Iterator<Item = (&'a Surface, u64)> + 'a {
    surfaces.iter().filter_map(move |(s, cells)| match s {
        Some(s) if s.envelope.as_slice() != envelope => Some((s, *cells)),
        _ => None,
    })
}

pub fn to_proto(surfaces: &Surfaces) -> Vec<SurfaceCellsProto> {
    surfaces
        .iter()
        .map(|(s, cells)| match s {
            Some(s) => SurfaceCellsProto {
                owner: Bytes::from(s.owner.clone()),
                envelope: Bytes::from(s.envelope.clone()),
                phlo_price: s.phlo_price,
                cells: *cells,
            },
            None => SurfaceCellsProto {
                owner: Bytes::new(),
                envelope: Bytes::new(),
                phlo_price: 0,
                cells: *cells,
            },
        })
        .collect()
}

pub fn from_proto(cells: Vec<SurfaceCellsProto>) -> Result<Surfaces, String> {
    let surfaces: Surfaces = cells
        .into_iter()
        .map(|c| {
            if c.cells == 0 {
                return Err("a surface tally counts at least one cell".to_string());
            }
            if c.owner.is_empty() {
                if !c.envelope.is_empty() || c.phlo_price != 0 {
                    return Err("the client's surface names no origin".to_string());
                }
                return Ok((None, c.cells));
            }
            WalletOwner::from_canonical_bytes(&c.owner)
                .ok_or_else(|| "a surface names no wallet".to_string())?;
            Ok((
                Some(Surface {
                    owner: c.owner.to_vec(),
                    envelope: c.envelope.to_vec(),
                    phlo_price: c.phlo_price,
                }),
                c.cells,
            ))
        })
        .collect::<Result<_, _>>()?;
    if surfaces.windows(2).any(|w| w[0].0 >= w[1].0) {
        return Err("surface tallies are sorted and distinct".to_string());
    }
    Ok(surfaces)
}

pub struct BlockPurses {
    reader: Box<dyn WalletReader + Send + Sync>,
    held: CommitmentTable,
    balances: Mutex<HashMap<Vec<u8>, Option<RevAmount>>>,
}

impl BlockPurses {
    pub fn new(reader: Box<dyn WalletReader + Send + Sync>, held: CommitmentTable) -> Self {
        BlockPurses {
            reader,
            held,
            balances: Mutex::new(HashMap::new()),
        }
    }

    pub fn free(&self, purse: &WalletOwner) -> Option<RevAmount> {
        let wallet = wallet_channel_id(purse);
        let balance = {
            let mut balances = self.balances.lock().ok()?;
            *balances
                .entry(wallet.clone())
                .or_insert_with(|| self.reader.balance(purse).ok())
        }?;
        balance.checked_sub(self.held.committed(&wallet))
    }

    pub fn available(&self, surface: &Surface) -> Option<u64> {
        let price = surface.phlo_price.max(1);
        surface_purse(surface)
            .ok()?
            .iter()
            .map(|purse| self.free(purse).map(|free| free / price))
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .min()
    }

    pub fn funds_cell(&self, record: &SealRecord) -> bool {
        record
            .purse()
            .iter()
            .all(|purse| self.free(purse).is_some_and(|free| free >= record.price()))
    }
}

pub struct EnvelopeFunding {
    purses: Arc<BlockPurses>,
    committed: Vec<WalletOwner>,
}

impl EnvelopeFunding {
    pub fn new(purses: Arc<BlockPurses>, committed: Vec<WalletOwner>) -> Arc<dyn Funding> {
        Arc::new(EnvelopeFunding { purses, committed })
    }
}

impl Funding for EnvelopeFunding {
    fn gates(&self, record: &SealRecord) -> bool { !self.committed.contains(&record.owner) }

    fn limit(&self, wallet: &WalletOwner) -> RevAmount { self.purses.free(wallet).unwrap_or(0) }
}

pub struct CloseFunding {
    purses: Arc<BlockPurses>,
    drawn: BTreeMap<Vec<u8>, RevAmount>,
}

impl CloseFunding {
    pub fn new<'a>(
        purses: Arc<BlockPurses>,
        runs: impl IntoIterator<Item = &'a BTreeMap<Vec<u8>, RevAmount>>,
    ) -> Arc<dyn Funding> {
        let mut drawn: BTreeMap<Vec<u8>, RevAmount> = BTreeMap::new();
        for run in runs {
            for (wallet, amount) in run {
                let d = drawn.entry(wallet.clone()).or_insert(0);
                *d = d.saturating_add(*amount);
            }
        }
        Arc::new(CloseFunding { purses, drawn })
    }
}

impl Funding for CloseFunding {
    fn gates(&self, _record: &SealRecord) -> bool { true }

    fn limit(&self, wallet: &WalletOwner) -> RevAmount {
        let free = self.purses.free(wallet).unwrap_or(0);
        free.saturating_sub(
            self.drawn
                .get(&wallet.canonical_bytes())
                .copied()
                .unwrap_or(0),
        )
    }
}

pub fn accept_metered(
    limit: &dyn Fn(&WalletOwner) -> RevAmount,
    logged: &[Refusal],
    drawn: &BTreeMap<Vec<u8>, RevAmount>,
    reached: &BTreeMap<Vec<u8>, RevAmount>,
) -> Result<(), String> {
    let mut steps = BTreeSet::new();
    let mut stops: BTreeMap<Vec<u8>, (RevAmount, bool)> = BTreeMap::new();
    for refusal in logged {
        if !steps.insert(refusal.step.clone()) {
            return Err("two refusals name one step".to_string());
        }
        if refusal.kind == RefusalKind::Charge {
            match reached.get(&refusal.step) {
                None => return Err("a refused step was not reached".to_string()),
                Some(charge) if *charge != refusal.charge => {
                    return Err("a refused step's charge is forged".to_string())
                }
                Some(_) => {}
            }
        }
        let owner = WalletOwner::from_canonical_bytes(&refusal.surface.owner)
            .ok_or_else(|| "a refusal names no wallet".to_string())?;
        let wallet = purse_of(&owner)
            .into_iter()
            .find(|wallet| {
                let limit = limit(wallet);
                let worked = drawn.get(&wallet.canonical_bytes()).copied().unwrap_or(0);
                refusal.drawn <= limit && refusal.drawn == worked
            })
            .ok_or_else(|| "a refusal's drawn amount is not the wallet's total".to_string())?;
        let limit = limit(&wallet);
        let unpayable = limit - refusal.drawn < refusal.charge;
        let stop = stops
            .entry(wallet.canonical_bytes())
            .or_insert((refusal.drawn, false));
        if stop.0 != refusal.drawn {
            return Err("the refusals of one wallet carry different drawn amounts".to_string());
        }
        stop.1 |= unpayable;
    }
    if stops.values().any(|(_, unpayable)| !unpayable) {
        return Err("a membrane stopped at a step that it could pay".to_string());
    }
    for (wallet, worked) in drawn {
        if stops.contains_key(wallet) {
            continue;
        }
        let owner = WalletOwner::from_canonical_bytes(wallet)
            .ok_or_else(|| "a drawn amount names no wallet".to_string())?;
        if *worked > limit(&owner) {
            return Err("work under a membrane exceeds what its wallet can pay".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(n: u8) -> Vec<u8> { WalletOwner::Unforgeable(vec![n; 32]).canonical_bytes() }

    fn surface(n: u8, envelope: u8) -> Option<Surface> {
        Some(Surface {
            owner: owner(n),
            envelope: vec![envelope],
            phlo_price: 3,
        })
    }

    #[test]
    fn own_and_foreign_surfaces_split_by_origin() {
        let tally: Surfaces = vec![(None, 2), (surface(1, 7), 3), (surface(2, 8), 4)];
        assert_eq!(own_cells(&tally, &[7]), 5);
        let foreign: Vec<_> = foreign(&tally, &[7]).map(|(s, c)| (s.clone(), c)).collect();
        assert_eq!(foreign, vec![(surface(2, 8).unwrap(), 4)]);
    }

    #[test]
    fn the_encoding_is_canonical() {
        let tally: Surfaces = vec![(None, 2), (surface(1, 7), 3)];
        assert_eq!(from_proto(to_proto(&tally)), Ok(tally.clone()));
        let mut reversed = to_proto(&tally);
        reversed.reverse();
        assert!(from_proto(reversed).is_err());
        let mut empty = to_proto(&tally);
        empty[1].cells = 0;
        assert!(from_proto(empty).is_err());
    }

    struct Balances(Vec<(WalletOwner, RevAmount)>);

    impl WalletReader for Balances {
        fn balance(
            &self,
            owner: &WalletOwner,
        ) -> Result<RevAmount, crate::rust::errors::CasperError> {
            Ok(self
                .0
                .iter()
                .find(|(o, _)| o == owner)
                .map_or(0, |(_, b)| *b))
        }
    }

    fn stored(owner: WalletOwner, envelope: u8, price: u64) -> SealRecord {
        use rholang::rust::interpreter::accounting::rev_stack::seal::{Origin, SealKind};
        SealRecord {
            owner,
            kind: SealKind::Process,
            origin: Origin {
                envelope: vec![envelope; 64],
                signers: Vec::new(),
                phlo_price: price,
            },
        }
    }

    #[test]
    fn a_stored_receive_is_funded_when_its_purse_less_the_blocks_holds_covers_one_cell() {
        let (alice, bob) = (
            WalletOwner::Unforgeable(vec![1; 32]),
            WalletOwner::Unforgeable(vec![2; 32]),
        );
        let reader = Balances(vec![(alice.clone(), 10), (bob.clone(), 3)]);
        let held =
            CommitmentTable::from_committed(BTreeMap::from([(wallet_channel_id(&alice), 7)]));
        let purses = Arc::new(BlockPurses::new(Box::new(reader), held));
        assert!(purses.funds_cell(&stored(alice.clone(), 1, 3)));
        assert!(!purses.funds_cell(&stored(alice.clone(), 1, 4)));
        assert!(purses.funds_cell(&stored(bob.clone(), 1, 3)));
        assert!(!purses.funds_cell(&stored(bob.clone(), 1, 4)));
        assert!(purses.funds_cell(&stored(WalletOwner::Unforgeable(vec![3; 32]), 1, 0)));
        assert!(!purses.funds_cell(&stored(WalletOwner::Unforgeable(vec![3; 32]), 1, 1)));
        let pair = WalletOwner::Compound(vec![alice.clone(), bob.clone()]);
        assert!(purses.funds_cell(&stored(pair.clone(), 1, 3)));
        assert!(!purses.funds_cell(&stored(pair.clone(), 1, 4)));
        let own = EnvelopeFunding::new(purses.clone(), vec![bob.clone()]);
        assert!(!own.gates(&stored(bob.clone(), 1, 9)));
        assert!(own.gates(&stored(alice.clone(), 1, 9)));
        assert_eq!(own.limit(&alice), 3);
        assert_eq!(own.limit(&bob), 3);
    }

    #[test]
    fn the_close_gates_every_stored_record_against_what_the_runs_left() {
        let (alice, bob) = (
            WalletOwner::Unforgeable(vec![1; 32]),
            WalletOwner::Unforgeable(vec![2; 32]),
        );
        let reader = Balances(vec![(alice.clone(), 10), (bob.clone(), 3)]);
        let held =
            CommitmentTable::from_committed(BTreeMap::from([(wallet_channel_id(&alice), 7)]));
        let purses = Arc::new(BlockPurses::new(Box::new(reader), held));
        let runs = [
            BTreeMap::from([(alice.canonical_bytes(), 1)]),
            BTreeMap::from([(alice.canonical_bytes(), 1), (bob.canonical_bytes(), 5)]),
        ];
        let close = CloseFunding::new(purses, runs.iter());
        assert!(close.gates(&stored(alice.clone(), 1, 1)));
        assert_eq!(close.limit(&alice), 1);
        assert_eq!(close.limit(&bob), 0);
    }

    #[test]
    fn a_compound_purse_is_each_of_its_components() {
        let pair = WalletOwner::Compound(vec![
            WalletOwner::Unforgeable(vec![1; 32]),
            WalletOwner::Unforgeable(vec![2; 32]),
        ]);
        let s = Surface {
            owner: pair.canonical_bytes(),
            envelope: vec![1],
            phlo_price: 1,
        };
        assert_eq!(surface_purse(&s).unwrap().len(), 2);
    }
}
