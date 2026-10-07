use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use rholang::rust::interpreter::host_work::HostWorkBudget;

use super::snapshot::reserve;
use super::{invalid, ordered_changes, CasperError, PrepaidReceiptChange, PrepaidReceiptLimits};

pub fn compose_prepaid_receipt_changes<'a>(
    first: &'a [PrepaidReceiptChange<'a>],
    second: &'a [PrepaidReceiptChange<'a>],
    limits: PrepaidReceiptLimits,
    budget: &HostWorkBudget,
) -> Result<Vec<PrepaidReceiptChange<'a>>, CasperError> {
    let count = first
        .len()
        .checked_add(second.len())
        .filter(|count| *count <= limits.entries)
        .ok_or_else(|| invalid("composed receipt entry limit exceeded"))?;
    let bytes = count
        .checked_mul(
            size_of::<PrepaidReceiptChange<'_>>() + 2 * size_of::<&PrepaidReceiptChange<'_>>(),
        )
        .ok_or_else(|| invalid("composed receipt allocation overflow"))?;
    reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        count
            .checked_mul(1 + count.checked_ilog2().unwrap_or(0) as usize)
            .ok_or_else(|| invalid("composed receipt sorting work overflow"))?,
    )?;
    let first = ordered_changes(first, limits)?;
    let second = ordered_changes(second, limits)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| invalid("composed receipt allocation failed"))?;
    let mut left = first.into_iter().peekable();
    let mut right = second.into_iter().peekable();
    while let (Some(a), Some(b)) = (left.peek(), right.peek()) {
        if a.receipt_id < b.receipt_id {
            result.push(*left.next().unwrap());
        } else if a.receipt_id > b.receipt_id {
            result.push(*right.next().unwrap());
        } else {
            let before = *left.next().unwrap();
            let after = *right.next().unwrap();
            if before.replacement != after.expected {
                return Err(invalid("composed receipt intermediate value differs"));
            }
            if before.expected != after.replacement {
                result.push(PrepaidReceiptChange {
                    receipt_id: before.receipt_id,
                    expected: before.expected,
                    replacement: after.replacement,
                });
            }
        }
    }
    result.extend(left.copied());
    result.extend(right.copied());
    ordered_changes(&result, limits)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use models::rust::phlo_wire::PhloWireLimits;

    use super::*;

    fn budget() -> HostWorkBudget {
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000)))
    }

    fn limits() -> PrepaidReceiptLimits {
        PrepaidReceiptLimits {
            entries: 8,
            value_bytes: 16,
            batch_bytes: 1024,
        }
    }

    #[test]
    fn overlapping_bucket_changes_have_one_original_to_final_transition() {
        let first = [PrepaidReceiptChange {
            receipt_id: [1; 32],
            expected: Some(b"old"),
            replacement: Some(b"with-birth"),
        }];
        let second = [PrepaidReceiptChange {
            receipt_id: [1; 32],
            expected: Some(b"with-birth"),
            replacement: Some(b"after-pop"),
        }];
        let composed =
            compose_prepaid_receipt_changes(&first, &second, limits(), &budget()).unwrap();
        assert_eq!(composed.len(), 1);
        assert_eq!(composed[0].expected, Some(b"old".as_slice()));
        assert_eq!(composed[0].replacement, Some(b"after-pop".as_slice()));
        let stale = [PrepaidReceiptChange {
            expected: Some(b"other"),
            ..second[0]
        }];
        assert!(compose_prepaid_receipt_changes(&first, &stale, limits(), &budget()).is_err());
    }

    #[test]
    fn retained_birth_and_existing_stack_pop_share_one_receipt_bucket() {
        let source = [9; 32];
        let bucket_limits = super::super::PrepaidReceiptBucketLimits {
            occurrences: 2,
            wire: PhloWireLimits {
                total_bytes: 1024,
                field_bytes: 512,
            },
        };
        let old = super::super::PrepaidReceiptBucket::new(source, &[b"old-cell"], bucket_limits)
            .unwrap()
            .encode(bucket_limits)
            .unwrap();
        let with_birth = super::super::PrepaidReceiptBucket::new(
            source,
            &[b"old-cell".as_slice(), b"born-cell".as_slice()],
            bucket_limits,
        )
        .unwrap()
        .encode(bucket_limits)
        .unwrap();
        let after_pop =
            super::super::PrepaidReceiptBucket::new(source, &[b"born-cell"], bucket_limits)
                .unwrap()
                .encode(bucket_limits)
                .unwrap();
        let id = super::super::PrepaidReceiptBucket::key_for_source(&source);
        let births = [PrepaidReceiptChange {
            receipt_id: id,
            expected: Some(&old),
            replacement: Some(&with_birth),
        }];
        let pops = [PrepaidReceiptChange {
            receipt_id: id,
            expected: Some(&with_birth),
            replacement: Some(&after_pop),
        }];
        let composed = compose_prepaid_receipt_changes(
            &births,
            &pops,
            PrepaidReceiptLimits {
                entries: 2,
                value_bytes: 1024,
                batch_bytes: 4096,
            },
            &budget(),
        )
        .unwrap();
        assert_eq!(composed.len(), 1);
        assert_eq!(composed[0].receipt_id, id);
        assert_eq!(composed[0].expected, Some(old.as_slice()));
        let final_bucket = super::super::PrepaidReceiptBucket::decode(
            composed[0].replacement.unwrap(),
            bucket_limits,
        )
        .unwrap();
        assert_eq!(final_bucket.receipts(), &[b"born-cell".as_slice()]);
    }
}
