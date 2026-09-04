// See casper/src/main/scala/coop/rchain/casper/util/DagOperations.scala

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use shared::rust::store::key_value_store::KvStoreError;

pub struct DagOperations;

// Wrapper for BlockMetadata with reverse ordering for BTreeSet
#[derive(Clone, Debug, PartialEq, Eq)]
struct ReverseOrderedBlockMetadata(BlockMetadata);

impl PartialOrd for ReverseOrderedBlockMetadata {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for ReverseOrderedBlockMetadata {
    fn cmp(&self, other: &Self) -> Ordering {
        // Equivalent to Scala's SortedSet with BlockMetadata.orderingByNum.reverse
        // Reverse ordering: highest blocknum first (equivalent to orderingByNum.reverse)
        BlockMetadata::ordering_by_num(&self.0, &other.0).reverse()
    }
}

impl DagOperations {
    fn metadata_from_cache_or_dag(
        metadata_cache: &mut HashMap<BlockHash, BlockMetadata>,
        block_hash: &BlockHash,
        dag: &KeyValueDagRepresentation,
    ) -> Result<BlockMetadata, KvStoreError> {
        if let Some(metadata) = metadata_cache.get(block_hash) {
            return Ok(metadata.clone());
        }

        let metadata = dag.lookup_unsafe(block_hash)?;
        metadata_cache.insert(block_hash.clone(), metadata.clone());
        Ok(metadata)
    }

    /// Conceptually, the LUCA is the lowest point at which the histories of b1 and b2 diverge.
    /// We compute by finding the first block that is the "lowest" (has highest blocknum) block common
    /// for both blocks' ancestors.
    /// `floor` is the oldest block the DAG is guaranteed to hold — the node's
    /// approved block. A DAG restored from LFS is truncated below it, so a
    /// walk that descends past it asks for parents that were never downloaded.
    /// Nothing below the approved block can be an answer anyway: it is
    /// finalized, so no fork under it is live.
    pub async fn lowest_universal_common_ancestor_many(
        blocks: &[BlockMetadata],
        dag: &KeyValueDagRepresentation,
        floor: &BlockMetadata,
    ) -> Result<BlockMetadata, KvStoreError> {
        if blocks.is_empty() {
            return Err(KvStoreError::InvalidArgument(
                "Cannot compute LUCA for an empty block set".to_string(),
            ));
        }

        if blocks.len() == 1 {
            return Ok(blocks[0].clone());
        }

        let mut current: BTreeSet<ReverseOrderedBlockMetadata> = BTreeSet::new();
        let mut metadata_cache: HashMap<BlockHash, BlockMetadata> = HashMap::new();

        for block in blocks {
            metadata_cache.insert(block.block_hash.clone(), block.clone());
            current.insert(ReverseOrderedBlockMetadata(block.clone()));
        }

        loop {
            if current.len() == 1 {
                break current;
            }

            let (head, tail) = (
                current
                    .iter()
                    .next()
                    .expect("BTreeSet should not be empty")
                    .0
                    .clone(),
                current.iter().skip(1).cloned(),
            );

            // The head is the highest remaining block, so once it is down to the
            // floor everything else is too and no further descent can raise the
            // answer. Checked BEFORE expanding, so a parent below the floor is
            // never read — that is the parent a truncated DAG does not hold.
            // The floor need not be an ancestor of an input that was itself below
            // it: such an input is under finality and carries no fork-choice
            // weight, and callers use this result as the bound of the region they
            // score, not as a proven ancestor.
            if head.block_number <= floor.block_number {
                return Ok(floor.clone());
            }

            let mut next: BTreeSet<ReverseOrderedBlockMetadata> = tail.collect();

            for parent_hash in &head.parents {
                let parent =
                    Self::metadata_from_cache_or_dag(&mut metadata_cache, parent_hash, dag)?;
                next.insert(ReverseOrderedBlockMetadata(parent));
            }

            current = next;
        }
        .into_iter()
        .next()
        .map(|wrapper| wrapper.0)
        .ok_or_else(|| KvStoreError::KeyNotFound("No common ancestor found".to_string()))
    }

    /// Conceptually, the LUCA is the lowest point at which the histories of b1 and b2 diverge.
    /// We compute by finding the first block that is the "lowest" (has highest blocknum) block common
    /// for both blocks' ancestors.
    pub async fn lowest_universal_common_ancestor(
        b1: &BlockMetadata,
        b2: &BlockMetadata,
        dag: &KeyValueDagRepresentation,
        floor: &BlockMetadata,
    ) -> Result<BlockMetadata, KvStoreError> {
        if b1 == b2 {
            return Ok(b1.clone());
        }

        Self::lowest_universal_common_ancestor_many(&[b1.clone(), b2.clone()], dag, floor).await
    }
}
