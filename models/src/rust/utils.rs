use std::collections::BTreeMap;

use expr::ExprInstance;
use serde::{Deserialize, Serialize};

use super::par_map::ParMap;
use super::par_map_type_mapper::ParMapTypeMapper;
use super::par_set::ParSet;
use super::par_set_type_mapper::ParSetTypeMapper;
use super::rholang::implicits::vector_par;
use crate::create_bit_vector;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::*;
use crate::rust::utils::connective::ConnectiveInstance::*;
use crate::rust::utils::expr::ExprInstance::{EVarBody, *};
use crate::rust::utils::var::VarInstance::{BoundVar, FreeVar, Wildcard};
use crate::rust::utils::var::WildcardMsg;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OptionResult<A, K> {
    pub continuation: K,
    pub data: A,
}

// Adding helper functions 'with_*' to protobuf message 'Par'
impl Par {
    pub fn with_sends(&self, new_sends: Vec<Send>) -> Par {
        Par {
            sends: new_sends,
            ..self.clone()
        }
    }

    pub fn with_receives(&self, new_receives: Vec<Receive>) -> Par {
        Par {
            receives: new_receives,
            ..self.clone()
        }
    }

    pub fn with_news(&self, new_news: Vec<New>) -> Par {
        Par {
            news: new_news,
            ..self.clone()
        }
    }

    pub fn with_exprs(&self, new_exprs: Vec<Expr>) -> Par {
        Par {
            exprs: new_exprs,
            ..self.clone()
        }
    }

    pub fn with_matches(&self, new_matches: Vec<Match>) -> Par {
        Par {
            matches: new_matches,
            ..self.clone()
        }
    }

    pub fn with_bundles(&self, new_bundles: Vec<Bundle>) -> Par {
        Par {
            bundles: new_bundles,
            ..self.clone()
        }
    }

    pub fn with_unforgeables(&self, new_unforgeables: Vec<GUnforgeable>) -> Par {
        Par {
            unforgeables: new_unforgeables,
            ..self.clone()
        }
    }

    pub fn with_connectives(&self, new_connectives: Vec<Connective>) -> Par {
        Par {
            connectives: new_connectives,
            ..self.clone()
        }
    }

    pub fn with_locally_free(&self, new_locally_free: Vec<u8>) -> Par {
        Par {
            locally_free: new_locally_free,
            ..self.clone()
        }
    }

    pub fn with_connective_used(&self, new_connective_used: bool) -> Par {
        Par {
            connective_used: new_connective_used,
            ..self.clone()
        }
    }

    // See models/src/main/scala/coop/rchain/models/rholang/implicits.scala - prepend
    pub fn prepend_send(&mut self, s: Send) -> Par {
        let mut new_sends = vec![s.clone()];
        new_sends.append(&mut self.sends);

        Par {
            sends: new_sends,
            locally_free: union(self.locally_free.clone(), s.locally_free),
            connective_used: self.connective_used || s.connective_used,
            ..self.clone()
        }
    }

    pub fn prepend_receive(&mut self, r: Receive) -> Par {
        let mut new_receives = vec![r.clone()];
        new_receives.append(&mut self.receives);

        Par {
            receives: new_receives,
            locally_free: union(self.locally_free.clone(), r.locally_free),
            connective_used: self.connective_used || r.connective_used,
            ..self.clone()
        }
    }

    pub fn prepend_match(&mut self, m: Match) -> Par {
        let mut new_matches = vec![m.clone()];
        new_matches.append(&mut self.matches);

        Par {
            matches: new_matches,
            locally_free: union(self.locally_free.clone(), m.locally_free),
            connective_used: self.connective_used || m.connective_used,
            ..self.clone()
        }
    }

    pub fn prepend_if(&mut self, i: If) -> Par {
        let mut new_conditionals = vec![i.clone()];
        new_conditionals.append(&mut self.conditionals);

        Par {
            conditionals: new_conditionals,
            locally_free: union(self.locally_free.clone(), i.locally_free),
            connective_used: self.connective_used || i.connective_used,
            ..self.clone()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sends.is_empty()
            && self.receives.is_empty()
            && self.news.is_empty()
            && self.matches.is_empty()
            && self.bundles.is_empty()
            && self.exprs.is_empty()
    }

    pub fn is_nil(&self) -> bool {
        self.sends.is_empty()
            && self.receives.is_empty()
            && self.news.is_empty()
            && self.matches.is_empty()
            && self.bundles.is_empty()
            && self.unforgeables.is_empty()
            && self.connectives.is_empty()
            && self.exprs.is_empty()
    }

    pub fn single_connective(&self) -> Option<Connective> {
        if self.sends.is_empty()
            && self.receives.is_empty()
            && self.news.is_empty()
            && self.exprs.is_empty()
            && self.matches.is_empty()
            && self.bundles.is_empty()
            && self.connectives.len() == 1
        {
            Some(self.connectives[0].clone())
        } else {
            None
        }
    }

    pub fn single_bundle(&self) -> Option<Bundle> {
        if self.sends.is_empty()
            && self.receives.is_empty()
            && self.news.is_empty()
            && self.exprs.is_empty()
            && self.matches.is_empty()
            && self.unforgeables.is_empty()
            && self.connectives.is_empty()
        {
            match self.bundles.as_slice() {
                [single] => Some(single.clone()),
                _ => None,
            }
        } else {
            None
        }
    }

    pub fn append(&self, other: Par) -> Par {
        Par {
            sends: [self.sends.clone(), other.sends].concat(),
            receives: [self.receives.clone(), other.receives].concat(),
            news: [self.news.clone(), other.news].concat(),
            exprs: [self.exprs.clone(), other.exprs].concat(),
            matches: [self.matches.clone(), other.matches].concat(),
            unforgeables: [self.unforgeables.clone(), other.unforgeables].concat(),
            bundles: [self.bundles.clone(), other.bundles].concat(),
            connectives: [self.connectives.clone(), other.connectives].concat(),
            conditionals: [self.conditionals.clone(), other.conditionals].concat(),
            locally_free: union(self.locally_free.clone(), other.locally_free),
            connective_used: self.connective_used || other.connective_used,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/package.scala - FreeMap
pub type FreeMap = BTreeMap<i32, Par>;
pub fn new_free_map() -> FreeMap { BTreeMap::new() }

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/package.scala - runFirst
// STUBBED OUT
pub fn run_first<A>() -> Option<(FreeMap, A)> { None }

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/package.scala - attemptOpt
// NOT FULLY IMPLEMENTED
pub fn attempt_opt(operation: Option<()>) -> Option<()> { operation.map(|_| ()) }

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/storage/package.scala - toSeq
pub fn to_vec(fm: FreeMap, max: i32) -> Vec<Par> {
    (0..max)
        .map(|i| match fm.get(&i) {
            Some(par) => par.clone(),
            None => Par::default(),
        })
        .collect()
}

pub fn union(bitset1: Vec<u8>, bitset2: Vec<u8>) -> Vec<u8> {
    let max_len = bitset1.len().max(bitset2.len());
    let mut result = vec![0; max_len];

    for i in 0..max_len {
        let bit1 = if i < bitset1.len() { bitset1[i] } else { 0 };
        let bit2 = if i < bitset2.len() { bitset2[i] } else { 0 };
        result[i] = bit1 | bit2;
    }

    result
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParSpatialMatcherUtils.scala - noFrees[Par]
pub fn no_frees(par: &Par) -> Par { par.with_exprs(no_frees_exprs(&par.exprs)) }

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParSpatialMatcherUtils.scala - noFrees[Seq[Expr]]
pub fn no_frees_exprs(exprs: &[Expr]) -> Vec<Expr> {
    exprs
        .iter()
        .filter(|expr| match &expr.expr_instance {
            Some(EVarBody(EVar { v: Some(v) })) => match &v.var_instance {
                Some(FreeVar(_)) => false,
                Some(Wildcard(_)) => false,
                _ => true,
            },

            _ => true,
        })
        .cloned()
        .collect()
}

// See shared/src/main/scala/coop/rchain/catscontrib/Alternative_.scala - guard
pub fn guard(condition: bool) -> Option<()> {
    if condition {
        Some(())
    } else {
        None
    }
}

// Helper functions
pub fn new_conn_and_body_par(
    _ps: Vec<Par>,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_connectives(vec![Connective {
        connective_instance: Some(ConnAndBody(ConnectiveBody { ps: _ps })),
    }])
}

pub fn new_conn_or_body_par(
    _ps: Vec<Par>,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_connectives(vec![Connective {
        connective_instance: Some(ConnOrBody(ConnectiveBody { ps: _ps })),
    }])
}

pub fn new_conn_not_body_par(
    _body: Par,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_connectives(vec![Connective {
        connective_instance: Some(ConnNotBody(_body)),
    }])
}

pub fn new_send(
    _chan: Par,
    _data: Vec<Par>,
    _persistent: bool,
    _locally_free: Vec<u8>,
    _connective_used: bool,
) -> Send {
    Send {
        chan: Some(_chan),
        data: _data,
        persistent: _persistent,
        locally_free: _locally_free,
        connective_used: _connective_used,
    }
}

pub fn new_send_par(
    _chan: Par,
    _data: Vec<Par>,
    _persistent: bool,
    _locally_free: Vec<u8>,
    _connective_used: bool,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_sends(vec![Send {
        chan: Some(_chan),
        data: _data,
        persistent: _persistent,
        locally_free: _locally_free,
        connective_used: _connective_used,
    }])
}

pub fn new_match_par(
    _target: Par,
    _cases: Vec<MatchCase>,
    _locally_free: Vec<u8>,
    _connective_used: bool,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_matches(vec![Match {
        target: Some(_target),
        cases: _cases,
        locally_free: _locally_free,
        connective_used: _connective_used,
    }])
}

pub fn new_receive_par(
    _binds: Vec<ReceiveBind>,
    _body: Par,
    _persistent: bool,
    _peek: bool,
    _bind_count: i32,
    _locally_free: Vec<u8>,
    _connective_used: bool,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_receives(vec![Receive {
        binds: _binds,
        body: Some(_body),
        persistent: _persistent,
        peek: _peek,
        bind_count: _bind_count,
        locally_free: _locally_free,
        connective_used: _connective_used,
        condition: None,
    }])
}

pub fn new_new_par(
    _bind_count: i32,
    _p: Par,
    _uri: Vec<String>,
    _injections: BTreeMap<String, Par>,
    _locally_free: Vec<u8>,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_news(vec![New {
        bind_count: _bind_count,
        p: Some(_p),
        uri: _uri,
        injections: _injections,
        locally_free: _locally_free,
    }])
}

pub fn new_eset_par(
    _ps: Vec<Par>,
    _locally_free: Vec<u8>,
    _connective_used: bool,
    _remainder: Option<Var>,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_exprs(vec![new_eset_expr(
        _ps,
        _locally_free,
        _connective_used,
        _remainder,
    )])
}

pub fn new_eset_expr(
    _ps: Vec<Par>,
    _locally_free: Vec<u8>,
    _connective_used: bool,
    _remainder: Option<Var>,
) -> Expr {
    Expr {
        expr_instance: Some(ESetBody(ParSetTypeMapper::par_set_to_eset(ParSet::new(
            _ps,
            _connective_used,
            _locally_free,
            _remainder,
        )))),
    }
}

pub fn new_emap_par(
    _kvs: Vec<KeyValuePair>,
    _locally_free: Vec<u8>,
    _connective_used: bool,
    _remainder: Option<Var>,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_exprs(vec![new_emap_expr(
        _kvs,
        _locally_free,
        _connective_used,
        _remainder,
    )])
}

pub fn new_emap_expr(
    _kvs: Vec<KeyValuePair>,
    _locally_free: Vec<u8>,
    _connective_used: bool,
    _remainder: Option<Var>,
) -> Expr {
    Expr {
        expr_instance: Some(EMapBody(ParMapTypeMapper::par_map_to_emap(ParMap::new(
            _kvs.into_iter()
                .filter_map(|kv| {
                    if let (Some(key), Some(value)) = (kv.key, kv.value) {
                        Some((key, value))
                    } else {
                        None
                    }
                })
                .collect(),
            _connective_used,
            _locally_free,
            _remainder,
        )))),
    }
}

pub fn new_key_value_pair(_key: Par, _value: Par) -> KeyValuePair {
    KeyValuePair {
        key: Some(_key),
        value: Some(_value),
    }
}

pub fn new_gint_par(value: i64, _locally_free_par: Vec<u8>, _connective_used_par: bool) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_exprs(vec![new_gint_expr(value)])
}

pub fn new_gint_expr(value: i64) -> Expr {
    Expr {
        expr_instance: Some(GInt(value)),
    }
}

pub fn new_gbool_par(value: bool, _locally_free_par: Vec<u8>, _connective_used_par: bool) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_exprs(vec![new_gbool_expr(value)])
}

pub fn new_gbool_expr(value: bool) -> Expr {
    Expr {
        expr_instance: Some(GBool(value)),
    }
}

pub fn new_gstring_par(
    value: String,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_exprs(vec![new_gstring_expr(value)])
}

pub fn new_gstring_expr(value: String) -> Expr {
    Expr {
        expr_instance: Some(GString(value)),
    }
}

pub fn new_guri_par(value: String, _locally_free_par: Vec<u8>, _connective_used_par: bool) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_exprs(vec![new_guri_expr(value)])
}

pub fn new_guri_expr(value: String) -> Expr {
    Expr {
        expr_instance: Some(GUri(value)),
    }
}

pub fn new_gdouble_expr(value: f64) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::GDouble(value.to_bits())),
    }
}

pub fn new_gbigint_expr(bytes: Vec<u8>) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::GBigInt(bytes)),
    }
}

pub fn new_gbigrat_expr(numerator: Vec<u8>, denominator: Vec<u8>) -> Expr {
    use crate::rhoapi::GBigRational;
    Expr {
        expr_instance: Some(ExprInstance::GBigRat(GBigRational {
            numerator,
            denominator,
        })),
    }
}

pub fn new_gfixedpoint_expr(unscaled: Vec<u8>, scale: u32) -> Expr {
    use crate::rhoapi::GFixedPoint;
    Expr {
        expr_instance: Some(ExprInstance::GFixedPoint(GFixedPoint { unscaled, scale })),
    }
}

pub fn new_wildcard_par(_locally_free_par: Vec<u8>, _connective_used_par: bool) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_exprs(vec![Expr {
        expr_instance: Some(EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(Wildcard(WildcardMsg {})),
            }),
        })),
    }])
}

pub fn new_wildcard_expr() -> Expr {
    Expr {
        expr_instance: Some(EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(Wildcard(WildcardMsg {})),
            }),
        })),
    }
}

pub fn new_wildcard_var() -> Var {
    Var {
        var_instance: Some(Wildcard(WildcardMsg {})),
    }
}

pub fn new_boundvar_par(value: i32, _locally_free_par: Vec<u8>, _connective_used_par: bool) -> Par {
    vector_par(create_bit_vector(&[value as usize]), _connective_used_par)
        .with_exprs(vec![new_boundvar_expr(value)])
}

pub fn new_boundvar_expr(value: i32) -> Expr {
    Expr {
        expr_instance: Some(EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(BoundVar(value)),
            }),
        })),
    }
}

// "connective_used" is always "true" on "freevar"
pub fn new_freevar_par(value: i32, _locally_free_par: Vec<u8>) -> Par {
    vector_par(_locally_free_par, true).with_exprs(vec![new_freevar_expr(value)])
}

pub fn new_freevar_expr(value: i32) -> Expr {
    Expr {
        expr_instance: Some(EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(FreeVar(value)),
            }),
        })),
    }
}

pub fn new_freevar_var(value: i32) -> Var {
    Var {
        var_instance: Some(FreeVar(value)),
    }
}

pub fn new_elist_par(
    _ps: Vec<Par>,
    _locally_free: Vec<u8>,
    _connective_used_elist: bool,
    _remainder: Option<Var>,
    _locally_free_par: Vec<u8>,
    _connective_used_par: bool,
) -> Par {
    vector_par(_locally_free_par, _connective_used_par).with_exprs(vec![new_elist_expr(
        _ps,
        _locally_free,
        _connective_used_elist,
        _remainder,
    )])
}

pub fn new_elist_expr(
    _ps: Vec<Par>,
    _locally_free: Vec<u8>,
    _connective_used: bool,
    _remainder: Option<Var>,
) -> Expr {
    Expr {
        expr_instance: Some(EListBody(EList {
            ps: _ps,
            locally_free: _locally_free,
            connective_used: _connective_used,
            remainder: _remainder,
        })),
    }
}

pub fn new_etuple_par(_ps: Vec<Par>) -> Par {
    vector_par(Vec::new(), false).with_exprs(vec![new_etuple_expr(_ps, Vec::new(), false)])
}

pub fn new_etuple_expr(_ps: Vec<Par>, _locally_free: Vec<u8>, _connective_used: bool) -> Expr {
    Expr {
        expr_instance: Some(ETupleBody(ETuple {
            ps: _ps,
            locally_free: _locally_free,
            connective_used: _connective_used,
        })),
    }
}

pub fn new_eplus_par_gint(
    lhs_value: i64,
    rhs_value: i64,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPlusBody(EPlus {
            p1: Some(new_gint_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gint_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }])
}

pub fn new_eplus_par(lhs_value: Par, rhs_value: Par) -> Par {
    let locally_free = union(
        lhs_value.locally_free.clone(),
        rhs_value.locally_free.clone(),
    );
    let connective_used = lhs_value.connective_used || rhs_value.connective_used;

    Par::default()
        .with_exprs(vec![Expr {
            expr_instance: Some(EPlusBody(EPlus {
                p1: Some(lhs_value),
                p2: Some(rhs_value),
            })),
        }])
        .with_locally_free(locally_free)
        .with_connective_used(connective_used)
}

pub fn new_bundle_par(body: Par, write_flag: bool, read_flag: bool) -> Par {
    Par::default().with_bundles(vec![Bundle {
        body: Some(body),
        write_flag,
        read_flag,
    }])
}

pub fn new_eminus_expr_gint(
    lhs_value: i64,
    rhs_value: i64,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(EMinusBody(EMinus {
            p1: Some(new_gint_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gint_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_ediv_expr_gint(
    lhs_value: i64,
    rhs_value: i64,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(EDivBody(EDiv {
            p1: Some(new_gint_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gint_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_eplus_expr_gint(
    lhs_value: i64,
    rhs_value: i64,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(EPlusBody(EPlus {
            p1: Some(new_gint_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gint_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_emult_expr_gint(
    lhs_value: i64,
    rhs_value: i64,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(EMultBody(EMult {
            p1: Some(new_gint_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gint_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_eeq_expr_gint(
    lhs_value: i64,
    rhs_value: i64,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(EEqBody(EEq {
            p1: Some(new_gint_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gint_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_eneq_expr_gint(
    lhs_value: i64,
    rhs_value: i64,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(ENeqBody(ENeq {
            p1: Some(new_gint_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gint_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_elt_expr_gint(
    lhs_value: i64,
    rhs_value: i64,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(ELtBody(ELt {
            p1: Some(new_gint_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gint_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_elte_expr_gint(
    lhs_value: i64,
    rhs_value: i64,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(ELteBody(ELte {
            p1: Some(new_gint_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gint_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_egt_expr_gbool(
    lhs_value: bool,
    rhs_value: bool,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(EGtBody(EGt {
            p1: Some(new_gbool_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gbool_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_egte_expr_gbool(
    lhs_value: bool,
    rhs_value: bool,
    locally_free_par: Vec<u8>,
    connective_used_par: bool,
) -> Expr {
    Expr {
        expr_instance: Some(EGteBody(EGte {
            p1: Some(new_gbool_par(
                lhs_value,
                locally_free_par.clone(),
                connective_used_par,
            )),
            p2: Some(new_gbool_par(
                rhs_value,
                locally_free_par,
                connective_used_par,
            )),
        })),
    }
}

pub fn new_eor_expr(lhs: Par, rhs: Par) -> Expr {
    Expr {
        expr_instance: Some(EOrBody(EOr {
            p1: Some(lhs),
            p2: Some(rhs),
        })),
    }
}

pub fn new_emethod_expr(
    method_name: String,
    target: Par,
    arguments: Vec<Par>,
    locally_free: Vec<u8>,
) -> Expr {
    Expr {
        expr_instance: Some(EMethodBody(EMethod {
            method_name,
            target: Some(target),
            arguments,
            locally_free,
            connective_used: false,
        })),
    }
}

pub fn new_par_from_par_set(
    elements: Vec<Par>,
    locally_free: Vec<u8>,
    connective_used: bool,
    remainder: Option<Var>,
) -> Par {
    let par_set = ParSet::new(elements, connective_used, locally_free, remainder);

    Par {
        exprs: vec![Expr {
            expr_instance: Some(ESetBody(ParSetTypeMapper::par_set_to_eset(par_set))),
        }],
        ..Default::default()
    }
}

pub fn new_gbytearray_par(bytes: Vec<u8>, locally_free: Vec<u8>, connective_used: bool) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(GByteArray(bytes)),
        }],
        locally_free,
        connective_used,
        ..Default::default()
    }
}

pub fn new_gsys_auth_token_par(locally_free: Vec<u8>, connective_used: bool) -> Par {
    Par {
        unforgeables: vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
        }],
        locally_free,
        connective_used,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn expr_of(par: &Par) -> &ExprInstance { par.exprs[0].expr_instance.as_ref().unwrap() }

    #[test]
    fn with_helpers_replace_only_their_field() {
        let base = gint(1)
            .with_locally_free(vec![9])
            .with_connective_used(true);
        assert_eq!(base.exprs.len(), 1);
        assert_eq!(base.locally_free, vec![9]);
        assert!(base.connective_used);

        let cleared = base.with_exprs(vec![]);
        assert!(cleared.exprs.is_empty());
        assert_eq!(cleared.locally_free, vec![9]);

        let with_send = base.with_sends(vec![Send::default()]);
        assert_eq!(with_send.sends.len(), 1);
        assert_eq!(with_send.exprs.len(), 1);

        assert_eq!(
            base.with_receives(vec![Receive::default()]).receives.len(),
            1
        );
        assert_eq!(base.with_news(vec![New::default()]).news.len(), 1);
        assert_eq!(base.with_matches(vec![Match::default()]).matches.len(), 1);
        assert_eq!(base.with_bundles(vec![Bundle::default()]).bundles.len(), 1);
        assert_eq!(
            base.with_unforgeables(vec![GUnforgeable::default()])
                .unforgeables
                .len(),
            1
        );
        assert_eq!(
            base.with_connectives(vec![Connective::default()])
                .connectives
                .len(),
            1
        );
    }

    #[test]
    fn prepend_send_puts_new_send_first_and_merges_metadata() {
        let existing = Send {
            chan: Some(gint(1)),
            ..Default::default()
        };
        let mut par = Par::default()
            .with_sends(vec![existing.clone()])
            .with_locally_free(vec![0b01]);
        let new_send = Send {
            chan: Some(gint(2)),
            locally_free: vec![0b10],
            connective_used: true,
            ..Default::default()
        };

        let result = par.prepend_send(new_send.clone());
        assert_eq!(result.sends, vec![new_send, existing]);
        assert_eq!(result.locally_free, vec![0b11]);
        assert!(result.connective_used);
    }

    #[test]
    fn prepend_receive_match_and_if_merge_metadata() {
        let receive = Receive {
            locally_free: vec![0b10],
            connective_used: true,
            ..Default::default()
        };
        let from_receive = Par::default()
            .with_locally_free(vec![0b01])
            .prepend_receive(receive.clone());
        assert_eq!(from_receive.receives, vec![receive]);
        assert_eq!(from_receive.locally_free, vec![0b11]);
        assert!(from_receive.connective_used);

        let match_ = Match {
            locally_free: vec![0b100],
            ..Default::default()
        };
        let from_match = Par::default().prepend_match(match_.clone());
        assert_eq!(from_match.matches, vec![match_]);
        assert_eq!(from_match.locally_free, vec![0b100]);

        let conditional = If {
            locally_free: vec![0b1000],
            ..Default::default()
        };
        let from_if = Par::default().prepend_if(conditional.clone());
        assert_eq!(from_if.conditionals, vec![conditional]);
        assert_eq!(from_if.locally_free, vec![0b1000]);
    }

    #[test]
    fn is_empty_and_is_nil_differ_on_unforgeables_and_connectives() {
        assert!(Par::default().is_empty());
        assert!(Par::default().is_nil());

        let with_unf = new_gsys_auth_token_par(Vec::new(), false);
        assert!(with_unf.is_empty());
        assert!(!with_unf.is_nil());

        let with_conn = Par::default().with_connectives(vec![Connective::default()]);
        assert!(with_conn.is_empty());
        assert!(!with_conn.is_nil());

        assert!(!gint(1).is_empty());
        assert!(!gint(1).is_nil());
    }

    #[test]
    fn single_connective_requires_lone_connective() {
        let lone = Par::default().with_connectives(vec![Connective::default()]);
        assert_eq!(lone.single_connective(), Some(Connective::default()));

        let two =
            Par::default().with_connectives(vec![Connective::default(), Connective::default()]);
        assert_eq!(two.single_connective(), None);

        let mixed = gint(1).with_connectives(vec![Connective::default()]);
        assert_eq!(mixed.single_connective(), None);
    }

    #[test]
    fn single_bundle_requires_lone_bundle() {
        let bundle = Bundle {
            body: Some(gint(1)),
            write_flag: true,
            read_flag: false,
        };
        let lone = Par::default().with_bundles(vec![bundle.clone()]);
        assert_eq!(lone.single_bundle(), Some(bundle.clone()));

        let two = Par::default().with_bundles(vec![bundle.clone(), bundle.clone()]);
        assert_eq!(two.single_bundle(), None);
        assert_eq!(gint(1).with_bundles(vec![bundle]).single_bundle(), None);
    }

    #[test]
    fn append_concatenates_processes_and_merges_metadata() {
        let left = gint(1)
            .with_locally_free(vec![0b01])
            .with_sends(vec![Send::default()])
            .with_exprs(gint(1).exprs);
        let mut right = gint(2).with_locally_free(vec![0b10]);
        right.connective_used = true;

        let combined = left.append(right);
        assert_eq!(combined.sends.len(), 1);
        assert_eq!(combined.exprs, vec![
            gint(1).exprs[0].clone(),
            gint(2).exprs[0].clone()
        ]);
        assert_eq!(combined.locally_free, vec![0b11]);
        assert!(combined.connective_used);
    }

    #[test]
    fn union_ors_bitsets_of_unequal_length() {
        assert_eq!(union(vec![0b01], vec![0b10, 0b100]), vec![0b11, 0b100]);
        assert_eq!(union(vec![], vec![]), Vec::<u8>::new());
        assert_eq!(union(vec![7], vec![]), vec![7]);
    }

    #[test]
    fn to_vec_fills_missing_indices_with_default_par() {
        let mut free_map = new_free_map();
        free_map.insert(1, gint(10));
        let result = to_vec(free_map, 3);
        assert_eq!(result, vec![Par::default(), gint(10), Par::default()]);
    }

    #[test]
    fn guard_and_attempt_opt_and_run_first() {
        assert_eq!(guard(true), Some(()));
        assert_eq!(guard(false), None);
        assert_eq!(attempt_opt(Some(())), Some(()));
        assert_eq!(attempt_opt(None), None);
        assert!(run_first::<i32>().is_none());
    }

    #[test]
    fn no_frees_drops_free_vars_and_wildcards_only() {
        let exprs = vec![
            new_gint_expr(1),
            new_freevar_expr(0),
            new_wildcard_expr(),
            new_boundvar_expr(2),
        ];
        let par = Par::default().with_exprs(exprs);
        let filtered = no_frees(&par);
        assert_eq!(filtered.exprs, vec![new_gint_expr(1), new_boundvar_expr(2)]);
    }

    #[test]
    fn ground_constructors_wrap_expected_expr_instances() {
        assert_eq!(expr_of(&gint(42)), &GInt(42));
        assert_eq!(
            expr_of(&new_gbool_par(true, Vec::new(), false)),
            &GBool(true)
        );
        assert_eq!(
            expr_of(&new_gstring_par("s".to_string(), Vec::new(), false)),
            &GString("s".to_string())
        );
        assert_eq!(
            expr_of(&new_guri_par("uri".to_string(), Vec::new(), false)),
            &GUri("uri".to_string())
        );
        assert_eq!(
            new_gdouble_expr(1.5).expr_instance,
            Some(ExprInstance::GDouble(1.5f64.to_bits()))
        );
        assert_eq!(
            new_gbigint_expr(vec![1, 2]).expr_instance,
            Some(ExprInstance::GBigInt(vec![1, 2]))
        );
        assert_eq!(
            new_gbigrat_expr(vec![1], vec![2]).expr_instance,
            Some(ExprInstance::GBigRat(GBigRational {
                numerator: vec![1],
                denominator: vec![2],
            }))
        );
        assert_eq!(
            new_gfixedpoint_expr(vec![5], 2).expr_instance,
            Some(ExprInstance::GFixedPoint(crate::rhoapi::GFixedPoint {
                unscaled: vec![5],
                scale: 2,
            }))
        );
    }

    #[test]
    fn var_constructors_build_expected_var_instances() {
        assert_eq!(
            new_wildcard_var().var_instance,
            Some(Wildcard(WildcardMsg {}))
        );
        assert_eq!(new_freevar_var(3).var_instance, Some(FreeVar(3)));
        assert_eq!(
            expr_of(&new_wildcard_par(Vec::new(), false)),
            &new_wildcard_expr().expr_instance.unwrap()
        );

        let bound = new_boundvar_par(3, Vec::new(), false);
        assert_eq!(
            expr_of(&bound),
            &new_boundvar_expr(3).expr_instance.unwrap()
        );
        assert_eq!(bound.locally_free, create_bit_vector(&[3]));

        let free = new_freevar_par(1, Vec::new());
        assert_eq!(expr_of(&free), &new_freevar_expr(1).expr_instance.unwrap());
        assert!(free.connective_used);
    }

    #[test]
    fn collection_constructors_wrap_elements() {
        let elist = new_elist_par(
            vec![gint(1), gint(2)],
            vec![1],
            true,
            Some(new_freevar_var(0)),
            Vec::new(),
            false,
        );
        match expr_of(&elist) {
            EListBody(list) => {
                assert_eq!(list.ps, vec![gint(1), gint(2)]);
                assert_eq!(list.locally_free, vec![1]);
                assert!(list.connective_used);
                assert_eq!(list.remainder, Some(new_freevar_var(0)));
            }
            other => panic!("expected EListBody, got {:?}", other),
        }

        let etuple = new_etuple_par(vec![gint(1)]);
        match expr_of(&etuple) {
            ETupleBody(tuple) => assert_eq!(tuple.ps, vec![gint(1)]),
            other => panic!("expected ETupleBody, got {:?}", other),
        }

        let eset = new_eset_par(
            vec![gint(2), gint(1)],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        );
        match expr_of(&eset) {
            ESetBody(set) => assert_eq!(set.ps.len(), 2),
            other => panic!("expected ESetBody, got {:?}", other),
        }

        let emap = new_emap_par(
            vec![new_key_value_pair(gint(1), gint(10)), KeyValuePair {
                key: None,
                value: Some(gint(20)),
            }],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        );
        match expr_of(&emap) {
            EMapBody(map) => assert_eq!(map.kvs.len(), 1),
            other => panic!("expected EMapBody, got {:?}", other),
        }
    }

    #[test]
    fn new_par_from_par_set_builds_eset_par() {
        let par = new_par_from_par_set(vec![gint(1)], Vec::new(), false, None);
        match expr_of(&par) {
            ESetBody(set) => assert_eq!(set.ps, vec![gint(1)]),
            other => panic!("expected ESetBody, got {:?}", other),
        }
    }

    #[test]
    fn binary_gint_expr_constructors_wrap_both_operands() {
        let cases: Vec<(Expr, &str)> = vec![
            (new_eplus_expr_gint(1, 2, Vec::new(), false), "plus"),
            (new_eminus_expr_gint(1, 2, Vec::new(), false), "minus"),
            (new_emult_expr_gint(1, 2, Vec::new(), false), "mult"),
            (new_ediv_expr_gint(1, 2, Vec::new(), false), "div"),
            (new_eeq_expr_gint(1, 2, Vec::new(), false), "eq"),
            (new_eneq_expr_gint(1, 2, Vec::new(), false), "neq"),
            (new_elt_expr_gint(1, 2, Vec::new(), false), "lt"),
            (new_elte_expr_gint(1, 2, Vec::new(), false), "lte"),
        ];
        for (expr, name) in cases {
            let (p1, p2) = match expr.expr_instance.unwrap() {
                EPlusBody(EPlus { p1, p2 }) => (p1, p2),
                EMinusBody(EMinus { p1, p2 }) => (p1, p2),
                EMultBody(EMult { p1, p2 }) => (p1, p2),
                EDivBody(EDiv { p1, p2 }) => (p1, p2),
                EEqBody(EEq { p1, p2 }) => (p1, p2),
                ENeqBody(ENeq { p1, p2 }) => (p1, p2),
                ELtBody(ELt { p1, p2 }) => (p1, p2),
                ELteBody(ELte { p1, p2 }) => (p1, p2),
                other => panic!("unexpected instance for {}: {:?}", name, other),
            };
            assert_eq!(p1.unwrap(), gint(1), "lhs of {}", name);
            assert_eq!(p2.unwrap(), gint(2), "rhs of {}", name);
        }
    }

    #[test]
    fn gbool_comparison_constructors_wrap_both_operands() {
        match new_egt_expr_gbool(true, false, Vec::new(), false).expr_instance {
            Some(EGtBody(EGt { p1, p2 })) => {
                assert_eq!(p1.unwrap(), new_gbool_par(true, Vec::new(), false));
                assert_eq!(p2.unwrap(), new_gbool_par(false, Vec::new(), false));
            }
            other => panic!("expected EGtBody, got {:?}", other),
        }
        match new_egte_expr_gbool(false, true, Vec::new(), false).expr_instance {
            Some(EGteBody(EGte { p1, p2 })) => {
                assert_eq!(p1.unwrap(), new_gbool_par(false, Vec::new(), false));
                assert_eq!(p2.unwrap(), new_gbool_par(true, Vec::new(), false));
            }
            other => panic!("expected EGteBody, got {:?}", other),
        }
    }

    #[test]
    fn eplus_par_constructors_merge_operand_metadata() {
        let direct = new_eplus_par_gint(1, 2, Vec::new(), false);
        match expr_of(&direct) {
            EPlusBody(EPlus { p1, p2 }) => {
                assert_eq!(p1.as_ref().unwrap(), &gint(1));
                assert_eq!(p2.as_ref().unwrap(), &gint(2));
            }
            other => panic!("expected EPlusBody, got {:?}", other),
        }

        let mut rhs = gint(2).with_locally_free(vec![0b10]);
        rhs.connective_used = true;
        let combined = new_eplus_par(gint(1).with_locally_free(vec![0b01]), rhs);
        assert_eq!(combined.locally_free, vec![0b11]);
        assert!(combined.connective_used);
    }

    #[test]
    fn misc_expr_constructors_carry_their_arguments() {
        match new_eor_expr(gint(1), gint(2)).expr_instance {
            Some(EOrBody(EOr { p1, p2 })) => {
                assert_eq!(p1.unwrap(), gint(1));
                assert_eq!(p2.unwrap(), gint(2));
            }
            other => panic!("expected EOrBody, got {:?}", other),
        }

        match new_emethod_expr("nth".to_string(), gint(1), vec![gint(0)], vec![1]).expr_instance {
            Some(EMethodBody(method)) => {
                assert_eq!(method.method_name, "nth");
                assert_eq!(method.target.unwrap(), gint(1));
                assert_eq!(method.arguments, vec![gint(0)]);
                assert_eq!(method.locally_free, vec![1]);
                assert!(!method.connective_used);
            }
            other => panic!("expected EMethodBody, got {:?}", other),
        }
    }

    #[test]
    fn bundle_bytearray_and_sys_auth_token_constructors() {
        let bundle = new_bundle_par(gint(1), true, false);
        assert_eq!(bundle.bundles, vec![Bundle {
            body: Some(gint(1)),
            write_flag: true,
            read_flag: false,
        }]);

        let bytes = new_gbytearray_par(vec![1, 2], vec![3], true);
        assert_eq!(expr_of(&bytes), &GByteArray(vec![1, 2]));
        assert_eq!(bytes.locally_free, vec![3]);
        assert!(bytes.connective_used);

        let token = new_gsys_auth_token_par(vec![4], true);
        assert_eq!(
            token.unforgeables[0].unf_instance,
            Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {}))
        );
        assert_eq!(token.locally_free, vec![4]);
    }

    #[test]
    fn connective_constructors_wrap_bodies() {
        let and = new_conn_and_body_par(vec![gint(1)], vec![1], true);
        assert_eq!(
            and.connectives[0].connective_instance,
            Some(ConnAndBody(ConnectiveBody { ps: vec![gint(1)] }))
        );
        assert_eq!(and.locally_free, vec![1]);
        assert!(and.connective_used);

        let or = new_conn_or_body_par(vec![gint(2)], Vec::new(), false);
        assert_eq!(
            or.connectives[0].connective_instance,
            Some(ConnOrBody(ConnectiveBody { ps: vec![gint(2)] }))
        );

        let not = new_conn_not_body_par(gint(3), Vec::new(), false);
        assert_eq!(
            not.connectives[0].connective_instance,
            Some(ConnNotBody(gint(3)))
        );
    }

    #[test]
    fn process_constructors_populate_wrapped_structs() {
        let send = new_send(gint(1), vec![gint(2)], true, vec![1], true);
        assert_eq!(send.chan, Some(gint(1)));
        assert_eq!(send.data, vec![gint(2)]);
        assert!(send.persistent);
        assert_eq!(send.locally_free, vec![1]);
        assert!(send.connective_used);

        let send_par = new_send_par(
            gint(1),
            vec![gint(2)],
            false,
            Vec::new(),
            false,
            vec![2],
            true,
        );
        assert_eq!(send_par.sends.len(), 1);
        assert_eq!(send_par.locally_free, vec![2]);
        assert!(send_par.connective_used);

        let match_par = new_match_par(
            gint(1),
            vec![MatchCase::default()],
            vec![1],
            true,
            Vec::new(),
            false,
        );
        assert_eq!(match_par.matches[0].target, Some(gint(1)));
        assert_eq!(match_par.matches[0].cases.len(), 1);

        let receive_par = new_receive_par(
            vec![ReceiveBind::default()],
            gint(9),
            true,
            true,
            2,
            vec![1],
            false,
            Vec::new(),
            false,
        );
        let receive = &receive_par.receives[0];
        assert_eq!(receive.binds.len(), 1);
        assert_eq!(receive.body, Some(gint(9)));
        assert!(receive.persistent);
        assert!(receive.peek);
        assert_eq!(receive.bind_count, 2);
        assert_eq!(receive.condition, None);

        let new_par = new_new_par(
            2,
            gint(5),
            vec!["rho:io:stdout".to_string()],
            BTreeMap::new(),
            vec![1],
            Vec::new(),
            false,
        );
        let new = &new_par.news[0];
        assert_eq!(new.bind_count, 2);
        assert_eq!(new.p, Some(gint(5)));
        assert_eq!(new.uri, vec!["rho:io:stdout".to_string()]);
    }
}
