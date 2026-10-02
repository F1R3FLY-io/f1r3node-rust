use std::sync::Arc;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingIntentV1, PhloFundingIntentV2,
};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::*;
use crate::rust::rholang::runtime::RuntimeOps;

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10_000_000)))
}

fn receipt() -> Receipt<'static> {
    Receipt {
        trade_id: [7; 32],
        source_identity: [8; 32],
        custody: b"purse-b",
        asset: b"REV",
        unspent: 5,
    }
}

fn intent() -> PhloFundingIntentV2<'static> {
    let source = PhloSourcePolicyV1::new(b"purse-b", 10, 10, true, vec![], PhloSourceLimits {
        wire: PhloWireLimits {
            total_bytes: 1024,
            field_bytes: 1024,
        },
        resource_permissions: 0,
        authority_nodes: 0,
    })
    .unwrap();
    PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 10,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![],
            },
            schedule_commitment: [9; 32],
            total_exposure: 10,
            sources: vec![source],
        },
        grant_uses: vec![],
        conversion: PhloConversionCompositionV2::SeparatePrior {
            accepted_trade: [7; 32],
            output_source_index: 0,
            output_asset: b"REV",
        },
    }
}

async fn fixture() -> (RuntimeManager, RuntimeOps) {
    let stores = RSpaceStore {
        history: Arc::new(InMemoryKeyValueStore::new()),
        roots: Arc::new(InMemoryKeyValueStore::new()),
        cold: Arc::new(InMemoryKeyValueStore::new()),
    };
    let (manager, _) = RuntimeManager::create_with_history(
        stores,
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        Arc::new(Default::default()),
        ExternalServices::noop(),
    );
    let runtime = RuntimeOps::new(manager.spawn_runtime().await);
    (manager, runtime)
}

#[tokio::test]
async fn accepted_trade_reader_requires_a_protected_rooted_receipt() {
    let (manager, mut runtime) = fixture().await;
    let empty_root: [u8; 32] = runtime
        .runtime
        .create_checkpoint()
        .await
        .root
        .bytes()
        .try_into()
        .unwrap();
    assert_eq!(
        manager.read_accepted_trade_receipt(empty_root, [7; 32], &budget()),
        Err(AcceptedTradeStateError::IssuerUnavailable)
    );
    let encoded = receipt().encode().unwrap();
    let name = channel([7; 32]);
    runtime
        .runtime
        .reducer
        .space
        .produce(
            name,
            ListParWithRandom {
                pars: vec![RhoByteArray::create_par(encoded)],
                random_state: Vec::new(),
                cost_authority: None,
                cost_stack: None,
            },
            false,
        )
        .await
        .unwrap();
    let root: [u8; 32] = runtime
        .runtime
        .create_checkpoint()
        .await
        .root
        .bytes()
        .try_into()
        .unwrap();
    assert_eq!(
        manager.read_accepted_trade_receipt([99; 32], [7; 32], &budget()),
        Err(AcceptedTradeStateError::Root)
    );
    let trade = manager
        .read_accepted_trade_receipt(root, [7; 32], &budget())
        .unwrap();
    assert_eq!(trade.root, root);
    assert_eq!(trade.output_source_identity, [8; 32]);
    assert_eq!(trade.output_custody, b"purse-b");
    assert_eq!(trade.output_asset, b"REV");
    assert_eq!(trade.unspent_output, 5);
    let exhausted = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(matches!(
        manager.read_accepted_trade_receipt(root, [7; 32], &exhausted),
        Err(AcceptedTradeStateError::HostBudget(_))
    ));
    assert!(exhausted.is_rejected());
    runtime
        .runtime
        .reducer
        .space
        .produce(
            channel([7; 32]),
            ListParWithRandom {
                pars: vec![RhoByteArray::create_par(b"malformed".to_vec())],
                random_state: Vec::new(),
                cost_authority: None,
                cost_stack: None,
            },
            false,
        )
        .await
        .unwrap();
    let malformed_root: [u8; 32] = runtime
        .runtime
        .create_checkpoint()
        .await
        .root
        .bytes()
        .try_into()
        .unwrap();
    assert_eq!(
        manager.read_accepted_trade_receipt(malformed_root, [7; 32], &budget()),
        Err(AcceptedTradeStateError::Record)
    );
}

#[test]
fn accepted_trade_transition_binds_original_asset_and_exact_old_value() {
    let root = [3; 32];
    let trade = RootedAcceptedTrade {
        root,
        trade_id: [7; 32],
        output_source_identity: [8; 32],
        output_custody: b"purse-b".to_vec(),
        output_asset: b"REV".to_vec(),
        unspent_output: 5,
    };
    let output = RootedPhysicalCapacity {
        root,
        source_identity: [8; 32],
        custody: b"purse-b".to_vec(),
        asset: b"REV".to_vec(),
        available: 5,
    };
    let intent = intent();
    let prepared =
        plan_separate_prior_receipt_transition(root, &intent, &trade, &output, 4, 3, &budget())
            .unwrap()
            .unwrap();
    assert_eq!(prepared.realized_debit(), 3);
    assert_eq!(Receipt::decode(prepared.expected()).unwrap().unspent, 5);
    assert_eq!(Receipt::decode(prepared.replacement()).unwrap().unspent, 2);
    assert_eq!(
        prepared.check_live_old_value(root, Some(prepared.expected())),
        Ok(())
    );
    assert_eq!(
        prepared.check_live_old_value([4; 32], Some(prepared.expected())),
        Err(AcceptedTradeStateError::Root)
    );
    assert_eq!(
        prepared.check_live_old_value(root, Some(prepared.replacement())),
        Err(AcceptedTradeStateError::Stale)
    );
    assert_eq!(
        plan_separate_prior_receipt_transition(root, &intent, &trade, &output, 4, 5, &budget()),
        Err(AcceptedTradeStateError::Amount)
    );
    assert_eq!(
        plan_separate_prior_receipt_transition(root, &intent, &trade, &output, 4, 0, &budget()),
        Ok(None)
    );
}
