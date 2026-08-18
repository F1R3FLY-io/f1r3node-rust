use itertools::Itertools;
use models::rhoapi::{Bundle, Expr, GUnforgeable, Match, New, Par, Receive, Send};

use super::par_count::ParCount;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParSpatialMatcherUtils.scala - subPars
pub fn sub_pars(
    par: &Par,
    min: &ParCount,
    max: &ParCount,
    min_prune: &ParCount,
    max_prune: &ParCount,
) -> impl Iterator<Item = (Par, Par)> {
    fn feasible_range(
        available: usize,
        minimum: u64,
        maximum: u64,
        minimum_prune: u64,
        maximum_prune: u64,
    ) -> (usize, usize) {
        let available_u64 = u64::try_from(available)
            .expect("a resident repeated-field length fits the u64 multiplicity domain");
        let Some(after_required_prune) = available_u64.checked_sub(minimum_prune) else {
            return (1, 0);
        };
        let upper = maximum.min(after_required_prune);
        let lower = minimum.max(available_u64.saturating_sub(maximum_prune));
        if lower > upper {
            return (1, 0);
        }
        (
            usize::try_from(lower).expect("lower subset bound is at most the family length"),
            usize::try_from(upper).expect("upper subset bound is at most the family length"),
        )
    }

    let (send_min, send_max) = feasible_range(
        usize::try_from(par.family_cardinality::<Send>())
            .expect("a resident Send family fits usize"),
        min.sends,
        max.sends,
        min_prune.sends,
        max_prune.sends,
    );
    let (receive_min, receive_max) = feasible_range(
        usize::try_from(par.family_cardinality::<Receive>())
            .expect("a resident Receive family fits usize"),
        min.receives,
        max.receives,
        min_prune.receives,
        max_prune.receives,
    );
    let (new_min, new_max) = feasible_range(
        usize::try_from(par.family_cardinality::<New>()).expect("a resident New family fits usize"),
        min.news,
        max.news,
        min_prune.news,
        max_prune.news,
    );
    let (expr_min, expr_max) = feasible_range(
        usize::try_from(par.family_cardinality::<Expr>())
            .expect("a resident Expr family fits usize"),
        min.exprs,
        max.exprs,
        min_prune.exprs,
        max_prune.exprs,
    );
    let (match_min, match_max) = feasible_range(
        usize::try_from(par.family_cardinality::<Match>())
            .expect("a resident Match family fits usize"),
        min.matches,
        max.matches,
        min_prune.matches,
        max_prune.matches,
    );
    let (unf_min, unf_max) = feasible_range(
        usize::try_from(par.family_cardinality::<GUnforgeable>())
            .expect("a resident unforgeable family fits usize"),
        min.unforgeables,
        max.unforgeables,
        min_prune.unforgeables,
        max_prune.unforgeables,
    );
    let (bundle_min, bundle_max) = feasible_range(
        usize::try_from(par.family_cardinality::<Bundle>())
            .expect("a resident Bundle family fits usize"),
        min.bundles,
        max.bundles,
        min_prune.bundles,
        max_prune.bundles,
    );

    // This ideally should return type 'Iterator' instead of type 'Vec'
    fn min_max_subsets<A: Clone + std::fmt::Debug>(
        _as: &Vec<A>,
        min_size: usize,
        max_size: usize,
    ) -> Vec<(Vec<A>, Vec<A>)> {
        type Counted<A> = (Vec<A>, Vec<A>, usize);

        /// Bottom-up form of `counted_max_subsets`. Processing suffixes from
        /// right to left emits the same result order as the recursive
        /// definition without retaining one native frame per element.
        fn counted_max_subsets<A: Clone>(_as: &[A], max_size: usize) -> Vec<Counted<A>> {
            let mut child = vec![(Vec::new(), Vec::new(), 0)];
            for start in (0.._as.len()).rev() {
                let head = &_as[start];
                let mut results = vec![(Vec::new(), _as[start..].to_vec(), 0)];
                for (mut tail, mut complement, count) in child {
                    if count == max_size {
                        complement.insert(0, head.clone());
                        results.push((tail, complement, count));
                    } else if tail.is_empty() {
                        tail.insert(0, head.clone());
                        results.push((tail, complement, 1));
                    } else {
                        complement.insert(0, head.clone());
                        tail.insert(0, head.clone());
                        results.push((tail.clone(), complement.clone(), count));
                        results.push((tail, complement, count + 1));
                    }
                }
                child = results;
            }
            child
        }

        enum SubsetWork {
            Call {
                start: usize,
                min_size: usize,
                max_size: usize,
            },
            Combine {
                head: usize,
                decr: usize,
                min_size: usize,
                max_size: usize,
            },
        }

        // Explicit call/return machine for `worker`. A value-stack entry is
        // exactly the vector one recursive invocation formerly returned.
        let mut work = vec![SubsetWork::Call {
            start: 0,
            min_size,
            max_size,
        }];
        let mut values: Vec<Vec<Counted<A>>> = Vec::new();

        while let Some(task) = work.pop() {
            match task {
                SubsetWork::Call {
                    start: _,
                    min_size,
                    max_size,
                } if min_size > max_size => values.push(Vec::new()),
                SubsetWork::Call {
                    start,
                    min_size: 0,
                    max_size,
                } => {
                    if max_size == 0 {
                        values.push(vec![(Vec::new(), _as[start..].to_vec(), 0)]);
                    } else {
                        values.push(counted_max_subsets(&_as[start..], max_size));
                    }
                }
                SubsetWork::Call { start, .. } if start == _as.len() => {
                    values.push(Vec::new());
                }
                SubsetWork::Call {
                    start,
                    min_size,
                    max_size,
                } => {
                    let decr = min_size - 1;
                    work.push(SubsetWork::Combine {
                        head: start,
                        decr,
                        min_size,
                        max_size,
                    });
                    work.push(SubsetWork::Call {
                        start: start + 1,
                        min_size: decr,
                        max_size,
                    });
                }
                SubsetWork::Combine {
                    head,
                    decr,
                    min_size,
                    max_size,
                } => {
                    let counted_tail = values
                        .pop()
                        .expect("the subset PDA reduces one child result");
                    let mut results = Vec::new();
                    for (mut tail, mut complement, count) in counted_tail {
                        if count == max_size {
                            complement.insert(0, _as[head].clone());
                            results.push((tail, complement, count));
                        } else if count == decr {
                            tail.insert(0, _as[head].clone());
                            results.push((tail, complement, min_size));
                        } else {
                            complement.insert(0, _as[head].clone());
                            tail.insert(0, _as[head].clone());
                            results.push((tail.clone(), complement.clone(), count));
                            results.push((tail, complement, count + 1));
                        }
                    }
                    values.push(results);
                }
            }
        }

        let result = values.pop().expect("the subset PDA emits one root result");
        debug_assert!(values.is_empty(), "the subset PDA leaves no sibling values");
        result
            .into_iter()
            .map(|(selected, complement, _)| (selected, complement))
            .collect()
    }

    let sends = par.decode_occurrences::<Send>();
    let receives = par.decode_occurrences::<Receive>();
    let news = par.decode_occurrences::<New>();
    let exprs = par.decode_occurrences::<Expr>();
    let matches = par.decode_occurrences::<Match>();
    let unforgeables = par.decode_occurrences::<GUnforgeable>();
    let bundles = par.decode_occurrences::<Bundle>();

    min_max_subsets(&sends, send_min, send_max)
        .into_iter()
        .cartesian_product(min_max_subsets(&receives, receive_min, receive_max).into_iter())
        .cartesian_product(min_max_subsets(&news, new_min, new_max).into_iter())
        .cartesian_product(min_max_subsets(&exprs, expr_min, expr_max).into_iter())
        .cartesian_product(min_max_subsets(&matches, match_min, match_max).into_iter())
        .cartesian_product(min_max_subsets(&unforgeables, unf_min, unf_max).into_iter())
        .cartesian_product(min_max_subsets(&bundles, bundle_min, bundle_max).into_iter())
        .map(
            |(
                (((((sub_sends, sub_receives), sub_news), sub_exprs), sub_matches), sub_unfs),
                sub_bundles,
            )| {
                let build = |sends, receives, news, exprs, matches, unforgeables, bundles| {
                    let mut par = Par::default();
                    par.replace_sends(sends);
                    par.replace_receives(receives);
                    par.replace_news(news);
                    par.replace_exprs(exprs);
                    par.replace_matches(matches);
                    par.replace_unforgeables(unforgeables);
                    par.replace_bundles(bundles);
                    par
                };
                (
                    build(
                        sub_sends.0,
                        sub_receives.0,
                        sub_news.0,
                        sub_exprs.0,
                        sub_matches.0,
                        sub_unfs.0,
                        sub_bundles.0,
                    ),
                    build(
                        sub_sends.1,
                        sub_receives.1,
                        sub_news.1,
                        sub_exprs.1,
                        sub_matches.1,
                        sub_unfs.1,
                        sub_bundles.1,
                    ),
                )
            },
        )
}
