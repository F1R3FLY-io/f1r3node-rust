use std::mem::size_of;

use rspace_plus_plus::rspace::errors::RSpaceError;
use shared::rust::clone_backing::CloneBacking;

use super::exports::{
    Bundle, CostSignedTerm, CostStack, Expr, GUnforgeable, If, Match, New, Par, Receive, Send,
};
use super::par_count::ParCount;
use super::spatial_matcher::MatcherWork;

fn clone_slice<A: Clone + CloneBacking>(values: &[A], work: &MatcherWork<'_>) -> Option<Vec<A>> {
    // Changed by D-O1 (DR-109): block accounting charges inline bytes
    // once per enclosing block.
    // work.reserve_slice(values)?;
    work.reserve_blocks_slice_copy_and_cleanup(values)?;
    Some(values.to_vec())
}

fn push<T>(values: &mut Vec<T>, value: T, work: &MatcherWork<'_>) -> Option<()> {
    work.reserve_vec(values, 1)?;
    values.push(value);
    Some(())
}

fn insert_head<A: Clone + CloneBacking>(
    values: &mut Vec<A>,
    head: &A,
    work: &MatcherWork<'_>,
) -> Option<()> {
    let scanned = values
        .len()
        .checked_mul(size_of::<A>())
        .or_else(|| work.reject(RSpaceError::HostWorkRejected))?;
    work.reserve(1, scanned, 0)?;
    // Changed by D-O1 (DR-109): block accounting charges inline bytes
    // once per enclosing block.
    // work.reserve_clone(head)?;
    work.reserve_blocks_copy_and_cleanup(head)?;
    work.reserve_vec(values, 1)?;
    values.insert(0, head.clone());
    Some(())
}

fn counted_max_subsets<A: Clone + CloneBacking>(
    values: &[A],
    max_size: isize,
    work: &MatcherWork<'_>,
) -> Option<Vec<(Vec<A>, Vec<A>, isize)>> {
    work.reserve(1, 0, 0)?;
    let mut results = Vec::new();
    push(&mut results, (Vec::new(), Vec::new(), 0), work)?;
    for index in (0..values.len()).rev() {
        work.reserve(1, 0, 0)?;
        let head = &values[index];
        let mut next = Vec::new();
        push(
            &mut next,
            (Vec::new(), clone_slice(&values[index..], work)?, 0),
            work,
        )?;
        for (mut tail, mut complement, count) in results {
            if count == max_size {
                insert_head(&mut complement, head, work)?;
                push(&mut next, (tail, complement, count), work)?;
            } else if tail.is_empty() {
                insert_head(&mut tail, head, work)?;
                push(&mut next, (tail, complement, 1), work)?;
            } else {
                insert_head(&mut complement, head, work)?;
                insert_head(&mut tail, head, work)?;
                let tail_copy = clone_slice(&tail, work)?;
                let complement_copy = clone_slice(&complement, work)?;
                push(&mut next, (tail_copy, complement_copy, count), work)?;
                push(&mut next, (tail, complement, count + 1), work)?;
            }
        }
        results = next;
    }
    Some(results)
}

fn subset_worker<A: Clone + CloneBacking>(
    values: &[A],
    min_size: isize,
    max_size: isize,
    work: &MatcherWork<'_>,
) -> Option<Vec<(Vec<A>, Vec<A>, isize)>> {
    work.reserve(1, 0, 0)?;
    if max_size < 0 || min_size > max_size {
        return Some(Vec::new());
    }
    if min_size <= 0 {
        if max_size == 0 {
            let mut result = Vec::new();
            push(
                &mut result,
                (Vec::new(), clone_slice(values, work)?, 0),
                work,
            )?;
            return Some(result);
        }
        return counted_max_subsets(values, max_size, work);
    }

    let Ok(depth) = usize::try_from(min_size) else {
        return work.reject(RSpaceError::HostWorkRejected);
    };
    if depth > values.len() {
        return Some(Vec::new());
    }
    let mut results = counted_max_subsets(&values[depth..], max_size, work)?;
    for index in (0..depth).rev() {
        work.reserve(1, 0, 0)?;
        let head = &values[index];
        let minimum = min_size - index as isize;
        let decr = minimum - 1;
        let mut next = Vec::new();
        for (mut tail, mut complement, count) in results {
            if count == max_size {
                insert_head(&mut complement, head, work)?;
                push(&mut next, (tail, complement, count), work)?;
            } else if count == decr {
                insert_head(&mut tail, head, work)?;
                push(&mut next, (tail, complement, minimum), work)?;
            } else {
                insert_head(&mut complement, head, work)?;
                insert_head(&mut tail, head, work)?;
                let tail_copy = clone_slice(&tail, work)?;
                let complement_copy = clone_slice(&complement, work)?;
                push(&mut next, (tail_copy, complement_copy, count), work)?;
                push(&mut next, (tail, complement, count + 1), work)?;
            }
        }
        results = next;
    }
    Some(results)
}

fn min_max_subsets<A: Clone + CloneBacking>(
    values: &[A],
    min_size: isize,
    max_size: isize,
    work: &MatcherWork<'_>,
) -> Option<Vec<(Vec<A>, Vec<A>)>> {
    let counted = subset_worker(values, min_size, max_size, work)?;
    let mut subsets = Vec::new();
    for (selected, remainder, _) in counted {
        push(&mut subsets, (selected, remainder), work)?;
    }
    Some(subsets)
}

fn clone_pair<A: Clone + CloneBacking>(
    pair: &(Vec<A>, Vec<A>),
    work: &MatcherWork<'_>,
) -> Option<(Vec<A>, Vec<A>)> {
    Some((clone_slice(&pair.0, work)?, clone_slice(&pair.1, work)?))
}

struct SubPars<'a> {
    sends: Vec<(Vec<Send>, Vec<Send>)>,
    receives: Vec<(Vec<Receive>, Vec<Receive>)>,
    news: Vec<(Vec<New>, Vec<New>)>,
    exprs: Vec<(Vec<Expr>, Vec<Expr>)>,
    matches: Vec<(Vec<Match>, Vec<Match>)>,
    unforgeables: Vec<(Vec<GUnforgeable>, Vec<GUnforgeable>)>,
    bundles: Vec<(Vec<Bundle>, Vec<Bundle>)>,
    conditionals: Vec<(Vec<If>, Vec<If>)>,
    cost_signed_terms: Vec<(Vec<CostSignedTerm>, Vec<CostSignedTerm>)>,
    cost_stacks: Vec<(Vec<CostStack>, Vec<CostStack>)>,
    positions: [usize; 10],
    exhausted: bool,
    work: MatcherWork<'a>,
}

impl SubPars<'_> {
    fn length(&self, field: usize) -> usize {
        match field {
            0 => self.sends.len(),
            1 => self.receives.len(),
            2 => self.news.len(),
            3 => self.exprs.len(),
            4 => self.matches.len(),
            5 => self.unforgeables.len(),
            6 => self.bundles.len(),
            7 => self.conditionals.len(),
            8 => self.cost_signed_terms.len(),
            9 => self.cost_stacks.len(),
            _ => unreachable!(),
        }
    }

    fn advance(&mut self) {
        for field in (0..10).rev() {
            self.positions[field] += 1;
            if self.positions[field] < self.length(field) {
                return;
            }
            self.positions[field] = 0;
        }
        self.exhausted = true;
    }
}

impl Iterator for SubPars<'_> {
    type Item = Option<(Par, Par)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted {
            return None;
        }
        let candidate = (|| {
            self.work.reserve(1, 10 * size_of::<usize>(), 0)?;
            let sends = clone_pair(&self.sends[self.positions[0]], &self.work)?;
            let receives = clone_pair(&self.receives[self.positions[1]], &self.work)?;
            let news = clone_pair(&self.news[self.positions[2]], &self.work)?;
            let exprs = clone_pair(&self.exprs[self.positions[3]], &self.work)?;
            let matches = clone_pair(&self.matches[self.positions[4]], &self.work)?;
            let unforgeables = clone_pair(&self.unforgeables[self.positions[5]], &self.work)?;
            let bundles = clone_pair(&self.bundles[self.positions[6]], &self.work)?;
            let conditionals = clone_pair(&self.conditionals[self.positions[7]], &self.work)?;
            let cost_signed_terms =
                clone_pair(&self.cost_signed_terms[self.positions[8]], &self.work)?;
            let cost_stacks = clone_pair(&self.cost_stacks[self.positions[9]], &self.work)?;
            Some((
                Par {
                    sends: sends.0,
                    receives: receives.0,
                    news: news.0,
                    exprs: exprs.0,
                    matches: matches.0,
                    unforgeables: unforgeables.0,
                    bundles: bundles.0,
                    connectives: Vec::new(),
                    conditionals: conditionals.0,
                    locally_free: Vec::new(),
                    connective_used: false,
                    cost_signed_terms: cost_signed_terms.0,
                    cost_stacks: cost_stacks.0,
                },
                Par {
                    sends: sends.1,
                    receives: receives.1,
                    news: news.1,
                    exprs: exprs.1,
                    matches: matches.1,
                    unforgeables: unforgeables.1,
                    bundles: bundles.1,
                    connectives: Vec::new(),
                    conditionals: conditionals.1,
                    locally_free: Vec::new(),
                    connective_used: false,
                    cost_signed_terms: cost_signed_terms.1,
                    cost_stacks: cost_stacks.1,
                },
            ))
        })();
        if candidate.is_some() {
            self.advance();
        } else {
            self.exhausted = true;
        }
        Some(candidate)
    }
}

pub(super) fn sub_pars<'a>(
    par: &Par,
    min: &ParCount,
    max: &ParCount,
    min_prune: &ParCount,
    max_prune: &ParCount,
    work: MatcherWork<'a>,
) -> Option<impl Iterator<Item = Option<(Par, Par)>> + 'a> {
    work.reserve(1, 10 * size_of::<usize>(), 0)?;
    let bounds = [
        (
            par.sends.len(),
            min.sends,
            max.sends,
            min_prune.sends,
            max_prune.sends,
        ),
        (
            par.receives.len(),
            min.receives,
            max.receives,
            min_prune.receives,
            max_prune.receives,
        ),
        (
            par.news.len(),
            min.news,
            max.news,
            min_prune.news,
            max_prune.news,
        ),
        (
            par.exprs.len(),
            min.exprs,
            max.exprs,
            min_prune.exprs,
            max_prune.exprs,
        ),
        (
            par.matches.len(),
            min.matches,
            max.matches,
            min_prune.matches,
            max_prune.matches,
        ),
        (
            par.unforgeables.len(),
            min.unforgeables,
            max.unforgeables,
            min_prune.unforgeables,
            max_prune.unforgeables,
        ),
        (
            par.bundles.len(),
            min.bundles,
            max.bundles,
            min_prune.bundles,
            max_prune.bundles,
        ),
        (
            par.conditionals.len(),
            min.conditionals,
            max.conditionals,
            min_prune.conditionals,
            max_prune.conditionals,
        ),
        (
            par.cost_signed_terms.len(),
            min.cost_signed_terms,
            max.cost_signed_terms,
            min_prune.cost_signed_terms,
            max_prune.cost_signed_terms,
        ),
        (
            par.cost_stacks.len(),
            min.cost_stacks,
            max.cost_stacks,
            min_prune.cost_stacks,
            max_prune.cost_stacks,
        ),
    ];
    let mut sizes = [(0isize, 0isize); 10];
    for (size, (len, minimum, maximum, min_remaining, max_remaining)) in
        sizes.iter_mut().zip(bounds)
    {
        let (len, minimum, maximum, min_remaining, max_remaining) = (
            isize::try_from(len).ok(),
            isize::try_from(minimum).ok(),
            isize::try_from(maximum).ok(),
            isize::try_from(min_remaining).ok(),
            isize::try_from(max_remaining).ok(),
        );
        let (Some(len), Some(minimum), Some(maximum), Some(min_remaining), Some(max_remaining)) =
            (len, minimum, maximum, min_remaining, max_remaining)
        else {
            return work.reject(RSpaceError::HostWorkRejected);
        };
        *size = (
            minimum.max(len - max_remaining),
            maximum.min(len - min_remaining),
        );
    }
    let sends = min_max_subsets(&par.sends, sizes[0].0, sizes[0].1, &work)?;
    let receives = min_max_subsets(&par.receives, sizes[1].0, sizes[1].1, &work)?;
    let news = min_max_subsets(&par.news, sizes[2].0, sizes[2].1, &work)?;
    let exprs = min_max_subsets(&par.exprs, sizes[3].0, sizes[3].1, &work)?;
    let matches = min_max_subsets(&par.matches, sizes[4].0, sizes[4].1, &work)?;
    let unforgeables = min_max_subsets(&par.unforgeables, sizes[5].0, sizes[5].1, &work)?;
    let bundles = min_max_subsets(&par.bundles, sizes[6].0, sizes[6].1, &work)?;
    let conditionals = min_max_subsets(&par.conditionals, sizes[7].0, sizes[7].1, &work)?;
    let cost_signed_terms = min_max_subsets(&par.cost_signed_terms, sizes[8].0, sizes[8].1, &work)?;
    let cost_stacks = min_max_subsets(&par.cost_stacks, sizes[9].0, sizes[9].1, &work)?;
    let exhausted = [
        sends.len(),
        receives.len(),
        news.len(),
        exprs.len(),
        matches.len(),
        unforgeables.len(),
        bundles.len(),
        conditionals.len(),
        cost_signed_terms.len(),
        cost_stacks.len(),
    ]
    .contains(&0);
    Some(SubPars {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        conditionals,
        cost_signed_terms,
        cost_stacks,
        positions: [0; 10],
        exhausted,
        work,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use models::rust::utils::new_gstring_expr;

    use super::*;
    use crate::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

    #[test]
    fn two_fields_keep_subset_product_order() {
        let par = Par {
            sends: vec![Send::default()],
            exprs: vec![Expr::default()],
            ..Par::default()
        };
        let zero = ParCount::new(&Par::default());
        let max = ParCount::new(&par);
        let context = SpatialMatcherContext::new();
        let pairs: Vec<_> = sub_pars(&par, &zero, &max, &zero, &max, context.work().unwrap())
            .unwrap()
            .map(Option::unwrap)
            .collect();
        let sizes: Vec<_> = pairs
            .iter()
            .map(|(selected, _)| (selected.sends.len(), selected.exprs.len()))
            .collect();
        assert_eq!(sizes, [(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn subset_order_is_stable_across_bounded_sizes() {
        let context = SpatialMatcherContext::new();
        let pairs = min_max_subsets(&[1u8, 2, 3], 0, 2, &context.work().unwrap()).unwrap();
        assert_eq!(pairs, [
            (vec![], vec![1, 2, 3]),
            (vec![1], vec![2, 3]),
            (vec![1, 2], vec![1, 3]),
            (vec![1, 2], vec![1, 3]),
            (vec![1, 2, 3], vec![1, 2]),
            (vec![1, 2, 3], vec![1, 2]),
            (vec![2, 3], vec![1, 2]),
        ]);
        let forced = min_max_subsets(&[1u8, 2, 3], 2, 2, &context.work().unwrap()).unwrap();
        assert_eq!(forced, [
            (vec![1, 2], vec![3]),
            (vec![1, 2, 3], vec![2]),
            (vec![2, 3], vec![1, 2]),
        ]);
    }

    #[test]
    fn impossible_large_minimum_finishes_without_recursion() {
        let values = vec![0u8; 65_535];
        let context = SpatialMatcherContext::new();
        let pairs = min_max_subsets(&values, 65_536, 65_536, &context.work().unwrap()).unwrap();
        assert!(pairs.is_empty());
    }

    #[test]
    fn subset_build_and_candidate_copy_reject_before_output() {
        let par = Par {
            exprs: vec![new_gstring_expr("payload".repeat(1024))],
            ..Par::default()
        };
        let zero = ParCount::new(&Par::default());
        let max = ParCount::new(&par);
        let totals = Mutex::new([0usize; 3]);
        let unlimited = |operations: usize, scanned: usize, backing: usize| {
            let mut totals = totals.lock().unwrap();
            for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        let context = SpatialMatcherContext::with_meter(&unlimited).unwrap();
        let mut subsets =
            sub_pars(&par, &zero, &max, &zero, &max, context.work().unwrap()).unwrap();
        let built = *totals.lock().unwrap();
        assert!(subsets.next().unwrap().is_some());
        let emitted = *totals.lock().unwrap();
        assert!(emitted[2] > built[2]);

        for (limit_backing, fails_during_build) in [(built[2] - 1, true), (emitted[2] - 1, false)] {
            let spent = Mutex::new([0usize; 3]);
            let meter = |operations: usize, scanned: usize, backing: usize| {
                let mut spent = spent.lock().unwrap();
                let amounts = [operations, scanned, backing];
                if spent[2] + backing > limit_backing {
                    return Err(RSpaceError::HostWorkRejected);
                }
                for (used, amount) in spent.iter_mut().zip(amounts) {
                    *used += amount;
                }
                Ok(())
            };
            let context = SpatialMatcherContext::with_meter(&meter).unwrap();
            let subsets = sub_pars(&par, &zero, &max, &zero, &max, context.work().unwrap());
            if fails_during_build {
                assert!(subsets.is_none());
            } else {
                assert!(subsets.unwrap().next().unwrap().is_none());
            }
            assert!(matches!(
                context.take_error(),
                Some(RSpaceError::HostWorkRejected)
            ));
        }
    }
}
