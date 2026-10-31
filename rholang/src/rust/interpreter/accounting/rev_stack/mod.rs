pub mod bond;
pub mod cell;
pub mod channel;
pub mod compute;
pub mod genesis;
pub mod hooks;
pub mod journal;
pub mod native;
pub mod ops;
pub mod owner;
pub mod reserve;

#[cfg(test)]
mod tests;

pub use channel::{wallet_channel, wallet_channel_id};
pub use ops::{CellStore, HotStoreCells, RevStacks};
pub use owner::WalletOwner;

pub type RevAmount = u64;

pub const MAX_AMOUNT: RevAmount = i64::MAX as RevAmount;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RevStackError {
    #[error("invalid wallet owner: {0}")]
    InvalidOwner(String),
    #[error("invalid cell amount {0}: must be in 1..=i64::MAX")]
    InvalidAmount(RevAmount),
    #[error("insufficient funds: balance {balance}, requested {requested}")]
    InsufficientFunds {
        balance: RevAmount,
        requested: RevAmount,
    },
    #[error("wallet balance overflow")]
    Overflow,
    #[error("corrupt wallet: {0}")]
    CorruptWallet(String),
    #[error("store error: {0}")]
    Store(String),
}
