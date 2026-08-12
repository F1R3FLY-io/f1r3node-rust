use std::collections::HashMap;
use std::sync::OnceLock;

use casper::rust::genesis::contracts::standard_deploys::SYSTEM_VAULT_PUB_KEY;
use crypto::rust::public_key::PublicKey;
use models::casper::{BlockEventInfo, TransferInfo};
use models::rhoapi::Par;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::transaction::helpers;

/// Base58 vault address of the PoS system vault. The precharge transfer pays
/// the deployer's phlo budget here and the refund returns the unused remainder
/// from here, so this address is the order-free marker that distinguishes those
/// system transfers from genuine user transfers.
fn system_vault_addr() -> &'static str {
    static ADDR: OnceLock<String> = OnceLock::new();
    ADDR.get_or_init(|| {
        VaultAddress::from_public_key(&SYSTEM_VAULT_PUB_KEY)
            .expect("SYSTEM_VAULT_PUB_KEY must be a valid curve point")
            .to_base58()
    })
}

/// Extract transfers from a `BlockEventInfo` for each deploy, keyed by deploy signature.
///
/// Scans each deploy's execution report for COMM events on the `transfer_unforgeable`
/// channel, then extracts from/to/amount/success from the produce data.
///
/// Only extracts user deploy transfers (not PreCharge/Refund/System deploys).
/// The deployer address is derived from `DeployInfo.deployer` so that transfer
/// attribution does not depend on the precharge batch being parseable.
///
/// The precharge is identified structurally — its recipient is the PoS system
/// vault and its amount equals `phlo_limit * phlo_price` — rather than by
/// batch position, so the result is independent of which report batch the
/// precharge lands in. Genesis replays run without cost accounting, so no
/// precharge transfer is produced there and nothing is excluded.
pub fn extract_transfers_from_report(
    report: &BlockEventInfo,
    transfer_unforgeable: &Par,
) -> HashMap<String, Vec<TransferInfo>> {
    let mut transfers_by_deploy: HashMap<String, Vec<TransferInfo>> = HashMap::new();
    let system_vault = system_vault_addr();

    for deploy in &report.deploys {
        let deploy_info = deploy.deploy_info.as_ref();
        let deploy_sig = deploy_info.map(|info| info.sig.clone()).unwrap_or_default();

        if deploy.report.is_empty() {
            continue;
        }

        let deployer_addr = deploy_info.and_then(|info| {
            let pk_bytes = hex::decode(&info.deployer).ok()?;
            let pk = PublicKey::from_bytes(&pk_bytes);
            let vault = VaultAddress::from_public_key(&pk)?;
            Some(vault.to_base58())
        });

        // Amount the precharge moves from the deployer's vault to the system
        // vault. Matches `DeployData::total_phlo_charge` used to build the
        // PreChargeDeploy. `wrapping_mul` mirrors release-mode `*` so the
        // comparison stays consistent with the value the runtime produced.
        let precharge_amount = deploy_info
            .map(|info| info.phlo_limit.wrapping_mul(info.phlo_price))
            .unwrap_or(0);

        let mut user_transfers = Vec::new();
        for single_report in &deploy.report {
            for transfer in find_transfers_in_report(single_report, transfer_unforgeable) {
                let from_deployer = deployer_addr
                    .as_deref()
                    .is_some_and(|addr| transfer.from_addr == addr);
                // The precharge is the deployer-funded transfer to the PoS
                // system vault for exactly the authorised phlo charge.
                let is_precharge = precharge_amount > 0
                    && transfer.to_addr == *system_vault
                    && transfer.amount == precharge_amount;
                if from_deployer && !is_precharge {
                    user_transfers.push(transfer);
                }
            }
        }

        transfers_by_deploy.insert(deploy_sig, user_transfers);
    }

    transfers_by_deploy
}

/// Scan a single report for transfer events on the transfer_unforgeable channel.
fn find_transfers_in_report(
    report: &models::casper::SingleReport,
    transfer_unforgeable: &Par,
) -> Vec<TransferInfo> {
    let mut transfers = Vec::new();

    // Collect raw transactions from Comm events
    let mut raw_transactions: Vec<RawTransfer> = Vec::new();
    for event in &report.events {
        if let Some(models::casper::report_proto::Report::Comm(comm)) = &event.report {
            if let Some(channel) = comm.consume.as_ref().and_then(|c| c.channels.first()) {
                if *channel == *transfer_unforgeable {
                    if let Some(produce) = comm.produces.first() {
                        if let Some(tx) = helpers::parse_transaction_from_produce(produce) {
                            raw_transactions.push(RawTransfer {
                                from_addr: tx.from_addr,
                                to_addr: tx.to_addr,
                                amount: tx.amount,
                                ret_unforgeable: tx.ret_unforgeable,
                            });
                        }
                    }
                }
            }
        }
    }

    // Collect failure info from Produce events
    let ret_unforgeables: std::collections::HashSet<Par> = raw_transactions
        .iter()
        .map(|t| t.ret_unforgeable.clone())
        .collect();

    let mut failed_map: HashMap<Par, Option<String>> = HashMap::new();
    for event in &report.events {
        if let Some(models::casper::report_proto::Report::Produce(produce)) = &event.report {
            if let Some(channel) = &produce.channel {
                if ret_unforgeables.contains(channel) {
                    if let Some(fail_reason) = helpers::parse_failure_from_produce(&produce.data) {
                        failed_map.insert(channel.clone(), fail_reason);
                    }
                }
            }
        }
    }

    // Build TransferInfo with success/failure
    for tx in raw_transactions {
        let fail_reason = failed_map.get(&tx.ret_unforgeable).cloned().flatten();
        transfers.push(TransferInfo {
            from_addr: tx.from_addr,
            to_addr: tx.to_addr,
            amount: tx.amount,
            success: fail_reason.is_none(),
            fail_reason: fail_reason.unwrap_or_default(),
        });
    }

    transfers
}

struct RawTransfer {
    from_addr: String,
    to_addr: String,
    amount: i64,
    ret_unforgeable: Par,
}

#[cfg(test)]
mod tests {
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::casper::{
        report_proto, BlockEventInfo, DeployInfo, DeployInfoWithEventData, LightBlockInfo,
        ReportCommProto, ReportConsumeProto, ReportProduceProto, ReportProto, SingleReport,
    };
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{Expr, GPrivate, GUnforgeable, ListParWithRandom};

    use super::*;

    fn make_deployer() -> (String, String) {
        let secp256k1 = Secp256k1;
        let (_sk, pk) = secp256k1.new_key_pair();
        let deployer_hex = hex::encode(&pk.bytes);
        let vault_addr = VaultAddress::from_public_key(&pk)
            .expect("freshly generated key should be a valid curve point")
            .to_base58();
        (deployer_hex, vault_addr)
    }

    fn make_par_string(s: &str) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GString(s.to_string())),
            }],
            ..Default::default()
        }
    }

    fn make_par_int(n: i64) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GInt(n)),
            }],
            ..Default::default()
        }
    }

    fn make_transfer_unforgeable() -> Par {
        Par {
            unforgeables: vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![0x42; 32] })),
            }],
            ..Default::default()
        }
    }

    fn make_comm_event(
        transfer_unforgeable: &Par,
        from: &str,
        to: &str,
        amount: i64,
    ) -> ReportCommProto {
        let ret_unforg = make_transfer_unforgeable();
        let produce_data = ListParWithRandom {
            pars: vec![
                make_par_string(from),
                Par::default(),
                make_par_string(to),
                make_par_int(amount),
                Par::default(),
                ret_unforg,
            ],
            random_state: vec![],
        };

        ReportCommProto {
            consume: Some(ReportConsumeProto {
                channels: vec![transfer_unforgeable.clone()],
                patterns: vec![],
                peeks: vec![],
            }),
            produces: vec![ReportProduceProto {
                channel: Some(transfer_unforgeable.clone()),
                data: Some(produce_data),
            }],
        }
    }

    fn make_transfer_report(
        transfer_unforgeable: &Par,
        from: &str,
        to: &str,
        amount: i64,
    ) -> SingleReport {
        SingleReport {
            events: vec![ReportProto {
                report: Some(report_proto::Report::Comm(make_comm_event(
                    transfer_unforgeable,
                    from,
                    to,
                    amount,
                ))),
            }],
        }
    }

    /// Build a `DeployInfo` whose `phlo_limit * phlo_price` matches the
    /// precharge amount used by the test precharge transfers (100). Tests that
    /// include a precharge transfer must send it to `system_vault_addr()` for
    /// exactly this amount so the structural filter recognises it.
    fn make_deploy_info(deploy_sig: &str, deployer_hex: &str) -> DeployInfo {
        DeployInfo {
            sig: deploy_sig.to_string(),
            deployer: deployer_hex.to_string(),
            phlo_price: 1,
            phlo_limit: 100,
            ..Default::default()
        }
    }

    fn make_block_event(deploy: DeployInfoWithEventData) -> BlockEventInfo {
        BlockEventInfo {
            block_info: Some(LightBlockInfo::default()),
            deploys: vec![deploy],
            system_deploys: vec![],
            post_state_hash: vec![].into(),
        }
    }

    fn make_deploy_with_transfer(
        deploy_sig: &str,
        transfer_unforgeable: &Par,
        deployer_hex: &str,
        deployer_addr: &str,
        to: &str,
        amount: i64,
    ) -> BlockEventInfo {
        let precharge_report = make_transfer_report(
            transfer_unforgeable,
            deployer_addr,
            system_vault_addr(),
            100,
        );
        let user_report = make_transfer_report(transfer_unforgeable, deployer_addr, to, amount);
        make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info(deploy_sig, deployer_hex)).into(),
            report: vec![precharge_report, user_report],
        })
    }

    #[test]
    fn extract_transfers_finds_user_transfers() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();
        let report = make_deploy_with_transfer(
            "deploy_abc",
            &transfer_unforgeable,
            &deployer_hex,
            &deployer_addr,
            "receiver_addr",
            1000,
        );

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result.get("deploy_abc").expect("should have deploy entry");
        assert_eq!(transfers.len(), 1, "should have one user transfer");

        let t = &transfers[0];
        assert_eq!(t.from_addr, deployer_addr);
        assert_eq!(t.to_addr, "receiver_addr");
        assert_eq!(t.amount, 1000);
        assert!(t.success);
        assert!(t.fail_reason.is_empty());
    }

    #[test]
    fn extract_transfers_handles_precharge_absent() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 500);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info("deploy_single_batch", &deployer_hex)).into(),
            report: vec![user_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_single_batch")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            1,
            "single-batch user transfer must not be dropped"
        );
        assert_eq!(transfers[0].amount, 500);
    }

    #[test]
    fn extract_transfers_handles_unparseable_precharge() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let empty_precharge = SingleReport { events: vec![] };
        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 300);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info("deploy_no_precharge_tx", &deployer_hex)).into(),
            report: vec![empty_precharge, user_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_no_precharge_tx")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            1,
            "user transfer must survive unparseable precharge"
        );
        assert_eq!(transfers[0].amount, 300);
    }

    #[test]
    fn extract_transfers_excludes_refund_and_system_side_effects() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let precharge_report = make_transfer_report(
            &transfer_unforgeable,
            &deployer_addr,
            system_vault_addr(),
            100,
        );
        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 700);
        let refund_report = make_transfer_report(
            &transfer_unforgeable,
            system_vault_addr(),
            &deployer_addr,
            50,
        );
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info("deploy_with_refund", &deployer_hex)).into(),
            report: vec![precharge_report, user_report, refund_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_with_refund")
            .expect("should have deploy entry");
        assert_eq!(transfers.len(), 1, "only the user transfer should remain");
        assert_eq!(transfers[0].to_addr, "receiver_addr");
        assert_eq!(transfers[0].amount, 700);
    }

    #[test]
    fn extract_transfers_preserves_entry_for_no_transfer_deploy() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, _deployer_addr) = make_deployer();

        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info("deploy_no_transfer", &deployer_hex)).into(),
            report: vec![SingleReport { events: vec![] }, SingleReport {
                events: vec![],
            }],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_no_transfer")
            .expect("deploy with non-empty report must always receive a map entry");
        assert!(transfers.is_empty(), "should have no transfers");
    }

    #[test]
    fn extract_transfers_skips_empty_report_deploy() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, _deployer_addr) = make_deployer();

        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info("deploy_empty_report", &deployer_hex)).into(),
            report: vec![],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        assert!(
            !result.contains_key("deploy_empty_report"),
            "deploy with empty report should not receive a map entry"
        );
    }

    /// Regression guard for the order-dependence bug: the precharge is
    /// identified by recipient + amount, not by batch position, so a
    /// precharge landing in a non-first batch must still be excluded and the
    /// genuine first-batch user transfer must survive. Under the old
    /// `.skip(1)` logic this case silently swapped the precharge for the user
    /// transfer in the API response.
    #[test]
    fn extract_transfers_precharge_in_non_first_batch_is_excluded() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 700);
        let precharge_report = make_transfer_report(
            &transfer_unforgeable,
            &deployer_addr,
            system_vault_addr(),
            100,
        );
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info("deploy_precharge_second", &deployer_hex)).into(),
            report: vec![user_report, precharge_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_precharge_second")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            1,
            "precharge in a non-first batch must still be excluded"
        );
        assert_eq!(transfers[0].to_addr, "receiver_addr");
        assert_eq!(transfers[0].amount, 700);
    }

    /// Regression guard for the genesis-block bug: genesis deploys replay
    /// without cost accounting, so they produce no precharge. When such a
    /// deploy happens to span multiple report batches, every batch is a
    /// user-deploy batch and the first deployer-funded transfer is genuine.
    /// Under the old `.skip(1)` logic this first transfer was discarded.
    #[test]
    fn extract_transfers_genesis_multi_batch_keeps_first_transfer() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        // Two genuine user transfers across two batches, no precharge. A
        // genesis deploy carries no precharge, so deploy info is left with
        // default phlo pricing (charge amount 0 -> nothing to exclude).
        let first_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_a", 111);
        let second_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_b", 222);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(DeployInfo {
                sig: "deploy_genesis".to_string(),
                deployer: deployer_hex,
                ..Default::default()
            })
            .into(),
            report: vec![first_report, second_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_genesis")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            2,
            "genesis user transfers must not be dropped when multiple batches are present"
        );
        assert_eq!(transfers[0].to_addr, "receiver_a");
        assert_eq!(transfers[0].amount, 111);
        assert_eq!(transfers[1].to_addr, "receiver_b");
        assert_eq!(transfers[1].amount, 222);
    }
}
