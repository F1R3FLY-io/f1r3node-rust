// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Shared fixtures for the detector-totality test family (UC-101..UC-108,
// UC-112, prop_t_9_11_*, pre_fix_bug_11).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.12.
// Rocq: formal/rocq/slashing/theories/EquivocationDetector.v
// (theorems `detector_total`, `detector_no_unsafe_lookup`,
// `detector_permutation_invariant`).
//
// `DetectorFixture` wires an in-memory block store + DAG against the
// production `EquivocationDetector` so each test can build small
// synthetic DAGs and assert the detector's classification (Valid,
// AdmissibleEquivocation, IgnorableEquivocation, NeglectedEquivocation).
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

use std::collections::{BTreeSet, HashMap};

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::equivocation_detector::EquivocationDetector;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, F1r3flyState, Header, Justification, ValidatorBondGeneration,
};
use models::rust::equivocation_record::EquivocationRecord;
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
        let mut genesis = block(1, validators[0].clone(), 0, vec![], validators.clone());
        genesis.header.parents_hash_list.clear();
        genesis.body.state.pre_state_hash = hash(101);
        block_store
            .put_block_message(&genesis)
            .expect("store genesis");
        dag_storage
            .insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
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

    pub fn add_record(&self, offender_index: usize, base_seq: i32, detected_hashes: &[Bytes]) {
        self.add_record_at_generation(
            offender_index,
            BondGeneration::GENESIS,
            base_seq,
            detected_hashes,
        );
    }

    pub fn add_record_at_generation(
        &self,
        offender_index: usize,
        bond_generation: BondGeneration,
        base_seq: i32,
        detected_hashes: &[Bytes],
    ) {
        let record = EquivocationRecord::new(
            self.validators[offender_index].clone(),
            bond_generation,
            base_seq,
            detected_hashes.iter().cloned().collect::<BTreeSet<_>>(),
        );
        self.dag_storage
            .insert_equivocation_record(record)
            .expect("insert record");
    }

    pub async fn check(&self, block: &BlockMessage) -> Either<BlockError, ValidBlock> {
        let pre_state_bonds = block
            .body
            .state
            .bonds
            .iter()
            .map(|bond| (bond.validator.clone(), bond.stake))
            .collect();
        self.check_with_pre_state_bonds(block, &pre_state_bonds)
            .await
    }

    pub async fn check_with_pre_state_bonds(
        &self,
        block: &BlockMessage,
        pre_state_bonds: &HashMap<Bytes, i64>,
    ) -> Either<BlockError, ValidBlock> {
        let pre_state_generations = self
            .validators
            .iter()
            .cloned()
            .map(|validator| (validator, BondGeneration::GENESIS))
            .collect();
        EquivocationDetector::check_neglected_equivocations_with_update(
            block,
            &self
                .dag_storage
                .get_representation()
                .expect("dag representation"),
            &self.block_store,
            &self.genesis,
            &self.dag_storage,
            pre_state_bonds,
            &pre_state_generations,
        )
        .await
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
    let mut bond_generations = bonded_validators
        .iter()
        .cloned()
        .map(|validator| ValidatorBondGeneration {
            validator,
            generation: BondGeneration::GENESIS,
        })
        .collect::<Vec<_>>();
    bond_generations.sort();
    let active_validators = bond_generations
        .iter()
        .map(|generation| generation.validator.clone())
        .collect();
    BlockMessage {
        block_hash: hash(hash_byte),
        header: Header {
            parents_hash_list: vec![hash(1)],
            timestamp: i64::from(hash_byte),
            version: models::rust::block_metadata::CERTIFIED_ADMISSION_PROTOCOL_VERSION,
            extra_bytes: Bytes::new(),
            sender_bond_generation: Some(BondGeneration::GENESIS),
            objective_equivocation_evidence_delta: vec![],
            finalized_floor: None,
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: hash(102),
                post_state_hash: hash(hash_byte.saturating_add(101)),
                bonds: bonded_validators
                    .into_iter()
                    .map(|validator| Bond {
                        validator,
                        stake: 100,
                    })
                    .collect(),
                bond_generations,
                active_validators,
                block_number: i64::from(seq_num),
            },
            deploys: vec![],
            rejected_deploys: vec![],
            rejected_state_effects: vec![],
            system_deploys: vec![],
            extra_bytes: Bytes::new(),
        },
        justifications,
        sender,
        seq_num,
        sig: Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: "root".to_string(),
        extra_bytes: Bytes::new(),
        finalized_floor_certificate: None,
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
