// `sub_pars` must yield the same splits, in the same order, as Scala's
// `ParSpatialMatcherUtils.subPars`. The oracle below transcribes the Scala
// recursion directly.

use models::rhoapi::{Expr, GUnforgeable, Par, Send};
use models::rust::utils::{new_gint_expr, new_gint_par};
use rholang::rust::interpreter::matcher::par_count::ParCount;
use rholang::rust::interpreter::matcher::sub_pars::sub_pars;

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

fn scala_sub_pars(par: &Par, bounds: &Bounds) -> Vec<(Par, Par)> {
    let (s_min, s_max) = bounds.range(par.sends.len(), |c| c.sends);
    let (e_min, e_max) = bounds.range(par.exprs.len(), |c| c.exprs);
    let (u_min, u_max) = bounds.range(par.unforgeables.len(), |c| c.unforgeables);
    let mut out = Vec::new();
    for (sends, rest_sends) in min_max_subsets(&par.sends, s_min, s_max) {
        for (exprs, rest_exprs) in min_max_subsets(&par.exprs, e_min, e_max) {
            for (unfs, rest_unfs) in min_max_subsets(&par.unforgeables, u_min, u_max) {
                out.push((
                    Par::default()
                        .with_sends(sends.clone())
                        .with_exprs(exprs.clone())
                        .with_unforgeables(unfs.clone()),
                    Par::default()
                        .with_sends(rest_sends.clone())
                        .with_exprs(rest_exprs.clone())
                        .with_unforgeables(rest_unfs.clone()),
                ));
            }
        }
    }
    out
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

fn check(target: Par, min: ParCount, max: ParCount, min_prune: ParCount, max_prune: ParCount) {
    let bounds = Bounds {
        min: &min,
        max: &max,
        min_prune: &min_prune,
        max_prune: &max_prune,
    };
    let actual: Vec<(Par, Par)> = sub_pars(&target, &min, &max, &min_prune, &max_prune).collect();
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
    for (subset, complement) in sub_pars(&target, &none, &none._max(), &none, &none._max()) {
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
    let mut splits = sub_pars(&target, &none, &none._max(), &none, &none._max());
    let (first, rest) = splits.next().expect("a first split");
    assert!(first.sends.is_empty());
    assert_eq!(rest.sends.len(), 40);
    let (second, _) = splits.next().expect("a second split");
    assert_eq!(second.sends, vec![send(0)]);
}
