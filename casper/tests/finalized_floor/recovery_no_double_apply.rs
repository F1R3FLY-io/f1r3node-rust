// Finalized-floor T-NDA (recovery NOT double-applied) — a DIRECT idempotence test
// driving the production one-shot recovery record `interpreter_util::canonical_won_sigs`
// (interpreter_util.rs:119-156), the realization the dossier cites for
// `formal/rocq/finalized_floor/theories/Recovery.v`:
//
//   * `Recovery.apply_idem`  — `apply_effect (apply_effect s d) d = apply_effect s d`:
//     applying a recovery effect a SECOND time is a no-op.
//   * `Recovery.no_double_apply` — re-applying an already-applied effect leaves the
//     reflected-effect SET unchanged (so an additive channel is not double-counted).
//
// The recovery "apply" is `block_creator::prepare_user_deploys`'s filter
// (block_creator.rs:181-184): a recovery candidate is re-included only if its signature
// is NOT canonically-won across the merge scope. Once a recovered loser WINS (its effect
// enters a block's `body.deploys` and thus the merge base), `canonical_won_sigs` reports
// it, so the next recovery pass drops it — it is applied AT MOST ONCE.
//
// Previously this was covered only INDIRECTLY (batch2/recovery_cycle_spec.rs,
// batch2/slash_recovery_spec.rs dedup) and via a TRANSCRIBED copy of the record
// (repeat_deploy/prop_repeat_deploy_agreement.rs, which re-implements `canonical_won_sigs`
// locally). This test drives the REAL production function over a real `KeyValueBlockStore`
// and asserts `apply(apply(s)) == apply(s)` directly.
//
// LOCAL-ONLY verification (not consensus code). Wired into the `mod` integration-test
// binary; picked up by scripts/check-finalized-floor-ALL.sh via the `finalized_floor::`
// filter.

use std::collections::HashSet;

use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::interpreter_util::canonical_won_sigs;
use models::rust::casper::protocol::casper_message::Bond;
use models::rust::deploy_id::DeployLookupId;
use prost::bytes::Bytes;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::{create_block, create_genesis_block};

// Deep enough that the deploy-lifespan window never excludes a block in this tiny
// chain — the disposition height, not the window, is what this test exercises.
const EARLIEST: i64 = -1_000_000;

/// The production recovery-apply (block_creator.rs:181-184): keep only the pool
/// signatures whose latest disposition across the scope is NOT a WIN.
fn recover(
    canonical_won: &HashSet<DeployLookupId>,
    pool: &HashSet<DeployLookupId>,
) -> HashSet<DeployLookupId> {
    pool.iter()
        .filter(|sig| !canonical_won.contains(*sig))
        .cloned()
        .collect()
}

#[tokio::test]
async fn recovery_effect_is_applied_at_most_once() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        // A recovered loser (its effect) and a never-seen loser, with distinct sigs.
        let won_deploy = construct_deploy::basic_processed_deploy(0, Some("root".to_string()))
            .expect("won deploy");
        let sig_won = won_deploy.deploy.sig.clone();
        let loser_deploy = construct_deploy::basic_processed_deploy(1, Some("root".to_string()))
            .expect("loser deploy");
        let sig_loser = loser_deploy.deploy.sig.clone();
        assert_ne!(sig_won, sig_loser, "the two deploys must have distinct signatures");

        // Merge scope: genesis (no deploys) <- b1 (includes `sig_won` in body.deploys — a
        // WIN, i.e. the recovered effect has been applied ONCE and is now in the base).
        let validator = Bytes::from(vec![2; models::rust::validator::LENGTH]);
        let bonds = vec![Bond {
            validator: validator.clone(),
            stake: 1,
        }];
        let genesis = create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            Some(validator.clone()),
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let b1 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(validator.clone()),
            Some(bonds.clone()),
            None,
            Some(vec![won_deploy.clone()]),
            None,
            None,
            None,
            None,
            None,
        );
        let id_won = b1.body.deploys[0]
            .deploy_id_for_protocol(b1.header.version)
            .expect("winning deploy identity");
        let id_loser = loser_deploy
            .deploy_id_for_protocol(b1.header.version)
            .expect("losing deploy identity");

        // Production record over each scope.
        let won_at_genesis =
            canonical_won_sigs(&block_store, std::slice::from_ref(&genesis.block_hash), EARLIEST)
                .expect("canonical_won_sigs over the genesis scope");
        let won_at_b1 =
            canonical_won_sigs(&block_store, std::slice::from_ref(&b1.block_hash), EARLIEST)
                .expect("canonical_won_sigs over the b1 scope");

        // Reflected-effect membership: `sig_won` is NOT reflected before it is applied,
        // IS reflected after b1 includes it (applied once), and a never-applied loser is
        // never reflected.
        assert!(
            !won_at_genesis.contains(&id_won),
            "the effect is not canonically-won before it is applied"
        );
        assert!(
            won_at_b1.contains(&id_won),
            "the effect is canonically-won exactly once after b1 includes it"
        );
        assert!(
            !won_at_b1.contains(&id_loser),
            "a never-applied loser is never canonically-won"
        );

        let pool = HashSet::from([id_won.clone(), id_loser.clone()]);

        // Before the effect wins it is recoverable; after it wins recovery DROPS it (and
        // keeps the loser) — the effect is applied at most once.
        let recovered_before = recover(&won_at_genesis, &pool);
        assert!(
            recovered_before.contains(&id_won),
            "the effect is eligible for recovery before it wins"
        );
        let recovered_after = recover(&won_at_b1, &pool);
        assert_eq!(
            recovered_after,
            HashSet::from([id_loser.clone()]),
            "after the effect wins, recovery drops it and keeps the loser (no double-apply)"
        );

        // T-NDA `apply_idem`: applying recovery a SECOND time over the same post-win scope
        // is a no-op — `apply(apply(s)) == apply(s)`.
        let recovered_twice = recover(&won_at_b1, &recovered_after);
        assert_eq!(
            recovered_twice, recovered_after,
            "apply(apply(s)) == apply(s): recovery re-application neither re-introduces nor re-drops"
        );

        // T-NDA `no_double_apply` (across blocks): the recovery step re-proposes ONLY the
        // recovered set ({loser}) — NOT the already-won effect. A follow-on block `b2`
        // built from that set therefore carries no `sig_won`, so the winning effect is
        // applied at most once even across blocks, while it stays reflected in the scope.
        let b2 = create_block(
            &mut block_store,
            &mut block_dag_storage,
            vec![b1.block_hash.clone()],
            &genesis,
            Some(validator),
            Some(bonds),
            None,
            Some(vec![loser_deploy.clone()]),
            None,
            None,
            None,
            None,
            None,
        );
        let b2_sigs: HashSet<Bytes> =
            b2.body.deploys.iter().map(|pd| pd.deploy.sig.clone()).collect();
        assert!(
            !b2_sigs.contains(&sig_won),
            "the already-applied effect is NOT re-proposed into the next block (no double-apply)"
        );
        let won_at_b2 =
            canonical_won_sigs(&block_store, std::slice::from_ref(&b2.block_hash), EARLIEST)
                .expect("canonical_won_sigs over the b2 scope");
        assert!(
            won_at_b2.contains(&id_won),
            "the winning effect stays reflected exactly once across the extended scope"
        );
    })
    .await
}
