//! ★★ **The test genesis's `vaults` order was RANDOM, and that order is CONSENSUS-VISIBLE.**
//!
//! This file is the guard for the defect and, more importantly, the *proof that the guard is not
//! vacuous*. Both are needed, because "N builds agreed" is a statement no one should trust on its
//! own: it is exactly what a build that never varied in the first place would also report.
//!
//! # The mechanism, end to end
//!
//! ```text
//!   create_bonds(..) ─→ HashMap<PublicKey, i64>          ⚠ UNORDERED
//!            │
//!            │  bonds.iter()          ← random per map INSTANCE (RandomState re-seeds per map)
//!            ▼
//!   Genesis.vaults: Vec<Vault>        ← positional, and this was the arrival order
//!            │
//!            │  VaultsGenerator::create_from_user_vaults   (vaults_generator.rs:18-28)
//!            ▼
//!   Rholang SOURCE TEXT:  match [("<base58>", 9000000), ("<base58>", 0), …] { vaults => … }
//!            │
//!            │  CompiledRholangSource → DeployData.term → Signed<DeployData>
//!            ▼
//!   deploy signature ─→ compute_genesis executes the deploys IN LIST ORDER
//!            │                                          └─ and the deploy RNG is seeded from it
//!            ▼
//!   post_state_hash  ─→ F1r3flyState ─→ Body ─→ block_hash
//! ```
//!
//! Nothing between the `HashMap` and the hash re-canonicalises the sequence. So the test genesis
//! computed a different `post_state_hash` on every build from byte-identical sources, and
//! `GENESIS_CACHE` — keyed on the whole `GenesisParameters` tuple, which embeds `Genesis.vaults`
//! *in order* — missed on every call and rebuilt genesis from scratch.
//!
//! The fix sorts the bond-derived vault tail by `vault_address.to_base58()` in **both** copies of
//! the builder (`casper/tests/util/genesis_builder.rs` and
//! `casper/src/rust/test_utils/util/genesis_builder.rs`). The sort key is the rendered base58
//! string deliberately: that string is literally what lands in the deploy term, so the ordering is
//! imposed on the consensus artifact rather than on some incidental in-memory representation.
//!
//! # ⚠ Why [`vault_order_changes_the_signed_genesis_deploy`] exists
//!
//! [`vault_order_is_stable_across_independent_parameter_builds`] would pass trivially if vault
//! order did not matter, or if `create_bonds` happened to be order-stable. The control removes both
//! escapes **without any appeal to probability**: it feeds two hand-written permutations of the
//! *same* vault multiset through the *same* public deploy builder and shows the emitted term and
//! its signature differ. Order-sensitivity is therefore a demonstrated property of the pipeline,
//! not an assumption, and stability under the real API is a real result.
//!
//! An earlier design for that control drew N `HashMap`s and asserted "at least two distinct orders
//! appear". That is a *probabilistic* refusal — with four validators there are only 24 orders — and
//! a gate that can flake is worse than no gate. The permutation form is exact.

use std::collections::BTreeSet;

use casper::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use casper::rust::genesis::contracts::standard_deploys;
use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::test_utils::util::genesis_builder::{
    deterministic_genesis_fixture_key_pair, GenesisFixtureKeyCohort,
};
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use crypto::rust::public_key::PublicKey;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::{
    genesis_cache_delta_across_foreign_thread, genesis_cache_stats_this_thread, GenesisBuilder,
};
use crate::util::rholang::resources;
use crate::util::rholang::resources::generate_scope_id;

/// How many independent builds each cell draws. Six is the floor the work item asked for; the cheap
/// cells use more because they cost microseconds and more draws is strictly more evidence.
const N_BUILDS: usize = 6;
const N_CHEAP_BUILDS: usize = 24;
const DEFAULT_GENESIS_POST_STATE_HASH: &str =
    "28ca4bcf56ec1987f14d1217272d1962c0032b2bbd0e825039c575df20a925ca";

/// The consensus-visible projection of a vault list: the base58 addresses **in order**, paired with
/// their balances. This is what `VaultsGenerator` renders, so two vault lists with this projection
/// equal are indistinguishable to genesis, and two that differ produce different genesis.
fn vault_order_fingerprint(vaults: &[Vault]) -> Vec<(String, u64)> {
    vaults
        .iter()
        .map(|v| (v.vault_address.to_base58(), v.initial_balance))
        .collect()
}

#[test]
fn default_genesis_uses_the_shared_deterministic_keyspace() {
    let (validators, funded_vaults, genesis) =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);

    assert_eq!(
        validators.len(),
        4,
        "the default fixture must have four validators"
    );
    for (index, (_, actual_pk)) in validators.iter().enumerate() {
        let (_, expected_pk) =
            deterministic_genesis_fixture_key_pair(GenesisFixtureKeyCohort::Validator, index);
        assert_eq!(
            actual_pk, &expected_pk,
            "validator fixture key {index} drifted"
        );
    }
    let mut expected_validator_pks: Vec<PublicKey> = (0..validators.len())
        .map(|index| {
            deterministic_genesis_fixture_key_pair(GenesisFixtureKeyCohort::Validator, index).1
        })
        .collect();
    expected_validator_pks.sort_by(|left, right| left.bytes.cmp(&right.bytes));
    assert_eq!(
        genesis
            .proof_of_stake
            .validators
            .iter()
            .map(|validator| validator.pk.clone())
            .collect::<Vec<_>>(),
        expected_validator_pks,
        "the deterministic validator cohort did not reach Genesis's canonically sorted validators"
    );

    assert_eq!(
        funded_vaults.len(),
        4,
        "two fixed plus two cohort-funded vaults"
    );
    for (index, (_, actual_pk)) in funded_vaults.iter().skip(2).enumerate() {
        let (_, expected_pk) =
            deterministic_genesis_fixture_key_pair(GenesisFixtureKeyCohort::FundedVault, index);
        assert_eq!(
            actual_pk, &expected_pk,
            "funded-vault fixture key {index} drifted"
        );
        assert!(
            genesis.vaults.iter().any(|vault| {
                vault.initial_balance == 9_000_000
                    && vault.vault_address
                        == VaultAddress::from_public_key(&expected_pk)
                            .expect("fixture vault address")
            }),
            "funded-vault fixture key {index} did not reach the Genesis value"
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The guard
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **The test genesis's vault order must be identical across independent parameter builds.**
///
/// Watched RED before the fix: with `bonds.iter()` chained unsorted, this reported many distinct
/// orders out of `N_CHEAP_BUILDS` draws — the `HashMap` iteration order differs per constructed
/// map, so every call to `build_genesis_parameters_with_defaults` produced a different `Vec<Vault>`
/// from the same four validators.
///
/// This is the cheapest possible statement of the defect: no runtime, no RSpace, no interpreter —
/// just the parameters, which is where the nondeterminism is introduced.
#[test]
fn vault_order_is_stable_across_independent_parameter_builds() {
    let orders: BTreeSet<Vec<(String, u64)>> = (0..N_CHEAP_BUILDS)
        .map(|_| {
            let (_, _, genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
            vault_order_fingerprint(&genesis.vaults)
        })
        .collect();

    assert_eq!(
        orders.len(),
        1,
        "★★ {N_CHEAP_BUILDS} independent builds of the DEFAULT genesis parameters produced \
         {} DISTINCT vault orders. `Genesis.vaults` is rendered positionally into Rholang source \
         text by `VaultsGenerator::create_from_user_vaults`, so each distinct order is a distinct \
         signed deploy and a distinct `post_state_hash`. The bond-derived tail must be sorted \
         where it is built — `casper/tests/util/genesis_builder.rs` AND \
         `casper/src/rust/test_utils/util/genesis_builder.rs`. Orders seen: {:#?}",
        orders.len(),
        orders,
    );
}

/// ★★ **THE CONTROL: vault ORDER, not vault CONTENT, decides the signed deploy.**
///
/// Two hand-written permutations of one vault multiset, through the public deploy builder the
/// genesis path itself uses (`standard_deploys::vaults_generator`, called from
/// `Genesis::default_blessed_terms_with_timestamp`). Deterministic in both directions:
///
/// - the reversed permutation MUST produce a different `term` and a different `sig`;
/// - the identical permutation MUST produce byte-identical `term` and `sig`.
///
/// The second half also pins that the deploy builder has no hidden per-call entropy, which is what
/// makes the first half attributable to order alone.
#[test]
fn vault_order_changes_the_signed_genesis_deploy() {
    // Four distinguishable addresses. The hex must be a valid uncompressed secp256k1 encoding
    // shape for `from_public_key`, which is why these are the 130-char repeats used elsewhere in
    // the genesis suites (`pos_spec.rs:27`).
    let vaults: Vec<Vault> = ["1", "2", "3", "4"]
        .iter()
        .enumerate()
        .map(|(i, d)| Vault {
            vault_address: VaultAddress::from_public_key(&PublicKey::from_bytes(
                &hex::decode(d.repeat(130)).expect("vault key hex must decode"),
            ))
            .expect("VaultAddress::from_public_key must succeed for a well-formed key"),
            initial_balance: (i as u64 + 1) * 1000,
        })
        .collect();

    let reversed: Vec<Vault> = vaults.iter().rev().cloned().collect();

    let mk = |vs: Vec<Vault>| {
        standard_deploys::vaults_generator(vs, i64::MAX, 1565818101792, true, "root")
    };

    let forward = mk(vaults.clone());
    let backward = mk(reversed);
    let forward_again = mk(vaults.clone());

    // ── Half 1: order is load-bearing. ──────────────────────────────────────────────────────────
    assert_ne!(
        forward.data.term, backward.data.term,
        "★★ reversing the vault list MUST change the rendered deploy term. If it does not, \
         `VaultsGenerator` has begun canonicalising internally and the ordering guard above is \
         measuring nothing — re-derive this whole file before trusting it.",
    );
    assert_ne!(
        forward.sig, backward.sig,
        "★★ a different term MUST yield a different signature: the signature is over the term, and \
         it is the signature that travels in the block. This is the step that makes vault ORDER a \
         consensus fact rather than a formatting detail.",
    );

    // ── Half 2: nothing ELSE varies, so half 1 is attributable to order. ────────────────────────
    assert_eq!(
        forward.data.term, forward_again.data.term,
        "★ the SAME vault order must render the SAME term. A difference here would mean the deploy \
         builder carries per-call entropy (a timestamp, a nonce, a random seed), and then neither \
         half of this cell nor the guard above would be attributable to vault order.",
    );
    assert_eq!(
        forward.sig, forward_again.sig,
        "★ the SAME term must produce the SAME signature — the genesis deploy signature is \
         deterministic (fixed key, fixed timestamp), which is the premise the whole genesis \
         reproducibility argument rests on.",
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// The expensive rung: the hash itself
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// One independent process can only produce one observation. Pinning that
/// observation makes the assertion cross-process: separate test processes must
/// all arrive at the same constant from the same deterministic fixture keys.
#[tokio::test]
async fn default_genesis_post_state_hash_is_pinned_across_processes() {
    let scope_id = generate_scope_id();
    let mut kvs_manager = resources::mk_test_rnode_store_manager_shared(scope_id.clone());
    let m_store = RuntimeManager::mergeable_store(&mut *kvs_manager)
        .await
        .expect("mergeable store");
    let r_store = kvs_manager.r_space_stores().await.expect("rspace stores");
    let runtime_manager = RuntimeManager::create_with_store(
        r_store,
        m_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    );
    let (_, _, genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let block = Genesis::create_genesis_block(&runtime_manager, &genesis)
        .await
        .expect("default genesis must build");
    let actual = hex::encode(&block.body.state.post_state_hash);

    assert_eq!(
        actual, DEFAULT_GENESIS_POST_STATE_HASH,
        "default test genesis moved. The prior process-scoped variance came from lazily generated \
         validator and funded-vault fixture keys; if the fixture inputs intentionally changed, \
         remeasure this golden in multiple independent processes before updating it"
    );

    let _ = std::fs::remove_dir_all(scope_id);
}

/// ★★ **`post_state_hash` must be identical across `N_BUILDS` independent genesis computations.**
///
/// ⚠ **The cache is bypassed deliberately.** Routing this through
/// `GenesisBuilder::build_genesis_with_parameters` would consult `GENESIS_CACHE`, and after the fix
/// calls 2..N are cache HITS returning a *clone of the first result* — so the hashes would be equal
/// by construction and the cell would be vacuous while looking like the strongest evidence in the
/// file. Calling `Genesis::create_genesis_block` directly forces `compute_genesis` to run
/// `N_BUILDS` times for real.
///
/// Watched RED before the fix: distinct `post_state_hash` values, one per build.
#[tokio::test]
async fn genesis_post_state_hash_is_identical_across_independent_builds() {
    let scope_id = generate_scope_id();
    let mut kvs_manager = resources::mk_test_rnode_store_manager_shared(scope_id.clone());

    let m_store = RuntimeManager::mergeable_store(&mut *kvs_manager)
        .await
        .expect("mergeable store");
    let r_store = kvs_manager.r_space_stores().await.expect("rspace stores");

    let runtime_manager = RuntimeManager::create_with_store(
        r_store,
        m_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    );

    // One fingerprint per build, so a failure names WHICH builds disagreed and on what.
    let mut observed: Vec<(String, Vec<(String, u64)>)> = Vec::with_capacity(N_BUILDS);

    for i in 0..N_BUILDS {
        // A FRESH parameter build each iteration — that is the point. Reusing one `Genesis` would
        // only prove `compute_genesis` is a function, which nobody doubted.
        let (_, _, genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
        let vault_order = vault_order_fingerprint(&genesis.vaults);

        let block = Genesis::create_genesis_block(&runtime_manager, &genesis)
            .await
            .unwrap_or_else(|e| panic!("genesis build {i} must succeed: {e:?}"));

        observed.push((
            hex::encode(&block.body.state.post_state_hash),
            vault_order,
        ));
    }

    let distinct_hashes: BTreeSet<&String> = observed.iter().map(|(h, _)| h).collect();
    let distinct_orders: BTreeSet<&Vec<(String, u64)>> = observed.iter().map(|(_, o)| o).collect();

    assert_eq!(
        distinct_hashes.len(),
        1,
        "★★ {N_BUILDS} independent genesis computations produced {} DISTINCT `post_state_hash` \
         values from byte-identical sources. Genesis is not reproducible, which means two nodes \
         running this shard configuration cannot agree. Distinct vault orders seen alongside: {}. \
         Full observations (hash, vault order): {:#?}",
        distinct_hashes.len(),
        distinct_orders.len(),
        observed,
    );

    // ★ The attribution rung: if the hashes agree it should be BECAUSE the inputs agree. Hashes
    // agreeing while the vault orders differ would mean the vault order is not actually reaching
    // the hash, and then this cell is not guarding what its name claims.
    assert_eq!(
        distinct_orders.len(),
        1,
        "★★ the `post_state_hash` values agreed but the vault ORDERS did not ({} distinct). Either \
         `VaultsGenerator` now canonicalises internally — in which case the ordering fix is \
         redundant and this file needs re-deriving — or the vaults deploy is no longer reaching \
         `compute_genesis` at all, which would be a much larger defect. Observations: {:#?}",
        distinct_orders.len(),
        observed,
    );

    let _ = std::fs::remove_dir_all(&scope_id);
}

/// ★★ **THE DISCRIMINATING EXPERIMENT: does the surviving genesis instability follow the PROCESS
/// or the RSPACE SCOPE?**
///
/// # The two rows that could not answer it, and why
///
/// The genesis `post_state_hash` had been measured twice, with opposite results:
///
/// | measurement | result |
/// |---|---|
/// | 6 independent builds in ONE process | **agree** — 1 distinct value ([`genesis_post_state_hash_is_identical_across_independent_builds`]) |
/// | 4 independent nextest PROCESSES | **DISAGREE** — 4 distinct values |
///
/// ⚠ Those two rows differ in **two** variables at once, so neither can attribute anything. Each
/// nextest process got a fresh process *and* a fresh `generate_scope_id()`, while the six in-process
/// builds shared the process, the scope, the store manager, **and one `RuntimeManager` instance**.
/// A design that varies two things and observes one difference has measured their conjunction.
///
/// # What the existing data DOES settle, which is the useful part
///
/// `evaluate_with_term` seeds its RNG with `Blake2b512Random::create_from_length(128)`, which is
/// `rand::thread_rng().fill(…)` (`blake2b512_random.rs:91`) and therefore draws FRESH entropy on
/// every call. If that were the explanation, the six same-process builds would have DISAGREED. They
/// agreed. ⇒ **per-call randomness is refuted**, and so is per-deploy RSpace event-log ordering
/// (#171 S3) for exactly the same reason. The surviving signature is *state that is constant across
/// builds sharing a process **or** a scope, and varies otherwise* — which points at store/history
/// state rather than at any RNG.
///
/// # This cell, which varies EXACTLY ONE of them
///
/// Six builds in **one** process, each with a **fresh** `generate_scope_id()` — and therefore a
/// fresh store manager, fresh RSpace stores, and a fresh `RuntimeManager`. The process is held
/// constant; the scope is not. So the outcome reads directly:
///
/// ```text
///        ┌──────────────── one process ────────────────┐
///        │  scope₁   scope₂   scope₃  …  scope₆        │      AGREE  ⇒ the varying thing is
///        │    │        │        │          │           │             PROCESS-scoped state, and
///        │  store₁   store₂   store₃  …  store₆        │             the 4-process disagreement
///        │    ▼        ▼        ▼          ▼           │             is a HARNESS fact
///        │   hash₁    hash₂    hash₃  …  hash₆         │
///        └─────────────────────────────────────────────┘      DIFFER ⇒ the varying thing is
///                                                                    SCOPE/STORE-derived, and
///                                                                    #171 S3 is LIVE again
/// ```
///
/// # ★★★ THE RESULT — MEASURED: **AGREE**, so the RSPACE SCOPE is EXCLUDED
///
/// Six builds, six distinct `generate_scope_id()`s, one process: **1 distinct `post_state_hash`**.
/// PASS in **76.63 s**.
///
/// | rows, now three, and only two variables between them | result |
/// |---|---|
/// | 6 builds, ONE process, ONE scope | agree — 1 distinct |
/// | 6 builds, ONE process, SIX scopes ← **this cell** | **agree — 1 distinct** |
/// | 4 builds, FOUR processes, four scopes | DISAGREE — 4 distinct |
///
/// Rows 1 and 2 differ in the scope alone and agree; rows 2 and 3 differ in the process alone and
/// disagree. ⇒ **the varying state is PROCESS-scoped — randomised once per process and then reused
/// — and it is NOT derived from the RSpace scope, the store manager, or the `RuntimeManager`.**
/// #171's S3 (per-deploy RSpace event-log order) stays refuted: a fresh store per build did not move
/// the hash.
///
/// ⚠ **Why that agreement is real work and not a cache serving five clones — MEASURED, because the
/// obvious objection to any "N builds agreed" claim is that N−1 of them never ran.** This cell
/// bypasses `GENESIS_CACHE` the same way its sibling does, by calling `Genesis::create_genesis_block`
/// directly, and the **wall time proves the bypass**: 76.63 s for six builds is 12.8 s per build,
/// against the sibling's 88 s / 6 = 14.7 s per build. Five cache hits would have returned in
/// microseconds and the total would sit near one build's cost, not six. So six genesis computations
/// were actually performed.
///
/// At the time this cell landed it did not identify *which* per-process value varied. That remaining
/// attribution is now closed by [`default_genesis_uses_the_shared_deterministic_keyspace`]: the
/// default builder's validator and funded-vault key cohorts were initialized by
/// `Secp256k1::new_key_pair()` in `lazy_static!` values. After replacing them with the shared
/// deterministic fixture keyspace,
/// [`default_genesis_post_state_hash_is_pinned_across_processes`] passed in four independent
/// processes at `28ca4bcf56ec1987…20a925ca` (14.58–14.82 s each).
///
/// # ⚠ Why asserting agreement is the CORRECT gate and not merely the convenient one
///
/// Two validators computing genesis for the same shard have different store paths, different LMDB
/// files, and no shared process. If a fresh RSpace scope can move `post_state_hash`, they cannot
/// agree on genesis — so "a fresh scope must not move the hash" is a real consensus requirement, and
/// a RED here is a genuine defect statement rather than an artifact of the instrument.
///
/// # ⚠ What this cell deliberately does NOT do
///
/// It reads nothing but `block.body.state.post_state_hash`. Inspecting the tuplespace per channel
/// would be self-defeating: `HotStore::get_data` (`hot_store.rs:354`) takes a **write** lock and
/// history-fills, and `changes()` then emits that fill as a store action — so the obvious
/// "read-only" inspection API MUTATES the state that becomes the checkpoint. *"It only reads" is a
/// property of the API, not of reading.*
#[tokio::test]
async fn genesis_post_state_hash_is_identical_across_independent_rspace_scopes() {
    // (scope_id, post_state_hash, vault order) per build, so a failure names WHICH scope diverged.
    let mut observed: Vec<(String, String, Vec<(String, u64)>)> = Vec::with_capacity(N_BUILDS);

    for i in 0..N_BUILDS {
        // ★ THE ONE VARIED VARIABLE. A fresh scope per build ⇒ fresh scoped LMDB database names,
        // hence a fresh store manager, fresh RSpace stores and a fresh `RuntimeManager`. The
        // existing cell hoists all four out of the loop; that is the entire difference.
        let scope_id = generate_scope_id();
        let mut kvs_manager = resources::mk_test_rnode_store_manager_shared(scope_id.clone());

        let m_store = RuntimeManager::mergeable_store(&mut *kvs_manager)
            .await
            .expect("mergeable store");
        let r_store = kvs_manager.r_space_stores().await.expect("rspace stores");

        let runtime_manager = RuntimeManager::create_with_store(
            r_store,
            m_store,
            std::sync::Arc::new(Genesis::default_mergeable_tags()),
            rholang::rust::interpreter::external_services::ExternalServices::noop(),
        );

        // Fresh parameters too, matching the sibling cell exactly so the two differ in ONE thing.
        let (_, _, genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
        let vault_order = vault_order_fingerprint(&genesis.vaults);

        let block = Genesis::create_genesis_block(&runtime_manager, &genesis)
            .await
            .unwrap_or_else(|e| panic!("genesis build {i} must succeed: {e:?}"));

        observed.push((
            scope_id,
            hex::encode(&block.body.state.post_state_hash),
            vault_order,
        ));
    }

    let distinct_scopes: BTreeSet<&String> = observed.iter().map(|(s, _, _)| s).collect();
    let distinct_hashes: BTreeSet<&String> = observed.iter().map(|(_, h, _)| h).collect();
    let distinct_orders: BTreeSet<&Vec<(String, u64)>> =
        observed.iter().map(|(_, _, o)| o).collect();

    // ★ NON-VACUITY FLOOR, asserted FIRST: if the scopes were not actually distinct this cell has
    // silently degenerated into a duplicate of the sibling and discriminates nothing.
    assert_eq!(
        distinct_scopes.len(),
        N_BUILDS,
        "★ FLOOR: {N_BUILDS} builds produced only {} distinct scope ids, so the ONE variable this \
         cell exists to vary did not vary and its result cannot attribute anything.",
        distinct_scopes.len(),
    );

    // ★ The attribution rung, same as the sibling: agreement must be BECAUSE the inputs agreed.
    assert_eq!(
        distinct_orders.len(),
        1,
        "★★ the vault ORDERS differ across builds ({} distinct), so any hash result here is \
         confounded by input variation and this cell cannot discriminate scope from process. \
         Observations: {:#?}",
        distinct_orders.len(),
        observed,
    );

    assert_eq!(
        distinct_hashes.len(),
        1,
        "★★★ {N_BUILDS} genesis computations in ONE process with {N_BUILDS} DISTINCT RSpace scopes \
         produced {} DISTINCT `post_state_hash` values from identical parameters and identical \
         vault order.\n\
         ⇒ ATTRIBUTION: the surviving genesis instability is SCOPE/STORE-derived, not merely \
         process-scoped. Two validators with different store paths cannot agree on genesis, and \
         #171's S3 (per-deploy RSpace event-log order) is LIVE again — the six-builds-agree row \
         refuted it only under a SHARED scope.\n\
         Observations (scope, hash, vault order): {:#?}",
        distinct_hashes.len(),
        observed,
    );

    for (scope_id, _, _) in &observed {
        let _ = std::fs::remove_dir_all(scope_id);
    }
}

/// ★ **Independent evidence, from a mechanism that was never designed as a determinism check:
/// `GENESIS_CACHE` now HITS.**
///
/// The cache is keyed on the whole `GenesisParameters` tuple, and that tuple embeds
/// `Genesis.vaults` **in order**. So the cache is an oracle for parameter stability that predates
/// this work item and cannot have been tuned to agree with it: with the vault tail unsorted, two
/// calls with logically identical parameters hashed to different keys and every call was a MISS
/// (`do_build_genesis` printing "Cache misses: N / N (100.00%)"); with the tail sorted, the first
/// call misses and the rest hit.
///
/// ⚠ The cache is a process-wide `static` shared with every other test in this binary, so the
/// absolute counters are not this cell's to predict. What IS this cell's to predict is the
/// **delta**: `N_BUILDS` calls must add at most ONE miss.
///
/// ⚠⚠ **AND THE DELTA USED TO BE UNTRUSTWORTHY, INTERMITTENTLY.** The counters were a pair of
/// process-wide `AtomicU64`s, and libtest runs one binary's tests concurrently on many threads
/// of ONE process — so a NEIGHBOUR's `build_genesis_with_parameters` landed inside this
/// bracket. The reported failure was *"Got 7 accesses for 6 calls"*, and it was invisible under
/// `cargo nextest run` (one process per test) while live under `cargo test`. The counters are
/// now `thread_local!`; see [`genesis_cache_stats_this_thread`] for the mechanism and
/// [`a_foreign_threads_genesis_build_stays_out_of_this_threads_bracket`] for the DETERMINISTIC
/// guard that replaces re-running the race.
///
/// ★ The irony is the useful part, so it is recorded rather than smoothed over: this cell
/// arrived with `8064d4b6`, whose subject says the genesis instability *"follows the PROCESS,
/// not the RSPACE SCOPE"* — and its own instrument was then broken by process-shared state.
///
/// ⚠⚠ **`None`, NOT `Some(parameters.clone())` — and this was measured, not reasoned.** The first
/// version of this cell built the parameters once and passed `Some(parameters.clone())` to every
/// call. That version **PASSED WITH BOTH SORTS DISABLED**: cloning one tuple makes the cache key
/// identical by construction, so it tested `DashMap`'s ability to find a key it had just inserted
/// and said nothing whatever about ordering. Passing `None` reproduces what the real callers
/// do — `build_genesis_with_parameters` falls through to
/// `build_genesis_parameters_with_defaults(None, None)`, constructing a FRESH `bonds` `HashMap`
/// per call — which is precisely the path on which every call used to miss.
#[tokio::test]
async fn genesis_cache_hits_once_the_vault_order_is_stable() {
    let mut builder = GenesisBuilder::new();

    let (accesses_before, misses_before) = genesis_cache_stats_this_thread();

    for i in 0..N_BUILDS {
        // `None` ⇒ parameters rebuilt from scratch inside the call. That is the whole point.
        builder
            .build_genesis_with_parameters(None)
            .await
            .unwrap_or_else(|e| panic!("cached genesis build {i} must succeed: {e:?}"));
    }

    let (accesses_after, misses_after) = genesis_cache_stats_this_thread();
    let accesses = accesses_after - accesses_before;
    let misses = misses_after - misses_before;

    assert_eq!(
        accesses, N_BUILDS as u64,
        "★ each `build_genesis_with_parameters` call must register exactly one cache access, \
         otherwise the miss delta below cannot be interpreted. Got {accesses} accesses for \
         {N_BUILDS} calls.",
    );

    assert!(
        misses <= 1,
        "★★ {N_BUILDS} calls that each rebuild the DEFAULT parameters caused {misses} cache \
         MISSES, out of {accesses} accesses. `GENESIS_CACHE` is keyed on `GenesisParameters`, \
         which embeds `Genesis.vaults` in order, so more than one miss means logically identical \
         parameters are still hashing differently — the vault (or bonds) order is not stable. \
         This is also the performance symptom: every miss rebuilds the whole genesis block. \
         Measured at 6/6 (100%) with the two `bond_vaults.sort_by` calls disabled.",
    );
}

/// ★★ **A FOREIGN THREAD'S GENESIS BUILD MUST STAY OUT OF THIS THREAD'S BRACKET — the
/// deterministic replacement for re-running a scheduler race.**
///
/// This is the guard for the counter-scope defect, and it is written this way on purpose.
/// `genesis_cache_hits_once_the_vault_order_is_stable` failed only when the libtest scheduler
/// happened to place a neighbour's genesis build inside its bracket: the failure was real
/// (*"Got 7 accesses for 6 calls"*) and reproducible in the full suite, but a filtered run of
/// this module passes, so "re-run it and see" is not evidence about the counter. **A test that
/// depends on an interleaving is not a test of the property.**
///
/// So the interleaving is *constructed*: one genesis build is performed on a freshly spawned OS
/// thread, joined, and the calling thread's counter delta across it is asserted to be zero. That
/// is exactly the property the bracket needs, stated without a race.
///
/// | counters | delta this asserts | verdict |
/// |---|---|---|
/// | process-wide `AtomicU64` (before) | `(1, 0)` or `(1, 1)` — the foreign build's own counts | **FAILS** |
/// | `thread_local! { Cell<u64> }` (after) | `(0, 0)` | passes |
///
/// ★ It is invariant under reverting the fix: change the counters back to `static AtomicU64`
/// and this goes RED regardless of how the scheduler behaves, because there is no scheduler
/// involved.
///
/// ⚠ The build must be a REAL one, not a stub — it is asserted to have happened by reading the
/// foreign thread's own delta through a channel. A no-op "build" would make the zero delta
/// vacuous, which is the same shape of hole the `Some(parameters.clone())` version of the cell
/// above fell into.
#[test]
fn a_foreign_threads_genesis_build_stays_out_of_this_threads_bracket() {
    let (foreign_tx, foreign_rx) = std::sync::mpsc::channel::<(u64, u64)>();

    let local_delta = genesis_cache_delta_across_foreign_thread(move || {
        let (before_accesses, before_misses) = genesis_cache_stats_this_thread();
        // A current-thread runtime, matching every `#[tokio::test]` caller, so the counter
        // increments land on THIS thread.
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("foreign thread runtime")
            .block_on(async {
                GenesisBuilder::new()
                    .build_genesis_with_parameters(None)
                    .await
                    .expect("the foreign thread's genesis build must succeed");
            });
        let (after_accesses, after_misses) = genesis_cache_stats_this_thread();
        foreign_tx
            .send((
                after_accesses - before_accesses,
                after_misses - before_misses,
            ))
            .expect("the foreign thread must report its own delta");
    });

    let foreign_delta = foreign_rx
        .recv()
        .expect("the foreign thread must have reported before it was joined");

    // ── NON-VACUITY: the foreign thread really did build ─────────────────────────────────
    assert_eq!(
        foreign_delta.0, 1,
        "VACUOUS GUARD: the foreign thread recorded {} accesses, not 1, so it did not perform \
         exactly one genesis build and a zero LOCAL delta proves nothing about scoping",
        foreign_delta.0
    );

    // ── THE CLAIM ────────────────────────────────────────────────────────────────────────
    assert_eq!(
        local_delta,
        (0, 0),
        "★★ THE CACHE COUNTERS ARE NOT THREAD-SCOPED. A genesis build performed entirely on a \
         FOREIGN thread moved this thread's counters by {local_delta:?}. Every bracketed \
         measurement in this file is then perturbable by any neighbouring test that builds \
         genesis — which is the `c4b23376` class, and the reason \
         `genesis_cache_hits_once_the_vault_order_is_stable` reported 'Got 7 accesses for 6 \
         calls' in-suite while passing alone. The counters must be `thread_local!`, not \
         `static`; serialising this file with a `Mutex` would fix only this file and would cost \
         ~13 s per genesis build in wall clock."
    );
}

/// ★ **The two builder copies must not drift.**
///
/// `casper/tests/util/genesis_builder.rs` and `casper/src/rust/test_utils/util/genesis_builder.rs`
/// carry the same `build_genesis_parameters`, and the unsorted `.chain(bonds.iter()…)` was present
/// in BOTH — the second is easy to miss because it lives under `src/`, not `tests/`. There is no
/// way to call the `src` copy's function and compare directly (they build from different static key
/// pairs), so what this cell can check is the property both must satisfy: the bond-derived tail is
/// sorted by rendered address.
///
/// The head is deliberately NOT required to be sorted: `genesis_vaults` is an ordered `Vec`
/// (`DEFAULT_PUB`, `DEFAULT_PUB2`, then extras) whose positions callers index into.
#[test]
fn the_bond_derived_vault_tail_is_sorted_by_rendered_address() {
    let (_, genesis_vaults, genesis) =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);

    let head_len = genesis_vaults.len();
    assert!(
        genesis.vaults.len() > head_len,
        "★ the fixture must actually HAVE a bond-derived tail, or this cell checks an empty slice \
         and passes for the wrong reason. vaults={} head={head_len}",
        genesis.vaults.len(),
    );

    let tail: Vec<String> = genesis.vaults[head_len..]
        .iter()
        .map(|v| v.vault_address.to_base58())
        .collect();

    let mut sorted = tail.clone();
    sorted.sort();

    assert_eq!(
        tail, sorted,
        "★★ the bond-derived vault tail is NOT sorted by rendered base58 address. That tail comes \
         from `bonds.iter()` over a `HashMap`, so unsorted means run-varying, and run-varying \
         means a run-varying `post_state_hash`.",
    );
}

/// ★★ **`ProofOfStake.validators` was the SECOND unordered member of the cache key — found by
/// measurement, after the vaults fix was already in.**
///
/// The vaults sort alone left `genesis_cache_hits_once_the_vault_order_is_stable` RED at **5 misses
/// out of 6 accesses**. `validators` is collected from the same `bonds` `HashMap` and sits inside
/// `GenesisParameters`, so it varied per call on its own. Both test builders now sort it by
/// `pk.bytes`.
///
/// # ⚠ This is NOT a consensus fix, and the distinction matters
///
/// | | reaches | canonicalised by |
/// |---|---|---|
/// | `vaults` order | `post_state_hash` (rendered deploy text) | **nothing** — hence the fix |
/// | `validators` order | nothing | **S1** `initial_bonds`, **S2** `bonds_proto` |
///
/// Production still depends entirely on S1/S2, because production builds `validators` from its own
/// `HashMap`s (`engine/approve_block_protocol.rs:167`, `engine/block_approver_protocol.rs:199`) and
/// the two sorted builders are test-only. Sorting here fixes the TEST CACHE and nothing else.
///
/// # What this cell pins
///
/// 1. `validators` is sorted at construction (so the cache key is stable), and
/// 2. the sort key **agrees with S1's**, so `initial_bonds` renders byte-identically to before —
///    the change is invisible to genesis.
///
/// ⚠ It deliberately does NOT assert that `validators` *used to be* unsorted: with four validators
/// one draw in 24 is already sorted, so such an assertion would be a probabilistic refusal, exactly
/// the flake this file avoids elsewhere. The RED that establishes the disorder is the cache cell's,
/// which is a 6-draw measurement reported in prose rather than a gate.
#[test]
fn validator_order_is_stable_and_agrees_with_the_downstream_sorts() {
    let (_, _, genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let validators = &genesis.proof_of_stake.validators;

    assert!(
        validators.len() >= 2,
        "★ with fewer than two validators every order is the same order and this cell is vacuous. \
         Got {}",
        validators.len(),
    );

    let keys: Vec<Vec<u8>> = validators.iter().map(|v| v.pk.bytes.to_vec()).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        keys, sorted,
        "★★ `ProofOfStake.validators` is not sorted by `pk.bytes` at construction. It is part of \
         the `GENESIS_CACHE` key, so unsorted means every `build_genesis_with_parameters(None)` \
         call hashes to a fresh key and rebuilds the whole genesis block.",
    );

    // ★ IDEMPOTENCE WITH S1. `initial_bonds` sorts by the same key, so applying our sort first must
    // not change what it renders. This is what makes the new sort provably consensus-neutral: if
    // the keys disagreed, sorting here COULD alter the deploy text and would be a genesis change
    // masquerading as a cache optimisation.
    let mut shuffled = validators.clone();
    shuffled.reverse();
    assert_eq!(
        ProofOfStake::initial_bonds(validators),
        ProofOfStake::initial_bonds(&shuffled),
        "★★ `initial_bonds` rendered DIFFERENTLY for a reversed validator list. S1's `sort_by` is \
         supposed to make it order-insensitive; if it does not, then sorting `validators` in the \
         builders is a CONSENSUS-VISIBLE change and not the cache-only change its comment claims.",
    );
}

/// ★ **S1 keeps `$$initialBonds$$` stable, and that must remain true independently of the builders.**
///
/// `ProofOfStake::initial_bonds` renders the validator set into the Rholang source substituted into
/// `PoS.rhox`, so its output reaches `post_state_hash`. This cell is the standing guard on S1
/// itself: whatever the builders do, the rendered map must be one single string across many builds.
///
/// It is kept separate from the cell above because it survives the builders being changed — it would
/// still catch S1's `sort_by` being deleted even if `validators` were canonicalised at every
/// construction site in the tree, which is the scenario in which S1 looks most like dead code.
#[test]
fn the_rendered_initial_bonds_map_is_stable_across_independent_builds() {
    let rendered: BTreeSet<String> = (0..N_CHEAP_BUILDS)
        .map(|_| {
            let (_, _, genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
            ProofOfStake::initial_bonds(&genesis.proof_of_stake.validators)
        })
        .collect();

    assert_eq!(
        rendered.len(),
        1,
        "★★ `ProofOfStake::initial_bonds` produced {} DISTINCT renderings across \
         {N_CHEAP_BUILDS} builds. That function's `sort_by` (S1, \
         `genesis/contracts/proof_of_stake.rs`) is the ONLY thing standing between a `HashMap`'s \
         iteration order and the `$$initialBonds$$` text substituted into `PoS.rhox` — and hence \
         the `post_state_hash`. If it is producing distinct output, that sort has been weakened or \
         removed. ⚠ The repair is NOT to delete S1 and rely on the builders: PRODUCTION does not go \
         through the builders at all (`engine/approve_block_protocol.rs:167`, \
         `engine/block_approver_protocol.rs:199` each build `validators` from their own \
         `HashMap`), so S1 is production's only canonicaliser. Renderings: {:#?}",
        rendered.len(),
        rendered,
    );

    // Guard against the other way this could pass for the wrong reason.
    let (_, _, genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    assert!(
        genesis.proof_of_stake.validators.len() >= 2,
        "★ with fewer than two validators every order is the same order and this cell is vacuous. \
         Got {}",
        genesis.proof_of_stake.validators.len(),
    );
}
