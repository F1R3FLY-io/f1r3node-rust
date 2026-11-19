use models::rhoapi::Par;

use super::par_count::ParCount;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParSpatialMatcherUtils.scala - subPars
/// Every split of `par` into a sub-par and its complement whose per-kind sizes
/// lie within the bounds, in the order Scala's `subPars` yields them. The
/// splits are produced one at a time, so a caller that stops at the first
/// useful split does not pay for the rest.
pub fn sub_pars(
    par: &Par,
    min: &ParCount,
    max: &ParCount,
    min_prune: &ParCount,
    max_prune: &ParCount,
) -> SubPars {
    let lens = per_kind(&ParCount::new(par));
    let (min, max) = (per_kind(min), per_kind(max));
    let (min_prune, max_prune) = (per_kind(min_prune), per_kind(max_prune));

    SubPars {
        par: par.clone(),
        cursors: std::array::from_fn(|kind| {
            let len = lens[kind] as isize;
            let range = SizeRange {
                min: std::cmp::max(min[kind] as isize, len - max_prune[kind] as isize),
                max: std::cmp::min(max[kind] as isize, len - min_prune[kind] as isize),
            };
            SubsetCursor::new(lens[kind], range)
        }),
        started: false,
    }
}

/// The counts in the order the splits nest: sends, receives, news, exprs,
/// matches, unforgeables, bundles.
fn per_kind(count: &ParCount) -> [usize; 7] {
    [
        count.sends,
        count.receives,
        count.news,
        count.exprs,
        count.matches,
        count.unforgeables,
        count.bundles,
    ]
}

#[derive(Clone, Copy)]
struct SizeRange {
    min: isize,
    max: isize,
}

/// Steps through the subsets of `len` elements whose size lies in a range, in
/// increasing order of the bitmask that has bit `i` set when element `i` is in
/// the subset. This is the order of Scala's `minMaxSubsets`.
struct SubsetCursor {
    len: usize,
    min: usize,
    max: usize,
    empty: bool,
    mask: Vec<bool>,
}

impl SubsetCursor {
    fn new(len: usize, range: SizeRange) -> Self {
        let max = range.max.min(len as isize);
        let min = range.min.max(0);
        let empty = max < 0 || min > max;
        SubsetCursor {
            len,
            min: min.max(0) as usize,
            max: max.max(0) as usize,
            empty,
            mask: vec![false; len],
        }
    }

    fn popcount(&self) -> usize { self.mask.iter().filter(|bit| **bit).count() }

    /// Adds `1 << from` to the mask. Returns false when the mask overflows.
    fn add_bit(&mut self, from: usize) -> bool {
        for bit in self.mask.iter_mut().skip(from) {
            if *bit {
                *bit = false;
            } else {
                *bit = true;
                return true;
            }
        }
        false
    }

    /// Moves the mask to the smallest value at or above it whose size is in
    /// range. Returns false when no such value fits in `len` bits.
    fn settle(&mut self) -> bool {
        while self.popcount() > self.max {
            let lowest = self
                .mask
                .iter()
                .position(|bit| *bit)
                .expect("a mask with more bits than max has a set bit");
            if !self.add_bit(lowest) {
                return false;
            }
        }
        while self.popcount() < self.min {
            match self.mask.iter().position(|bit| !*bit) {
                Some(lowest_clear) => self.mask[lowest_clear] = true,
                None => return false,
            }
        }
        true
    }

    fn first(&mut self) -> bool {
        if self.empty {
            return false;
        }
        self.mask.iter_mut().for_each(|bit| *bit = false);
        self.settle()
    }

    fn advance(&mut self) -> bool { self.len > 0 && self.add_bit(0) && self.settle() }

    fn split<A: Clone>(&self, items: &[A]) -> (Vec<A>, Vec<A>) {
        let mut chosen = Vec::new();
        let mut rest = Vec::new();
        for (item, in_subset) in items.iter().zip(&self.mask) {
            if *in_subset {
                chosen.push(item.clone());
            } else {
                rest.push(item.clone());
            }
        }
        (chosen, rest)
    }
}

/// The splits of one par, sends varying slowest and bundles fastest.
pub struct SubPars {
    par: Par,
    cursors: [SubsetCursor; 7],
    started: bool,
}

impl SubPars {
    fn step(&mut self) -> bool {
        if !self.started {
            self.started = true;
            return self.cursors.iter_mut().all(SubsetCursor::first);
        }
        for kind in (0..self.cursors.len()).rev() {
            if self.cursors[kind].advance() {
                return self.cursors[kind + 1..].iter_mut().all(SubsetCursor::first);
            }
        }
        false
    }

    fn current(&self) -> (Par, Par) {
        let [sends, receives, news, exprs, matches, unforgeables, bundles] = &self.cursors;
        let (sub_sends, rest_sends) = sends.split(&self.par.sends);
        let (sub_receives, rest_receives) = receives.split(&self.par.receives);
        let (sub_news, rest_news) = news.split(&self.par.news);
        let (sub_exprs, rest_exprs) = exprs.split(&self.par.exprs);
        let (sub_matches, rest_matches) = matches.split(&self.par.matches);
        let (sub_unforgeables, rest_unforgeables) = unforgeables.split(&self.par.unforgeables);
        let (sub_bundles, rest_bundles) = bundles.split(&self.par.bundles);
        let part = |sends, receives, news, exprs, matches, unforgeables, bundles| Par {
            sends,
            receives,
            news,
            exprs,
            matches,
            unforgeables,
            bundles,
            connectives: Vec::new(),
            conditionals: Vec::new(),
            locally_free: Vec::new(),
            connective_used: false,
        };
        (
            part(
                sub_sends,
                sub_receives,
                sub_news,
                sub_exprs,
                sub_matches,
                sub_unforgeables,
                sub_bundles,
            ),
            part(
                rest_sends,
                rest_receives,
                rest_news,
                rest_exprs,
                rest_matches,
                rest_unforgeables,
                rest_bundles,
            ),
        )
    }
}

impl Iterator for SubPars {
    type Item = (Par, Par);

    fn next(&mut self) -> Option<(Par, Par)> { self.step().then(|| self.current()) }
}
