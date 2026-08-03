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

pub fn concatenate_pars(mut p: Par, mut that: Par) -> Par {
    Par {
        sends: std::mem::take(&mut that.sends)
            .into_iter()
            .chain(std::mem::take(&mut p.sends))
            .collect(),
        receives: std::mem::take(&mut that.receives)
            .into_iter()
            .chain(std::mem::take(&mut p.receives))
            .collect(),
        news: std::mem::take(&mut that.news)
            .into_iter()
            .chain(std::mem::take(&mut p.news))
            .collect(),
        exprs: std::mem::take(&mut that.exprs)
            .into_iter()
            .chain(std::mem::take(&mut p.exprs))
            .collect(),
        matches: std::mem::take(&mut that.matches)
            .into_iter()
            .chain(std::mem::take(&mut p.matches))
            .collect(),
        unforgeables: std::mem::take(&mut that.unforgeables)
            .into_iter()
            .chain(std::mem::take(&mut p.unforgeables))
            .collect(),
        bundles: std::mem::take(&mut that.bundles)
            .into_iter()
            .chain(std::mem::take(&mut p.bundles))
            .collect(),
        connectives: std::mem::take(&mut that.connectives)
            .into_iter()
            .chain(std::mem::take(&mut p.connectives))
            .collect(),
        conditionals: std::mem::take(&mut that.conditionals)
            .into_iter()
            .chain(std::mem::take(&mut p.conditionals))
            .collect(),
        locally_free: union(
            std::mem::take(&mut that.locally_free),
            std::mem::take(&mut p.locally_free),
        ),
        connective_used: that.connective_used || p.connective_used,
    }
}
