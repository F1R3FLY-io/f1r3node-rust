//! P9 step 3: N2, determinism across processes, and the replay of native space state across
//! blocks (P9 design v2, section 6, amendments A13, A14 and A16; plan revision M-5).
//!
//! Each test starts its own binary again with `current_exe()` and a role variable, and each child
//! runs one role in a fresh process (`support::process`). The children keep their stores in a run
//! directory under `RRS_SCRATCH`, or under `target/verification/reified-rspaces/scratch`, and
//! never in the RAM-backed temporary directory of the host. Every deploy runs in native execution
//! at the v6 limits (decision 69).
//!
//! | Test | Property |
//! |---|---|
//! | `n2_cross_process_post_state_equal` | The children `n2-a` and `n2-b` each play one deploy set in a fresh process over an in-memory store, and the parent plays the set twice in its own process. For each deploy, all four runs give the same pre-state and post-state roots, outputs, event log, evidence bytes and phlo. |
//! | `space_cross_block_replay` | The child `propose` plays three blocks over one LMDB store. Block 1 creates a durable space of each family and an agent space, with records and waiters. Block 2 uses them, fires the waiters of block 1, and creates and ends a `temp` space and a `seq` space inside each deploy. Block 3 runs joins, PathMap prefix deliveries and agent calls. The child `validate` opens the store in another process and certifies each deploy once from its pre-state. The parent plays the blocks in its own process and compares every record of the child `propose` with its own. |
//!
//! The deploy set of N2 holds the 24 family programs with the qualifiers `default` and `temp`,
//! two VectorDB programs with queries, the six HashSet families with `seq`, and the six deploys of
//! the conformance kit of the reference agent. The PathMap family programs make prefix
//! deliveries. The children run the same test function as the parent, so they have the tokio
//! worker count of the parent (finding F-47-2). Only the child `validate` certifies, because a
//! validator certifies each deploy once (amendment A16). In the replay across blocks, each family
//! forwards its deliveries to a channel of its own, and the parent checks the deliveries of each
//! family after its run.

#[path = "support/mod.rs"]
mod support;

use support::blocks::{
    block_families, blocks, deliveries, Deploy, AGENT_OUT, HASHSET_LIMIT, VECTORDB_CTOR,
};
use support::family_programs::{
    hashset_program, kit_deploys, program_with, space_program, Inner, Outer, HASHSET_INNERS,
    HASHSET_UNIT_TAKE,
};
use support::native::{NativeChain, AMPLE_LIMIT};
use support::process::{
    assert_same_record, certify_record, play_recorded, read_records, run_directory, write_records,
    Child, DeployRecord,
};

// The phlo limit of the HashSet deploys, the constructor of the VectorDB spaces and `Deploy`
// moved to `support::blocks` (P9 step 4), which the LFS suite shares.

/// Plays `deploys` in order on `chain` and returns the record of each deploy.
async fn play_all(chain: &mut NativeChain, deploys: &[Deploy]) -> Vec<DeployRecord> {
    let mut records = Vec::with_capacity(deploys.len());
    for deploy in deploys {
        let (_, record) = play_recorded(chain, &deploy.term, &deploy.label, deploy.limit).await;
        records.push(record);
    }
    records
}

/// Checks that two runs of one deploy set give the same records, deploy by deploy.
fn assert_same_records(own: &[DeployRecord], other: &[DeployRecord], run: &str) {
    assert_eq!(
        own.len(),
        other.len(),
        "{run}: the runs played the same number of deploys"
    );
    for (mine, theirs) in own.iter().zip(other) {
        assert_same_record(mine, theirs);
    }
}

// ─── N2 ────────────────────────────────────────────────────────────────────────────────────────

/// The child processes of N2.
const N2: Child<'static> = Child {
    test: "n2_cross_process_post_state_equal",
    role_variable: "RRS_P9_N2_ROLE",
    directory_variable: "RRS_P9_N2_DIR",
};

/// A VectorDB program of the outer store `outer`: two records, a list query and a waiter with a
/// threshold, so the space makes a list delivery and a fire with a score.
fn vectordb_program(outer: &str) -> String {
    space_program(
        &format!("rho:space:{outer}:vectordb"),
        VECTORDB_CTOR,
        r#"a!([1.0, 0.0]) | a!([0.6, 0.8]) |
for (@xs <- a ~ ([1.0, 0.0]; k=2)) { out!(["list", xs]) } |
for (@y <- b ~ ([0.0, 1.0]; threshold=0.9)) { out!(["near", y]) } | b!([0.0, 1.0])"#,
    )
}

/// The deploy set of N2, in order.
fn n2_deploys() -> Vec<Deploy> {
    let mut deploys = Vec::with_capacity(64);
    for qualifier in ["default", "temp"] {
        for outer in Outer::ALL {
            for inner in Inner::ALL {
                deploys.push(Deploy::new(
                    format!("n2 {}:{}:{qualifier}", outer.name(), inner.name()),
                    program_with(outer, inner, qualifier),
                    AMPLE_LIMIT,
                ));
            }
        }
    }
    for outer in ["hashmap", "vector"] {
        deploys.push(Deploy::new(
            format!("n2 {outer}:vectordb"),
            vectordb_program(outer),
            AMPLE_LIMIT,
        ));
    }
    for inner in HASHSET_INNERS {
        deploys.push(Deploy::new(
            format!("n2 hashset:{inner}:seq"),
            hashset_program(inner, HASHSET_UNIT_TAKE),
            HASHSET_LIMIT,
        ));
    }
    for (step, term) in kit_deploys().into_iter().enumerate() {
        deploys.push(Deploy::new(format!("n2 kit {step}"), term, AMPLE_LIMIT));
    }
    deploys
}

/// One run of the deploy set of N2 on a fresh native chain over an in-memory store.
async fn n2_run() -> Vec<DeployRecord> {
    let mut chain = NativeChain::new().await;
    play_all(&mut chain, &n2_deploys()).await
}

/// N2 (amendment A14): two child processes and two runs of the parent give the same record for
/// each deploy of the set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn n2_cross_process_post_state_equal() {
    if let Some((role, directory)) = N2.role() {
        let records = n2_run().await;
        write_records(&directory, &role, &records);
        return;
    }
    let directory = run_directory("n2");
    N2.spawn("n2-a", &directory).await;
    N2.spawn("n2-b", &directory).await;
    let first = n2_run().await;
    let second = n2_run().await;
    assert_eq!(
        first.len(),
        n2_deploys().len(),
        "the run plays every deploy"
    );
    assert_same_records(&first, &second, "the second run of the parent");
    assert_same_records(&first, &read_records(&directory, "n2-a"), "the child n2-a");
    assert_same_records(&first, &read_records(&directory, "n2-b"), "the child n2-b");
    std::fs::remove_dir_all(&directory).expect("remove the run directory");
}

// ─── The replay across blocks ──────────────────────────────────────────────────────────────────

/// The child processes of the replay across blocks.
const REPLAY: Child<'static> = Child {
    test: "space_cross_block_replay",
    role_variable: "RRS_P9_REPLAY_ROLE",
    directory_variable: "RRS_P9_REPLAY_DIR",
};

// The families of the blocks, their stash and delivery channels, `with_stash`, the agent
// channel, the ephemeral spaces of block 2 and the deploys of the three blocks moved to
// `support::blocks` (P9 step 4), which the LFS suite shares.

/// The three blocks of the replay across blocks, in order (`support::blocks`).
fn replay_deploys() -> Vec<Deploy> { blocks().into_iter().flatten().collect() }

/// The replay across blocks (amendment A16): the child `propose` plays the three blocks over one
/// LMDB store, the child `validate` certifies each deploy once from its pre-state in another
/// process, and the parent plays the blocks in its own process and compares the records.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_cross_block_replay() {
    if let Some((role, directory)) = REPLAY.role() {
        let store = directory.join("lmdb");
        return match role.as_str() {
            "propose" => {
                let mut chain = NativeChain::open(&store, None).await;
                let records = play_all(&mut chain, &replay_deploys()).await;
                write_records(&directory, "propose", &records);
            }
            "validate" => {
                let records = read_records(&directory, "propose");
                let first = records.first().expect("the blocks have deploys");
                let mut chain = NativeChain::open(&store, Some(&first.pre())).await;
                for (record, deploy) in records.iter().zip(replay_deploys()) {
                    certify_record(&mut chain, record, deploy.limit).await;
                }
            }
            other => panic!("unknown role {other}"),
        };
    }
    let directory = run_directory("cross-block-replay");
    REPLAY.spawn("propose", &directory).await;
    REPLAY.spawn("validate", &directory).await;
    let proposed = read_records(&directory, "propose");
    for pair in proposed.windows(2) {
        assert_eq!(
            pair[1].pre_root, pair[0].post_root,
            "{}: each deploy starts at the root of the deploy before it",
            pair[1].label
        );
    }
    let mut warm = NativeChain::new().await;
    let own = play_all(&mut warm, &replay_deploys()).await;
    assert_same_records(&own, &proposed, "the child propose");
    for family in block_families() {
        let mut expected = vec!["fired", "join", "seq", "take", "temp"];
        if family.label.starts_with("pathmap:") {
            expected.push("prefix");
        }
        expected.sort_unstable();
        assert_eq!(
            tags_on(&warm, &deliveries(&family.label)).await,
            expected,
            "{}: the deliveries of the three blocks",
            family.label
        );
    }
    assert_eq!(
        tags_on(&warm, AGENT_OUT).await,
        vec!["agent call", "agent fired", "agent take"],
        "the deliveries of the agent space"
    );
    std::fs::remove_dir_all(&directory).expect("remove the run directory");
}

/// The tags of the deliveries `[tag, …]` on the public channel `channel` of `chain`, sorted.
async fn tags_on(chain: &NativeChain, channel: &str) -> Vec<String> {
    let mut tags: Vec<String> = chain
        .values(channel)
        .await
        .iter()
        .flatten()
        .filter_map(|value| match value.exprs.as_slice() {
            [models::rhoapi::Expr {
                expr_instance: Some(models::rhoapi::expr::ExprInstance::EListBody(list)),
            }] => match list.ps.first().map(|first| first.exprs.as_slice()) {
                Some(
                    [models::rhoapi::Expr {
                        expr_instance: Some(models::rhoapi::expr::ExprInstance::GString(tag)),
                    }],
                ) => Some(tag.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    tags.sort_unstable();
    tags
}
