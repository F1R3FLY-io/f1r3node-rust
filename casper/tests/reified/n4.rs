//! N4: native spaces under Casper's proposal, validation and merge (P9 design v2, section 8,
//! amendments A2, A3, A17, A18 and A20; plan decisions 82, 84 and 108).
//!
//! N4 runs chains of single-deploy blocks. One durable family fits the v6 replay limits in one
//! deploy (measurement R1 of P9 step 1), so each deploy creates or touches the space of one
//! family. The rows keep the form that three facts of Casper gave them (decision 82). The probes
//! of decision 108 show that the merged base changed each fact:
//!
//! * The leader rule: node 0 proposes every space block. On the merged base, a proposer that is
//!   not the deploy-inclusion leader also includes its own offered deploys (probe R1). Its
//!   fallback counts its stored offered envelopes (DR-116, item 8).
//! * The funding rule: each payer touches only its own spaces. On the merged base, an owner funds
//!   its stored records (DR-135). So a deploy of one payer can peek and take the records of
//!   another payer in a valid block (probe R7).
//! * The cost cursor: two sibling offers of one payer conflict on its cost cursor, and the merge
//!   rejects one of them (probe R8b). So the concurrent row takes two payers whose cost-cursor
//!   buckets are disjoint (DR-119). On the merged base, every node validates such a merge block
//!   (DR-116, gap G6). A merge also keeps two sequential offers of one payer in one branch
//!   (probe R10).
//!
//! A v6 block of the merged base can hold an ordered batch of offers (DR-134, item 9, probe R9).
//! Each N4 block holds one offer.
//!
//! | Test | Property |
//! |---|---|
//! | `n4_space_chain_propose_validate` | Node 0 proposes ten blocks: the creations of a queue, a set, a priority queue, a VectorDB space and a PathMap space, each with records and a waiter, and then a touch of each space. Node 1 validates each block. Producer and validator have equal settlement receipts. At the last block, both nodes read the same deliveries of each family: the config of the handle, a take and a fired waiter. |
//! | `n4_space_merge_with_empty_sibling` | Node 0 proposes two space blocks of two payers, and node 1 proposes an empty block from genesis. Node 2 merges them. The main parent of the merge block is the empty block, the merge rejects no deploy, and every node validates the merge block. Node 0 then touches the queue of its branch on the merge, and every node validates that block. |
//! | `n4_concurrent_space_blocks_agree` | Two searched payers create their own spaces in sibling blocks over genesis, the first on node 0 and the second on node 1. Every node validates both siblings and the merge block of node 2, and the merge rejects no deploy (decision 108). |
//! | `n4_tampered_space_block_rejected` | A space block with a changed post-state hash, and the same block with a channel added to the footprint of a space row of its operation journal, are each signed again. The validator rejects each with its reason, keeps its DAG heads, and then accepts the untampered block. A timeout guards against a hang. |
//!
//! The keys are the genesis vaults of the test genesis, two labeled signers and two searched
//! signers. The time stamps are fixed per deploy, and every offer has the phlo limit [`PHLO`] and
//! the exposure [`EXPOSURE`].

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::block_status::BlockError;
use casper::rust::casper::{Casper, MultiParentCasper};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use models::rhoapi::expr::ExprInstance;
// Added by decision 92: the space-row check decodes the channels of a footprint.
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, Par};
use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedUserDeploy};
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativeBudgetTraceLimits, NativePhloRegionLimits,
};
// Changed by decision 92: the space-row check reads no recording.
// use rholang::rust::interpreter::accounting::{
//     decode_native_budget_recording, decode_native_operation_journal,
//     encode_native_operation_journal, NativeBudgetRecording, NativeOperationRecord,
//     NativeRecordingWireLimits,
// };
use rholang::rust::interpreter::accounting::{
    decode_native_budget_recording, decode_native_operation_journal,
    encode_native_operation_journal, NativeOperationRecord, NativeRecordingWireLimits,
};
// Added by decision 92: a space row names the space counter of its space.
use rholang::rust::interpreter::spaces::namespace::{
    decode_reified_id, sole_unforgeable, ReifiedId,
};

// Changed by decision 92: DR-132 names every journal row by its index, so no row has a sub-order
// segment.
// use rspace_plus_plus::rspace::space_store_handle::SUB_OPERATION_TAG;

// Changed by decision 84 (the revert of the DR-119 backport): the payers need no bucket search.
// use super::support::{
//     accept, offered_v6_genesis, offered_v6_genesis_with_signers, owner_cursor_buckets, propose,
//     rejection, searched_signer, signed_offer, submit,
// };
// Changed by decision 108: the concurrent row searches its payers by bucket again, and the merge
// row keeps its labeled payers.
// use super::support::{
//     accept, labeled_signer, offered_v6_genesis, offered_v6_genesis_with_signers, propose,
//     rejection, signed_offer, submit,
// };
use super::support::{
    accept, labeled_signer, offered_v6_genesis, offered_v6_genesis_with_signers,
    owner_cursor_buckets, propose, rejection, searched_signer, signed_offer, submit,
};
use crate::helper::block_util::resign_block;
use crate::helper::test_node::TestNode;

/// The phlo limit of every N4 offer.
const PHLO: u64 = 2_000_000;

/// The exposure of every N4 offer: more than the phlo limit at the price plus the fixed fee.
const EXPOSURE: u64 = 5_000_000;

/// The bound of the validation of a tampered block (finding F-47-4: a forged host row can hang).
const VALIDATION_TIMEOUT: Duration = Duration::from_secs(600);

/// A durable family of N4: a space with records on `a`, a waiter on `b`, and a touch that takes a
/// record, fires the waiter and stores a record (the stored families of P7).
struct Family {
    label: &'static str,
    urn: &'static str,
    ctor: &'static str,
    state: &'static str,
    touch: &'static str,
}

/// The five families of the space chain.
const FAMILIES: [Family; 5] = [
    Family {
        label: "queue",
        urn: "rho:space:hashmap:queue",
        ctor: r#""default""#,
        state: r#"a!(1) | a!(2) | a!(3) | for (@2 <- b) { out!(["fired", 2]) }"#,
        touch: r#"for (@x <- a) { out!(["take", x]) | a!(x + 10) } | b!(2)"#,
    },
    Family {
        label: "set",
        urn: "rho:space:hashmap:set",
        ctor: r#""default""#,
        state: r#"a!(1) | a!(1) | a!(2) | for (@3 <- b) { out!(["fired", 3]) }"#,
        touch: r#"for (@1 <- a) { out!(["take", 1]) | a!(11) } | b!(3)"#,
    },
    Family {
        label: "priorityqueue",
        urn: "rho:space:hashmap:priorityqueue",
        ctor: r#""default""#,
        state: r#"a!(1; priority=1) | a!(2; priority=0) | a!(3) |
for (@z <- b ~ (; priority=0)) { out!(["fired", z]) }"#,
        touch: r#"for (@x <- a) { out!(["take", x]) | a!(x + 10; priority=1) } | b!(9; priority=0)"#,
    },
    Family {
        label: "vectordb",
        urn: "rho:space:hashmap:vectordb",
        ctor: r#""default"; capacity=16, dimensions=2"#,
        state: r#"a!([1.0, 0.0]) | a!([0.6, 0.8]) |
for (@w <- b ~ ([0.0, 1.0]; threshold=0.9)) { out!(["fired", w]) }"#,
        touch: r#"for (@x <- a ~ ([1.0, 0.0])) { out!(["take", x]) | a!([0.0, 2.0]) } | b!([0.0, 1.0])"#,
    },
    Family {
        label: "pathmap",
        urn: "rho:space:pathmap:bag",
        ctor: r#""default""#,
        state: r#"use space { @[*a, 1]!(5) | @[*a, 2]!(6) } | for (@v <- b) { out!(["fired", v]) }"#,
        touch: r#"for (@v <- a) { out!(["take", v]) } | use space { @[*b, 3]!(7) }"#,
    },
];

impl Family {
    /// The public channel that holds the handle and the names `a` and `b`.
    fn stash(&self, scope: &str) -> String { format!("rrs-n4-stash-{scope}-{}", self.label) }

    /// The public channel of the deliveries of the family.
    fn deliveries(&self, scope: &str) -> String { format!("rrs-n4-out-{scope}-{}", self.label) }

    /// The deploy that creates the space, its records and its waiter, and stores the handle and
    /// the names on the stash of `scope`.
    fn creation(&self, scope: &str) -> String {
        format!(
            r#"new factory(`{urn}`), out in {{
  for (space <- factory!?({ctor})) {{
    for (a <- space!?("gensym")) {{
      for (b <- space!?("gensym")) {{
        @"{stash}"!(*space, *a, *b) | {state}
      }}
    }}
  }} |
  for (@value <= out) {{ @"{channel}"!(value) }}
}}"#,
            urn = self.urn,
            ctor = self.ctor,
            stash = self.stash(scope),
            state = self.state,
            channel = self.deliveries(scope),
        )
    }

    /// The deploy that calls the handle method `config` and touches the stored state.
    fn touching(&self, scope: &str) -> String {
        format!(
            r#"new out in {{
  for (space, a, b <<- @"{stash}") {{
    for (@config <- space!?("config")) {{ out!(["config", config]) }} | {touch}
  }} |
  for (@value <= out) {{ @"{channel}"!(value) }}
}}"#,
            stash = self.stash(scope),
            touch = self.touch,
            channel = self.deliveries(scope),
        )
    }
}

/// The tags of the deliveries `[tag, …]` on the public channel `channel` at the post-state of
/// `block`, as `node` reads them.
async fn tags_at(node: &TestNode, block: &BlockMessage, channel: &str) -> BTreeSet<String> {
    let data = node
        .runtime_manager
        .get_data(
            block.body.state.post_state_hash.clone(),
            &new_gstring_par(channel.to_string(), Vec::new(), false),
        )
        .await
        .expect("the node reads the post-state");
    data.iter()
        .filter_map(|value| match value.exprs.first()?.expr_instance.as_ref()? {
            ExprInstance::EListBody(list) => {
                match list.ps.first()?.exprs.first()?.expr_instance.as_ref()? {
                    ExprInstance::GString(tag) => Some(tag.clone()),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// Asserts that `block` holds one offered deploy that did not fail, with a deploy log.
fn assert_space_deploy(block: &BlockMessage, label: &str) {
    assert_eq!(
        block.body.deploys.len(),
        1,
        "{label}: the block holds one deploy"
    );
    let offered = block.body.deploys[0]
        .as_offered()
        .unwrap_or_else(|| panic!("{label}: the deploy is offered"));
    assert!(!offered.is_failed(), "{label}: the deploy succeeds");
    assert!(
        !offered.deploy_log().is_empty(),
        "{label}: the deploy log holds the events"
    );
}

/// Asserts that the producer `nodes[0]` and the validator `nodes[1]` hold the same settlement
/// receipt for `deploy_id`.
async fn assert_equal_receipts(nodes: &[TestNode], deploy_id: &Vec<u8>, label: &str) {
    let producer = BlockAPI::find_offered_settlement_receipt(&nodes[0].engine_cell, deploy_id)
        .await
        .expect("the producer reads its receipt")
        .unwrap_or_else(|| panic!("{label}: the producer holds a receipt"));
    let validator = BlockAPI::find_offered_settlement_receipt(&nodes[1].engine_cell, deploy_id)
        .await
        .expect("the validator reads its receipt")
        .unwrap_or_else(|| panic!("{label}: the validator holds a receipt"));
    assert_eq!(producer, validator, "{label}: the settlement receipts");
}

// Changed by decision 108: the merged base has the cost-cursor buckets of DR-119, so the concurrent
// row searches its payers again. Decision 84 had disabled this helper. Its doc states the fact
// that probe R8b measured on the merged base.
/// Two searched payers whose cost-cursor buckets are disjoint (DR-119). A merge rejects one of
/// two sibling offers of one payer, because they conflict on its cost cursor (probe R8b of
/// decision 108). So the concurrent row gives each sibling its own payer.
fn disjoint_payers() -> [(PrivateKey, PublicKey); 2] {
    let first = searched_signer("rrs-n4-first", |buckets| buckets[0] != buckets[1]);
    let first_buckets = owner_cursor_buckets(&first.1);
    let second = searched_signer("rrs-n4-second", |buckets| {
        buckets[0] != buckets[1] && buckets.iter().all(|bucket| !first_buckets.contains(bucket))
    });
    [first, second]
}

/// Two labeled payers, one for each space block of the merge row (decision 82, fact 3;
/// verification.md, section 34.6). On the merged base, a merge also keeps two sequential offers
/// of one payer in one branch (probe R10 of decision 108), and the row keeps its two payers.
fn two_payers() -> [(PrivateKey, PublicKey); 2] {
    [
        labeled_signer("rrs-n4-first"),
        labeled_signer("rrs-n4-second"),
    ]
}

/// N4 (decision 82): node 0, the leader, proposes a chain of space blocks, and node 1 validates
/// each block.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n4_space_chain_propose_validate() {
    let genesis = offered_v6_genesis(2).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("the network starts");
    let owner = &genesis.genesis_vaults[0];
    let creations = FAMILIES
        .iter()
        .map(|family| (family, family.creation("chain")));
    let touches = FAMILIES
        .iter()
        .map(|family| (family, family.touching("chain")));
    let mut last = None;
    for (stamp, (family, term)) in (1i64..).zip(creations.chain(touches)) {
        let label = format!("block {stamp} {}", family.label);
        let deploy_id = submit(&nodes[0], signed_offer(owner, &term, stamp, PHLO, EXPOSURE)).await;
        let block = propose(&mut nodes, 0).await;
        assert_space_deploy(&block, &label);
        accept(&mut nodes[1], &block).await;
        assert_equal_receipts(&nodes, &deploy_id, &label).await;
        last = Some(block);
    }
    let last = last.expect("the chain has blocks");
    for family in &FAMILIES {
        let channel = family.deliveries("chain");
        let producer = tags_at(&nodes[0], &last, &channel).await;
        assert_eq!(
            producer,
            BTreeSet::from(["config", "fired", "take"].map(String::from)),
            "{}: the deliveries of the chain",
            family.label
        );
        assert_eq!(
            tags_at(&nodes[1], &last, &channel).await,
            producer,
            "{}: the validator reads the deliveries of the producer",
            family.label
        );
    }
}

/// N4 (amendment A3): an empty sibling is the main parent of the merge block, and the merge keeps
/// the space blocks of the other branch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n4_space_merge_with_empty_sibling() {
    // Changed by decision 84 (the revert of the DR-119 backport): two labeled payers.
    // let payers = disjoint_payers();
    let payers = two_payers();
    let genesis = offered_v6_genesis_with_signers(3, &payers).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("the network starts");
    let [queue, set, ..] = &FAMILIES;
    submit(
        &nodes[0],
        signed_offer(&payers[0], &queue.creation("merge"), 1, PHLO, EXPOSURE),
    )
    .await;
    let first = propose(&mut nodes, 0).await;
    submit(
        &nodes[0],
        signed_offer(&payers[1], &set.creation("merge"), 2, PHLO, EXPOSURE),
    )
    .await;
    let second = propose(&mut nodes, 0).await;
    assert_space_deploy(&first, "the queue block");
    assert_space_deploy(&second, "the set block");
    nodes[1].allow_empty_blocks = true;
    let empty = propose(&mut nodes, 1).await;
    assert!(empty.body.deploys.is_empty(), "the sibling is empty");
    for block in [&first, &second, &empty] {
        accept(&mut nodes[2], block).await;
    }
    nodes[2].allow_empty_blocks = true;
    let merge = propose(&mut nodes, 2).await;
    assert_eq!(
        merge.header.parents_hash_list.first(),
        Some(&empty.block_hash),
        "the empty sibling is the main parent of the merge block"
    );
    assert!(
        merge.header.parents_hash_list.contains(&second.block_hash),
        "the merge block has the space branch as a parent"
    );
    assert!(
        merge.body.rejected_deploys.is_empty(),
        "the merge rejects no deploy"
    );
    accept(&mut nodes[0], &empty).await;
    accept(&mut nodes[0], &merge).await;
    accept(&mut nodes[1], &first).await;
    accept(&mut nodes[1], &second).await;
    accept(&mut nodes[1], &merge).await;
    submit(
        &nodes[0],
        signed_offer(&payers[0], &queue.touching("merge"), 3, PHLO, EXPOSURE),
    )
    .await;
    let touch = propose(&mut nodes, 0).await;
    assert_space_deploy(&touch, "the touch on the merge");
    assert!(
        touch.header.parents_hash_list.contains(&merge.block_hash),
        "the touch block has the merge block as a parent"
    );
    accept(&mut nodes[1], &touch).await;
    accept(&mut nodes[2], &touch).await;
    assert_eq!(
        tags_at(&nodes[2], &touch, &queue.deliveries("merge")).await,
        BTreeSet::from(["config", "fired", "take"].map(String::from)),
        "the touch reads the queue of the space branch through the merge"
    );
}

// Changed by decision 108: the row returns with the merge of feature/cost-accounted-rho, which
// fixes both defects that disabled it (decision 84). DR-119 gives the vault cost-cursor buckets
// (bug 11004), and DR-116 removes the envelope-buffer failure of the merge block (gap G6).
/// N4 (decision 82): two payers with disjoint cost-cursor buckets create their own spaces in
/// sibling blocks over genesis.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n4_concurrent_space_blocks_agree() {
    let signers = disjoint_payers();
    let genesis = offered_v6_genesis_with_signers(3, &signers).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("the network starts");
    let [queue, set, ..] = &FAMILIES;
    submit(
        &nodes[0],
        signed_offer(&signers[0], &queue.creation("x"), 1, PHLO, EXPOSURE),
    )
    .await;
    let x = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("node 0 creates the first sibling");
    submit(
        &nodes[1],
        signed_offer(&signers[1], &set.creation("y"), 2, PHLO, EXPOSURE),
    )
    .await;
    let y = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("node 1 creates the second sibling");
    assert_space_deploy(&x, "the first sibling");
    assert_space_deploy(&y, "the second sibling");
    for node in nodes.iter_mut() {
        accept(node, &x).await;
        accept(node, &y).await;
    }
    nodes[2].allow_empty_blocks = true;
    let merge = propose(&mut nodes, 2).await;
    let parents: BTreeSet<_> = merge.header.parents_hash_list.iter().cloned().collect();
    assert!(
        parents.contains(&x.block_hash) && parents.contains(&y.block_hash),
        "the merge block has both siblings as parents"
    );
    assert!(
        merge.body.rejected_deploys.is_empty(),
        "the merge rejects no deploy"
    );
    accept(&mut nodes[0], &merge).await;
    accept(&mut nodes[1], &merge).await;
}

/// The wire limits of the evidence of a v6 offer.
fn recording_limits() -> NativeRecordingWireLimits {
    let protocol = offered_funded_v6_limits();
    NativeRecordingWireLimits {
        wire: PhloWireLimits {
            total_bytes: protocol.evidence.field_bytes,
            field_bytes: protocol.evidence.field_bytes,
        },
        budget: NativeBudgetTraceLimits {
            attempts: protocol.deploy_log_events,
            path_segments: 1_024,
            regions: NativePhloRegionLimits {
                regions: protocol.envelope.payload.funding.authority_nodes,
                encoded_authority_bytes: protocol.funding_case.key_bytes,
            },
        },
        operations: protocol.deploy_log_events,
        source_entries: protocol.deploy_log_items,
        footprint_entries: protocol.deploy_log_items,
        footprint_bytes: protocol.evidence.field_bytes,
        predecessor_edges: protocol.deploy_log_items,
        total_path_segments: protocol.deploy_log_items,
    }
}

// Changed by decision 92: DR-132 of the merged base names every journal row by its index, as the
// path `[(index, 0)]`, so no row has a sub-order segment. A row of a space transaction names the
// space counter `S(sid)` of its space in its footprint instead.
// /// True when `row` is a staged space operation: its path ends with a sub-order segment
// /// (decision 19, D3).
// fn is_space_row(recording: &NativeBudgetRecording, row: &NativeOperationRecord) -> bool {
//     recording
//         .paths
//         .last_segment(row.occurrence.path)
//         .is_some_and(|(_, tag)| tag == SUB_OPERATION_TAG)
// }
/// True when `row` is a row of a space transaction: a channel of its footprint is the space
/// counter `S(sid)` of a space (decision 92). A footprint holds each channel in the bincode form
/// of its `Par`, as the recorder writes it.
fn is_space_row(row: &NativeOperationRecord) -> bool {
    row.footprint.iter().any(|channel| {
        let Ok(channel) = bincode::deserialize::<Par>(channel) else {
            return false;
        };
        match sole_unforgeable(&channel).map(|unforgeable| &unforgeable.unf_instance) {
            Some(Some(UnfInstance::GPrivateBody(GPrivate { id }))) => {
                matches!(decode_reified_id(id), Some(ReifiedId::SpaceCounter(_)))
            }
            _ => false,
        }
    })
}

/// Derives the predecessors of each row from the footprints, as the journal check requires.
fn derive_predecessors(rows: &mut [NativeOperationRecord]) {
    let mut last: HashMap<Arc<[u8]>, usize> = HashMap::with_capacity(rows.len());
    for (index, row) in rows.iter_mut().enumerate() {
        let mut expected: Vec<usize> = row
            .footprint
            .iter()
            .filter_map(|channel| last.get(channel).copied())
            .collect();
        expected.sort_unstable();
        expected.dedup();
        row.predecessors = expected.into();
        for channel in row.footprint.iter() {
            last.insert(Arc::clone(channel), index);
        }
    }
}

/// Added by CD-2 (the final verification of merge 2, decisions 47 and 93): the forgeries of
/// `rows`, in the order of the text before of [`with_forged_space_row`], as the rholang sweep
/// `space_native_forged_footprints_never_hang` builds them. For each space row and each later
/// host row, the forgery adds the first channel of the host row that the footprint of the space
/// row lacks, and the predecessors follow the new footprints.
fn forgeries(
    rows: &[NativeOperationRecord],
) -> impl Iterator<Item = Vec<NativeOperationRecord>> + '_ {
    let mut space = Vec::with_capacity(rows.len());
    space.extend(rows.iter().map(is_space_row));
    let space_rows = space.iter().filter(|&&is_space| is_space).count();
    let mut pairs = Vec::with_capacity(space_rows * (rows.len() - space_rows));
    for (target, _) in space.iter().enumerate().filter(|&(_, &is_space)| is_space) {
        for (host, _) in space
            .iter()
            .enumerate()
            .skip(target + 1)
            .filter(|&(_, &is_space)| !is_space)
        {
            match rows[host]
                .footprint
                .iter()
                .find(|channel| !rows[target].footprint.contains(channel))
            {
                None => {}
                Some(channel) => pairs.push((target, Arc::clone(channel))),
            }
        }
    }
    pairs.into_iter().map(move |(target, channel)| {
        let mut forged = rows.to_vec();
        let mut footprint = Vec::with_capacity(forged[target].footprint.len() + 1);
        footprint.extend(forged[target].footprint.iter().cloned());
        footprint.push(channel);
        footprint.sort();
        footprint.dedup();
        forged[target].footprint = footprint.into();
        derive_predecessors(&mut forged);
        forged
    })
}

/// Added by CD-2 (the final verification of merge 2, decision 93): true when each predecessor of
/// each row of `rows` closed its budget interval before the row opened its own, as the rholang
/// sweep `space_native_forged_footprints_never_hang` checks. The journal check rejects every
/// other predecessor (`check_dependencies`).
fn intervals_ordered(rows: &[NativeOperationRecord]) -> bool {
    rows.iter().all(|row| {
        row.predecessors
            .iter()
            .all(|prior| rows[*prior].budget_end <= row.budget_start)
    })
}

/// `block` with its offered deploy's operation journal replaced by the first forgery of
/// [`forgeries`] that passes the journal check ([`intervals_ordered`]): a space row gets a
/// channel of a later host row, and the predecessors follow the new footprints.
// Changed by CD-2 (the final verification of merge 2, decisions 47 and 93): the forgery passes
// the journal check. The text before:
// /// `block` with its offered deploy's operation journal replaced: the first space row gets a
// /// channel of a later host row, and the predecessors follow the new footprints.
fn with_forged_space_row(block: &BlockMessage) -> BlockMessage {
    let protocol = offered_funded_v6_limits();
    let offered = block.body.deploys[0]
        .as_offered()
        .expect("the deploy is offered");
    let evidence = offered
        .evidence(protocol.evidence)
        .expect("the evidence decodes");
    let wire = recording_limits();
    let mut recording = decode_native_budget_recording(evidence.budget_recording, wire)
        .expect("the recording decodes");
    let journal = decode_native_operation_journal(evidence.operation_journal, &mut recording, wire)
        .expect("the journal decodes");
    // Changed by CD-2 (the final verification of merge 2, decisions 47 and 93): the forgeries
    // copy the journal rows. The text before:
    // let mut rows: Vec<NativeOperationRecord> = journal.to_vec();
    let rows: Vec<NativeOperationRecord> = journal.to_vec();
    // Changed by decision 92: the space-row check reads only the row.
    // let (target, channel) = (0..rows.len())
    //     .filter(|&target| is_space_row(&recording, &rows[target]))
    //     .find_map(|target| {
    //         rows[target + 1..]
    //             .iter()
    //             .filter(|row| !is_space_row(&recording, row))
    //             .find_map(|row| {
    // Changed by CD-2 (the final verification of merge 2, decisions 47 and 93, the forgery rule
    // of the rholang sweep `space_native_forged_footprints_never_hang`): racing evaluation can
    // run the two rows of a new predecessor edge at the same time, and then the journal check
    // rejects the forgery before the replay (`check_dependencies`). The forgery is the first one,
    // in the order of the text before, whose new predecessors each closed their budget interval
    // before their row opened, so the replay must reject it. The text before:
    // let (target, channel) = (0..rows.len())
    //     .filter(|&target| is_space_row(&rows[target]))
    //     .find_map(|target| {
    //         rows[target + 1..]
    //             .iter()
    //             .filter(|row| !is_space_row(row))
    //             .find_map(|row| {
    //                 row.footprint
    //                     .iter()
    //                     .find(|channel| !rows[target].footprint.contains(channel))
    //                     .map(Arc::clone)
    //             })
    //             .map(|channel| (target, channel))
    //     })
    //     .expect("a space row and a later host row with another channel exist");
    // let mut footprint = rows[target].footprint.to_vec();
    // footprint.push(channel);
    // footprint.sort();
    // footprint.dedup();
    // rows[target].footprint = footprint.into();
    // derive_predecessors(&mut rows);
    let rows = forgeries(&rows)
        .find(|forged| intervals_ordered(forged))
        .expect("a forgery of a space row passes the journal check");
    let rows: Arc<[NativeOperationRecord]> = rows.into();
    let journal_bytes = encode_native_operation_journal(&rows, &recording.paths, wire)
        .expect("the forged journal encodes");
    let mut altered_evidence = evidence.clone();
    altered_evidence.operation_journal = &journal_bytes;
    let mut altered_proto = offered
        .to_proto(protocol.evidence)
        .expect("the deploy has a proto");
    altered_proto.native_cost_evidence = Some(
        altered_evidence
            .encode(protocol.evidence)
            .expect("the evidence encodes")
            .into(),
    );
    let mut altered = block.clone();
    altered.body.deploys[0] =
        ProcessedUserDeploy::from_proto(altered_proto).expect("the deploy decodes");
    altered
}

/// Validates `altered` on `node` within [`VALIDATION_TIMEOUT`], and checks that the node rejects
/// it with an error that satisfies `expected`, without a change of its DAG heads.
async fn assert_rejected(
    node: &mut TestNode,
    altered: &BlockMessage,
    label: &str,
    expected: impl Fn(&BlockError) -> bool,
) {
    let heads = node
        .casper
        .block_dag()
        .await
        .expect("the node reads its DAG")
        .latest_message_hashes();
    let error = tokio::time::timeout(VALIDATION_TIMEOUT, rejection(node, altered))
        .await
        .unwrap_or_else(|_| panic!("{label}: the validation completes"));
    assert!(expected(&error), "{label}: the rejection reason: {error:?}");
    assert!(
        !node.casper.dag_contains(&altered.block_hash),
        "{label}: the DAG does not hold the block"
    );
    assert_eq!(
        node.casper
            .block_dag()
            .await
            .expect("the node reads its DAG")
            .latest_message_hashes(),
        heads,
        "{label}: the DAG heads do not change"
    );
}

/// N4 (amendment A18): a tampered space block is rejected, and the untampered block is then
/// accepted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n4_tampered_space_block_rejected() {
    let genesis = offered_v6_genesis(2).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("the network starts");
    let [queue, ..] = &FAMILIES;
    let owner = &genesis.genesis_vaults[0];
    submit(
        &nodes[0],
        signed_offer(owner, &queue.creation("tamper"), 1, PHLO, EXPOSURE),
    )
    .await;
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the proposer creates a block");
    assert_space_deploy(&block, "the space block");
    let key = &nodes[0]
        .validator_id_opt
        .as_ref()
        .expect("node 0 is a validator")
        .private_key
        .clone();

    let mut changed_root = block.clone();
    let mut post_state = changed_root.body.state.post_state_hash.to_vec();
    post_state[0] ^= 0x01;
    changed_root.body.state.post_state_hash = post_state.into();
    let changed_root = resign_block(&changed_root, key);
    assert_rejected(
        &mut nodes[1],
        &changed_root,
        "a changed post-state hash",
        |error| {
            matches!(error, BlockError::BlockException(cause)
                if cause.to_string().contains("offered block replay post-state differs from commitment"))
        },
    )
    .await;

    let forged = resign_block(&with_forged_space_row(&block), key);
    assert_rejected(&mut nodes[1], &forged, "a forged space row", |error| {
        // Changed by decision 124 (L10): no failure closes the epoch, so the replay of the forged
        // row ends with a base verdict. It has incomplete operations when no task reaches a row
        // behind the forged row. A task that reaches one parks counted, and the stall rule ends
        // the replay once every live task waits (DR-132). The text before:
        // matches!(error, BlockError::BlockException(cause)
        //     if cause.to_string().contains("native replay epoch is closed"))
        match error {
            BlockError::BlockException(cause) => {
                let cause = cause.to_string();
                cause.contains("native replay has incomplete operations")
                    || cause.contains("native replay stalled")
            }
            _ => false,
        }
    })
    .await;

    accept(&mut nodes[0], &block).await;
    accept(&mut nodes[1], &block).await;
}
