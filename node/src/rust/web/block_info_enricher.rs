use std::collections::HashMap;

use crypto::rust::public_key::PublicKey;
use models::casper::{BlockEventInfo, ReportPhase, TransferInfo};
use models::rhoapi::Par;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use super::transaction::helpers;

/// Extract user transfers from a report, keyed by deploy signature.
///
/// ## Marked path (authoritative when present)
///
/// A cost-accounted deploy's execution report is produced by running the
/// three phases precharge -> user -> refund in that fixed order. Each
/// segment carries an explicit `ReportPhase` marker set by the
/// reporting rspace at the phase boundary. When any segment of a
/// deploy's report carries a marker other than `UNSPECIFIED`, the
/// marker is authoritative: only `USER` segments are scanned for
/// transfers where `from_addr == deployer_addr`, and `PRECHARGE` and
/// `REFUND` segments are dropped wholesale. No positional skip, no
/// shape check, and no `phlo_limit * phlo_price` arithmetic run on this
/// path. The `to_addr` is not checked because the precharge pays to an
/// unforgeable-derived vault that is not knowable in the web layer.
///
/// The rig (recorded-COMM) events are replayed before the first phase
/// emits anything, and `replay_deploy_e` sets the initial phase before
/// `rig`, so for a cost-accounted deploy the rig segment is tagged
/// `PRECHARGE` and dropped wholesale on the marked path. The fallback
/// path only dropped `report[0]` when the shape check passed, so this
/// is a deliberate difference between the two paths: the marker is
/// authoritative and does not depend on the precharge amount.
///
/// Genesis (`with_cost_accounting == false`) emits only `USER`
/// segments. System deploys emit only `UNSPECIFIED` segments.
///
/// ## Mixed report
///
/// If some segments are marked and some are `UNSPECIFIED` (a report
/// partly produced by a node on the new model and partly by an older
/// one), the marked path is used and `UNSPECIFIED` segments are treated
/// as `USER` rather than dropped. Losing data is the failure mode this
/// whole line of work exists to prevent. The choice is recorded here so
/// a future change does not silently narrow it.
///
/// ## Unrecognized phase values
///
/// A `phase` value that this build does not know (a future proto value,
/// or malformed data) is classified as unknown. It is not folded into
/// `UNSPECIFIED`. The policy is asymmetric on purpose:
///
/// - On the marked path, this module drops an unknown segment and logs
///   a warning. It never scans that segment as `USER`, because a future
///   system phase must not leak system transfers into the user list.
/// - When every segment is unknown, this module recognizes no marker,
///   so the report falls to the positional fallback. The fallback scans
///   those segments. The deployer-sender filter in
///   `find_transfers_in_report` still applies, so a refund-shaped
///   transfer cannot leak. Only a precharge that fails the shape check
///   can leak.
///
/// The first rule can drop user transfers that a newer node wrote in a
/// segment this build cannot name. This is the deliberate trade against
/// exposure of system transfers. It is the one place where this module
/// prefers loss over exposure.
///
/// ## Fallback path (compatibility)
///
/// When every segment is `UNSPECIFIED` (a node predating this change, or
/// a report replayed from data written before it), the positional
/// logic from the initial fix runs verbatim: `report[0]` is treated as
/// the precharge after a shape check (one deployer-sent transfer of
/// `phlo_limit * phlo_price`), and on mismatch it is not skipped plus a
/// warning. An unmarked report is expected during rollout, not a
/// defect, so the fallback is logged once per deploy at `debug!`.
///
/// The fallback and the `phlo_limit * phlo_price` shape check become
/// removable once every report-producing node is on the new model. They
/// are kept here for wire-compatibility both directions. Do not delete
/// them in this change (tracked as a follow-up).
///
/// ## Deployer derivation
///
/// If the deployer address cannot be derived from `DeployInfo.deployer`
/// (bad hex, invalid public key, or missing info), a warning is emitted
/// and the deploy's transfers are dropped. Silently losing user data
/// here would repeat the same shape of bug that the marker now guards
/// against.
///
/// Map-entry contract: a deploy with a non-empty `report` always
/// receives a map entry (possibly with an empty vector). A deploy with
/// an empty `report` receives no entry.
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
        let deploy_sig = deploy_info
            .map(|info| {
                if info.deploy_id.is_empty() {
                    info.sig.clone()
                } else {
                    hex::encode(&info.deploy_id)
                }
            })
            .unwrap_or_default();

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

        let classified: Vec<Option<ReportPhase>> = deploy
            .report
            .iter()
            .map(|b| classify_phase(b.phase))
            .collect();

        // A report is "marked" only when some segment carries a
        // recognized phase other than `Unspecified`. An unrecognized
        // value does not activate the marked path, so the report falls
        // to the positional fallback instead of being scanned as USER.
        let any_marked = classified
            .iter()
            .any(|c| c.map(is_marker_phase).unwrap_or(false));

        // Warn after the path is known. The two paths treat an
        // unrecognized segment differently, so one message cannot
        // describe both.
        if classified.iter().any(Option::is_none) {
            if any_marked {
                tracing::warn!(
                    target: "f1r3fly.node.web.enricher",
                    deploy_sig = %deploy_sig,
                    "report contains a segment with an unrecognized phase; \
                     that segment is dropped on the marked path"
                );
            } else {
                tracing::warn!(
                    target: "f1r3fly.node.web.enricher",
                    deploy_sig = %deploy_sig,
                    "report carries no recognized phase marker and contains \
                     unrecognized phase values; those segments are scanned \
                     by the positional fallback"
                );
            }
        }

        let mut user_transfers = Vec::new();
        if any_marked {
            // Marked path (also covers mixed reports): keep USER and
            // real UNSPECIFIED (treated as USER). Drop PRECHARGE and
            // REFUND wholesale. Ignore unrecognized discriminants
            // (warned above) so a future or malformed phase is never
            // scanned as USER. No positional skip, no shape check, no
            // amount arithmetic on this path.
            let has_unspecified = classified
                .iter()
                .any(|c| matches!(c, Some(ReportPhase::Unspecified)));
            if has_unspecified {
                tracing::debug!(
                    target: "f1r3fly.node.web.enricher",
                    deploy_sig = %deploy_sig,
                    "marked report also contains UNSPECIFIED segments; \
                     treating them as USER"
                );
            }
            for (single_report, c) in deploy.report.iter().zip(classified.iter()) {
                match c {
                    Some(phase) if is_system_phase(*phase) => continue,
                    Some(_) => {
                        user_transfers.extend(find_transfers_in_report(
                            single_report,
                            transfer_unforgeable,
                            deployer_addr.as_deref(),
                        ));
                    }
                    None => continue,
                }
            }
        } else {
            // Fallback path: every segment is UNSPECIFIED. This is the
            // expected shape during rollout, not a defect.
            tracing::debug!(
                target: "f1r3fly.node.web.enricher",
                deploy_sig = %deploy_sig,
                "report carries no phase marker; using positional precharge fallback"
            );

            for single_report in &deploy.report {
                user_transfers.extend(find_transfers_in_report(
                    single_report,
                    transfer_unforgeable,
                    deployer_addr.as_deref(),
                ));
            }
        }

        transfers_by_deploy.insert(deploy_sig, user_transfers);
    }

    transfers_by_deploy
}

/// Classify a raw proto `phase` value into a recognized `ReportPhase`,
/// or `None` when this build does not know the value. Mirrors the serde
/// conversion in `ReportPhaseSerde`, which maps an unknown value to
/// `Unspecified`, but keeps the distinction visible so the marked path
/// can log and drop unknown segments instead of scanning them as USER.
fn classify_phase(phase: i32) -> Option<ReportPhase> { ReportPhase::try_from(phase).ok() }

/// A recognized phase acts as a marker when it is not `UNSPECIFIED`. A
/// marker on any segment makes the marked path authoritative for the
/// whole report.
///
/// The match is exhaustive on purpose. A new variant in the proto enum
/// must break this build and force an explicit decision here and in
/// `is_system_phase`.
fn is_marker_phase(phase: ReportPhase) -> bool {
    match phase {
        ReportPhase::Precharge | ReportPhase::User | ReportPhase::Refund => true,
        ReportPhase::Unspecified => false,
    }
}

/// A segment is "system" when its phase is `PRECHARGE` or `REFUND`.
/// Used only for the per-segment drop decision on the marked path. The
/// `any_marked` check calls `is_marker_phase` instead.
///
/// `classify_phase` maps an unrecognized proto value to `None` before
/// this point, so an unknown value never reaches here. The marked path
/// logs and drops such a segment rather than scanning it as USER.
///
/// The match is exhaustive for the reason given on `is_marker_phase`.
fn is_system_phase(phase: ReportPhase) -> bool {
    match phase {
        ReportPhase::Precharge | ReportPhase::Refund => true,
        ReportPhase::Unspecified | ReportPhase::User => false,
    }
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
        ReportCommProto, ReportConsumeProto, ReportPhase, ReportProduceProto, ReportProto,
        SingleReport,
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
            ..Default::default()
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
        make_transfer_report_phase(
            transfer_unforgeable,
            from,
            to,
            amount,
            ReportPhase::Unspecified,
        )
    }

    /// Same as `make_transfer_report` but lets the test pin the phase
    /// marker on the batch. Marked-path tests use this so the batch is
    /// not routed to the positional fallback.
    fn make_transfer_report_phase(
        transfer_unforgeable: &Par,
        from: &str,
        to: &str,
        amount: i64,
        phase: ReportPhase,
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
            phase: phase as i32,
        }
    }

    fn make_empty_report(phase: ReportPhase) -> SingleReport {
        SingleReport {
            events: vec![],
            phase: phase as i32,
        }
    }

    fn make_deploy_info_with_charge(
        deploy_sig: &str,
        deployer_hex: &str,
        _phlo_limit: i64,
        _phlo_price: i64,
    ) -> DeployInfo {
        DeployInfo {
            sig: deploy_sig.to_string(),
            deployer: deployer_hex.to_string(),
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

        let empty_precharge = make_empty_report(ReportPhase::Unspecified);
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
            report: vec![
                make_empty_report(ReportPhase::Unspecified),
                make_empty_report(ReportPhase::Unspecified),
            ],
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
            phase: ReportPhase::Unspecified as i32,
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

    // === Marked path (explicit ReportPhase on batches) ===
    //
    // The marker is authoritative when present: PRECHARGE and REFUND
    // batches are dropped wholesale. USER batches are scanned for
    // deployer-sent transfers. No positional skip, no shape check, no
    // phlo_limit * phlo_price arithmetic.

    // 1. Marked report, normal order [PRECHARGE, USER, REFUND] -> only
    //    the user transfer. Precharge and refund dropped by marker.
    #[test]
    fn marked_report_normal_order_drops_precharge_and_refund() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let precharge = make_transfer_report_phase(
            &transfer_unforgeable,
            &deployer_addr,
            "pos_vault_addr",
            100,
            ReportPhase::Precharge,
        );
        let user = make_transfer_report_phase(
            &transfer_unforgeable,
            &deployer_addr,
            "receiver_addr",
            1000,
            ReportPhase::User,
        );
        let refund = make_transfer_report_phase(
            &transfer_unforgeable,
            "pos_vault_addr",
            &deployer_addr,
            50,
            ReportPhase::Refund,
        );
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_marked_normal",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![precharge, user, refund],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_marked_normal")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            1,
            "marked path: only the USER transfer is kept"
        );
        assert_eq!(transfers[0].to_addr, "receiver_addr");
        assert_eq!(transfers[0].amount, 1000);
        assert_eq!(transfers[0].from_addr, deployer_addr);
    }

    // 2. Marked report, batches deliberately out of order
    //    [USER, PRECHARGE, REFUND] -> the user transfer is returned,
    //    the precharge is not, and no positional warning is emitted.
    //    The marked path never runs the report[0] shape check, so the
    //    "does not match the expected precharge shape" warning that the
    //    fallback would fire (amount 700 != 100) cannot be reached.
    //    Position no longer matters.
    #[test]
    fn marked_report_out_of_order_keeps_user_without_positional_warning() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        // report[0] is the USER transfer (amount 700, not the precharge
        // amount 100). On the fallback path this would trigger the
        // shape-check warning. On the marked path it does not.
        let user = make_transfer_report_phase(
            &transfer_unforgeable,
            &deployer_addr,
            "receiver_addr",
            700,
            ReportPhase::User,
        );
        let precharge = make_transfer_report_phase(
            &transfer_unforgeable,
            &deployer_addr,
            "pos_vault_addr",
            100,
            ReportPhase::Precharge,
        );
        let refund = make_transfer_report_phase(
            &transfer_unforgeable,
            "pos_vault_addr",
            &deployer_addr,
            50,
            ReportPhase::Refund,
        );
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_marked_ooo",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![user, precharge, refund],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_marked_ooo")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            1,
            "marked path is NOT positional: only the USER batch is kept, \
             even when it is not report[0]"
        );
        assert_eq!(transfers[0].to_addr, "receiver_addr");
        assert_eq!(transfers[0].amount, 700);
        assert_eq!(transfers[0].from_addr, deployer_addr);
    }

    // 3. Marked report with no PRECHARGE batch -> user transfers
    //    returned, no warning. The marked path does not require a
    //    precharge batch to be present.
    #[test]
    fn marked_report_without_precharge_keeps_user_transfers() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let user = make_transfer_report_phase(
            &transfer_unforgeable,
            &deployer_addr,
            "receiver_addr",
            456,
            ReportPhase::User,
        );
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_marked_no_precharge",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![user],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_marked_no_precharge")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            1,
            "marked path: the single USER transfer is returned"
        );
        assert_eq!(transfers[0].to_addr, "receiver_addr");
        assert_eq!(transfers[0].amount, 456);
    }

    // 5. Mixed marked/unmarked report -> marked path used. UNSPECIFIED
    //    batches are treated as USER (data is never silently dropped).
    #[test]
    fn mixed_report_treats_unspecified_as_user() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let user_marked = make_transfer_report_phase(
            &transfer_unforgeable,
            &deployer_addr,
            "receiver_a",
            100,
            ReportPhase::User,
        );
        // An UNSPECIFIED batch in a mixed report is kept and scanned,
        // not dropped, so a genuine user transfer is not lost.
        let user_unmarked =
            make_transfer_report(&transfer_unforgeable, &deployer_addr, "receiver_b", 200);
        let precharge = make_transfer_report_phase(
            &transfer_unforgeable,
            &deployer_addr,
            "pos_vault_addr",
            50,
            ReportPhase::Precharge,
        );
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_mixed",
                &deployer_hex,
                50,
                1,
            ))
            .into(),
            report: vec![user_marked, user_unmarked, precharge],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_mixed")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            2,
            "mixed report: USER and UNSPECIFIED (as USER) kept, PRECHARGE dropped"
        );
        assert_eq!(transfers[0].to_addr, "receiver_a");
        assert_eq!(transfers[0].amount, 100);
        assert_eq!(transfers[1].to_addr, "receiver_b");
        assert_eq!(transfers[1].amount, 200);
    }

    // Marked report with only a PRECHARGE segment -> entry present,
    // empty vector, no warning.
    #[test]
    fn marked_report_with_only_precharge_yields_empty_entry() {
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let precharge = make_transfer_report_phase(
            &transfer_unforgeable,
            &deployer_addr,
            "pos_vault_addr",
            100,
            ReportPhase::Precharge,
        );
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_marked_precharge_only",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![precharge],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_marked_precharge_only")
            .expect("precharge-only marked deploy with non-empty report must receive a map entry");
        assert!(
            transfers.is_empty(),
            "PRECHARGE dropped by marker; no user transfers remain"
        );
    }

    // Regression test. An unrecognized phase discriminant must not be scanned as USER on
    // the marked path.
    #[test]
    fn unknown_phase_on_marked_report_is_ignored_not_scanned_as_user() {
        init_logger();
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let precharge = make_transfer_report_phase(
            &transfer_unforgeable,
            &deployer_addr,
            "pos_vault_addr",
            100,
            ReportPhase::Precharge,
        );
        // phase 99 is not a known ReportPhase discriminant.
        let unknown = SingleReport {
            events: vec![ReportProto {
                report: Some(report_proto::Report::Comm(make_comm_event(
                    &transfer_unforgeable,
                    &deployer_addr,
                    "receiver_addr",
                    500,
                ))),
            }],
            phase: 99,
        };
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_unknown_phase",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![precharge, unknown],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_unknown_phase")
            .expect("should have deploy entry");
        assert!(
            transfers.is_empty(),
            "unknown phase segment must not be scanned as USER; \
             precharge dropped, unknown ignored"
        );
    }

    // A report where every segment carries an unknown phase value does
    // not activate the marked path, so it falls to the positional
    // fallback. The fallback scans the segments (after the shape check
    // on report[0]); here report[0] fails the precharge shape check
    // (amount 500 != 100), so nothing is skipped and the transfer is
    // returned via the compatibility path.
    #[test]
    fn all_unknown_phase_report_falls_to_fallback_path() {
        init_logger();
        let transfer_unforgeable = make_transfer_unforgeable();
        let (deployer_hex, deployer_addr) = make_deployer();

        let unknown = SingleReport {
            events: vec![ReportProto {
                report: Some(report_proto::Report::Comm(make_comm_event(
                    &transfer_unforgeable,
                    &deployer_addr,
                    "receiver_addr",
                    500,
                ))),
            }],
            phase: 99,
        };
        let report = make_block_event(DeployInfoWithEventData {
            deploy_info: Some(make_deploy_info_with_charge(
                "deploy_all_unknown",
                &deployer_hex,
                100,
                1,
            ))
            .into(),
            report: vec![unknown],
        });

        let result = extract_transfers_from_report(&report, &transfer_unforgeable);

        let transfers = result
            .get("deploy_all_unknown")
            .expect("should have deploy entry");
        assert_eq!(
            transfers.len(),
            1,
            "unknown-only report uses the fallback path; the transfer is returned, not dropped"
        );
        assert_eq!(transfers[0].amount, 500);
    }
}
