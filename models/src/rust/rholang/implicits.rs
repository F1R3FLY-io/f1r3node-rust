// See models/src/main/scala/coop/rchain/models/rholang/implicits.scala

use prost::Message;
use uuid::Uuid;

use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{Bundle, Expr, GPrivate, GUnforgeable, Par};
use crate::rust::par_bag::ParBag;

// Somehow they are not initializing 'locally_free' and 'connective_used' fields
pub fn vector_par(_locally_free: Vec<u8>, _connective_used: bool) -> Par {
    Par::from_parts(ParBag::new(), _locally_free, _connective_used)
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
    if p.family_cardinality::<crate::rhoapi::Send>() == 0
        && p.family_cardinality::<crate::rhoapi::Receive>() == 0
        && p.family_cardinality::<crate::rhoapi::New>() == 0
        && p.family_cardinality::<crate::rhoapi::Match>() == 0
        && p.family_cardinality::<Bundle>() == 0
        && p.family_cardinality::<Expr>() == 1
    {
        p.first::<Expr>()
            .expect("Par invariant: stored Expr keys are canonical")
            .map(|(value, _)| value)
    } else {
        None
    }
}

pub fn single_bundle(p: &Par) -> Option<Bundle> { p.single_bundle() }

pub fn single_unforgeable(p: &Par) -> Option<GUnforgeable> {
    if p.family_cardinality::<crate::rhoapi::Send>() == 0
        && p.family_cardinality::<crate::rhoapi::Receive>() == 0
        && p.family_cardinality::<crate::rhoapi::New>() == 0
        && p.family_cardinality::<Expr>() == 0
        && p.family_cardinality::<crate::rhoapi::Match>() == 0
        && p.family_cardinality::<Bundle>() == 0
        && p.family_cardinality::<crate::rhoapi::Connective>() == 0
        && p.family_cardinality::<GUnforgeable>() == 1
    {
        p.first::<GUnforgeable>()
            .expect("Par invariant: stored GUnforgeable keys are canonical")
            .map(|(value, _)| value)
    } else {
        None
    }
}

pub fn concatenate_pars(p: Par, that: Par) -> Par {
    that.parallel(p)
        .expect("two finite Par values cannot overflow u64 multiplicity")
}
