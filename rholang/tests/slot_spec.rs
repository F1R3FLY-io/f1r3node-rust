use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::Par;
use models::rust::normalizer_env::with_deployer_id;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::rev_stack::{RevStacks, WalletOwner};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString, RhoTuple2};
use rholang::rust::interpreter::system_processes::{BlockData, DeployData};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

struct Key {
    pk: PublicKey,
    owner: WalletOwner,
    address: String,
}

fn key() -> Key {
    let (_, pk) = Secp256k1.new_key_pair();
    let owner = WalletOwner::from_public_key(&pk).unwrap();
    let address = VaultAddress::from_public_key(&pk).unwrap().to_base58();
    Key { pk, owner, address }
}

async fn runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    create_runtimes(store, false, &mut Vec::new()).await.0
}

fn stacks(runtime: &RhoRuntimeImpl) -> RevStacks {
    RevStacks::over_hot_store(runtime.reducer.space.hot_store().unwrap())
}

async fn at_block(runtime: &RhoRuntimeImpl, block_number: i64, signer: &Key) {
    runtime
        .set_block_data(BlockData {
            time_stamp: 1,
            block_number,
            sender: signer.pk.clone(),
            seq_num: 1,
        })
        .await;
    runtime
        .set_deploy_data(DeployData {
            timestamp: 1,
            deployer_id: signer.pk.clone(),
            deploy_id: vec![1],
            cosigners: Vec::new(),
            phlo_price: 1,
            phlo_limit: 0,
        })
        .await;
}

async fn run(runtime: &RhoRuntimeImpl, signer: &Key, term: &str) {
    let result = runtime
        .evaluate(
            term,
            Cost::create(i64::MAX, "slot spec".to_string()),
            with_deployer_id(&signer.pk),
            Blake2b512Random::create_from_bytes(&[]),
        )
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}

async fn replies(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<(bool, Par)> {
    runtime
        .get_data(&RhoString::create_par(channel.to_string()))
        .await
        .into_iter()
        .map(|d| {
            let (ok, value) = RhoTuple2::unapply(&d.a.pars[0]).unwrap();
            (RhoBoolean::unapply(&ok).unwrap(), value)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn holding_a_slot_name_is_the_authority_to_draw_on_its_stack() {
    let runtime = runtime().await;
    let (alice, bob) = (key(), key());
    stacks(&runtime).deposit(&alice.owner, 1_000).unwrap();
    at_block(&runtime, 1, &alice).await;
    let term = format!(
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), slot, funded, drawn in {{
             w!("transfer", (*d, *slot, 500), *funded) |
             for (_ <- funded) {{
               w!("transfer", (*slot, "{}", 200), *drawn) |
               for (@r <- drawn) {{ @"drawn"!(r) | w!("balance", *slot, "left") }}
             }}
           }}"#,
        bob.address
    );
    run(&runtime, &alice, &term).await;
    assert_eq!(
        replies(&runtime, "drawn").await,
        vec![(true, Par::default())]
    );
    let left = replies(&runtime, "left").await;
    assert_eq!(RhoNumber::unapply(&left[0].1), Some(300));
    assert_eq!(stacks(&runtime).balance(&bob.owner).unwrap(), 200);
    assert_eq!(stacks(&runtime).balance(&alice.owner).unwrap(), 500);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_grant_is_drawn_only_inside_its_window() {
    let runtime = runtime().await;
    let (sponsor, newcomer) = (key(), key());
    stacks(&runtime).deposit(&sponsor.owner, 1_000).unwrap();
    at_block(&runtime, 1, &sponsor).await;
    run(
        &runtime,
        &sponsor,
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), slot, funded in {
             w!("fundGrant", (*d, (*slot, 10, 20), 300), *funded) |
             for (_ <- funded) { @"slot"!(*slot) }
           }"#,
    )
    .await;
    let draw = |out: &str| {
        format!(
            r#"new w(`rho:rev:wallet`) in {{
                 for (@slot <<- @"slot") {{
                   w!("drawGrant", ((slot, 10, 20), "{}", 100), "{out}") |
                   w!("grantBalance", (slot, 10, 20), "{out}-left")
                 }}
               }}"#,
            newcomer.address
        )
    };
    for (block, out, ok) in [
        (5, "early", false),
        (15, "inside", true),
        (25, "late", false),
    ] {
        at_block(&runtime, block, &newcomer).await;
        run(&runtime, &newcomer, &draw(out)).await;
        assert_eq!(replies(&runtime, out).await[0].0, ok, "block {block}");
    }
    let left = replies(&runtime, "late-left").await;
    assert_eq!(RhoNumber::unapply(&left[0].1), Some(200));
    assert_eq!(stacks(&runtime).balance(&newcomer.owner).unwrap(), 100);
    assert_eq!(stacks(&runtime).balance(&sponsor.owner).unwrap(), 700);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_term_sealed_with_a_slot_it_holds_runs() {
    let runtime = runtime().await;
    let alice = key();
    at_block(&runtime, 1, &alice).await;
    run(
        &runtime,
        &alice,
        r#"new slot in { {% @"ran"!(1) %}[slot] }"#,
    )
    .await;
    let ran = runtime
        .get_data(&RhoString::create_par("ran".to_string()))
        .await;
    assert_eq!(ran.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_grant_returns_each_funders_share() {
    use rholang::rust::interpreter::accounting::rev_stack::funders::share;
    use rholang::rust::interpreter::accounting::rev_stack::grant::Grant;
    use rholang::rust::interpreter::accounting::rev_stack::HotStoreCells;
    use rholang::rust::interpreter::rho_type::RhoName;

    let runtime = runtime().await;
    let (sponsor, carol, newcomer, anyone) = (key(), key(), key(), key());
    stacks(&runtime).deposit(&sponsor.owner, 1_000).unwrap();
    stacks(&runtime).deposit(&carol.owner, 1_000).unwrap();
    at_block(&runtime, 1, &sponsor).await;
    run(
        &runtime,
        &sponsor,
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`), slot, funded in {
             w!("fundGrant", (*d, (*slot, 10, 20), 300), *funded) |
             for (_ <- funded) { @"slot"!!(*slot) }
           }"#,
    )
    .await;
    at_block(&runtime, 2, &carol).await;
    run(
        &runtime,
        &carol,
        r#"new w(`rho:rev:wallet`), d(`rho:system:deployerId`) in {
             for (@slot <<- @"slot") { w!("fundGrant", (*d, (slot, 10, 20), 100), "carol-funded") }
           }"#,
    )
    .await;
    assert!(replies(&runtime, "carol-funded").await[0].0);

    at_block(&runtime, 15, &newcomer).await;
    run(
        &runtime,
        &newcomer,
        &format!(
            r#"new w(`rho:rev:wallet`) in {{
                 for (@slot <<- @"slot") {{ w!("drawGrant", ((slot, 10, 20), "{}", 100), "drawn") }}
               }}"#,
            newcomer.address
        ),
    )
    .await;
    assert!(replies(&runtime, "drawn").await[0].0);

    let slot_par = runtime
        .get_data(&RhoString::create_par("slot".to_string()))
        .await[0]
        .a
        .pars[0]
        .clone();
    let slot = RhoName::unapply(&slot_par).unwrap();
    let grant = Grant::new(slot.id, 10, 20).unwrap().owner();
    let cells = HotStoreCells(runtime.reducer.space.hot_store().unwrap());
    assert_eq!(share(&cells, &grant, &sponsor.owner).unwrap(), 225);
    assert_eq!(share(&cells, &grant, &carol.owner).unwrap(), 75);

    let give_back = |funder: &str, out: &str| {
        format!(
            r#"new w(`rho:rev:wallet`) in {{
                 for (@slot <<- @"slot") {{ w!("returnGrant", ((slot, 10, 20), "{funder}"), "{out}") }}
               }}"#
        )
    };
    at_block(&runtime, 20, &anyone).await;
    run(&runtime, &anyone, &give_back(&sponsor.address, "too-soon")).await;
    assert!(!replies(&runtime, "too-soon").await[0].0);
    at_block(&runtime, 21, &anyone).await;
    run(&runtime, &anyone, &give_back(&sponsor.address, "sponsor-back")).await;
    run(&runtime, &anyone, &give_back(&carol.address, "carol-back")).await;
    assert!(replies(&runtime, "sponsor-back").await[0].0);
    assert!(replies(&runtime, "carol-back").await[0].0);
    assert_eq!(stacks(&runtime).balance(&sponsor.owner).unwrap(), 700 + 225);
    assert_eq!(stacks(&runtime).balance(&carol.owner).unwrap(), 900 + 75);
    assert_eq!(stacks(&runtime).balance(&grant).unwrap(), 0);
    run(&runtime, &anyone, &give_back(&carol.address, "again")).await;
    assert!(!replies(&runtime, "again").await[0].0);
}
