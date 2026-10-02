use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::PhloFundingIntentV1;
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};

use super::*;

fn limits() -> PhloWireLimits {
    PhloWireLimits {
        total_bytes: 2048,
        field_bytes: 2048,
    }
}

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10_000_000)))
}

fn quote_bytes(id: u8, use_policy: PhloQuoteUseV2) -> Vec<u8> {
    PhloQuoteEvidenceV2 {
        quote_id: [id; 32],
        network: b"testnet",
        shard: b"root",
        context_commitment: [2; 32],
        input_custody: b"input-purse",
        input_asset: b"INPUT",
        input_authority: b"input-owner",
        provider_custody: b"provider-purse",
        provider_authority: b"provider-owner",
        output_asset: b"REV",
        output_recipient: b"purse-b",
        fee_recipient: b"fee-purse",
        schedule_commitment: [7; 32],
        valid_from: Some(5),
        valid_until: Some(10),
        use_policy,
        max_output: 10,
        max_input_debit: 20,
        input_hold_cap: 30,
        provider_hold_cap: 15,
        rate_numerator: 3,
        rate_denominator: 2,
        fixed_input_fee: 2,
        input_scale: 1,
        output_scale: 1,
    }
    .encode(limits())
    .unwrap()
}

fn intent<'a>(bytes: &'a [u8]) -> PhloFundingIntentV2<'a> {
    let source_limits = PhloSourceLimits {
        wire: limits(),
        resource_permissions: 0,
        authority_nodes: 0,
    };
    PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 10,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![],
            },
            schedule_commitment: [7; 32],
            total_exposure: 50,
            sources: vec![
                PhloSourcePolicyV1::new(b"purse-a", 50, 50, true, vec![], source_limits).unwrap(),
                PhloSourcePolicyV1::new(b"purse-b", 50, 50, true, vec![], source_limits).unwrap(),
            ],
        },
        grant_uses: vec![],
        conversion: PhloConversionCompositionV2::AtomicQuote {
            quote_commitment: quote_evidence_commitment(bytes),
            quote_evidence: bytes,
            output_source_index: 1,
            input_custody: b"input-purse",
            input_asset: b"INPUT",
            provider_custody: b"provider-purse",
            output_asset: b"REV",
            max_input_debit: 20,
            max_output_debit: 10,
            input_hold_cap: 30,
            provider_hold_cap: 15,
        },
    }
}

fn context() -> ConversionContext<'static> {
    ConversionContext {
        root: [3; 32],
        network: b"testnet",
        shard: b"root",
        context_commitment: [2; 32],
        accepted_height: 7,
    }
}

fn snapshots(
    bytes: &[u8],
    input_available: u128,
    provider_available: u128,
) -> (RootedInputPermission, RootedQuoteEscrow) {
    let quote = PhloQuoteEvidenceV2::decode(bytes, limits()).unwrap();
    let commitment = quote_evidence_commitment(bytes);
    (
        RootedInputPermission {
            root: [3; 32],
            quote_commitment: commitment,
            authority: b"input-owner".to_vec(),
            source: RootedPhysicalCapacity {
                root: [3; 32],
                source_identity: [10; 32],
                custody: b"input-purse".to_vec(),
                asset: b"INPUT".to_vec(),
                available: input_available,
            },
        },
        RootedQuoteEscrow {
            root: [3; 32],
            quote_commitment: commitment,
            quote_id: quote.quote_id,
            provider_authority: b"provider-owner".to_vec(),
            source: RootedPhysicalCapacity {
                root: [3; 32],
                source_identity: [11; 32],
                custody: b"provider-purse".to_vec(),
                asset: b"REV".to_vec(),
                available: provider_available,
            },
            prior_output_used: 0,
            accepted_operations: BTreeSet::new(),
        },
    )
}

fn request<'a>(
    intent: &'a PhloFundingIntentV2<'a>,
    input: &'a RootedInputPermission,
    provider: &'a RootedQuoteEscrow,
    operation_id: u8,
    maximum_output: u128,
    realized_output: u128,
) -> AtomicQuoteUse<'a> {
    AtomicQuoteUse {
        intent,
        input,
        provider,
        operation_id: [operation_id; 32],
        maximum_output,
        realized_output,
    }
}

#[test]
fn atomic_quote_plan_keeps_original_asset_releases_and_exact_fee() {
    let bytes = quote_bytes(1, PhloQuoteUseV2::Once);
    let intent = intent(&bytes);
    let (input, provider) = snapshots(&bytes, 17, 10);
    let plans = plan_atomic_quote_uses(
        context(),
        &[request(&intent, &input, &provider, 8, 10, 3)],
        &budget(),
    )
    .unwrap();
    let [plan] = plans.as_slice() else {
        panic!("one plan")
    };
    assert_eq!(plan.input_hold, 17);
    assert_eq!(plan.provider_hold, 10);
    assert_eq!(plan.amount.total_input, 7);
    assert_eq!(plan.amount.conversion_fee, 2);
    assert_eq!(plan.input_release, 10);
    assert_eq!(plan.provider_release, 7);
    assert_eq!(plan.input_asset, b"INPUT");
    assert_eq!(plan.output_asset, b"REV");
    assert_eq!(plan.input_custody, b"input-purse");
    assert_eq!(plan.provider_custody, b"provider-purse");
    assert_eq!(plan.conversion_fee_recipient, b"fee-purse");
}

#[test]
fn atomic_quote_rejects_stale_root_wrong_authority_and_expiry() {
    let bytes = quote_bytes(1, PhloQuoteUseV2::Once);
    let intent = intent(&bytes);
    let (mut input, mut provider) = snapshots(&bytes, 17, 10);
    input.root = [4; 32];
    let use_ = request(&intent, &input, &provider, 8, 10, 3);
    assert_eq!(
        plan_atomic_quote_uses(context(), &[use_], &budget()),
        Err(ConversionPlanError::Root)
    );
    input.root = [3; 32];
    provider.provider_authority = b"other".to_vec();
    assert_eq!(
        plan_atomic_quote_uses(
            context(),
            &[request(&intent, &input, &provider, 8, 10, 3)],
            &budget(),
        ),
        Err(ConversionPlanError::Provenance)
    );
    provider.provider_authority = b"provider-owner".to_vec();
    let mut expired = context();
    expired.accepted_height = 11;
    assert_eq!(
        plan_atomic_quote_uses(
            expired,
            &[request(&intent, &input, &provider, 8, 10, 3)],
            &budget(),
        ),
        Err(ConversionPlanError::Context)
    );
}

#[test]
fn atomic_quote_rejects_duplicate_and_shared_physical_overclaim() {
    let bytes_a = quote_bytes(1, PhloQuoteUseV2::Once);
    let bytes_b = quote_bytes(2, PhloQuoteUseV2::Once);
    let intent_a = intent(&bytes_a);
    let intent_b = intent(&bytes_b);
    let (input_a, provider_a) = snapshots(&bytes_a, 30, 15);
    let (input_b, provider_b) = snapshots(&bytes_b, 30, 15);
    let a = request(&intent_a, &input_a, &provider_a, 8, 10, 3);
    let b = request(&intent_b, &input_b, &provider_b, 9, 10, 3);
    assert_eq!(
        plan_atomic_quote_uses(context(), &[a.clone(), b.clone()], &budget()),
        Err(ConversionPlanError::PhysicalCapacity)
    );
    assert_eq!(
        plan_atomic_quote_uses(context(), &[a.clone(), a], &budget()),
        Err(ConversionPlanError::Duplicate)
    );
    let mut accepted = provider_a.clone();
    accepted.accepted_operations.insert([8; 32]);
    assert_eq!(
        plan_atomic_quote_uses(
            context(),
            &[request(&intent_a, &input_a, &accepted, 8, 10, 3)],
            &budget(),
        ),
        Err(ConversionPlanError::Duplicate)
    );
}

#[test]
fn cumulative_quote_use_respects_prior_and_new_exposure() {
    let bytes = quote_bytes(1, PhloQuoteUseV2::Cumulative { output_ceiling: 15 });
    let intent = intent(&bytes);
    let (input, mut provider) = snapshots(&bytes, 30, 15);
    provider.prior_output_used = 5;
    let a = request(&intent, &input, &provider, 8, 5, 3);
    let b = request(&intent, &input, &provider, 9, 5, 2);
    assert!(plan_atomic_quote_uses(context(), &[a.clone(), b], &budget()).is_ok());
    assert_eq!(
        plan_atomic_quote_uses(
            context(),
            &[a, request(&intent, &input, &provider, 9, 6, 2)],
            &budget(),
        ),
        Err(ConversionPlanError::QuoteCapacity)
    );
}

#[test]
fn conversion_exhaustion_and_protocol_cap_reject_before_plan_publication() {
    let bytes = quote_bytes(1, PhloQuoteUseV2::Once);
    let intent = intent(&bytes);
    let (input, provider) = snapshots(&bytes, 17, 10);
    let request = request(&intent, &input, &provider, 8, 10, 3);
    let exhausted = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(matches!(
        plan_atomic_quote_uses(context(), &[request.clone()], &exhausted),
        Err(ConversionPlanError::HostBudget(_))
    ));
    assert!(exhausted.is_rejected());
    let oversized = vec![request; offered_conversion_plan_limits().uses + 1];
    assert_eq!(
        plan_atomic_quote_uses(context(), &oversized, &budget()),
        Err(ConversionPlanError::ProtocolLimit)
    );
}

#[test]
fn same_quote_cannot_reopen_once_use_or_change_physical_source_identity() {
    let bytes = quote_bytes(1, PhloQuoteUseV2::Once);
    let intent_a = intent(&bytes);
    let (input, provider) = snapshots(&bytes, 30, 20);
    assert_eq!(
        plan_atomic_quote_uses(
            context(),
            &[
                request(&intent_a, &input, &provider, 8, 3, 1),
                request(&intent_a, &input, &provider, 9, 3, 1),
            ],
            &budget(),
        ),
        Err(ConversionPlanError::QuoteCapacity)
    );
    let bytes_b = quote_bytes(2, PhloQuoteUseV2::Once);
    let intent_b = intent(&bytes_b);
    let (input_b, mut provider_b) = snapshots(&bytes_b, 30, 20);
    provider_b.source.source_identity = [99; 32];
    assert_eq!(
        plan_atomic_quote_uses(
            context(),
            &[
                request(&intent_a, &input, &provider, 8, 3, 1),
                request(&intent_b, &input_b, &provider_b, 9, 3, 1),
            ],
            &budget(),
        ),
        Err(ConversionPlanError::Provenance)
    );
}

#[test]
fn separate_prior_requires_rooted_receipt_and_current_output_backing() {
    let bytes = quote_bytes(1, PhloQuoteUseV2::Once);
    let mut intent = intent(&bytes);
    intent.conversion = PhloConversionCompositionV2::SeparatePrior {
        accepted_trade: [9; 32],
        output_source_index: 1,
        output_asset: b"REV",
    };
    let trade = RootedAcceptedTrade {
        root: [3; 32],
        trade_id: [9; 32],
        output_source_identity: [12; 32],
        output_custody: b"purse-b".to_vec(),
        output_asset: b"REV".to_vec(),
        unspent_output: 5,
    };
    let output = RootedPhysicalCapacity {
        root: [3; 32],
        source_identity: [12; 32],
        custody: b"purse-b".to_vec(),
        asset: b"REV".to_vec(),
        available: 3,
    };
    assert_eq!(
        check_separate_prior([3; 32], &intent, &trade, &output, 3, &budget()),
        Ok(())
    );
    assert_eq!(
        check_separate_prior([3; 32], &intent, &trade, &output, 4, &budget()),
        Err(ConversionPlanError::PhysicalCapacity)
    );
    let mut forged = trade.clone();
    forged.trade_id = [10; 32];
    assert_eq!(
        check_separate_prior([3; 32], &intent, &forged, &output, 3, &budget()),
        Err(ConversionPlanError::Provenance)
    );
    forged = trade;
    forged.root = [4; 32];
    assert_eq!(
        check_separate_prior([3; 32], &intent, &forged, &output, 3, &budget()),
        Err(ConversionPlanError::Root)
    );
}
