use std::collections::hash_map::RandomState;
use std::collections::HashMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Bundle, EPathMap, Expr, Par};
use models::rust::epathmap_trie_codec::EPathMapMode;
use num_bigint::BigInt;

use super::{extract_key_from_expr, unforg_from_proto, RhoExpr, RhoPathMap, RhoPathMapBinding};

#[derive(Clone, Copy)]
enum SequenceKind {
    Tuple,
    List,
    Set,
}

#[derive(Clone, Copy)]
enum UnaryKind {
    Not,
    Neg,
}

#[derive(Clone, Copy)]
enum BinaryKind {
    Plus,
    Minus,
    Mult,
    Div,
    Mod,
    Lt,
    Lte,
    Gt,
    Gte,
    Eq,
    Neq,
    And,
    Or,
    Concat,
    Interpolate,
    Diff,
    Matches,
}

enum Work {
    Par(Par),
    ParOrNil(Option<Par>),
    Expr(Expr),
    Bundle(Bundle),
    PathMap(EPathMap),
    Value(Option<RhoExpr>),
    FinishNil,
    FinishPar {
        arity: usize,
        has_process_fields: bool,
    },
    FinishSequence {
        kind: SequenceKind,
        arity: usize,
    },
    FinishLegacyMap {
        arity: usize,
    },
    FinishPathSet {
        arity: usize,
    },
    FinishPathMap {
        arity: usize,
    },
    FinishUnary(UnaryKind),
    FinishBinary(BinaryKind),
    FinishBundle {
        read: bool,
        write: bool,
    },
    FinishZipper {
        current_path: Vec<RhoExpr>,
    },
    FinishMethod {
        name: String,
        argument_count: usize,
    },
}

fn nil_expr() -> RhoExpr {
    RhoExpr::ExprUnknown {
        type_name: "Nil".to_owned(),
    }
}

fn take_values(values: &mut Vec<Option<RhoExpr>>, arity: usize) -> Vec<Option<RhoExpr>> {
    let first = values
        .len()
        .checked_sub(arity)
        .expect("RhoExpr PDA continuation arity exceeds its value stack");
    values.split_off(first)
}

fn required(value: Option<RhoExpr>) -> RhoExpr {
    value.expect("RhoExpr PDA required child did not apply its Nil fallback")
}

fn push_pars(work: &mut Vec<Work>, pars: Vec<Par>) {
    work.extend(pars.into_iter().rev().map(Work::Par));
}

fn drive(root: Work) -> Option<RhoExpr> {
    let mut work = Vec::with_capacity(64);
    let mut values = Vec::with_capacity(64);
    work.push(root);

    while let Some(step) = work.pop() {
        match step {
            Work::Value(value) => values.push(value),
            Work::ParOrNil(Some(par)) => {
                work.push(Work::FinishNil);
                work.push(Work::Par(par));
            }
            Work::ParOrNil(None) => values.push(Some(nil_expr())),
            Work::FinishNil => {
                let value = values
                    .pop()
                    .expect("RhoExpr PDA Nil continuation has no child");
                values.push(Some(value.unwrap_or_else(nil_expr)));
            }
            Work::Par(mut par) => {
                let has_process_fields = !par.sends.is_empty()
                    || !par.receives.is_empty()
                    || !par.news.is_empty()
                    || !par.matches.is_empty()
                    || !par.connectives.is_empty();
                let exprs = std::mem::take(&mut par.exprs);
                let unforgeables = std::mem::take(&mut par.unforgeables);
                let bundles = std::mem::take(&mut par.bundles);
                let arity = exprs.len() + unforgeables.len() + bundles.len();
                work.push(Work::FinishPar {
                    arity,
                    has_process_fields,
                });
                work.extend(bundles.into_iter().rev().map(Work::Bundle));
                work.extend(
                    unforgeables
                        .into_iter()
                        .rev()
                        .map(|unforg| Work::Value(unforg_from_proto(unforg))),
                );
                work.extend(exprs.into_iter().rev().map(Work::Expr));
            }
            Work::FinishPar {
                arity,
                has_process_fields,
            } => {
                let data: Vec<RhoExpr> = take_values(&mut values, arity)
                    .into_iter()
                    .flatten()
                    .collect();
                let value = match data.len() {
                    0 if has_process_fields => Some(RhoExpr::ExprUnknown {
                        type_name: "Process".to_owned(),
                    }),
                    0 => None,
                    1 => data.into_iter().next(),
                    _ => Some(RhoExpr::ExprPar { data }),
                };
                values.push(value);
            }
            Work::Bundle(bundle) => {
                work.push(Work::FinishBundle {
                    read: bundle.read_flag,
                    write: bundle.write_flag,
                });
                work.push(Work::ParOrNil(bundle.body));
            }
            Work::FinishBundle { read, write } => {
                let data = required(
                    values
                        .pop()
                        .expect("RhoExpr PDA bundle continuation has no body"),
                );
                values.push(Some(RhoExpr::ExprBundle {
                    data: Box::new(data),
                    read,
                    write,
                }));
            }
            Work::PathMap(pathmap) => match pathmap.mode() {
                EPathMapMode::Empty => values.push(Some(RhoExpr::ExprPathMap {
                    data: RhoPathMap::Empty,
                })),
                EPathMapMode::Set => {
                    let arity = pathmap.len();
                    work.push(Work::FinishPathSet { arity });
                    let first = work.len();
                    pathmap
                        .into_raw_set_entries(|key| {
                            let entry = models::rust::canonical_path::decode_trie_path(&key)
                                .expect("set-mode EPathMap keys are canonical Par paths");
                            work.push(Work::ParOrNil(Some(entry)));
                        })
                        .expect("set-mode EPathMap exposes set entries");
                    work[first..].reverse();
                }
                EPathMapMode::Map => {
                    let arity = pathmap.len();
                    work.push(Work::FinishPathMap { arity });
                    let first = work.len();
                    pathmap
                        .into_raw_map_entries(|key, value| {
                            let key = models::rust::canonical_path::decode_trie_path(&key)
                                .expect("map-mode EPathMap keys are canonical Par paths");
                            work.push(Work::ParOrNil(Some(key)));
                            work.push(Work::ParOrNil(Some(value)));
                        })
                        .expect("map-mode EPathMap exposes map entries");
                    work[first..].reverse();
                }
            },
            Work::FinishPathSet { arity } => {
                let entries = take_values(&mut values, arity)
                    .into_iter()
                    .map(required)
                    .collect();
                values.push(Some(RhoExpr::ExprPathMap {
                    data: RhoPathMap::Set { entries },
                }));
            }
            Work::FinishPathMap { arity } => {
                let mut produced = take_values(&mut values, arity * 2).into_iter();
                let mut entries = Vec::with_capacity(arity);
                while let Some(key) = produced.next() {
                    let value = produced
                        .next()
                        .expect("RhoExpr PDA map binding is missing its value");
                    entries.push(RhoPathMapBinding {
                        key: required(key),
                        value: required(value),
                    });
                }
                values.push(Some(RhoExpr::ExprPathMap {
                    data: RhoPathMap::Map { entries },
                }));
            }
            Work::Expr(mut expr) => {
                let Some(instance) = expr.expr_instance.take() else {
                    values.push(None);
                    continue;
                };
                match instance {
                    ExprInstance::GBool(data) => values.push(Some(RhoExpr::ExprBool { data })),
                    ExprInstance::GInt(data) => values.push(Some(RhoExpr::ExprInt { data })),
                    ExprInstance::GString(data) => {
                        values.push(Some(RhoExpr::ExprString { data }));
                    }
                    ExprInstance::GUri(data) => values.push(Some(RhoExpr::ExprUri { data })),
                    ExprInstance::GByteArray(data) => values.push(Some(RhoExpr::ExprBytes {
                        data: hex::encode(data),
                    })),
                    ExprInstance::GDouble(bits) => values.push(Some(RhoExpr::ExprFloat {
                        data: f64::from_bits(bits),
                    })),
                    ExprInstance::GBigInt(bytes) => values.push(Some(RhoExpr::ExprBigInt {
                        data: BigInt::from_signed_bytes_be(&bytes).to_string(),
                    })),
                    ExprInstance::GBigRat(rat) => values.push(Some(RhoExpr::ExprBigRat {
                        numerator: BigInt::from_signed_bytes_be(&rat.numerator).to_string(),
                        denominator: BigInt::from_signed_bytes_be(&rat.denominator).to_string(),
                    })),
                    ExprInstance::GFixedPoint(fp) => {
                        values.push(Some(RhoExpr::ExprFixedPoint {
                            value: BigInt::from_signed_bytes_be(&fp.unscaled).to_string(),
                            scale: fp.scale,
                        }));
                    }
                    ExprInstance::ETupleBody(tuple) => {
                        let arity = tuple.ps.len();
                        work.push(Work::FinishSequence {
                            kind: SequenceKind::Tuple,
                            arity,
                        });
                        push_pars(&mut work, tuple.ps);
                    }
                    ExprInstance::EListBody(list) => {
                        let arity = list.ps.len();
                        work.push(Work::FinishSequence {
                            kind: SequenceKind::List,
                            arity,
                        });
                        push_pars(&mut work, list.ps);
                    }
                    ExprInstance::ESetBody(set) => {
                        let arity = set.ps.len();
                        work.push(Work::FinishSequence {
                            kind: SequenceKind::Set,
                            arity,
                        });
                        push_pars(&mut work, set.ps);
                    }
                    ExprInstance::EMapBody(map) => {
                        let first = work.len();
                        let mut arity = 0;
                        for pair in map.kvs {
                            if let (Some(key), Some(value)) = (pair.key, pair.value) {
                                work.push(Work::Par(key));
                                work.push(Work::Par(value));
                                arity += 1;
                            }
                        }
                        work[first..].reverse();
                        work.insert(first, Work::FinishLegacyMap { arity });
                    }
                    ExprInstance::EPathmapBody(pathmap) => work.push(Work::PathMap(pathmap)),
                    ExprInstance::EZipperBody(zipper) => {
                        let current_path = zipper
                            .current_path
                            .into_iter()
                            .map(|bytes| RhoExpr::ExprBytes {
                                data: hex::encode(bytes),
                            })
                            .collect();
                        work.push(Work::FinishZipper { current_path });
                        match zipper.pathmap {
                            Some(pathmap) => work.push(Work::PathMap(pathmap)),
                            None => work.push(Work::Value(Some(RhoExpr::ExprPathMap {
                                data: RhoPathMap::Empty,
                            }))),
                        }
                    }
                    ExprInstance::ENotBody(op) => {
                        work.push(Work::FinishUnary(UnaryKind::Not));
                        work.push(Work::ParOrNil(op.p));
                    }
                    ExprInstance::ENegBody(op) => {
                        work.push(Work::FinishUnary(UnaryKind::Neg));
                        work.push(Work::ParOrNil(op.p));
                    }
                    ExprInstance::EPlusBody(op) => {
                        push_binary(&mut work, BinaryKind::Plus, op.p1, op.p2)
                    }
                    ExprInstance::EMinusBody(op) => {
                        push_binary(&mut work, BinaryKind::Minus, op.p1, op.p2)
                    }
                    ExprInstance::EMultBody(op) => {
                        push_binary(&mut work, BinaryKind::Mult, op.p1, op.p2)
                    }
                    ExprInstance::EDivBody(op) => {
                        push_binary(&mut work, BinaryKind::Div, op.p1, op.p2)
                    }
                    ExprInstance::EModBody(op) => {
                        push_binary(&mut work, BinaryKind::Mod, op.p1, op.p2)
                    }
                    ExprInstance::ELtBody(op) => {
                        push_binary(&mut work, BinaryKind::Lt, op.p1, op.p2)
                    }
                    ExprInstance::ELteBody(op) => {
                        push_binary(&mut work, BinaryKind::Lte, op.p1, op.p2)
                    }
                    ExprInstance::EGtBody(op) => {
                        push_binary(&mut work, BinaryKind::Gt, op.p1, op.p2)
                    }
                    ExprInstance::EGteBody(op) => {
                        push_binary(&mut work, BinaryKind::Gte, op.p1, op.p2)
                    }
                    ExprInstance::EEqBody(op) => {
                        push_binary(&mut work, BinaryKind::Eq, op.p1, op.p2)
                    }
                    ExprInstance::ENeqBody(op) => {
                        push_binary(&mut work, BinaryKind::Neq, op.p1, op.p2)
                    }
                    ExprInstance::EAndBody(op) => {
                        push_binary(&mut work, BinaryKind::And, op.p1, op.p2)
                    }
                    ExprInstance::EOrBody(op) => {
                        push_binary(&mut work, BinaryKind::Or, op.p1, op.p2)
                    }
                    ExprInstance::EPlusPlusBody(op) => {
                        push_binary(&mut work, BinaryKind::Concat, op.p1, op.p2)
                    }
                    ExprInstance::EPercentPercentBody(op) => {
                        push_binary(&mut work, BinaryKind::Interpolate, op.p1, op.p2)
                    }
                    ExprInstance::EMinusMinusBody(op) => {
                        push_binary(&mut work, BinaryKind::Diff, op.p1, op.p2)
                    }
                    ExprInstance::EMatchesBody(op) => {
                        push_binary(&mut work, BinaryKind::Matches, op.target, op.pattern)
                    }
                    ExprInstance::EMethodBody(method) => {
                        let argument_count = method.arguments.len();
                        work.push(Work::FinishMethod {
                            name: method.method_name,
                            argument_count,
                        });
                        push_pars(&mut work, method.arguments);
                        work.push(Work::ParOrNil(method.target));
                    }
                    ExprInstance::EVarBody(var) => {
                        let index = var
                            .v
                            .and_then(|value| value.var_instance)
                            .map(|value| match value {
                                models::rhoapi::var::VarInstance::BoundVar(index)
                                | models::rhoapi::var::VarInstance::FreeVar(index) => index,
                                models::rhoapi::var::VarInstance::Wildcard(_) => -1,
                            })
                            .unwrap_or(-1);
                        values.push(Some(RhoExpr::ExprVar { index }));
                    }
                }
            }
            Work::FinishSequence { kind, arity } => {
                let data = take_values(&mut values, arity)
                    .into_iter()
                    .flatten()
                    .collect();
                let value = match kind {
                    SequenceKind::Tuple => RhoExpr::ExprTuple { data },
                    SequenceKind::List => RhoExpr::ExprList { data },
                    SequenceKind::Set => RhoExpr::ExprSet { data },
                };
                values.push(Some(value));
            }
            Work::FinishLegacyMap { arity } => {
                let mut produced = take_values(&mut values, arity * 2).into_iter();
                let mut data = HashMap::with_capacity(arity);
                while let Some(key) = produced.next() {
                    let value = produced
                        .next()
                        .expect("RhoExpr PDA legacy map key is missing its value");
                    if let (Some(key), Some(value)) = (key, value) {
                        data.insert(extract_key_from_expr(&key), value);
                    }
                }
                values.push(Some(RhoExpr::ExprMap { data }));
            }
            Work::FinishUnary(kind) => {
                let data = Box::new(required(
                    values
                        .pop()
                        .expect("RhoExpr PDA unary continuation has no child"),
                ));
                values.push(Some(match kind {
                    UnaryKind::Not => RhoExpr::ExprNot { data },
                    UnaryKind::Neg => RhoExpr::ExprNeg { data },
                }));
            }
            Work::FinishBinary(kind) => {
                let mut pair = take_values(&mut values, 2).into_iter();
                let left = Box::new(required(pair.next().expect("binary left child is missing")));
                let right = Box::new(required(
                    pair.next().expect("binary right child is missing"),
                ));
                values.push(Some(build_binary(kind, left, right)));
            }
            Work::FinishZipper { current_path } => {
                let pathmap = required(
                    values
                        .pop()
                        .expect("RhoExpr PDA zipper continuation has no PathMap"),
                );
                values.push(Some(RhoExpr::ExprTuple {
                    data: vec![pathmap, RhoExpr::ExprList { data: current_path }],
                }));
            }
            Work::FinishMethod {
                name,
                argument_count,
            } => {
                let mut produced = take_values(&mut values, argument_count + 1).into_iter();
                let target = Box::new(required(
                    produced.next().expect("RhoExpr method target is missing"),
                ));
                let args = produced.flatten().collect();
                values.push(Some(RhoExpr::ExprMethod { target, name, args }));
            }
        }
    }

    assert_eq!(values.len(), 1, "RhoExpr PDA must terminate with one value");
    values.pop().expect("RhoExpr PDA final value disappeared")
}

fn push_binary(work: &mut Vec<Work>, kind: BinaryKind, left: Option<Par>, right: Option<Par>) {
    work.push(Work::FinishBinary(kind));
    work.push(Work::ParOrNil(right));
    work.push(Work::ParOrNil(left));
}

fn build_binary(kind: BinaryKind, left: Box<RhoExpr>, right: Box<RhoExpr>) -> RhoExpr {
    match kind {
        BinaryKind::Plus => RhoExpr::ExprPlus { left, right },
        BinaryKind::Minus => RhoExpr::ExprMinus { left, right },
        BinaryKind::Mult => RhoExpr::ExprMult { left, right },
        BinaryKind::Div => RhoExpr::ExprDiv { left, right },
        BinaryKind::Mod => RhoExpr::ExprMod { left, right },
        BinaryKind::Lt => RhoExpr::ExprLt { left, right },
        BinaryKind::Lte => RhoExpr::ExprLte { left, right },
        BinaryKind::Gt => RhoExpr::ExprGt { left, right },
        BinaryKind::Gte => RhoExpr::ExprGte { left, right },
        BinaryKind::Eq => RhoExpr::ExprEq { left, right },
        BinaryKind::Neq => RhoExpr::ExprNeq { left, right },
        BinaryKind::And => RhoExpr::ExprAnd { left, right },
        BinaryKind::Or => RhoExpr::ExprOr { left, right },
        BinaryKind::Concat => RhoExpr::ExprConcat { left, right },
        BinaryKind::Interpolate => RhoExpr::ExprInterpolate { left, right },
        BinaryKind::Diff => RhoExpr::ExprDiff { left, right },
        BinaryKind::Matches => RhoExpr::ExprMatches {
            target: left,
            pattern: right,
        },
    }
}

pub(super) fn from_par(par: Par) -> Option<RhoExpr> { drive(Work::Par(par)) }

#[cfg(test)]
pub(super) fn from_expr(expr: Expr) -> Option<RhoExpr> { drive(Work::Expr(expr)) }

#[cfg(test)]
pub(super) fn from_bundle(bundle: Bundle) -> Option<RhoExpr> { drive(Work::Bundle(bundle)) }

enum CloneSequenceKind {
    Par,
    Tuple,
    List,
    Set,
}

enum CloneWork<'a> {
    Expr(&'a RhoExpr),
    FinishSequence {
        kind: CloneSequenceKind,
        arity: usize,
    },
    FinishLegacyMap {
        keys: Vec<String>,
        capacity: usize,
        hasher: RandomState,
    },
    FinishPathSet {
        arity: usize,
    },
    FinishPathMap {
        arity: usize,
    },
    FinishUnary(UnaryKind),
    FinishBinary(BinaryKind),
    FinishBundle {
        read: bool,
        write: bool,
    },
    FinishMethod {
        name: String,
        argument_count: usize,
    },
}

fn clone_take_values(values: &mut Vec<RhoExpr>, arity: usize) -> Vec<RhoExpr> {
    let first = values
        .len()
        .checked_sub(arity)
        .expect("RhoExpr Clone PDA continuation arity exceeds its value stack");
    values.split_off(first)
}

fn clone_push_sequence<'a>(
    work: &mut Vec<CloneWork<'a>>,
    kind: CloneSequenceKind,
    data: &'a [RhoExpr],
) {
    work.push(CloneWork::FinishSequence {
        kind,
        arity: data.len(),
    });
    work.extend(data.iter().rev().map(CloneWork::Expr));
}

fn clone_push_binary<'a>(
    work: &mut Vec<CloneWork<'a>>,
    kind: BinaryKind,
    left: &'a RhoExpr,
    right: &'a RhoExpr,
) {
    work.push(CloneWork::FinishBinary(kind));
    work.push(CloneWork::Expr(right));
    work.push(CloneWork::Expr(left));
}

fn clone_rho_expr(root: &RhoExpr) -> RhoExpr {
    let mut work = Vec::with_capacity(64);
    let mut values = Vec::with_capacity(64);
    work.push(CloneWork::Expr(root));

    while let Some(step) = work.pop() {
        match step {
            CloneWork::Expr(expr) => match expr {
                RhoExpr::ExprPar { data } => {
                    clone_push_sequence(&mut work, CloneSequenceKind::Par, data)
                }
                RhoExpr::ExprTuple { data } => {
                    clone_push_sequence(&mut work, CloneSequenceKind::Tuple, data)
                }
                RhoExpr::ExprList { data } => {
                    clone_push_sequence(&mut work, CloneSequenceKind::List, data)
                }
                RhoExpr::ExprSet { data } => {
                    clone_push_sequence(&mut work, CloneSequenceKind::Set, data)
                }
                RhoExpr::ExprMap { data } => {
                    let entries: Vec<_> = data.iter().collect();
                    let keys = entries.iter().map(|(key, _)| (*key).clone()).collect();
                    work.push(CloneWork::FinishLegacyMap {
                        keys,
                        capacity: data.capacity(),
                        hasher: data.hasher().clone(),
                    });
                    work.extend(
                        entries
                            .into_iter()
                            .rev()
                            .map(|(_, value)| CloneWork::Expr(value)),
                    );
                }
                RhoExpr::ExprPathMap {
                    data: RhoPathMap::Empty,
                } => {
                    values.push(RhoExpr::ExprPathMap {
                        data: RhoPathMap::Empty,
                    });
                }
                RhoExpr::ExprPathMap {
                    data: RhoPathMap::Set { entries },
                } => {
                    work.push(CloneWork::FinishPathSet {
                        arity: entries.len(),
                    });
                    work.extend(entries.iter().rev().map(CloneWork::Expr));
                }
                RhoExpr::ExprPathMap {
                    data: RhoPathMap::Map { entries },
                } => {
                    work.push(CloneWork::FinishPathMap {
                        arity: entries.len(),
                    });
                    for binding in entries.iter().rev() {
                        work.push(CloneWork::Expr(&binding.value));
                        work.push(CloneWork::Expr(&binding.key));
                    }
                }
                RhoExpr::ExprBool { data } => values.push(RhoExpr::ExprBool { data: *data }),
                RhoExpr::ExprInt { data } => values.push(RhoExpr::ExprInt { data: *data }),
                RhoExpr::ExprString { data } => {
                    values.push(RhoExpr::ExprString { data: data.clone() });
                }
                RhoExpr::ExprUri { data } => {
                    values.push(RhoExpr::ExprUri { data: data.clone() });
                }
                RhoExpr::ExprBytes { data } => {
                    values.push(RhoExpr::ExprBytes { data: data.clone() });
                }
                RhoExpr::ExprFloat { data } => values.push(RhoExpr::ExprFloat { data: *data }),
                RhoExpr::ExprBigInt { data } => {
                    values.push(RhoExpr::ExprBigInt { data: data.clone() });
                }
                RhoExpr::ExprBigRat {
                    numerator,
                    denominator,
                } => values.push(RhoExpr::ExprBigRat {
                    numerator: numerator.clone(),
                    denominator: denominator.clone(),
                }),
                RhoExpr::ExprFixedPoint { value, scale } => {
                    values.push(RhoExpr::ExprFixedPoint {
                        value: value.clone(),
                        scale: *scale,
                    });
                }
                RhoExpr::ExprUnforg { data } => {
                    values.push(RhoExpr::ExprUnforg { data: data.clone() });
                }
                RhoExpr::ExprBundle { data, read, write } => {
                    work.push(CloneWork::FinishBundle {
                        read: *read,
                        write: *write,
                    });
                    work.push(CloneWork::Expr(data));
                }
                RhoExpr::ExprNot { data } => {
                    work.push(CloneWork::FinishUnary(UnaryKind::Not));
                    work.push(CloneWork::Expr(data));
                }
                RhoExpr::ExprNeg { data } => {
                    work.push(CloneWork::FinishUnary(UnaryKind::Neg));
                    work.push(CloneWork::Expr(data));
                }
                RhoExpr::ExprPlus { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Plus, left, right)
                }
                RhoExpr::ExprMinus { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Minus, left, right)
                }
                RhoExpr::ExprMult { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Mult, left, right)
                }
                RhoExpr::ExprDiv { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Div, left, right)
                }
                RhoExpr::ExprMod { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Mod, left, right)
                }
                RhoExpr::ExprLt { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Lt, left, right)
                }
                RhoExpr::ExprLte { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Lte, left, right)
                }
                RhoExpr::ExprGt { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Gt, left, right)
                }
                RhoExpr::ExprGte { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Gte, left, right)
                }
                RhoExpr::ExprEq { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Eq, left, right)
                }
                RhoExpr::ExprNeq { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Neq, left, right)
                }
                RhoExpr::ExprAnd { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::And, left, right)
                }
                RhoExpr::ExprOr { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Or, left, right)
                }
                RhoExpr::ExprConcat { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Concat, left, right)
                }
                RhoExpr::ExprInterpolate { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Interpolate, left, right)
                }
                RhoExpr::ExprDiff { left, right } => {
                    clone_push_binary(&mut work, BinaryKind::Diff, left, right)
                }
                RhoExpr::ExprMatches { target, pattern } => {
                    clone_push_binary(&mut work, BinaryKind::Matches, target, pattern)
                }
                RhoExpr::ExprMethod { target, name, args } => {
                    work.push(CloneWork::FinishMethod {
                        name: name.clone(),
                        argument_count: args.len(),
                    });
                    work.extend(args.iter().rev().map(CloneWork::Expr));
                    work.push(CloneWork::Expr(target));
                }
                RhoExpr::ExprVar { index } => {
                    values.push(RhoExpr::ExprVar { index: *index });
                }
                RhoExpr::ExprSysAuthToken => values.push(RhoExpr::ExprSysAuthToken),
                RhoExpr::ExprUnknown { type_name } => values.push(RhoExpr::ExprUnknown {
                    type_name: type_name.clone(),
                }),
            },
            CloneWork::FinishSequence { kind, arity } => {
                let data = clone_take_values(&mut values, arity);
                values.push(match kind {
                    CloneSequenceKind::Par => RhoExpr::ExprPar { data },
                    CloneSequenceKind::Tuple => RhoExpr::ExprTuple { data },
                    CloneSequenceKind::List => RhoExpr::ExprList { data },
                    CloneSequenceKind::Set => RhoExpr::ExprSet { data },
                });
            }
            CloneWork::FinishLegacyMap {
                keys,
                capacity,
                hasher,
            } => {
                let cloned_values = clone_take_values(&mut values, keys.len());
                let mut data = HashMap::with_capacity_and_hasher(capacity, hasher);
                data.extend(keys.into_iter().zip(cloned_values));
                values.push(RhoExpr::ExprMap { data });
            }
            CloneWork::FinishPathSet { arity } => {
                let entries = clone_take_values(&mut values, arity);
                values.push(RhoExpr::ExprPathMap {
                    data: RhoPathMap::Set { entries },
                });
            }
            CloneWork::FinishPathMap { arity } => {
                let mut cloned = clone_take_values(&mut values, arity * 2).into_iter();
                let mut entries = Vec::with_capacity(arity);
                while let Some(key) = cloned.next() {
                    entries.push(RhoPathMapBinding {
                        key,
                        value: cloned
                            .next()
                            .expect("RhoExpr Clone PDA map binding is missing its value"),
                    });
                }
                values.push(RhoExpr::ExprPathMap {
                    data: RhoPathMap::Map { entries },
                });
            }
            CloneWork::FinishUnary(kind) => {
                let data = Box::new(
                    values
                        .pop()
                        .expect("RhoExpr Clone PDA unary continuation has no child"),
                );
                values.push(match kind {
                    UnaryKind::Not => RhoExpr::ExprNot { data },
                    UnaryKind::Neg => RhoExpr::ExprNeg { data },
                });
            }
            CloneWork::FinishBinary(kind) => {
                let mut pair = clone_take_values(&mut values, 2).into_iter();
                let left = Box::new(pair.next().expect("cloned binary left child is missing"));
                let right = Box::new(pair.next().expect("cloned binary right child is missing"));
                values.push(build_binary(kind, left, right));
            }
            CloneWork::FinishBundle { read, write } => {
                let data = values
                    .pop()
                    .expect("RhoExpr Clone PDA bundle continuation has no child");
                values.push(RhoExpr::ExprBundle {
                    data: Box::new(data),
                    read,
                    write,
                });
            }
            CloneWork::FinishMethod {
                name,
                argument_count,
            } => {
                let mut cloned = clone_take_values(&mut values, argument_count + 1).into_iter();
                let target = Box::new(
                    cloned
                        .next()
                        .expect("RhoExpr Clone PDA method target is missing"),
                );
                values.push(RhoExpr::ExprMethod {
                    target,
                    name,
                    args: cloned.collect(),
                });
            }
        }
    }

    assert_eq!(values.len(), 1, "RhoExpr Clone PDA must produce one value");
    values
        .pop()
        .expect("RhoExpr Clone PDA final value disappeared")
}

fn take_box(boxed: &mut Box<RhoExpr>) -> RhoExpr {
    std::mem::replace(boxed.as_mut(), RhoExpr::ExprSysAuthToken)
}

fn take_children(expr: &mut RhoExpr, out: &mut Vec<RhoExpr>) {
    match expr {
        RhoExpr::ExprPar { data }
        | RhoExpr::ExprTuple { data }
        | RhoExpr::ExprList { data }
        | RhoExpr::ExprSet { data } => out.append(data),
        RhoExpr::ExprMap { data } => out.extend(std::mem::take(data).into_values()),
        RhoExpr::ExprPathMap { data } => match data {
            RhoPathMap::Empty => {}
            RhoPathMap::Set { entries } => out.append(entries),
            RhoPathMap::Map { entries } => {
                for binding in std::mem::take(entries) {
                    out.push(binding.key);
                    out.push(binding.value);
                }
            }
        },
        RhoExpr::ExprBundle { data, .. }
        | RhoExpr::ExprNot { data }
        | RhoExpr::ExprNeg { data } => out.push(take_box(data)),
        RhoExpr::ExprPlus { left, right }
        | RhoExpr::ExprMinus { left, right }
        | RhoExpr::ExprMult { left, right }
        | RhoExpr::ExprDiv { left, right }
        | RhoExpr::ExprMod { left, right }
        | RhoExpr::ExprLt { left, right }
        | RhoExpr::ExprLte { left, right }
        | RhoExpr::ExprGt { left, right }
        | RhoExpr::ExprGte { left, right }
        | RhoExpr::ExprEq { left, right }
        | RhoExpr::ExprNeq { left, right }
        | RhoExpr::ExprAnd { left, right }
        | RhoExpr::ExprOr { left, right }
        | RhoExpr::ExprConcat { left, right }
        | RhoExpr::ExprInterpolate { left, right }
        | RhoExpr::ExprDiff { left, right } => {
            out.push(take_box(left));
            out.push(take_box(right));
        }
        RhoExpr::ExprMatches { target, pattern } => {
            out.push(take_box(target));
            out.push(take_box(pattern));
        }
        RhoExpr::ExprMethod { target, args, .. } => {
            out.push(take_box(target));
            out.append(args);
        }
        RhoExpr::ExprBool { .. }
        | RhoExpr::ExprInt { .. }
        | RhoExpr::ExprString { .. }
        | RhoExpr::ExprUri { .. }
        | RhoExpr::ExprBytes { .. }
        | RhoExpr::ExprFloat { .. }
        | RhoExpr::ExprBigInt { .. }
        | RhoExpr::ExprBigRat { .. }
        | RhoExpr::ExprFixedPoint { .. }
        | RhoExpr::ExprUnforg { .. }
        | RhoExpr::ExprVar { .. }
        | RhoExpr::ExprSysAuthToken
        | RhoExpr::ExprUnknown { .. } => {}
    }
}

enum JsonWork<'a> {
    Expr(&'a RhoExpr),
    PathMap(&'a RhoPathMap),
    Binding(&'a RhoPathMapBinding),
    Unforg(&'a super::RhoUnforg),
    Text(&'static str),
    String(&'a str),
    Bool(bool),
    I64(i64),
    U32(u32),
    F64(f64),
}

fn json_push_expr_array<'a>(work: &mut Vec<JsonWork<'a>>, data: &'a [RhoExpr]) {
    work.push(JsonWork::Text("]"));
    for (index, value) in data.iter().enumerate().rev() {
        work.push(JsonWork::Expr(value));
        if index > 0 {
            work.push(JsonWork::Text(","));
        }
    }
    work.push(JsonWork::Text("["));
}

fn json_push_bindings<'a>(work: &mut Vec<JsonWork<'a>>, entries: &'a [RhoPathMapBinding]) {
    work.push(JsonWork::Text("]"));
    for (index, binding) in entries.iter().enumerate().rev() {
        work.push(JsonWork::Binding(binding));
        if index > 0 {
            work.push(JsonWork::Text(","));
        }
    }
    work.push(JsonWork::Text("["));
}

fn json_push_binary<'a>(
    work: &mut Vec<JsonWork<'a>>,
    prefix: &'static str,
    left: &'a RhoExpr,
    right: &'a RhoExpr,
    output: &mut String,
) {
    output.push_str(prefix);
    work.push(JsonWork::Text("}}"));
    work.push(JsonWork::Expr(right));
    work.push(JsonWork::Text(",\"right\":"));
    work.push(JsonWork::Expr(left));
}

fn rho_expr_json(root: &RhoExpr) -> String {
    let mut output = String::with_capacity(256);
    let mut work = Vec::with_capacity(64);
    work.push(JsonWork::Expr(root));

    while let Some(step) = work.pop() {
        match step {
            JsonWork::Text(text) => output.push_str(text),
            JsonWork::String(value) => output.push_str(
                &serde_json::to_string(value).expect("serializing one JSON string cannot fail"),
            ),
            JsonWork::Bool(value) => output.push_str(if value { "true" } else { "false" }),
            JsonWork::I64(value) => output.push_str(&value.to_string()),
            JsonWork::U32(value) => output.push_str(&value.to_string()),
            JsonWork::F64(value) => output.push_str(
                &serde_json::to_string(&value).expect("serializing one JSON float cannot fail"),
            ),
            JsonWork::Expr(expr) => match expr {
                RhoExpr::ExprPar { data } => {
                    output.push_str("{\"ExprPar\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    json_push_expr_array(&mut work, data);
                }
                RhoExpr::ExprTuple { data } => {
                    output.push_str("{\"ExprTuple\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    json_push_expr_array(&mut work, data);
                }
                RhoExpr::ExprList { data } => {
                    output.push_str("{\"ExprList\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    json_push_expr_array(&mut work, data);
                }
                RhoExpr::ExprSet { data } => {
                    output.push_str("{\"ExprSet\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    json_push_expr_array(&mut work, data);
                }
                RhoExpr::ExprMap { data } => {
                    output.push_str("{\"ExprMap\":{\"data\":{");
                    work.push(JsonWork::Text("}}}"));
                    let entries: Vec<_> = data.iter().collect();
                    for (index, (key, value)) in entries.into_iter().enumerate().rev() {
                        work.push(JsonWork::Expr(value));
                        work.push(JsonWork::Text(":"));
                        work.push(JsonWork::String(key));
                        if index > 0 {
                            work.push(JsonWork::Text(","));
                        }
                    }
                }
                RhoExpr::ExprPathMap { data } => {
                    output.push_str("{\"ExprPathMap\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::PathMap(data));
                }
                RhoExpr::ExprBool { data } => {
                    output.push_str("{\"ExprBool\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::Bool(*data));
                }
                RhoExpr::ExprInt { data } => {
                    output.push_str("{\"ExprInt\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::I64(*data));
                }
                RhoExpr::ExprString { data } => {
                    output.push_str("{\"ExprString\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::String(data));
                }
                RhoExpr::ExprUri { data } => {
                    output.push_str("{\"ExprUri\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::String(data));
                }
                RhoExpr::ExprBytes { data } => {
                    output.push_str("{\"ExprBytes\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::String(data));
                }
                RhoExpr::ExprFloat { data } => {
                    output.push_str("{\"ExprFloat\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::F64(*data));
                }
                RhoExpr::ExprBigInt { data } => {
                    output.push_str("{\"ExprBigInt\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::String(data));
                }
                RhoExpr::ExprBigRat {
                    numerator,
                    denominator,
                } => {
                    output.push_str("{\"ExprBigRat\":{\"numerator\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::String(denominator));
                    work.push(JsonWork::Text(",\"denominator\":"));
                    work.push(JsonWork::String(numerator));
                }
                RhoExpr::ExprFixedPoint { value, scale } => {
                    output.push_str("{\"ExprFixedPoint\":{\"value\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::U32(*scale));
                    work.push(JsonWork::Text(",\"scale\":"));
                    work.push(JsonWork::String(value));
                }
                RhoExpr::ExprUnforg { data } => {
                    output.push_str("{\"ExprUnforg\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::Unforg(data));
                }
                RhoExpr::ExprBundle { data, read, write } => {
                    output.push_str("{\"ExprBundle\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::Bool(*write));
                    work.push(JsonWork::Text(",\"write\":"));
                    work.push(JsonWork::Bool(*read));
                    work.push(JsonWork::Text(",\"read\":"));
                    work.push(JsonWork::Expr(data));
                }
                RhoExpr::ExprNot { data } => {
                    output.push_str("{\"ExprNot\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::Expr(data));
                }
                RhoExpr::ExprNeg { data } => {
                    output.push_str("{\"ExprNeg\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::Expr(data));
                }
                RhoExpr::ExprPlus { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprPlus\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprMinus { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprMinus\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprMult { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprMult\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprDiv { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprDiv\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprMod { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprMod\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprLt { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprLt\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprLte { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprLte\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprGt { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprGt\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprGte { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprGte\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprEq { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprEq\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprNeq { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprNeq\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprAnd { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprAnd\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprOr { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprOr\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprConcat { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprConcat\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprInterpolate { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprInterpolate\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprDiff { left, right } => json_push_binary(
                    &mut work,
                    "{\"ExprDiff\":{\"left\":",
                    left,
                    right,
                    &mut output,
                ),
                RhoExpr::ExprMatches { target, pattern } => {
                    output.push_str("{\"ExprMatches\":{\"target\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::Expr(pattern));
                    work.push(JsonWork::Text(",\"pattern\":"));
                    work.push(JsonWork::Expr(target));
                }
                RhoExpr::ExprMethod { target, name, args } => {
                    output.push_str("{\"ExprMethod\":{\"target\":");
                    work.push(JsonWork::Text("}}"));
                    json_push_expr_array(&mut work, args);
                    work.push(JsonWork::Text(",\"args\":"));
                    work.push(JsonWork::String(name));
                    work.push(JsonWork::Text(",\"name\":"));
                    work.push(JsonWork::Expr(target));
                }
                RhoExpr::ExprVar { index } => {
                    output.push_str("{\"ExprVar\":{\"index\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::I64(i64::from(*index)));
                }
                RhoExpr::ExprSysAuthToken => output.push_str("\"ExprSysAuthToken\""),
                RhoExpr::ExprUnknown { type_name } => {
                    output.push_str("{\"ExprUnknown\":{\"type_name\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::String(type_name));
                }
            },
            JsonWork::PathMap(pathmap) => match pathmap {
                RhoPathMap::Empty => output.push_str("\"Empty\""),
                RhoPathMap::Set { entries } => {
                    output.push_str("{\"Set\":{\"entries\":");
                    work.push(JsonWork::Text("}}"));
                    json_push_expr_array(&mut work, entries);
                }
                RhoPathMap::Map { entries } => {
                    output.push_str("{\"Map\":{\"entries\":");
                    work.push(JsonWork::Text("}}"));
                    json_push_bindings(&mut work, entries);
                }
            },
            JsonWork::Binding(binding) => {
                output.push_str("{\"key\":");
                work.push(JsonWork::Text("}"));
                work.push(JsonWork::Expr(&binding.value));
                work.push(JsonWork::Text(",\"value\":"));
                work.push(JsonWork::Expr(&binding.key));
            }
            JsonWork::Unforg(unforg) => match unforg {
                super::RhoUnforg::UnforgPrivate { data } => {
                    output.push_str("{\"UnforgPrivate\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::String(data));
                }
                super::RhoUnforg::UnforgDeploy { data } => {
                    output.push_str("{\"UnforgDeploy\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::String(data));
                }
                super::RhoUnforg::UnforgDeployer { data } => {
                    output.push_str("{\"UnforgDeployer\":{\"data\":");
                    work.push(JsonWork::Text("}}"));
                    work.push(JsonWork::String(data));
                }
                super::RhoUnforg::UnforgSysAuthToken => {
                    output.push_str("\"UnforgSysAuthToken\"");
                }
            },
        }
    }

    output
}

impl Clone for RhoExpr {
    fn clone(&self) -> Self { clone_rho_expr(self) }
}

impl Drop for RhoExpr {
    fn drop(&mut self) {
        let mut work = Vec::new();
        take_children(self, &mut work);
        while let Some(mut child) = work.pop() {
            take_children(&mut child, &mut work);
        }
    }
}

impl std::fmt::Debug for RhoExpr {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&rho_expr_json(self))
    }
}

impl serde::Serialize for RhoExpr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error;

        let raw = serde_json::value::RawValue::from_string(rho_expr_json(self))
            .map_err(S::Error::custom)?;
        raw.serialize(serializer)
    }
}
