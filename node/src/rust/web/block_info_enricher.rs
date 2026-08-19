use std::collections::HashMap;

use crypto::rust::public_key::PublicKey;
use models::casper::{BlockEventInfo, TransferInfo};
use models::rhoapi::Par;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::transaction::helpers;

/// Extract user transfers from a report, keyed by deploy signature.
///
/// Each cost-accounted deploy's execution report is produced by running the
/// three phases precharge → user → refund in that fixed order, ending each
/// with a soft checkpoint. Genesis replays run without cost accounting and
/// produce no precharge batch at all. Genesis standard deploys are built with
/// `phlo_price: 0`, so their charge is `0`.
///
/// The precharge is excluded by position (`report[0]`) after a shape check:
/// exactly one deployer-funded transfer of `phlo_limit * phlo_price`. The
/// `to_addr` is NOT checked because the precharge pays to `posVaultAddr`, an
/// unforgeable-derived address that is not knowable in the web layer (it is
/// only exposed via `PoS(@"getInitialPosVault")` at runtime). The batch
/// order invariant is pinned by
/// `casper/tests/batch1/precharge_report_shape_spec.rs`. The refund is
/// excluded automatically because its sender is the PoS vault, not the
/// deployer, so the deployer-sender filter drops it.
///
/// If `report[0]` does not match the expected precharge shape (no transfer,
/// wrong sender, wrong amount, or more than one transfer), the invariant is
/// treated as violated: `report[0]` is *not* skipped, one warning per affected
/// deploy is emitted, and all deployer-funded transfers are returned. This
/// deliberately over-reports rather than silently dropping a genuine user
/// transfer, and the warning keeps the case diagnosable.
///
/// If the deployer address cannot be derived from `DeployInfo.deployer` (bad
/// hex, invalid public key, or missing info), a warning is emitted and the
/// deploy's transfers are dropped — silently losing user data here would repeat
/// the same shape of bug that the position-based precharge filter guards
/// against.
///
/// Map-entry contract: a deploy with a non-empty `report` always receives a
/// map entry (possibly with an empty vector); a deploy with an empty `report`
/// receives no entry.
pub fn extract_transfers_from_report(
    report: &BlockEventInfo,
    transfer_unforgeable: &Par,
) -> HashMap<String, Vec<TransferInfo>> {
    let mut transfers_by_deploy: HashMap<String, Vec<TransferInfo>> = HashMap::new();

    for deploy in &report.deploys {
        if deploy.report.is_empty() {
            continue;
        }

        let deploy_info = deploy.deploy_info.as_ref();
        let deploy_sig = deploy_info.map(|info| info.sig.clone()).unwrap_or_default();

        let deployer_addr = deploy_info.and_then(|info| {
            let pk_bytes = hex::decode(&info.deployer).ok()?;
            let pk = PublicKey::from_bytes(&pk_bytes);
            VaultAddress::from_public_key(&pk).map(|v| v.to_base58())
        });

        if deployer_addr.is_none() {
            tracing::warn!(
                target: "f1r3fly.node.web.enricher",
                deploy_sig = %deploy_sig,
                "deployer vault address could not be derived from deploy info; \
                 user transfers for this deploy will be dropped"
            );
            transfers_by_deploy.insert(deploy_sig, Vec::new());
            continue;
        }

        let precharge_amount = deploy_info
            .map(|info| info.phlo_limit.wrapping_mul(info.phlo_price))
            .unwrap_or(0);

        let batches: &[models::casper::SingleReport] = if precharge_amount == 0 {
            &deploy.report
        } else {
            let first = &deploy.report[0];
            let found =
                find_transfers_in_report(first, transfer_unforgeable, deployer_addr.as_deref());
            let precharge_consistent = found.len() == 1
                && deployer_addr.as_deref() == Some(found[0].from_addr.as_str())
                && found[0].amount == precharge_amount;

            if precharge_consistent {
                &deploy.report[1..]
            } else {
                if found.len() == 1 {
                    tracing::warn!(
                        target: "f1r3fly.node.web.enricher",
                        deploy_sig = %deploy_sig,
                        expected_amount = precharge_amount,
                        found_transfers = 1,
                        found_amount = found[0].amount,
                        found_sender = %found[0].from_addr,
                        "report[0] does not match the expected precharge shape; \
                         not skipping it — the precharge may appear as a user transfer"
                    );
                } else {
                    tracing::warn!(
                        target: "f1r3fly.node.web.enricher",
                        deploy_sig = %deploy_sig,
                        expected_amount = precharge_amount,
                        found_transfers = found.len(),
                        "report[0] does not match the expected precharge shape; \
                         not skipping it — the precharge may appear as a user transfer"
                    );
                }
                &deploy.report
            }
        };

        let mut user_transfers = Vec::new();
        for single_report in batches {
            user_transfers.extend(find_transfers_in_report(
                single_report,
                transfer_unforgeable,
                deployer_addr.as_deref(),
            ));
        }

        transfers_by_deploy.insert(deploy_sig, user_transfers);
    }

    transfers_by_deploy
}

/// Scan a single report for user transfer events on the transfer_unforgeable
/// channel. Only transfers whose sender is the deployer are returned; the
/// refund (PoS vault → deployer) and other non-deployer sends are dropped in
/// the first pass.
fn find_transfers_in_report(
    report: &models::casper::SingleReport,
    transfer_unforgeable: &Par,
    deployer_addr: Option<&str>,
) -> Vec<TransferInfo> {
    let deployer_addr = match deployer_addr {
        Some(addr) => addr,
        None => return Vec::new(),
    };

    let mut raw_transactions: Vec<RawTransfer> = Vec::new();
    for event in &report.events {
        if let Some(models::casper::report_proto::Report::Comm(comm)) = &event.report {
            if let Some(channel) = comm.consume.as_ref().and_then(|c| c.channels.first()) {
                if *channel == *transfer_unforgeable {
                    if let Some(produce) = comm.produces.first() {
                        if let Some(tx) = helpers::parse_transaction_from_produce(produce) {
                            if tx.from_addr == deployer_addr {
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
    }

    if raw_transactions.is_empty() {
        return Vec::new();
    }

    let mut failed: Vec<Option<String>> = vec![None; raw_transactions.len()];
    for event in &report.events {
        if let Some(models::casper::report_proto::Report::Produce(produce)) = &event.report {
            if let Some(channel) = &produce.channel {
                for (i, tx) in raw_transactions.iter().enumerate() {
                    if *channel == tx.ret_unforgeable {
                        if let Some(fail_reason) =
                            helpers::parse_failure_from_produce(&produce.data)
                        {
                            failed[i] = fail_reason;
                        }
                    }
                }
            }
        }
    }

    let mut transfers = Vec::with_capacity(raw_transactions.len());
    for (tx, fail_reason) in raw_transactions.into_iter().zip(failed.into_iter()) {
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

    fn init_logger() { shared::rust::tracing_init::init_for_tests(); }

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
        let ret_unforg_for_data = ret_unforg.clone();
        let produce_data = ListParWithRandom {
            pars: vec![
                make_par_string(from),
                Par::default(),
                make_par_string(to),
                make_par_int(amount),
                Par::default(),
                ret_unforg_for_data,
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
                channel: Some(ret_unforg),
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

    fn make_deploy_info_with_charge(
        deploy_sig: &str,
        deployer_hex: &str,
        phlo_limit: i64,
        phlo_price: i64,
    ) -> DeployInfo {
        DeployInfo {
            sig: deploy_sig.to_string(),
            deployer: deployer_hex.to_string(),
            phlo_limit,
            phlo_price,
            ..Default::default()
        }
    }

    fn make_block_event(deploy: DeployInfoWithEventData) -> BlockEventInfo {
        BlockEventInfo {
            block_info: Some(LightBlockInfo::default()).into(),
            deploys: vec![deploy],
            system_deploys: vec![],
            post_state_hash: vec![].into(),
        }
    }

    // 1. [precharge, user], amounts consistent → one user transfer, precharge
    //    excluded, no warning.
    #[test]
    fn precharge_then_user_consistent_excludes_precharge() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let precharge_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "pos_vault_addr", 100);
        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 1000);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_abc",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![precharge_report, user_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result.get("deploy_abc").expect("should have deploy entry");
        assert_eq!(transfers.len(), 1, "precharge excluded, user transfer kept");
        assert_eq!(transfers[0].to_addr, "receiver_addr");
        assert_eq!(transfers[0].amount, 1000);
        assert_eq!(transfers[0].from_addr, deployer_addr);
    }

    // 2. [precharge, user, refund] → only the user transfer (refund excluded
    //    by sender).
    #[test]
    fn precharge_user_refund_keeps_only_user_transfer() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let precharge_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "pos_vault_addr", 100);
        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 700);
        let refund_report =
            make_transfer_report(&transfer_unforgeable, "pos_vault_addr", &deployer_addr, 50);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_with_refund",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
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

    // 3. precharge_amount > 0, report[0] empty → nothing skipped, user
    //    transfers returned, warning emitted. Assert on observable behaviour
    //    (returned transfers), not on log output.
    #[test]
    fn precharge_amount_positive_but_report0_empty_returns_user_transfers() {
        init_logger();
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let empty_precharge = SingleReport { events: vec![] };
        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 300);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_empty_precharge",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![empty_precharge, user_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_empty_precharge")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            1,
            "report[0] not skipped; user transfer returned"
        );
        assert_eq!(transfers[0].amount, 300);
    }

    // 4. precharge_amount > 0, report[0] holds a transfer with the wrong
    //    amount → nothing skipped, that transfer is returned too, warning
    //    emitted.
    #[test]
    fn precharge_amount_positive_but_report0_wrong_amount_is_not_skipped() {
        init_logger();
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        // Expected precharge amount is 100, but report[0] carries 999 from the
        // deployer — wrong amount, so the shape check fails and report[0] is
        // not skipped.
        let wrong_precharge =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "pos_vault_addr", 999);
        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 500);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_wrong_precharge",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![wrong_precharge, user_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_wrong_precharge")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            2,
            "report[0] not skipped; both the wrong-amount transfer and the user transfer returned"
        );
        assert_eq!(transfers[0].amount, 999);
        assert_eq!(transfers[1].amount, 500);
    }

    // 5. Genesis shape: phlo_price == 0, several batches, no precharge → all
    //    transfers returned. Regression guard for the old `.skip(1)` genesis
    //    bug.
    #[test]
    fn genesis_shape_with_zero_phlo_price_keeps_all_transfers() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let first_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_a", 111);
        let second_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_b", 222);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_genesis",
                &deployer_hex,
                i64::MAX,
                0,
            ))
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

    // 6. Non-empty report, no transfers → entry present, vector empty.
    #[test]
    fn non_empty_report_with_no_transfers_has_empty_entry() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, _deployer_addr) = make_deployer();

        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_no_transfer",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
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

    // 7. Empty report → no entry.
    #[test]
    fn empty_report_deploy_gets_no_entry() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, _deployer_addr) = make_deployer();

        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_empty_report",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        assert!(
            !result.contains_key("deploy_empty_report"),
            "deploy with empty report should not receive a map entry"
        );
    }

    // 8. Multiple user transfers within one batch and across batches → all
    //    returned, order preserved.
    #[test]
    fn multiple_user_transfers_across_batches_all_returned_in_order() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let precharge_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "pos_vault_addr", 100);

        // First user batch holds two transfers in the same batch.
        let batch_with_two = SingleReport {
            events: vec![
                ReportProto {
                    report: Some(report_proto::Report::Comm(make_comm_event(
                        &transfer_unforgeable,
                        &deployer_addr,
                        "receiver_a",
                        10,
                    ))),
                },
                ReportProto {
                    report: Some(report_proto::Report::Comm(make_comm_event(
                        &transfer_unforgeable,
                        &deployer_addr,
                        "receiver_b",
                        20,
                    ))),
                },
            ],
        };

        // Second user batch holds one transfer.
        let batch_with_one =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_c", 30);

        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_multi",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![precharge_report, batch_with_two, batch_with_one],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_multi")
            .expect("should have deploy entry");
        assert_eq!(transfers.len(), 3, "all user transfers returned in order");
        assert_eq!(transfers[0].to_addr, "receiver_a");
        assert_eq!(transfers[0].amount, 10);
        assert_eq!(transfers[1].to_addr, "receiver_b");
        assert_eq!(transfers[1].amount, 20);
        assert_eq!(transfers[2].to_addr, "receiver_c");
        assert_eq!(transfers[2].amount, 30);
    }

    // The deployer is derived from `DeployInfo.deployer` only. A malformed
    // deployer hex (non-fatal) yields no matching transfers but still a map
    // entry, never a panic. This test also exercises the `precharge_amount > 0`
    // path (phlo_limit=100 * phlo_price=1 = 100): the shape check must be
    // skipped entirely when `deployer_addr` is `None`, so only the
    // "deployer vault address could not be derived" warning fires — not the
    // misleading "report[0] does not match the expected precharge shape"
    // warning.
    #[test]
    fn invalid_deployer_hex_yields_empty_entry_without_panic() {
        init_logger();
        let transfer_unforgeable = make_transfer_unforgeable();
        let (_deployer_hex, deployer_addr) = make_deployer();

        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 123);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(DeployInfo {
                sig: "deploy_bad_deployer".to_string(),
                deployer: "not-hex".to_string(),
                phlo_limit: 100,
                phlo_price: 1,
                ..Default::default()
            })
            .into(),
            report: vec![user_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_bad_deployer")
            .expect("non-empty report must still get a map entry");
        assert!(
            transfers.is_empty(),
            "no transfer matches an unparseable deployer"
        );
    }

    // 9. Precharge only: report == [precharge_batch], precharge_amount > 0,
    //    no user transfers. report[0] matches the precharge shape so it is
    //    skipped; report[1..] is empty, so the deploy still receives a map
    //    entry — an empty vector, not a missing key — and no warning is
    //    emitted.
    #[test]
    fn precharge_only_yields_empty_entry() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let precharge_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "pos_vault_addr", 100);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_precharge_only",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![precharge_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        // A deploy with a non-empty report always receives a map entry,
        // possibly empty — never a missing key.
        let transfers = result
            .get("deploy_precharge_only")
            .expect("precharge-only deploy with non-empty report must still receive a map entry");
        assert!(
            transfers.is_empty(),
            "report[0] is the consistent precharge and is skipped; no user transfers remain"
        );
    }

    // 10. User transfer batch precedes the precharge batch. report[0] holds a
    //     deployer-sent transfer whose amount != precharge_amount; report[1]
    //     holds the precharge. The shape check on report[0] fails (amount
    //     mismatch), so nothing is skipped: both transfers are returned and a
    //     warning is emitted. The genuine user transfer in report[0] is present.
    #[test]
    fn user_transfer_before_precharge_is_not_dropped() {
        init_logger();
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 777);
        let precharge_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "pos_vault_addr", 100);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_user_before_precharge",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![user_report, precharge_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_user_before_precharge")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            2,
            "report[0] not skipped; the user transfer and the precharge are both returned"
        );
        // The genuine user transfer must survive — never a silent drop.
        assert_eq!(transfers[0].to_addr, "receiver_addr");
        assert_eq!(transfers[0].amount, 777);
        assert_eq!(transfers[0].from_addr, deployer_addr);
        assert_eq!(transfers[1].to_addr, "pos_vault_addr");
        assert_eq!(transfers[1].amount, 100);
    }

    // 11. Precharge absent entirely but precharge_amount > 0: report contains
    //     only a single user-transfer batch. report[0] fails the shape check
    //     (its amount != precharge_amount), so nothing is skipped; the user
    //     transfer is returned and a warning is emitted.
    #[test]
    fn precharge_absent_one_user_transfer_is_returned() {
        init_logger();
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let user_report =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_addr", 456);
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_no_precharge_one_user",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![user_report],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_no_precharge_one_user")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            1,
            "report[0] not skipped; the single user transfer is returned"
        );
        assert_eq!(transfers[0].to_addr, "receiver_addr");
        assert_eq!(transfers[0].amount, 456);
        assert_eq!(transfers[0].from_addr, deployer_addr);
    }
}
