// `sub_pars` must yield the same splits, in the same order, as Scala's
// `ParSpatialMatcherUtils.subPars`. The oracle below transcribes the Scala
// recursion directly.

use models::rhoapi::{Bundle, Expr, GUnforgeable, Match, New, Par, Receive, Send};
use models::rust::utils::{new_gint_expr, new_gint_par};
use rholang::rust::interpreter::matcher::par_count::ParCount;
use rholang::rust::interpreter::matcher::spatial_matcher::StepBudget;
use rholang::rust::interpreter::matcher::sub_pars::{sub_pars, SubPars};

fn unlimited() -> StepBudget { StepBudget::new(usize::MAX) }

type Split<A> = (Vec<A>, Vec<A>);

fn counted_max_subsets<A: Clone>(items: &[A], max: isize) -> Vec<(Vec<A>, Vec<A>, isize)> {
    match items.split_first() {
        None => vec![(Vec::new(), Vec::new(), 0)],
        Some((head, rest)) => {
            let mut out = vec![(Vec::new(), items.to_vec(), 0)];
            for (tail, complement, count) in counted_max_subsets(rest, max) {
                let with_head = |mut v: Vec<A>| {
                    v.insert(0, head.clone());
                    v
                };
                if count == max {
                    out.push((tail, with_head(complement), count));
                } else if tail.is_empty() {
                    out.push((with_head(tail), complement, 1));
                } else {
                    out.push((tail.clone(), with_head(complement.clone()), count));
                    out.push((with_head(tail), complement, count + 1));
                }
            }
            out
        }
    }
}

fn worker<A: Clone>(items: &[A], min: isize, max: isize) -> Vec<(Vec<A>, Vec<A>, isize)> {
    if max < 0 || min > max {
        return Vec::new();
    }
    if min <= 0 {
        return if max == 0 {
            vec![(Vec::new(), items.to_vec(), 0)]
        } else {
            counted_max_subsets(items, max)
        };
    }
    match items.split_first() {
        None => Vec::new(),
        Some((head, rest)) => {
            let decr = min - 1;
            let mut out = Vec::new();
            for (tail, complement, count) in worker(rest, decr, max) {
                let with_head = |mut v: Vec<A>| {
                    v.insert(0, head.clone());
                    v
                };
                if count == max {
                    out.push((tail, with_head(complement), count));
                } else if count == decr {
                    out.push((with_head(tail), complement, min));
                } else {
                    out.push((tail.clone(), with_head(complement.clone()), count));
                    out.push((with_head(tail), complement, count + 1));
                }
            }
            out
        }
    }
}

fn min_max_subsets<A: Clone>(items: &[A], min: isize, max: isize) -> Vec<Split<A>> {
    worker(items, min, max)
        .into_iter()
        .map(|(sub, rest, _)| (sub, rest))
        .collect()
}

struct Bounds<'a> {
    min: &'a ParCount,
    max: &'a ParCount,
    min_prune: &'a ParCount,
    max_prune: &'a ParCount,
}

impl Bounds<'_> {
    fn range(&self, len: usize, field: fn(&ParCount) -> usize) -> (isize, isize) {
        let len = len as isize;
        (
            std::cmp::max(
                field(self.min) as isize,
                len - field(self.max_prune) as isize,
            ),
            std::cmp::min(
                field(self.max) as isize,
                len - field(self.min_prune) as isize,
            ),
        )
    }
}

/// Extends every split in `acc` by every split of one kind, earlier kinds
/// varying slowest, as Scala's nested for-comprehension does.
fn nest<A: Clone>(
    acc: Vec<(Par, Par)>,
    items: &[A],
    (min, max): (isize, isize),
    set: fn(&mut Par, Vec<A>),
) -> Vec<(Par, Par)> {
    let splits = min_max_subsets(items, min, max);
    let mut out = Vec::new();
    for (sub, rest) in acc {
        for (chosen, left) in &splits {
            let (mut sub, mut rest) = (sub.clone(), rest.clone());
            set(&mut sub, chosen.clone());
            set(&mut rest, left.clone());
            out.push((sub, rest));
        }
    }
    out
}

fn scala_sub_pars(par: &Par, bounds: &Bounds) -> Vec<(Par, Par)> {
    let acc = vec![(Par::default(), Par::default())];
    let acc = nest(
        acc,
        &par.sends,
        bounds.range(par.sends.len(), |c| c.sends),
        |p, v| p.sends = v,
    );
    let acc = nest(
        acc,
        &par.receives,
        bounds.range(par.receives.len(), |c| c.receives),
        |p, v| p.receives = v,
    );
    let acc = nest(
        acc,
        &par.news,
        bounds.range(par.news.len(), |c| c.news),
        |p, v| p.news = v,
    );
    let acc = nest(
        acc,
        &par.exprs,
        bounds.range(par.exprs.len(), |c| c.exprs),
        |p, v| p.exprs = v,
    );
    let acc = nest(
        acc,
        &par.matches,
        bounds.range(par.matches.len(), |c| c.matches),
        |p, v| p.matches = v,
    );
    let acc = nest(
        acc,
        &par.unforgeables,
        bounds.range(par.unforgeables.len(), |c| c.unforgeables),
        |p, v| p.unforgeables = v,
    );
    nest(
        acc,
        &par.bundles,
        bounds.range(par.bundles.len(), |c| c.bundles),
        |p, v| p.bundles = v,
    )
}

fn send(tag: i64) -> Send {
    Send {
        chan: Some(new_gint_par(tag, Vec::new(), false)),
        data: vec![],
        persistent: false,
        locally_free: vec![],
        connective_used: false,
    }
}

fn unforgeable(tag: u8) -> GUnforgeable {
    GUnforgeable {
        unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GPrivateBody(
            models::rhoapi::GPrivate { id: vec![tag] },
        )),
    }
}

fn par(sends: usize, exprs: usize, unforgeables: usize) -> Par {
    Par::default()
        .with_sends((0..sends as i64).map(send).collect())
        .with_exprs((0..exprs as i64).map(new_gint_expr).collect::<Vec<Expr>>())
        .with_unforgeables((0..unforgeables as u8).map(unforgeable).collect())
}

fn count(sends: usize, exprs: usize, unforgeables: usize) -> ParCount {
    let mut c = ParCount::new(&Par::default());
    c.sends = sends;
    c.exprs = exprs;
    c.unforgeables = unforgeables;
    c
}

/// Every sub-par with the complement built for it.
fn with_complements(mut splits: SubPars) -> Vec<(Par, Par)> {
    let mut out = Vec::new();
    while let Some(sub) = splits.next() {
        out.push((sub, splits.complement().expect("an unlimited budget")));
    }
    out
}

fn check(target: Par, min: ParCount, max: ParCount, min_prune: ParCount, max_prune: ParCount) {
    let bounds = Bounds {
        min: &min,
        max: &max,
        min_prune: &min_prune,
        max_prune: &max_prune,
    };
    let actual = with_complements(sub_pars(
        &target,
        &min,
        &max,
        &min_prune,
        &max_prune,
        &unlimited(),
    ));
    let expected = scala_sub_pars(&target, &bounds);
    assert_eq!(
        actual, expected,
        "target {target:?} min {min:?} max {max:?}"
    );
}

#[test]
fn splits_match_scala_in_content_and_order() {
    let none = || count(0, 0, 0);
    let all = || count(0, 0, 0)._max();
    check(par(0, 0, 0), none(), all(), none(), all());
    check(par(3, 0, 0), none(), all(), none(), all());
    check(par(4, 2, 1), none(), all(), none(), all());
    check(par(4, 2, 1), count(1, 0, 0), count(2, 1, 1), none(), all());
    check(
        par(5, 1, 2),
        count(2, 1, 0),
        count(3, 1, 2),
        count(1, 0, 0),
        count(3, 1, 2),
    );
    check(
        par(4, 3, 0),
        none(),
        count(4, 3, 0),
        count(3, 2, 0),
        count(3, 2, 0),
    );
    check(par(3, 2, 2), count(4, 0, 0), all(), none(), all());
    check(par(6, 0, 0), count(2, 0, 0), count(4, 0, 0), none(), all());
}

#[test]
fn every_split_is_a_partition() {
    let target = par(5, 3, 2);
    let none = count(0, 0, 0);
    for (subset, complement) in with_complements(sub_pars(
        &target,
        &none,
        &none._max(),
        &none,
        &none._max(),
        &unlimited(),
    )) {
        let mut sends = subset.sends.clone();
        sends.extend(complement.sends.clone());
        assert_eq!(sends.len(), target.sends.len());
        assert!(target.sends.iter().all(|s| sends.contains(s)));
        let mut exprs = subset.exprs.clone();
        exprs.extend(complement.exprs.clone());
        assert_eq!(exprs.len(), target.exprs.len());
        assert!(target.exprs.iter().all(|e| exprs.contains(e)));
    }
}

#[test]
fn the_first_split_of_a_large_par_comes_without_the_others() {
    let target = par(40, 0, 0);
    let none = count(0, 0, 0);
    let mut splits = sub_pars(
        &target,
        &none,
        &none._max(),
        &none,
        &none._max(),
        &unlimited(),
    );
    let first = splits.next().expect("a first split");
    assert!(first.sends.is_empty());
    assert_eq!(splits.complement().expect("a complement").sends.len(), 40);
    let second = splits.next().expect("a second split");
    assert_eq!(second.sends, vec![send(0)]);
}

/// `n` of every kind a par holds.
fn every_kind(n: usize) -> Par {
    let tags = || 0..n as i64;
    par(n, n, n)
        .with_receives(
            tags()
                .map(|i| Receive {
                    bind_count: i as i32,
                    ..Default::default()
                })
                .collect(),
        )
        .with_news(
            tags()
                .map(|i| New {
                    bind_count: i as i32,
                    ..Default::default()
                })
                .collect(),
        )
        .with_matches(
            tags()
                .map(|i| Match {
                    target: Some(new_gint_par(i, Vec::new(), false)),
                    ..Default::default()
                })
                .collect(),
        )
        .with_bundles(
            tags()
                .map(|i| Bundle {
                    body: Some(new_gint_par(i, Vec::new(), false)),
                    ..Default::default()
                })
                .collect(),
        )
}

fn every_kind_count(n: usize) -> ParCount {
    let mut c = count(n, n, n);
    c.receives = n;
    c.news = n;
    c.matches = n;
    c.bundles = n;
    c
}

#[test]
fn splits_match_scala_across_every_kind() {
    let none = || every_kind_count(0);
    check(every_kind(2), none(), none()._max(), none(), none()._max());
    check(
        every_kind(2),
        every_kind_count(1),
        every_kind_count(1),
        none(),
        none()._max(),
    );
}

#[test]
fn every_element_a_split_copies_is_charged_to_the_budget() {
    let target = par(0, 1000, 0);
    let none = count(0, 0, 0);
    let budget = 50_000;
    let copied: usize = sub_pars(
        &target,
        &none,
        &none._max(),
        &none,
        &none._max(),
        &StepBudget::new(budget),
    )
    .map(|sub| sub.exprs.len())
    .sum();
    assert!(copied > 0);
    assert!(copied <= budget, "{copied} elements copied");
}

#[test]
fn single_element_sub_pars_cost_a_constant_each() {
    let target = par(0, 1000, 0);
    let one = count(0, 1, 0);
    let none = count(0, 0, 0);
    let yielded = sub_pars(
        &target,
        &one,
        &one,
        &none,
        &none._max(),
        &StepBudget::new(20 * target.exprs.len()),
    )
    .count();
    assert_eq!(yielded, target.exprs.len());
}
