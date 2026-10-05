//! The param schemas of the space families and the suggestion for an unknown name (P4 design
//! C.7, step 2, and C.8).
//!
//! A schema lists the named params that a family accepts. The factory checks the params of its
//! constructor against the constructor schema, and the handle method `produce` checks its
//! params against the send schema. P4 registers only `hashmap:queue`, and both of its schemas
//! are empty. A family without a schema is not available.
//!
//! The suggestion for an unknown name is the candidate with the least Levenshtein distance,
//! when that distance is at most [`SUGGESTION_DISTANCE`]. A tie goes to the bytewise least
//! candidate, so the suggestion is deterministic.

use models::rust::space_factory_urn::{SpaceFactoryUrn, SpaceInner, SpaceOuter};

/// The greatest edit distance of a suggestion.
pub const SUGGESTION_DISTANCE: usize = 2;

/// The named params of the factory of `family`. A family that this phase does not register has
/// no schema.
pub fn constructor_schema(family: SpaceFactoryUrn) -> Option<&'static [&'static str]> {
    match (family.outer, family.inner) {
        (SpaceOuter::HashMap, SpaceInner::Queue) => Some(&[]),
        (SpaceOuter::HashMap, _)
        | (SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::Vector | SpaceOuter::HashSet, _) => {
            None
        }
    }
}

/// The named params of a send to a collection of `family`: the schema of its discipline. The
/// queue discipline takes none, so `priority` is an unknown param of a queue.
pub fn send_schema(family: SpaceFactoryUrn) -> Option<&'static [&'static str]> {
    match (family.outer, family.inner) {
        (SpaceOuter::HashMap, SpaceInner::Queue) => Some(&[]),
        (SpaceOuter::HashMap, _)
        | (SpaceOuter::PathMap | SpaceOuter::Array | SpaceOuter::Vector | SpaceOuter::HashSet, _) => {
            None
        }
    }
}

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
    use super::*;

    #[test]
    fn space_factory_schemas_cover_only_the_registered_family() {
        for family in SpaceFactoryUrn::all() {
            let registered = family == SpaceFactoryUrn::HASHMAP_QUEUE;
            assert_eq!(constructor_schema(family).is_some(), registered, "{family}");
            assert_eq!(send_schema(family).is_some(), registered, "{family}");
        }
        assert_eq!(
            constructor_schema(SpaceFactoryUrn::HASHMAP_QUEUE),
            Some(&[][..])
        );
        assert_eq!(send_schema(SpaceFactoryUrn::HASHMAP_QUEUE), Some(&[][..]));
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
