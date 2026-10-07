//! The param schemas of the space families and the suggestion for an unknown name (P4 design
//! C.7, step 2, and C.8, and P5 design v3, A.7, A.8 and A.10).
//!
//! A schema lists the named params that a family accepts. The factory checks the params of its
//! constructor against the constructor schema. The handle method `produce` checks its params
//! against the send schema, which the family dispatch gives (`families::send_params`), so the
//! wired families have one source of truth. A family without a constructor schema is not
//! available.
//!
//! P5 gives a constructor schema to the 24 families of the HashMap, PathMap, Array and Vector
//! stores whose inner collection is a bag, a queue, a stack, a set, a cell or a priority queue:
//!
//! | Families                                         | Params                         | Required |
//! |--------------------------------------------------|--------------------------------|----------|
//! | `hashmap`, `pathmap` and `vector`: bag, queue, stack, set, cell | none            | none     |
//! | `hashmap`, `pathmap` and `vector`: `priorityqueue` | `priorities`                 | none     |
//! | `array`: bag, queue, stack, set, cell            | `cyclic`, `size`               | `size`   |
//! | `array`: `priorityqueue`                         | `cyclic`, `priorities`, `size` | `size`   |
//!
//! Each param has one value rule ([`ParamRule`]):
//!
//! * `size` is a `GInt` in `[1, i64::MAX]` (F7). An Array space needs it. Its storage is lazy.
//! * `cyclic` is a `GBool` (F7). An Array space without it is fixed, as with `cyclic = false`.
//! * `priorities` is a `GInt` of at least 1 (I-10). Without it, the priority levels have no bound.
//!
//! The VectorDB families (P6) and the HashSet families (P7) have no schema yet.
//!
//! The suggestion for an unknown name is the candidate with the least Levenshtein distance,
//! when that distance is at most [`SUGGESTION_DISTANCE`]. A tie goes to the bytewise least
//! candidate, so the suggestion is deterministic.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};

use crate::rust::interpreter::spaces::record::{expr_par, sole_expr};

/// The greatest edit distance of a suggestion.
pub const SUGGESTION_DISTANCE: usize = 2;

/// The name of the param that gives the number of index names of an Array space (F7).
pub const SIZE_PARAM: &str = "size";

/// The name of the param that makes the index names of an Array space wrap around (F7).
pub const CYCLIC_PARAM: &str = "cyclic";

/// The name of the param that bounds the priority levels of a priority queue space (I-10).
pub const PRIORITIES_PARAM: &str = "priorities";

/// The constructor schema of a family without params.
const NO_PARAMS: &[&str] = &[];

/// The constructor schema of a HashMap or a Vector priority queue.
const PRIORITY_QUEUE_PARAMS: &[&str] = &[PRIORITIES_PARAM];

/// The constructor schema of an Array family other than the priority queue.
const ARRAY_PARAMS: &[&str] = &[CYCLIC_PARAM, SIZE_PARAM];

/// The constructor schema of the Array priority queue.
const ARRAY_PRIORITY_QUEUE_PARAMS: &[&str] = &[CYCLIC_PARAM, PRIORITIES_PARAM, SIZE_PARAM];

/// The params that every Array factory requires.
const ARRAY_REQUIRED: &[&str] = &[SIZE_PARAM];

/// The one table of the constructor schemas: the named params of the factory of `family` in
/// bytewise order, and the params that it requires. [`constructor_schema`] and
/// [`required_params`] are its two views.
fn schema(family: SpaceFactoryUrn) -> Option<(&'static [&'static str], &'static [&'static str])> {
    match (family.outer, family.inner) {
        (
            SpaceOuter::HashMap | SpaceOuter::PathMap | SpaceOuter::Vector,
            SpaceInner::Bag
            | SpaceInner::Queue
            | SpaceInner::Stack
            | SpaceInner::Set
            | SpaceInner::Cell,
        ) => Some((NO_PARAMS, NO_PARAMS)),
        (
            SpaceOuter::HashMap | SpaceOuter::PathMap | SpaceOuter::Vector,
            SpaceInner::PriorityQueue,
        ) => Some((PRIORITY_QUEUE_PARAMS, NO_PARAMS)),
        (
            SpaceOuter::Array,
            SpaceInner::Bag
            | SpaceInner::Queue
            | SpaceInner::Stack
            | SpaceInner::Set
            | SpaceInner::Cell,
        ) => Some((ARRAY_PARAMS, ARRAY_REQUIRED)),
        (SpaceOuter::Array, SpaceInner::PriorityQueue) => {
            Some((ARRAY_PRIORITY_QUEUE_PARAMS, ARRAY_REQUIRED))
        }
        (
            SpaceOuter::HashMap | SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::Vector,
            SpaceInner::VectorDb,
        )
        | (SpaceOuter::HashSet, _) => None,
    }
}

/// The named params of the factory of `family`, in bytewise order. A family outside the
/// families of this phase has no schema.
pub fn constructor_schema(family: SpaceFactoryUrn) -> Option<&'static [&'static str]> {
    schema(family).map(|(params, _)| params)
}

/// The named params that the factory of `family` requires: `size` for an Array family (F7). A
/// family without a schema requires none.
pub fn required_params(family: SpaceFactoryUrn) -> &'static [&'static str] {
    schema(family).map_or(NO_PARAMS, |(_, required)| required)
}

/// The rule of the value of a named constructor param.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamRule {
    /// `size`: a `GInt` in `[1, i64::MAX]` (F7).
    Size,
    /// `cyclic`: a `GBool` (F7).
    Cyclic,
    /// `priorities`: a `GInt` of at least 1 (I-10).
    Priorities,
}

impl ParamRule {
    /// The rule of the param `key`, or `None` for a name that no schema holds.
    pub fn of(key: &str) -> Option<ParamRule> {
        match key {
            SIZE_PARAM => Some(ParamRule::Size),
            CYCLIC_PARAM => Some(ParamRule::Cyclic),
            PRIORITIES_PARAM => Some(ParamRule::Priorities),
            _ => None,
        }
    }

    /// The canonical form of `value` when the rule accepts it: the one ground expression of
    /// `value` without a bit set. A value with another shape, with a connective flag or out of
    /// range gives `None`. The bit sets are not part of a value, so they do not count.
    pub fn canonical(self, value: &Par) -> Option<Par> {
        match (self, sole_expr(value), value.connective_used) {
            (
                ParamRule::Size | ParamRule::Priorities,
                Some(ExprInstance::GInt(count @ 1..=i64::MAX)),
                false,
            ) => Some(expr_par(ExprInstance::GInt(*count))),
            (ParamRule::Cyclic, Some(ExprInstance::GBool(flag)), false) => {
                Some(expr_par(ExprInstance::GBool(*flag)))
            }
            (ParamRule::Size | ParamRule::Priorities | ParamRule::Cyclic, _, _) => None,
        }
    }
}

// Disabled by P5 S1 (P5 design v3, A.3.4): the send schema of a family is the schema of its
// discipline, and the family dispatch gives it (`families::send_params`). A second table here
// could drift from the dispatch.
// /// The named params of a send to a collection of `family`: the schema of its discipline. The
// /// queue discipline takes none, so `priority` is an unknown param of a queue.
// pub fn send_schema(family: SpaceFactoryUrn) -> Option<&'static [&'static str]> {
//     match (family.outer, family.inner) {
//         (SpaceOuter::HashMap, SpaceInner::Queue) => Some(&[]),
//         (SpaceOuter::HashMap, _)
//         | (SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::Vector | SpaceOuter::HashSet, _) => {
//             None
//         }
//     }
// }

/// The candidate that is closest to `word`, at a Levenshtein distance of at most
/// [`SUGGESTION_DISTANCE`]. A tie goes to the bytewise least candidate.
pub fn suggestion<'a>(word: &str, candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .filter_map(|candidate| {
            distance_within(word, candidate, SUGGESTION_DISTANCE)
                .map(|distance| (distance, *candidate))
        })
        .min()
        .map(|(_, candidate)| candidate)
}

/// The Levenshtein distance of `word` and `candidate` over chars, when it is at most `bound`.
/// The function reads at most `|candidate| + bound + 1` chars of `word`, so a long word costs
/// no more than a short one.
fn distance_within(word: &str, candidate: &str, bound: usize) -> Option<usize> {
    let target: Vec<char> = candidate.chars().collect();
    let limit = target.len().checked_add(bound)?;
    let source: Vec<char> = word.chars().take(limit.checked_add(1)?).collect();
    if source.len() > limit || target.len() > source.len().checked_add(bound)? {
        return None;
    }
    let width = target.len().checked_add(1)?;
    let mut previous: Vec<usize> = (0..width).collect();
    let mut current: Vec<usize> = Vec::with_capacity(width);
    for (row, source_char) in source.iter().enumerate() {
        current.clear();
        current.push(row.checked_add(1)?);
        for (column, target_char) in target.iter().enumerate() {
            let substitution = previous
                .get(column)?
                .checked_add(usize::from(source_char != target_char))?;
            let insertion = current.get(column)?.checked_add(1)?;
            let deletion = previous.get(column.checked_add(1)?)?.checked_add(1)?;
            current.push(substitution.min(insertion).min(deletion));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous
        .last()
        .copied()
        .filter(|distance| *distance <= bound)
}

#[cfg(test)]
mod tests {
    use models::rust::utils::{new_gbool_par, new_gint_par, new_gstring_par};

    use super::*;
    use crate::rust::interpreter::spaces::factory::urn::{is_wired, registered_channel};
    use crate::rust::interpreter::spaces::families::send_params;

    /// The families that P5 gives a constructor schema: HashMap, PathMap, Array and Vector with
    /// the inner collections bag to priority queue.
    fn has_p5_schema(family: SpaceFactoryUrn) -> bool {
        matches!(
            family.outer,
            SpaceOuter::HashMap | SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::Vector
        ) && family.inner != SpaceInner::VectorDb
    }

    #[test]
    fn space_factory_schemas_cover_the_p5_families() {
        let mut covered = 0;
        for family in SpaceFactoryUrn::all() {
            assert_eq!(
                constructor_schema(family).is_some(),
                has_p5_schema(family),
                "{family}"
            );
            assert_eq!(send_params(family).is_some(), is_wired(family), "{family}");
            if registered_channel(family).is_some() {
                assert!(constructor_schema(family).is_some(), "{family}");
                assert!(send_params(family).is_some(), "{family}");
            }
            let Some(params) = constructor_schema(family) else {
                assert!(required_params(family).is_empty(), "{family}");
                continue;
            };
            covered += 1;
            assert!(
                params.windows(2).all(|pair| pair[0] < pair[1]),
                "{family}: the keys are distinct and in bytewise order"
            );
            assert!(
                params.iter().all(|key| ParamRule::of(key).is_some()),
                "{family}: every key has a value rule"
            );
            let required = required_params(family);
            assert!(
                required.iter().all(|key| params.contains(key)),
                "{family}: a required key is a key of the schema"
            );
            let (expected_params, expected_required): (&[&str], &[&str]) =
                match (family.outer, family.inner) {
                    (SpaceOuter::Array, SpaceInner::PriorityQueue) => {
                        (&["cyclic", "priorities", "size"], &["size"])
                    }
                    (SpaceOuter::Array, _) => (&["cyclic", "size"], &["size"]),
                    (_, SpaceInner::PriorityQueue) => (&["priorities"], &[]),
                    _ => (&[], &[]),
                };
            assert_eq!(params, expected_params, "{family}");
            assert_eq!(required, expected_required, "{family}");
        }
        assert_eq!(covered, 24);
        assert_eq!(
            constructor_schema(SpaceFactoryUrn::HASHMAP_QUEUE),
            Some(&[][..])
        );
        assert_eq!(send_params(SpaceFactoryUrn::HASHMAP_QUEUE), Some(&[][..]));
    }

    #[test]
    fn space_factory_param_rules_accept_only_their_canonical_values() {
        let int = |value: i64| new_gint_par(value, Vec::new(), false);
        let flag = |value: bool| new_gbool_par(value, Vec::new(), false);
        for (key, rule) in [
            ("size", ParamRule::Size),
            ("cyclic", ParamRule::Cyclic),
            ("priorities", ParamRule::Priorities),
        ] {
            assert_eq!(ParamRule::of(key), Some(rule));
        }
        for key in ["Size", "sizes", "priority", "capacity", ""] {
            assert_eq!(ParamRule::of(key), None, "{key}");
        }
        for rule in [ParamRule::Size, ParamRule::Priorities] {
            for value in [1, 2, 1_000, i64::MAX] {
                assert_eq!(rule.canonical(&int(value)), Some(int(value)), "{value}");
            }
            for value in [0, -1, i64::MIN] {
                assert_eq!(rule.canonical(&int(value)), None, "{value}");
            }
            assert_eq!(rule.canonical(&flag(true)), None);
        }
        for value in [true, false] {
            assert_eq!(ParamRule::Cyclic.canonical(&flag(value)), Some(flag(value)));
        }
        assert_eq!(ParamRule::Cyclic.canonical(&int(1)), None);
        let mut with_bits = int(4);
        with_bits.locally_free = vec![0b0000_0001];
        assert_eq!(ParamRule::Size.canonical(&with_bits), Some(int(4)));
        let mut flagged = int(4);
        flagged.connective_used = true;
        assert_eq!(ParamRule::Size.canonical(&flagged), None);
        let mut pair = int(4);
        pair.exprs.extend(int(5).exprs);
        assert_eq!(ParamRule::Size.canonical(&pair), None);
        for other in [
            Par::default(),
            new_gstring_par("4".to_string(), Vec::new(), false),
        ] {
            for rule in [ParamRule::Size, ParamRule::Cyclic, ParamRule::Priorities] {
                assert_eq!(rule.canonical(&other), None, "{rule:?} {other:?}");
            }
        }
    }

    #[test]
    fn space_factory_suggestion_is_the_nearest_candidate_within_two_edits() {
        let methods = ["config", "consume", "gensym", "produce", "qualifier"];
        assert_eq!(suggestion("prodcue", &methods), Some("produce"));
        assert_eq!(suggestion("produce", &methods), Some("produce"));
        assert_eq!(suggestion("gensim", &methods), Some("gensym"));
        assert_eq!(suggestion("qualifer", &methods), Some("qualifier"));
        assert_eq!(suggestion("conf", &methods), Some("config"));
        assert_eq!(suggestion("consum", &methods), Some("consume"));
        assert_eq!(suggestion("fetch", &methods), None);
        assert_eq!(suggestion("", &methods), None);
        assert_eq!(
            suggestion("persistant", &["persistent"]),
            Some("persistent")
        );
        assert_eq!(suggestion("priority", &["persistent"]), None);
        assert_eq!(suggestion("priority", &[]), None);
        assert_eq!(suggestion("prioritx", &["priority"]), Some("priority"));
    }

    #[test]
    fn space_factory_suggestion_breaks_ties_bytewise() {
        assert_eq!(suggestion("cat", &["cab", "bat"]), Some("bat"));
        assert_eq!(suggestion("cat", &["bat", "cab"]), Some("bat"));
        assert_eq!(suggestion("ab", &["a", "abc", "b"]), Some("a"));
        assert_eq!(suggestion("abcd", &["abxy", "abcx"]), Some("abcx"));
    }

    #[test]
    fn space_factory_distance_is_levenshtein_and_bounded() {
        assert_eq!(distance_within("kitten", "sitting", 3), Some(3));
        assert_eq!(distance_within("kitten", "sitting", 2), None);
        assert_eq!(distance_within("flaw", "lawn", 2), Some(2));
        assert_eq!(distance_within("", "ab", 2), Some(2));
        assert_eq!(distance_within("abc", "", 2), None);
        assert_eq!(distance_within("ünï", "uni", 2), Some(2));
        let long = "p".repeat(1 << 20);
        assert_eq!(distance_within(&long, "produce", 2), None);
        assert_eq!(suggestion(&long, &["produce"]), None);
    }
}
