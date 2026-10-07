//! The pure layer of the PathMap spaces (P5e: plan "PathMap delivery", decision 9.3, F9).
//!
//! In a PathMap space a channel is a path. A send on a path is also a send on every prefix of the
//! path (FIPS, "PathMap (For MeTTa)"). This module holds the parts of the PathMap outer store that
//! read no store and write no store:
//!
//! - [`path_codec`]: the path of a channel, the keys of a path and its prefixes, and the segment
//!   codec of EPM1.
//! - [`reroot`]: the re-rooting of a delivered argument under the suffix of its path.
//! - [`adapter`]: the logical PathMap, and the adapter of the `EPathMap` of this tree.
//!
//! The engine of the PathMap spaces builds on these parts in a later wave (P5 design v3, A.11).
//! The engine is the outer store, the markers, the token and the agent.
//!
//! An example: a datum `v` on `@[0, 1, 2]` in the space `sid`, and a receive on `@[0, 1]`.
//!
//! ```text
//! path p = [0, 1, 2]                              (channel_path of @[0, 1, 2])
//! path_key(p)      = 03 00 | 03 02 | 03 04 | 00   (seg(0), seg(1), seg(2), terminator)
//! D(sid, path_key(p))                             holds the datum v
//! prefix keys of p = ε, 03 00, 03 00 03 02, 03 00 03 02 03 04
//! I(sid, prefix_key(q)) for each q ⪯ p            holds the markers of v and the waiters on q
//! q = [0, 1] ⪯ p                                  prefix_key(q) is a byte prefix of path_key(p)
//! the receive on @[0, 1] binds R([2], v)          for example [2, 42] when v = 42
//! ```
//!
//! Every function reserves its reads, copies and allocations on the meter of the job before it
//! makes them (decision 42). The formal model of this layer is `PathMapPrefix.v`.

pub mod adapter;
pub mod path_codec;
pub mod reroot;

pub use adapter::{
    canonical_entry, EPathMapAdapter, HeadEPathMapAdapter, LogicalEntry, LogicalPath,
    LogicalPathMap, PathMapMode,
};
pub use path_codec::{
    channel_path, compare_keys, encode_path, entry_key, into_path_elements, is_prefix_key,
    path_elements, path_key, plain_list, plain_list_elements, prefix_key, segment, EncodedPath,
    PATH_DEPTH_CAP,
};
pub use reroot::{reroot, reroot_payload};

/// The property runner of the P5 suites, shared with the integration tests: a fixed seed, 64
/// cases for a pure property, and no failure persistence.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../../../tests/support/props.rs"]
mod props;

/// The builders, the generators and the metering checks of the unit tests of this module.
#[cfg(test)]
mod test_support {
    use std::fmt::Debug;

    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{
        CostSignature, CostStack, EList, EPathMap, ESet, ETuple, EZipper, Expr, GBigRational,
        GFixedPoint, GPrivate, GUnforgeable, Par, Send,
    };
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
    use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
    use models::rust::rholang::sorter::sortable::Sortable;
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;
    use rspace_plus_plus::rspace::errors::RSpaceError;

    pub use super::props::{check, pure_config};
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::reified::costs::ScanMeter;
    use crate::rust::interpreter::spaces::errors::SpaceError;

    /// The dimensions that a space job charges (decision 42).
    pub const CHARGED: [HostWorkDimension; 3] = [
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::SearchStateBytes,
    ];

    /// A meter without a budget, as the production host runs a job.
    pub fn unmetered() -> ScanMeter { ScanMeter::new(None) }

    /// A `Par` that holds exactly the expression `instance`.
    pub fn expr(instance: ExprInstance) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(instance),
            }],
            ..Par::default()
        }
    }

    /// `GBool(value)`.
    pub fn gbool(value: bool) -> Par { expr(ExprInstance::GBool(value)) }

    /// `GInt(value)`.
    pub fn gint(value: i64) -> Par { expr(ExprInstance::GInt(value)) }

    /// `GString(text)`.
    pub fn gstring(text: &str) -> Par { expr(ExprInstance::GString(text.to_string())) }

    /// `GUri(text)`.
    pub fn guri(text: &str) -> Par { expr(ExprInstance::GUri(text.to_string())) }

    /// `GByteArray(bytes)`.
    pub fn gbytes(bytes: Vec<u8>) -> Par { expr(ExprInstance::GByteArray(bytes)) }

    /// `GDouble(bits)`.
    pub fn gdouble(bits: u64) -> Par { expr(ExprInstance::GDouble(bits)) }

    /// `GBigInt(bytes)`.
    pub fn gbigint(bytes: Vec<u8>) -> Par { expr(ExprInstance::GBigInt(bytes)) }

    /// `GBigRat(numerator, denominator)`.
    pub fn gbigrat(numerator: Vec<u8>, denominator: Vec<u8>) -> Par {
        expr(ExprInstance::GBigRat(GBigRational {
            numerator,
            denominator,
        }))
    }

    /// `GFixedPoint(unscaled, scale)`.
    pub fn gfixed(unscaled: Vec<u8>, scale: u32) -> Par {
        expr(ExprInstance::GFixedPoint(GFixedPoint { unscaled, scale }))
    }

    /// `GFloat32(bits)`, which has no structural arm.
    pub fn gfloat32(bits: u32) -> Par { expr(ExprInstance::GFloat32(bits)) }

    /// The name `GPrivate(id)`.
    pub fn gprivate(id: Vec<u8>) -> Par {
        Par {
            unforgeables: vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
            }],
            ..Par::default()
        }
    }

    /// The plain list of `ps`.
    pub fn glist(ps: Vec<Par>) -> Par {
        expr(ExprInstance::EListBody(EList {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }))
    }

    /// The ground tuple of `ps`.
    pub fn gtuple(ps: Vec<Par>) -> Par {
        expr(ExprInstance::ETupleBody(ETuple {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
        }))
    }

    /// The set of `ps`, which has no structural arm.
    pub fn eset(ps: Vec<Par>) -> Par {
        expr(ExprInstance::ESetBody(ESet {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }))
    }

    /// The `EPathMap` value with the entries `ps`.
    pub fn epathmap(ps: Vec<Par>) -> EPathMap {
        EPathMap {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }
    }

    /// The PathMap value `{| ps |}`.
    pub fn pathmap(ps: Vec<Par>) -> Par { expr(ExprInstance::EPathmapBody(epathmap(ps))) }

    /// A zipper over the PathMap `{| ps |}`.
    pub fn zipper(ps: Vec<Par>) -> Par {
        expr(ExprInstance::EZipperBody(EZipper {
            pathmap: Some(epathmap(ps)),
            current_path: Vec::new(),
            is_write_zipper: false,
            locally_free: Vec::new(),
            connective_used: false,
        }))
    }

    /// The process `chan!(data)`.
    pub fn send(chan: Par, data: Vec<Par>) -> Par {
        Par {
            sends: vec![Send {
                chan: Some(chan),
                data,
                persistent: false,
                locally_free: Vec::new(),
                connective_used: false,
            }],
            ..Par::default()
        }
    }

    /// A `Par` with the cost stack of one quoted `Par`.
    pub fn cost_stacked(quoted: Par) -> Par {
        Par {
            cost_stacks: vec![CostStack {
                cells: vec![CostSignature {
                    value: Some(models::rhoapi::cost_signature::Value::Quote(quoted)),
                }],
            }],
            ..gint(1)
        }
    }

    /// The canonical form of `par`: the term of the host sorter.
    pub fn sorted(par: &Par) -> Par { ParSortMatcher::sort_match(par).term }

    /// Runs `run` under a budget with no bound, and returns its result and its usage in the
    /// charged dimensions.
    pub fn usage_of<T>(run: impl Fn(&ScanMeter) -> Result<T, SpaceError>) -> (T, [u64; 3]) {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let value = run(&ScanMeter::new(Some(budget.clone())))
            .expect("a budget with no bound accepts the work");
        (
            value,
            CHARGED.map(|dimension| budget.usage(dimension).get()),
        )
    }

    /// Checks the metering of `run`. With its usage as the limits it succeeds, and it gives the
    /// result of a run without a budget. A cut of one unit in any charged dimension rejects it
    /// with `HostWorkRejected`.
    pub fn assert_cut<T: Debug + PartialEq>(run: impl Fn(&ScanMeter) -> Result<T, SpaceError>) {
        let (metered, usage) = usage_of(&run);
        assert_eq!(
            run(&unmetered()).expect("a run without a budget succeeds"),
            metered,
            "a metered run gives the result of a run without a budget"
        );
        assert!(
            usage.iter().any(|used| *used > 0),
            "the work reserves something"
        );
        let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        for (dimension, used) in CHARGED.into_iter().zip(usage) {
            exact.set(dimension, HostWorkLimit::new(used));
        }
        assert_eq!(
            run(&ScanMeter::new(Some(HostWorkBudget::new(exact)))).expect("the exact usage fits"),
            metered
        );
        for (dimension, used) in CHARGED.into_iter().zip(usage) {
            if let Some(cut) = used.checked_sub(1) {
                let mut limits = exact;
                limits.set(dimension, HostWorkLimit::new(cut));
                assert_eq!(
                    run(&ScanMeter::new(Some(HostWorkBudget::new(limits)))).err(),
                    Some(SpaceError::Host(RSpaceError::HostWorkRejected)),
                    "a cut of {dimension} rejects the work"
                );
            }
        }
    }

    /// A leaf path element: every scalar arm, a name, and leaves outside the structural grammar.
    /// The small ranges make equal elements likely.
    pub fn leaf() -> impl Strategy<Value = Par> {
        prop_oneof![
            any::<bool>().prop_map(gbool),
            (-3i64..300).prop_map(gint),
            prop_oneof![Just(i64::MIN), Just(i64::MAX)].prop_map(gint),
            "[ab]{0,2}".prop_map(|text| gstring(&text)),
            "[ab]{0,2}".prop_map(|text| guri(&text)),
            prop::collection::vec(0u8..3, 0..3).prop_map(gbytes),
            prop_oneof![Just(0u64), Just(1.5f64.to_bits()), any::<u64>()].prop_map(gdouble),
            prop::collection::vec(0u8..3, 0..2).prop_map(gbigint),
            (
                prop::collection::vec(0u8..3, 0..2),
                prop::collection::vec(0u8..3, 0..2)
            )
                .prop_map(|(numerator, denominator)| gbigrat(numerator, denominator)),
            (prop::collection::vec(0u8..3, 0..2), 0u32..300)
                .prop_map(|(unscaled, scale)| gfixed(unscaled, scale)),
            prop::collection::vec(0u8..3, 0..3).prop_map(gprivate),
            Just(Par::default()),
            (0i64..3).prop_map(|value| eset(vec![gint(value)])),
            (0u32..3).prop_map(gfloat32),
            (0i64..2).prop_map(|value| send(gint(value), vec![gint(value)])),
        ]
    }

    /// A path element: a leaf, or a list or a tuple of path elements.
    pub fn element() -> impl Strategy<Value = Par> {
        leaf().prop_recursive(2, 12, 3, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..3).prop_map(glist),
                prop::collection::vec(inner, 0..3).prop_map(gtuple),
            ]
        })
    }

    /// A path of at most 3 elements.
    pub fn path() -> impl Strategy<Value = Vec<Par>> { prop::collection::vec(element(), 0..4) }

    /// The entries of a PathMap value: elements, and lists that hold a path.
    pub fn entries() -> impl Strategy<Value = Vec<Par>> {
        prop::collection::vec(prop_oneof![element(), path().prop_map(glist)], 0..4)
    }

    /// A PathMap value.
    pub fn map_value() -> impl Strategy<Value = Par> { entries().prop_map(pathmap) }

    /// A value that is not a PathMap. A list can hold a PathMap, and a zipper is not a PathMap.
    pub fn other_value() -> impl Strategy<Value = Par> {
        prop_oneof![
            4 => element(),
            1 => entries().prop_map(|ps| glist(vec![pathmap(ps)])),
            1 => entries().prop_map(zipper),
        ]
    }

    /// Fails the case when `condition` does not hold.
    pub fn ensure(condition: bool, message: &str) -> Result<(), TestCaseError> {
        match condition {
            true => Ok(()),
            false => Err(TestCaseError::fail(message.to_string())),
        }
    }
}
