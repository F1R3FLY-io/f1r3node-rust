use std::cmp::Eq;
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
}

impl<P, T, R> MaximumBipartiteMatch<P, T, R>
where
    P: Debug + Clone,
    T: Debug + Clone + Hash + Eq + Ord,
    R: Debug + Clone,
{
    pub fn new(match_function: Box<dyn FnMut(P, T) -> Option<R>>) -> Self {
        MaximumBipartiteMatch { match_function }
    }

    /// Pairs every pattern with a distinct target it matches, or `None` when
    /// no such pairing exists. Patterns are placed in order, each taking the
    /// first unseen target it matches and displacing an earlier pattern onto
    /// another target when that one is taken. The pairs are ordered by target,
    /// then by the target's position.
    pub fn find_matches(&mut self, patterns: Vec<P>, targets: Vec<T>) -> Option<Vec<(T, P, R)>> {
        let mut search = Search {
            patterns: &patterns,
            targets: &targets,
            match_function: &mut self.match_function,
            claims: (0..targets.len()).map(|_| None).collect(),
            seen: vec![false; targets.len()],
        };
        for pattern in 0..patterns.len() {
            search.seen.fill(false);
            if !search.find_match(pattern, 0) {
                return None;
            }
        }

        let mut claimed: Vec<(usize, usize, R)> = search
            .claims
            .into_iter()
            .enumerate()
            .filter_map(|(target, claim)| claim.map(|(pattern, result)| (target, pattern, result)))
            .collect();
        claimed.sort_by(|(a, _, _), (b, _, _)| targets[*a].cmp(&targets[*b]).then(a.cmp(b)));
        Some(
            claimed
                .into_iter()
                .map(|(target, pattern, result)| {
                    (targets[target].clone(), patterns[pattern].clone(), result)
                })
                .collect(),
        )
    }
}

/// One run of the augmenting-path search over borrowed patterns and targets.
/// `claims[t]` is the pattern holding target `t` and what matching it gave;
/// `seen[t]` marks the targets tried while placing the current pattern.
struct Search<'a, P, T, R> {
    patterns: &'a [P],
    targets: &'a [T],
    match_function: &'a mut Box<dyn FnMut(P, T) -> Option<R>>,
    claims: Vec<Option<(usize, R)>>,
    seen: Vec<bool>,
}

impl<P: Clone, T: Clone, R> Search<'_, P, T, R> {
    /// Places `pattern` on the first unseen target from `from` on that it
    /// matches, displacing that target's holder if it can move elsewhere.
    fn find_match(&mut self, pattern: usize, from: usize) -> bool {
        for target in from..self.targets.len() {
            if self.seen[target] {
                continue;
            }
            if let Some(result) =
                (self.match_function)(self.patterns[pattern].clone(), self.targets[target].clone())
            {
                self.seen[target] = true;
                return self.try_claim(target, pattern, result);
            }
        }
        false
    }

    fn try_claim(&mut self, target: usize, pattern: usize, result: R) -> bool {
        let holder = match &self.claims[target] {
            None => {
                self.claims[target] = Some((pattern, result));
                return true;
            }
            Some((holder, _)) => *holder,
        };
        if self.find_match(holder, target) {
            self.claims[target] = Some((pattern, result));
            true
        } else {
            self.find_match(pattern, target)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::rc::Rc;

    use super::MaximumBipartiteMatch;

    /// The algorithm as first ported, which copied each pattern's candidate
    /// list at every step: the reference the index-based search must agree
    /// with call for call.
    struct Reference<P, T: Ord, R> {
        match_function: Box<dyn FnMut(P, T) -> Option<R>>,
        matches: BTreeMap<(T, usize), ((P, Vec<(T, usize)>), R)>,
        seen: BTreeSet<(T, usize)>,
    }

    impl<P: Clone, T: Clone + Ord, R: Clone> Reference<P, T, R> {
        fn find_matches(&mut self, patterns: Vec<P>, targets: Vec<T>) -> Option<Vec<(T, P, R)>> {
            let ts: Vec<(T, usize)> = targets.into_iter().enumerate().map(|(i, t)| (t, i)).collect();
            for p in patterns {
                self.seen.clear();
                if !self.find_match((p, ts.clone())) {
                    return None;
                }
            }
            Some(
                self.matches
                    .iter()
                    .map(|(t, (p, r))| (t.0.clone(), p.0.clone(), r.clone()))
                    .collect(),
            )
        }

        fn find_match(&mut self, pattern: (P, Vec<(T, usize)>)) -> bool {
            let (p, candidates) = pattern.clone();
            let Some((candidate, rest)) = candidates.split_first() else {
                return false;
            };
            if self.seen.contains(candidate) {
                return self.find_match((p, rest.to_vec()));
            }
            match (self.match_function)(p.clone(), candidate.0.clone()) {
                Some(result) => {
                    self.seen.insert(candidate.clone());
                    match self.matches.get(candidate).map(|m| m.0.clone()) {
                        None => {
                            self.matches.insert(candidate.clone(), (pattern, result));
                            true
                        }
                        Some(previous) => {
                            if self.find_match(previous) {
                                self.matches.insert(candidate.clone(), (pattern, result));
                                true
                            } else {
                                self.find_match(pattern)
                            }
                        }
                    }
                }
                None => self.find_match((p, rest.to_vec())),
            }
        }
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self, bound: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % bound
        }
    }

    /// A match function whose answers depend on its own history, as the
    /// matcher's does through its accumulated free map, and which logs every
    /// call so the two searches can be compared call for call.
    fn stateful(
        accepts: Rc<Vec<Vec<bool>>>,
        log: Rc<RefCell<Vec<(u8, u8)>>>,
    ) -> Box<dyn FnMut(u8, u8) -> Option<u32>> {
        let mut calls = 0u32;
        Box::new(move |p, t| {
            calls += 1;
            log.borrow_mut().push((p, t));
            accepts[p as usize][t as usize].then_some(calls)
        })
    }

    #[test]
    fn the_index_search_makes_the_reference_searchs_calls_and_matches() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..5000 {
            let patterns_len = rng.next(7) as u8;
            let targets_len = rng.next(9) as u8;
            let density = 1 + rng.next(4);
            let accepts: Rc<Vec<Vec<bool>>> = Rc::new(
                (0..patterns_len)
                    .map(|_| (0..16).map(|_| rng.next(density) == 0).collect())
                    .collect(),
            );
            let patterns: Vec<u8> = (0..patterns_len).collect();
            let targets: Vec<u8> = (0..targets_len).map(|_| rng.next(16) as u8).collect();

            let reference_log = Rc::new(RefCell::new(Vec::new()));
            let mut reference = Reference {
                match_function: stateful(accepts.clone(), reference_log.clone()),
                matches: BTreeMap::new(),
                seen: BTreeSet::new(),
            };
            let expected = reference.find_matches(patterns.clone(), targets.clone());

            let log = Rc::new(RefCell::new(Vec::new()));
            let mut search = MaximumBipartiteMatch::new(stateful(accepts.clone(), log.clone()));
            let found = search.find_matches(patterns, targets.clone());

            assert_eq!(found, expected, "targets {targets:?}");
            assert_eq!(*log.borrow(), *reference_log.borrow(), "targets {targets:?}");
        }
    }
}
