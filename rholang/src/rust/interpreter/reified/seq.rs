//! The send rule of `seq` spaces (P7, design v3 W3: SQ-1 and SQ-2).
//!
//! FIPS lines 279 to 285: a `seq` channel is temp, it is never sent, alone or inside a process,
//! it is not used in a concurrent process, and it is used in at most one sequential process. The
//! engine checks the uses with one claim for each channel (SQ-3, `spaces::ephemeral::SeqClaims`).
//! This module checks the sends.
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
//! type of the `Par` tree, so a new field is a compile error until the scan handles it. Before
//! each step it reserves one `VerificationOperations` unit for the node, the `VerificationBytes`
//! of each id that it reads, and the `SearchStateBytes` of each entry that it adds to the work
//! list (A14).
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
//!     node ← pop(work)                         ▷ one VerificationOperations unit
//!     for each GPrivate id of node:             ▷ the VerificationBytes of the id
//!       if id = n(sid…), sid ∈ seq and sid ≠ exempt: abort SeqSent(sid)
//!     push every Par, Expr and CostSignature child of node
//! ```

use std::collections::BTreeSet;
use std::mem::size_of;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::cost_signature::Value as SignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, CostSignature, CostSignatureCompound, CostSignedTerm,
    CostStack, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap, EMatches, EMethod, EMinus,
    EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus,
    ESet, ETuple, EVar, EZipper, Expr, GPrivate, GUnforgeable, If, KeyValuePair, Match, MatchCase,
    New, Par, Receive, ReceiveBind, Send,
};
use rspace_plus_plus::rspace::errors::RSpaceError;

use super::costs::ScanMeter;
use super::system_channels::{handle_of, op_of};
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::namespace::{
    decode_reified_id, reified_id_of, Durability, ReifiedId, SpaceId, HANDLE_OP, SID_LEN,
};
use crate::rust::interpreter::spaces::record::sole_expr;

/// The index of the channel argument in the data of a handle call `[ret, method, ch, msg, …]`.
const CHANNEL_ARGUMENT: usize = 2;

/// SQ-2 for one send: checks the data elements `pars` of a send on `chan` against the `seq`
/// spaces `seq` of the evaluation, with the two exemptions of the module docs. With no `seq`
/// space nothing is scanned and nothing is reserved.
pub fn check_send(
    chan: &Par,
    pars: &[Par],
    seq: &BTreeSet<SpaceId>,
    meter: &ScanMeter,
) -> Result<(), SpaceError> {
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
        scan(par, seq, exempt, meter)?;
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

/// The work list of one scan. Each push reserves the `SearchStateBytes` of its entry.
struct Work<'a, 'm> {
    nodes: Vec<Node<'a>>,
    meter: &'m ScanMeter,
}

impl<'a> Work<'a, '_> {
    fn push(&mut self, node: Node<'a>) -> Result<(), SpaceError> {
        self.meter.charge(0, 0, size_of::<Node<'_>>())?;
        self.nodes
            .try_reserve(1)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        self.nodes.push(node);
        Ok(())
    }

    fn par(&mut self, par: &'a Par) -> Result<(), SpaceError> { self.push(Node::Par(par)) }

    fn optional(&mut self, par: &'a Option<Par>) -> Result<(), SpaceError> {
        match par {
            Some(par) => self.par(par),
            None => Ok(()),
        }
    }

    fn pars(&mut self, pars: &'a [Par]) -> Result<(), SpaceError> {
        pars.iter().try_for_each(|par| self.par(par))
    }

    fn signature(&mut self, signature: &'a Option<CostSignature>) -> Result<(), SpaceError> {
        match signature {
            Some(signature) => self.push(Node::Signature(signature)),
            None => Ok(()),
        }
    }
}

/// Scans `root` for a seq name of `seq` other than a name of `exempt`.
fn scan(
    root: &Par,
    seq: &BTreeSet<SpaceId>,
    exempt: Option<SpaceId>,
    meter: &ScanMeter,
) -> Result<(), SpaceError> {
    let mut work = Work {
        nodes: Vec::new(),
        meter,
    };
    work.par(root)?;
    while let Some(node) = work.nodes.pop() {
        meter.charge(1, 0, 0)?;
        match node {
            Node::Par(par) => scan_par(par, &mut work, seq, exempt, meter)?,
            Node::Expr(expr) => scan_expr(expr, &mut work)?,
            Node::Signature(signature) => scan_signature(signature, &mut work)?,
        }
    }
    Ok(())
}

/// Checks the unforgeables of `par` and adds every child of `par` to the work list.
fn scan_par<'a>(
    par: &'a Par,
    work: &mut Work<'a, '_>,
    seq: &BTreeSet<SpaceId>,
    exempt: Option<SpaceId>,
    meter: &ScanMeter,
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
    } = par;
    for unforgeable in unforgeables {
        check_unforgeable(unforgeable, seq, exempt, meter)?;
    }
    for send in sends {
        let Send {
            chan,
            data,
            persistent: _,
            locally_free: _,
            connective_used: _,
        } = send;
        work.optional(chan)?;
        work.pars(data)?;
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
            } = bind;
            work.pars(patterns)?;
            work.optional(source)?;
            work.signature(cost_signature)?;
        }
        work.optional(body)?;
        work.optional(condition)?;
    }
    for new in news {
        let New {
            bind_count: _,
            p,
            uri: _,
            injections,
            locally_free: _,
        } = new;
        work.optional(p)?;
        for injected in injections.values() {
            work.par(injected)?;
        }
    }
    for expr in exprs {
        work.push(Node::Expr(expr))?;
    }
    for matched in matches {
        let Match {
            target,
            cases,
            locally_free: _,
            connective_used: _,
        } = matched;
        work.optional(target)?;
        for case in cases {
            let MatchCase {
                pattern,
                source,
                free_count: _,
                guard,
            } = case;
            work.optional(pattern)?;
            work.optional(source)?;
            work.optional(guard)?;
        }
    }
    for bundle in bundles {
        let Bundle {
            body,
            write_flag: _,
            read_flag: _,
        } = bundle;
        work.optional(body)?;
    }
    for connective in connectives {
        let Connective {
            connective_instance,
        } = connective;
        match connective_instance {
            Some(ConnectiveInstance::ConnAndBody(ConnectiveBody { ps }))
            | Some(ConnectiveInstance::ConnOrBody(ConnectiveBody { ps })) => work.pars(ps)?,
            Some(ConnectiveInstance::ConnNotBody(negated)) => work.par(negated)?,
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
        work.optional(condition)?;
        work.optional(if_true)?;
        work.optional(if_false)?;
    }
    for term in cost_signed_terms {
        let CostSignedTerm { body, signature } = term;
        work.optional(body)?;
        work.signature(signature)?;
    }
    for stack in cost_stacks {
        let CostStack { cells } = stack;
        for cell in cells {
            work.push(Node::Signature(cell))?;
        }
    }
    Ok(())
}

/// Fails with `SeqSent` when `unforgeable` is a seq name of `seq` other than a name of `exempt`.
/// The read of the id is reserved first, and the lookup in `seq` too for a name of an ephemeral
/// space.
fn check_unforgeable(
    unforgeable: &GUnforgeable,
    seq: &BTreeSet<SpaceId>,
    exempt: Option<SpaceId>,
    meter: &ScanMeter,
) -> Result<(), SpaceError> {
    let GUnforgeable { unf_instance } = unforgeable;
    match unf_instance {
        Some(UnfInstance::GPrivateBody(GPrivate { id })) => {
            meter.charge(0, id.len(), 0)?;
            match decode_reified_id(id) {
                Some(ReifiedId::Name(space, _)) if space.durability == Durability::Ephemeral => {
                    meter.ordered_lookup(seq.len(), SID_LEN)?;
                    match seq.contains(&space.sid) && exempt != Some(space.sid) {
                        true => Err(SpaceError::SeqSent { space: space.sid }),
                        false => Ok(()),
                    }
                }
                Some(_) | None => Ok(()),
            }
        }
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
fn scan_expr<'a>(expr: &'a Expr, work: &mut Work<'a, '_>) -> Result<(), SpaceError> {
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
            | ExprInstance::GFloat32(_),
        )
        | None => Ok(()),
        Some(ExprInstance::ENotBody(ENot { p }) | ExprInstance::ENegBody(ENeg { p })) => {
            work.optional(p)
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
            work.optional(p1)?;
            work.optional(p2)
        }
        Some(ExprInstance::EVarBody(EVar { v: _ })) => Ok(()),
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
        Some(ExprInstance::EMapBody(EMap {
            kvs,
            locally_free: _,
            connective_used: _,
            remainder: _,
        })) => {
            for pair in kvs {
                let KeyValuePair { key, value } = pair;
                work.optional(key)?;
                work.optional(value)?;
            }
            Ok(())
        }
        Some(ExprInstance::EMethodBody(EMethod {
            method_name: _,
            target,
            arguments,
            locally_free: _,
            connective_used: _,
        })) => {
            work.optional(target)?;
            work.pars(arguments)
        }
        Some(ExprInstance::EZipperBody(EZipper {
            pathmap,
            current_path: _,
            is_write_zipper: _,
            locally_free: _,
            connective_used: _,
        })) => match pathmap {
            Some(EPathMap {
                ps,
                locally_free: _,
                connective_used: _,
                remainder: _,
            }) => work.pars(ps),
            None => Ok(()),
        },
        Some(ExprInstance::EMatchesBody(EMatches { target, pattern })) => {
            work.optional(target)?;
            work.optional(pattern)
        }
    }
}

/// Adds every `Par` and every nested signature of `signature` to the work list.
fn scan_signature<'a>(
    signature: &'a CostSignature,
    work: &mut Work<'a, '_>,
) -> Result<(), SpaceError> {
    let CostSignature { value } = signature;
    match value {
        Some(SignatureValue::Quote(par) | SignatureValue::Name(par)) => work.par(par),
        Some(SignatureValue::Compound(CostSignatureCompound { elements })) => elements
            .iter()
            .try_for_each(|element| work.push(Node::Signature(element))),
        Some(
            SignatureValue::Ground(_) | SignatureValue::BoundLevel(_) | SignatureValue::Unit(_),
        )
        | None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
    use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};

    use super::*;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::spaces::namespace::{self, gprivate, NameTail, NativeSpace};

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

    fn unmetered() -> ScanMeter { ScanMeter::new(None) }

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
                check_send(
                    &string("out"),
                    &[int(0), data],
                    &seq_of(&[seq]),
                    &unmetered()
                ),
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
                    check_send(&string("out"), &[data], &seq_of(&[seq]), &unmetered()),
                    Ok(()),
                    "{id:?}"
                );
            }
        }
        // With no seq space nothing is scanned, so a seq name of an ended space passes.
        assert_eq!(
            check_send(
                &string("out"),
                &[name(seq, 7)],
                &BTreeSet::new(),
                &unmetered()
            ),
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
            check_send(&op, &[handle(seq), args.clone()], &spaces, &unmetered()),
            Ok(())
        );
        // Without a bare handle first, the send to HANDLE_OP is scanned.
        assert_eq!(
            check_send(&op, &[string("h"), args], &spaces, &unmetered()),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
        // The channel argument of S's own produce may hold names of S.
        let call = |ch: Par, msg: Par| vec![string("r"), string("produce"), ch, msg];
        assert_eq!(
            check_send(
                &handle(seq),
                &call(x.clone(), list(vec![])),
                &spaces,
                &unmetered()
            ),
            Ok(())
        );
        // The message is scanned in full.
        assert_eq!(
            check_send(
                &handle(seq),
                &call(x.clone(), list(vec![x.clone()])),
                &spaces,
                &unmetered()
            ),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
        // A name of another seq space in the channel argument is sent.
        assert_eq!(
            check_send(
                &handle(seq),
                &call(name(other, 2), list(vec![])),
                &spaces,
                &unmetered()
            ),
            Err(SpaceError::SeqSent { space: other.sid })
        );
        // Only the method produce exempts its channel argument.
        assert_eq!(
            check_send(
                &handle(seq),
                &[string("r"), string("config"), x.clone()],
                &spaces,
                &unmetered()
            ),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
        // The handle of another space exempts nothing.
        assert_eq!(
            check_send(
                &handle(other),
                &call(x.clone(), list(vec![])),
                &spaces,
                &unmetered()
            ),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
        // The return name of a call is scanned.
        assert_eq!(
            check_send(
                &handle(seq),
                &[x, string("produce"), name(seq, 3), list(vec![])],
                &spaces,
                &unmetered()
            ),
            Err(SpaceError::SeqSent { space: seq.sid })
        );
    }

    #[test]
    fn reified_seq_scan_reserves_each_node_before_it_reads_it() {
        let seq = space(1, Durability::Ephemeral);
        let deep = (0..64).fold(int(0), |inner, _| list(vec![inner]));
        let budget = |operations: u64| {
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
            limits.set(
                HostWorkDimension::VerificationOperations,
                HostWorkLimit::new(operations),
            );
            ScanMeter::new(Some(HostWorkBudget::new(limits)))
        };
        // 64 lists, each a Par node and an Expr node, and the innermost Par and its Expr.
        assert!(check_send(
            &string("out"),
            &[deep.clone()],
            &seq_of(&[seq]),
            &budget(130)
        )
        .is_ok());
        assert!(matches!(
            check_send(&string("out"), &[deep], &seq_of(&[seq]), &budget(129)),
            Err(SpaceError::Host(RSpaceError::HostWorkRejected))
        ));
    }
}
