use std::cmp::Eq;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::hash::Hash;

use rspace_plus_plus::rspace::errors::RSpaceError;
use shared::rust::clone_backing::{BackingError, CloneBacking, Walker};
use shared::rust::collection_backing::tree_backing;

use super::spatial_matcher::MatcherWork;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/MaximumBipartiteMatch.scala
pub struct MaximumBipartiteMatch<'a, P, T, R>
where
    P: Debug + Clone + CloneBacking,
    T: Debug + Clone + CloneBacking + Hash + Eq,
    R: Debug + Clone + CloneBacking,
{
    match_function: Box<dyn FnMut(P, T) -> Option<R> + 'a>,
    work: MatcherWork<'a>,
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

impl<A: CloneBacking> CloneBacking for Indexed<A> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        walker.push(&self.value)?;
        walker.push(&self.index)
    }
}

impl<'a, P, T, R> MaximumBipartiteMatch<'a, P, T, R>
where
    P: Debug + Clone + CloneBacking,
    T: Debug + Clone + CloneBacking + Hash + Eq + Ord,
    R: Debug + Clone + CloneBacking,
{
    pub(super) fn new(
        match_function: Box<dyn FnMut(P, T) -> Option<R> + 'a>,
        work: MatcherWork<'a>,
    ) -> Self {
        MaximumBipartiteMatch {
            match_function,
            work,
            matches: BTreeMap::new(),
            seen_targets: BTreeSet::new(),
        }
    }

    pub fn find_matches(&mut self, patterns: Vec<P>, targets: Vec<T>) -> Option<Vec<(T, P, R)>> {
        let mut ts = Vec::new();
        self.work.reserve_vec(&mut ts, targets.len())?;
        for (index, value) in targets.into_iter().enumerate() {
            ts.push(Indexed { value, index });
        }
        let mut ps = Vec::new();
        self.work.reserve_vec(&mut ps, patterns.len())?;
        for pattern in patterns {
            self.work.reserve_clone(&ts)?;
            ps.push((pattern, ts.clone()));
        }
        for pattern in ps {
            self.reset_seen()?;
            if !self.find_match(pattern)? {
                return None;
            }
        }
        let mut result = Vec::new();
        self.work.reserve_vec(&mut result, self.matches.len())?;
        for (target, (pattern, match_result)) in &self.matches {
            self.work.reserve_clone(&target.value)?;
            self.work.reserve_clone(&pattern.0)?;
            self.work.reserve_clone(match_result)?;
            result.push((
                target.value.clone(),
                pattern.0.clone(),
                match_result.clone(),
            ));
        }
        Some(result)
    }

    fn find_match(&mut self, pattern: Pattern<P, T>) -> Option<bool> {
        let mut frames = Vec::new();
        self.work.reserve_vec(&mut frames, 1)?;
        frames.push(SearchFrame::Search {
            pattern: pattern.0,
            candidates: pattern.1,
            next: 0,
        });
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
                        self.work.reserve(1, 0, 0)?;
                        let candidate = &candidates[next];
                        if !self.not_seen(candidate)? {
                            next += 1;
                            continue;
                        }
                        self.work.reserve_clone(&pattern)?;
                        self.work.reserve_clone(&candidate.value)?;
                        let matched =
                            (self.match_function)(pattern.clone(), candidate.value.clone());
                        self.work.reserve(0, 0, 0)?;
                        if let Some(result) = matched {
                            self.work.reserve_clone(candidate)?;
                            let candidate = candidate.clone();
                            self.work.reserve_clone(&candidate)?;
                            self.add_seen(candidate.clone())?;
                            let previous_pattern = self.get_match(&candidate)?;
                            if next != 0 {
                                let remaining = candidates.len() - next;
                                let Some(scanned) =
                                    remaining.checked_mul(std::mem::size_of::<Candidate<T>>())
                                else {
                                    return self.work.reject(RSpaceError::HostWorkRejected);
                                };
                                self.work.reserve(remaining, scanned, 0)?;
                                candidates.drain(..next);
                            }
                            let current_pattern = (pattern, candidates);
                            match previous_pattern {
                                None => {
                                    self.claim_match(candidate, current_pattern, result)?;
                                    completed = Some(true);
                                }
                                Some((pattern, candidates)) => {
                                    self.work.reserve_vec(&mut frames, 2)?;
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
                        self.claim_match(candidate, pattern, result)?;
                        completed = Some(true);
                    } else {
                        self.work.reserve_vec(&mut frames, 1)?;
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

    fn reset_seen(&mut self) -> Option<()> {
        self.work.reserve(self.seen_targets.len(), 0, 0)?;
        self.seen_targets.clear();
        Some(())
    }

    fn not_seen(&self, candidate: &Candidate<T>) -> Option<bool> {
        self.work.reserve_inspect(candidate)?;
        self.work.reserve_inspect(&self.seen_targets)?;
        Some(!self.seen_targets.contains(candidate))
    }

    fn add_seen(&mut self, candidate: Candidate<T>) -> Option<()> {
        let Some(entries) = self.seen_targets.len().checked_add(1) else {
            return self.work.reject(RSpaceError::HostWorkRejected);
        };
        let Some((operations, bytes)) = tree_backing::<Candidate<T>, ()>(entries) else {
            return self.work.reject(RSpaceError::HostWorkRejected);
        };
        self.work.reserve(operations, bytes, bytes)?;
        self.work.reserve_inspect(&candidate)?;
        self.work.reserve_inspect(&self.seen_targets)?;
        self.seen_targets.insert(candidate);
        Some(())
    }

    fn get_match(&self, candidate: &Candidate<T>) -> Option<Option<Pattern<P, T>>> {
        self.work.reserve_inspect(candidate)?;
        self.work.reserve_inspect(&self.matches)?;
        match self.matches.get(candidate) {
            Some(matched) => {
                self.work.reserve_clone(&matched.0)?;
                Some(Some(matched.0.clone()))
            }
            None => Some(None),
        }
    }

    fn claim_match(
        &mut self,
        candidate: Candidate<T>,
        pattern: Pattern<P, T>,
        result: R,
    ) -> Option<()> {
        let Some(entries) = self.matches.len().checked_add(1) else {
            return self.work.reject(RSpaceError::HostWorkRejected);
        };
        let Some((operations, bytes)) = tree_backing::<Candidate<T>, (Pattern<P, T>, R)>(entries)
        else {
            return self.work.reject(RSpaceError::HostWorkRejected);
        };
        self.work.reserve(operations, bytes, bytes)?;
        self.work.reserve_inspect(&candidate)?;
        self.work.reserve_inspect(&self.matches)?;
        self.matches.insert(candidate, (pattern, result));
        Some(())
    }
}

#[cfg(test)]
mod metered_tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;
    use crate::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

    fn reassign(work: MatcherWork<'_>) -> Option<Vec<(u8, u8, u8)>> {
        let mut search = MaximumBipartiteMatch::new(
            Box::new(|pattern: u8, target: u8| {
                (pattern == 0 || pattern == target).then_some(target)
            }),
            work,
        );
        search.find_matches(vec![0, 1], vec![1, 2])
    }

    #[test]
    fn nested_reassignment_preserves_candidate_order() {
        let context = SpatialMatcherContext::new();
        let mut search = MaximumBipartiteMatch::new(
            Box::new(|pattern: u8, target: u8| {
                (pattern == 0 || pattern == target).then_some(target)
            }),
            context.work().unwrap(),
        );
        assert_eq!(
            search.find_matches(vec![0, 1, 2], vec![1, 2, 3]),
            Some(vec![(1, 1, 1), (2, 2, 2), (3, 0, 3)])
        );
    }

    #[test]
    fn long_candidate_scan_uses_constant_call_stack() {
        let context = SpatialMatcherContext::new();
        let mut search = MaximumBipartiteMatch::new(
            Box::new(|_: u16, target: u16| (target == 16_384).then_some(target)),
            context.work().unwrap(),
        );
        assert_eq!(
            search.find_matches(vec![7], (0..=16_384).collect()),
            Some(vec![(16_384, 7, 16_384)])
        );
    }

    #[test]
    fn operation_exhaustion_interrupts_candidate_scan() {
        let started = AtomicBool::new(false);
        let calls = AtomicUsize::new(0);
        let remaining = AtomicUsize::new(2048);
        let meter = |operations: usize, _: usize, _: usize| {
            if started.load(Ordering::Relaxed) {
                remaining
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                        left.checked_sub(operations)
                    })
                    .map_err(|_| RSpaceError::HostWorkRejected)?;
            }
            Ok(())
        };
        let context = SpatialMatcherContext::with_meter(&meter).unwrap();
        let mut search = MaximumBipartiteMatch::new(
            Box::new(|_: u16, _: u16| {
                started.store(true, Ordering::Relaxed);
                calls.fetch_add(1, Ordering::Relaxed);
                None::<u16>
            }),
            context.work().unwrap(),
        );
        assert!(search
            .find_matches(vec![7], (0..16_384).collect())
            .is_none());
        assert!(matches!(
            context.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));
        assert!(calls.load(Ordering::Relaxed) > 0);
        assert!(calls.load(Ordering::Relaxed) < 16_384);
    }

    #[test]
    fn reassignment_preserves_order_and_rejects_each_short_dimension() {
        let ordinary = SpatialMatcherContext::new();
        let expected = reassign(ordinary.work().unwrap()).unwrap();
        assert_eq!(expected.len(), 2);
        let used = Mutex::new([0usize; 3]);
        let unlimited = |operations: usize, scanned: usize, backing: usize| {
            let mut totals = used.lock().unwrap();
            for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        let context = SpatialMatcherContext::with_meter(&unlimited).unwrap();
        assert_eq!(reassign(context.work().unwrap()), Some(expected));
        assert!(context.take_error().is_none());
        let required = *used.lock().unwrap();
        assert!(required.iter().all(|value| *value > 0));
        for dimension in 0..3 {
            let mut limit = required;
            limit[dimension] -= 1;
            let spent = Mutex::new([0usize; 3]);
            let meter = |operations: usize, scanned: usize, backing: usize| {
                let mut next = spent.lock().unwrap();
                let amounts = [operations, scanned, backing];
                if next
                    .iter()
                    .zip(amounts)
                    .zip(limit)
                    .any(|((used, add), max)| *used + add > max)
                {
                    return Err(RSpaceError::HostWorkRejected);
                }
                for (used, add) in next.iter_mut().zip(amounts) {
                    *used += add;
                }
                Ok(())
            };
            match SpatialMatcherContext::with_meter(&meter) {
                Ok(context) => {
                    assert!(reassign(context.work().unwrap()).is_none());
                    assert!(matches!(
                        context.take_error(),
                        Some(RSpaceError::HostWorkRejected)
                    ));
                }
                Err(RSpaceError::HostWorkRejected) => {}
                Err(error) => panic!("unexpected matcher error: {error}"),
            }
        }
    }
}
