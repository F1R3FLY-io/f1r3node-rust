//! The re-rooting of a PathMap delivery (P5e: plan "PathMap delivery", decision 9.3).
//!
//! A send on the path `p` is also a send on every prefix `q ⪯ p`, as the FIPS states. A receive
//! on `q` gets each argument `v` of the datum re-rooted under the suffix `s = p∖q`, the elements
//! of `p` after `q`:
//!
//! ```text
//! R(ε, v)  = v
//! R(s, m)  = {| s |}                             s ≠ ε and m is a PathMap with no entry (the unit rule)
//! R(s, m)  = {| s ++ elems(e) : e ∈ m |}         s ≠ ε and m is a PathMap with entries
//! R(s, v)  = [s_1, …, s_k, v]                    s ≠ ε and v is not a PathMap
//! ```
//!
//! The FIPS example holds: `@[0,1,2]!({|"hi"|})` received at `@[0,1]` binds `{|[2,"hi"]|}`,
//! `@[0,1,2]!(42)` binds `[2, 42]`, and `@[0,1,2,"hi"]!({||})` binds `{|[2,"hi"]|}` too. In the
//! FIPS, `{||}` at the path `p` stands for the path `p` itself. The empty `EPathMap` of Rholang
//! has no entry, so the unit rule applies only under a suffix that is not empty, and `R(ε, {||})`
//! is `{||}`.
//!
//! A PathMap argument goes through an [`EPathMapAdapter`]: the adapter decodes the logical map,
//! the suffix prefixes each path, and the adapter encodes the result. So the delivered map is in
//! canonical form, with its entries in key order and one entry for each path. The order and the
//! repeats of the entries of the sent map do not change it. A pattern matches the re-rooted
//! value.
//!
//! The laws of `PathMapPrefix.v` hold for this function:
//!
//! - `reroot_ident`: `R(ε, v) = v`.
//! - `reroot_compose`: `R(s1, R(s2, m)) = R(s1 ++ s2, m)` for a PathMap `m`, and for every other
//!   value `v`, `R(s1 ++ s2, v)` prepends `s1` to the list `R(s2, v)` when `s2 ≠ ε`.
//! - `reroot_injective`: under a suffix that is not empty, two values have equal deliveries
//!   exactly when they are logically equal. The empty map and the map of the empty path both
//!   stand for the unit, so they are logically equal.
//!
//! The function takes the value and moves its parts into the result. It reserves each copy of an
//! element of the suffix and each allocation, and the adapter reserves its own work (decision
//! 42). `R(ε, v)` returns `v` with no work.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPathMap, Expr, Par};
use rspace_plus_plus::rspace::errors::RSpaceError;

use super::adapter::{EPathMapAdapter, LogicalPath, LogicalPathMap};
use super::path_codec::plain_list;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::record::sole_expr;

/// `R(suffix, value)`: the argument `value` of a datum, as a receive on a prefix sees it. The
/// `suffix` is the rest of the path of the datum. `adapter` reads and writes PathMap values.
pub fn reroot<A: EPathMapAdapter>(
    suffix: &[Par],
    value: Par,
    adapter: &A,
    meter: &ScanMeter,
) -> Result<Par, SpaceError> {
    match (suffix.is_empty(), is_pathmap(&value)) {
        (true, _) => Ok(value),
        (false, true) => {
            let map = into_pathmap(value)?;
            let logical = adapter.decode(map, meter)?;
            let rest = LogicalPath::new(copy_elements(suffix, 0, meter)?, meter)?;
            let rerooted = match logical.is_empty() {
                true => LogicalPathMap::singleton(rest, meter)?,
                false => logical.prefixed(&rest, meter)?,
            };
            pathmap_par(adapter.encode(rerooted, meter)?, meter)
        }
        (false, false) => {
            let mut elements = copy_elements(suffix, 1, meter)?;
            elements.push(value);
            plain_list(elements, meter)
        }
    }
}

/// `R(suffix, v)` for each argument `v` of `payload`, in order.
pub fn reroot_payload<A: EPathMapAdapter>(
    suffix: &[Par],
    payload: Vec<Par>,
    adapter: &A,
    meter: &ScanMeter,
) -> Result<Vec<Par>, SpaceError> {
    let mut rerooted = meter.buffer::<Par>(payload.len())?;
    for value in payload {
        rerooted.push(reroot(suffix, value, adapter, meter)?);
    }
    Ok(rerooted)
}

/// True when `value` is exactly one `EPathMap`, with no other part. The bit sets do not count,
/// as for `sole_expr`.
fn is_pathmap(value: &Par) -> bool {
    matches!(sole_expr(value), Some(ExprInstance::EPathmapBody(_)))
}

/// The `EPathMap` of a value for which [`is_pathmap`] holds.
fn into_pathmap(value: Par) -> Result<EPathMap, SpaceError> {
    let Par { mut exprs, .. } = value;
    match exprs.pop() {
        Some(Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }) => Ok(map),
        _ => Err(SpaceError::Inconsistent("a PathMap value has no PathMap")),
    }
}

/// Copies of `elements` in a list with room for `extra` more elements. Each copy and the list
/// are reserved first.
fn copy_elements(
    elements: &[Par],
    extra: usize,
    meter: &ScanMeter,
) -> Result<Vec<Par>, SpaceError> {
    let room = elements
        .len()
        .checked_add(extra)
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut copies = meter.buffer::<Par>(room)?;
    for element in elements {
        meter.reserve_copy(element)?;
        copies.push(element.clone());
    }
    Ok(copies)
}

/// The `Par` of the PathMap value `map`, with the bit sets of the map. The copy of the bit set
/// and the expression list are reserved first.
fn pathmap_par(map: EPathMap, meter: &ScanMeter) -> Result<Par, SpaceError> {
    let locally_free = meter.copy(&map.locally_free)?;
    let connective_used = map.connective_used;
    let mut exprs = meter.buffer::<Expr>(1)?;
    exprs.push(Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map)),
    });
    Ok(Par {
        exprs,
        locally_free,
        connective_used,
        ..Par::default()
    })
}

#[cfg(test)]
mod tests {
    use models::rhoapi::var::VarInstance;
    use models::rhoapi::Var;
    use proptest::prelude::*;

    use super::*;
    use crate::rust::interpreter::spaces::pathmap::adapter::HeadEPathMapAdapter;
    use crate::rust::interpreter::spaces::pathmap::path_codec::{path_elements, tag};
    use crate::rust::interpreter::spaces::pathmap::test_support::*;

    fn r(suffix: &[Par], value: Par) -> Par {
        reroot(suffix, value, &HeadEPathMapAdapter, &unmetered()).expect("the value re-roots")
    }

    fn ints(values: &[i64]) -> Vec<Par> { values.iter().map(|value| gint(*value)).collect() }

    /// The logical value of `value`: the keys of the paths of a PathMap, or the value itself. The
    /// empty map stands for the unit `{ε}`, as in the FIPS.
    #[derive(Debug, PartialEq)]
    enum Logical {
        Map(Vec<Vec<u8>>),
        Other(Par),
    }

    fn logical(value: &Par) -> Logical {
        match is_pathmap(value) {
            true => {
                let map = HeadEPathMapAdapter
                    .decode(
                        into_pathmap(value.clone()).expect("a PathMap"),
                        &unmetered(),
                    )
                    .expect("the map decodes");
                let keys: Vec<Vec<u8>> = map
                    .entries()
                    .iter()
                    .map(|entry| entry.path().key().to_vec())
                    .collect();
                match keys.is_empty() {
                    true => Logical::Map(vec![vec![tag::TERM]]),
                    false => Logical::Map(keys),
                }
            }
            false => Logical::Other(value.clone()),
        }
    }

    /// The entry `[e]` for a bare entry `e`, and `e` for a list of one element: the other form
    /// of the same path.
    fn other_form(entry: &Par) -> Par {
        match path_elements(entry) {
            [element] if element == entry => glist(vec![entry.clone()]),
            [element] => element.clone(),
            _ => entry.clone(),
        }
    }

    /// Two values that are logically equal, or two values that are likely to be distinct.
    fn value_pair() -> impl Strategy<Value = (Par, Par)> {
        prop_oneof![
            (map_value(), map_value()),
            entries().prop_map(|entries| {
                let mut reversed = entries.clone();
                reversed.reverse();
                (pathmap(entries), pathmap(reversed))
            }),
            entries().prop_map(|entries| {
                let doubled = [entries.clone(), entries.clone()].concat();
                (pathmap(entries), pathmap(doubled))
            }),
            entries().prop_map(|entries| {
                let swapped = entries.iter().map(other_form).collect();
                (pathmap(entries), pathmap(swapped))
            }),
            Just((pathmap(Vec::new()), pathmap(vec![glist(Vec::new())]))),
            (other_value(), other_value()),
            other_value().prop_map(|value| (value.clone(), value)),
            (map_value(), other_value()),
        ]
        .prop_map(|(left, right)| (sorted(&left), sorted(&right)))
    }

    // The FIPS example of the PathMap outer store (`PathMapPrefix.v`, `fips_example`).
    #[test]
    fn fips_example() {
        let hi = gstring("hi");
        let hello = gstring("hello");
        let there = gstring("there");
        // `@[0,1,2]!({|"hi"|}) | @[0,1,2]!({|"hello"|}) | @[0,1,3]!({|"there"|})`, received on
        // `@[0,1]`, binds one of `{|[2,"hi"]|}`, `{|[2,"hello"]|}` and `{|[3,"there"]|}`.
        assert_eq!(
            r(&ints(&[2]), pathmap(vec![hi.clone()])),
            pathmap(vec![glist(vec![gint(2), hi.clone()])])
        );
        assert_eq!(
            r(&ints(&[2]), pathmap(vec![hello.clone()])),
            pathmap(vec![glist(vec![gint(2), hello.clone()])])
        );
        assert_eq!(
            r(&ints(&[3]), pathmap(vec![there.clone()])),
            pathmap(vec![glist(vec![gint(3), there.clone()])])
        );
        // `@[0,1,2]!(42)` received on `@[0,1]` binds `[2, 42]`.
        assert_eq!(r(&ints(&[2]), gint(42)), glist(ints(&[2, 42])));
        // `@[0,1,2,"hi"]!({||})` received on `@[0,1]` binds `{|[2,"hi"]|}` (the unit rule).
        assert_eq!(
            r(&[gint(2), hi.clone()], pathmap(Vec::new())),
            pathmap(vec![glist(vec![gint(2), hi.clone()])])
        );
        // Received on `@[]`, the datum shows its whole path.
        assert_eq!(
            r(&ints(&[0, 1, 2]), pathmap(vec![hi.clone()])),
            pathmap(vec![glist(vec![gint(0), gint(1), gint(2), hi.clone()])])
        );
        // On the exact path nothing changes, and `{||}` stays empty.
        assert_eq!(r(&[], pathmap(vec![hi.clone()])), pathmap(vec![hi.clone()]));
        assert_eq!(r(&[], pathmap(Vec::new())), pathmap(Vec::new()));
        // `@[0,1,2]!({|"hi"|})` and `@[0,1,2,"hi"]!({||})` deliver one value on every prefix.
        let path = ints(&[0, 1, 2]);
        for depth in 0..=path.len() {
            let suffix = &path[depth..];
            let longer = [suffix.to_vec(), vec![hi.clone()]].concat();
            assert_eq!(
                r(suffix, pathmap(vec![hi.clone()])),
                r(&longer, pathmap(Vec::new())),
                "{depth}"
            );
        }
    }

    #[test]
    fn a_pathmap_delivery_is_in_canonical_form() {
        // The entries of the sent map in any order and with repeats give one delivered map.
        let sent = pathmap(vec![
            gstring("b"),
            glist(vec![gstring("a")]),
            gstring("a"),
            glist(vec![gint(1), gint(2)]),
        ]);
        assert_eq!(
            r(&ints(&[9]), sent),
            pathmap(vec![
                glist(vec![gint(9), gint(1), gint(2)]),
                glist(vec![gint(9), gstring("a")]),
                glist(vec![gint(9), gstring("b")]),
            ])
        );
        // The empty path entry is the unit too.
        assert_eq!(
            r(&ints(&[9]), pathmap(vec![glist(Vec::new())])),
            r(&ints(&[9]), pathmap(Vec::new()))
        );
        // A list value is one element of the delivered list, and a zipper is not a PathMap.
        assert_eq!(
            r(&ints(&[9]), glist(ints(&[1, 2]))),
            glist(vec![gint(9), glist(ints(&[1, 2]))])
        );
        let cursor = zipper(vec![gint(1)]);
        assert_eq!(r(&ints(&[9]), cursor.clone()), glist(vec![gint(9), cursor]));
        assert_eq!(
            reroot_payload(
                &ints(&[9]),
                vec![gint(1), pathmap(vec![gint(2)])],
                &HeadEPathMapAdapter,
                &unmetered()
            ),
            Ok(vec![
                glist(ints(&[9, 1])),
                pathmap(vec![glist(ints(&[9, 2]))])
            ])
        );
    }

    #[test]
    fn a_rerooting_fails_closed() {
        let meter = unmetered();
        let nested = pathmap(vec![pathmap(vec![gint(1)])]);
        assert_eq!(
            reroot(&ints(&[1]), nested.clone(), &HeadEPathMapAdapter, &meter),
            Err(SpaceError::UnsupportedPathElement)
        );
        assert_eq!(
            reroot(&[], nested.clone(), &HeadEPathMapAdapter, &meter),
            Ok(nested),
            "the exact path delivers the value as it is"
        );
        assert_eq!(
            reroot(
                &[pathmap(Vec::new())],
                pathmap(vec![gint(1)]),
                &HeadEPathMapAdapter,
                &meter
            ),
            Err(SpaceError::UnsupportedPathElement)
        );
        let pattern = expr(ExprInstance::EPathmapBody(EPathMap {
            remainder: Some(Var {
                var_instance: Some(VarInstance::FreeVar(0)),
            }),
            connective_used: true,
            ..epathmap(vec![gint(1)])
        }));
        assert_eq!(
            reroot(&ints(&[1]), pattern, &HeadEPathMapAdapter, &meter),
            Err(SpaceError::Inconsistent("a PathMap value has a remainder"))
        );
        assert_eq!(
            reroot_payload(
                &ints(&[1]),
                vec![gint(1), pathmap(vec![zipper(Vec::new())])],
                &HeadEPathMapAdapter,
                &meter
            ),
            Err(SpaceError::UnsupportedPathElement)
        );
    }

    #[test]
    fn a_rerooting_rejects_a_cut_budget() {
        let suffix = vec![gint(2), send(gint(0), vec![gint(1)])];
        let map = pathmap(vec![gstring("b"), glist(ints(&[1, 2])), gstring("b")]);
        assert_cut(|meter| reroot(&suffix, map.clone(), &HeadEPathMapAdapter, meter));
        assert_cut(|meter| reroot(&suffix, pathmap(Vec::new()), &HeadEPathMapAdapter, meter));
        assert_cut(|meter| reroot(&suffix, gint(42), &HeadEPathMapAdapter, meter));
        assert_cut(|meter| {
            reroot_payload(
                &suffix,
                vec![gint(42), map.clone()],
                &HeadEPathMapAdapter,
                meter,
            )
        });
        // The exact path does no work.
        let (value, usage) =
            usage_of(|meter| reroot(&[], map.clone(), &HeadEPathMapAdapter, meter));
        assert_eq!(value, map);
        assert_eq!(usage, [0, 0, 0]);
    }

    // `PathMapPrefix.v`, `reroot_ident`.
    #[test]
    fn prop_reroot_ident() {
        check(
            pure_config(),
            prop_oneof![map_value(), other_value()],
            |value| ensure(r(&[], value.clone()) == value, "R(ε, v) = v"),
        );
    }

    // `PathMapPrefix.v`, `reroot_compose`.
    #[test]
    fn prop_reroot_compose() {
        check(
            pure_config(),
            (path(), path(), map_value(), other_value()),
            |(outer, inner, map, other)| {
                let (map, other) = (sorted(&map), sorted(&other));
                let joined = [outer.clone(), inner.clone()].concat();
                ensure(
                    r(&outer, r(&inner, map.clone())) == r(&joined, map),
                    "R(s1, R(s2, m)) = R(s1 ++ s2, m) for a PathMap m",
                )?;
                match inner.is_empty() {
                    true => Ok(()),
                    false => {
                        let shorter = r(&inner, other.clone());
                        let extended = [outer, path_elements(&shorter).to_vec()].concat();
                        ensure(
                            r(&joined, other) == glist(extended),
                            "R(s1 ++ s2, v) prepends s1 to the list R(s2, v)",
                        )
                    }
                }
            },
        );
    }

    // `PathMapPrefix.v`, `reroot_injective`: under a suffix that is not empty, two values have
    // equal deliveries exactly when they are logically equal. The reducer sorts every datum, so
    // the values are sorted.
    #[test]
    fn prop_reroot_injective() {
        check(
            pure_config(),
            (path(), value_pair()),
            |(suffix, (left, right))| {
                ensure(
                    (r(&[], left.clone()) == r(&[], right.clone())) == (left == right),
                    "R(ε, ·) is injective",
                )?;
                match suffix.is_empty() {
                    true => Ok(()),
                    false => ensure(
                        (r(&suffix, left.clone()) == r(&suffix, right.clone()))
                            == (logical(&left) == logical(&right)),
                        "equal deliveries exactly for logically equal values",
                    ),
                }
            },
        );
    }

    // The FIPS equation `@p!({|a|}) = @(p ++ [a])!({||})` for an element `a` that is not a plain
    // list: both deliver one value on every prefix of `p` (`PathMapPrefix.v`, `unit_shift`).
    #[test]
    fn prop_unit_shift() {
        check(pure_config(), (path(), element()), |(path, element)| {
            let element = sorted(&element);
            match path_elements(&element) {
                [single] if single == &element => {
                    for depth in 0..=path.len() {
                        let suffix = &path[depth..];
                        let longer = [suffix.to_vec(), vec![element.clone()]].concat();
                        ensure(
                            r(suffix, pathmap(vec![element.clone()]))
                                == r(&longer, pathmap(Vec::new())),
                            "{|a|} on p and {||} on p ++ [a] deliver one value",
                        )?;
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        });
    }
}
