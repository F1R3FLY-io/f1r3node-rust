pub mod check_balance;
pub mod close_block_deploy;
pub mod direct_wallet_funding;
pub mod genesis_resource_policy;
pub mod monetary_cursor;
pub mod prepaid_receipts;
pub mod redeem_deploy;
pub mod slash_deploy;
pub mod vault_cost_deploy;
pub mod vault_payer;

pub const VALIDATOR_HANDLER_COST_PER_DEPLOY: i64 = 3;
