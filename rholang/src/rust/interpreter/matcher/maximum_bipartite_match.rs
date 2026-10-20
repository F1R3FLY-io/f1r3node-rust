use std::cmp::Eq;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::hash::Hash;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/MaximumBipartiteMatch.scala
pub struct MaximumBipartiteMatch<P, T, R>
where
    P: Debug + Clone,
    T: Debug + Clone + Hash + Eq,
    R: Debug + Clone,
{
    match_function: Box<dyn FnMut(P, T) -> Option<R>>,
    matches: BTreeMap<Candidate<T>, (Pattern<P, T>, R)>,
    seen_targets: BTreeSet<Candidate<T>>,
}

type Pattern<P, T> = (P, Vec<Candidate<T>>);
type Candidate<T> = Indexed<T>;

enum SearchFrame<P, T, R> {
    Search {
        pattern: P,
        candidates: Vec<Candidate<T>>,
        next: usize,
    },
    Resume {
        candidate: Candidate<T>,
        pattern: Pattern<P, T>,
        result: R,
    },
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Ord, PartialOrd)]
struct Indexed<A> {
    value: A,
    index: usize,
}

impl<P, T, R> MaximumBipartiteMatch<P, T, R>
where
    P: Debug + Clone,
    T: Debug + Clone + Hash + Eq + Ord,
    R: Debug + Clone,
{
    pub fn new(match_function: Box<dyn FnMut(P, T) -> Option<R>>) -> Self {
        MaximumBipartiteMatch {
            match_function,
            matches: BTreeMap::new(),
            seen_targets: BTreeSet::new(),
        }
    }

    pub fn find_matches(&mut self, patterns: Vec<P>, targets: Vec<T>) -> Option<Vec<(T, P, R)>> {
        let mut ts = Vec::with_capacity(targets.len());
        for (index, value) in targets.into_iter().enumerate() {
            ts.push(Indexed { value, index });
        }
        let mut ps = Vec::with_capacity(patterns.len());
        for pattern in patterns {
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.work.reserve_clone(&ts)?;
            ps.push((pattern, ts.clone()));
        }
        for pattern in ps {
            self.reset_seen();
            if !self.find_match(pattern)? {
                return None;
            }
        }
        let mut result = Vec::with_capacity(self.matches.len());
        for (target, (pattern, match_result)) in &self.matches {
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.work.reserve_clone(&target.value)?;
            // self.work.reserve_clone(&pattern.0)?;
            // self.work.reserve_clone(match_result)?;
            result.push((
                target.value.clone(),
                pattern.0.clone(),
                match_result.clone(),
            ));
        }
        Some(result)
    }

    fn find_match(&mut self, pattern: Pattern<P, T>) -> Option<bool> {
        let mut frames = vec![SearchFrame::Search {
            pattern: pattern.0,
            candidates: pattern.1,
            next: 0,
        }];
        let mut completed = None;

        while let Some(frame) = frames.pop() {
            match frame {
                SearchFrame::Search {
                    pattern,
                    mut candidates,
                    mut next,
                } => {
                    let mut deferred = false;
                    while next < candidates.len() {
                        let candidate = &candidates[next];
                        if !self.not_seen(candidate) {
                            next += 1;
                            continue;
                        }
                        // Changed by D-O1 (DR-109): block accounting charges inline bytes
                        // once per enclosing block.
                        // self.work.reserve_clone(&pattern)?;
                        // self.work.reserve_clone(&candidate.value)?;
                        let matched =
                            (self.match_function)(pattern.clone(), candidate.value.clone());
                        if let Some(result) = matched {
                            // Changed by D-O1 (DR-109): block accounting charges inline bytes
                            // once per enclosing block.
                            // self.work.reserve_clone(candidate)?;
                            let candidate = candidate.clone();
                            // Changed by D-O1 (DR-109): block accounting charges inline bytes
                            // once per enclosing block.
                            // self.work.reserve_clone(&candidate)?;
                            self.add_seen(candidate.clone());
                            let previous_pattern = self.get_match(&candidate);
                            if next != 0 {
                                candidates.drain(..next);
                            }
                            let current_pattern = (pattern, candidates);
                            match previous_pattern {
                                None => {
                                    self.claim_match(candidate, current_pattern, result);
                                    completed = Some(true);
                                }
                                Some((pattern, candidates)) => {
                                    frames.push(SearchFrame::Resume {
                                        candidate,
                                        pattern: current_pattern,
                                        result,
                                    });
                                    frames.push(SearchFrame::Search {
                                        pattern,
                                        candidates,
                                        next: 0,
                                    });
                                    completed = None;
                                    deferred = true;
                                }
                            }
                            break;
                        }
                        next += 1;
                    }
                    if !deferred && completed.is_none() {
                        completed = Some(false);
                    }
                }
                SearchFrame::Resume {
                    candidate,
                    pattern,
                    result,
                } => {
                    if completed.take()? {
                        self.claim_match(candidate, pattern, result);
                        completed = Some(true);
                    } else {
                        frames.push(SearchFrame::Search {
                            pattern: pattern.0,
                            candidates: pattern.1,
                            next: 0,
                        });
                    }
                }
            }
        }
        completed
    }

    fn reset_seen(&mut self) { self.seen_targets.clear(); }

    fn not_seen(&self, candidate: &Candidate<T>) -> bool {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.work.reserve_inspect(candidate)?;
        // self.work.reserve_inspect(&self.seen_targets)?;
        !self.seen_targets.contains(candidate)
    }

    fn add_seen(&mut self, candidate: Candidate<T>) {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.work.reserve_inspect(&candidate)?;
        // self.work.reserve_inspect(&self.seen_targets)?;
        self.seen_targets.insert(candidate);
    }

    fn get_match(&self, candidate: &Candidate<T>) -> Option<Pattern<P, T>> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.work.reserve_inspect(candidate)?;
        // self.work.reserve_inspect(&self.matches)?;
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.work.reserve_clone(&matched.0)?;
        self.matches.get(candidate).map(|matched| matched.0.clone())
    }

    fn claim_match(&mut self, candidate: Candidate<T>, pattern: Pattern<P, T>, result: R) {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.work.reserve_inspect(&candidate)?;
        // self.work.reserve_inspect(&self.matches)?;
        self.matches.insert(candidate, (pattern, result));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reassign() -> Option<Vec<(u8, u8, u8)>> {
        let mut search = MaximumBipartiteMatch::new(Box::new(|pattern: u8, target: u8| {
            (pattern == 0 || pattern == target).then_some(target)
        }));
        search.find_matches(vec![0, 1], vec![1, 2])
    }

    #[test]
    fn nested_reassignment_preserves_candidate_order() {
        let mut search = MaximumBipartiteMatch::new(Box::new(|pattern: u8, target: u8| {
            (pattern == 0 || pattern == target).then_some(target)
        }));
        assert_eq!(
            search.find_matches(vec![0, 1, 2], vec![1, 2, 3]),
            Some(vec![(1, 1, 1), (2, 2, 2), (3, 0, 3)])
        );
    }

    #[test]
    fn long_candidate_scan_uses_constant_call_stack() {
        let mut search = MaximumBipartiteMatch::new(Box::new(|_: u16, target: u16| {
            (target == 16_384).then_some(target)
        }));
        assert_eq!(
            search.find_matches(vec![7], (0..=16_384).collect()),
            Some(vec![(16_384, 7, 16_384)])
        );
    }

    /// Changed by DR-131: the search no longer charges host work. The test
    /// keeps its check that a reassignment returns the matches in candidate
    /// order.
    #[test]
    fn reassignment_preserves_order() {
        assert_eq!(reassign(), Some(vec![(1, 1, 1), (2, 0, 2)]));
    }
}
