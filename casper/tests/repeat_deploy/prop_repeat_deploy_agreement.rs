// P7 proptests — proposer ↔ validator AGREEMENT on the repeat-deploy expiration
// window. The load-bearing claim: an honest recovery block is NEVER wrongly flagged
// `InvalidRepeatDeploy`, because the proposer and the validator gate on the SAME
// canonical-won record over the SAME `earliest = block_number - deploy_lifespan`
// window.
//
// Three consensus sites, all transcribed faithfully below:
//
//   * SHARED RECORD — `interpreter_util::canonical_won_sigs`
//     (casper/src/rust/util/rholang/interpreter_util.rs:119-156, with
//     `record_disposition` :160-173). BFS the closure of ALL `parents` down to the
//     deploy-lifespan floor `earliest_block_number` (blocks with `bn < earliest` are
//     neither recorded nor traversed); record each `body.deploys` sig as a WIN and
//     each `body.rejected_deploys` sig as a REJECTION at that block's height; keep the
//     highest-block disposition (tie ⇒ REJECTION so a keep-one loser stays
//     re-proposable); return the sigs whose latest disposition is a WIN.
//
//   * PROPOSER — `block_creator::prepare_user_deploys`
//     (block_creator.rs:127-128 computes `earliest = block_number - deploy_lifespan`;
//     :171-182 keeps `valid_unique = valid.filter(|d| !canonical_won.contains(&d.sig))`).
//     The proposer INCLUDES only deploys NOT canonically-won; it GATES OUT exactly the
//     canonically-won sigs.
//
//   * VALIDATOR — `Validate::repeat_deploy`
//     (validate.rs:516-546; :528 computes `earliest = block.body.state.block_number -
//     expiration_threshold`; :537-543 forms `deploy_key_set = block.body.deploys
//     .filter(|pd| canonical_won.contains(&pd.deploy.sig))`; :544-546 returns Valid the
//     moment that set is empty, BEFORE the ancestor-BFS confirmation gate). The
//     validator FLAGS exactly the block's deploys that ARE canonically-won.
//
// Why `earliest` matches on both sides: the validator's `expiration_threshold` IS the
// shard `deploy_lifespan` — `validation_dispatcher.rs:88` passes
// `casper_shard_conf.deploy_lifespan as i32` into `block_summary` → `repeat_deploy`,
// and the proposer uses `shard_conf.deploy_lifespan` at block_creator.rs:128. Same
// shard config ⇒ same threshold; an honest block declares the parents it was built on
// ⇒ same parent set. So both sides compute the IDENTICAL `canonical_won` set, and:
//
//   proposer's block.deploys = pool \ canonical_won   (gates out canonical_won)
//   validator's flagged set   = block.deploys ∩ canonical_won
//     ⇒ validator flags (pool \ canonical_won) ∩ canonical_won = ∅   (no honest flag).
//
// `repeat_deploy` reads block numbers / bodies out of a live LMDB `KeyValueBlockStore`
// + `CasperSnapshot` DAG (heavy runtime scaffolding), so — as the sibling
// finalized_floor/fork_choice proptests do — we transcribe the pure window logic and
// exercise the AGREEMENT property on it. The ancestor-BFS second gate (validate.rs:557-570)
// is a confirmation that an honest block NEVER reaches: `deploy_key_set.is_empty()`
// short-circuits to Valid at :544-546 first.
//
// Properties:
//   (1) AGREEMENT   — the validator flags the honest proposer's block with ∅
//                     (no honest recovery block is flagged InvalidRepeatDeploy).
//   (2) SAME RECORD — over an arbitrary block, the validator flags EXACTLY the sigs the
//                     proposer gates out = the block's sigs ∩ canonical_won.
//   (3) WINDOW      — cross-check: `canonical_won_sigs` (parent-graph BFS) equals an
//                     INDEPENDENT linear scan of the chain for the highest in-window
//                     disposition per sig (a keep-one loser or a below-window/expired
//                     deploy is NEVER canonically-won, hence re-proposable and unflagged).
//
// LOCAL-ONLY verification (not consensus code). Discoverable under `cargo test -p casper`
// (wired into the `mod` integration-test binary via casper/tests/mod.rs).

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use proptest::prelude::*;

/// `u32` stands in for a deploy signature (`Bytes` in the real code); distinct
/// integers ⇒ distinct sigs.
type Sig = u32;

/// The deploy-signature universe every block draws its wins/rejections from and the
/// proposer's candidate pool spans. Small, so the window/disposition logic is
/// densely exercised.
const SIG_COUNT: usize = 5;

/// Faithful to the fields of `BlockMessage` that the three sites read:
/// `body.state.block_number`, `header.parents_hash_list`, `body.deploys[].deploy.sig`
/// (wins), `body.rejected_deploys[].sig` (rejections). `u32` stands in for `BlockHash`.
#[derive(Clone, Debug)]
struct Block {
    number: i64,
    hash: u32,
    parents: Vec<u32>,
    wins: Vec<Sig>,
    rejections: Vec<Sig>,
}

/// Transcription of `interpreter_util::record_disposition` (interpreter_util.rs:160-173):
/// move `disposition[sig]` toward the latest (highest-block) verdict; a higher block
/// wins; at a tie a REJECTION overrides a WIN (a loser must stay re-proposable).
fn record_disposition(disposition: &mut HashMap<Sig, (i64, bool)>, sig: Sig, bn: i64, won: bool) {
    match disposition.get(&sig) {
        Some((best_bn, _)) if *best_bn > bn => {}
        Some((best_bn, best_won)) if *best_bn == bn && !*best_won => {}
        _ => {
            disposition.insert(sig, (bn, won));
        }
    }
}

/// Transcription of `interpreter_util::canonical_won_sigs` (interpreter_util.rs:128-155):
/// BFS the closure of ALL parents down to `earliest`; record win/reject dispositions;
/// return the sigs whose latest disposition is a WIN.
fn canonical_won_sigs(store: &HashMap<u32, Block>, parents: &[u32], earliest: i64) -> HashSet<Sig> {
    let mut disposition: HashMap<Sig, (i64, bool)> = HashMap::new();
    let mut visited: HashSet<u32> = HashSet::new();
    let mut queue: VecDeque<u32> = parents.iter().cloned().collect();
    while let Some(hash) = queue.pop_front() {
        if !visited.insert(hash) {
            continue;
        }
        let Some(block) = store.get(&hash) else {
            continue;
        };
        let bn = block.number;
        if bn < earliest {
            continue;
        }
        for sig in &block.wins {
            record_disposition(&mut disposition, *sig, bn, true);
        }
        for sig in &block.rejections {
            record_disposition(&mut disposition, *sig, bn, false);
        }
        for p in &block.parents {
            queue.push_back(*p);
        }
    }
    disposition
        .into_iter()
        .filter_map(|(sig, (_, won))| won.then_some(sig))
        .collect()
}

/// PROPOSER selection (block_creator.rs:179-182): include only deploys NOT canonically-won.
fn proposer_selects(pool: &[Sig], canonical_won: &HashSet<Sig>) -> HashSet<Sig> {
    pool.iter()
        .copied()
        .filter(|s| !canonical_won.contains(s))
        .collect()
}

/// VALIDATOR flag set (validate.rs:537-543): the block's deploys that ARE canonically-won
/// (`deploy_key_set`). An honest block yields ∅ here and is Valid at :544-546.
fn validator_flags(block_deploys: &[Sig], canonical_won: &HashSet<Sig>) -> HashSet<Sig> {
    block_deploys
        .iter()
        .copied()
        .filter(|s| canonical_won.contains(s))
        .collect()
}

/// INDEPENDENT (non-BFS) ground truth for a linear chain: every block is reachable from
/// the tip, so `canonical_won` is exactly the set of sigs whose highest in-window
/// (`number >= earliest`) block is a WIN. A genuine cross-check of the parent-graph BFS.
fn ground_truth_canonical(chain: &[Block], earliest: i64) -> BTreeSet<Sig> {
    let mut result = BTreeSet::new();
    for sig in 0..SIG_COUNT as u32 {
        let mut best: Option<(i64, bool)> = None;
        for b in chain {
            if b.number < earliest {
                continue;
            }
            let is_win = b.wins.contains(&sig);
            let is_rej = b.rejections.contains(&sig);
            if !is_win && !is_rej {
                continue;
            }
            let won = is_win; // wins/rejections are disjoint within a block
            match best {
                Some((bn, _)) if bn > b.number => {}
                Some((bn, w)) if bn == b.number && !w => {}
                _ => best = Some((b.number, won)),
            }
        }
        if let Some((_, true)) = best {
            result.insert(sig);
        }
    }
    result
}

#[derive(Debug)]
struct Scenario {
    store: HashMap<u32, Block>,
    chain: Vec<Block>,
    tip_hash: u32,
    earliest: i64,
    pool: Vec<Sig>,
}

// A linear parent-chain of 1..=6 blocks. Block i (0-indexed) has height/hash i+1 and
// parent [i] (the previous block; block 1 is the genesis-ish root with no parent). Each
// (block, sig) gets a disposition code 0=absent, 1=win, 2=reject — so wins/rejections are
// DISJOINT within a block (faithful: a deploy is included XOR rejected in one block). The
// new block sits at height N+1 on parent [N]; `earliest = (N+1) - threshold` sweeps the
// window across the chain. The pool is the whole sig universe (the proposer considers all).
prop_compose! {
    fn scenario()(
        grid in prop::collection::vec(
            prop::collection::vec(0u8..=2, SIG_COUNT),
            1..=6,
        ),
        threshold in 0i64..=6,
    ) -> Scenario {
        let n = grid.len();
        let mut store: HashMap<u32, Block> = HashMap::with_capacity(n);
        let mut chain: Vec<Block> = Vec::with_capacity(n);
        for (i, row) in grid.iter().enumerate() {
            let height = (i + 1) as i64;
            let hash = (i + 1) as u32;
            let parents = if i == 0 { Vec::new() } else { vec![i as u32] };
            let mut wins = Vec::new();
            let mut rejections = Vec::new();
            for (sig, &code) in row.iter().enumerate() {
                match code {
                    1 => wins.push(sig as u32),
                    2 => rejections.push(sig as u32),
                    _ => {}
                }
            }
            let block = Block { number: height, hash, parents, wins, rejections };
            store.insert(hash, block.clone());
            chain.push(block);
        }
        let tip_hash = n as u32;
        let new_block_number = (n + 1) as i64;
        let earliest = new_block_number - threshold;
        let pool: Vec<Sig> = (0..SIG_COUNT as u32).collect();
        Scenario { store, chain, tip_hash, earliest, pool }
    }
}

proptest! {
    // (1) AGREEMENT: the validator flags the honest proposer's block with ∅. This IS
    // "no honest recovery block is flagged InvalidRepeatDeploy" — the P7 claim.
    #[test]
    fn honest_block_is_never_flagged(sc in scenario()) {
        let canonical_won = canonical_won_sigs(&sc.store, &[sc.tip_hash], sc.earliest);
        // proposer's emitted block.deploys = pool gated by canonical_won.
        let honest: Vec<Sig> = proposer_selects(&sc.pool, &canonical_won).into_iter().collect();
        // validator's deploy_key_set over that block.
        let flagged = validator_flags(&honest, &canonical_won);
        prop_assert!(
            flagged.is_empty(),
            "honest recovery block wrongly flagged InvalidRepeatDeploy for sigs {:?} \
             (canonical_won = {:?}, earliest = {})",
            flagged, canonical_won, sc.earliest
        );
    }

    // (2) SAME RECORD: over an arbitrary block (here the whole pool), the validator flags
    // EXACTLY the sigs the proposer gates out = block.deploys ∩ canonical_won. The two
    // sides key on the identical record, so their decisions can never diverge.
    #[test]
    fn validator_flags_exactly_proposer_gates_out(sc in scenario()) {
        let canonical_won = canonical_won_sigs(&sc.store, &[sc.tip_hash], sc.earliest);

        let flagged_all: BTreeSet<Sig> =
            validator_flags(&sc.pool, &canonical_won).into_iter().collect();

        let selected: HashSet<Sig> = proposer_selects(&sc.pool, &canonical_won);
        let gated_out: BTreeSet<Sig> =
            sc.pool.iter().copied().filter(|s| !selected.contains(s)).collect();
        prop_assert_eq!(&flagged_all, &gated_out);

        // Both equal pool ∩ canonical_won; since pool spans the whole sig universe and
        // canonical_won ⊆ universe, that is canonical_won itself.
        let canonical_won_bt: BTreeSet<Sig> = canonical_won.iter().copied().collect();
        prop_assert_eq!(&flagged_all, &canonical_won_bt);
    }

    // (3) WINDOW cross-check: the parent-graph BFS `canonical_won_sigs` agrees with an
    // INDEPENDENT linear scan of the chain (highest in-window disposition per sig). This
    // pins the latest-disposition + `earliest` window semantics: a keep-one loser (latest
    // = rejection) and a below-window/expired deploy are NEVER canonically-won, so the
    // proposer re-proposes them and the validator never flags them.
    #[test]
    fn canonical_won_matches_independent_window_scan(sc in scenario()) {
        let canonical_won: BTreeSet<Sig> =
            canonical_won_sigs(&sc.store, &[sc.tip_hash], sc.earliest).into_iter().collect();
        let ground = ground_truth_canonical(&sc.chain, sc.earliest);
        prop_assert_eq!(canonical_won, ground);
    }
}

// Deterministic examples nailing the three exemption/inclusion cases the window governs.
// These make the latest-disposition and `earliest` semantics concrete and regression-proof.
mod concrete_cases {
    use super::*;

    fn store_of(blocks: Vec<Block>) -> (HashMap<u32, Block>, Vec<Block>) {
        let mut store = HashMap::new();
        for b in &blocks {
            store.insert(b.hash, b.clone());
        }
        (store, blocks)
    }

    // Keep-one LOSER: sig A won at height 1, then REJECTED at height 2. Latest = rejection
    // ⇒ NOT canonical-won ⇒ proposer re-proposes A, validator does not flag it.
    #[test]
    fn latest_rejection_is_reproposable() {
        let a: Sig = 0;
        let (store, _chain) = store_of(vec![
            Block {
                number: 1,
                hash: 1,
                parents: vec![],
                wins: vec![a],
                rejections: vec![],
            },
            Block {
                number: 2,
                hash: 2,
                parents: vec![1],
                wins: vec![],
                rejections: vec![a],
            },
        ]);
        let canonical_won = canonical_won_sigs(&store, &[2], 0);
        assert!(
            !canonical_won.contains(&a),
            "latest disposition is a rejection ⇒ re-proposable"
        );
        assert!(proposer_selects(&[a], &canonical_won).contains(&a));
        assert!(validator_flags(&[a], &canonical_won).is_empty());
    }

    // Canonical WIN: sig A rejected at height 1, then WON at height 2. Latest = win ⇒
    // canonical-won ⇒ proposer gates A out; a block that re-includes A IS flagged.
    #[test]
    fn latest_win_is_gated_and_flagged() {
        let a: Sig = 0;
        let (store, _chain) = store_of(vec![
            Block {
                number: 1,
                hash: 1,
                parents: vec![],
                wins: vec![],
                rejections: vec![a],
            },
            Block {
                number: 2,
                hash: 2,
                parents: vec![1],
                wins: vec![a],
                rejections: vec![],
            },
        ]);
        let canonical_won = canonical_won_sigs(&store, &[2], 0);
        assert!(
            canonical_won.contains(&a),
            "latest disposition is a win ⇒ canonically-won"
        );
        assert!(
            !proposer_selects(&[a], &canonical_won).contains(&a),
            "proposer gates it out"
        );
        assert_eq!(validator_flags(&[a], &canonical_won), HashSet::from([a]));
    }

    // EXPIRED below the window: sig A won only at height 1, but `earliest = 2` cuts it off.
    // ⇒ NOT canonical-won ⇒ re-proposable and unflagged (agreement holds across the window).
    #[test]
    fn below_window_is_reproposable() {
        let a: Sig = 0;
        let (store, _chain) = store_of(vec![
            Block {
                number: 1,
                hash: 1,
                parents: vec![],
                wins: vec![a],
                rejections: vec![],
            },
            Block {
                number: 2,
                hash: 2,
                parents: vec![1],
                wins: vec![],
                rejections: vec![],
            },
        ]);
        let canonical_won = canonical_won_sigs(&store, &[2], 2); // earliest = 2 excludes height 1
        assert!(
            !canonical_won.contains(&a),
            "won only below earliest ⇒ expired ⇒ re-proposable"
        );
        assert!(validator_flags(&[a], &canonical_won).is_empty());
    }
}
