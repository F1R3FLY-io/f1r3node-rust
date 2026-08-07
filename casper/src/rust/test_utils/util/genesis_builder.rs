// See casper/src/test/scala/coop/rchain/casper/util/GenesisBuilder.scala

use std::collections::HashMap;
use std::path::PathBuf;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use dashmap::DashMap;
use lazy_static::lazy_static;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, F1r3flyState, Header,
};
use prost::bytes;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use tempfile::TempDir;

use crate::rust::errors::CasperError;
use crate::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use crate::rust::genesis::contracts::validator::Validator;
use crate::rust::genesis::contracts::vault::Vault;
use crate::rust::genesis::genesis::Genesis;
use crate::rust::test_utils::util::rholang::resources::{
    block_dag_storage_from_dyn, generate_scope_id, get_shared_lmdb_path, mergeable_store_from_dyn,
    mk_test_rnode_store_manager_shared,
};
use crate::rust::util::construct_deploy::{DEFAULT_PUB, DEFAULT_PUB2, DEFAULT_SEC, DEFAULT_SEC2};
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

type GenesisParameters = (
    Vec<(PrivateKey, PublicKey)>,
    Vec<(PrivateKey, PublicKey)>,
    Genesis,
);

/// Disjoint deterministic key cohorts used by the default test genesis.
///
/// These keys are public test fixtures, never secrets. The cohort tag and index
/// are encoded as a small, non-zero secp256k1 scalar so the result is stable
/// across processes and architectures without carrying duplicate hex tables in
/// the two GenesisBuilder implementations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenesisFixtureKeyCohort {
    Validator,
    FundedVault,
}

pub fn deterministic_genesis_fixture_key_pair(
    cohort: GenesisFixtureKeyCohort,
    index: usize,
) -> (PrivateKey, PublicKey) {
    let index = u64::try_from(index).expect("genesis fixture key index must fit u64");
    let mut scalar = [0u8; 32];
    scalar[23] = match cohort {
        GenesisFixtureKeyCohort::Validator => 1,
        GenesisFixtureKeyCohort::FundedVault => 2,
    };
    scalar[24..].copy_from_slice(&index.to_be_bytes());

    let secret_key = PrivateKey::from_bytes(&scalar);
    let public_key = Secp256k1.to_public(&secret_key);
    (secret_key, public_key)
}

lazy_static! {

  static ref DEFAULT_VALIDATOR_KEY_PAIRS: [(PrivateKey, PublicKey); 4] = {
    std::array::from_fn(|index| {
      deterministic_genesis_fixture_key_pair(GenesisFixtureKeyCohort::Validator, index)
    })
  };

  pub static ref DEFAULT_VALIDATOR_SKS: [PrivateKey; 4] = {
    std::array::from_fn(|i| DEFAULT_VALIDATOR_KEY_PAIRS[i].0.clone())
  };

  pub static ref DEFAULT_VALIDATOR_PKS: [PublicKey; 4] = {
    std::array::from_fn(|i| DEFAULT_VALIDATOR_KEY_PAIRS[i].1.clone())
  };

  static ref DEFAULT_POS_MULTI_SIG_PUBLIC_KEYS: [String; 3] = [
      "04db91a53a2b72fcdcb201031772da86edad1e4979eb6742928d27731b1771e0bc40c9e9c9fa6554bdec041a87cee423d6f2e09e9dfb408b78e85a4aa611aad20c".to_string(),
      "042a736b30fffcc7d5a58bb9416f7e46180818c82b15542d0a7819d1a437aa7f4b6940c50db73a67bfc5f5ec5b5fa555d24ef8339b03edaa09c096de4ded6eae14".to_string(),
      "047f0f0f5bbe1d6d1a8dac4d88a3957851940f39a57cd89d55fe25b536ab67e6d76fd3f365c83e5bfe11fe7117e549b1ae3dd39bfc867d1c725a4177692c4e7754".to_string(),
  ];

  // STATIC CACHE: Shared across all GenesisBuilder instances
  static ref GENESIS_CACHE: DashMap<GenesisParameters, GenesisContext> = DashMap::new();
}

// ═══════════════════════════════════════════════════════════════════════════════════════
// Cache counters — the SIBLING of `casper/tests/util/genesis_builder.rs`'s pair
// ═══════════════════════════════════════════════════════════════════════════════════════
//
// ★ THIS IS THE SECOND INSTANCE, AND IT WAS FOUND BY ASKING "WHAT ELSE HAS THIS SHAPE?"
// RATHER THAN BY IT FAILING.
//
// The `tests/` copy's counters were a pair of process-wide `AtomicU64`s bracketed by
// `genesis_cache_hits_once_the_vault_order_is_stable`, and a neighbour test's genesis build
// landed inside the bracket — *"Got 7 accesses for 6 calls"* under `cargo test`, invisible
// under `cargo nextest run`. That copy is now `thread_local!`.
//
// ⚠ **This copy is LATENT, not live**: it exports no `genesis_cache_stats`, so nothing can
// bracket it and nothing can currently be wrong. It is thread-scoped anyway, for one reason:
// the two builders are a known drift pair (`the_bond_derived_vault_tail_is_sorted_by_rendered_address`
// exists because the unsorted `.chain(bonds.iter()…)` was present in BOTH and the `src/` copy
// is the easy one to miss). Leaving the broken shape here is leaving the defect one
// copy-paste away from being live again.
//
// The process-wide totals are retained for the `println!`, which asks a genuinely
// process-wide question — [`GENESIS_CACHE`] is process-wide, so its hit rate is too.
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

/// PROCESS-WIDE totals, for the `println!` DIAGNOSTIC only.
static CACHE_ACCESSES_TOTAL: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES_TOTAL: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// PER-THREAD accesses. Not exported: this copy has no bracketing caller. If one is ever
    /// added, expose a reader whose NAME states the scope — never a bare `genesis_cache_stats`.
    static CACHE_ACCESSES: Cell<u64> = const { Cell::new(0) };
    /// PER-THREAD misses. Same note.
    static CACHE_MISSES: Cell<u64> = const { Cell::new(0) };
}

/// Bump both scopes' access counters. Two call sites in this copy
/// (`build_genesis_with_parameters` and `build_genesis_with_validators_num`), which is exactly
/// why it is a function: the pair cannot be updated at one site and forgotten at the other.
fn record_cache_access() {
    CACHE_ACCESSES_TOTAL.fetch_add(1, Ordering::SeqCst);
    CACHE_ACCESSES.with(|c| c.set(c.get() + 1));
}

/// Bump both scopes' miss counters and return the PROCESS-WIDE `(misses, accesses)`.
fn record_cache_miss() -> (u64, u64) {
    CACHE_MISSES.with(|c| c.set(c.get() + 1));
    let misses = CACHE_MISSES_TOTAL.fetch_add(1, Ordering::SeqCst) + 1;
    (misses, CACHE_ACCESSES_TOTAL.load(Ordering::SeqCst))
}

pub struct GenesisBuilder {
    vaults: Option<Vec<Vault>>,
}

impl GenesisBuilder {
    pub fn new() -> Self { Self { vaults: None } }

    pub fn with_vaults(mut self, vaults: Vec<Vault>) -> Self {
        self.vaults = Some(vaults);
        self
    }

    pub fn create_bonds(validators: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
        validators
            .into_iter()
            .enumerate()
            .map(|(i, v)| (v, (i as i64) * 2 + 1))
            .collect()
    }

    pub fn build_test_genesis(validator_key_pairs: Vec<(PrivateKey, PublicKey)>) -> BlockMessage {
        let validator_pks: Vec<PublicKey> = validator_key_pairs
            .iter()
            .map(|(_, pk)| pk.clone())
            .collect();

        let bonds_map = Self::create_bonds(validator_pks);

        // Convert to the Bond format used in genesis block
        let bonds: Vec<Bond> = bonds_map
            .into_iter()
            .map(|(public_key, stake)| Bond {
                validator: public_key.bytes.clone(),
                stake,
            })
            .collect();

        let state = F1r3flyState {
            pre_state_hash: bytes::Bytes::new(),
            post_state_hash: bytes::Bytes::new(),
            block_number: 0,
            bonds,
        };

        let body = Body {
            state,
            deploys: vec![],
            rejected_deploys: vec![],
            system_deploys: vec![],
            extra_bytes: bytes::Bytes::new(),
        };

        let header = Header {
            parents_hash_list: vec![],
            timestamp: 0, // Using 0 like in GenesisBuilder
            version: 1,
            extra_bytes: bytes::Bytes::new(),
        };

        BlockMessage {
            block_hash: bytes::Bytes::from(Blake2b256::hash(b"test_genesis".to_vec())), // Create a deterministic hash
            header,
            body,
            justifications: vec![],
            sender: bytes::Bytes::new(),
            seq_num: 0,
            sig: bytes::Bytes::new(),
            sig_algorithm: "secp256k1".to_string(),
            shard_id: "root".to_string(), // Using "root" like in GenesisBuilder
            extra_bytes: bytes::Bytes::new(),
        }
    }

    pub async fn create_genesis(&mut self) -> Result<BlockMessage, CasperError> {
        let context = self.build_genesis_with_parameters(None).await?;
        Ok(context.genesis_block)
    }

    pub fn build_genesis_parameters_with_defaults(
        bonds_function: Option<fn(Vec<PublicKey>) -> HashMap<PublicKey, i64>>,
        validators_num: Option<usize>,
    ) -> GenesisParameters {
        let bonds_function = bonds_function.unwrap_or(Self::create_bonds);
        let validators_num = validators_num.unwrap_or(4);

        Self::build_genesis_parameters(
            DEFAULT_VALIDATOR_KEY_PAIRS
                .iter()
                .take(validators_num)
                .cloned()
                .collect(),
            &bonds_function(
                DEFAULT_VALIDATOR_PKS
                    .iter()
                    .take(validators_num)
                    .cloned()
                    .collect(),
            ),
        )
    }

    pub fn build_genesis_parameters_with_random(
        bonds_function: Option<fn(Vec<PublicKey>) -> HashMap<PublicKey, i64>>,
        validators_num: Option<usize>,
    ) -> GenesisParameters {
        let bonds_function = bonds_function.unwrap_or(Self::create_bonds);
        let validators_num = validators_num.unwrap_or(4);

        // 4 default fixed validators, others are random generated
        let random_validator_key_pairs: Vec<(PrivateKey, PublicKey)> = (5..validators_num)
            .map(|_| Secp256k1.new_key_pair())
            .collect();
        let (_, random_validator_pks): (Vec<PrivateKey>, Vec<PublicKey>) =
            random_validator_key_pairs.iter().cloned().unzip();

        Self::build_genesis_parameters(
            DEFAULT_VALIDATOR_KEY_PAIRS
                .iter()
                .cloned()
                .chain(random_validator_key_pairs.into_iter())
                .collect(),
            &bonds_function(
                DEFAULT_VALIDATOR_PKS
                    .iter()
                    .cloned()
                    .chain(random_validator_pks.into_iter())
                    .collect(),
            ),
        )
    }

    pub fn build_genesis_parameters(
        validator_key_pairs: Vec<(PrivateKey, PublicKey)>,
        bonds: &HashMap<PublicKey, i64>,
    ) -> GenesisParameters {
        let mut genesis_vaults: Vec<(PrivateKey, PublicKey)> = vec![
            (DEFAULT_SEC.clone(), DEFAULT_PUB.clone()),
            (DEFAULT_SEC2.clone(), DEFAULT_PUB2.clone()),
        ];

        for index in 0..validator_key_pairs.len().saturating_sub(2) {
            genesis_vaults.push(deterministic_genesis_fixture_key_pair(
                GenesisFixtureKeyCohort::FundedVault,
                index,
            ));
        }

        // ★★ LOAD-BEARING SORT — the `vaults` ORDER IS CONSENSUS-VISIBLE.
        //
        // `bonds` is a `HashMap`, so `bonds.iter()` yields a per-instance RANDOM order
        // (`RandomState::new()` bumps a thread-local seed for every map constructed, so even
        // two maps with identical contents in ONE process iterate differently). That order
        // is not cosmetic: `VaultsGenerator::create_from_user_vaults`
        // (`casper/src/rust/genesis/contracts/vaults_generator.rs:18-28`) renders this `Vec`
        // POSITIONALLY into Rholang SOURCE TEXT — `("<base58>", <balance>), …` — which
        // becomes `DeployData.term`, is signed, seeds the deploy RNG, and is executed in
        // that order by `compute_genesis`. Nothing downstream re-sorts it. Left unsorted,
        // the test genesis therefore produces a DIFFERENT `post_state_hash` on every build
        // from byte-identical inputs, and `GENESIS_CACHE` (keyed on `GenesisParameters`,
        // which embeds `Genesis.vaults` in order) misses on every call and rebuilds.
        //
        // The key is `to_base58()` deliberately: that string is exactly what is emitted into
        // the deploy term, so ordering by it orders the consensus artifact itself rather
        // than some incidental in-memory representation. Only the bond-derived TAIL is
        // sorted — `genesis_vaults` is an ordered `Vec` whose positions callers rely on.
        //
        // ⚠ This is the SIBLING of the identical sort in
        // `casper/tests/util/genesis_builder.rs`; the two copies must not drift.
        //
        // ⚠ PRODUCTION CONSTRAINT (not a defect, but consensus-visible): outside tests the
        // `vaults` order comes from `wallets.txt` LINE ORDER — `VaultParser::parse`
        // (`casper/src/rust/util/vault_parser.rs:37-49`) pushes one `Vault` per line into a
        // `Vec` and never sorts. It is deterministic per node but NOT canonical across
        // nodes: two operators whose `wallets.txt` list the same vaults in a different order
        // compute a DIFFERENT genesis (different `post_state_hash`, hence a different
        // `block_hash`) and will not agree in the genesis ceremony. `wallets.txt` is part of
        // the shard's agreed configuration, byte-for-byte, line order included.
        let mut bond_vaults: Vec<Vault> = bonds
            .iter()
            .map(|(pk, _)| {
                // Initial validator vaults contain 0 Rev
                VaultAddress::from_public_key(pk)
                    .map(|vault_address| Vault {
                        vault_address,
                        initial_balance: 0,
                    })
                    .expect("GenesisBuilder: Failed to create rev address")
            })
            .collect();
        bond_vaults.sort_by(|a, b| {
            a.vault_address
                .to_base58()
                .cmp(&b.vault_address.to_base58())
        });

        let vaults: Vec<Vault> = genesis_vaults
            .iter()
            .map(|(_, pk)| Self::predefined_vault(pk))
            .collect::<Vec<Vault>>()
            .into_iter()
            .chain(bond_vaults)
            .collect();

        // ★★ THE SECOND UNORDERED MEMBER OF `GenesisParameters`, and it was found by measurement,
        // not by reading: after sorting `vaults` above, `GENESIS_CACHE` STILL missed 5 times out of
        // 6 accesses. `ProofOfStake::validators` is collected from the same `bonds` `HashMap` and is
        // part of the cache key, so it varied per call on its own.
        //
        // ⚠ Unlike the vaults order, this one does NOT reach `post_state_hash` or `block_hash` —
        // it is masked downstream by the two load-bearing sorts S1
        // (`genesis/contracts/proof_of_stake.rs::initial_bonds`, which orders the rendered
        // `$$initialBonds$$` map) and S2 (`genesis/genesis.rs::bonds_proto`, which orders
        // `F1r3flyState::bonds`). So this sort is NOT a consensus fix and must not be mistaken for
        // one; production still relies entirely on S1/S2, because production builds `validators`
        // from its own `HashMap`s (`engine/approve_block_protocol.rs:167`,
        // `engine/block_approver_protocol.rs:199`) and this file is test-only.
        //
        // What it does fix is the TEST CACHE: `GenesisParameters` is the `DashMap` key, so an
        // unordered member there means logically identical parameters hash differently and every
        // call rebuilds the whole genesis block.
        //
        // The key is `pk.bytes`, chosen to MATCH S1's and S2's key exactly. That makes this sort
        // idempotent with respect to both, so the rendered Rholang and the block body are
        // byte-identical to what they were before — this changes cache behaviour and nothing else.
        //
        // ⚠ This is the SIBLING of the identical sort in `casper/tests/util/genesis_builder.rs`;
        // the two copies must not drift.
        let mut validators: Vec<Validator> = bonds
            .into_iter()
            .map(|(pk, stake)| Validator {
                pk: pk.clone(),
                stake: *stake,
            })
            .collect();
        validators.sort_by(|a, b| a.pk.bytes.cmp(&b.pk.bytes));

        (validator_key_pairs, genesis_vaults, Genesis {
            shard_id: "root".to_string(),
            timestamp: 0,
            proof_of_stake: ProofOfStake {
                minimum_bond: 1,
                maximum_bond: i64::MAX,
                // Epoch length is set to large number to prevent trigger of epoch change
                // in PoS close block method, which causes block merge conflicts
                // - epoch change can be set as a parameter in Rholang tests (e.g. PoSSpec)
                epoch_length: 1000,
                quarantine_length: 50000,
                number_of_active_validators: 100,
                validators,
                pos_multi_sig_public_keys: DEFAULT_POS_MULTI_SIG_PUBLIC_KEYS.to_vec(),
                pos_multi_sig_quorum: DEFAULT_POS_MULTI_SIG_PUBLIC_KEYS.len() as u32 - 1,
                max_cosigners_per_deploy:
                    crate::rust::casper_conf::DEFAULT_MAX_COSIGNERS_PER_DEPLOY,
                initial_phlogiston: crate::rust::casper_conf::DEFAULT_INITIAL_PHLOGISTON,
                epoch_phlogiston: crate::rust::casper_conf::DEFAULT_EPOCH_PHLOGISTON,
            },
            vaults,
            supply: i64::MAX,
            block_number: 0,
            version: 1,
            native_token_name: "F1R3CAP".to_string(),
            native_token_symbol: "F1R3".to_string(),
            native_token_decimals: 8,
        })
    }

    fn predefined_vault(pubkey: &PublicKey) -> Vault {
        Vault {
            vault_address: VaultAddress::from_public_key(pubkey)
                .expect("GenesisBuilder: Failed to create rev address"),
            initial_balance: 9000000,
        }
    }

    pub async fn build_genesis_with_parameters(
        &mut self,
        parameters: Option<GenesisParameters>,
    ) -> Result<GenesisContext, CasperError> {
        let parameters =
            parameters.unwrap_or(Self::build_genesis_parameters_with_defaults(None, None));
        record_cache_access();

        if GENESIS_CACHE.contains_key(&parameters) {
            Ok(GENESIS_CACHE.get(&parameters).unwrap().value().clone())
        } else {
            let context = self.do_build_genesis(&parameters).await?;
            GENESIS_CACHE.insert(parameters, context.clone());
            Ok(context)
        }
    }

    pub async fn build_genesis_with_validators_num(
        &mut self,
        validators_num: usize,
    ) -> Result<GenesisContext, CasperError> {
        let parameters = Self::build_genesis_parameters_with_random(None, Some(validators_num));
        record_cache_access();

        if GENESIS_CACHE.contains_key(&parameters) {
            Ok(GENESIS_CACHE.get(&parameters).unwrap().value().clone())
        } else {
            let context = self.do_build_genesis(&parameters).await?;
            GENESIS_CACHE.insert(parameters, context.clone());
            Ok(context)
        }
    }

    async fn do_build_genesis(
        &mut self,
        parameters: &GenesisParameters,
    ) -> Result<GenesisContext, CasperError> {
        // ⚠ PROCESS-WIDE figures, and they are the right ones HERE: the cache is process-wide,
        // so its hit rate is too. The per-thread counters are for assertions, not for this line.
        let (cache_misses, cache_accesses) = record_cache_miss();
        println!(
            "Genesis block cache miss, building a new genesis. Cache misses: {} / {} ({:.2}%) cache accesses (process-wide).",
            cache_misses,
            cache_accesses,
            (cache_misses as f64 / cache_accesses as f64) * 100.0
        );

        let (validator_key_pairs, genesis_vaults, mut genesis_parameters) = parameters.clone();

        // If vaults were provided via with_vaults(), use them instead of default vaults
        if let Some(ref vaults) = self.vaults {
            genesis_parameters.vaults = vaults.clone();
        }

        // With shared LMDB, we don't need to create a separate directory for storage.
        // Use the shared LMDB path instead. The directory is kept for backward compatibility
        // and logging purposes, but actual LMDB storage is in the shared environment.
        let storage_directory_path = get_shared_lmdb_path();
        // No TempDir guard needed since we're using the shared environment

        // Generate a shared RSpace scope_id that will be used by all nodes in this test
        let rspace_scope_id = generate_scope_id();

        // Build genesis in a scoped block to ensure LMDB handles are closed
        let genesis = {
            // Create genesis with rspace_scope_id so TestNodes can share the same RSpace stores
            let mut kvs_manager = mk_test_rnode_store_manager_shared(rspace_scope_id.clone());
            let r_store = (&mut *kvs_manager)
                .r_space_stores()
                .await
                .expect("Failed to create RSpaceStore");

            let m_store = mergeable_store_from_dyn(&mut *kvs_manager).await?;
            let mut runtime_manager = RuntimeManager::create_with_store(
                r_store,
                m_store,
                std::sync::Arc::new(Genesis::default_mergeable_tags()),
                rholang::rust::interpreter::external_services::ExternalServices::noop(),
            );

            let genesis =
                Genesis::create_genesis_block(&mut runtime_manager, &genesis_parameters).await?;
            let block_store = KeyValueBlockStore::create_from_kvm(&mut *kvs_manager).await?;
            block_store.put(genesis.block_hash.clone(), &genesis)?;

            let block_dag_storage = block_dag_storage_from_dyn(&mut *kvs_manager).await?;
            block_dag_storage.insert(
                &genesis,
                block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Approved,
            )?;

            genesis
            // ← kvs_manager drops here, closing LMDB handles
        };

        // Return context with scope_id.
        // With shared LMDB, storage_directory points to the shared environment path.
        // No TempDir guard needed since we're using the shared environment.
        Ok(GenesisContext {
            genesis_block: genesis,
            validator_key_pairs,
            genesis_vaults,
            storage_directory: storage_directory_path,
            rspace_scope_id,
            _tempdir_guard: None, // No tempdir guard needed with shared LMDB
        })
    }
}

pub struct GenesisContext {
    pub genesis_block: BlockMessage,
    pub validator_key_pairs: Vec<(PrivateKey, PublicKey)>,
    pub genesis_vaults: Vec<(PrivateKey, PublicKey)>,
    pub storage_directory: PathBuf,
    /// The shared RSpace scope_id for all nodes in the same test.
    /// All TestNodes in this test share the same RSpace stores to see each other's state.
    pub rspace_scope_id: String,
    // Keep TempDir guard alive to prevent auto-cleanup while context is in use
    // Only the original context holds Some(tempdir), clones have None
    #[cfg(feature = "test-utils")]
    _tempdir_guard: Option<TempDir>,
    #[cfg(not(feature = "test-utils"))]
    _tempdir_guard: Option<()>, // Placeholder when feature is disabled
}

// Manual Clone implementation: clones don't get the TempDir guard
// This is intentional - only the original context (stored in cache) keeps the directory alive
impl Clone for GenesisContext {
    fn clone(&self) -> Self {
        Self {
            genesis_block: self.genesis_block.clone(),
            validator_key_pairs: self.validator_key_pairs.clone(),
            genesis_vaults: self.genesis_vaults.clone(),
            storage_directory: self.storage_directory.clone(),
            rspace_scope_id: self.rspace_scope_id.clone(),
            _tempdir_guard: None, // Clones don't own the directory
        }
    }
}

impl GenesisContext {
    pub fn validator_sks(&self) -> Vec<PrivateKey> {
        self.validator_key_pairs
            .iter()
            .map(|(sk, _)| sk.clone())
            .collect()
    }

    pub fn validator_pks(&self) -> Vec<PublicKey> {
        self.validator_key_pairs
            .iter()
            .map(|(_, pk)| pk.clone())
            .collect()
    }

    pub fn genesis_vaults_sks(&self) -> Vec<PrivateKey> {
        self.genesis_vaults
            .iter()
            .map(|(sk, _)| sk.clone())
            .collect()
    }

    pub fn genesis_vaults_pks(&self) -> Vec<PublicKey> {
        self.genesis_vaults
            .iter()
            .map(|(_, pk)| pk.clone())
            .collect()
    }
}
