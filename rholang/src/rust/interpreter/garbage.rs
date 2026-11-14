// Conservative garbage-collection analysis for Rholang unforgeable names.
// See docs/rholang/garbage-collection.md for the theorem and proof.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{Expr, Par};
use models::rust::utils::bitset_contains;

use super::matcher::has_locally_free::HasLocallyFree;

/// Returns `true` iff bound-variable index `idx` (measured in the de Bruijn
/// scope that directly contains `par`) appears in `par` only in positions
/// from which it cannot escape the lexical closure of the `new` that bound
/// it:
///
/// - top-level `Send.chan` or `Receive.source` positions (reachable
///   through par-composition, bundles in channel position, nested `new.p`,
///   match-case sources, and if-branches — constructs that reduce
///   immediately rather than park a body in rspace).
///
/// Any other occurrence — including any mention of `idx` inside a
/// `Receive` body — disqualifies trappedness. Receive bodies are stored
/// as parked continuations and are executed later if the enclosing
/// receive fires; even when the receive is parked on `idx` itself, a
/// persistent receive combined with a nested receive on a non-trapped
/// channel can still leak a further-reaching reference. See
/// `docs/rholang/garbage-collection.md`.
pub fn is_scope_trapped(idx: usize, par: &Par) -> bool { check_par(idx, par) }

fn check_par(idx: usize, par: &Par) -> bool {
    if !bitset_contains(&par.locally_free, idx) {
        return true;
    }

    for s in &par.sends {
        if !bitset_contains(&s.locally_free, idx) {
            continue;
        }
        let chan = s.chan.as_ref().expect("send chan");
        if !check_in_chan(idx, chan) {
            return false;
        }
        for d in &s.data {
            if bitset_contains(&d.locally_free, idx) {
                return false;
            }
        }
    }

    for r in &par.receives {
        if !bitset_contains(&r.locally_free, idx) {
            continue;
        }
        for b in &r.binds {
            let source = b.source.as_ref().expect("receive source");
            if !check_in_chan(idx, source) {
                return false;
            }
            for p in &b.patterns {
                if bitset_contains(&p.locally_free, idx) {
                    return false;
                }
            }
        }
        // Strictest-safe rule: if the trapped variable appears anywhere in
        // the Receive body, we cannot prove garbage without reasoning about
        // when the enclosing receive fires. Even if the receive is parked
        // on `idx` itself, a persistent receive on `idx` could fire from
        // an inside-closure send and emit a further-reaching reference
        // (e.g., through a nested receive on a non-trapped channel inside
        // the body). Rather than enumerate those sub-cases, disallow any
        // occurrence of `idx` inside a stored body.
        let body = r.body.as_ref().expect("receive body");
        let body_idx = idx + r.bind_count as usize;
        if bitset_contains(&body.locally_free, body_idx) {
            return false;
        }
        if let Some(cond) = &r.condition {
            if bitset_contains(&cond.locally_free, body_idx) {
                return false;
            }
        }
    }

    for n in &par.news {
        if !bitset_contains(&n.locally_free, idx) {
            continue;
        }
        let inner_body = n.p.as_ref().expect("new body");
        if !check_par(idx + n.bind_count as usize, inner_body) {
            return false;
        }
    }

    for m in &par.matches {
        if !bitset_contains(&m.locally_free, idx) {
            continue;
        }
        let target = m.target.as_ref().expect("match target");
        if bitset_contains(&target.locally_free, idx) {
            return false;
        }
        for c in &m.cases {
            let pat = c.pattern.as_ref().expect("match pattern");
            if bitset_contains(&pat.locally_free, idx) {
                return false;
            }
            let src = c.source.as_ref().expect("match case source");
            if !check_par(idx + c.free_count as usize, src) {
                return false;
            }
            if let Some(g) = &c.guard {
                if bitset_contains(&g.locally_free, idx + c.free_count as usize) {
                    return false;
                }
            }
        }
    }

    for b in &par.bundles {
        let body = b.body.as_ref().expect("bundle body");
        if bitset_contains(&body.locally_free, idx) {
            return false;
        }
    }

    for e in &par.exprs {
        if bitset_contains(&e.locally_free(0), idx) {
            return false;
        }
    }

    for cond in &par.conditionals {
        let c = cond.condition.as_ref().expect("if condition");
        if bitset_contains(&c.locally_free, idx) {
            return false;
        }
        let t = cond.if_true.as_ref().expect("if_true");
        if !check_par(idx, t) {
            return false;
        }
        let f = cond.if_false.as_ref().expect("if_false");
        if !check_par(idx, f) {
            return false;
        }
    }

    for c in &par.connectives {
        if bitset_contains(&c.locally_free(0), idx) {
            return false;
        }
    }

    true
}

fn check_in_chan(idx: usize, chan: &Par) -> bool {
    if !bitset_contains(&chan.locally_free, idx) {
        return true;
    }

    if !chan.sends.is_empty()
        || !chan.receives.is_empty()
        || !chan.news.is_empty()
        || !chan.matches.is_empty()
        || !chan.conditionals.is_empty()
        || !chan.connectives.is_empty()
    {
        return false;
    }

    for b in &chan.bundles {
        let body = b.body.as_ref().expect("bundle body");
        if !check_in_chan(idx, body) {
            return false;
        }
    }

    for e in &chan.exprs {
        if !check_expr_in_chan(idx, e) {
            return false;
        }
    }

    true
}

fn check_expr_in_chan(idx: usize, e: &Expr) -> bool {
    if let Some(ExprInstance::EVarBody(ev)) = &e.expr_instance {
        if let Some(VarInstance::BoundVar(i)) = ev.v.as_ref().and_then(|v| v.var_instance.as_ref())
        {
            if (*i as usize) == idx {
                return true;
            }
        }
    }
    !bitset_contains(&e.locally_free(0), idx)
}

/// Returns the `GPrivate.id` bytes iff `par` is a bare `GPrivate` and
/// nothing else. `None` for every other channel shape, including the
/// system unforgeables `GDeployId`, `GDeployerId`, `GSysAuthToken` that
/// must never be swept.
pub fn gprivate_bytes_if_bare(par: &Par) -> Option<&[u8]> {
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.exprs.is_empty()
        || !par.matches.is_empty()
        || !par.bundles.is_empty()
        || !par.connectives.is_empty()
        || !par.conditionals.is_empty()
    {
        return None;
    }
    if par.unforgeables.len() != 1 {
        return None;
    }
    match &par.unforgeables[0].unf_instance {
        Some(UnfInstance::GPrivateBody(gp)) => Some(&gp.id),
        _ => None,
    }
}

/// Pin the GPrivate id behind `par` in `registry`, returning an RAII handle.
///
/// Call from Rust-side code that predicts or otherwise holds a reference to
/// an unforgeable name — for example an exploratory deploy that computes its
/// return channel via `Tools::unforgeable_name_rng` and later reads from it.
/// While the handle is live, `sweep_channels` on the same space skips this
/// id even if it was classified as scope-trapped by `is_scope_trapped`.
///
/// Returns `None` when `par` is not a bare `GPrivate` (the only channel
/// shape today's sweep considers). Callers who care about richer channel
/// shapes should treat `None` as "nothing to pin" and proceed — the sweep
/// would not have targeted those shapes anyway.
pub fn pin_unforgeable(
    registry: &std::sync::Arc<rspace_plus_plus::rspace::gc_roots::GcRootRegistry>,
    par: &Par,
) -> Option<rspace_plus_plus::rspace::gc_roots::GcRootHandle> {
    gprivate_bytes_if_bare(par).map(|id| registry.pin_by_key(id.to_vec()))
}

/// Returns `true` iff `par` is a bare `GPrivate` whose id is currently pinned
/// in `registry`. Mirrors `pin_unforgeable`'s acceptance predicate: non-bare
/// pars return `false` (they can't be pinned, so they can't be "pinned").
pub fn is_unforgeable_pinned(
    registry: &rspace_plus_plus::rspace::gc_roots::GcRootRegistry,
    par: &Par,
) -> bool {
    gprivate_bytes_if_bare(par).is_some_and(|id| registry.is_pinned_by_key(id))
}

#[cfg(test)]
mod tests;
