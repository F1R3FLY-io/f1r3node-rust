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
        // LEG-1: `s` is OWNED by this function, so it is MOVED into the vector
        // rather than deep-cloned. `<Send as Clone>::clone` recurses through the
        // whole `Par` subtree — a Theta(depth) NATIVE-STACK traversal (2.78
        // KiB/level release) paid once per prepend. Its two cached fields are read
        // BEFORE the move. See docs/design/audits/theta-depth-traversals-2026-07-26.md.
        let locally_free = union(self.locally_free.clone(), s.locally_free.clone());
        let connective_used = self.connective_used || s.connective_used;

        let mut new_sends = Vec::with_capacity(self.sends.len() + 1);
        new_sends.push(s);
        new_sends.append(&mut self.sends);

        Par {
            sends: new_sends,
            locally_free,
            connective_used,
            ..self.clone()
        }
    }

    pub fn prepend_receive(&mut self, r: Receive) -> Par {
        // LEG-1: `r` is OWNED by this function, so it is MOVED into the vector
        // rather than deep-cloned. `<Receive as Clone>::clone` recurses through the
        // whole `Par` subtree — a Theta(depth) NATIVE-STACK traversal (2.78
        // KiB/level release) paid once per prepend. Its two cached fields are read
        // BEFORE the move. See docs/design/audits/theta-depth-traversals-2026-07-26.md.
        let locally_free = union(self.locally_free.clone(), r.locally_free.clone());
        let connective_used = self.connective_used || r.connective_used;

        let mut new_receives = Vec::with_capacity(self.receives.len() + 1);
        new_receives.push(r);
        new_receives.append(&mut self.receives);

        Par {
            receives: new_receives,
            locally_free,
            connective_used,
            ..self.clone()
        }
    }

    pub fn prepend_match(&mut self, m: Match) -> Par {
        // LEG-1: `m` is OWNED by this function, so it is MOVED into the vector
        // rather than deep-cloned. `<Match as Clone>::clone` recurses through the
        // whole `Par` subtree — a Theta(depth) NATIVE-STACK traversal (2.78
        // KiB/level release) paid once per prepend. Its two cached fields are read
        // BEFORE the move. See docs/design/audits/theta-depth-traversals-2026-07-26.md.
        let locally_free = union(self.locally_free.clone(), m.locally_free.clone());
        let connective_used = self.connective_used || m.connective_used;

        let mut new_matches = Vec::with_capacity(self.matches.len() + 1);
        new_matches.push(m);
        new_matches.append(&mut self.matches);

        Par {
            matches: new_matches,
            locally_free,
            connective_used,
            ..self.clone()
        }
    }

    pub fn prepend_if(&mut self, i: If) -> Par {
        // LEG-1: `i` is OWNED by this function, so it is MOVED into the vector
        // rather than deep-cloned. `<If as Clone>::clone` recurses through the
        // whole `Par` subtree — a Theta(depth) NATIVE-STACK traversal (2.78
        // KiB/level release) paid once per prepend. Its two cached fields are read
        // BEFORE the move. See docs/design/audits/theta-depth-traversals-2026-07-26.md.
        let locally_free = union(self.locally_free.clone(), i.locally_free.clone());
        let connective_used = self.connective_used || i.connective_used;

        let mut new_conditionals = Vec::with_capacity(self.conditionals.len() + 1);
        new_conditionals.push(i);
        new_conditionals.append(&mut self.conditionals);

        Par {
            conditionals: new_conditionals,
            locally_free,
            connective_used,
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

/// Anything that carries a [`FreeMap`] an attempt can write to, and therefore
/// anything that can be *isolated*.
///
/// One method, because one method is all the law needs: the isolation
/// combinators below take the map away for the duration of an attempt and put
/// it back afterwards, and they never look at anything else the state holds.
/// `rholang`'s `SpatialMatcherContext` is the only implementor today; the trait
/// exists so that the law lives at ONE address — a `rholang`-local copy of the
/// snapshot/restore beside this one is exactly the three-copies defect that
/// `a1feb437` cost.
pub trait IsolatableState {
    fn free_map_mut(&mut self) -> &mut FreeMap;
}

/// What an isolated attempt did.
///
/// **Total**: [`attempt_opt`] always runs its thunk, so there is no "never
/// ran" case (contrast `GuardDisposition::Undecidable`, which exists because a
/// guard genuinely may not be decidable). Exactly two things can happen, and
/// both are values.
///
/// ★ **This is the opposite of collapsing a disposition.** `Attempt<T>` carries
/// strictly MORE than the `Option<T>` it is built from — the same verdict, plus
/// the promise about what happened to the caller's bindings — and the
/// projection back down is explicit and total ([`Attempt::into_option`]). The
/// caller that needs it most is the negation, which *inverts* the disposition:
/// with a bare `Option`, "the inner attempt refused" and "the negation
/// succeeded" are both `Some(())`, and that conflation is what hid a leak for
/// as long as it hid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attempt<T> {
    /// The attempt succeeded, with this value.
    ///
    /// Whether its bindings survived is the combinator's contract, not this
    /// variant's: [`attempt_opt`] discards them, [`attempt_opt_keeping_bindings`]
    /// keeps them.
    Bound(T),
    /// The attempt refused — and its bindings **have been reverted**. The
    /// caller's free map is byte-identical to what it was at entry, under
    /// *both* combinators. That is the whole invariant, stated once.
    Refused,
}

impl<T> Attempt<T> {
    /// Project back onto the `Option` the matcher's traits answer in.
    ///
    /// Total and explicit: `Bound(v) ↦ Some(v)`, `Refused ↦ None`. Nothing is
    /// lost that the caller had not already decided to stop caring about.
    pub fn into_option(self) -> Option<T> {
        match self {
            Attempt::Bound(value) => Some(value),
            Attempt::Refused => None,
        }
    }
}

/// **The isolation law, written once.**
///
/// Runs `f` against `s` exactly as it stands, then *restores* `s`'s free map to
/// its entry value and hands the map `f` produced back as a value. Nothing is
/// lost and nothing is silently committed — the caller decides, and the two
/// wrappers below are the only two decisions the matcher makes.
///
/// ```text
///        entry                     during f                    return
///     ┌───────────┐             ┌───────────┐             ┌───────────┐
///  s: │ init      │──clone──┐   │ init + δ  │             │ init      │  ← restored
///     └───────────┘         │   └───────────┘             └───────────┘
///                           └────────────────────move────▶( init + δ ) ← returned
/// ```
///
/// The restore is a `std::mem::replace` — a **move**, not a clone — so the
/// entry snapshot is the only thing this costs. That is the same shape as
/// `list_match::match_function`'s per-attempt isolation, which is why the two
/// read alike.
///
/// # Why the produced map comes back as a value
///
/// Because "an attempt owns its own state" and "an attempt's bindings are
/// worthless" are different claims. A conjunction that succeeds has bound
/// something the pattern really does bind; a negation that succeeds has bound
/// nothing, because its body was numbered in a free map the normalizer already
/// discarded. Returning the map rather than committing it lets each caller say
/// which it is, in one word, at its own site.
pub fn isolate_free_map<S: IsolatableState, T>(
    s: &mut S,
    f: impl FnOnce(&mut S) -> Option<T>,
) -> (Option<T>, FreeMap) {
    // `initState <- get` — the ONLY clone isolation introduces.
    let init_state = s.free_map_mut().clone();

    let effect = f(s);

    // `resultState <- get; set(initState); yield resultState`, as ONE move.
    let result_state = std::mem::replace(s.free_map_mut(), init_state);

    (effect, result_state)
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/package.scala - attemptOpt
/// An attempt whose bindings **never** survive it, whichever way it goes.
///
/// This is the disposition of the two connectives that bind nothing by
/// construction:
///
/// * a **negation** — `~P`'s body is normalized against a fresh `FreeMap` that
///   is then discarded, so a free variable under `~` is `FreeVar(0)` in a
///   numbering nobody kept; and
/// * a **disjunction** — `P \/ Q`'s branches disagree about which variables
///   they would bind, so the disjunction binds none of them.
///
/// Both *probe*: they ask whether an attempt succeeds and keep only the answer.
///
/// ```rust
/// # use models::rust::utils::{Attempt, FreeMap, IsolatableState, attempt_opt, new_free_map};
/// # use models::rhoapi::Par;
/// # struct Ctx { free_map: FreeMap }
/// # impl IsolatableState for Ctx { fn free_map_mut(&mut self) -> &mut FreeMap { &mut self.free_map } }
/// # let mut ctx = Ctx { free_map: new_free_map() };
/// // A negation inverts the disposition — and both arms are reached with the
/// // caller's map already restored, because the restore is unconditional here.
/// let negation = match attempt_opt(&mut ctx, |s| {
///     s.free_map_mut().insert(0, Par::default()); // the body binds …
///     None::<()>                                  // … and then refuses
/// }) {
///     Attempt::Bound(_) => None,      // inner matched  → negation refuses
///     Attempt::Refused => Some(()),   // inner refused  → negation succeeds
/// };
/// assert_eq!(negation, Some(()));
/// assert_eq!(ctx.free_map, new_free_map()); // ★ and it bound nothing
/// ```
pub fn attempt_opt<S: IsolatableState, T>(
    s: &mut S,
    f: impl FnOnce(&mut S) -> Option<T>,
) -> Attempt<T> {
    match isolate_free_map(s, f) {
        (Some(value), _discarded) => Attempt::Bound(value),
        (None, _discarded) => Attempt::Refused,
    }
}

/// An attempt whose bindings survive **exactly when it succeeds**.
///
/// The keep-on-success wrapper over [`isolate_free_map`]. This is the
/// disposition of the sites that legitimately bind and then may be asked to
/// take it back:
///
/// * a **conjunction** — the right conjunct sees the left one's bindings, and a
///   successful conjunction binds their union; a refused one must un-bind *all*
///   of its conjuncts, not merely the one that refused, which is why the
///   isolation goes around the whole fold rather than around each conjunct; and
/// * a **`sub_pars` candidate** — the retry loop that offers a connective one
///   split of the target after another. A rejected split must not ride into the
///   accepted one.
///
/// The success path re-installs by moving the produced map back, so it costs
/// two pointer-sized moves on top of the entry clone and no second traversal.
pub fn attempt_opt_keeping_bindings<S: IsolatableState, T>(
    s: &mut S,
    f: impl FnOnce(&mut S) -> Option<T>,
) -> Attempt<T> {
    match isolate_free_map(s, f) {
        (Some(value), produced) => {
            *s.free_map_mut() = produced;
            Attempt::Bound(value)
        }
        (None, _reverted) => Attempt::Refused,
    }
}

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
    // println!("new_eset_expr: _ps: {:?}", _ps);
    // println!("new_eset_expr: _locally_free: {:?}", _locally_free);
    // println!("new_eset_expr: _connective_used: {:?}", _connective_used);
    // println!("new_eset_expr: _remainder: {:?}", _remainder);
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

// ─────────────────────────────────────────────────────────────────────────────
// The isolation law, pinned at its own address
// ─────────────────────────────────────────────────────────────────────────────
//
// `IsolatableState` is one method wide and the two combinators are generic over
// it, so the law can be exercised here — with a two-field stand-in state and no
// matcher at all — rather than only through `rholang`'s connective arms. What
// `rholang/tests/matcher_connective_isolation.rs` then tests is that the arms
// USE it; what these tests fix is what "it" means.
#[cfg(test)]
mod isolation_tests {
    use super::{
        attempt_opt, attempt_opt_keeping_bindings, isolate_free_map, new_free_map, Attempt,
        FreeMap, IsolatableState, Par,
    };

    /// The smallest thing that can be isolated: a free map and nothing else.
    struct Probe {
        free_map: FreeMap,
    }

    impl Probe {
        fn empty() -> Self {
            Probe {
                free_map: new_free_map(),
            }
        }
    }

    impl IsolatableState for Probe {
        fn free_map_mut(&mut self) -> &mut FreeMap { &mut self.free_map }
    }

    /// A distinguishable `Par` per level, so an assertion can name *which*
    /// binding survived rather than only how many did.
    fn marker(level: i32) -> Par {
        crate::rust::utils::new_gint_par(level as i64, Vec::new(), false)
    }

    #[test]
    fn isolate_free_map_restores_the_entry_map_and_returns_the_produced_one() {
        let mut probe = Probe::empty();
        probe.free_map.insert(0, marker(0));

        let (effect, produced) = isolate_free_map(&mut probe, |s| {
            s.free_map_mut().insert(1, marker(1));
            Some(())
        });

        assert_eq!(effect, Some(()), "the thunk ran and answered");
        assert_eq!(produced.get(&1), Some(&marker(1)), "δ came back as a value");
        assert_eq!(
            probe.free_map.get(&1),
            None,
            "★ and the caller's map is the one it had at entry"
        );
        assert_eq!(probe.free_map.get(&0), Some(&marker(0)), "entry map intact");
    }

    #[test]
    fn attempt_opt_discards_the_bindings_of_a_success() {
        let mut probe = Probe::empty();

        let attempt = attempt_opt(&mut probe, |s| {
            s.free_map_mut().insert(0, marker(0));
            Some(7)
        });

        assert_eq!(attempt, Attempt::Bound(7), "the verdict is carried out");
        assert_eq!(
            probe.free_map,
            new_free_map(),
            "★ a probing attempt keeps the answer and none of the bindings"
        );
    }

    #[test]
    fn attempt_opt_discards_the_bindings_of_a_refusal() {
        let mut probe = Probe::empty();

        let attempt = attempt_opt(&mut probe, |s| {
            s.free_map_mut().insert(0, marker(0));
            None::<()>
        });

        assert_eq!(attempt, Attempt::Refused);
        assert_eq!(probe.free_map, new_free_map(), "the refusal reverted");
    }

    #[test]
    fn attempt_opt_keeping_bindings_keeps_a_success_and_reverts_a_refusal() {
        let mut probe = Probe::empty();
        probe.free_map.insert(9, marker(9));

        let kept = attempt_opt_keeping_bindings(&mut probe, |s| {
            s.free_map_mut().insert(0, marker(0));
            Some(())
        });
        assert_eq!(kept, Attempt::Bound(()));
        assert_eq!(
            probe.free_map.get(&0),
            Some(&marker(0)),
            "★ a committing attempt that succeeded really did bind"
        );

        let refused = attempt_opt_keeping_bindings(&mut probe, |s| {
            s.free_map_mut().insert(1, marker(1));
            None::<()>
        });
        assert_eq!(refused, Attempt::Refused);
        assert_eq!(
            probe.free_map.get(&1),
            None,
            "★ and one that refused bound nothing"
        );
        assert_eq!(
            probe.free_map.get(&0),
            Some(&marker(0)),
            "without disturbing what was already there"
        );
        assert_eq!(probe.free_map.get(&9), Some(&marker(9)), "or the entry map");
    }

    /// ★ The difference between the two combinators is the whole reason there
    /// are two, so it is asserted directly rather than left to the two tests
    /// above to imply.
    #[test]
    fn the_two_combinators_differ_on_exactly_one_cell() {
        let bind = |s: &mut Probe| {
            s.free_map_mut().insert(0, marker(0));
            Some(())
        };
        let bind_then_refuse = |s: &mut Probe| {
            s.free_map_mut().insert(0, marker(0));
            None::<()>
        };

        let mut a = Probe::empty();
        attempt_opt(&mut a, bind);
        let mut b = Probe::empty();
        attempt_opt_keeping_bindings(&mut b, bind);
        assert_ne!(a.free_map, b.free_map, "★ they differ on SUCCESS");

        let mut c = Probe::empty();
        attempt_opt(&mut c, bind_then_refuse);
        let mut d = Probe::empty();
        attempt_opt_keeping_bindings(&mut d, bind_then_refuse);
        assert_eq!(c.free_map, d.free_map, "and agree on REFUSAL");
        assert_eq!(c.free_map, new_free_map(), "both having reverted it");
    }

    /// `into_option` is total: it is the projection back onto the `Option` the
    /// matcher's traits answer in, and it loses only the state promise.
    #[test]
    fn into_option_is_the_total_projection() {
        assert_eq!(Attempt::Bound(3).into_option(), Some(3));
        assert_eq!(Attempt::<i32>::Refused.into_option(), None);
    }
}
