use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::rev_stack::compute::compute_wallet_owner;
use rholang::rust::interpreter::accounting::rev_stack::{wallet_channel_id, RevAmount};

use super::block_record::BlockRecord;
use super::gate::WalletId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    CommCharge,
    ComputeCharge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevTransfer {
    pub envelope_id: Bytes,
    pub from: WalletId,
    pub to: WalletId,
    pub amount: RevAmount,
    pub kind: TransferKind,
}

pub fn transfers(record: &BlockRecord) -> Vec<RevTransfer> {
    let Some(settlement) = record.settlement() else {
        return Vec::new();
    };
    let compute = wallet_channel_id(&compute_wallet_owner());
    let mut out = Vec::new();
    for charge in &settlement.charges {
        if charge.comm_rev > 0 {
            out.push(RevTransfer {
                envelope_id: charge.envelope_sig.clone(),
                from: charge.wallet.clone(),
                to: settlement.proposer.clone(),
                amount: charge.comm_rev,
                kind: TransferKind::CommCharge,
            });
        }
        if charge.compute_rev > 0 {
            out.push(RevTransfer {
                envelope_id: charge.envelope_sig.clone(),
                from: charge.wallet.clone(),
                to: compute.clone(),
                amount: charge.compute_rev,
                kind: TransferKind::ComputeCharge,
            });
        }
    }
    out
}

pub fn wallet_history<'a>(
    records: impl IntoIterator<Item = &'a BlockRecord>,
    wallet: &WalletId,
) -> Vec<RevTransfer> {
    records
        .into_iter()
        .flat_map(transfers)
        .filter(|t| &t.from == wallet || &t.to == wallet)
        .collect()
}
