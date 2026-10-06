use thiserror::Error;

use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const NATIVE_WALLET_RECEIPT_V1_DOMAIN: &[u8] = b"f1r3node:native-wallet-receipt:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeWalletReceiptLimits {
    pub wire: PhloWireLimits,
    pub payers: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeWalletReceiptRow<'a> {
    pub address: &'a [u8],
    pub resource_rev: u128,
    pub fee_rev: u128,
    pub post_balance: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeWalletReceiptV1<'a> {
    pub rows: Vec<NativeWalletReceiptRow<'a>>,
    pub resource_rev: u128,
    pub fee_rev: u128,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum NativeWalletReceiptError {
    #[error("native wallet receipt has an unsupported format")]
    FormatDomain,
    #[error("native wallet receipt has no payers or exceeds the payer limit")]
    PayerLimit,
    #[error("native wallet receipt has an empty or noncanonical payer address")]
    PayerAddress,
    #[error("native wallet receipt has a noncanonical field width")]
    FieldWidth,
    #[error("native wallet receipt REV amounts overflow or fail conservation")]
    RevConservation,
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

impl NativeWalletReceiptLimits {
    fn nested(self) -> PhloWireLimits {
        PhloWireLimits {
            total_bytes: self.wire.field_bytes,
            field_bytes: self.wire.field_bytes,
        }
    }
}

impl NativeWalletReceiptV1<'_> {
    pub fn rev_spent(&self) -> Result<u128, NativeWalletReceiptError> {
        self.resource_rev
            .checked_add(self.fee_rev)
            .ok_or(NativeWalletReceiptError::RevConservation)
    }

    fn check(&self, limits: NativeWalletReceiptLimits) -> Result<(), NativeWalletReceiptError> {
        if self.rows.is_empty()
            || self.rows.len() > limits.payers
            || u32::try_from(self.rows.len()).is_err()
        {
            return Err(NativeWalletReceiptError::PayerLimit);
        }
        let mut previous: Option<&[u8]> = None;
        let mut resource_rev = 0u128;
        let mut fee_rev = 0u128;
        for row in &self.rows {
            if row.address.is_empty() || previous.is_some_and(|prior| prior >= row.address) {
                return Err(NativeWalletReceiptError::PayerAddress);
            }
            previous = Some(row.address);
            resource_rev = resource_rev
                .checked_add(row.resource_rev)
                .ok_or(NativeWalletReceiptError::RevConservation)?;
            fee_rev = fee_rev
                .checked_add(row.fee_rev)
                .ok_or(NativeWalletReceiptError::RevConservation)?;
            row.resource_rev
                .checked_add(row.fee_rev)
                .ok_or(NativeWalletReceiptError::RevConservation)?;
        }
        if resource_rev != self.resource_rev || fee_rev != self.fee_rev {
            return Err(NativeWalletReceiptError::RevConservation);
        }
        self.rev_spent()?;
        Ok(())
    }

    pub fn encode(
        &self,
        limits: NativeWalletReceiptLimits,
    ) -> Result<Vec<u8>, NativeWalletReceiptError> {
        self.check(limits)?;
        let mut rows = PhloWireEncoder::new(limits.nested());
        rows.bytes(
            &u32::try_from(self.rows.len())
                .map_err(|_| NativeWalletReceiptError::PayerLimit)?
                .to_be_bytes(),
        )?;
        for row in &self.rows {
            let mut encoded = PhloWireEncoder::new(limits.nested());
            for field in [
                row.address,
                &row.resource_rev.to_be_bytes(),
                &row.fee_rev.to_be_bytes(),
                &row.post_balance.to_be_bytes(),
            ] {
                encoded.bytes(field)?;
            }
            rows.bytes(encoded.as_bytes())?;
        }
        let mut wire = PhloWireEncoder::new(limits.wire);
        for field in [
            NATIVE_WALLET_RECEIPT_V1_DOMAIN,
            rows.as_bytes(),
            &self.resource_rev.to_be_bytes(),
            &self.fee_rev.to_be_bytes(),
        ] {
            wire.bytes(field)?;
        }
        Ok(wire.into_bytes())
    }
}

impl<'a> NativeWalletReceiptV1<'a> {
    pub fn decode(
        input: &'a [u8],
        limits: NativeWalletReceiptLimits,
    ) -> Result<Self, NativeWalletReceiptError> {
        let mut wire = PhloWireDecoder::new(input, limits.wire)?;
        if wire.bytes()? != NATIVE_WALLET_RECEIPT_V1_DOMAIN {
            return Err(NativeWalletReceiptError::FormatDomain);
        }
        let mut rows_wire = PhloWireDecoder::new(wire.bytes()?, limits.nested())?;
        let resource_rev = u128::from_be_bytes(fixed_field(&mut wire)?);
        let fee_rev = u128::from_be_bytes(fixed_field(&mut wire)?);
        wire.finish()?;
        let count = u32::from_be_bytes(fixed_field(&mut rows_wire)?) as usize;
        if count == 0 || count > limits.payers {
            return Err(NativeWalletReceiptError::PayerLimit);
        }
        let mut rows = Vec::new();
        rows.try_reserve_exact(count)
            .map_err(|_| PhloWireError::AllocationFailed)?;
        for _ in 0..count {
            let mut row = PhloWireDecoder::new(rows_wire.bytes()?, limits.nested())?;
            let address = row.bytes()?;
            let resource_rev = u128::from_be_bytes(fixed_field(&mut row)?);
            let fee_rev = u128::from_be_bytes(fixed_field(&mut row)?);
            let post_balance = u64::from_be_bytes(fixed_field(&mut row)?);
            row.finish()?;
            rows.push(NativeWalletReceiptRow {
                address,
                resource_rev,
                fee_rev,
                post_balance,
            });
        }
        rows_wire.finish()?;
        let receipt = Self {
            rows,
            resource_rev,
            fee_rev,
        };
        receipt.check(limits)?;
        Ok(receipt)
    }
}

fn fixed_field<const N: usize>(
    wire: &mut PhloWireDecoder<'_>,
) -> Result<[u8; N], NativeWalletReceiptError> {
    wire.bytes()?
        .try_into()
        .map_err(|_| NativeWalletReceiptError::FieldWidth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> NativeWalletReceiptLimits {
        NativeWalletReceiptLimits {
            wire: PhloWireLimits {
                total_bytes: 4096,
                field_bytes: 2048,
            },
            payers: 2,
        }
    }

    #[test]
    fn canonical_receipt_roundtrips_and_conserves_charges() {
        let receipt = NativeWalletReceiptV1 {
            rows: vec![
                NativeWalletReceiptRow {
                    address: b"a",
                    resource_rev: 7,
                    fee_rev: 2,
                    post_balance: 91,
                },
                NativeWalletReceiptRow {
                    address: b"b",
                    resource_rev: 3,
                    fee_rev: 1,
                    post_balance: 96,
                },
            ],
            resource_rev: 10,
            fee_rev: 3,
        };
        let encoded = receipt.encode(limits()).unwrap();
        assert_eq!(
            NativeWalletReceiptV1::decode(&encoded, limits()).unwrap(),
            receipt
        );
        assert_eq!(receipt.rev_spent().unwrap(), 13);
        for length in 0..encoded.len() {
            assert!(NativeWalletReceiptV1::decode(&encoded[..length], limits()).is_err());
        }
    }

    #[test]
    fn duplicate_or_unsorted_payers_and_nonconserving_totals_reject() {
        let mut receipt = NativeWalletReceiptV1 {
            rows: vec![
                NativeWalletReceiptRow {
                    address: b"b",
                    resource_rev: 7,
                    fee_rev: 2,
                    post_balance: 91,
                },
                NativeWalletReceiptRow {
                    address: b"a",
                    resource_rev: 3,
                    fee_rev: 1,
                    post_balance: 96,
                },
            ],
            resource_rev: 10,
            fee_rev: 3,
        };
        assert!(receipt.encode(limits()).is_err());
        receipt.rows.swap(0, 1);
        receipt.resource_rev += 1;
        assert_eq!(
            receipt.encode(limits()),
            Err(NativeWalletReceiptError::RevConservation)
        );
    }
}
