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

type Candidate<T> = Indexed<T>;

#[derive(Debug, Clone, Eq, Hash, PartialEq, Ord, PartialOrd)]
struct Indexed<A> {
    value: A,
    index: usize,
}

struct Assignment<R> {
    pattern_index: usize,
    result: R,
}

struct SearchFrame<R> {
    pattern_index: usize,
    next_target: usize,
    /// The edge by which this frame displaced its pattern's current
    /// assignment. It is committed only after the child finds an alternative.
    pending: Option<(usize, R)>,
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

    pub fn find_matches(&mut self, patterns: Vec<P>, targets: Vec<T>) -> Option<Vec<(T, P, R)>> {
        let mut assignments: Vec<Option<Assignment<R>>> =
            (0..targets.len()).map(|_| None).collect();

        for pattern_index in 0..patterns.len() {
            if !self.augment(pattern_index, &patterns, &targets, &mut assignments) {
                return None;
            }
        }

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
                        assignment.result,
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
                    (target.value, patterns[pattern_index].clone(), result)
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
    ) -> bool {
        let mut seen_targets = vec![false; targets.len()];
        let mut stack = vec![SearchFrame {
            pattern_index: root_pattern,
            next_target: 0,
            pending: None,
        }];

        loop {
            let Some(frame) = stack.last_mut() else {
                return false;
            };
            let mut descended = false;

            while frame.next_target < targets.len() {
                let target_index = frame.next_target;
                frame.next_target += 1;
                if seen_targets[target_index] {
                    continue;
                }

                let Some(result) = (self.match_function)(
                    patterns[frame.pattern_index].clone(),
                    targets[target_index].clone(),
                ) else {
                    continue;
                };
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
                        stack.push(SearchFrame {
                            pattern_index: previous_pattern,
                            next_target: 0,
                            pending: None,
                        });
                        descended = true;
                        break;
                    }
                }
            }

            if descended {
                continue;
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
}
