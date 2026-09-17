use std::num::NonZeroUsize;

use rholang::rust::interpreter::accounting::authority::monetary_funding_signatures_with_host_work;
use rholang::rust::interpreter::accounting::monetary_allocation::{MonetaryCohort, MonetaryCursor};

use super::*;

fn inventory_for(
    signatures: &BTreeMap<SigKey, CostSignature>,
    joint: SigKey,
    joint_balance: u64,
) -> AuthorityPhysicalInventory<SigKey> {
    let mut inventory = AuthorityPhysicalInventory::default();
    for (key, signature) in signatures {
        insert_physical_balance(
            &mut inventory,
            *key,
            signature,
            if *key == joint { joint_balance } else { 8 },
        )
        .unwrap();
    }
    inventory
}

#[test]
fn monetary_fee_joint_and_individual_vaults_share_equal_rotation() {
    let deploy = v6_threshold_envelope(&[0, 2, 3]);
    let event = fee_authority_event(&deploy).unwrap();
    let signatures = monetary_funding_signatures_with_host_work(&event, &[], None).unwrap();
    let joint = accounting::funding_sig(&deploy).lane_hash();
    let inventory = inventory_for(&signatures, joint, 8);
    let cohort =
        MonetaryCohort::from_inventory(&signatures, &inventory, NonZeroUsize::new(65).unwrap())
            .unwrap();
    assert_eq!(cohort.payers().len(), 4);
    let mut available = inventory.balances.clone();
    let mut total = ResourceMultiset::default();
    let mut cursor = MonetaryCursor::INITIAL;
    let context = [11; 32];
    let count = NonZeroUsize::new(cohort.payers().len()).unwrap();
    for _ in 0..12 {
        let plan = cohort.plan(&available, 1, &context, cursor).unwrap();
        assert_eq!(plan.settlement().custody_debit.0.values().sum::<u64>(), 1);
        available = available
            .checked_sub(&plan.settlement().custody_debit)
            .unwrap();
        total = total.checked_add(&plan.settlement().custody_debit).unwrap();
        cursor = plan
            .cursor_transition()
            .checked_successor(&cohort.scope_id(&context), cursor, count)
            .unwrap();
    }
    for payer in cohort.payers() {
        assert_eq!(total.get(payer.custody()), 3);
    }
    assert_eq!(cursor.position(), 0);
    assert_eq!(cursor.revision(), 12);
}

#[test]
fn monetary_fee_joint_top_up_preserves_the_cohort_scope() {
    let deploy = v6_threshold_envelope(&[0, 2]);
    let event = fee_authority_event(&deploy).unwrap();
    let signatures = monetary_funding_signatures_with_host_work(&event, &[], None).unwrap();
    let joint = accounting::funding_sig(&deploy).lane_hash();
    let before = inventory_for(&signatures, joint, 0);
    let after = inventory_for(&signatures, joint, 8);
    let first =
        MonetaryCohort::from_inventory(&signatures, &before, NonZeroUsize::new(65).unwrap())
            .unwrap();
    let topped_up =
        MonetaryCohort::from_inventory(&signatures, &after, NonZeroUsize::new(65).unwrap())
            .unwrap();
    assert_eq!(first, topped_up);
    assert_eq!(first.scope_id(&[4; 32]), topped_up.scope_id(&[4; 32]));
    let plan = first.allocate(&before.balances, 2, 0).unwrap();
    assert_eq!(plan.settlement.logical_debit.get(&joint), 0);
    let plan = topped_up
        .allocate(&after.balances, 3, plan.next_cursor)
        .unwrap();
    assert_eq!(plan.settlement.logical_debit.get(&joint), 1);
}

#[test]
fn monetary_fee_authorized_subset_joint_vault_is_an_equal_payer() {
    let deploy = v6_threshold_envelope(&[0, 2, 3]);
    let selected = deploy.selected_signers_v61().unwrap();
    let subset = Sig::And(
        Box::new(Sig::Ground(accounting::principal_ground_v61(
            &selected[0].pk.bytes,
        ))),
        Box::new(Sig::Ground(accounting::principal_ground_v61(
            &selected[1].pk.bytes,
        ))),
    );
    let signature = sig_to_cost_signature(&subset).unwrap();
    let event = fee_authority_event(&deploy).unwrap();
    let signatures =
        monetary_funding_signatures_with_host_work(&event, &[signature], None).unwrap();
    assert!(signatures.contains_key(&subset.lane_hash()));
    let joint = accounting::funding_sig(&deploy).lane_hash();
    let inventory = inventory_for(&signatures, joint, 8);
    let cohort =
        MonetaryCohort::from_inventory(&signatures, &inventory, NonZeroUsize::new(5).unwrap())
            .unwrap();
    assert_eq!(cohort.payers().len(), 5);
    let plan = cohort.allocate(&inventory.balances, 5, 0).unwrap();
    assert_eq!(plan.settlement.custody_debit.0.len(), 5);
    assert!(plan
        .settlement
        .custody_debit
        .0
        .values()
        .all(|amount| *amount == 1));
    assert_eq!(plan.settlement.logical_debit.get(&subset.lane_hash()), 1);
}

#[test]
fn monetary_cohort_native_legacy_and_principal_aliases_share_one_position() {
    let deploy = v6_threshold_envelope(&[0, 2]);
    let signer = deploy.selected_signers_v61().unwrap()[0];
    let legacy = accounting::funding_sig_single(&signer.pk.bytes);
    let principal = Sig::Ground(accounting::principal_ground_v61(&signer.pk.bytes));
    assert_ne!(legacy.lane_hash(), principal.lane_hash());
    let signatures = BTreeMap::from([
        (legacy.lane_hash(), sig_to_cost_signature(&legacy).unwrap()),
        (
            principal.lane_hash(),
            sig_to_cost_signature(&principal).unwrap(),
        ),
    ]);
    let inventory = inventory_for(&signatures, [0; 32], 8);
    let cohort =
        MonetaryCohort::from_inventory(&signatures, &inventory, NonZeroUsize::new(1).unwrap())
            .unwrap();
    assert_eq!(cohort.payers().len(), 1);
    assert_eq!(cohort.payers()[0].logical_lanes().len(), 2);
    let plan = cohort.allocate(&inventory.balances, 1, 0).unwrap();
    assert_eq!(
        plan.settlement
            .custody_debit
            .get(cohort.payers()[0].custody()),
        1
    );
    assert_eq!(plan.settlement.logical_debit.0.values().sum::<u64>(), 1);
    assert_eq!(plan.next_cursor, 0);
}

#[test]
fn monetary_fee_certificate_rejects_missing_and_malformed_wire_evidence() {
    let deploy = v6_threshold_envelope(&[0, 2, 3]);
    let processed = processed_with_bound(&deploy, 0, 0);
    let wire = processed.authority_funding_certificate.unwrap();
    let decoded = authority_certificate_from_proto(&wire).unwrap();
    assert_eq!(authority_certificate_to_proto(&decoded), wire);
    for mutation in 0..12 {
        let mut forged = wire.clone();
        match mutation {
            0 => forged.fee_plan = None,
            1 => forged.protocol_version = 8,
            2 => forged.fee_allocation.clear(),
            3 => forged.fee_plan.as_mut().unwrap().policy_version += 1,
            4 => forged.fee_plan.as_mut().unwrap().policy_context = vec![0; 31].into(),
            5 => forged.fee_plan.as_mut().unwrap().scope = vec![0; 32].into(),
            6 => forged.fee_plan.as_mut().unwrap().payer_custodies.clear(),
            7 => forged.fee_plan.as_mut().unwrap().payer_custodies.reverse(),
            8 => forged.fee_plan.as_mut().unwrap().obligation = 2,
            9 => forged.fee_plan.as_mut().unwrap().expected_revision = -1,
            10 => forged.fee_plan.as_mut().unwrap().next_revision += 1,
            11 => forged.fee_plan.as_mut().unwrap().next_position = i64::MAX,
            _ => unreachable!(),
        }
        assert!(
            authority_certificate_from_proto(&forged).is_err(),
            "mutation {mutation}"
        );
    }
    let mut later = wire.clone();
    let fields = later.fee_plan.as_mut().unwrap();
    fields.expected_revision += 1;
    fields.next_revision += 1;
    let later = authority_certificate_from_proto(&later).unwrap();
    assert_ne!(decoded.certificate_id(), later.certificate_id());
    let transition = later.fee_plan.as_ref().unwrap().transition().unwrap();
    assert!(transition
        .checked_successor(
            transition.scope(),
            MonetaryCursor::INITIAL,
            later.fee_plan.as_ref().unwrap().payer_count()
        )
        .is_err());
}

#[tokio::test]
async fn monetary_fee_planning_does_not_advance_rejected_or_uncommitted_cursors() {
    let deploy = v6_threshold_envelope(&[0, 2, 3]);
    let event = fee_authority_event(&deploy).unwrap();
    let eligible = monetary_funding_signatures_with_host_work(&event, &[], None).unwrap();
    let inventory = inventory_for(&eligible, accounting::funding_sig(&deploy).lane_hash(), 8);
    let reader = MockSupplyReader::new();
    let mut accepted = BTreeMap::new();
    let mut balances = inventory.balances.clone();
    let mut totals = ResourceMultiset::default();
    for revision in 0..8 {
        let before = accepted.clone();
        assert!(monetary_fee::plan_monetary_fee(
            &deploy,
            &inventory,
            &ResourceMultiset::default(),
            &reader,
            &accepted,
            None
        )
        .await
        .unwrap()
        .is_none());
        assert_eq!(accepted, before);
        let (allocation, evidence) = monetary_fee::plan_monetary_fee(
            &deploy, &inventory, &balances, &reader, &accepted, None,
        )
        .await
        .unwrap()
        .unwrap();
        let repeated = monetary_fee::plan_monetary_fee(
            &deploy, &inventory, &balances, &reader, &accepted, None,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!((&allocation, &evidence), (&repeated.0, &repeated.1));
        assert_eq!(accepted, before);
        assert_eq!(evidence.fields().expected_revision, revision);
        assert_eq!(evidence.fields().next_revision, revision + 1);
        balances = balances.checked_sub(&allocation.custody_debit).unwrap();
        totals = totals.checked_add(&allocation.custody_debit).unwrap();
        accepted.insert(
            evidence.fields().scope,
            evidence.transition().unwrap().next(),
        );
    }
    assert_eq!(totals.0.len(), 4);
    assert!(totals.0.values().all(|amount| *amount == 2));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn monetary_fee_wire_rejects_every_incorrect_total(amount in any::<u64>()) {
        prop_assume!(amount != 1);
        let deploy = v6_threshold_envelope(&[0, 2, 3]);
        let processed = processed_with_bound(&deploy, 0, 0);
        let mut certificate = authority_certificate_from_proto(
            processed.authority_funding_certificate.as_ref().unwrap(),
        ).unwrap();
        certificate.fee_allocation = ResourceMultiset::singleton(
            accounting::funding_sig(&deploy).lane_hash(), amount,
        );
        let error = authority_certificate_from_proto(
            &authority_certificate_to_proto(&certificate),
        ).unwrap_err();
        prop_assert!(matches!(error, CasperError::InvalidCostSettlement(ref message)
            if message == "funding certificate monetary fee policy is invalid"));
    }

    #[test]
    fn monetary_fee_unsigned_members_never_enter_the_cohort(mask in 1_u8..16) {
        prop_assume!(mask.count_ones() >= 2);
        let selected: Vec<_> = (0..4).filter(|index| mask & (1 << index) != 0).collect();
        let deploy = v6_threshold_envelope(&selected);
        let presentations: Vec<_> = deploy.signers().iter().map(|signer| {
            sig_to_cost_signature(&Sig::Ground(accounting::principal_ground_v61(&signer.pk.bytes))).unwrap()
        }).collect();
        let event = fee_authority_event(&deploy).unwrap();
        let signatures = monetary_funding_signatures_with_host_work(&event, &presentations, None).unwrap();
        let joint = accounting::funding_sig(&deploy).lane_hash();
        let inventory = inventory_for(&signatures, joint, 8);
        let cohort = MonetaryCohort::from_inventory(&signatures, &inventory, NonZeroUsize::new(65).unwrap()).unwrap();
        prop_assert_eq!(cohort.payers().len(), selected.len() + 1);
        for signer in deploy.signers() {
            let lane = Sig::Ground(accounting::principal_ground_v61(&signer.pk.bytes)).lane_hash();
            prop_assert_eq!(signatures.contains_key(&lane), !signer.sig.is_empty());
        }
    }
}
