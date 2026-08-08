use std::collections::HashMap;

use crypto::rust::public_key::PublicKey;
use models::casper::{BlockEventInfo, TransferInfo};
use models::rhoapi::Par;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::transaction::helpers;

/// Extract transfers from a `BlockEventInfo` for each deploy, keyed by deploy signature.
///
/// Scans each deploy's execution report for COMM events on the `transfer_unforgeable`
/// channel, then extracts from/to/amount/success from the produce data.
///
/// Only extracts user deploy transfers (not PreCharge/Refund/System deploys),
/// where sender == deployer.
pub fn extract_transfers_from_report(
    report: &BlockEventInfo,
    transfer_unforgeable: &Par,
) -> HashMap<String, Vec<TransferInfo>> {
    let mut transfers_by_deploy: HashMap<String, Vec<TransferInfo>> = HashMap::new();

    for deploy in &report.deploys {
        let deploy_sig = deploy
            .deploy_info
            .as_ref()
            .map(|info| info.sig.clone())
            .unwrap_or_default();

        let deployer_pk = deploy
            .deploy_info
            .as_ref()
            .map(|info| info.deployer.clone())
            .unwrap_or_default();

        let Ok(pk_bytes) = hex::decode(&deployer_pk) else {
            continue;
        };
        let pk = PublicKey::from_bytes(&pk_bytes);
        let Some(deployer_vault) = VaultAddress::from_public_key(&pk) else {
            continue;
        };
        let deployer_addr = deployer_vault.to_base58();

        let user_transfers: Vec<TransferInfo> = deploy
            .report
            .iter()
            .flat_map(|single_report| find_transfers_in_report(single_report, transfer_unforgeable))
            .filter(|t| t.from_addr == deployer_addr)
            .skip(1)
            .collect();

        if !user_transfers.is_empty() {
            transfers_by_deploy.insert(deploy_sig, user_transfers);
        }
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

    fn make_deploy_reports(
        transfer_unforgeable: &Par,
        from: &str,
        to: &str,
        amount: i64,
    ) -> Vec<SingleReport> {
        // Precharge: system transfer from deployer to validator (fixed gas payment)
        let precharge_ret = make_transfer_unforgeable();
        let precharge_data = ListParWithRandom {
            pars: vec![
                make_par_string(from),
                Par::default(),
                make_par_string("validator_addr"),
                make_par_int(100),
                Par::default(),
                precharge_ret,
            ],
            random_state: vec![],
        };

        let precharge_comm = ReportCommProto {
            consume: Some(ReportConsumeProto {
                channels: vec![transfer_unforgeable.clone()],
                patterns: vec![],
                peeks: vec![],
            }),
            produces: vec![ReportProduceProto {
                channel: Some(transfer_unforgeable.clone()),
                data: Some(precharge_data),
            }],
        };

        // User transfer: actual user-initiated transfer
        let user_ret = make_transfer_unforgeable();
        let user_data = ListParWithRandom {
            pars: vec![
                make_par_string(from),
                Par::default(),
                make_par_string(to),
                make_par_int(amount),
                Par::default(),
                user_ret,
            ],
            random_state: vec![],
        };

        let user_comm = ReportCommProto {
            consume: Some(ReportConsumeProto {
                channels: vec![transfer_unforgeable.clone()],
                patterns: vec![],
                peeks: vec![],
            }),
            produces: vec![ReportProduceProto {
                channel: Some(transfer_unforgeable.clone()),
                data: Some(user_data),
            }],
        };

        let precharge_report = SingleReport {
            events: vec![ReportProto {
                report: Some(report_proto::Report::Comm(precharge_comm)),
            }],
        };

        let user_report = SingleReport {
            events: vec![ReportProto {
                report: Some(report_proto::Report::Comm(user_comm)),
            }],
        };

        vec![precharge_report, user_report]
    }

    fn make_block_event_info_with_transfer(
        deploy_sig: &str,
        transfer_unforgeable: &Par,
        _from: &str,
        to: &str,
        amount: i64,
        deployer_hex: &str,
    ) -> BlockEventInfo {
        let deployer_vault = {
            let pk_bytes = hex::decode(deployer_hex).unwrap();
            let pk = PublicKey::from_bytes(&pk_bytes);
            VaultAddress::from_public_key(&pk).unwrap()
        };
        let deployer_addr = deployer_vault.to_base58();

        BlockEventInfo {
            block_info: Some(LightBlockInfo::default()),
            deploys: vec![DeployInfoWithEventData {
                deploy_info: Some(DeployInfo {
                    sig: deploy_sig.to_string(),
                    deployer: deployer_hex.to_string(),
                    ..Default::default()
                }),
                report: make_deploy_reports(transfer_unforgeable, &deployer_addr, to, amount),
            }],
            system_deploys: vec![],
            post_state_hash: vec![].into(),
        }
    }

    #[test]
    fn extract_transfers_finds_user_transfers() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();
        let report = make_block_event_info_with_transfer(
            "deploy_abc",
            &transfer_unforgeable,
            &deployer_addr,
            "receiver_addr",
            1000,
            &deployer_hex,
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
    fn extract_transfers_returns_empty_for_no_transfer_deploy() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, _deployer_addr) = make_deployer();

        // Deploy with empty reports (no COMM events on transfer channel)
        let report = BlockEventInfo {
            block_info: Some(LightBlockInfo::default()),
            deploys: vec![DeployInfoWithEventData {
                deploy_info: Some(DeployInfo {
                    sig: "deploy_no_transfer".to_string(),
                    deployer: deployer_hex,
                    ..Default::default()
                }),
                report: vec![SingleReport { events: vec![] }, SingleReport {
                    events: vec![],
                }],
            }],
            system_deploys: vec![],
            post_state_hash: vec![].into(),
        };

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        assert!(
            result.get("deploy_no_transfer").is_none(),
            "deploy with no transfers should have no map entry at all"
        );
    }

    #[test]
    fn extract_transfers_two_users() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer1_hex, deployer1_addr) = make_deployer();
        let (deployer2_hex, deployer2_addr) = make_deployer();

        let report = BlockEventInfo {
            block_info: Some(LightBlockInfo::default()),
            deploys: vec![
                DeployInfoWithEventData {
                    deploy_info: Some(DeployInfo {
                        sig: "deploy_user1".to_string(),
                        deployer: deployer1_hex.clone(),
                        ..Default::default()
                    }),
                    report: make_deploy_reports(
                        &transfer_unforgeable,
                        &deployer1_addr,
                        "receiver_1",
                        500,
                    ),
                },
                DeployInfoWithEventData {
                    deploy_info: Some(DeployInfo {
                        sig: "deploy_user2".to_string(),
                        deployer: deployer2_hex.clone(),
                        ..Default::default()
                    }),
                    report: make_deploy_reports(
                        &transfer_unforgeable,
                        &deployer2_addr,
                        "receiver_2",
                        750,
                    ),
                },
            ],
            system_deploys: vec![],
            post_state_hash: vec![].into(),
        };

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers1 = result
            .get("deploy_user1")
            .expect("should have user1 deploy entry");
        assert_eq!(transfers1.len(), 1, "user1 should have one user transfer");
        let t1 = &transfers1[0];
        assert_eq!(t1.from_addr, deployer1_addr);
        assert_eq!(t1.to_addr, "receiver_1");
        assert_eq!(t1.amount, 500);
        assert!(t1.success);

        let transfers2 = result
            .get("deploy_user2")
            .expect("should have user2 deploy entry");
        assert_eq!(transfers2.len(), 1, "user2 should have one user transfer");
        let t2 = &transfers2[0];
        assert_eq!(t2.from_addr, deployer2_addr);
        assert_eq!(t2.to_addr, "receiver_2");
        assert_eq!(t2.amount, 750);
        assert!(t2.success);

        assert_eq!(result.len(), 2, "should have exactly 2 deploy entries");
    }
}
