use std::mem::size_of;

use super::*;

pub fn sig_to_cost_signature_metered(
    signature: &Sig,
    backing: &dyn BackingMeter,
) -> Result<CostSignature, AuthorityError> {
    let cleanup = |operations: usize, scanned, bytes| {
        backing.reserve(
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            scanned,
            bytes,
        )
    };
    let meter = SorterMeter::new(&cleanup);
    let mut pending = meter.vec::<&Sig>(1).map_err(authority_backing_error)?;
    pending.push(signature);
    let mut elements = meter
        .vec::<CostSignature>(0)
        .map_err(authority_backing_error)?;
    while let Some(current) = pending.pop() {
        meter
            .reserve(1, size_of::<Sig>(), 0)
            .map_err(authority_backing_error)?;
        match current {
            Sig::Unit => {}
            Sig::Ground(bytes) | Sig::Quote(bytes) => {
                // Changed by D-O1 (DR-110): block accounting charges inline bytes
                // once per enclosing block.
                // let atom = CostSignature {
                //     value: Some(CostSignatureValue::Ground(
                //         meter.clone(bytes).map_err(authority_backing_error)?,
                //     )),
                // };
                let atom = CostSignature {
                    value: Some(CostSignatureValue::Ground(
                        meter.clone_blocks(bytes).map_err(authority_backing_error)?,
                    )),
                };
                meter
                    .push(&mut elements, atom)
                    .map_err(authority_backing_error)?;
            }
            Sig::And(left, right) => {
                meter
                    .push(&mut pending, right.as_ref())
                    .map_err(authority_backing_error)?;
                meter
                    .push(&mut pending, left.as_ref())
                    .map_err(authority_backing_error)?;
            }
            _ => return Err(AuthorityError::UnsupportedFundingSignature),
        }
    }
    match elements.len() {
        0 => Ok(CostSignature {
            value: Some(CostSignatureValue::Unit(true)),
        }),
        1 => elements.pop().ok_or(AuthorityError::MalformedCompound),
        _ => sort_signature_metered(
            &CostSignature {
                value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                    elements,
                })),
            },
            &meter,
        )
        .map(|sorted| sorted.term)
        .map_err(authority_backing_error),
    }
}

pub fn cost_region_metered(
    signature: &CostSignature,
    entropy: &[u8],
    discriminator: u32,
    backing: &dyn BackingMeter,
) -> Result<CostRegion, AuthorityError> {
    let cleanup = |operations: usize, scanned, bytes| {
        backing.reserve(
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            scanned,
            bytes,
        )
    };
    let meter = SorterMeter::new(&cleanup);
    let signature = canonical_cost_signature_metered(signature, &cleanup)?;
    let encoded_len = signature.encoded_len();
    let mut signature_bytes = meter
        .vec::<u8>(encoded_len)
        .map_err(authority_backing_error)?;
    meter
        .nested_encode(&signature, encoded_len)
        .map_err(authority_backing_error)?;
    signature
        .encode(&mut signature_bytes)
        .map_err(|_| AuthorityError::HostWorkRejected)?;
    let preimage_len = REGION_DOMAIN
        .len()
        .checked_add(size_of::<u64>())
        .and_then(|length| length.checked_add(entropy.len()))
        .and_then(|length| length.checked_add(size_of::<u32>()))
        .and_then(|length| length.checked_add(size_of::<u64>()))
        .and_then(|length| length.checked_add(signature_bytes.len()))
        .ok_or(AuthorityError::HostWorkRejected)?;
    let mut bytes = meter
        .vec::<u8>(preimage_len)
        .map_err(authority_backing_error)?;
    bytes.extend_from_slice(REGION_DOMAIN);
    bytes.extend_from_slice(
        &u64::try_from(entropy.len())
            .map_err(|_| AuthorityError::HostWorkRejected)?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(entropy);
    bytes.extend_from_slice(&discriminator.to_le_bytes());
    bytes.extend_from_slice(
        &u64::try_from(signature_bytes.len())
            .map_err(|_| AuthorityError::HostWorkRejected)?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&signature_bytes);
    meter
        .reserve(
            preimage_len
                .checked_add(1)
                .ok_or(AuthorityError::HostWorkRejected)?,
            preimage_len,
            32,
        )
        .map_err(authority_backing_error)?;
    Ok(CostRegion {
        instance_id: Blake2b256::hash(bytes),
        signature: Some(signature),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct Budget {
        remaining: Cell<usize>,
        calls: Cell<usize>,
    }

    impl Budget {
        fn new(remaining: usize) -> Self {
            Self {
                remaining: Cell::new(remaining),
                calls: Cell::new(0),
            }
        }
    }

    impl BackingMeter for Budget {
        fn reserve(&self, _: usize, _: usize, _: usize) -> Result<(), BackingError> {
            if self.remaining.get() == 0 {
                return Err(BackingError::Rejected);
            }
            self.remaining.set(self.remaining.get() - 1);
            self.calls.set(self.calls.get() + 1);
            Ok(())
        }
    }

    fn nested_signature() -> Sig {
        Sig::And(
            Box::new(Sig::And(
                Box::new(Sig::Ground(b"z".to_vec())),
                Box::new(Sig::Unit),
            )),
            Box::new(Sig::And(
                Box::new(Sig::Quote(b"a".to_vec())),
                Box::new(Sig::Ground(b"a".to_vec())),
            )),
        )
    }

    #[test]
    fn fallback_signature_and_region_keep_historical_bytes() {
        for signature in [
            Sig::Unit,
            Sig::Ground(b"ground".to_vec()),
            Sig::Quote(b"quote".to_vec()),
            Sig::And(
                Box::new(Sig::Unit),
                Box::new(Sig::Ground(b"ground".to_vec())),
            ),
            Sig::And(
                Box::new(Sig::And(
                    Box::new(Sig::Ground(b"ground".to_vec())),
                    Box::new(Sig::Unit),
                )),
                Box::new(Sig::Quote(b"quote".to_vec())),
            ),
            nested_signature(),
        ] {
            let legacy_signature = sig_to_cost_signature(&signature).unwrap();
            let metered_signature =
                sig_to_cost_signature_metered(&signature, &Budget::new(usize::MAX)).unwrap();
            assert_eq!(metered_signature, legacy_signature);
            assert_eq!(
                metered_signature.encode_to_vec(),
                legacy_signature.encode_to_vec()
            );
            for discriminator in [0, 7, u32::MAX] {
                let legacy_region =
                    cost_region(&legacy_signature, b"fallback identity", discriminator).unwrap();
                let metered_region = cost_region_metered(
                    &metered_signature,
                    b"fallback identity",
                    discriminator,
                    &Budget::new(usize::MAX),
                )
                .unwrap();
                assert_eq!(metered_region, legacy_region);
            }
        }
    }

    #[test]
    fn fallback_reservation_cuts_reject_before_returning_authority() {
        let signature = nested_signature();
        let conversion = Budget::new(usize::MAX);
        let canonical = sig_to_cost_signature_metered(&signature, &conversion).unwrap();
        assert!(conversion.calls.get() > 1);
        for accepted in 0..conversion.calls.get() {
            assert_eq!(
                sig_to_cost_signature_metered(&signature, &Budget::new(accepted)),
                Err(AuthorityError::HostWorkRejected),
                "conversion cut={accepted}",
            );
        }
        let region = Budget::new(usize::MAX);
        let expected = cost_region_metered(&canonical, b"identity", 3, &region).unwrap();
        assert_eq!(expected, cost_region(&canonical, b"identity", 3).unwrap());
        assert!(region.calls.get() > 1);
        for accepted in 0..region.calls.get() {
            assert_eq!(
                cost_region_metered(&canonical, b"identity", 3, &Budget::new(accepted)),
                Err(AuthorityError::HostWorkRejected),
                "region cut={accepted}",
            );
        }
    }

    #[test]
    fn fallback_walk_is_iterative_and_budget_limited() {
        let mut signature = Sig::Ground(b"leaf".to_vec());
        for _ in 0..1024 {
            signature = Sig::And(Box::new(Sig::Unit), Box::new(signature));
        }
        assert_eq!(
            sig_to_cost_signature_metered(&signature, &Budget::new(usize::MAX)).unwrap(),
            sig_to_cost_signature(&Sig::Ground(b"leaf".to_vec())).unwrap(),
        );
        assert_eq!(
            sig_to_cost_signature_metered(&signature, &Budget::new(8)),
            Err(AuthorityError::HostWorkRejected),
        );
    }
}
