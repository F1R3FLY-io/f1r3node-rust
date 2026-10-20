use super::exports::{
    Bundle, CostSignedTerm, CostStack, Expr, GUnforgeable, If, Match, New, Par, Receive, Send,
};
use super::par_count::ParCount;

fn counted_max_subsets<A: Clone>(values: &[A], max_size: isize) -> Vec<(Vec<A>, Vec<A>, isize)> {
    let mut results = vec![(Vec::new(), Vec::new(), 0)];
    for index in (0..values.len()).rev() {
        let head = &values[index];
        let added = results
            .iter()
            .map(
                |(tail, _, count)| match *count == max_size || tail.is_empty() {
                    true => 1,
                    false => 2,
                },
            )
            .sum::<usize>();
        let mut next = Vec::with_capacity(added + 1);
        next.push((Vec::new(), values[index..].to_vec(), 0));
        for (mut tail, mut complement, count) in results {
            if count == max_size {
                complement.insert(0, head.clone());
                next.push((tail, complement, count));
            } else if tail.is_empty() {
                tail.insert(0, head.clone());
                next.push((tail, complement, 1));
            } else {
                complement.insert(0, head.clone());
                tail.insert(0, head.clone());
                next.push((tail.clone(), complement.clone(), count));
                next.push((tail, complement, count + 1));
            }
        }
        results = next;
    }
    results
}

fn subset_worker<A: Clone>(
    values: &[A],
    min_size: isize,
    max_size: isize,
) -> Vec<(Vec<A>, Vec<A>, isize)> {
    if max_size < 0 || min_size > max_size {
        return Vec::new();
    }
    if min_size <= 0 {
        if max_size == 0 {
            return vec![(Vec::new(), values.to_vec(), 0)];
        }
        return counted_max_subsets(values, max_size);
    }

    let depth = min_size.unsigned_abs();
    if depth > values.len() {
        return Vec::new();
    }
    let mut results = counted_max_subsets(&values[depth..], max_size);
    for index in (0..depth).rev() {
        let head = &values[index];
        let minimum = min_size - index as isize;
        let decr = minimum - 1;
        let added = results
            .iter()
            .map(|(_, _, count)| match *count == max_size || *count == decr {
                true => 1,
                false => 2,
            })
            .sum::<usize>();
        let mut next = Vec::with_capacity(added);
        for (mut tail, mut complement, count) in results {
            if count == max_size {
                complement.insert(0, head.clone());
                next.push((tail, complement, count));
            } else if count == decr {
                tail.insert(0, head.clone());
                next.push((tail, complement, minimum));
            } else {
                complement.insert(0, head.clone());
                tail.insert(0, head.clone());
                next.push((tail.clone(), complement.clone(), count));
                next.push((tail, complement, count + 1));
            }
        }
        results = next;
    }
    results
}

fn min_max_subsets<A: Clone>(
    values: &[A],
    min_size: isize,
    max_size: isize,
) -> Vec<(Vec<A>, Vec<A>)> {
    subset_worker(values, min_size, max_size)
        .into_iter()
        .map(|(selected, remainder, _)| (selected, remainder))
        .collect()
}

struct SubPars {
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
}

impl SubPars {
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

impl Iterator for SubPars {
    type Item = (Par, Par);

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted {
            return None;
        }
        let sends = self.sends[self.positions[0]].clone();
        let receives = self.receives[self.positions[1]].clone();
        let news = self.news[self.positions[2]].clone();
        let exprs = self.exprs[self.positions[3]].clone();
        let matches = self.matches[self.positions[4]].clone();
        let unforgeables = self.unforgeables[self.positions[5]].clone();
        let bundles = self.bundles[self.positions[6]].clone();
        let conditionals = self.conditionals[self.positions[7]].clone();
        let cost_signed_terms = self.cost_signed_terms[self.positions[8]].clone();
        let cost_stacks = self.cost_stacks[self.positions[9]].clone();
        let candidate = (
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
        );
        self.advance();
        Some(candidate)
    }
}

pub(super) fn sub_pars(
    par: &Par,
    min: &ParCount,
    max: &ParCount,
    min_prune: &ParCount,
    max_prune: &ParCount,
) -> Option<impl Iterator<Item = (Par, Par)>> {
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
            isize::try_from(len).ok()?,
            isize::try_from(minimum).ok()?,
            isize::try_from(maximum).ok()?,
            isize::try_from(min_remaining).ok()?,
            isize::try_from(max_remaining).ok()?,
        );
        *size = (
            minimum.max(len - max_remaining),
            maximum.min(len - min_remaining),
        );
    }
    let sends = min_max_subsets(&par.sends, sizes[0].0, sizes[0].1);
    let receives = min_max_subsets(&par.receives, sizes[1].0, sizes[1].1);
    let news = min_max_subsets(&par.news, sizes[2].0, sizes[2].1);
    let exprs = min_max_subsets(&par.exprs, sizes[3].0, sizes[3].1);
    let matches = min_max_subsets(&par.matches, sizes[4].0, sizes[4].1);
    let unforgeables = min_max_subsets(&par.unforgeables, sizes[5].0, sizes[5].1);
    let bundles = min_max_subsets(&par.bundles, sizes[6].0, sizes[6].1);
    let conditionals = min_max_subsets(&par.conditionals, sizes[7].0, sizes[7].1);
    let cost_signed_terms = min_max_subsets(&par.cost_signed_terms, sizes[8].0, sizes[8].1);
    let cost_stacks = min_max_subsets(&par.cost_stacks, sizes[9].0, sizes[9].1);
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Direct width probe for the private subset builder. Keep this inside its
    /// owning module: driving a public spatial match would also measure PDA
    /// frames and would no longer isolate the exact-width `sub_pars` path.
    ///
    /// Run the ignored lib test in a separate process with PROBE_DEPTH and
    /// PROBE_STACK set; as with stack_depth_probe, a native stack overflow
    /// aborts the process rather than unwinding.
    #[test]
    #[ignore = "measurement probe: PROBE_DEPTH and PROBE_STACK select width and thread stack"]
    fn sub_pars_width_stack_probe() {
        let width: usize = std::env::var("PROBE_DEPTH")
            .unwrap_or_else(|_| "10".to_owned())
            .parse()
            .expect("PROBE_DEPTH must be a non-negative integer");
        let stack: usize = std::env::var("PROBE_STACK")
            .unwrap_or_else(|_| "8388608".to_owned())
            .parse()
            .expect("PROBE_STACK must be a non-negative integer");
        let handle = std::thread::Builder::new()
            .stack_size(stack)
            .spawn(move || {
                let mut par = Par::default();
                par.exprs = (0..width)
                    .map(|index| {
                        let mut value =
                            models::rust::utils::new_gint_par(index as i64, Vec::new(), false);
                        value.exprs.pop().expect("GInt has one expression")
                    })
                    .collect();
                let zero = ParCount::new(&Par::default());
                let mut exact = zero.clone();
                exact.exprs = width;
                let subsets = sub_pars(&par, &exact, &exact, &zero, &zero)
                    .expect("exact-width subset construction")
                    .collect::<Vec<_>>();
                assert_eq!(subsets.len(), 1, "the exact-width subset is unique");
                assert_eq!(subsets[0].0.exprs.len(), width);
                std::mem::forget(subsets);
                std::mem::forget(par);
            })
            .expect("spawn subset probe thread");
        handle.join().expect("subset probe thread panicked");
    }

    #[test]
    fn two_fields_keep_subset_product_order() {
        let par = models::par_from_default! {
            sends: vec![Send::default()],
            exprs: vec![Expr::default()],
            ..Par::default()
        };
        let zero = ParCount::new(&Par::default());
        let max = ParCount::new(&par);
        let pairs: Vec<_> = sub_pars(&par, &zero, &max, &zero, &max)
            .expect("the counts of a small Par fit in isize")
            .collect();
        let sizes: Vec<_> = pairs
            .iter()
            .map(|(selected, _)| (selected.sends.len(), selected.exprs.len()))
            .collect();
        assert_eq!(sizes, [(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn subset_order_is_stable_across_bounded_sizes() {
        let pairs = min_max_subsets(&[1u8, 2, 3], 0, 2);
        assert_eq!(pairs, [
            (vec![], vec![1, 2, 3]),
            (vec![1], vec![2, 3]),
            (vec![1, 2], vec![1, 3]),
            (vec![1, 2], vec![1, 3]),
            (vec![1, 2, 3], vec![1, 2]),
            (vec![1, 2, 3], vec![1, 2]),
            (vec![2, 3], vec![1, 2]),
        ]);
        let forced = min_max_subsets(&[1u8, 2, 3], 2, 2);
        assert_eq!(forced, [
            (vec![1, 2], vec![3]),
            (vec![1, 2, 3], vec![2]),
            (vec![2, 3], vec![1, 2]),
        ]);
    }

    #[test]
    fn impossible_large_minimum_finishes_without_recursion() {
        let values = vec![0u8; 65_535];
        let pairs = min_max_subsets(&values, 65_536, 65_536);
        assert!(pairs.is_empty());
    }
}
