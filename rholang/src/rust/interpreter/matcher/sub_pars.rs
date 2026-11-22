use std::collections::VecDeque;

use models::rhoapi::Par;

use super::par_count::ParCount;
use super::spatial_matcher::StepBudget;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParSpatialMatcherUtils.scala - subPars
/// The sub-pars of `par` whose per-kind sizes lie within the bounds, in the
/// order Scala's `subPars` yields them. Each sub-par is produced on demand,
/// and every element a split reads or copies spends `budget`, so the work of
/// an enumeration is bounded by the budget whatever the shape of `par`. The
/// iteration ends when the budget does.
pub fn sub_pars(
    par: &Par,
    min: &ParCount,
    max: &ParCount,
    min_prune: &ParCount,
    max_prune: &ParCount,
    budget: &StepBudget,
) -> SubPars {
    let lens = per_kind(&ParCount::new(par));
    let (min, max) = (per_kind(min), per_kind(max));
    let (min_prune, max_prune) = (per_kind(min_prune), per_kind(max_prune));
    let affordable = budget.spend(lens.iter().sum()).is_some();

    SubPars {
        par: if affordable {
            par.clone()
        } else {
            Par::default()
        },
        done: !affordable,
        cursors: std::array::from_fn(|kind| {
            let len = lens[kind] as isize;
            let range = SizeRange {
                min: std::cmp::max(min[kind] as isize, len - max_prune[kind] as isize),
                max: std::cmp::min(max[kind] as isize, len - min_prune[kind] as isize),
            };
            SubsetCursor::new(lens[kind], range)
        }),
        started: false,
        budget: budget.clone(),
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
/// the subset, which is the order of Scala's `minMaxSubsets`. The subset is
/// held as its positions in ascending order, so a step costs the positions it
/// moves rather than `len`.
struct SubsetCursor {
    len: usize,
    min: usize,
    max: usize,
    empty: bool,
    positions: VecDeque<usize>,
}

impl SubsetCursor {
    fn new(len: usize, range: SizeRange) -> Self {
        let max = range.max.min(len as isize);
        let min = range.min.max(0);
        SubsetCursor {
            len,
            min: min as usize,
            max: max.max(0) as usize,
            empty: max < 0 || min > max,
            positions: VecDeque::new(),
        }
    }

    /// How many positions, from the lowest, run on consecutively from `start`.
    fn run_from(&self, start: usize, budget: &StepBudget) -> Option<usize> {
        let mut run = 0;
        while self.positions.get(run) == Some(&(start + run)) {
            budget.spend(1)?;
            run += 1;
        }
        Some(run)
    }

    /// Moves to the smallest mask at or above the current one whose size is in
    /// range. `None` when no such mask fits in `len` bits or the budget runs out.
    fn settle(&mut self, budget: &StepBudget) -> Option<()> {
        while self.positions.len() > self.max {
            let lowest = self.positions[0];
            let run = self.run_from(lowest, budget)?;
            if run == 1 {
                // Adding the lowest bit moves it up one place without changing
                // the size, so every mask until it meets the next set bit is
                // skipped; jump to just below that bit.
                let next = *self.positions.get(1)?;
                budget.spend(1)?;
                self.positions[0] = next - 1;
                continue;
            }
            budget.spend(run)?;
            self.positions.drain(..run);
            let carried = lowest + run;
            if carried >= self.len {
                return None;
            }
            self.positions.push_front(carried);
        }
        while self.positions.len() < self.min {
            let lowest_clear = self.run_from(0, budget)?;
            if lowest_clear >= self.len {
                return None;
            }
            budget.spend(lowest_clear + 1)?;
            self.positions.insert(lowest_clear, lowest_clear);
        }
        Some(())
    }

    fn first(&mut self, budget: &StepBudget) -> Option<()> {
        if self.empty {
            return None;
        }
        budget.spend(self.positions.len() + 1)?;
        self.positions.clear();
        self.settle(budget)
    }

    /// The next mask: the current one plus one, then settled into range.
    fn advance(&mut self, budget: &StepBudget) -> Option<()> {
        let trailing = self.run_from(0, budget)?;
        if trailing >= self.len {
            return None;
        }
        budget.spend(trailing + 1)?;
        self.positions.drain(..trailing);
        self.positions.push_front(trailing);
        self.settle(budget)
    }

    fn chosen<A: Clone>(&self, items: &[A]) -> Vec<A> {
        self.positions
            .iter()
            .map(|position| items[*position].clone())
            .collect()
    }

    fn rest<A: Clone>(&self, items: &[A]) -> Vec<A> {
        let mut chosen = self.positions.iter().peekable();
        items
            .iter()
            .enumerate()
            .filter(|(position, _)| {
                if chosen.peek() == Some(&position) {
                    chosen.next();
                    false
                } else {
                    true
                }
            })
            .map(|(_, item)| item.clone())
            .collect()
    }
}

/// The sub-pars of one par, sends varying slowest and bundles fastest. The
/// complement of the current sub-par is built only when asked for.
pub struct SubPars {
    par: Par,
    done: bool,
    cursors: [SubsetCursor; 7],
    started: bool,
    budget: StepBudget,
}

impl SubPars {
    fn step(&mut self) -> Option<()> {
        let budget = &self.budget;
        if !self.started {
            self.started = true;
            return self
                .cursors
                .iter_mut()
                .try_for_each(|cursor| cursor.first(budget));
        }
        for kind in (0..self.cursors.len()).rev() {
            if self.cursors[kind].advance(budget).is_some() {
                return self.cursors[kind + 1..]
                    .iter_mut()
                    .try_for_each(|cursor| cursor.first(budget));
            }
        }
        None
    }

    /// The current sub-par, spending a step per element it copies.
    fn sub(&self) -> Option<Par> {
        self.budget.spend(
            self.cursors
                .iter()
                .map(|cursor| cursor.positions.len())
                .sum::<usize>()
                + 1,
        )?;
        let [sends, receives, news, exprs, matches, unforgeables, bundles] = &self.cursors;
        Some(Par {
            sends: sends.chosen(&self.par.sends),
            receives: receives.chosen(&self.par.receives),
            news: news.chosen(&self.par.news),
            exprs: exprs.chosen(&self.par.exprs),
            matches: matches.chosen(&self.par.matches),
            unforgeables: unforgeables.chosen(&self.par.unforgeables),
            bundles: bundles.chosen(&self.par.bundles),
            ..Default::default()
        })
    }

    /// The complement of the sub-par last yielded, spending a step per element
    /// of the par. `None` when the budget runs out.
    pub fn complement(&self) -> Option<Par> {
        self.budget
            .spend(self.cursors.iter().map(|cursor| cursor.len).sum::<usize>() + 1)?;
        let [sends, receives, news, exprs, matches, unforgeables, bundles] = &self.cursors;
        Some(Par {
            sends: sends.rest(&self.par.sends),
            receives: receives.rest(&self.par.receives),
            news: news.rest(&self.par.news),
            exprs: exprs.rest(&self.par.exprs),
            matches: matches.rest(&self.par.matches),
            unforgeables: unforgeables.rest(&self.par.unforgeables),
            bundles: bundles.rest(&self.par.bundles),
            ..Default::default()
        })
    }
}

impl Iterator for SubPars {
    type Item = Par;

    fn next(&mut self) -> Option<Par> {
        if self.done {
            return None;
        }
        let sub = self.step().and_then(|()| self.sub());
        self.done = sub.is_none();
        sub
    }
}
