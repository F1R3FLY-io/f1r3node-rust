// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Shared fixtures for the detector-totality test family (UC-101..UC-108,
// UC-112, prop_t_9_11_*, pre_fix_bug_11).
//
// Reference: docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.12.
// Rocq: formal/rocq/slashing/theories/EquivocationDetector.v
// (theorems `detector_total`, `detector_no_unsafe_lookup`,
// `detector_permutation_invariant`).
//
// `DetectorFixture` wires an in-memory block store + DAG against the
// production `EquivocationDetector` so each test can build small
// synthetic DAGs, hold equivocations, and assert the neglect verdict
// (Valid or NeglectedEquivocation).
// `assert_valid` / `assert_neglected` keep the asserts terse so each UC
// test stays under ~30 lines.
//
// Invariants this fixture protects:
//   1. Totality: `check(block)` never returns Err/panics on any well-formed
//      synthetic input.
//   2. Permutation invariance: justification order does not change the
//      classification (see UC-102, UC-104, UC-106).
//   3. Missing-pointer tolerance: a justification whose hash is not in the
//      store is treated as obliviousness, not as a store inconsistency.

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::equivocation_detector::EquivocationDetector;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, F1r3flyState, Header, Justification,
};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

pub struct DetectorFixture {
    pub block_store: KeyValueBlockStore,
    pub dag_storage: BlockDagKeyValueStorage,
    pub genesis: BlockMessage,
    pub validators: Vec<Bytes>,
}

impl DetectorFixture {
    pub async fn new() -> Self {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let validators = (0u8..8u8).map(validator).collect::<Vec<_>>();
        let genesis = block(1, validators[0].clone(), 0, vec![], validators.clone());
        block_store
            .put_block_message(&genesis)
            .expect("store genesis");
        dag_storage
            .insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
            )
            .expect("insert genesis");
        Self {
            block_store,
            dag_storage,
            genesis,
            validators,
        }
    }

    pub fn add_block(&self, block: &BlockMessage) {
        self.block_store
            .put_block_message(block)
            .expect("store block");
        self.dag_storage
            .insert(
                block,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
            )
            .expect("insert block");
    }

    /// Holds an equivocation of `validators[offender_index]` above `base_seq`:
    /// a block at `base_seq + 1` stored with its sibling.
    pub fn add_equivocation(&self, offender_index: usize, base_seq: i32) {
        let equivocation = block(
            240 + offender_index as u8,
            self.validators[offender_index].clone(),
            base_seq + 1,
            vec![],
            self.validators.clone(),
        );
        self.dag_storage
            .insert(
                &equivocation,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Equivocation {
                    sibling: hash(239 - offender_index as u8),
                },
            )
            .expect("insert equivocation");
    }

    pub async fn check(&self, block: &BlockMessage) -> Either<BlockError, ValidBlock> {
        EquivocationDetector::check_neglected_equivocations(
            block,
            &self
                .dag_storage
                .get_representation()
                .expect("dag representation"),
            &self.block_store,
            &self.genesis,
            &block.body.state.bonds,
        )
        .expect("detector is total")
    }
}

pub fn hash(byte: u8) -> Bytes { Bytes::from(vec![byte; 32]) }

pub fn validator(byte: u8) -> Bytes { Bytes::from(vec![byte; 65]) }

pub fn justification(validator: Bytes, latest_block_hash: Bytes) -> Justification {
    Justification {
        validator,
        latest_block_hash,
    }
}

pub fn block(
    hash_byte: u8,
    sender: Bytes,
    seq_num: i32,
    justifications: Vec<Justification>,
    bonded_validators: Vec<Bytes>,
) -> BlockMessage {
    BlockMessage {
        block_hash: hash(hash_byte),
        header: Header {
            parents_hash_list: vec![],
            timestamp: i64::from(hash_byte),
            version: 1,
            extra_bytes: Bytes::new(),
            chain_id: Bytes::new(),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: hash(hash_byte.saturating_add(100)),
                post_state_hash: hash(hash_byte.saturating_add(101)),
                bonds: bonded_validators
                    .into_iter()
                    .map(|validator| Bond {
                        validator,
                        stake: 100.into(),
                    })
                    .collect(),
                block_number: i64::from(seq_num),
            },
            deploys: vec![],
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
            applied_from_scope: vec![],
            merge_base: Bytes::new(),
        },
        justifications,
        sender,
        seq_num,
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
    }
}

pub fn assert_valid(result: Either<BlockError, ValidBlock>) {
    assert_eq!(result, Either::Right(ValidBlock::Valid));
}

pub fn assert_neglected(result: Either<BlockError, ValidBlock>) {
    assert_eq!(
        result,
        Either::Left(BlockError::Invalid(InvalidBlock::NeglectedEquivocation))
    );
}
