use super::lazy_relation::{EdgeResult, LazyRelation, RelationCursor, RelationalMatchStats};

pub type MaximumBipartiteMatchStats = RelationalMatchStats;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/MaximumBipartiteMatch.scala
pub struct MaximumBipartiteMatch<P, T, R>
where
    P: Clone,
    T: Clone + Ord,
    R: Clone,
{
    match_function: Box<dyn for<'pattern, 'target> FnMut(&'pattern P, &'target T) -> Option<R>>,
    cache_pattern: Box<dyn Fn(&P) -> bool>,
    stats: MaximumBipartiteMatchStats,
}

type Candidate<T> = Indexed<T>;

#[derive(Debug, Clone, Eq, Hash, PartialEq, Ord, PartialOrd)]
struct Indexed<A> {
    value: A,
    index: usize,
}

struct Assignment<R> {
    pattern_index: usize,
    result: EdgeResult<R>,
}

struct SearchFrame<R> {
    pattern_index: usize,
    next_target: usize,
    relation_cursor: RelationCursor,
    /// The edge by which this frame displaced its pattern's current
    /// assignment. It is committed only after the child finds an alternative.
    pending: Option<(usize, EdgeResult<R>)>,
}

impl<P, T, R> MaximumBipartiteMatch<P, T, R>
where
    P: Clone,
    T: Clone + Ord,
    R: Clone,
{
    /// Construct the compatibility matcher without relation retention.
    ///
    /// The callback is accepted as `FnMut`, so callers that have not proved it
    /// pure retain the historical call order and call count. Use
    /// [`new_with_cache_policy`](Self::new_with_cache_policy) to opt proven-pure
    /// pattern rows into lazy relational reuse.
    pub fn new(
        match_function: Box<dyn for<'pattern, 'target> FnMut(&'pattern P, &'target T) -> Option<R>>,
    ) -> Self {
        Self::new_with_cache_policy(match_function, Box::new(|_| false))
    }

    /// Construct a matcher with a per-pattern relation-retention policy.
    ///
    /// A non-cacheable row follows the same iterative Kuhn traversal but does
    /// not retain edge results. This is useful for cheap, intentionally dense
    /// pattern families such as AC remainder fillers; expensive structural
    /// rows still receive exact lazy relation reuse. Returning `true` for a
    /// pattern is a semantic assertion that `match_function(pattern, target)`
    /// is pure for every target: the result may be evaluated once and replayed
    /// on later augmenting paths. Impure rows must return `false`.
    pub fn new_with_cache_policy(
        match_function: Box<dyn for<'pattern, 'target> FnMut(&'pattern P, &'target T) -> Option<R>>,
        cache_pattern: Box<dyn Fn(&P) -> bool>,
    ) -> Self {
        MaximumBipartiteMatch {
            match_function,
            cache_pattern,
            stats: MaximumBipartiteMatchStats::default(),
        }
    }

    /// Work counters from the most recent `find_matches` call.
    pub fn stats(&self) -> MaximumBipartiteMatchStats { self.stats }

    pub fn find_matches(&mut self, patterns: Vec<P>, targets: Vec<T>) -> Option<Vec<(T, P, R)>> {
        self.find_indexed_matches(patterns, targets).map(|matches| {
            matches
                .into_iter()
                .map(|(_, target, pattern, result)| (target, pattern, result))
                .collect()
        })
    }

    /// Return the same deterministic matching as [`find_matches`](Self::find_matches),
    /// retaining each target's original position.
    ///
    /// Target identity is positional, not merely structural: an AC subject may contain
    /// byte-identical siblings, and a remainder that selects one of them must not select
    /// every equal sibling.  The ordinary compatibility surface drops the index, while
    /// remainder reconstruction consumes it to preserve exact multiplicity in linear time.
    pub(crate) fn find_indexed_matches(
        &mut self,
        patterns: Vec<P>,
        targets: Vec<T>,
    ) -> Option<Vec<(usize, T, P, R)>> {
        self.stats = MaximumBipartiteMatchStats::default();
        let mut assignments: Vec<Option<Assignment<R>>> =
            (0..targets.len()).map(|_| None).collect();
        let mut relation =
            LazyRelation::new(patterns.iter().map(|pattern| (self.cache_pattern)(pattern)));

        for pattern_index in 0..patterns.len() {
            if !self.augment(
                pattern_index,
                &patterns,
                &targets,
                &mut assignments,
                &mut relation,
            ) {
                return None;
            }
        }

        relation.clear();
        let mut ordered: Vec<(Candidate<T>, usize, R)> = assignments
            .into_iter()
            .enumerate()
            .filter_map(|(target_index, assignment)| {
                assignment.map(|assignment| {
                    (
                        Indexed {
                            value: targets[target_index].clone(),
                            index: target_index,
                        },
                        assignment.pattern_index,
                        assignment.result.into_owned(),
                    )
                })
            })
            .collect();
        // Preserve the old BTreeMap<Candidate<T>, ...> observation order:
        // target value first, original target index as the duplicate tie-break.
        ordered.sort_by(|left, right| left.0.cmp(&right.0));
        Some(
            ordered
                .into_iter()
                .map(|(target, pattern_index, result)| {
                    (
                        target.index,
                        target.value,
                        patterns[pattern_index].clone(),
                        result,
                    )
                })
                .collect(),
        )
    }

    /// Iterative Kuhn augmenting-path search.
    ///
    /// The former implementation represented recursion by cloning the
    /// remaining candidate slice into every `Pattern` frame. At width `T` that
    /// retained Θ(P*T) candidates before useful matching state. Here one frame
    /// stores only `(pattern index, next target, pending edge)`; the shared
    /// target and pattern arrays are never copied.
    fn augment(
        &mut self,
        root_pattern: usize,
        patterns: &[P],
        targets: &[T],
        assignments: &mut [Option<Assignment<R>>],
        relation: &mut LazyRelation<R>,
    ) -> bool {
        let mut seen_targets = vec![false; targets.len()];
        if relation.is_cacheable(root_pattern) && relation.has_scanned_targets(root_pattern) {
            self.stats.relation_row_reuses += 1;
        }
        self.stats.augmenting_frames += 1;
        let mut stack = vec![SearchFrame {
            pattern_index: root_pattern,
            next_target: 0,
            relation_cursor: relation.cursor(root_pattern),
            pending: None,
        }];

        loop {
            let Some(frame) = stack.last_mut() else {
                return false;
            };

            if let Some((target_index, result)) =
                self.next_edge(frame, patterns, targets, relation, &seen_targets)
            {
                seen_targets[target_index] = true;

                match assignments[target_index].as_ref() {
                    None => {
                        assignments[target_index] = Some(Assignment {
                            pattern_index: frame.pattern_index,
                            result,
                        });

                        // The leaf has an alternative. Commit every displaced
                        // parent edge while unwinding the explicit PDA stack.
                        stack.pop();
                        while let Some(mut parent) = stack.pop() {
                            let (parent_target, parent_result) = parent
                                .pending
                                .take()
                                .expect("an augmenting parent has one displaced edge");
                            assignments[parent_target] = Some(Assignment {
                                pattern_index: parent.pattern_index,
                                result: parent_result,
                            });
                        }
                        return true;
                    }
                    Some(previous) => {
                        let previous_pattern = previous.pattern_index;
                        frame.pending = Some((target_index, result));
                        if relation.is_cacheable(previous_pattern)
                            && relation.has_scanned_targets(previous_pattern)
                        {
                            self.stats.relation_row_reuses += 1;
                        }
                        self.stats.augmenting_frames += 1;
                        stack.push(SearchFrame {
                            pattern_index: previous_pattern,
                            next_target: 0,
                            relation_cursor: relation.cursor(previous_pattern),
                            pending: None,
                        });
                        continue;
                    }
                }
            }

            // This displaced pattern has no alternative. Its parent keeps the
            // old assignment and resumes after the failed candidate.
            stack.pop();
            if let Some(parent) = stack.last_mut() {
                parent.pending = None;
            } else {
                return false;
            }
        }
    }

    /// Return the next successful, as-yet-unseen edge for `frame`.
    ///
    /// Successful edges from the row snapshot are replayed first. Once they
    /// are exhausted, only the previously unseen target suffix is evaluated.
    /// Thus every pattern-target pair reaches `match_function` at most once.
    fn next_edge(
        &mut self,
        frame: &mut SearchFrame<R>,
        patterns: &[P],
        targets: &[T],
        relation: &mut LazyRelation<R>,
        seen_targets: &[bool],
    ) -> Option<(usize, EdgeResult<R>)> {
        if !relation.is_cacheable(frame.pattern_index) {
            while frame.next_target < targets.len() {
                let target_index = frame.next_target;
                frame.next_target += 1;
                if seen_targets[target_index] {
                    continue;
                }
                self.stats.edge_evaluations += 1;
                if let Some(result) =
                    (self.match_function)(&patterns[frame.pattern_index], &targets[target_index])
                {
                    self.stats.successful_edge_evaluations += 1;
                    return Some((target_index, EdgeResult::Owned(result)));
                }
            }
            return None;
        }

        while let Some((target_index, result)) =
            relation.next_cached(frame.pattern_index, &mut frame.relation_cursor)
        {
            self.stats.cached_edge_visits += 1;
            if !seen_targets[target_index] {
                return Some((target_index, EdgeResult::Shared(result)));
            }
        }

        while let Some(target_index) = relation.next_unscanned(frame.pattern_index, targets.len()) {
            self.stats.edge_evaluations += 1;
            let Some(result) =
                (self.match_function)(&patterns[frame.pattern_index], &targets[target_index])
            else {
                continue;
            };
            self.stats.successful_edge_evaluations += 1;
            let result = relation.record_success(frame.pattern_index, target_index, result);
            if !seen_targets[target_index] {
                return Some((target_index, EdgeResult::Shared(result)));
            }
        }

        None
    }
}
