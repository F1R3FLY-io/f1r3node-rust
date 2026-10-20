//! G1-4b (DR-123): tests of the check before execution. One property test
//! covers each invariant: the hold sums, the placement, the caps, a
//! brute-force oracle, determinism, feasibility and the on-chain rule. Unit
//! tests cover the edges, the settlement limits (arbitration M3) and the
//! analysis of deep terms, which runs to completion after DR-131.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{CostSignature, EVar, Expr, New, Par, Receive, ReceiveBind, Send, Var};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::phlo_source::PhloSourceLimits;
use models::rust::phlo_wire::PhloWireLimits;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
// Added by dedup (task 11415, D2): the gate no longer imports `Message`, so
// these tests import it for `encode_to_vec`.
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::cost_accounting::CostAccounting;
use rholang::rust::interpreter::accounting::delta_sigma::estimate::dismantle_term;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};
use rholang::rust::interpreter::accounting::phlo_execution::PhloSourceConsent;
use rholang::rust::interpreter::accounting::SignatureChannel;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reduce::DebruijnInterpreter;
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;
use crate::rust::util::rholang::costacc::production_limits::offered_funded_v6_production_limits;

// ── Fixtures ───────────────────────────────────────────────────────────────

const LANES: usize = 2;

fn limits() -> NativeOfferedFamilyLimits { offered_funded_v6_production_limits().family }

/// The native schedule: compute, introduction, transfer and trace, each with
/// weight 1.
fn descriptor(price: u64) -> PhloScheduleV1<'static> {
    let identities: [&'static [u8]; 4] = [b"compute", b"introduction", b"transfer", b"trace"];
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(identities)
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: price,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

fn source_limits() -> PhloSourceLimits {
    PhloSourceLimits {
        wire: PhloWireLimits {
            total_bytes: 1_048_576,
            field_bytes: 524_288,
        },
        resource_permissions: 64,
        authority_nodes: 65_536,
    }
}

/// One lane: a ground signature of one unit, its authority, its lane key and
/// the encoded supply channel that keys its resources.
struct Lane {
    signature: CostSignature,
    authority: Sig,
    key: [u8; 32],
    location: Vec<u8>,
}

fn lane(tag: u8) -> Lane {
    let signature = CostSignature {
        value: Some(CostSignatureValue::Ground(vec![tag; 4])),
    };
    let authority = cost_signature_to_sig(&signature).expect("a ground signature is a Sig");
    Lane {
        key: authority.lane_hash(),
        location: SignatureChannel::from_sig(&authority).par.encode_to_vec(),
        signature,
        authority,
    }
}

#[derive(Clone, Debug)]
struct SourceCase {
    capacity: u64,
    hold_cap: u64,
    debit_cap: u64,
    fee: bool,
    /// One permission flag for each count of the case.
    permits: Vec<bool>,
}

/// One generated instance: the counts of the estimate, its completeness, the
/// signed limit (B plus `extra`), and the sources at R0.
#[derive(Clone, Debug)]
struct Case {
    price: u64,
    /// (lane, class, count), with distinct (lane, class) and count ≥ 1.
    counts: Vec<(usize, usize, u64)>,
    extra: u64,
    complete: bool,
    sources: Vec<SourceCase>,
    /// The custody tag of each source. The tags are distinct.
    custody: Vec<u8>,
    total_exposure: u128,
    cursors: (usize, usize),
}

impl Case {
    fn bound(&self) -> u64 { self.counts.iter().map(|(_, _, count)| count).sum() }

    fn limit(&self) -> u64 { self.bound() + self.extra }

    fn remainder(&self) -> u128 {
        match self.complete {
            true => 0,
            false => u128::from(self.extra) * u128::from(self.price),
        }
    }

    fn cap(&self, index: usize) -> u64 {
        let source = &self.sources[index];
        source.capacity.min(source.hold_cap).min(source.debit_cap)
    }
}

fn arb_case(max_price: u64) -> impl Strategy<Value = Case> {
    (
        1usize..=3,
        1..=max_price,
        any::<bool>(),
        0u64..=3,
        0u128..=40,
        prop::collection::btree_map((0..LANES, 0usize..4), 1u64..=3, 0..=3),
    )
        .prop_flat_map(|(n, price, complete, extra, total_exposure, entries)| {
            let m = entries.len();
            let sources = prop::collection::vec(
                (
                    0u64..=12,
                    0u64..=12,
                    0u64..=12,
                    prop::bool::weighted(0.75),
                    prop::collection::vec(prop::bool::weighted(0.75), m),
                ),
                n,
            );
            let custody = Just((1..=n as u8).collect::<Vec<_>>()).prop_shuffle();
            (
                Just(price),
                Just(complete),
                Just(extra),
                Just(total_exposure),
                Just(entries),
                sources,
                custody,
                0..n,
                0..n,
            )
        })
        .prop_map(
            |(price, complete, extra, total_exposure, entries, sources, custody, resource, fee)| {
                Case {
                    price,
                    counts: entries
                        .into_iter()
                        .map(|((lane, class), count)| (lane, class, count))
                        .collect(),
                    extra,
                    complete,
                    sources: sources
                        .into_iter()
                        .map(|(capacity, hold_cap, debit_cap, fee, permits)| SourceCase {
                            capacity,
                            hold_cap,
                            debit_cap,
                            fee,
                            permits,
                        })
                        .collect(),
                    custody,
                    total_exposure,
                    cursors: (resource, fee),
                }
            },
        )
}

/// The estimate of a case: one count for each (lane, class), B = Σ counts
/// (weights and units are 1), and the reason `OpenData` when incomplete.
fn estimate_of(case: &Case, lanes: &[Lane]) -> CostEstimate {
    CostEstimate {
        quantities: case
            .counts
            .iter()
            .map(|(lane, class, count)| ((lanes[*lane].key, *class), *count))
            .collect(),
        bound_phlo: case.bound(),
        signatures: case
            .counts
            .iter()
            .map(|(lane, _, _)| (lanes[*lane].key, lanes[*lane].signature.clone()))
            .collect(),
        completeness: match case.complete {
            true => Completeness::Complete,
            false => Completeness::Incomplete(IncompleteReason::OpenData),
        },
    }
}

/// Builds the estimate, the signed terms and the funding side of `case`, and
/// passes them to `body`.
fn with_instance<R>(
    case: &Case,
    onchain: &[bool],
    body: impl FnOnce(&CostEstimate, &GateTerms<'_>, &GateFunding<'_>) -> R,
) -> R {
    let lanes = (0..LANES)
        .map(|tag| lane(tag as u8 + 1))
        .collect::<Vec<_>>();
    let schedule = descriptor(case.price);
    let terms_bytes = schedule
        .encode(PhloGenesisPolicy::LIMITS)
        .expect("the schedule encodes");
    let binding =
        PhloScheduleBinding::new(&schedule, PhloGenesisPolicy::LIMITS).expect("the schedule binds");
    let estimate = estimate_of(case, &lanes);
    let controls = PhloControlsV1 {
        limit: case.limit(),
        price_ceiling: case.price,
        required_owner_ceilings: vec![case.price],
        permitted_schedules: vec![schedule.clone()],
    };
    let custodies = case
        .custody
        .iter()
        .map(|tag| [*tag; 32])
        .collect::<Vec<_>>();
    let permissions = case
        .sources
        .iter()
        .map(|source| {
            case.counts
                .iter()
                .zip(&source.permits)
                .filter(|(_, permitted)| **permitted)
                .map(|((lane, class, _), _)| PhloResource {
                    location: &lanes[*lane].location,
                    class: *class,
                    acquisition_terms: &terms_bytes,
                    authority: &lanes[*lane].authority,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let policies = case
        .sources
        .iter()
        .zip(&custodies)
        .zip(&permissions)
        .map(|((source, custody), resources)| {
            PhloSourceConsent {
                custody,
                hold_cap: source.hold_cap,
                debit_cap: source.debit_cap,
                fee_permitted: source.fee,
                resources,
            }
            .wire_policy(
                limits().measured.settlement.matching.execution,
                source_limits(),
            )
            .expect("the source policy encodes")
        })
        .collect::<Vec<_>>();
    let mut fill_order = (0..case.sources.len()).collect::<Vec<_>>();
    fill_order.sort_by_key(|position| custodies[*position]);
    let funding = GateFunding {
        sources: case
            .sources
            .iter()
            .zip(&custodies)
            .map(|(source, custody)| PhloFundingSource {
                custody,
                capacity: source.capacity,
                exposure_limit: source.hold_cap,
                debit_limit: source.debit_cap,
            })
            .collect(),
        signed: &policies,
        onchain: onchain.to_vec(),
        fill_order,
        total_exposure: case.total_exposure,
        cursors: case.cursors,
    };
    let terms = GateTerms {
        controls: &controls,
        minimum_price: 0,
        limit: case.limit(),
        price: case.price,
        schedule: &binding,
        selected_terms: &terms_bytes,
    };
    body(&estimate, &terms, &funding)
}

fn decide_case(case: &Case, onchain: &[bool]) -> Result<OfferedGate, OfferedGateRejection> {
    with_instance(case, onchain, |estimate, terms, funding| {
        decide(
            estimate,
            onchain.iter().any(|flag| *flag),
            terms,
            funding,
            limits(),
        )
    })
}

fn signed_only(case: &Case) -> Vec<bool> { vec![false; case.sources.len()] }

// ── The brute-force oracle ─────────────────────────────────────────────────

/// Whether some placement fits: every obligation, the fee included, splits
/// over its eligible sources, each source holds at most min(balance, hold
/// cap, debit cap), the remainder fits the room that is left, and the total
/// hold stays within the total exposure. It enumerates every split.
fn oracle_fits(case: &Case) -> bool {
    let caps = (0..case.sources.len())
        .map(|index| case.cap(index))
        .collect::<Vec<_>>();
    let mut columns = vec![(
        1u64,
        case.sources
            .iter()
            .map(|source| source.fee)
            .collect::<Vec<_>>(),
    )];
    for (index, (_, _, count)) in case.counts.iter().enumerate() {
        columns.push((
            count * case.price,
            case.sources
                .iter()
                .map(|source| source.permits[index])
                .collect(),
        ));
    }
    let mut loads = vec![0u64; caps.len()];
    let first = columns[0].0;
    search(
        &columns,
        (0, 0, first),
        &caps,
        &mut loads,
        case.remainder(),
        case.total_exposure,
    )
}

/// Places what is `left` of column `at.0`, from source `at.1` on.
fn search(
    columns: &[(u64, Vec<bool>)],
    at: (usize, usize, u64),
    caps: &[u64],
    loads: &mut [u64],
    remainder: u128,
    exposure: u128,
) -> bool {
    let (column, source, left) = at;
    if column == columns.len() {
        let room = caps
            .iter()
            .zip(loads.iter())
            .map(|(cap, load)| u128::from(cap - load))
            .sum::<u128>();
        let known = loads.iter().map(|load| u128::from(*load)).sum::<u128>();
        return room >= remainder && known + remainder <= exposure;
    }
    if source == caps.len() {
        let next = columns.get(column + 1).map_or(0, |next| next.0);
        return left == 0
            && search(
                columns,
                (column + 1, 0, next),
                caps,
                loads,
                remainder,
                exposure,
            );
    }
    let most = match columns[column].1[source] {
        true => left.min(caps[source] - loads[source]),
        false => 0,
    };
    for amount in (0..=most).rev() {
        loads[source] += amount;
        let found = search(
            columns,
            (column, source + 1, left - amount),
            caps,
            loads,
            remainder,
            exposure,
        );
        loads[source] -= amount;
        if found {
            return true;
        }
    }
    false
}

// ── Property tests, one per invariant ──────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Hold sums (design §6.1, §6.2): a complete estimate holds B·phloPrice + 1
    /// and an incomplete one holds phloLimit·phloPrice + 1.
    #[test]
    fn the_total_hold_is_the_cost_or_the_full_limit(case in arb_case(3)) {
        if let Ok(gate) = decide_case(&case, &signed_only(&case)) {
            let price = u128::from(case.price);
            let expected = match case.complete {
                true => u128::from(case.bound()) * price + 1,
                false => u128::from(case.limit()) * price + 1,
            };
            prop_assert_eq!(gate.total_hold(), expected);
            prop_assert_eq!(gate.estimate_phlo(), case.bound());
        }
    }

    /// Placement: the known demand and the fee sit on eligible sources, the
    /// obligations are covered exactly, and the remainder fills the sources
    /// in canonical custody order.
    #[test]
    fn known_demand_sits_on_eligible_sources_and_the_remainder_follows_custody_order(
        case in arb_case(3),
    ) {
        let onchain = signed_only(&case);
        let placed = with_instance(&case, &onchain, |estimate, terms, funding| {
            let bound = match case.complete {
                true => estimate.bound_phlo,
                false => terms.limit,
            };
            with_estimated_obligations(estimate, terms, bound, limits(), |obligations| {
                let eligible = eligibility(obligations, funding, limits())?;
                let placed = place_holds(
                    obligations,
                    &funding.sources,
                    &eligible,
                    &funding.onchain,
                    funding.total_exposure,
                    funding.cursors,
                    case.remainder(),
                    &funding.fill_order,
                    limits().policy,
                )?;
                Ok((eligible, obligations.amounts().to_vec(), placed, funding.fill_order.clone()))
            })
        });
        if let Ok((eligible, amounts, placed, fill_order)) = placed {
            for (row, (cells, hold)) in placed.assignment.iter().zip(&placed.known).enumerate() {
                for (column, amount) in cells.iter().enumerate() {
                    prop_assert!(*amount == 0 || eligible[row][column]);
                }
                prop_assert_eq!(cells.iter().sum::<u64>(), *hold);
            }
            for (column, amount) in amounts.iter().enumerate() {
                prop_assert_eq!(
                    placed.assignment.iter().map(|cells| cells[column]).sum::<u64>(),
                    *amount
                );
            }
            let rooms = fill_order
                .iter()
                .map(|position| u128::from(case.cap(*position)) - u128::from(placed.known[*position]))
                .collect::<Vec<_>>();
            let parts = fill_order
                .iter()
                .map(|position| placed.remainder[*position])
                .collect::<Vec<_>>();
            let partial = parts.iter().rposition(|part| *part > 0);
            if let Some(last) = partial {
                for index in 0..last {
                    prop_assert_eq!(parts[index], rooms[index]);
                }
                prop_assert!(parts[last + 1..].iter().all(|part| *part == 0));
            }
            prop_assert_eq!(parts.iter().sum::<u128>(), case.remainder());
        }
    }

    /// Caps: each hold is at most min(balance, hold cap, debit cap), and the
    /// total is at most the total exposure.
    #[test]
    fn each_hold_stays_within_the_caps_of_its_source(case in arb_case(3)) {
        if let Ok(gate) = decide_case(&case, &signed_only(&case)) {
            for (custody, hold) in gate.holds() {
                let position = case
                    .custody
                    .iter()
                    .position(|tag| [*tag; 32] == *custody)
                    .expect("every hold names a source");
                prop_assert!(*hold <= u128::from(case.cap(position)));
            }
            prop_assert!(gate.total_hold() <= case.total_exposure);
        }
    }

    /// Oracle: the check rejects for insufficient phlogiston exactly when an
    /// exhaustive search on the small instance finds no fit.
    #[test]
    fn the_check_rejects_exactly_when_no_placement_fits(case in arb_case(1)) {
        let decision = decide_case(&case, &signed_only(&case));
        prop_assert_eq!(decision.is_ok(), oracle_fits(&case), "{:?}", decision);
        if let Err(rejection) = decision {
            prop_assert!(
                matches!(rejection, OfferedGateRejection::InsufficientPhlogiston(_)),
                "{rejection:?}"
            );
        }
    }

    /// Determinism (DR-102, design §6.4): the decision depends only on its
    /// inputs.
    #[test]
    fn the_decision_depends_only_on_its_inputs(case in arb_case(3)) {
        let onchain = signed_only(&case);
        let first = decide_case(&case, &onchain);
        let second = decide_case(&case, &onchain);
        prop_assert_eq!(&first, &second);
    }

    /// Feasibility (design §12): when realized use is at most the estimate for
    /// every lane and class and balances do not change, the selection after
    /// execution succeeds with capacity equal to the balance at R0.
    #[test]
    fn realized_use_within_the_estimate_settles(
        case in arb_case(3),
        realized in prop::collection::vec(0u64..=3, 3),
    ) {
        let case = Case { complete: true, ..case };
        let onchain = signed_only(&case);
        if decide_case(&case, &onchain).is_ok() {
            let realized_case = Case {
                counts: case
                    .counts
                    .iter()
                    .zip(&realized)
                    .map(|((lane, class, count), used)| (*lane, *class, (*count).min(*used)))
                    .filter(|(_, _, used)| *used > 0)
                    .collect(),
                ..case.clone()
            };
            let kept = case
                .counts
                .iter()
                .zip(&realized)
                .map(|((_, _, count), used)| (*count).min(*used) > 0)
                .collect::<Vec<_>>();
            let realized_case = Case {
                sources: realized_case
                    .sources
                    .iter()
                    .map(|source| SourceCase {
                        permits: source
                            .permits
                            .iter()
                            .zip(&kept)
                            .filter(|(_, kept)| **kept)
                            .map(|(permitted, _)| *permitted)
                            .collect(),
                        ..source.clone()
                    })
                    .collect(),
                extra: case.limit() - realized_case.bound(),
                ..realized_case
            };
            let settled = with_instance(&realized_case, &onchain, |estimate, terms, funding| {
                with_estimated_obligations(estimate, terms, estimate.bound_phlo, limits(), |obligations| {
                    let eligible = eligibility(obligations, funding, limits())?;
                    place_holds(
                        obligations,
                        &funding.sources,
                        &eligible,
                        &funding.onchain,
                        funding.total_exposure,
                        funding.cursors,
                        0,
                        &funding.fill_order,
                        limits().policy,
                    )
                })
            });
            prop_assert!(settled.is_ok(), "{:?}", settled);
        }
    }

    /// The on-chain rule (map gap fill G-5): an offer with an on-chain payer
    /// is incomplete, binds the signed limit, and its on-chain purses hold at
    /// most the fee.
    #[test]
    fn an_on_chain_purse_holds_at_most_the_fee(
        case in arb_case(3),
        flags in prop::collection::vec(any::<bool>(), 3),
    ) {
        let onchain = flags[..case.sources.len()].to_vec();
        if let Ok(gate) = decide_case(&case, &onchain) {
            if onchain.iter().any(|flag| *flag) {
                prop_assert_eq!(gate.bound_source(), NativeBoundSource::SignedLimit);
                if case.complete {
                    prop_assert_eq!(
                        gate.kind(),
                        &OfferedEstimateKind::Incomplete(IncompleteReason::OnChainPayer)
                    );
                }
            }
            for (custody, hold) in gate.holds() {
                let position = case
                    .custody
                    .iter()
                    .position(|tag| [*tag; 32] == *custody)
                    .expect("every hold names a source");
                if onchain[position] {
                    prop_assert!(*hold <= 1, "an on-chain purse holds {hold}");
                }
            }
        }
    }

    /// Classification (design §6.1): only a complete estimate of an offer
    /// without on-chain payers binds `Certificate`, with the bound B.
    #[test]
    fn only_a_complete_estimate_binds_the_certificate(
        case in arb_case(3),
        flags in prop::collection::vec(any::<bool>(), 3),
    ) {
        let onchain = flags[..case.sources.len()].to_vec();
        if let Ok(gate) = decide_case(&case, &onchain) {
            let certified = case.complete && !onchain.iter().any(|flag| *flag);
            prop_assert_eq!(
                gate.bound_source() == NativeBoundSource::Certificate,
                certified
            );
            let bound = match certified {
                true => case.bound(),
                false => case.limit(),
            };
            prop_assert_eq!(gate.resource_bound(), bound);
        }
    }
}

/// The generated instances reach both outcomes of the check, and accepted
/// incomplete estimates place a remainder, so the property tests above are
/// not vacuous.
#[test]
fn generated_instances_reach_acceptance_and_rejection() {
    for (price, label) in [(3, "price up to 3"), (1, "oracle")] {
        let mut runner = TestRunner::deterministic();
        let strategy = arb_case(price);
        let (mut accepted, mut rejected, mut incomplete, mut remainder) = (0, 0, 0, 0);
        for _ in 0..256 {
            let case = strategy
                .new_tree(&mut runner)
                .expect("a generated case")
                .current();
            match decide_case(&case, &signed_only(&case)) {
                Ok(_) => {
                    accepted += 1;
                    if !case.complete {
                        incomplete += 1;
                        if case.remainder() > 0 {
                            remainder += 1;
                        }
                    }
                }
                Err(_) => rejected += 1,
            }
        }
        eprintln!(
            "{label}: accepted {accepted}, rejected {rejected}, incomplete {incomplete}, remainder {remainder}"
        );
        assert!(accepted >= 48, "{label}: {accepted} accepted");
        assert!(rejected >= 48, "{label}: {rejected} rejected");
        assert!(
            incomplete >= 16,
            "{label}: {incomplete} incomplete accepted"
        );
        assert!(remainder >= 8, "{label}: {remainder} remainders placed");
    }
}

// ── Edges ──────────────────────────────────────────────────────────────────

fn simple_case(complete: bool) -> Case {
    Case {
        price: 2,
        counts: vec![(0, 0, 3), (0, 1, 2), (1, 2, 1)],
        extra: 4,
        complete,
        sources: vec![
            SourceCase {
                capacity: 100,
                hold_cap: 100,
                debit_cap: 100,
                fee: true,
                permits: vec![true; 3],
            },
            SourceCase {
                capacity: 100,
                hold_cap: 100,
                debit_cap: 100,
                fee: false,
                permits: vec![true; 3],
            },
        ],
        custody: vec![2, 1],
        total_exposure: 1_000,
        cursors: (0, 0),
    }
}

/// An estimate above the signed limit is rejected before any other step
/// (design §6.3 item 1).
#[test]
fn an_estimate_above_the_signed_limit_is_rejected() {
    let case = simple_case(true);
    let decision = with_instance(&case, &signed_only(&case), |estimate, terms, funding| {
        let short = GateTerms {
            limit: estimate.bound_phlo - 1,
            ..*terms
        };
        decide(estimate, false, &short, funding, limits())
    });
    assert_eq!(
        decision,
        Err(OfferedGateRejection::EstimateAboveLimit {
            estimate: case.bound(),
            limit: case.bound() - 1
        })
    );
}

/// The worked case: a complete estimate holds B·price + 1 under
/// `Certificate`, and an incomplete one fills the remainder in custody order.
#[test]
fn the_holds_of_a_complete_and_an_incomplete_estimate() {
    let complete = decide_case(&simple_case(true), &[false, false]).expect("the holds fit");
    assert_eq!(complete.kind(), &OfferedEstimateKind::Complete);
    assert_eq!(complete.bound_source(), NativeBoundSource::Certificate);
    assert_eq!(complete.resource_bound(), 6);
    assert_eq!(complete.total_hold(), 6 * 2 + 1);
    let incomplete = decide_case(&simple_case(false), &[false, false]).expect("the holds fit");
    assert_eq!(incomplete.bound_source(), NativeBoundSource::SignedLimit);
    assert_eq!(incomplete.resource_bound(), 10);
    assert_eq!(incomplete.total_hold(), 10 * 2 + 1);
    assert_eq!(
        incomplete
            .holds()
            .iter()
            .map(|(custody, _)| custody[0])
            .collect::<Vec<_>>(),
        vec![1, 2],
        "the holds follow canonical custody order"
    );
}

/// The remainder of an incomplete estimate is placed completely or not at
/// all (P1:1990-1992): one atomic unit too little capacity rejects.
#[test]
fn a_remainder_that_does_not_fit_rejects_the_offer() {
    let mut case = simple_case(false);
    let hold = case.limit() * case.price + 1;
    for source in &mut case.sources {
        source.capacity = hold / 2;
    }
    case.sources[0].capacity = hold - case.sources[1].capacity;
    assert!(decide_case(&case, &[false, false]).is_ok());
    case.sources[0].capacity -= 1;
    assert_eq!(
        decide_case(&case, &[false, false]),
        Err(OfferedGateRejection::InsufficientPhlogiston(
            InsufficientCause::RemainderLeft { left: 1 }
        ))
    );
}

/// A total hold above the signed total exposure rejects the offer.
#[test]
fn a_total_hold_above_the_exposure_rejects_the_offer() {
    let mut case = simple_case(false);
    let hold = u128::from(case.limit() * case.price + 1);
    case.total_exposure = hold;
    assert!(decide_case(&case, &[false, false]).is_ok());
    case.total_exposure = hold - 1;
    assert!(matches!(
        decide_case(&case, &[false, false]),
        Err(OfferedGateRejection::InsufficientPhlogiston(_))
    ));
}

/// The rejection text of P1:2210.
#[test]
fn holds_that_do_not_fit_are_insufficient_phlogiston() {
    let mut case = simple_case(true);
    for source in &mut case.sources {
        source.fee = false;
    }
    let rejection = decide_case(&case, &[false, false]).expect_err("no source pays the fee");
    assert_eq!(
        rejection,
        OfferedGateRejection::InsufficientPhlogiston(InsufficientCause::NoPlacement)
    );
    assert!(rejection.to_string().starts_with("insufficient phlogiston"));
}

/// Arbitration M3: a witness above the settlement limits rejects the offer
/// before execution. Settlement would check the same limits after all the
/// work.
#[test]
fn a_witness_above_the_settlement_limits_rejects_before_execution() {
    let case = simple_case(true);
    let mut narrow = limits();
    narrow
        .measured
        .settlement
        .matching
        .execution
        .resource_entries = 2 * case.counts.len() - 1;
    let decision = with_instance(&case, &signed_only(&case), |estimate, terms, funding| {
        decide(estimate, false, terms, funding, narrow)
    });
    assert!(
        matches!(decision, Err(OfferedGateRejection::SettlementLimits(_))),
        "{decision:?}"
    );
    narrow
        .measured
        .settlement
        .matching
        .execution
        .resource_entries = 2 * case.counts.len();
    let decision = with_instance(&case, &signed_only(&case), |estimate, terms, funding| {
        decide(estimate, false, terms, funding, narrow)
    });
    assert!(decision.is_ok(), "{decision:?}");
}

// ── The analysis inside the check ──────────────────────────────────────────

fn evaluator() -> Arc<DebruijnInterpreter> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a test runtime")
        .block_on(async {
            let mut stores = InMemoryStoreManager::new();
            let store = stores.r_space_stores().await.expect("in-memory stores");
            let space = RSpace::create(store, Arc::new(Box::new(Matcher))).expect("a test space");
            let space: RhoISpace = Arc::new(Box::new(space));
            DebruijnInterpreter::new(
                space,
                Arc::new(HashMap::new()),
                Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                Arc::new(HashMap::new()),
                CostAccounting::empty_cost(),
            )
        })
}

fn funding() -> Sig { Sig::Ground(vec![9, 9, 9, 9]) }

fn bound_level(level: i32) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(level)),
            }),
        })),
    }])
}

/// `depth` levels of `new x in { x!(1) | for (_ <- x) { … } }`, built from the
/// inside out without recursion.
fn deep_chain(depth: usize) -> Par {
    let mut par = Par::default();
    for _ in 0..depth {
        let mut level = Par::default();
        level.sends.push(Send {
            chan: Some(bound_level(0)),
            data: vec![Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(1)),
            }])],
            persistent: false,
            locally_free: vec![1],
            connective_used: false,
        });
        level.receives.push(Receive {
            binds: vec![ReceiveBind {
                patterns: Vec::new(),
                source: Some(bound_level(0)),
                remainder: None,
                free_count: 0,
                cost_signature: None,
            }],
            body: Some(par),
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: vec![1],
            connective_used: false,
            condition: None,
        });
        par = Par::default().with_news(vec![New {
            bind_count: 1,
            p: Some(level),
            uri: Vec::new(),
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        }]);
    }
    par
}

/// The G1-4a finding, after DR-131: the analysis of a 1,000-level chain runs
/// to completion on its heap work stacks, and the check gets its estimate.
#[test]
fn a_deep_chain_is_analyzed_to_completion() {
    let term = deep_chain(1_000);
    let schedule = descriptor(1);
    let binding =
        PhloScheduleBinding::new(&schedule, PhloGenesisPolicy::LIMITS).expect("the schedule binds");
    let reducer = evaluator();
    let estimate = estimate_resolved(&term, &funding(), &binding, &reducer);
    dismantle_term(term);
    assert!(estimate.is_ok(), "{estimate:?}");
}

/// A term that does not parse is invalid.
#[test]
fn a_term_that_does_not_parse_is_invalid() {
    let schedule = descriptor(1);
    let binding =
        PhloScheduleBinding::new(&schedule, PhloGenesisPolicy::LIMITS).expect("the schedule binds");
    let reducer = evaluator();
    let rand =
        || crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_bytes(b"gate");
    assert!(matches!(
        estimate_term(
            "new x in {",
            HashMap::new(),
            rand(),
            &funding(),
            &binding,
            &reducer,
        ),
        Err(OfferedGateRejection::Invalid(_))
    ));
}

/// A real estimate passes the witness check under the bound B: the units of
/// the estimate and of the meter agree, and the hold is B·price + 1.
#[test]
fn a_real_complete_estimate_holds_its_cost_plus_the_fee() {
    let price = 3;
    let schedule = descriptor(price);
    let terms_bytes = schedule
        .encode(PhloGenesisPolicy::LIMITS)
        .expect("the schedule encodes");
    let binding =
        PhloScheduleBinding::new(&schedule, PhloGenesisPolicy::LIMITS).expect("the schedule binds");
    let reducer = evaluator();
    let estimate = estimate_term(
        "new x in { x!(0) | for (@n <- x) { Nil } }",
        HashMap::new(),
        crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_bytes(b"gate"),
        &funding(),
        &binding,
        &reducer,
    )
    .expect("the estimate runs");
    assert_eq!(estimate.completeness, Completeness::Complete);
    assert!(estimate.bound_phlo > 0);
    let authorities = estimate
        .signatures
        .iter()
        .map(|(lane, signature)| {
            let authority = cost_signature_to_sig(signature).expect("a static lane is a Sig");
            let location = SignatureChannel::from_sig(&authority).par.encode_to_vec();
            (*lane, (authority, location))
        })
        .collect::<BTreeMap<_, _>>();
    let permissions = estimate
        .quantities
        .keys()
        .map(|(lane, class)| {
            let (authority, location) = &authorities[lane];
            PhloResource {
                location,
                class: *class,
                acquisition_terms: &terms_bytes,
                authority,
            }
        })
        .collect::<Vec<_>>();
    let custody = [7u8; 32];
    let policies = vec![PhloSourceConsent {
        custody: &custody,
        hold_cap: 1_000_000,
        debit_cap: 1_000_000,
        fee_permitted: true,
        resources: &permissions,
    }
    .wire_policy(
        limits().measured.settlement.matching.execution,
        source_limits(),
    )
    .expect("the source policy encodes")];
    let limit = estimate.bound_phlo + 10;
    let controls = PhloControlsV1 {
        limit,
        price_ceiling: price,
        required_owner_ceilings: vec![price],
        permitted_schedules: vec![schedule.clone()],
    };
    let funding_side = GateFunding {
        sources: vec![PhloFundingSource {
            custody: &custody,
            capacity: 1_000_000,
            exposure_limit: 1_000_000,
            debit_limit: 1_000_000,
        }],
        signed: &policies,
        onchain: vec![false],
        fill_order: vec![0],
        total_exposure: 1_000_000,
        cursors: (0, 0),
    };
    let gate = decide(
        &estimate,
        false,
        &GateTerms {
            controls: &controls,
            minimum_price: 0,
            limit,
            price,
            schedule: &binding,
            selected_terms: &terms_bytes,
        },
        &funding_side,
        limits(),
    )
    .expect("the holds fit");
    assert_eq!(gate.bound_source(), NativeBoundSource::Certificate);
    assert_eq!(gate.resource_bound(), estimate.bound_phlo);
    assert_eq!(
        gate.total_hold(),
        u128::from(estimate.bound_phlo) * u128::from(price) + 1
    );
}

/// `levels` nested `new` terms inside a quoted signature, built from the
/// inside out without recursion.
fn quoted_signature(levels: usize) -> CostSignature {
    let mut inner = Par::default();
    for _ in 0..levels {
        inner = Par::default().with_news(vec![New {
            bind_count: 1,
            p: Some(inner),
            uri: Vec::new(),
            injections: BTreeMap::new(),
            locally_free: Vec::new(),
        }]);
    }
    CostSignature {
        value: Some(CostSignatureValue::Quote(inner)),
    }
}

/// The check converts lane signatures with the conversion of `born_resource`.
/// A shallow quoted signature and a signature nested 1,000 levels deep both
/// convert to the location of their supply channel. The depth stop of the
/// metered sorter left with DR-131.
#[test]
fn a_deep_lane_signature_converts_without_a_depth_stop() {
    let key = [42u8; 32];
    let estimate = |signature: CostSignature| CostEstimate {
        quantities: BTreeMap::from([((key, 0), 1)]),
        bound_phlo: 1,
        signatures: BTreeMap::from([(key, signature)]),
        completeness: Completeness::Complete,
    };
    for levels in [8, 1_000] {
        let mut lane = estimate(quoted_signature(levels));
        let converted = EstimatedResources::new(&lane).map(|resources| {
            resources
                .lanes
                .get(&key)
                .map(|(authority, location)| (authority.clone(), location.clone()))
        });
        let authority =
            cost_signature_to_sig(&lane.signatures[&key]).expect("a lane signature is a Sig");
        assert_eq!(
            converted,
            Ok(Some((
                authority.clone(),
                SignatureChannel::from_sig(&authority).par.encode_to_vec()
            ))),
            "the conversion gives the location of the supply channel at {levels} levels"
        );
        // Changed by the merge of feature/cost-accounted-rho (finding R3-33):
        // F1R3Lang gives `CostSignature` an iterative `Drop`, so a pattern cannot
        // move `value` out of it (E0509). The value is taken first. The old
        // pattern was:
        // if let Some(CostSignature {
        //     value: Some(CostSignatureValue::Quote(par)),
        // }) = lane.signatures.remove(&key)
        // {
        //     dismantle_term(par);
        // }
        if let Some(mut signature) = lane.signatures.remove(&key) {
            if let Some(CostSignatureValue::Quote(par)) = signature.value.take() {
                dismantle_term(par);
            }
        }
    }
}

// ── The client lanes (G1-6, DR-124) ────────────────────────────────────────

/// The decision of `case` after the restriction of its estimate to `client`.
fn decide_restricted(
    case: &Case,
    client: &BTreeSet<[u8; 32]>,
) -> Result<OfferedGate, OfferedGateRejection> {
    let onchain = signed_only(case);
    with_instance(case, &onchain, |estimate, terms, funding| {
        let restricted = estimate
            .clone()
            .retain_client_lanes(client, terms.schedule.descriptor())
            .map_err(analysis_rejection)?;
        decide(&restricted, false, terms, funding, limits())
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// When every lane is a client lane, the restriction changes no decision.
    #[test]
    fn client_lanes_keep_every_decision(case in arb_case(3)) {
        let lanes = (0..LANES)
            .map(|tag| lane(tag as u8 + 1).key)
            .collect::<BTreeSet<_>>();
        prop_assert_eq!(
            decide_restricted(&case, &lanes),
            decide_case(&case, &signed_only(&case))
        );
    }

    /// A lane outside the client lanes leaves the holds. B counts the client
    /// lane only. With a counted foreign lane, an accepted offer holds its
    /// signed limit under `SignedLimit`, and a complete estimate becomes
    /// incomplete with the reason `ForeignLane`.
    #[test]
    fn a_foreign_lane_leaves_the_holds(case in arb_case(3)) {
        let client = BTreeSet::from([lane(1).key]);
        let foreign = case.counts.iter().any(|(lane, _, _)| *lane == 1);
        let client_bound = case
            .counts
            .iter()
            .filter(|(lane, _, _)| *lane == 0)
            .map(|(_, _, count)| *count)
            .sum::<u64>();
        if let Ok(gate) = decide_restricted(&case, &client) {
            prop_assert_eq!(gate.estimate_phlo(), client_bound);
            if foreign {
                prop_assert_eq!(gate.bound_source(), NativeBoundSource::SignedLimit);
                prop_assert_eq!(
                    gate.total_hold(),
                    u128::from(case.limit()) * u128::from(case.price) + 1
                );
                if case.complete {
                    prop_assert_eq!(
                        gate.kind(),
                        &OfferedEstimateKind::Incomplete(IncompleteReason::ForeignLane)
                    );
                }
            }
        }
    }
}

/// The client lanes are the deploy lane and the lanes of the located sources.
#[test]
fn the_client_lanes_are_the_deploy_lane_and_the_located_sources() {
    let lanes = client_lane_set([1; 32], vec![[2; 32], [1; 32], [3; 32]]);
    assert_eq!(lanes, BTreeSet::from([[1; 32], [2; 32], [3; 32]]));
}

/// The loophole control of DR-124. A source that permits only some classes of
/// a client lane does not make that lane foreign: the class without an
/// eligible source still rejects the offer before execution. Only a lane that
/// no source is located at or permits leaves the holds (G1-6 fix).
#[test]
fn restricting_class_permissions_does_not_exclude_a_client_lane() {
    let case = Case {
        price: 2,
        counts: vec![(0, 0, 1), (1, 1, 1)],
        extra: 3,
        complete: true,
        sources: vec![SourceCase {
            capacity: 40,
            hold_cap: 40,
            debit_cap: 40,
            fee: true,
            permits: vec![true, false],
        }],
        custody: vec![1],
        total_exposure: 40,
        cursors: (0, 0),
    };
    assert_eq!(
        decide_restricted(&case, &BTreeSet::from([lane(1).key, lane(2).key])),
        Err(OfferedGateRejection::InsufficientPhlogiston(
            InsufficientCause::NoPlacement
        ))
    );
    let gate = decide_restricted(&case, &BTreeSet::from([lane(1).key]))
        .expect("the foreign lane leaves the holds");
    assert_eq!(
        gate.kind(),
        &OfferedEstimateKind::Incomplete(IncompleteReason::ForeignLane)
    );
    assert_eq!(gate.estimate_phlo(), 1);
    assert_eq!(gate.bound_source(), NativeBoundSource::SignedLimit);
    assert_eq!(gate.total_hold(), u128::from(case.limit()) * 2 + 1);
}

// ── The lanes that the sources permit (G1-6 fix, DR-124) ───────────────────

/// The lanes that the signed permissions of `case` name, for any class.
fn permitted_of(case: &Case) -> BTreeSet<[u8; 32]> {
    case.sources
        .iter()
        .flat_map(|source| {
            case.counts
                .iter()
                .zip(&source.permits)
                .filter(|(_, permitted)| **permitted)
                .map(|((lane_index, _, _), _)| lane(*lane_index as u8 + 1).key)
        })
        .collect()
}

/// The client lanes of `case`, as the check computes them. Lane 0 stands
/// for the deploy lane, and the other client lanes come from the signed
/// permissions of the sources (`permitted_lanes`).
fn client_of(case: &Case) -> BTreeSet<[u8; 32]> {
    with_instance(case, &signed_only(case), |_, _, funding| {
        let permitted = permitted_lanes(funding.signed).expect("the permitted lanes");
        client_lane_set(lane(1).key, permitted)
    })
}

/// The parts of a decision that a narrowing of permissions must keep.
fn summary(gate: &OfferedGate) -> (OfferedEstimateKind, u64, u64, NativeBoundSource, u128) {
    (
        gate.kind().clone(),
        gate.estimate_phlo(),
        gate.resource_bound(),
        gate.bound_source(),
        gate.total_hold(),
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The permitted lanes are the lanes of the signed permissions, with one
    /// lane for each permission.
    #[test]
    fn the_permitted_lanes_are_the_lanes_of_the_permissions(case in arb_case(3)) {
        let permissions = case
            .sources
            .iter()
            .map(|source| source.permits.iter().filter(|permitted| **permitted).count())
            .sum::<usize>();
        let expected = permitted_of(&case);
        with_instance(&case, &signed_only(&case), |_, _, funding| {
            let lanes = permitted_lanes(funding.signed).expect("the permitted lanes");
            prop_assert_eq!(lanes.len(), permissions);
            prop_assert_eq!(lanes.iter().copied().collect::<BTreeSet<_>>(), expected.clone());
            Ok(())
        })?;
    }

    /// Fewer permitted classes never lower the decision. Each lane keeps one
    /// permission, so the client lanes stay the same. The narrowed offer is
    /// then rejected, or it reaches the same kind, estimate, bound and total
    /// hold as the offer before the narrowing (the loophole control of DR-124).
    #[test]
    fn fewer_permitted_classes_reject_or_keep_the_decision(
        case in arb_case(3),
        mask in prop::collection::vec(any::<bool>(), 0..16),
    ) {
        let mut narrowed = case.clone();
        let mut flag = 0;
        for source in &mut narrowed.sources {
            for permitted in &mut source.permits {
                if mask.get(flag).copied().unwrap_or(false) {
                    *permitted = false;
                }
                flag += 1;
            }
        }
        for (index, (lane_index, _, _)) in case.counts.iter().enumerate() {
            let lane_permits = |sources: &[SourceCase]| {
                sources.iter().any(|source| {
                    case.counts
                        .iter()
                        .zip(&source.permits)
                        .any(|((other, _, _), permitted)| other == lane_index && *permitted)
                })
            };
            if lane_permits(&case.sources) && !lane_permits(&narrowed.sources) {
                if let Some(source) = case
                    .sources
                    .iter()
                    .position(|source| source.permits[index])
                {
                    narrowed.sources[source].permits[index] = true;
                }
            }
        }
        let client = client_of(&case);
        prop_assert_eq!(&client_of(&narrowed), &client);
        match (
            decide_restricted(&narrowed, &client),
            decide_restricted(&case, &client),
        ) {
            (Err(_), _) => {}
            (Ok(narrow), Ok(wide)) => prop_assert_eq!(summary(&narrow), summary(&wide)),
            (Ok(narrow), Err(rejection)) => prop_assert!(
                false,
                "a narrowing accepted a rejected offer: {:?} after {:?}",
                narrow,
                rejection
            ),
        }
    }

    /// When every counted lane is the deploy lane or a permitted lane, the
    /// restriction to the client lanes changes no decision. A permitted lane
    /// keeps its exact holds.
    #[test]
    fn permitted_lanes_keep_every_decision(case in arb_case(3)) {
        let mut sponsored = case.clone();
        for (index, (lane_index, _, _)) in case.counts.iter().enumerate() {
            if *lane_index != 0 && !permitted_of(&sponsored).contains(&lane(*lane_index as u8 + 1).key) {
                sponsored.sources[0].permits[index] = true;
            }
        }
        let client = client_of(&sponsored);
        prop_assert!(sponsored
            .counts
            .iter()
            .all(|(lane_index, _, _)| client.contains(&lane(*lane_index as u8 + 1).key)));
        prop_assert_eq!(
            decide_restricted(&sponsored, &client),
            decide_case(&sponsored, &signed_only(&sponsored))
        );
    }
}

/// The case of the 64-source test. Two sources each permit one class of a
/// lane at whose purse no source is located. The lane is a client lane, the
/// estimate stays complete, and each class lands on the source that permits
/// it. The fee lands on the fee source.
#[test]
fn a_lane_that_sources_permit_is_a_client_lane_with_exact_holds() {
    let source = |fee: bool, permits: Vec<bool>| SourceCase {
        capacity: 40,
        hold_cap: 40,
        debit_cap: 40,
        fee,
        permits,
    };
    let case = Case {
        price: 2,
        counts: vec![(1, 0, 2), (1, 1, 3)],
        extra: 3,
        complete: true,
        sources: vec![
            source(true, vec![true, false]),
            source(false, vec![false, true]),
        ],
        custody: vec![1, 2],
        total_exposure: 40,
        cursors: (0, 0),
    };
    let client = client_of(&case);
    assert_eq!(client, BTreeSet::from([lane(1).key, lane(2).key]));
    let gate = decide_restricted(&case, &client).expect("the permitted lane is held");
    assert_eq!(gate.kind(), &OfferedEstimateKind::Complete);
    assert_eq!(gate.bound_source(), NativeBoundSource::Certificate);
    assert_eq!(gate.estimate_phlo(), 5);
    assert_eq!(gate.resource_bound(), 5);
    assert_eq!(
        gate.holds().to_vec(),
        vec![([1u8; 32], 2u128 * 2 + 1), ([2u8; 32], 3u128 * 2)],
        "each class lands on the source that permits it, and the fee on the fee source"
    );
}

/// The loophole control for a permitted lane. A source permits one of the two
/// classes with demand of a lane at whose purse no source is located. The
/// lane stays a client lane, so the class without a permission rejects the
/// offer before execution. Only a lane with no permission at all is foreign,
/// and the offer then holds its signed limit.
#[test]
fn a_permitted_lane_without_a_permission_for_a_class_with_demand_rejects() {
    let case = Case {
        price: 2,
        counts: vec![(1, 0, 2), (1, 1, 3)],
        extra: 3,
        complete: true,
        sources: vec![SourceCase {
            capacity: 40,
            hold_cap: 40,
            debit_cap: 40,
            fee: true,
            permits: vec![true, false],
        }],
        custody: vec![1],
        total_exposure: 40,
        cursors: (0, 0),
    };
    let client = client_of(&case);
    assert!(client.contains(&lane(2).key));
    assert_eq!(
        decide_restricted(&case, &client),
        Err(OfferedGateRejection::InsufficientPhlogiston(
            InsufficientCause::NoPlacement
        ))
    );
    let mut unpermitted = case.clone();
    unpermitted.sources[0].permits = vec![false, false];
    let client = client_of(&unpermitted);
    assert_eq!(client, BTreeSet::from([lane(1).key]));
    let gate = decide_restricted(&unpermitted, &client).expect("the foreign lane leaves the holds");
    assert_eq!(
        gate.kind(),
        &OfferedEstimateKind::Incomplete(IncompleteReason::ForeignLane)
    );
    assert_eq!(gate.bound_source(), NativeBoundSource::SignedLimit);
    assert_eq!(gate.total_hold(), u128::from(unpermitted.limit()) * 2 + 1);
}

// ── The held-capacity guard (G1-7, DR-65 item 5) ──────────────────────────

/// A purse key from a small tag.
fn purse(tag: u8) -> [u8; 32] { [tag; 32] }

/// Maps over four purses, with small amounts and amounts near the top of
/// `u128`, so that both the inequality and the overflow of the sum occur.
fn arb_purse_map() -> impl Strategy<Value = BTreeMap<[u8; 32], u128>> {
    prop::collection::btree_map(
        (0u8..4).prop_map(purse),
        prop_oneof![0u128..=20, (u128::MAX - 20)..=u128::MAX],
        0..=4,
    )
}

/// The pointwise statement of the guard (design §8.3): the purses of the
/// debits and the later holds where balance_after(p) < debit(p) + later(p),
/// in ascending order. A sum that overflows fails its purse.
fn pointwise_failures(
    balances: &BTreeMap<[u8; 32], u128>,
    debits: &BTreeMap<[u8; 32], u128>,
    later: &BTreeMap<[u8; 32], u128>,
) -> Vec<[u8; 32]> {
    debits
        .keys()
        .chain(later.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|purse| {
            let required = debits
                .get(purse)
                .copied()
                .unwrap_or(0)
                .checked_add(later.get(purse).copied().unwrap_or(0));
            let balance = balances.get(purse).copied().unwrap_or(0);
            !matches!(required, Some(required) if required <= balance)
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The guard equals the pointwise inequality over every purse of the
    /// debits and the later holds, and it lists exactly the failing purses.
    #[test]
    fn the_guard_equals_the_pointwise_inequality(
        balances in arb_purse_map(),
        debits in arb_purse_map(),
        later in arb_purse_map(),
    ) {
        let failing = pointwise_failures(&balances, &debits, &later);
        let expected = match failing.is_empty() {
            true => Ok(()),
            false => Err(failing),
        };
        prop_assert_eq!(held_capacity_guard(&balances, &debits, &later), expected);
    }

    /// With no later holds, as with one offer per block, the guard is the
    /// condition under which SystemVault reserves each debit:
    /// balance_after(p) ≥ debit(p) for every debited purse.
    #[test]
    fn with_no_later_holds_the_guard_is_the_vault_condition(
        balances in arb_purse_map(),
        debits in arb_purse_map(),
    ) {
        let vault = debits
            .iter()
            .all(|(purse, debit)| *debit <= balances.get(purse).copied().unwrap_or(0));
        prop_assert_eq!(
            held_capacity_guard(&balances, &debits, &BTreeMap::new()).is_ok(),
            vault
        );
    }

    /// The rollback charge fits the restored balance when realized use is at
    /// most the estimate. With one offer per block the restored balance is
    /// the balance at R0. The check placed the hold of the estimate there,
    /// and the realized obligations, which a user failure charges in full,
    /// are placed within the same capacities. So the guard over the restored
    /// balances holds for the rollback debits.
    #[test]
    fn the_rollback_charge_fits_the_restored_balance(
        case in arb_case(3),
        realized in prop::collection::vec(0u64..=3, 3),
    ) {
        let case = Case { complete: true, ..case };
        let onchain = signed_only(&case);
        if decide_case(&case, &onchain).is_ok() {
            let kept = case
                .counts
                .iter()
                .zip(&realized)
                .map(|((_, _, count), used)| (*count).min(*used) > 0)
                .collect::<Vec<_>>();
            let realized_counts = case
                .counts
                .iter()
                .zip(&realized)
                .map(|((lane, class, count), used)| (*lane, *class, (*count).min(*used)))
                .filter(|(_, _, used)| *used > 0)
                .collect::<Vec<_>>();
            let realized_case = Case {
                counts: realized_counts,
                sources: case
                    .sources
                    .iter()
                    .map(|source| SourceCase {
                        permits: source
                            .permits
                            .iter()
                            .zip(&kept)
                            .filter(|(_, kept)| **kept)
                            .map(|(permitted, _)| *permitted)
                            .collect(),
                        ..source.clone()
                    })
                    .collect(),
                ..case.clone()
            };
            let realized_case = Case {
                extra: case.limit() - realized_case.bound(),
                ..realized_case
            };
            let fitted = with_instance(&realized_case, &onchain, |estimate, terms, funding| {
                let placed =
                    with_estimated_obligations(estimate, terms, estimate.bound_phlo, limits(), |obligations| {
                        let eligible = eligibility(obligations, funding, limits())?;
                        place_holds(
                            obligations,
                            &funding.sources,
                            &eligible,
                            &funding.onchain,
                            funding.total_exposure,
                            funding.cursors,
                            0,
                            &funding.fill_order,
                            limits().policy,
                        )
                    })?;
                let mut balances = BTreeMap::new();
                let mut debits = BTreeMap::new();
                for (position, source) in funding.sources.iter().enumerate() {
                    let custody: [u8; 32] = source
                        .custody
                        .try_into()
                        .expect("the case custodies have 32 bytes");
                    balances.insert(custody, u128::from(source.capacity));
                    debits.insert(custody, u128::from(placed.known[position]));
                }
                Ok::<_, OfferedGateRejection>(held_capacity_guard(&balances, &debits, &BTreeMap::new()))
            });
            prop_assert!(matches!(fitted, Ok(Ok(()))), "{:?}", fitted);
        }
    }
}

/// The example of design §8.3 with two members. A purse of 1,000,000 carries
/// the holds 500,001 and 390,001. The code of member 1 pays 600,000 out of
/// the purse, and its charge is 20,000. SystemVault alone would reserve the
/// debit, but the guard fails, because 400,000 − 20,000 < 390,001. Without
/// member 2 the guard holds.
#[test]
fn the_guard_keeps_a_later_hold_that_the_vault_condition_loses() {
    let balances = BTreeMap::from([(purse(1), 400_000u128)]);
    let debits = BTreeMap::from([(purse(1), 20_000u128)]);
    let later = BTreeMap::from([(purse(1), 390_001u128)]);
    assert_eq!(
        held_capacity_guard(&balances, &debits, &later),
        Err(vec![purse(1)])
    );
    assert_eq!(
        held_capacity_guard(&balances, &debits, &BTreeMap::new()),
        Ok(())
    );
    assert_eq!(
        held_capacity_guard(
            &BTreeMap::new(),
            &BTreeMap::from([(purse(2), 0u128)]),
            &BTreeMap::new()
        ),
        Ok(()),
        "a zero debit needs no balance"
    );
}
