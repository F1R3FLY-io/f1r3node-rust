use std::mem::size_of;

use crypto::rust::signatures::signed::ToMessage;
use models::rust::host_work::HostWorkDimension;
use models::rust::phlo_obligation::PHLO_OBLIGATION_V1_DOMAIN;
use models::rust::phlo_wire::PhloWireDecoder;
use models::rust::signed_phlo_deploy::FundedDeploy;
use rholang::rust::interpreter::accounting::phlo_execution::{
    CheckedRetainedBirthFunding, PhloObligationKey, RetainedCellBackingLimits,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;

use super::snapshot::reserve;
use super::{
    invalid, CapturedNativeRetainedBirths, CasperError, NativePrepaidCell, NativePrepaidCellLimits,
    NativePrepaidContribution, NativePrepaidOrigin, OrderedPrepaidCells, PrepaidCellLimits,
    PrepaidReceiptBucket, PrepaidReceiptBucketLimits, PrepaidReceiptChange, PrepaidReceiptLimits,
    PrepaidReceiptSnapshot,
};
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;

#[derive(Clone, Copy, Debug)]
pub struct NativeRetainedRecordLimits {
    pub backing: RetainedCellBackingLimits,
    pub cell: NativePrepaidCellLimits,
    pub stack: PrepaidCellLimits,
    pub aggregate_bytes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct NativeRetainedStackRecord {
    stack_id: [u8; 32],
    source_hash: [u8; 32],
    cells: Vec<u8>,
}

impl NativeRetainedStackRecord {
    pub fn stack_id(&self) -> &[u8; 32] { &self.stack_id }
    pub fn source_hash(&self) -> &[u8; 32] { &self.source_hash }
    pub fn encoded_cells(&self) -> &[u8] { &self.cells }
}

#[derive(Debug)]
pub struct NativeRetainedReceiptRecords<'r, 's, 'a, 'b, A = FundedDeploy> {
    births: &'r CapturedNativeRetainedBirths<'s, 'a, 'b, A>,
    records: Vec<NativeRetainedStackRecord>,
}

#[derive(Debug)]
pub struct PreparedNativeRetainedReceipts<'r, 's, 'a, 'b, A = FundedDeploy> {
    births: &'r CapturedNativeRetainedBirths<'s, 'a, 'b, A>,
    replacements: Vec<([u8; 32], Vec<u8>)>,
}

impl<'r, 's, 'a, 'b, A> PreparedNativeRetainedReceipts<'r, 's, 'a, 'b, A> {
    pub fn births(&self) -> &'r CapturedNativeRetainedBirths<'s, 'a, 'b, A> { self.births }
    pub fn changes(&self) -> impl ExactSizeIterator<Item = PrepaidReceiptChange<'_>> {
        self.replacements
            .iter()
            .map(|(key, bytes)| PrepaidReceiptChange {
                receipt_id: *key,
                expected: None,
                replacement: Some(bytes),
            })
    }
}

impl<'r, 's, 'a, 'b, A> NativeRetainedReceiptRecords<'r, 's, 'a, 'b, A> {
    pub fn births(&self) -> &'r CapturedNativeRetainedBirths<'s, 'a, 'b, A> { self.births }
    pub fn records(&self) -> &[NativeRetainedStackRecord] { &self.records }

    pub fn prepare_insertions(
        &self,
        snapshot: &PrepaidReceiptSnapshot,
        bucket: PrepaidReceiptBucketLimits,
        limits: PrepaidReceiptLimits,
        budget: &HostWorkBudget,
    ) -> Result<PreparedNativeRetainedReceipts<'r, 's, 'a, 'b, A>, CasperError> {
        let root = self
            .births
            .settlement()
            .snapshot()
            .wallets()
            .pre_state_root();
        let replacements =
            prepare_insertions(&self.records, root, snapshot, bucket, limits, budget)?;
        Ok(PreparedNativeRetainedReceipts {
            births: self.births,
            replacements,
        })
    }
}

pub(in crate::rust::util::rholang::costacc) fn prepare_insertions(
    records: &[NativeRetainedStackRecord],
    pre_state_root: [u8; 32],
    snapshot: &PrepaidReceiptSnapshot,
    bucket: PrepaidReceiptBucketLimits,
    limits: PrepaidReceiptLimits,
    budget: &HostWorkBudget,
) -> Result<Vec<([u8; 32], Vec<u8>)>, CasperError> {
    if snapshot.root() != pre_state_root {
        return Err(invalid(
            "retained receipt absence belongs to another pre-state",
        ));
    }
    if records.len() > limits.entries {
        return Err(invalid(
            "retained receipt insertion count exceeds its limit",
        ));
    }
    let mut remaining = limits.batch_bytes;
    let bytes = records
        .len()
        .checked_mul(size_of::<([u8; 32], Vec<u8>)>() + size_of::<PrepaidReceiptChange>())
        .ok_or_else(|| invalid("retained receipt insertion size overflow"))?;
    charge(budget, &mut remaining, bytes)?;
    let work = records
        .len()
        .checked_mul(64 * (1 + records.len().checked_ilog2().unwrap_or(0) as usize))
        .ok_or_else(|| invalid("retained receipt insertion work overflow"))?;
    reserve(budget, HostWorkDimension::VerificationOperations, work)?;
    let mut replacements = Vec::new();
    replacements
        .try_reserve_exact(records.len())
        .map_err(|_| invalid("retained receipt insertion allocation failed"))?;
    for record in records {
        let key = PrepaidReceiptBucket::key_for_source(record.source_hash());
        if snapshot.receipt(&pre_state_root, &key)? != Some(None) {
            return Err(invalid(
                "retained receipt insertion requires captured source-bucket absence",
            ));
        }
        let size = record
            .encoded_cells()
            .len()
            .checked_add(128)
            .ok_or_else(|| invalid("retained receipt bucket size overflow"))?;
        charge(budget, &mut remaining, size)?;
        let bytes =
            PrepaidReceiptBucket::new(*record.source_hash(), &[record.encoded_cells()], bucket)?
                .encode(bucket)?;
        if bytes.len() > limits.value_bytes {
            return Err(invalid(
                "retained receipt insertion value exceeds its limit",
            ));
        }
        replacements.push((key, bytes));
    }
    replacements.sort_unstable_by_key(|entry| entry.0);
    if replacements.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(invalid(
            "retained receipt insertion repeats a source bucket",
        ));
    }
    Ok(replacements)
}

impl<'s, 'a, 'b, A: std::fmt::Debug + serde::Serialize + ToMessage>
    CapturedNativeRetainedBirths<'s, 'a, 'b, A>
{
    pub fn prepare_cell_records<'r>(
        &'r self,
        limits: NativeRetainedRecordLimits,
        budget: &HostWorkBudget,
    ) -> Result<NativeRetainedReceiptRecords<'r, 's, 'a, 'b, A>, CasperError> {
        let wallets = self.settlement().snapshot().wallets();
        let deploy_id = wallets
            .authorization()
            .envelope()
            .envelope_commitment()
            .map_err(|error| invalid(&error.to_string()))?
            .as_ref()
            .try_into()
            .map_err(|_| invalid("retained acquisition deploy identity must contain 32 bytes"))?;
        let records = encode_retained_records(
            self.policy(),
            self.funding(),
            wallets.pre_state_root(),
            deploy_id,
            limits,
            budget,
        )?;
        Ok(NativeRetainedReceiptRecords {
            births: self,
            records,
        })
    }
}

fn charge(budget: &HostWorkBudget, remaining: &mut usize, bytes: usize) -> Result<(), CasperError> {
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or_else(|| invalid("retained receipt aggregate byte limit exceeded"))?;
    reserve(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    reserve(budget, HostWorkDimension::VerificationOperations, bytes)
}

pub(in crate::rust::util::rholang::costacc) fn encode_retained_records(
    policy: &AdoptedResourcePolicy,
    funding: &CheckedRetainedBirthFunding<'_, '_, '_>,
    pre_state_root: [u8; 32],
    deploy_id: [u8; 32],
    limits: NativeRetainedRecordLimits,
    budget: &HostWorkBudget,
) -> Result<Vec<NativeRetainedStackRecord>, CasperError> {
    let genesis_root = policy
        .genesis()
        .genesis_root()
        .as_ref()
        .try_into()
        .map_err(|_| invalid("retained acquisition genesis root must contain 32 bytes"))?;
    let mut remaining = limits.aggregate_bytes;
    let capture = funding.capture();
    let columns = capture.obligations().len();
    let mut count = 0usize;
    for binding in funding.births() {
        count = count
            .checked_add(binding.birth.cells.len())
            .filter(|n| *n <= limits.backing.cells)
            .ok_or_else(|| invalid("retained receipt cell limit exceeded"))?;
        if binding.birth.cells.len() > limits.stack.cells {
            return Err(invalid("retained receipt stack cell limit exceeded"));
        }
    }
    let bytes = columns
        .checked_mul(size_of::<Vec<(usize, usize)>>())
        .and_then(|n| {
            count
                .checked_mul(
                    size_of::<(usize, usize)>() + size_of::<Vec<u8>>() + size_of::<&[u8]>(),
                )
                .and_then(|m| n.checked_add(m))
        })
        .and_then(|n| {
            funding
                .births()
                .len()
                .checked_mul(size_of::<Vec<Vec<u8>>>() + size_of::<NativeRetainedStackRecord>())
                .and_then(|m| n.checked_add(m))
        })
        .ok_or_else(|| invalid("retained receipt allocation size overflow"))?;
    charge(budget, &mut remaining, bytes)?;
    let mut positions = Vec::new();
    positions
        .try_reserve_exact(columns)
        .map_err(|_| invalid("retained receipt column allocation failed"))?;
    for column in capture.obligations() {
        let mut entries = Vec::new();
        if matches!(column.key(), PhloObligationKey::RetainedResource(_)) {
            let quantity = usize::try_from(column.quantity())
                .map_err(|_| invalid("retained receipt quantity overflow"))?;
            entries
                .try_reserve_exact(quantity)
                .map_err(|_| invalid("retained receipt mapping allocation failed"))?;
        }
        positions.push(entries);
    }
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(funding.births().len())
        .map_err(|_| invalid("retained receipt stack allocation failed"))?;
    for (stack_index, binding) in funding.births().iter().enumerate() {
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(binding.birth.cells.len())
            .map_err(|_| invalid("retained receipt cell allocation failed"))?;
        cells.resize_with(binding.birth.cells.len(), Vec::new);
        encoded.push(cells);
        for (cell_index, column) in binding.obligation_positions.iter().copied().enumerate() {
            positions[column].push((stack_index, cell_index));
        }
    }
    let mut contributions_left = limits.backing.contributions;
    for (column, cells) in capture.obligations().zip(&positions) {
        if !matches!(column.key(), PhloObligationKey::RetainedResource(_)) {
            continue;
        }
        charge(budget, &mut remaining, column.encoded_key().len())?;
        let mut input = PhloWireDecoder::new(column.encoded_key(), limits.cell.wire)
            .map_err(|error| invalid(&error.to_string()))?;
        if input.bytes().map_err(|error| invalid(&error.to_string()))? != PHLO_OBLIGATION_V1_DOMAIN
            || input.bytes().map_err(|error| invalid(&error.to_string()))? != [2]
        {
            return Err(invalid(
                "retained receipt requires a canonical retained-resource key",
            ));
        }
        let resource = input.bytes().map_err(|error| invalid(&error.to_string()))?;
        input
            .finish()
            .map_err(|error| invalid(&error.to_string()))?;
        let split = column
            .split_retained_cells(
                RetainedCellBackingLimits {
                    cells: limits.backing.cells,
                    contributions: contributions_left,
                },
                budget,
            )
            .map_err(|error| invalid(&error.to_string()))?;
        if split.cells().len() != cells.len() {
            return Err(invalid(
                "retained receipt mapping differs from its checked quantity",
            ));
        }
        contributions_left = contributions_left
            .checked_sub(split.contribution_count())
            .ok_or_else(|| invalid("retained receipt contribution limit exceeded"))?;
        for (backing, (stack_index, cell_index)) in split.cells().zip(cells.iter().copied()) {
            let size = super::cell::encoded_size(resource.len(), backing.len(), limits.cell)?;
            let scratch = backing
                .len()
                .checked_mul(size_of::<NativePrepaidContribution>())
                .and_then(|n| n.checked_add(size))
                .ok_or_else(|| invalid("retained receipt cell size overflow"))?;
            charge(budget, &mut remaining, scratch)?;
            let mut contributions = Vec::new();
            contributions
                .try_reserve_exact(backing.len())
                .map_err(|_| invalid("retained receipt source allocation failed"))?;
            for entry in backing {
                let custody = capture.sources()[entry.source_index]
                    .source()
                    .custody
                    .try_into()
                    .map_err(|_| invalid("retained receipt custody must contain 32 bytes"))?;
                contributions.push(NativePrepaidContribution {
                    custody,
                    amount: entry.amount,
                });
            }
            let origin = NativePrepaidOrigin {
                genesis_root,
                pre_state_root,
                deploy_id,
                birth_source: funding.births()[stack_index].birth.produce_hash,
                cell_index: u64::try_from(cell_index)
                    .map_err(|_| invalid("retained receipt cell index overflow"))?,
            };
            encoded[stack_index][cell_index] =
                NativePrepaidCell::encode(policy, origin, resource, &contributions, limits.cell)?;
        }
    }
    let mut records = Vec::new();
    records
        .try_reserve_exact(encoded.len())
        .map_err(|_| invalid("retained receipt record allocation failed"))?;
    for (binding, cells) in funding.births().iter().zip(encoded) {
        let bytes = cells
            .iter()
            .try_fold(128usize, |total, cell| {
                total.checked_add(8)?.checked_add(cell.len())
            })
            .ok_or_else(|| invalid("retained receipt stack size overflow"))?;
        charge(budget, &mut remaining, bytes)?;
        let refs = cells.iter().map(Vec::as_slice).collect::<Vec<_>>();
        records.push(NativeRetainedStackRecord {
            stack_id: binding.birth.stack_id,
            source_hash: binding.birth.produce_hash,
            cells: OrderedPrepaidCells::encode(&refs, limits.stack)?,
        });
    }
    Ok(records)
}
