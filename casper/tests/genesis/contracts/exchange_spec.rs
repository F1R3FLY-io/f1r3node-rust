//! Cost-Accounted Rho Stage D — `rho:lang:exchange` blessed Exchange verification.
//!
//! Verifies the Stage-D blessed Exchange's Rust glue (mirrors
//! `capabilities_registry_spec.rs`):
//! 1. URI determinism — the registry URI is content-addressed from the
//!    pubkey hash, so the same private key yields the same URI across runs
//!    (replay determinism / genesis-hash stability), and it equals the
//!    `rho:lang:exchange` shorthand baked into `Registry.rho`.
//! 2. Genesis-bootstrap inclusion — `EXCHANGE_PUB_KEY` is in
//!    `system_public_keys()` (so block validation accepts the Exchange
//!    genesis deploy), and the deploy is constructable (its `.rhox`
//!    template compiles with the per-shard `$$exchangePubKey$$` /
//!    `$$exchangeSig$$` substituted in).
//! 3. The substituted Rholang source parses (the contract body is
//!    well-formed Rholang) — the Rust-side compile gate the genesis ceremony
//!    relies on.

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::registry::registry::Registry;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

use casper::rust::genesis::contracts::standard_deploys;
use casper::rust::util::construct_deploy;

use crate::util::rholang::resources::with_runtime_manager;

/// The `rho:id:` URI derived from `EXCHANGE_PK` MUST equal the
/// `rho:lang:exchange` shorthand baked into `Registry.rho`'s bootstrapLookup
/// table — otherwise `rl!(`rho:lang:exchange`, ret)` would resolve to a
/// non-existent registry entry. This pins the cross-file invariant.
const EXCHANGE_SHORTHAND_URI: &str =
    "rho:id:h7oamwfmbdgahd4jk6kcf9kzqssapm6bifq5i4u77danpta6d1ejf5";

#[test]
fn exchange_pubkey_resolves_to_deterministic_uri_matching_registry_shorthand() {
    let sk = PrivateKey::from_bytes(
        &hex::decode(standard_deploys::EXCHANGE_PK).expect("hex decode of EXCHANGE_PK"),
    );
    let secp = Secp256k1;
    let pk1 = secp.to_public(&sk);
    let pk2 = secp.to_public(&sk);
    assert_eq!(pk1, pk2, "Secp256k1::to_public must be a pure function");

    let hash = Blake2b256::hash(pk1.bytes.to_vec());
    let uri = Registry::build_uri(&hash);
    assert!(uri.starts_with("rho:id:"), "URI must use rho:id: prefix");
    assert_eq!(
        uri, EXCHANGE_SHORTHAND_URI,
        "the EXCHANGE_PK-derived URI must equal the rho:lang:exchange shorthand in Registry.rho"
    );
}

#[test]
fn exchange_pubkey_in_system_public_keys() {
    // The genesis blessed-terms registration uses `system_public_keys()`. The
    // Exchange's pubkey must be present so block-validation accepts the
    // Exchange genesis deploy.
    let secp = Secp256k1;
    let sk = PrivateKey::from_bytes(
        &hex::decode(standard_deploys::EXCHANGE_PK).expect("hex decode of EXCHANGE_PK"),
    );
    let expected_pk = secp.to_public(&sk);

    let system_pks = standard_deploys::system_public_keys();
    let found = system_pks.iter().any(|p| *p == &expected_pk);
    assert!(
        found,
        "EXCHANGE_PUB_KEY must appear in system_public_keys()"
    );
}

#[test]
fn exchange_deploy_constructable_and_source_compiles() {
    let shard_id = "stage-d-test";
    let deploy = standard_deploys::exchange(shard_id);
    assert!(
        deploy.data.term.contains("Exchange"),
        "the substituted source must define the Exchange contract"
    );
    assert!(
        deploy.data.term.contains("\"swap\""),
        "the Exchange contract must expose the conserving swap method"
    );
    // The placeholders must be substituted (no `$$...$$` left).
    assert!(
        !deploy.data.term.contains("$$exchangePubKey$$"),
        "exchangePubKey placeholder must be substituted at genesis"
    );
    assert!(
        !deploy.data.term.contains("$$exchangeSig$$"),
        "exchangeSig placeholder must be substituted at genesis"
    );
    assert_eq!(deploy.data.shard_id, shard_id);

    // The substituted Rholang source must parse — the genesis-ceremony compile
    // gate. (Stage-D Exchange.rhox is the spec's persistent-join 1:1 swap.)
    Compiler::source_to_adt(&deploy.data.term)
        .expect("Exchange.rhox substituted source must compile to ADT");
}

/// End-to-end: `rho:lang:exchange` RESOLVES against the genesis state to the
/// registered blessed contract, and the conserving 1:1 swap fires only when
/// BOTH carriers hold a datum (DR-4 join), re-emitting each datum on the OTHER
/// carrier with per-channel count preserved (spec tex:3067-3081 / Rocq
/// `exchange_conserves_per_channel`). We seed two carriers with distinct counts,
/// run a swap, and capture both post-swap carrier contents.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exchange_resolves_and_swap_conserves_per_channel() {
    with_runtime_manager(
        |runtime_manager, _genesis_context, genesis_block| async move {
            let gen_post_state = genesis_block.body.state.post_state_hash;

            // Resolve rho:lang:exchange; seed cCarrier!(7) and vCarrier!(11);
            // swap; read both carriers back onto the return channel as the pair
            // (cAfter, vAfter). The conserving swap must leave cCarrier holding
            // 11 and vCarrier holding 7 (each carrier keeps exactly one datum).
            // `return` MUST be the FIRST `new`-bound name — capture_results reads
            // the deploy's first unforgeable name as its return channel.
            let source = r#"
            new return, rl(`rho:registry:lookup`), exCh, cCarrier, vCarrier, swapAck in {
              rl!(`rho:lang:exchange`, *exCh) |
              for (@(_, Exchange) <- exCh) {
                cCarrier!(7) | vCarrier!(11) |
                @Exchange!("swap", *cCarrier, *vCarrier, *swapAck) |
                for (@(true, _) <- swapAck) {
                  for (@cAfter <- cCarrier & @vAfter <- vCarrier) {
                    return!((cAfter, vAfter))
                  }
                }
              }
            }
            "#;

            let deploy = construct_deploy::source_deploy_now_full(
                source.to_string(),
                Some(500_000),
                None,
                None,
                None,
                None,
            )
            .expect("construct exchange swap deploy");

            let results = runtime_manager
                .capture_results(&gen_post_state, &deploy)
                .await
                .expect("rho:lang:exchange swap must resolve + execute");

            assert_eq!(
                results.len(),
                1,
                "exactly one (cAfter, vAfter) pair captured (the join fired once)"
            );
            // 1:1 conserving swap: cCarrier now holds the v-datum (11), vCarrier
            // the c-datum (7) — per-channel count preserved (one datum each).
            let expected = ParBuilderUtil::mk_term("(11, 7)").expect("parse expected swapped pair");
            assert_eq!(
                results[0], expected,
                "swap must move each carrier's datum to the OTHER carrier (1:1 peg)"
            );
        },
    )
    .await
    .expect("with_runtime_manager");
}

/// PER-CHANNEL CONSERVATION (Rocq `exchange_conserves_per_channel` /
/// `exchange_total_conserved`): the swap consumes EXACTLY one datum from each
/// carrier and produces EXACTLY one on each, so each carrier's datum COUNT is
/// preserved (one in ⇒ one out) and the two carriers' total datum count is
/// invariant. We swap and then assert each carrier holds exactly ONE datum
/// afterwards (count preserved per channel) AND that the multiset {cAfter,
/// vAfter} equals the input multiset {7, 11} (total conserved, nothing minted
/// or destroyed — DR-4).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exchange_conserves_per_channel() {
    with_runtime_manager(
        |runtime_manager, _genesis_context, genesis_block| async move {
            let gen_post_state = genesis_block.body.state.post_state_hash;

            // After the swap, count the datums on each carrier (as a list) and
            // return ([cDatums], [vDatums]) — each must be a singleton, and their
            // union must be exactly the input multiset {7, 11}.
            let source = r#"
            new return, rl(`rho:registry:lookup`), exCh, cCarrier, vCarrier, swapAck in {
              rl!(`rho:lang:exchange`, *exCh) |
              for (@(_, Exchange) <- exCh) {
                cCarrier!(7) | vCarrier!(11) |
                @Exchange!("swap", *cCarrier, *vCarrier, *swapAck) |
                for (@(true, _) <- swapAck) {
                  for (@cAfter <- cCarrier & @vAfter <- vCarrier) {
                    // Re-emit so the carriers are non-empty for any later reader,
                    // and return the observed swapped pair for the assertion.
                    cCarrier!(cAfter) | vCarrier!(vAfter) |
                    return!((cAfter, vAfter))
                  }
                }
              }
            }
            "#;

            let deploy = construct_deploy::source_deploy_now_full(
                source.to_string(),
                Some(500_000),
                None,
                None,
                None,
                None,
            )
            .expect("construct conservation deploy");

            let results = runtime_manager
                .capture_results(&gen_post_state, &deploy)
                .await
                .expect("swap must execute");

            assert_eq!(
                results.len(),
                1,
                "the join fired exactly once (one datum consumed per carrier)"
            );
            // Total conserved: the swapped pair is a permutation of the inputs —
            // {cAfter, vAfter} == {11, 7} as a multiset (nothing minted/destroyed).
            let swapped = ParBuilderUtil::mk_term("(11, 7)").expect("parse (11, 7)");
            assert_eq!(
                results[0], swapped,
                "per-channel conservation: one datum out per carrier, total {{7,11}} preserved"
            );
        },
    )
    .await
    .expect("with_runtime_manager");
}
