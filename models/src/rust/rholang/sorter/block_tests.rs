// D-E4 (DR-111): the charges of the metered sorter in block mode.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::mem::size_of;

use shared::rust::clone_backing::{self, BackingError, BackingMeter, CloneBacking};

use super::cost_accounting_sorter::{sort_signature, sort_signature_metered};
use super::expr_sort_matcher::ExprSortMatcher;
use super::if_sort_matcher::IfSortMatcher;
use super::match_sort_matcher::MatchSortMatcher;
use super::metered::SorterMeter;
use super::new_sort_matcher::NewSortMatcher;
use super::par_sort_matcher::ParSortMatcher;
use super::receive_sort_matcher::ReceiveSortMatcher;
use super::score_tree::{ScoreAtom, Tree};
use super::send_sort_matcher::SendSortMatcher;
use super::sortable::Sortable;
use super::unforgeable_sort_matcher::UnforgeableSortMatcher;
use crate::rhoapi::cost_signature::Value;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{
    CostSignature, CostSignatureCompound, EList, EMethod, EPathMap, ETuple, EZipper, Expr,
    GBigRational, GFixedPoint, GPrivate, GUnforgeable, If, Match, New, Par, Receive, Send,
};

const SLOT: usize = size_of::<Tree<ScoreAtom>>();

/// Every reservation that `charge` makes through a sorter meter, in order.
fn sorter_log(
    charge: impl FnOnce(&SorterMeter<'_>) -> Result<(), BackingError>,
) -> Vec<[usize; 3]> {
    let log = RefCell::new(Vec::with_capacity(4_096));
    let backing = |operations: usize, scanned: usize, bytes: usize| {
        log.borrow_mut().push([operations, scanned, bytes]);
        Ok(())
    };
    charge(&SorterMeter::new(&backing)).expect("an unlimited meter");
    log.into_inner()
}

/// Every reservation of one walk of the shared walker, in order.
fn walk_log(walk: impl FnOnce(&dyn BackingMeter) -> Result<(), BackingError>) -> Vec<[usize; 3]> {
    let log = RefCell::new(Vec::with_capacity(4_096));
    let backing = |operations: usize, scanned: usize, bytes: usize| {
        log.borrow_mut().push([operations, scanned, bytes]);
        Ok(())
    };
    walk(&backing).expect("an unlimited walk");
    log.into_inner()
}

fn total(log: &[[usize; 3]]) -> [usize; 3] {
    log.iter()
        .fold([0; 3], |[o, s, b], [operations, scanned, bytes]| {
            [o + operations, s + scanned, b + bytes]
        })
}

/// The reservations of one block copy of `value` and the release of the copy.
fn copy_log<T: CloneBacking>(value: &T) -> Vec<[usize; 3]> {
    walk_log(|meter| clone_backing::reserve_blocks_copy_and_cleanup(value, meter))
}

/// How much the total charge grows from `small` to `large`, per dimension.
fn growth(small: &[[usize; 3]], large: &[[usize; 3]]) -> [usize; 3] {
    let [small, large] = [total(small), total(large)];
    [0, 1, 2].map(|index| large[index] - small[index])
}

/// A charge stops at its first rejected reservation: when the meter rejects
/// reservation `cut`, the charge returns the rejection and makes no later
/// reservation.
fn assert_stops_at_every_cut(
    name: &str,
    charge: &dyn Fn(&SorterMeter<'_>) -> Result<(), BackingError>,
) {
    let reservations = sorter_log(charge).len();
    assert!(reservations > 0, "{name}: the charge reserves");
    for cut in 0..reservations {
        let calls = Cell::new(0usize);
        let backing = |_: usize, _: usize, _: usize| {
            let call = calls.get();
            calls.set(call + 1);
            if call == cut {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            charge(&SorterMeter::new(&backing)),
            Err(BackingError::Rejected),
            "{name}: cut {cut}"
        );
        assert_eq!(calls.get(), cut + 1, "{name}: cut {cut}");
    }
}

/// The reservations of `score_vec(capacity)`: one read of each slot, then the
/// charge of `vec`.
fn score_vec_log(capacity: usize) -> Vec<[usize; 3]> {
    vec![[0, capacity * SLOT, 0], [capacity, 0, capacity * SLOT]]
}

/// The slots that Rule S charged in `log`: each `score_vec(c)` reserves the
/// pair `(0, c * SLOT, 0)`, `(c, 0, c * SLOT)`.
fn rule_s_slots(log: &[[usize; 3]]) -> usize {
    log.windows(2)
        .filter_map(|pair| {
            let [first, second] = [pair[0], pair[1]];
            let capacity = second[0];
            (first == [0, capacity * SLOT, 0] && second == [capacity, 0, capacity * SLOT])
                .then_some(capacity)
        })
        .sum()
}

/// A charge succeeds with exactly its usage and is rejected one unit short in
/// each dimension that it uses.
fn assert_exact_credit(name: &str, charge: &dyn Fn(&SorterMeter<'_>) -> Result<(), BackingError>) {
    let expected = total(&sorter_log(charge));
    let within = |limit: [usize; 3]| {
        let used = RefCell::new([0usize; 3]);
        let backing = |operations: usize, scanned: usize, bytes: usize| {
            let [o, s, b] = *used.borrow();
            let next = [o + operations, s + scanned, b + bytes];
            if next.iter().zip(limit).any(|(value, bound)| *value > bound) {
                return Err(BackingError::Rejected);
            }
            *used.borrow_mut() = next;
            Ok(())
        };
        charge(&SorterMeter::new(&backing))
    };
    within(expected).expect("the exact credit");
    for dimension in 0..3 {
        if expected[dimension] > 0 {
            let mut short = expected;
            short[dimension] -= 1;
            assert!(
                matches!(within(short), Err(BackingError::Rejected)),
                "{name}: dimension {dimension}"
            );
        }
    }
}

fn string_par(text: &str, locally_free: Vec<u8>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(text.to_owned())),
        }],
        locally_free,
        ..Default::default()
    }
}

fn private(id: Vec<u8>) -> GUnforgeable {
    GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
    }
}

/// D-E4 (DR-111): each block wrapper of the sorter meter makes exactly the
/// reservations of its shared walk, and `score_vec` prepays one read of each
/// slot before the charge of `vec`.
#[test]
fn sorter_block_wrappers_charge_the_shared_block_walks() {
    let bytes = vec![7u8; 4_096];
    let pars = vec![string_par(&"p".repeat(4_096), vec![1; 8]), Par::default()];
    let copy = sorter_log(|meter| meter.clone_blocks(&bytes).map(drop));
    assert_eq!(
        copy,
        walk_log(|meter| clone_backing::reserve_blocks_copy_and_cleanup(&bytes, meter))
    );
    let slice = sorter_log(|meter| meter.clone_slice_blocks(&pars).map(drop));
    assert_eq!(
        slice,
        walk_log(|meter| clone_backing::reserve_blocks_slice_copy_and_cleanup(&pars, meter))
    );
    let inspection = walk_log(|meter| clone_backing::inspect_blocks(&pars[0], meter));
    assert_eq!(
        sorter_log(|meter| meter.inspect_blocks(&pars[0])),
        inspection
    );
    assert_eq!(
        sorter_log(|meter| meter.hash_insert_blocks(&pars[0])),
        inspection.repeat(3)
    );
    let mut capacity = 0;
    let scores = sorter_log(|meter| {
        capacity = meter.score_vec(5)?.capacity();
        Ok(())
    });
    assert_eq!(scores, score_vec_log(5));
    assert!(capacity >= 5);
    assert_exact_credit("clone_blocks", &|meter| {
        meter.clone_blocks(&bytes).map(drop)
    });
    assert_exact_credit("clone_slice_blocks", &|meter| {
        meter.clone_slice_blocks(&pars).map(drop)
    });
    assert_exact_credit("hash_insert_blocks", &|meter| {
        meter.hash_insert_blocks(&pars[0])
    });
    assert_exact_credit("score_vec", &|meter| meter.score_vec(5).map(drop));
}

/// D-E4 (DR-111): the block wrappers and the main sorts stop at their first
/// rejected reservation. So no copy, read or allocation follows a rejection.
#[test]
fn sorter_block_wrappers_stop_at_every_cut() {
    let bytes = vec![7u8; 4_096];
    let pars = vec![string_par(&"p".repeat(4_096), vec![1; 8]), Par::default()];
    let signature = CostSignature {
        value: Some(Value::Compound(CostSignatureCompound {
            elements: vec![
                CostSignature {
                    value: Some(Value::Ground(vec![2; 40])),
                },
                CostSignature {
                    value: Some(Value::Ground(vec![1; 40])),
                },
            ],
        })),
    };
    let channel = Par {
        unforgeables: vec![private(vec![2; 32]), private(vec![1; 32])],
        ..Default::default()
    };
    assert_stops_at_every_cut("clone_blocks", &|meter| {
        meter.clone_blocks(&bytes).map(drop)
    });
    assert_stops_at_every_cut("clone_slice_blocks", &|meter| {
        meter.clone_slice_blocks(&pars).map(drop)
    });
    assert_stops_at_every_cut("inspect_blocks", &|meter| meter.inspect_blocks(&pars[0]));
    assert_stops_at_every_cut("hash_insert_blocks", &|meter| {
        meter.hash_insert_blocks(&pars[0])
    });
    assert_stops_at_every_cut("score_vec", &|meter| meter.score_vec(5).map(drop));
    assert_stops_at_every_cut("compound signature", &|meter| {
        sort_signature_metered(&signature, meter).map(drop)
    });
    assert_stops_at_every_cut("channel", &|meter| {
        ParSortMatcher::sort_match_metered(&channel, meter).map(drop)
    });
}

/// D-E4 (DR-111): a ground signature charges its depth entry, its header, two
/// block copies of the bytes (the term and the score leaf) and two score
/// vectors. The charge grows with the bytes by exactly two block copies.
#[test]
fn ground_signature_sort_charges_two_block_copies_and_score_vectors() {
    for size in [32, 4_096] {
        let bytes = vec![9u8; size];
        let signature = CostSignature {
            value: Some(Value::Ground(bytes.clone())),
        };
        let mut sorted = None;
        let log = sorter_log(|meter| {
            sorted = Some(sort_signature_metered(&signature, meter)?);
            Ok(())
        });
        assert_eq!(sorted, Some(sort_signature(&signature)));
        let copy = walk_log(|meter| clone_backing::reserve_blocks_copy_and_cleanup(&bytes, meter));
        let expected = [
            vec![[1, size_of::<usize>(), 0]],
            vec![[1, size_of::<CostSignature>(), 0]],
            copy.clone(),
            copy,
            score_vec_log(1),
            score_vec_log(2),
        ]
        .concat();
        assert_eq!(log, expected, "{size} bytes");
        assert_eq!(rule_s_slots(&log), 3);
    }
}

/// D-E4 (DR-111): a private name charges a score vector, a block copy of its
/// id for the score leaf, a block copy of the term and the score node.
#[test]
fn private_unforgeable_sort_charges_a_leaf_copy_and_a_term_copy() {
    for size in [32, 4_096] {
        let unforgeable = private(vec![5; size]);
        let log = sorter_log(|meter| {
            UnforgeableSortMatcher::sort_match_metered(&unforgeable, meter).map(drop)
        });
        let id = vec![5u8; size];
        let expected = [
            score_vec_log(1),
            walk_log(|meter| clone_backing::reserve_blocks_copy_and_cleanup(&id, meter)),
            walk_log(|meter| clone_backing::reserve_blocks_copy_and_cleanup(&unforgeable, meter)),
            score_vec_log(2),
        ]
        .concat();
        assert_eq!(log, expected, "{size} bytes");
    }
}

/// D-E4 (DR-111): every ground expression charges its depth entry, its header
/// and one block copy of its term. A string, URI, byte array or big integer
/// also charges one block copy of its payload for the score leaf. A rational
/// copies both parts, a fixed point copies its unscaled bytes, and a u64
/// copies its eight big-endian bytes as a slice. The rest are score vectors.
#[test]
fn ground_expression_sorts_charge_block_copies() {
    let expr = |instance: ExprInstance| Expr {
        expr_instance: Some(instance),
    };
    let text = "t".repeat(4_096);
    let bytes = vec![0x5Au8; 4_096];
    let rational = GBigRational {
        numerator: vec![1; 64],
        denominator: vec![3; 32],
    };
    let fixed = GFixedPoint {
        unscaled: vec![7; 48],
        scale: 6,
    };
    let wide = u64::MAX - 1;
    let wide_bytes = walk_log(|meter| {
        clone_backing::reserve_blocks_slice_copy_and_cleanup(&wide.to_be_bytes(), meter)
    });
    let leaf = |payload: Vec<[usize; 3]>| [payload, score_vec_log(1), score_vec_log(2)].concat();
    let cases = [
        ("GBool", expr(ExprInstance::GBool(true)), score_vec_log(2)),
        ("GInt", expr(ExprInstance::GInt(-7)), score_vec_log(2)),
        (
            "GString",
            expr(ExprInstance::GString(text.clone())),
            leaf(copy_log(&text)),
        ),
        (
            "GUri",
            expr(ExprInstance::GUri(text.clone())),
            leaf(copy_log(&text)),
        ),
        (
            "GByteArray",
            expr(ExprInstance::GByteArray(bytes.clone())),
            leaf(copy_log(&bytes)),
        ),
        (
            "GDouble",
            expr(ExprInstance::GDouble(1.5f64.to_bits())),
            score_vec_log(2),
        ),
        (
            "GFloat32",
            expr(ExprInstance::GFloat32(2.5f32.to_bits())),
            score_vec_log(2),
        ),
        (
            "GBigInt",
            expr(ExprInstance::GBigInt(bytes.clone())),
            leaf(copy_log(&bytes)),
        ),
        (
            "GBigRat",
            expr(ExprInstance::GBigRat(rational.clone())),
            [
                copy_log(&rational.numerator),
                copy_log(&rational.denominator),
                score_vec_log(2),
                score_vec_log(3),
            ]
            .concat(),
        ),
        (
            "GFixedPoint",
            expr(ExprInstance::GFixedPoint(fixed.clone())),
            [
                copy_log(&fixed.unscaled),
                score_vec_log(1),
                score_vec_log(2),
                score_vec_log(3),
            ]
            .concat(),
        ),
        (
            "GUint64",
            expr(ExprInstance::GUint64(wide)),
            leaf(wide_bytes),
        ),
        ("GInt32", expr(ExprInstance::GInt32(-3)), score_vec_log(2)),
        ("GUint32", expr(ExprInstance::GUint32(3)), score_vec_log(2)),
        (
            "GUint16",
            expr(ExprInstance::GUint16(65_535)),
            score_vec_log(2),
        ),
        ("GUint8", expr(ExprInstance::GUint8(255)), score_vec_log(2)),
        (
            "absent",
            Expr {
                expr_instance: None,
            },
            score_vec_log(1),
        ),
    ];
    for (name, value, tail) in cases {
        let mut sorted = None;
        let log = sorter_log(|meter| {
            sorted = Some(ExprSortMatcher::sort_match_metered(&value, meter)?);
            Ok(())
        });
        assert_eq!(
            sorted,
            Some(<ExprSortMatcher as Sortable<Expr>>::sort_match(&value)),
            "{name}"
        );
        let expected = [
            vec![[1, size_of::<usize>(), 0], [1, size_of::<Expr>(), 0]],
            copy_log(&value),
            tail,
        ]
        .concat();
        assert_eq!(log, expected, "{name}");
    }
}

/// D-E4 (DR-111), Rule S: every score vector prepays one read of each slot. A
/// ground atom has 3 slots, a compound of m ground atoms 5m + 1, and a channel
/// with two private names 12.
#[test]
fn score_vectors_prepay_one_read_per_slot() {
    let ground = |byte: u8| CostSignature {
        value: Some(Value::Ground(vec![byte; 32])),
    };
    let atom = sorter_log(|meter| sort_signature_metered(&ground(1), meter).map(drop));
    assert_eq!(rule_s_slots(&atom), 3);
    for atoms in [2u8, 3] {
        let compound = CostSignature {
            value: Some(Value::Compound(CostSignatureCompound {
                elements: (1..=atoms).map(ground).collect(),
            })),
        };
        let log = sorter_log(|meter| sort_signature_metered(&compound, meter).map(drop));
        assert_eq!(
            rule_s_slots(&log),
            5 * usize::from(atoms) + 1,
            "{atoms} atoms"
        );
    }
    let channel = Par {
        unforgeables: vec![private(vec![2; 32]), private(vec![1; 32])],
        ..Default::default()
    };
    let log = sorter_log(|meter| ParSortMatcher::sort_match_metered(&channel, meter).map(drop));
    assert_eq!(rule_s_slots(&log), 12);
}

/// D-E4 (DR-111): the locally free bytes of every container are copied once
/// with a block copy, either alone or inside one block copy of the whole
/// container. Bincode and the scores do not read them, so a 4 KiB pad grows
/// the charge by exactly the growth of one block copy. A method name is
/// copied twice: once for its score leaf and once in the copy of the method.
#[test]
fn locally_free_copies_are_single_block_copies() {
    let pad = vec![0xA5u8; 4_096];
    let copy_growth = growth(&copy_log(&Vec::<u8>::new()), &copy_log(&pad));
    let par_growth = |small: &Par, large: &Par| {
        growth(
            &sorter_log(|meter| ParSortMatcher::sort_match_metered(small, meter).map(drop)),
            &sorter_log(|meter| ParSortMatcher::sort_match_metered(large, meter).map(drop)),
        )
    };
    let expr_growth = |small: &ExprInstance, large: &ExprInstance| {
        let [small, large] = [small, large].map(|instance| Expr {
            expr_instance: Some(instance.clone()),
        });
        growth(
            &sorter_log(|meter| ExprSortMatcher::sort_match_metered(&small, meter).map(drop)),
            &sorter_log(|meter| ExprSortMatcher::sort_match_metered(&large, meter).map(drop)),
        )
    };

    let par = string_par("x", Vec::new());
    assert_eq!(
        par_growth(&par, &string_par("x", pad.clone())),
        copy_growth,
        "par"
    );

    let send = Send {
        chan: Some(Par::default()),
        data: vec![string_par("d", Vec::new())],
        ..Default::default()
    };
    let padded_send = Send {
        locally_free: pad.clone(),
        ..send.clone()
    };
    assert_eq!(
        growth(
            &sorter_log(|meter| SendSortMatcher::sort_match_metered(&send, meter).map(drop)),
            &sorter_log(|meter| SendSortMatcher::sort_match_metered(&padded_send, meter).map(drop)),
        ),
        copy_growth,
        "send"
    );

    let receive = Receive {
        body: Some(Par::default()),
        ..Default::default()
    };
    let padded_receive = Receive {
        locally_free: pad.clone(),
        ..receive.clone()
    };
    assert_eq!(
        growth(
            &sorter_log(|meter| ReceiveSortMatcher::sort_match_metered(&receive, meter).map(drop)),
            &sorter_log(|meter| {
                ReceiveSortMatcher::sort_match_metered(&padded_receive, meter).map(drop)
            }),
        ),
        copy_growth,
        "receive"
    );

    let matched = Match {
        target: Some(Par::default()),
        ..Default::default()
    };
    let padded_match = Match {
        locally_free: pad.clone(),
        ..matched.clone()
    };
    assert_eq!(
        growth(
            &sorter_log(|meter| MatchSortMatcher::sort_match_metered(&matched, meter).map(drop)),
            &sorter_log(
                |meter| MatchSortMatcher::sort_match_metered(&padded_match, meter).map(drop)
            ),
        ),
        copy_growth,
        "match"
    );

    let branch = If {
        condition: Some(Par::default()),
        if_true: Some(Par::default()),
        if_false: Some(Par::default()),
        ..Default::default()
    };
    let padded_branch = If {
        locally_free: pad.clone(),
        ..branch.clone()
    };
    assert_eq!(
        growth(
            &sorter_log(|meter| IfSortMatcher::sort_match_metered(&branch, meter).map(drop)),
            &sorter_log(|meter| IfSortMatcher::sort_match_metered(&padded_branch, meter).map(drop)),
        ),
        copy_growth,
        "if"
    );

    let new = New {
        bind_count: 1,
        p: Some(Par::default()),
        ..Default::default()
    };
    let padded_new = New {
        locally_free: pad.clone(),
        ..new.clone()
    };
    assert_eq!(
        growth(
            &sorter_log(|meter| NewSortMatcher::sort_match_metered(&new, meter).map(drop)),
            &sorter_log(|meter| NewSortMatcher::sort_match_metered(&padded_new, meter).map(drop)),
        ),
        copy_growth,
        "new"
    );

    let list = EList {
        ps: vec![string_par("l", Vec::new())],
        ..Default::default()
    };
    assert_eq!(
        expr_growth(
            &ExprInstance::EListBody(list.clone()),
            &ExprInstance::EListBody(EList {
                locally_free: pad.clone(),
                ..list
            }),
        ),
        copy_growth,
        "list"
    );

    let pathmap = EPathMap {
        ps: vec![string_par("m", Vec::new())],
        ..Default::default()
    };
    assert_eq!(
        expr_growth(
            &ExprInstance::EPathmapBody(pathmap.clone()),
            &ExprInstance::EPathmapBody(EPathMap {
                locally_free: pad.clone(),
                ..pathmap.clone()
            }),
        ),
        copy_growth,
        "pathmap"
    );

    let zipper = EZipper {
        pathmap: Some(pathmap.clone()),
        current_path: vec![vec![1, 2]],
        ..Default::default()
    };
    assert_eq!(
        expr_growth(
            &ExprInstance::EZipperBody(zipper.clone()),
            &ExprInstance::EZipperBody(EZipper {
                locally_free: pad.clone(),
                ..zipper.clone()
            }),
        ),
        copy_growth,
        "zipper"
    );
    assert_eq!(
        expr_growth(
            &ExprInstance::EZipperBody(zipper.clone()),
            &ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap {
                    locally_free: pad.clone(),
                    ..pathmap
                }),
                ..zipper
            }),
        ),
        copy_growth,
        "zipper pathmap"
    );

    let tuple = ETuple {
        ps: vec![string_par("t", Vec::new())],
        ..Default::default()
    };
    assert_eq!(
        expr_growth(
            &ExprInstance::ETupleBody(tuple.clone()),
            &ExprInstance::ETupleBody(ETuple {
                locally_free: pad.clone(),
                ..tuple
            }),
        ),
        copy_growth,
        "tuple"
    );

    let method = EMethod {
        method_name: "m".to_owned(),
        target: Some(Par::default()),
        arguments: vec![string_par("a", Vec::new())],
        ..Default::default()
    };
    assert_eq!(
        expr_growth(
            &ExprInstance::EMethodBody(method.clone()),
            &ExprInstance::EMethodBody(EMethod {
                locally_free: pad.clone(),
                ..method.clone()
            }),
        ),
        copy_growth,
        "method"
    );
    let long_name = format!("m{}", "n".repeat(4_096));
    let name_growth = growth(&copy_log(&method.method_name), &copy_log(&long_name));
    assert_eq!(
        expr_growth(
            &ExprInstance::EMethodBody(method.clone()),
            &ExprInstance::EMethodBody(EMethod {
                method_name: long_name,
                ..method
            }),
        ),
        name_growth.map(|dimension| 2 * dimension),
        "method name"
    );
}

/// D-E4 (DR-111): the injection scan of a New copies the injections once,
/// inspects the copy once for the loop, and inspects each sorted injection
/// term once for its release.
#[test]
fn new_injection_scan_charges_copy_iteration_and_release() {
    let injected = string_par(&"i".repeat(4_096), Vec::new());
    let injections = BTreeMap::from([
        ("rho:a".to_owned(), injected.clone()),
        ("rho:b".to_owned(), injected.clone()),
    ]);
    let new = New {
        bind_count: 1,
        p: Some(Par::default()),
        injections: injections.clone(),
        ..Default::default()
    };
    let log = sorter_log(|meter| NewSortMatcher::sort_match_metered(&new, meter).map(drop));
    let copy_then_scan = [
        walk_log(|meter| clone_backing::reserve_blocks_copy_and_cleanup(&injections, meter)),
        walk_log(|meter| clone_backing::inspect_blocks(&injections, meter)),
    ]
    .concat();
    assert!(
        log.windows(copy_then_scan.len())
            .any(|window| window == copy_then_scan),
        "the copy of the injections is followed by its inspection"
    );
    let sorted = <ParSortMatcher as Sortable<Par>>::sort_match(&injected).term;
    let release = walk_log(|meter| clone_backing::inspect_blocks(&sorted, meter));
    let releases = log
        .windows(release.len())
        .filter(|window| *window == release)
        .count();
    assert_eq!(releases, 2);
}

/// D-E4 (DR-111): the main sorts reserve their exact credit before their work.
#[test]
fn sorts_accept_exact_credit_and_reject_each_shorter_dimension() {
    let ground = CostSignature {
        value: Some(Value::Ground(vec![3; 65])),
    };
    let compound = CostSignature {
        value: Some(Value::Compound(CostSignatureCompound {
            elements: vec![ground.clone(), ground.clone()],
        })),
    };
    let channel = Par {
        unforgeables: vec![private(vec![2; 32]), private(vec![1; 32])],
        ..Default::default()
    };
    let string = Expr {
        expr_instance: Some(ExprInstance::GString("s".repeat(4_096))),
    };
    assert_exact_credit("ground signature", &|meter| {
        sort_signature_metered(&ground, meter).map(drop)
    });
    assert_exact_credit("compound signature", &|meter| {
        sort_signature_metered(&compound, meter).map(drop)
    });
    assert_exact_credit("channel", &|meter| {
        ParSortMatcher::sort_match_metered(&channel, meter).map(drop)
    });
    assert_exact_credit("string expression", &|meter| {
        ExprSortMatcher::sort_match_metered(&string, meter).map(drop)
    });
}
