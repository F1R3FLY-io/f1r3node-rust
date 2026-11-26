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
    pub fn find_matches(&mut self, patterns: &[P], targets: &[T]) -> Option<Vec<(T, P, R)>> {
        let mut search = Search {
            patterns,
            targets,
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

/// `count` interchangeable remainder patterns, each matching exactly the
/// targets `fits` allows, with `result` giving what a match returns at the
/// moment it is made.
pub struct Remainder<'a, P, R> {
    pub pattern: P,
    pub count: usize,
    pub fits: &'a [bool],
    pub result: &'a mut dyn FnMut() -> R,
}

impl<P, T, R> MaximumBipartiteMatch<P, T, R>
where
    P: Debug + Clone,
    T: Debug + Clone + Hash + Eq + Ord,
    R: Debug + Clone,
{
    /// What `find_matches` returns for `remainder.count` copies of
    /// `remainder.pattern` followed by `terms`, without calling the match
    /// function for a remainder. The remainders would take the first targets
    /// that fit them and push each other along on the way, which cost one
    /// pass over the targets per remainder; here a displaced remainder scans
    /// forward once, so placing every term costs one pass each. The match
    /// function sees the same calls in the same order.
    pub fn find_matches_with_remainder(
        &mut self,
        terms: &[P],
        remainder: Remainder<'_, P, R>,
        targets: &[T],
    ) -> Option<Vec<(T, P, R)>> {
        let first = (remainder.result)();
        let mut claims: Vec<Option<(Holder, R)>> = (0..targets.len()).map(|_| None).collect();
        let mut placed = 0;
        for (target, fits) in remainder.fits.iter().enumerate() {
            if placed == remainder.count {
                break;
            }
            if *fits {
                claims[target] = Some((Holder::Remainder, first.clone()));
                placed += 1;
            }
        }
        if placed < remainder.count {
            return None;
        }

        let mut search = RemainderSearch {
            terms,
            targets,
            match_function: &mut self.match_function,
            fits: remainder.fits,
            result: remainder.result,
            claims,
            seen: vec![false; targets.len()],
        };
        for term in 0..terms.len() {
            search.seen.fill(false);
            if !search.place_term(term, 0) {
                return None;
            }
        }

        let mut claimed: Vec<(usize, Holder, R)> = search
            .claims
            .into_iter()
            .enumerate()
            .filter_map(|(target, claim)| claim.map(|(holder, result)| (target, holder, result)))
            .collect();
        claimed.sort_by(|(a, _, _), (b, _, _)| targets[*a].cmp(&targets[*b]).then(a.cmp(b)));
        Some(
            claimed
                .into_iter()
                .map(|(target, holder, result)| {
                    let pattern = match holder {
                        Holder::Term(term) => terms[term].clone(),
                        Holder::Remainder => remainder.pattern.clone(),
                    };
                    (targets[target].clone(), pattern, result)
                })
                .collect(),
        )
    }
}

#[derive(Clone, Copy)]
enum Holder {
    Term(usize),
    Remainder,
}

/// The augmenting-path search with the remainders folded into one kind of
/// holder: a remainder pushed off a target looks for the next target that
/// fits it, which is where every remainder it displaced on the way would
/// have ended up.
struct RemainderSearch<'a, P, T, R> {
    terms: &'a [P],
    targets: &'a [T],
    match_function: &'a mut Box<dyn FnMut(P, T) -> Option<R>>,
    fits: &'a [bool],
    result: &'a mut dyn FnMut() -> R,
    claims: Vec<Option<(Holder, R)>>,
    seen: Vec<bool>,
}

impl<P: Clone, T: Clone, R> RemainderSearch<'_, P, T, R> {
    fn place_term(&mut self, term: usize, from: usize) -> bool {
        for target in from..self.targets.len() {
            if self.seen[target] {
                continue;
            }
            if let Some(result) =
                (self.match_function)(self.terms[term].clone(), self.targets[target].clone())
            {
                self.seen[target] = true;
                return self.claim(target, term, result);
            }
        }
        false
    }

    /// Places a displaced remainder on the first unseen target from `from` on
    /// that fits it. Each target it passes that another remainder holds gets
    /// a fresh result, as that remainder's own move would have given it.
    fn place_remainder(&mut self, from: usize) -> bool {
        let mut passed: Vec<(usize, R)> = Vec::new();
        for target in from..self.targets.len() {
            if self.seen[target] || !self.fits[target] {
                continue;
            }
            self.seen[target] = true;
            let result = (self.result)();
            match self.claims[target] {
                Some((Holder::Remainder, _)) => passed.push((target, result)),
                None => {
                    self.claims[target] = Some((Holder::Remainder, result));
                    for (held, result) in passed {
                        self.claims[held] = Some((Holder::Remainder, result));
                    }
                    return true;
                }
                Some((Holder::Term(term), _)) => {
                    if self.place_term(term, target) {
                        self.claims[target] = Some((Holder::Remainder, result));
                        for (held, result) in passed {
                            self.claims[held] = Some((Holder::Remainder, result));
                        }
                        return true;
                    }
                }
            }
        }
        false
    }

    fn claim(&mut self, target: usize, term: usize, result: R) -> bool {
        let previous = match &self.claims[target] {
            None => {
                self.claims[target] = Some((Holder::Term(term), result));
                return true;
            }
            Some((previous, _)) => *previous,
        };
        let moved = match previous {
            Holder::Term(holder) => self.place_term(holder, target),
            Holder::Remainder => self.place_remainder(target),
        };
        if moved {
            self.claims[target] = Some((Holder::Term(term), result));
            true
        } else {
            self.place_term(term, target)
        }
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
            let ts: Vec<(T, usize)> = targets
                .into_iter()
                .enumerate()
                .map(|(i, t)| (t, i))
                .collect();
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
            let found = search.find_matches(&patterns, &targets);

            assert_eq!(found, expected, "targets {targets:?}");
            assert_eq!(
                *log.borrow(),
                *reference_log.borrow(),
                "targets {targets:?}"
            );
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Slot {
        Term(u8),
        Rest,
    }

    /// Accepts a term as `accepts` says and a remainder wherever the target
    /// fits, answering either with the number of term calls made so far, as
    /// the matcher answers with the bindings found so far. Only term calls
    /// are logged and counted: a remainder match has no effect.
    fn terms_and_rest(
        accepts: Rc<Vec<Vec<bool>>>,
        fits: Rc<Vec<bool>>,
        calls: Rc<std::cell::Cell<u32>>,
        log: Rc<RefCell<Vec<(u8, u8)>>>,
    ) -> Box<dyn FnMut(Slot, u8) -> Option<u32>> {
        Box::new(move |slot, t| match slot {
            Slot::Term(p) => {
                calls.set(calls.get() + 1);
                log.borrow_mut().push((p, t));
                accepts[p as usize][t as usize].then_some(calls.get())
            }
            Slot::Rest => fits[t as usize].then_some(calls.get()),
        })
    }

    #[test]
    fn folding_the_remainders_makes_the_same_term_calls_and_matches() {
        use super::Remainder;

        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        for _ in 0..20_000 {
            let terms_len = rng.next(5) as u8;
            let targets_len = rng.next(12) as usize;
            let rest = rng.next(targets_len as u64 + 1) as usize;
            let density = 1 + rng.next(4);
            let accepts: Rc<Vec<Vec<bool>>> = Rc::new(
                (0..terms_len)
                    .map(|_| (0..16).map(|_| rng.next(density) == 0).collect())
                    .collect(),
            );
            let fit_density = 1 + rng.next(3);
            let fits: Rc<Vec<bool>> =
                Rc::new((0..16).map(|_| rng.next(fit_density) != 0).collect());
            let targets: Vec<u8> = (0..targets_len).map(|_| rng.next(16) as u8).collect();
            let terms: Vec<Slot> = (0..terms_len).map(Slot::Term).collect();

            let expected_calls = Rc::new(std::cell::Cell::new(0));
            let expected_log = Rc::new(RefCell::new(Vec::new()));
            let mut all = vec![Slot::Rest; rest];
            all.extend(terms.iter().cloned());
            let expected = MaximumBipartiteMatch::new(terms_and_rest(
                accepts.clone(),
                fits.clone(),
                expected_calls.clone(),
                expected_log.clone(),
            ))
            .find_matches(&all, &targets);

            let calls = Rc::new(std::cell::Cell::new(0));
            let log = Rc::new(RefCell::new(Vec::new()));
            let target_fits: Vec<bool> = targets.iter().map(|t| fits[*t as usize]).collect();
            let counted = calls.clone();
            let mut result = move || counted.get();
            let found =
                MaximumBipartiteMatch::new(terms_and_rest(accepts, fits, calls, log.clone()))
                    .find_matches_with_remainder(
                        &terms,
                        Remainder {
                            pattern: Slot::Rest,
                            count: rest,
                            fits: &target_fits,
                            result: &mut result,
                        },
                        &targets,
                    );

            assert_eq!(found, expected, "targets {targets:?}, {rest} remainders");
            assert_eq!(
                *log.borrow(),
                *expected_log.borrow(),
                "targets {targets:?}, {rest} remainders"
            );
        }
    }
}
