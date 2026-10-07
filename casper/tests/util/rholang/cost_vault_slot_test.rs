use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::vault_payer::balance_query_source;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use crypto::rust::signatures::signed::Signed;
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString};
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::rholang::resources::with_runtime_manager;

async fn apply_and_replay(
    manager: &RuntimeManager,
    state: &StateHash,
    deploy: Signed<DeployData>,
    block: BlockData,
) -> StateHash {
    let (post, processed, system) = manager
        .compute_state(state, vec![deploy], Vec::new(), block.clone(), None)
        .await
        .unwrap();
    assert_eq!(processed.len(), 1);
    assert!(!processed[0].is_failed);
    let replayed = manager
        .replay_compute_state(state, processed, system, &block, None, false)
        .await
        .unwrap();
    assert_eq!(post, replayed);
    post
}

async fn balance(manager: &RuntimeManager, root: &StateHash, address: &VaultAddress) -> i64 {
    let (values, _) = manager
        .play_exploratory_deploy(balance_query_source(address), root, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn data(manager: &RuntimeManager, root: &StateHash, name: &str) -> Vec<models::rhoapi::Par> {
    manager
        .get_data(
            root.clone(),
            &new_gstring_par(name.to_owned(), Vec::new(), false),
        )
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn legacy_cross_deploy_vault_funding_and_gateway_replay() {
    with_runtime_manager(|manager, genesis, genesis_block| async move {
        let shard = genesis_block.shard_id.clone();
        let initial = genesis_block.body.state.post_state_hash;
        let installer_key = genesis.genesis_vaults[0].0.clone();
        let sponsor_key = genesis.genesis_vaults[1].0.clone();
        let sponsor_address = VaultAddress::from_public_key(&genesis.genesis_vaults[1].1).unwrap();
        let gateway_key = genesis.genesis_vaults[2].0.clone();
        let gateway_public_key = hex::encode(&genesis.genesis_vaults[2].1.bytes);
        let sender = genesis.validator_pks()[0].clone();
        let block = |time_stamp: i64, block_number: i64| BlockData {
            time_stamp,
            block_number,
            sender: sender.clone(),
            seq_num: block_number as i32,
        };
        let installer_source = r#"new entry, slot, entryAddressCh, slotAddressCh,
            VaultAddress(`rho:vault:address`), DeployerIdOps(`rho:system:deployerId:ops`) in {
          for (@request, deployerId <= @"agent-trigger") {
            new publicKeyCh in {
              DeployerIdOps!("pubKeyBytes", *deployerId, *publicKeyCh) |
              for (@publicKey <- publicKeyCh) {
                if (publicKey == "GATEWAY_PUBLIC_KEY".hexToBytes()) {
                  {% for(@accepted <- entry) { @"agent-ran"!(true) } %}[entry -o slot] |
                  entry!(request)
                }
              }
            }
          } |
          entry :: () |
          VaultAddress!("fromUnforgeable", *entry, *entryAddressCh) |
          for (@entryAddress <- entryAddressCh) { @"agent-entry-address"!!(entryAddress) } |
          VaultAddress!("fromUnforgeable", *slot, *slotAddressCh) |
          for (@slotAddress <- slotAddressCh) { @"agent-slot-address"!!(slotAddress) }
        }"#
        .replace("GATEWAY_PUBLIC_KEY", &gateway_public_key);
        let installer = construct_deploy::source_deploy(
            installer_source,
            1,
            Some(1_000_000),
            None,
            Some(installer_key.clone()),
            None,
            Some(shard.clone()),
        )
        .unwrap();
        let installed = apply_and_replay(&manager, &initial, installer, block(1, 2)).await;
        let entry_values = data(&manager, &installed, "agent-entry-address").await;
        let slot_values = data(&manager, &installed, "agent-slot-address").await;
        assert_eq!(entry_values.len(), 1);
        assert_eq!(slot_values.len(), 1);
        let entry = VaultAddress::parse(&RhoString::unapply(&entry_values[0]).unwrap()).unwrap();
        let slot = VaultAddress::parse(&RhoString::unapply(&slot_values[0]).unwrap()).unwrap();
        assert_ne!(entry, slot);
        assert_eq!(balance(&manager, &installed, &entry).await, 0);
        assert_eq!(balance(&manager, &installed, &slot).await, 0);
        let sponsor_before = balance(&manager, &installed, &sponsor_address).await;
        let funded_amount = 100_000_i64;
        let funding_source = format!(
            r#"new rl(`rho:registry:lookup`), systemVaultCh, payerCh, authKeyCh,
                transferCh, deployerId(`rho:system:deployerId`) in {{
              rl!(`rho:vault:system`, *systemVaultCh) |
              for (@(_, systemVault) <- systemVaultCh) {{
                @systemVault!("find", "{}", *payerCh) |
                @systemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
                for (@(true, payer) <- payerCh & key <- authKeyCh) {{
                  @payer!("transferBatch", [("{}", {}), ("{}", {})], *key, *transferCh) |
                  for (@result <- transferCh) {{
                    match result {{
                      (true, _) => {{ @"agent-slot-funded"!(true) }}
                      _ => {{ @"agent-slot-funded"!(false) }}
                    }}
                  }}
                }}
              }}
            }}"#,
            sponsor_address.to_base58(),
            entry.to_base58(),
            funded_amount,
            slot.to_base58(),
            funded_amount,
        );
        let funding = construct_deploy::source_deploy(
            funding_source,
            2,
            Some(1_000_000),
            None,
            Some(sponsor_key),
            None,
            Some(shard.clone()),
        )
        .unwrap();
        let funded = apply_and_replay(&manager, &installed, funding, block(2, 3)).await;
        assert_eq!(balance(&manager, &funded, &entry).await, funded_amount);
        assert_eq!(balance(&manager, &funded, &slot).await, funded_amount);
        assert!(
            balance(&manager, &funded, &sponsor_address).await
                <= sponsor_before - 2 * funded_amount
        );
        let funded_marker = data(&manager, &funded, "agent-slot-funded").await;
        assert_eq!(funded_marker.len(), 1);
        assert_eq!(RhoBoolean::unapply(&funded_marker[0]), Some(true));
        let trigger_source =
            "new deployerId(`rho:system:deployerId`) in { @\"agent-trigger\"!(0, *deployerId) }"
                .to_owned();
        let unauthorized = construct_deploy::source_deploy(
            trigger_source.clone(),
            3,
            Some(1_000_000),
            None,
            Some(installer_key),
            None,
            Some(shard.clone()),
        )
        .unwrap();
        let rejected = apply_and_replay(&manager, &funded, unauthorized, block(3, 4)).await;
        assert!(data(&manager, &rejected, "agent-ran").await.is_empty());
        assert_eq!(balance(&manager, &rejected, &entry).await, funded_amount);
        assert_eq!(balance(&manager, &rejected, &slot).await, funded_amount);
        let trigger = construct_deploy::source_deploy(
            trigger_source,
            4,
            Some(1_000_000),
            None,
            Some(gateway_key),
            None,
            Some(shard),
        )
        .unwrap();
        let activated = apply_and_replay(&manager, &rejected, trigger, block(4, 5)).await;
        let ran = data(&manager, &activated, "agent-ran").await;
        assert_eq!(ran.len(), 1);
        assert_eq!(RhoBoolean::unapply(&ran[0]), Some(true));
        assert!(balance(&manager, &activated, &entry).await <= funded_amount);
        assert!(balance(&manager, &activated, &slot).await <= funded_amount);
    })
    .await
    .unwrap();
}
