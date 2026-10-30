use std::sync::{Arc, Mutex, MutexGuard};

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::hot_store::HotStore;
use rspace_plus_plus::rspace::internal::Datum;

use super::cell::{cell_amount, make_cell};
use super::channel::{wallet_channel, wallet_channel_id};
use super::owner::WalletOwner;
use super::{RevAmount, RevStackError, MAX_AMOUNT};

pub trait CellStore: Send + Sync {
    fn get_data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>>;
    fn put_datum(&self, channel: &Par, datum: Datum<ListParWithRandom>);
    fn remove_datum(&self, channel: &Par, index: i32) -> Result<(), RevStackError>;
}

pub struct HotStoreCells(
    pub Arc<Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>,
);

impl CellStore for HotStoreCells {
    fn get_data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>> { self.0.get_data(channel) }

    fn put_datum(&self, channel: &Par, datum: Datum<ListParWithRandom>) {
        self.0.put_datum(channel, datum)
    }

    fn remove_datum(&self, channel: &Par, index: i32) -> Result<(), RevStackError> {
        self.0
            .remove_datum(channel, index)
            .map_err(|e| RevStackError::Store(e.to_string()))
    }
}

const LOCK_STRIPES: usize = 256;

pub struct RevStacks {
    store: Arc<dyn CellStore>,
    locks: Vec<Mutex<()>>,
}

impl RevStacks {
    pub fn new(store: Arc<dyn CellStore>) -> Self {
        RevStacks {
            store,
            locks: (0..LOCK_STRIPES).map(|_| Mutex::new(())).collect(),
        }
    }

    pub fn over_hot_store(
        store: Arc<Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>,
    ) -> Self {
        Self::new(Arc::new(HotStoreCells(store)))
    }

    pub fn deposit(&self, owner: &WalletOwner, amount: RevAmount) -> Result<(), RevStackError> {
        if amount == 0 {
            return Ok(());
        }
        let (channel, id) = Self::locate(owner);
        let cell = make_cell(&channel, amount)?;
        let _guard = self.lock(&id);
        self.store.put_datum(&channel, cell);
        Ok(())
    }

    pub fn balance(&self, owner: &WalletOwner) -> Result<RevAmount, RevStackError> {
        let (channel, id) = Self::locate(owner);
        let _guard = self.lock(&id);
        self.read_cells(&channel).map(|(_, total)| total)
    }

    pub fn combine(&self, owner: &WalletOwner) -> Result<RevAmount, RevStackError> {
        let (channel, id) = Self::locate(owner);
        let _guard = self.lock(&id);
        let (count, total) = self.read_cells(&channel)?;
        if count > 1 {
            self.rewrite(&channel, count, total)?;
        }
        Ok(total)
    }

    pub fn spend(
        &self,
        owner: &WalletOwner,
        amount: RevAmount,
    ) -> Result<RevAmount, RevStackError> {
        let (channel, id) = Self::locate(owner);
        let _guard = self.lock(&id);
        let (count, total) = self.read_cells(&channel)?;
        let remaining = total
            .checked_sub(amount)
            .ok_or(RevStackError::InsufficientFunds {
                balance: total,
                requested: amount,
            })?;
        if count > 1 || remaining != total {
            self.rewrite(&channel, count, remaining)?;
        }
        Ok(remaining)
    }

    fn locate(owner: &WalletOwner) -> (Par, Vec<u8>) {
        (wallet_channel(owner), wallet_channel_id(owner))
    }

    fn lock(&self, id: &[u8]) -> MutexGuard<'_, ()> {
        let stripe = id.first().copied().unwrap_or(0) as usize % LOCK_STRIPES;
        self.locks[stripe]
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn read_cells(&self, channel: &Par) -> Result<(usize, RevAmount), RevStackError> {
        let data = self.store.get_data(channel);
        let mut total: RevAmount = 0;
        for datum in &data {
            let amount = cell_amount(datum).ok_or_else(|| {
                RevStackError::CorruptWallet(
                    "a datum on the wallet channel is not an amount cell".into(),
                )
            })?;
            total = total
                .checked_add(amount)
                .filter(|t| *t <= MAX_AMOUNT)
                .ok_or(RevStackError::Overflow)?;
        }
        Ok((data.len(), total))
    }

    fn rewrite(&self, channel: &Par, count: usize, amount: RevAmount) -> Result<(), RevStackError> {
        let cell = if amount > 0 {
            Some(make_cell(channel, amount)?)
        } else {
            None
        };
        for index in (0..count).rev() {
            self.store.remove_datum(channel, index as i32)?;
        }
        if let Some(cell) = cell {
            self.store.put_datum(channel, cell);
        }
        Ok(())
    }
}
