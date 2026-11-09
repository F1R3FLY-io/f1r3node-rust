//! The send rule of `seq` spaces (P7, design v3 W3: SQ-1 and SQ-2), and the causal histories of
//! their claims (decision 91).
//!
//! FIPS lines 279 to 285: a `seq` channel is temp, it is never sent, alone or inside a process,
//! it is not used in a concurrent process, and it is used in at most one sequential process. The
//! engine checks the uses with one claim for each channel (SQ-3, `spaces::ephemeral::SeqClaims`).
//! This module checks the sends.
//!
//! **Causal histories (decision 91, `SpaceCoordination.tla`).** Evaluation races (DR-132), so a
//! use of a channel is admitted when the last use of the channel happened before it, as the
//! causal history of the use shows ([`SeqHistory`]). The history of an evaluation task is the
//! task-local `SEQ_HISTORY`:
//!
//! * the root of an evaluation starts with the empty history (`with_history`);
//! * a task that the evaluator spawns starts with a copy of the history of its parent
//!   (`inherit`, the `Split` of the model);
//! * a fired body starts with the union of the histories of its participants: the history of
//!   each datum and of the continuation, which the book holds by random state (`with_history`,
//!   the `Comm` of the model);
//! * a committed use adds itself to the history of its task and of its own item (`learn` and
//!   `SeqClaims::store`, the `CommitUse` of the model).
//!
//! The verdict depends only on the program and on the matches of the COMMs, which replay takes
//! from the recorded rows, so play and replay agree.
//!
//! **SQ-1.** A seq name is an id `n(sid…)` of a `seq` space of the current root evaluation. The
//! handle of a `seq` space is not a name, so a program may send it.
//!
//! **SQ-2.** While the evaluation holds a `seq` space, `route_send` scans the evaluated data of
//! every send before any mutation. A data element that holds a seq name at any depth aborts the
//! deploy with `SeqSent`. Two shapes are exempt:
//!
//! * The forwarder hop: a send to `HANDLE_OP` whose first data element is a bare handle id. Only
//!   the handle continuation makes this shape (F25), and the scan of the send to the handle
//!   covered its arguments.
//! * In a send to the handle of a `seq` space `S`, the channel argument of the method `produce`
//!   may hold names of `S`. Its message and every other argument are scanned in full, and a name
//!   of another `seq` space in the channel argument is sent.
//!
//! The replies of system processes do not pass `route_send`, so the reply of `gensym` carries
//! its seq name.
//!
//! The scan walks a work list, so its depth needs no stack. It destructures every field of every
//! type of the `Par` tree, so a new field is a compile error until the scan handles it.
//!
//! ```text
//! CHECK-SEND(chan, pars, seq):
//!   if seq = ∅ or (op(chan) = HANDLE_OP and pars[0] is a bare handle id): return
//!   own ← the space of chan when chan is the handle of a space of seq
//!   for each data element pars[i]:
//!     exempt ← own when i = 2 and pars[1] = "produce", else none
//!     SCAN(pars[i], seq, exempt)
//! SCAN(root, seq, exempt):
//!   work ← [root]
//!   while work is not empty:
//!     node ← pop(work)
//!     for each GPrivate id of node:
//!       if id = n(sid…), sid ∈ seq and sid ≠ exempt: abort SeqSent(sid)
//!     push every Par, Expr and CostSignature child of node
//! ```

// Changed by decision 91: the causal histories need the map, the cell, the shared pointer and
// the task-local future.
// use std::collections::BTreeSet;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Arc;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::cost_signature::Value as SignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
// Changed by decision 112: the scan walks the new fields. The text before:
// ```
// use models::rhoapi::{
//     Bundle, Connective, ConnectiveBody, CostSignature, CostSignatureCompound, CostSignedTerm,
//     CostStack, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap, EMatches, EMethod, EMinus,
//     EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus,
//     ESet, ETuple, EVar, EZipper, Expr, GPrivate, GUnforgeable, If, KeyValuePair, Match, MatchCase,
//     New, Par, Receive, ReceiveBind, Send,
// };
// ```
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, CostSignature, CostSignatureCompound, CostSignedTerm,
    CostStack, EAnd, EDiv, EEq, EFree, EGt, EGte, EList, ELt, ELte, EMap, EMatches, EMethod,
    EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus,
    EPlusPlus, ESet, ETuple, EVar, EZipper, Expr, GPrivate, GUnforgeable, If, KeyValuePair, Match,
    MatchCase, New, Par, QuerySpec, Receive, ReceiveBind, Send, SendParams, SpaceAnnotation,
    UseBlock,
};
use tokio::task::futures::TaskLocalFuture;

use super::system_channels::{handle_of, op_of};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::namespace::{
    decode_reified_id, reified_id_of, Durability, ReifiedId, SpaceId, HANDLE_OP,
};
use crate::rust::interpreter::spaces::record::sole_expr;

/// The index of the channel argument in the data of a handle call `[ret, method, ch, msg, …]`.
const CHANNEL_ARGUMENT: usize = 2;

/// SQ-2 for one send: checks the data elements `pars` of a send on `chan` against the `seq`
/// spaces `seq` of the evaluation, with the two exemptions of the module docs. With no `seq`
/// space nothing is scanned.
pub fn check_send(chan: &Par, pars: &[Par], seq: &BTreeSet<SpaceId>) -> Result<(), SpaceError> {
    if seq.is_empty() {
        return Ok(());
    }
    if op_of(chan) == Some(HANDLE_OP) && pars.first().is_some_and(is_bare_handle) {
        return Ok(());
    }
    let own = handle_of(chan)
        .map(|space| space.sid)
        .filter(|sid| seq.contains(sid));
    let produce = pars.get(1).is_some_and(is_produce_method);
    for (index, par) in pars.iter().enumerate() {
        let exempt = match (own, index == CHANNEL_ARGUMENT && produce) {
            (Some(sid), true) => Some(sid),
            (Some(_), false) | (None, _) => None,
        };
        scan(par, seq, exempt)?;
    }
    Ok(())
}

/// SQ-2: the params of a send take the rule of its data.
pub fn check_params(
    chan: &Par,
    pars: &[Par],
    params: Option<&SendParams>,
    seq: &BTreeSet<SpaceId>,
) -> Result<(), SpaceError> {
    let Some(params) = params else {
        return Ok(());
    };
    if seq.is_empty() {
        return Ok(());
    }
    if op_of(chan) == Some(HANDLE_OP) && pars.first().is_some_and(is_bare_handle) {
        return Ok(());
    }
    for value in params.positional.iter().chain(params.named.values()) {
        scan(value, seq, None)?;
    }
    Ok(())
}

/// True for a `Par` that is exactly a handle id `E0‖H‖sid` or `E0‖h‖sid`.
fn is_bare_handle(par: &Par) -> bool { matches!(reified_id_of(par), Some(ReifiedId::Handle(_))) }

/// True for the method name `"produce"` of a handle call.
fn is_produce_method(par: &Par) -> bool {
    matches!(sole_expr(par), Some(ExprInstance::GString(method)) if method == "produce")
}

/// A node of the work list of the scan.
enum Node<'a> {
    Par(&'a Par),
    Expr(&'a Expr),
    Signature(&'a CostSignature),
}

/// The work list of one scan.
struct Work<'a> {
    nodes: Vec<Node<'a>>,
}

impl<'a> Work<'a> {
    fn push(&mut self, node: Node<'a>) { self.nodes.push(node); }

    fn par(&mut self, par: &'a Par) { self.push(Node::Par(par)); }

    fn optional(&mut self, par: &'a Option<Par>) {
        if let Some(par) = par {
            self.par(par);
        }
    }

    fn pars(&mut self, pars: &'a [Par]) {
        for par in pars {
            self.par(par);
        }
    }

    fn signature(&mut self, signature: &'a Option<CostSignature>) {
        if let Some(signature) = signature {
            self.push(Node::Signature(signature));
        }
    }
}

/// Scans `root` for a seq name of `seq` other than a name of `exempt`.
fn scan(root: &Par, seq: &BTreeSet<SpaceId>, exempt: Option<SpaceId>) -> Result<(), SpaceError> {
    let mut work = Work { nodes: Vec::new() };
    work.par(root);
    while let Some(node) = work.nodes.pop() {
        match node {
            Node::Par(par) => scan_par(par, &mut work, seq, exempt)?,
            Node::Expr(expr) => scan_expr(expr, &mut work),
            Node::Signature(signature) => scan_signature(signature, &mut work),
        }
    }
    Ok(())
}

/// Checks the unforgeables of `par` and adds every child of `par` to the work list.
fn scan_par<'a>(
    par: &'a Par,
    work: &mut Work<'a>,
    seq: &BTreeSet<SpaceId>,
    exempt: Option<SpaceId>,
) -> Result<(), SpaceError> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used: _,
        cost_signed_terms,
        cost_stacks,
        use_blocks,
    } = par;
    for unforgeable in unforgeables {
        check_unforgeable(unforgeable, seq, exempt)?;
    }
    for send in sends {
        let Send {
            chan,
            data,
            persistent: _,
            locally_free: _,
            connective_used: _,
            params,
        } = send;
        work.optional(chan);
        work.pars(data);
        if let Some(SendParams { positional, named }) = params {
            work.pars(positional);
            for value in named.values() {
                work.par(value);
            }
        }
    }
    for receive in receives {
        let Receive {
            binds,
            body,
            persistent: _,
            peek: _,
            bind_count: _,
            locally_free: _,
            connective_used: _,
            condition,
        } = receive;
        for bind in binds {
            let ReceiveBind {
                patterns,
                source,
                remainder: _,
                free_count: _,
                cost_signature,
                query_spec,
            } = bind;
            work.pars(patterns);
            work.optional(source);
            work.signature(cost_signature);
            if let Some(QuerySpec { payload, named }) = query_spec {
                work.pars(payload);
                for value in named.values() {
                    work.par(value);
                }
            }
        }
        work.optional(body);
        work.optional(condition);
    }
    for new in news {
        let New {
            bind_count: _,
            p,
            uri: _,
            injections,
            locally_free: _,
            space_types,
        } = new;
        work.optional(p);
        for injected in injections.values() {
            work.par(injected);
        }
        for annotation in space_types {
            let SpaceAnnotation { space } = annotation;
            work.optional(space);
        }
    }
    for expr in exprs {
        work.push(Node::Expr(expr));
    }
    for matched in matches {
        let Match {
            target,
            cases,
            locally_free: _,
            connective_used: _,
        } = matched;
        work.optional(target);
        for case in cases {
            let MatchCase {
                pattern,
                source,
                free_count: _,
                guard,
            } = case;
            work.optional(pattern);
            work.optional(source);
            work.optional(guard);
        }
    }
    for bundle in bundles {
        let Bundle {
            body,
            write_flag: _,
            read_flag: _,
        } = bundle;
        work.optional(body);
    }
    for connective in connectives {
        let Connective {
            connective_instance,
        } = connective;
        match connective_instance {
            Some(ConnectiveInstance::ConnAndBody(ConnectiveBody { ps }))
            | Some(ConnectiveInstance::ConnOrBody(ConnectiveBody { ps })) => work.pars(ps),
            Some(ConnectiveInstance::ConnNotBody(negated)) => work.par(negated),
            Some(
                ConnectiveInstance::VarRefBody(_)
                | ConnectiveInstance::ConnBool(_)
                | ConnectiveInstance::ConnInt(_)
                | ConnectiveInstance::ConnString(_)
                | ConnectiveInstance::ConnUri(_)
                | ConnectiveInstance::ConnByteArray(_),
            )
            | None => {}
        }
    }
    for conditional in conditionals {
        let If {
            condition,
            if_true,
            if_false,
            locally_free: _,
            connective_used: _,
        } = conditional;
        work.optional(condition);
        work.optional(if_true);
        work.optional(if_false);
    }
    for term in cost_signed_terms {
        // Changed by decision 90 (DR-135): a signed term also holds its held price, a plain
        // integer with no name in it.
        // let CostSignedTerm { body, signature } = term;
        let CostSignedTerm {
            body,
            signature,
            held_price: _,
        } = term;
        work.optional(body);
        work.signature(signature);
    }
    for stack in cost_stacks {
        let CostStack { cells } = stack;
        for cell in cells {
            work.push(Node::Signature(cell));
        }
    }
    for block in use_blocks {
        let UseBlock { space, body } = block;
        work.optional(space);
        work.optional(body);
    }
    Ok(())
}

/// Fails with `SeqSent` when `unforgeable` is a seq name of `seq` other than a name of `exempt`.
/// The lookup in `seq` is only for a name of an ephemeral space.
fn check_unforgeable(
    unforgeable: &GUnforgeable,
    seq: &BTreeSet<SpaceId>,
    exempt: Option<SpaceId>,
) -> Result<(), SpaceError> {
    let GUnforgeable { unf_instance } = unforgeable;
    match unf_instance {
        Some(UnfInstance::GPrivateBody(GPrivate { id })) => match decode_reified_id(id) {
            Some(ReifiedId::Name(space, _)) if space.durability == Durability::Ephemeral => {
                match seq.contains(&space.sid) && exempt != Some(space.sid) {
                    true => Err(SpaceError::SeqSent { space: space.sid }),
                    false => Ok(()),
                }
            }
            Some(_) | None => Ok(()),
        },
        Some(
            UnfInstance::GDeployIdBody(_)
            | UnfInstance::GDeployerIdBody(_)
            | UnfInstance::GSysAuthTokenBody(_)
            | UnfInstance::GAuthorityIdBody(_)
            | UnfInstance::GPrincipalIdBody(_),
        )
        | None => Ok(()),
    }
}

/// Adds every `Par` of `expr` to the work list.
fn scan_expr<'a>(expr: &'a Expr, work: &mut Work<'a>) {
    let Expr { expr_instance } = expr;
    match expr_instance {
        Some(
            ExprInstance::GBool(_)
            | ExprInstance::GInt(_)
            | ExprInstance::GString(_)
            | ExprInstance::GUri(_)
            | ExprInstance::GByteArray(_)
            | ExprInstance::GDouble(_)
            | ExprInstance::GBigInt(_)
            | ExprInstance::GBigRat(_)
            | ExprInstance::GFixedPoint(_)
            | ExprInstance::GFloat32(_)
            | ExprInstance::GUint64(_)
            | ExprInstance::GInt32(_)
            | ExprInstance::GUint32(_)
            | ExprInstance::GUint16(_)
            | ExprInstance::GUint8(_),
        )
        | None => (),
        Some(ExprInstance::ENotBody(ENot { p }) | ExprInstance::ENegBody(ENeg { p })) => {
            work.optional(p);
        }
        Some(
            ExprInstance::EMultBody(EMult { p1, p2 })
            | ExprInstance::EDivBody(EDiv { p1, p2 })
            | ExprInstance::EPlusBody(EPlus { p1, p2 })
            | ExprInstance::EMinusBody(EMinus { p1, p2 })
            | ExprInstance::ELtBody(ELt { p1, p2 })
            | ExprInstance::ELteBody(ELte { p1, p2 })
            | ExprInstance::EGtBody(EGt { p1, p2 })
            | ExprInstance::EGteBody(EGte { p1, p2 })
            | ExprInstance::EEqBody(EEq { p1, p2 })
            | ExprInstance::ENeqBody(ENeq { p1, p2 })
            | ExprInstance::EAndBody(EAnd { p1, p2 })
            | ExprInstance::EOrBody(EOr { p1, p2 })
            | ExprInstance::EPercentPercentBody(EPercentPercent { p1, p2 })
            | ExprInstance::EPlusPlusBody(EPlusPlus { p1, p2 })
            | ExprInstance::EMinusMinusBody(EMinusMinus { p1, p2 })
            | ExprInstance::EModBody(EMod { p1, p2 }),
        ) => {
            work.optional(p1);
            work.optional(p2);
        }
        Some(ExprInstance::EVarBody(EVar { v: _ })) => (),
        Some(ExprInstance::EListBody(EList {
            ps,
            locally_free: _,
            connective_used: _,
            remainder: _,
        }))
        | Some(ExprInstance::ETupleBody(ETuple {
            ps,
            locally_free: _,
            connective_used: _,
        }))
        | Some(ExprInstance::ESetBody(ESet {
            ps,
            locally_free: _,
            connective_used: _,
            remainder: _,
        }))
        | Some(ExprInstance::EPathmapBody(EPathMap {
            ps,
            locally_free: _,
            connective_used: _,
            remainder: _,
        })) => work.pars(ps),
        Some(ExprInstance::EFreeBody(EFree { theory: _, args })) => work.pars(args),
        Some(ExprInstance::EParamsBody(SendParams { positional, named })) => {
            work.pars(positional);
            for value in named.values() {
                work.par(value);
            }
        }
        Some(ExprInstance::EMapBody(EMap {
            kvs,
            locally_free: _,
            connective_used: _,
            remainder: _,
        })) => {
            for pair in kvs {
                let KeyValuePair { key, value } = pair;
                work.optional(key);
                work.optional(value);
            }
        }
        Some(ExprInstance::EMethodBody(EMethod {
            method_name: _,
            target,
            arguments,
            locally_free: _,
            connective_used: _,
        })) => {
            work.optional(target);
            work.pars(arguments);
        }
        Some(ExprInstance::EZipperBody(EZipper {
            pathmap,
            current_path: _,
            is_write_zipper: _,
            locally_free: _,
            connective_used: _,
        })) => {
            if let Some(EPathMap {
                ps,
                locally_free: _,
                connective_used: _,
                remainder: _,
            }) = pathmap
            {
                work.pars(ps);
            }
        }
        Some(ExprInstance::EMatchesBody(EMatches { target, pattern })) => {
            work.optional(target);
            work.optional(pattern);
        }
    }
}

/// Adds every `Par` and every nested signature of `signature` to the work list.
fn scan_signature<'a>(signature: &'a CostSignature, work: &mut Work<'a>) {
    let CostSignature { value } = signature;
    match value {
        Some(SignatureValue::Quote(par) | SignatureValue::Name(par)) => work.par(par),
        Some(SignatureValue::Compound(CostSignatureCompound { elements })) => {
            for element in elements {
                work.push(Node::Signature(element));
            }
        }
        Some(
            SignatureValue::Ground(_) | SignatureValue::BoundLevel(_) | SignatureValue::Unit(_),
        )
        | None => (),
    }
}

// ── Decision 91: the causal histories of the seq claims ──────────────────────────────────────

/// Added by decision 91: a causal history (QF-15). For each channel of a `seq` space, it holds
/// the number of the last use of that channel that happened before. The admitted uses of one
/// channel form a causal chain (`SeqCausalChain` of `SpaceCoordination.tla`), so the number of
/// the last known use of a channel stands for every earlier use of it, and one number for each
/// channel holds the whole history. The empty history has no map, so a deploy without a `seq`
/// use never allocates one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeqHistory(Option<Arc<BTreeMap<Vec<u8>, u64>>>);

impl SeqHistory {
    /// True when no use is in the history.
    pub fn is_empty(&self) -> bool { self.0.is_none() }

    /// The number of the last use of the channel `id` in the history, or 0.
    pub fn last_use(&self, id: &[u8]) -> u64 {
        self.0
            .as_ref()
            .and_then(|uses| uses.get(id).copied())
            .unwrap_or(0)
    }

    /// True when the use number `use_number` of the channel `id` is in the history
    /// (`Admissible` of `SpaceCoordination.tla`: `claim[c] \in hist[p]`).
    pub fn knows(&self, id: &[u8], use_number: u64) -> bool { self.last_use(id) >= use_number }

    /// The union of two histories: the larger use number of each channel (`Comm` of
    /// `SpaceCoordination.tla`: `hist[p] \cup hist[q]`).
    pub fn join(&self, other: &SeqHistory) -> SeqHistory {
        match (&self.0, &other.0) {
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(left), Some(right)) => {
                let mut uses = BTreeMap::clone(left);
                for (id, number) in right.iter() {
                    let known = uses.entry(id.clone()).or_insert(0);
                    *known = (*known).max(*number);
                }
                SeqHistory(Some(Arc::new(uses)))
            }
        }
    }

    /// The history with the uses `uses` added (`CommitUse` of `SpaceCoordination.tla`:
    /// `hist[p] \cup {id}`).
    pub fn with_uses(&self, uses: &[(Vec<u8>, u64)]) -> SeqHistory {
        if uses.is_empty() {
            return self.clone();
        }
        let mut known = self.0.as_deref().cloned().unwrap_or_default();
        for (id, number) in uses {
            let last = known.entry(id.clone()).or_insert(0);
            *last = (*last).max(*number);
        }
        SeqHistory(Some(Arc::new(known)))
    }
}

tokio::task_local! {
    /// Added by decision 91: the causal history of the current evaluation task. A task that
    /// the evaluator spawns starts with a copy of the history of its parent (`inherit`), and a
    /// fired body starts with the union of the histories of its participants
    /// (`with_history`).
    static SEQ_HISTORY: RefCell<SeqHistory>;
}

/// Added by decision 91: the causal history of the current task, or the empty history outside a
/// history scope.
pub fn current_history() -> SeqHistory {
    SEQ_HISTORY
        .try_with(|history| history.borrow().clone())
        .unwrap_or_default()
}

/// Added by decision 91: adds `uses` to the history of the current task, after the uses
/// committed. The reply of the job then gives the uses to the continuation of the task
/// (`CommitUse` of `SpaceCoordination.tla`). Outside a history scope it does nothing.
pub fn learn(uses: &[(Vec<u8>, u64)]) {
    if uses.is_empty() {
        return;
    }
    let _ = SEQ_HISTORY.try_with(|history| {
        let next = history.borrow().with_uses(uses);
        *history.borrow_mut() = next;
    });
}

/// Added by decision 91: runs `future` as a task with a copy of the history of the current task,
/// as both children of a split get the history of the parent (`Split` of
/// `SpaceCoordination.tla`). The spawn helper of the evaluator calls it for every task.
pub fn inherit<F: Future>(future: F) -> TaskLocalFuture<RefCell<SeqHistory>, F> {
    SEQ_HISTORY.scope(RefCell::new(current_history()), future)
}

/// Added by decision 91: runs `future` with the history `history`: the root of an evaluation
/// with the empty history, or a fired body with the union of the histories of its participants
/// (`Comm` of `SpaceCoordination.tla`).
pub fn with_history<F: Future>(
    history: SeqHistory,
    future: F,
) -> TaskLocalFuture<RefCell<SeqHistory>, F> {
    SEQ_HISTORY.scope(RefCell::new(history), future)
}

#[cfg(test)]
mod tests {
    use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};

    use super::*;
    use crate::rust::interpreter::spaces::namespace::{
        self, gprivate, NameTail, NativeSpace, SID_LEN,
    };

    /// Added by decision 91: the history with the uses `uses`.
    fn history(uses: &[(&[u8], u64)]) -> SeqHistory {
        let uses: Vec<(Vec<u8>, u64)> = uses.iter().map(|(id, n)| (id.to_vec(), *n)).collect();
        SeqHistory::default().with_uses(&uses)
    }

    /// Added by decision 91: the union of histories is commutative, associative and
    /// idempotent, the empty history is its unit, and a history knows every use up to the last
    /// known use of each channel (the per-channel clock of `SeqHistory`).
    #[test]
    fn seq_history_is_a_join_semilattice_of_per_channel_clocks() {
        let a = history(&[(b"x", 2), (b"y", 1)]);
        let b = history(&[(b"x", 5)]);
        let c = history(&[(b"z", 3)]);
        let empty = SeqHistory::default();
        assert_eq!(a.join(&b), b.join(&a));
        assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));
        assert_eq!(a.join(&a), a);
        assert_eq!(a.join(&empty), a);
        assert_eq!(empty.join(&a), a);
        assert!(empty.is_empty() && !a.is_empty());
        let joined = a.join(&b);
        assert_eq!(joined.last_use(b"x"), 5);
        assert!(joined.knows(b"x", 3), "use 5 of x stands for uses 1 to 5");
        assert!(!joined.knows(b"x", 6));
        assert!(joined.knows(b"y", 1) && !joined.knows(b"z", 1));
        assert_eq!(
            a.with_uses(&[(b"x".to_vec(), 1)]),
            a,
            "an older use changes nothing"
        );
        assert_eq!(a.with_uses(&[]), a);
    }

    /// Added by decision 91: a spawned task starts with a copy of the history of its parent, a
    /// change in the child stays in the child, and `learn` outside a scope does nothing
    /// (`Split` of `SpaceCoordination.tla`).
    #[tokio::test]
    async fn seq_history_inherit_copies_the_parent_history() {
        learn(&[(b"x".to_vec(), 1)]);
        assert!(current_history().is_empty(), "no scope, no history");
        let parent = history(&[(b"x", 1)]);
        let (child, parent_after) = with_history(parent.clone(), async {
            let child = inherit(async {
                learn(&[(b"y".to_vec(), 4)]);
                current_history()
            })
            .await;
            (child, current_history())
        })
        .await;
        assert_eq!(child, parent.with_uses(&[(b"y".to_vec(), 4)]));
        assert_eq!(parent_after, parent, "the child does not change its parent");
    }

    fn space(seed: u8, durability: Durability) -> NativeSpace {
        NativeSpace {
            sid: [seed; SID_LEN],
            durability,
        }
    }

    fn name(space: NativeSpace, tail: u8) -> Par {
        gprivate(namespace::name_id(space, NameTail::Random([tail; 32])))
    }

    fn handle(space: NativeSpace) -> Par { gprivate(namespace::handle_channel(space)) }

    fn list(ps: Vec<Par>) -> Par { new_elist_par(ps, Vec::new(), false, None, Vec::new(), false) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    fn seq_of(spaces: &[NativeSpace]) -> BTreeSet<SpaceId> {
        spaces.iter().map(|space| space.sid).collect()
    }

    /// `par` as the expression of a tuple, a map value, a quoted send, a bundle, a receive
    /// pattern, a match guard and two cost signatures: the places where a name can hide.
    fn hidden(par: Par) -> Vec<Par> {
        let quoted_send = Par {
            sends: vec![Send {
                chan: Some(string("out")),
                data: vec![par.clone()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let bundled = Par {
            bundles: vec![Bundle {
                body: Some(par.clone()),
                write_flag: true,
                read_flag: false,
            }],
            ..Default::default()
        };
        let pattern = Par {
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![par.clone()],
                    source: Some(string("in")),
                    ..Default::default()
                }],
                body: Some(Par::default()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let guarded = Par {
            matches: vec![Match {
                target: Some(int(1)),
                cases: vec![MatchCase {
                    pattern: Some(int(1)),
                    source: Some(Par::default()),
                    free_count: 0,
                    guard: Some(par.clone()),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let tuple = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                    ps: vec![int(0), par.clone()],
                    ..Default::default()
                })),
            }],
            ..Default::default()
        };
        let map = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: vec![KeyValuePair {
                        key: Some(string("k")),
                        value: Some(par.clone()),
                    }],
                    ..Default::default()
                })),
            }],
            ..Default::default()
        };
        let quoted_signature = Par {
            cost_stacks: vec![CostStack {
                cells: vec![CostSignature {
                    value: Some(SignatureValue::Compound(CostSignatureCompound {
                        elements: vec![CostSignature {
                            value: Some(SignatureValue::Quote(par.clone())),
                        }],
                    })),
                }],
            }],
            ..Default::default()
        };
        let named_signature = Par {
            cost_signed_terms: vec![CostSignedTerm {
                body: Some(Par::default()),
                signature: Some(CostSignature {
                    value: Some(SignatureValue::Name(par.clone())),
                }),
                // Added by decision 90 (DR-135): a signed term of the surface syntax has no
                // held price.
                held_price: None,
            }],
            ..Default::default()
        };
        let nested = list(vec![list(vec![list(vec![par.clone()])])]);
        vec![
            par,
            tuple,
            map,
            quoted_send,
            bundled,
            pattern,
            guarded,
            quoted_signature,
            named_signature,
            nested,
        ]
    }

    #[test]
    fn reified_seq_scan_finds_a_seq_name_at_every_place() {
        let seq = space(1, Durability::Ephemeral);
        for (index, data) in hidden(name(seq, 7)).into_iter().enumerate() {
            assert_eq!(
                check_send(&string("out"), &[int(0), data], &seq_of(&[seq])),
                Err(SpaceError::SeqSent { space: seq.sid }),
                "place {index}"
            );
        }
    }

    #[test]
    fn reified_seq_scan_passes_other_ids() {
        let seq = space(1, Durability::Ephemeral);
        let temp = space(2, Durability::Ephemeral);
        let durable = space(1, Durability::Durable);
        let ids = [
            name(temp, 7),
            name(durable, 7),
            handle(seq),
            gprivate(namespace::data_channel(
                seq,
                &namespace::CollectionKey::from_bytes(vec![1]).expect("a key"),
            )),
            gprivate(vec![9; 32]),
        ];
        for id in ids {
            for data in hidden(id.clone()) {
                assert_eq!(
                    check_send(&string("out"), &[data], &seq_of(&[seq])),
                    Ok(()),
                    "{id:?}"
                );
            }
        }
        // With no seq space nothing is scanned, so a seq name of an ended space passes.
        assert_eq!(
            check_send(&string("out"), &[name(seq, 7)], &BTreeSet::new()),
            Ok(())
        );
    }

    #[test]
    fn reified_seq_scan_exempts_the_forwarder_hop_and_the_own_channel_argument() {
        let seq = space(1, Durability::Ephemeral);
        let other = space(3, Durability::Ephemeral);
        let spaces = seq_of(&[seq, other]);
        let x = name(seq, 7);
        let op = gprivate(namespace::op_channel(HANDLE_OP));
        // The forwarder hop: HANDLE_OP!(h, args).
        let args = list(vec![
            string("r"),
            string("produce"),
            x.clone(),
            list(vec![]),
        ]);
        assert_eq!(
            check_send(&op, &[handle(seq), args.clone()], &spaces),
            Ok(())
        );
        // Without a bare handle first, the send to HANDLE_OP is scanned.
        assert_eq!(
            check_send(&op, &[string("h"), args], &spaces),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
        // The channel argument of S's own produce may hold names of S.
        let call = |ch: Par, msg: Par| vec![string("r"), string("produce"), ch, msg];
        assert_eq!(
            check_send(&handle(seq), &call(x.clone(), list(vec![])), &spaces),
            Ok(())
        );
        // The message is scanned in full.
        assert_eq!(
            check_send(
                &handle(seq),
                &call(x.clone(), list(vec![x.clone()])),
                &spaces
            ),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
        // A name of another seq space in the channel argument is sent.
        assert_eq!(
            check_send(&handle(seq), &call(name(other, 2), list(vec![])), &spaces),
            Err(SpaceError::SeqSent { space: other.sid })
        );
        // Only the method produce exempts its channel argument.
        assert_eq!(
            check_send(
                &handle(seq),
                &[string("r"), string("config"), x.clone()],
                &spaces
            ),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
        // The handle of another space exempts nothing.
        assert_eq!(
            check_send(&handle(other), &call(x.clone(), list(vec![])), &spaces),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
        // The return name of a call is scanned.
        assert_eq!(
            check_send(
                &handle(seq),
                &[x, string("produce"), name(seq, 3), list(vec![])],
                &spaces
            ),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
    }

    /// SQ-2 (decision 112): the params of a send take the rule of its data. A seq name in a
    /// positional value or a named value aborts. Absent params pass, a value without a seq name
    /// passes, the forwarder hop passes, and with no seq space nothing is scanned.
    #[test]
    fn check_params_rejects_a_seq_name() {
        let seq = space(1, Durability::Ephemeral);
        let spaces = seq_of(&[seq]);
        let x = name(seq, 7);
        let target = string("out");
        let positional = SendParams {
            positional: vec![x.clone()],
            named: BTreeMap::new(),
        };
        let named = SendParams {
            positional: Vec::new(),
            named: BTreeMap::from([("key".to_string(), list(vec![x.clone()]))]),
        };
        let op = gprivate(namespace::op_channel(HANDLE_OP));
        for params in [&positional, &named] {
            assert_eq!(
                check_params(&target, &[int(0)], Some(params), &spaces),
                Err(SpaceError::SeqSent { space: seq.sid })
            );
            assert_eq!(
                check_params(&target, &[int(0)], Some(params), &BTreeSet::new()),
                Ok(())
            );
            assert_eq!(
                check_params(&op, &[handle(seq)], Some(params), &spaces),
                Ok(())
            );
        }
        assert_eq!(check_params(&target, &[int(0)], None, &spaces), Ok(()));
        let clean = SendParams {
            positional: vec![int(1)],
            named: BTreeMap::from([("key".to_string(), int(2))]),
        };
        assert_eq!(
            check_params(&target, &[int(0)], Some(&clean), &spaces),
            Ok(())
        );
    }
}
