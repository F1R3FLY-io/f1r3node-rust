//! P9 step 3: N2, determinism across processes, and the replay of native space state across
//! blocks (P9 design v2, section 6, amendments A13, A14 and A16; plan revision M-5).
//!
//! Each test starts its own binary again with `current_exe()` and a role variable, and each child
//! runs one role in a fresh process (`support::process`). The children keep their stores in a run
//! directory under `RRS_SCRATCH`, or under `target/verification/reified-rspaces/scratch`, and
//! never in the RAM-backed temporary directory of the host. Every deploy runs in native execution
//! at the v6 host-work limits (decision 69).
//!
//! | Test | Property |
//! |---|---|
//! | `n2_cross_process_post_state_equal` | The children `n2-a` and `n2-b` each play one deploy set in a fresh process over an in-memory store, and the parent plays the set twice in its own process. For each deploy, all four runs give the same pre-state and post-state roots, outputs, event log, evidence bytes, phlo and host work. |
//! | `space_cross_block_replay` | The child `propose` plays three blocks over one LMDB store. Block 1 creates a durable space of each family and an agent space, with records and waiters. Block 2 uses them, fires the waiters of block 1, and creates and ends a `temp` space and a `seq` space inside each deploy. Block 3 runs joins, PathMap prefix deliveries and agent calls. The child `validate` opens the store in another process and certifies each deploy once from its pre-state. The parent plays the blocks in its own process and compares every record of the child `propose` with its own. |
//!
//! The deploy set of N2 holds the 24 family programs with the qualifiers `default` and `temp`,
//! two VectorDB programs with queries, the six HashSet families with `seq`, and the six deploys of
//! the conformance kit of the reference agent. The PathMap family programs make prefix
//! deliveries. The children run the same test function as the parent, so they have the tokio
//! worker count of the parent (finding F-47-2). Only the child `validate` certifies, because a
//! validator certifies each deploy once (amendment A16). In the replay across blocks, each family
//! forwards its deliveries to a channel of its own, and the parent checks the deliveries of each
//! family after its run. One shared channel would grow with the chain, and native replay charges
//! loads that grow with it (decision 79).

#[path = "support/mod.rs"]
mod support;

use support::family_programs::{
    hashset_program, kit_deploys, program_with, reference_agent_creation, space_program,
    with_kit_names, Inner, Outer, HASHSET_INNERS, HASHSET_UNIT_TAKE, KIT_NAMES,
};
use support::native::{NativeChain, AMPLE_LIMIT};
use support::process::{
    assert_same_record, certify_record, play_recorded, read_records, run_directory, write_records,
    Child, DeployRecord,
};

/// The phlo limit of the deploys of the HashSet families (`reified_hashset.rs`).
const HASHSET_LIMIT: u64 = 20 * AMPLE_LIMIT;

/// The constructor of the VectorDB spaces.
const VECTORDB_CTOR: &str = r#""default"; capacity=16, dimensions=2"#;

/// A deploy of a set: its label, which seeds its random state, its term and its phlo limit.
struct Deploy {
    label: String,
    term: String,
    limit: u64,
}

impl Deploy {
    fn new(label: impl Into<String>, term: impl Into<String>, limit: u64) -> Self {
        Self {
            label: label.into(),
            term: term.into(),
            limit,
        }
    }
}

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

/// A family of the replay across blocks: its label, its factory, its constructor, and the parts
/// of its three blocks over the names `a` to `d` of its space.
struct BlockFamily {
    label: String,
    urn: String,
    ctor: String,
    /// Block 1: records on `a` and a waiter on `b`.
    state: String,
    /// Block 2: a take of a record of `a`, a store, and a send that fires the waiter on `b`.
    touch: String,
    /// Block 3: a join of `c` and `d`, and a prefix delivery for a PathMap space.
    join: String,
}

/// The families of the replay across blocks: the 24 outer and inner families and the two
/// VectorDB families.
fn block_families() -> Vec<BlockFamily> {
    let mut families = Vec::with_capacity(26);
    for outer in Outer::ALL {
        for inner in Inner::ALL {
            let records = match inner {
                Inner::Cell => "a!(1)",
                Inner::Bag | Inner::Queue | Inner::Stack | Inner::Set | Inner::PriorityQueue => {
                    "a!(1) | a!(2)"
                }
            };
            let prefix = match outer {
                Outer::PathMap => {
                    r#" | use space { @[*c, 9]!(7) } | for (@v <- c) { out!(["prefix", v]) }"#
                }
                Outer::HashMap | Outer::Array | Outer::Vector => "",
            };
            families.push(BlockFamily {
                label: format!("{}:{}", outer.name(), inner.name()),
                urn: format!("rho:space:{}:{}", outer.name(), inner.name()),
                ctor: outer.ctor("default"),
                state: format!(r#"{records} | for (@v <- b) {{ out!(["fired", v]) }}"#),
                touch: r#"for (@x <- a) { out!(["take", x]) | a!(x + 10) } | b!(3)"#.to_string(),
                join: format!(
                    r#"for (@p <- d & @q <- a) {{ out!(["join", p, q]) }} | d!(5){prefix}"#
                ),
            });
        }
    }
    for outer in ["hashmap", "vector"] {
        families.push(BlockFamily {
            label: format!("{outer}:vectordb"),
            urn: format!("rho:space:{outer}:vectordb"),
            ctor: VECTORDB_CTOR.to_string(),
            state: r#"a!([1.0, 0.0]) | a!([0.6, 0.8]) |
for (@w <- b ~ ([0.0, 1.0]; threshold=0.9)) { out!(["fired", w]) }"#
                .to_string(),
            touch: r#"for (@x <- a ~ ([1.0, 0.0])) { out!(["take", x]) | a!([0.0, 2.0]) } | b!([0.0, 1.0])"#
                .to_string(),
            join: r#"for (@p <- d & @q <- a) { out!(["join", p, q]) } | d!([1.0, 1.0])"#
                .to_string(),
        });
    }
    families
}

/// The public channel that holds the handle and the names of the space of `family`.
fn stash(family: &BlockFamily) -> String { format!("rrs-p9-replay-{}", family.label) }

/// The public channel of the deliveries of the deploys of `label`. Each family has its own
/// channel, so no channel grows with the length of the chain. Native replay charges loads that
/// grow with the values on a channel, and play does not (decision 79, finding M-47-1). With
/// one shared channel, a deploy of block 2 needed more than the v6 limits in replay after about
/// 45 deploys (verification.md, section 34.4).
fn deliveries(label: &str) -> String { format!("rrs-p9-replay-out-{label}") }

/// A deploy that runs `body` over the stored handle `space` and the names `a` to `d` of
/// `family`, with the values on `out` forwarded to the channel of the family ([`deliveries`]).
fn with_stash(family: &BlockFamily, body: &str) -> String {
    format!(
        r#"new out in {{
  for (space, a, b, c, d <<- @"{stash}") {{
    {body}
  }} |
  for (@value <= out) {{ @"{channel}"!(value) }}
}}"#,
        stash = stash(family),
        channel = deliveries(&family.label),
    )
}

/// The channel of the deliveries of the agent deploys. Each agent deploy makes one call of the
/// agent, as each step of the conformance kit does: native replay charges more host work than
/// play for a call of a stored contract (decision 79), and two calls of the reference agent in
/// one deploy need more than the v6 limits in replay.
const AGENT_OUT: &str = "rrs-p9-replay-out-agent";

/// The `temp` space and the `seq` space that each deploy of block 2 creates and ends.
const EPHEMERAL: &str = r#"new tf(`rho:space:hashmap:queue`), sf(`rho:space:hashset:bag`) in {
  for (t <- tf!?("temp")) { for (n <- t!?("gensym")) { n!(1) | for (@z <- n) { out!(["temp", z]) } } } |
  for (S <- sf!?("seq")) { for (u <- S!?("gensym")) { S!?("produce", *u, []); for ( <- u) { out!(["seq", 1]) } } }
}"#;

/// The three blocks of the replay across blocks, in order.
fn replay_deploys() -> Vec<Deploy> {
    let families = block_families();
    let mut deploys = Vec::with_capacity(3 * families.len() + 8);
    for family in &families {
        deploys.push(Deploy::new(
            format!("block 1 {}", family.label),
            format!(
                r#"new factory(`{urn}`), out in {{
  for (space <- factory!?({ctor})) {{
    for (a <- space!?("gensym")) {{ for (b <- space!?("gensym")) {{
      for (c <- space!?("gensym")) {{ for (d <- space!?("gensym")) {{
        @"{stash}"!(*space, *a, *b, *c, *d) | {state}
      }} }}
    }} }}
  }} |
  for (@value <= out) {{ @"{channel}"!(value) }}
}}"#,
                urn = family.urn,
                ctor = family.ctor,
                stash = stash(family),
                state = family.state,
                channel = deliveries(&family.label),
            ),
            AMPLE_LIMIT,
        ));
    }
    deploys.push(Deploy::new(
        "block 1 agent",
        reference_agent_creation(),
        AMPLE_LIMIT,
    ));
    deploys.push(Deploy::new("block 1 agent names", KIT_NAMES, AMPLE_LIMIT));
    deploys.push(Deploy::new(
        "block 1 agent message",
        with_kit_names("@x!(1, 2)"),
        AMPLE_LIMIT,
    ));
    deploys.push(Deploy::new(
        "block 1 agent waiter",
        with_kit_names(&format!(
            r#"for (@a, @b <- @y) {{ @"{AGENT_OUT}"!(["agent fired", a, b]) }}"#
        )),
        AMPLE_LIMIT,
    ));
    for family in &families {
        deploys.push(Deploy::new(
            format!("block 2 {}", family.label),
            with_stash(family, &format!("{} | {EPHEMERAL}", family.touch)),
            HASHSET_LIMIT,
        ));
    }
    deploys.push(Deploy::new(
        "block 2 agent wake",
        with_kit_names("@y!(3, 4)"),
        AMPLE_LIMIT,
    ));
    deploys.push(Deploy::new(
        "block 2 agent take",
        with_kit_names(&format!(
            r#"for (@a, @b <- @x) {{ @"{AGENT_OUT}"!(["agent take", a, b]) }}"#
        )),
        AMPLE_LIMIT,
    ));
    for family in &families {
        deploys.push(Deploy::new(
            format!("block 3 {}", family.label),
            with_stash(family, &family.join),
            AMPLE_LIMIT,
        ));
    }
    deploys.push(Deploy::new(
        "block 3 agent call",
        with_kit_names("@x!(5, 6)"),
        AMPLE_LIMIT,
    ));
    deploys.push(Deploy::new(
        "block 3 agent take",
        with_kit_names(&format!(
            r#"for (@a, @b <- @x) {{ @"{AGENT_OUT}"!(["agent call", a, b]) }}"#
        )),
        AMPLE_LIMIT,
    ));
    deploys
}

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
