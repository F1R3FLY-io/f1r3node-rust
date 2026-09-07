// See models/src/main/scala/coop/rchain/models/rholang/implicits.scala

use prost::Message;
use uuid::Uuid;

use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{Bundle, Expr, GPrivate, GUnforgeable, Par};
use crate::rust::utils::union;

// Somehow they are not initializing 'locally_free' and 'connective_used' fields
pub fn vector_par(_locally_free: Vec<u8>, _connective_used: bool) -> Par {
    Par {
        sends: Vec::new(),
        receives: Vec::new(),
        news: Vec::new(),
        exprs: Vec::new(),
        matches: Vec::new(),
        unforgeables: Vec::new(),
        bundles: Vec::new(),
        connectives: Vec::new(),
        conditionals: Vec::new(),
        locally_free: _locally_free,
        connective_used: _connective_used,
    }
}

pub struct GPrivateBuilder;

impl GPrivateBuilder {
    pub fn new_par() -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: Uuid::new_v4().to_string().encode_to_vec(),
            })),
        }])
    }

    pub fn new_par_from_string(s: String) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: s.encode_to_vec(),
            })),
        }])
    }
}

pub fn single_expr(p: &Par) -> Option<Expr> {
    if p.sends.is_empty()
        && p.receives.is_empty()
        && p.news.is_empty()
        && p.matches.is_empty()
        && p.bundles.is_empty()
    {
        match &p.exprs {
            vec if vec.len() == 1 => vec.first().cloned(),
            _ => None,
        }
    } else {
        None
    }
}

pub fn single_bundle(p: &Par) -> Option<Bundle> {
    if p.sends.is_empty()
        && p.receives.is_empty()
        && p.news.is_empty()
        && p.exprs.is_empty()
        && p.matches.is_empty()
        && p.unforgeables.is_empty()
        && p.connectives.is_empty()
    {
        match &p.bundles {
            vec if vec.len() == 1 => vec.first().cloned(),
            _ => None,
        }
    } else {
        None
    }
}

pub fn single_unforgeable(p: &Par) -> Option<GUnforgeable> {
    if p.sends.is_empty()
        && p.receives.is_empty()
        && p.news.is_empty()
        && p.exprs.is_empty()
        && p.matches.is_empty()
        && p.bundles.is_empty()
        && p.connectives.is_empty()
    {
        match &p.unforgeables {
            vec if vec.len() == 1 => vec.first().cloned(),
            _ => None,
        }
    } else {
        None
    }
}

pub fn concatenate_pars(p: Par, that: Par) -> Par {
    Par {
        sends: that.sends.into_iter().chain(p.sends).collect(),
        receives: that.receives.into_iter().chain(p.receives).collect(),
        news: that.news.into_iter().chain(p.news).collect(),
        exprs: that.exprs.into_iter().chain(p.exprs).collect(),
        matches: that.matches.into_iter().chain(p.matches).collect(),
        unforgeables: that
            .unforgeables
            .into_iter()
            .chain(p.unforgeables)
            .collect(),
        bundles: that.bundles.into_iter().chain(p.bundles).collect(),
        connectives: that.connectives.into_iter().chain(p.connectives).collect(),
        conditionals: that
            .conditionals
            .into_iter()
            .chain(p.conditionals)
            .collect(),
        locally_free: union(that.locally_free, p.locally_free),
        connective_used: that.connective_used || p.connective_used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::Send;
    use crate::rust::utils::{new_bundle_par, new_gint_par};

    fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    #[test]
    fn vector_par_sets_only_the_metadata_fields() {
        let par = vector_par(vec![1, 2], true);
        assert!(par.is_nil());
        assert_eq!(par.locally_free, vec![1, 2]);
        assert!(par.connective_used);
    }

    #[test]
    fn gprivate_builder_generates_distinct_ids() {
        let a = GPrivateBuilder::new_par();
        let b = GPrivateBuilder::new_par();
        assert_eq!(a.unforgeables.len(), 1);
        assert_ne!(a.unforgeables, b.unforgeables);
    }

    #[test]
    fn gprivate_builder_from_string_is_deterministic() {
        let a = GPrivateBuilder::new_par_from_string("id".to_string());
        let b = GPrivateBuilder::new_par_from_string("id".to_string());
        assert_eq!(a, b);
        match &a.unforgeables[0].unf_instance {
            Some(UnfInstance::GPrivateBody(gp)) => {
                assert_eq!(gp.id, "id".to_string().encode_to_vec())
            }
            other => panic!("expected GPrivateBody, got {:?}", other),
        }
    }

    #[test]
    fn single_expr_requires_exactly_one_expr_and_nothing_else() {
        let par = gint(1);
        assert_eq!(single_expr(&par), Some(par.exprs[0].clone()));

        let two_exprs = par.with_exprs(vec![par.exprs[0].clone(), par.exprs[0].clone()]);
        assert_eq!(single_expr(&two_exprs), None);

        let with_send = par.with_sends(vec![Send::default()]);
        assert_eq!(single_expr(&with_send), None);
    }

    #[test]
    fn single_bundle_requires_exactly_one_bundle_and_nothing_else() {
        let bundle_par = new_bundle_par(gint(1), true, false);
        assert_eq!(
            single_bundle(&bundle_par),
            Some(bundle_par.bundles[0].clone())
        );

        let mixed = bundle_par.with_exprs(gint(2).exprs);
        assert_eq!(single_bundle(&mixed), None);
        assert_eq!(single_bundle(&Par::default()), None);
    }

    #[test]
    fn single_unforgeable_requires_exactly_one_unforgeable_and_nothing_else() {
        let unf_par = GPrivateBuilder::new_par();
        assert_eq!(
            single_unforgeable(&unf_par),
            Some(unf_par.unforgeables[0].clone())
        );

        let mixed = unf_par.with_exprs(gint(1).exprs);
        assert_eq!(single_unforgeable(&mixed), None);
        assert_eq!(single_unforgeable(&Par::default()), None);
    }

    #[test]
    fn concatenate_pars_puts_second_argument_first_and_merges_metadata() {
        let mut first = gint(1).with_locally_free(vec![0b01]);
        first.connective_used = false;
        let mut second = gint(2).with_locally_free(vec![0b10]);
        second.connective_used = true;

        let combined = concatenate_pars(first.clone(), second.clone());
        assert_eq!(combined.exprs, vec![
            second.exprs[0].clone(),
            first.exprs[0].clone()
        ]);
        assert_eq!(combined.locally_free, vec![0b11]);
        assert!(combined.connective_used);
    }

    #[test]
    fn concatenate_pars_keeps_all_process_lists() {
        let sends = Par::default().with_sends(vec![Send::default()]);
        let combined = concatenate_pars(sends, GPrivateBuilder::new_par());
        assert_eq!(combined.sends.len(), 1);
        assert_eq!(combined.unforgeables.len(), 1);
    }
}
