//! Test-only recursive specification for the production RhoExpr conversion PDA.
//!
//! This is the bounded differential oracle that preserves the implementation
//! replaced by `rho_expr_pda`. It must never be used by production code or by
//! deep-stack tests: its purpose is to make shallow semantic equivalence
//! executable while the production implementation remains stack-safe.

use std::collections::HashMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Bundle, EPathMap, Expr, Par};
use models::rust::epathmap_trie_codec::EPathMapMode;
use num_bigint::BigInt;

use super::{extract_key_from_expr, unforg_from_proto, RhoExpr, RhoPathMap, RhoPathMapBinding};

pub(super) fn from_par(mut par: Par) -> Option<RhoExpr> {
    let has_process_fields = !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.connectives.is_empty();

    let exprs = std::mem::take(&mut par.exprs)
        .into_iter()
        .filter_map(from_expr);
    let unforgeables = std::mem::take(&mut par.unforgeables)
        .into_iter()
        .filter_map(unforg_from_proto);
    let bundles = std::mem::take(&mut par.bundles)
        .into_iter()
        .filter_map(from_bundle);
    let data: Vec<_> = exprs.chain(unforgeables).chain(bundles).collect();

    match data.len() {
        0 if has_process_fields => Some(RhoExpr::ExprUnknown {
            type_name: "Process".to_owned(),
        }),
        0 => None,
        1 => data.into_iter().next(),
        _ => Some(RhoExpr::ExprPar { data }),
    }
}

fn nil_or(par: Option<Par>) -> RhoExpr {
    par.and_then(from_par).unwrap_or(RhoExpr::ExprUnknown {
        type_name: "Nil".to_owned(),
    })
}

fn from_bundle(bundle: Bundle) -> Option<RhoExpr> {
    Some(RhoExpr::ExprBundle {
        data: Box::new(nil_or(bundle.body)),
        read: bundle.read_flag,
        write: bundle.write_flag,
    })
}

fn from_pathmap(pathmap: EPathMap) -> RhoExpr {
    let data = match pathmap.mode() {
        EPathMapMode::Empty => RhoPathMap::Empty,
        EPathMapMode::Set => {
            let mut entries = Vec::with_capacity(pathmap.len());
            pathmap
                .entry_trie()
                .for_each_raw_set_entry(|key| {
                    let entry = models::rust::canonical_path::decode_trie_path(key)
                        .expect("set-mode EPathMap keys are canonical Par paths");
                    entries.push(nil_or(Some(entry)));
                })
                .expect("set-mode EPathMap exposes set entries");
            RhoPathMap::Set { entries }
        }
        EPathMapMode::Map => {
            let mut entries = Vec::with_capacity(pathmap.len());
            pathmap
                .entry_trie()
                .for_each_raw_map_entry(|key, value| {
                    let key = models::rust::canonical_path::decode_trie_path(key)
                        .expect("map-mode EPathMap keys are canonical Par paths");
                    entries.push(RhoPathMapBinding {
                        key: nil_or(Some(key)),
                        value: nil_or(Some(value.clone())),
                    });
                })
                .expect("map-mode EPathMap exposes map entries");
            RhoPathMap::Map { entries }
        }
    };
    RhoExpr::ExprPathMap { data }
}

macro_rules! binary {
    ($variant:ident, $op:expr) => {{
        let op = $op;
        RhoExpr::$variant {
            left: Box::new(nil_or(op.p1)),
            right: Box::new(nil_or(op.p2)),
        }
    }};
}

pub(super) fn from_expr(expr: Expr) -> Option<RhoExpr> {
    Some(match expr.expr_instance? {
        ExprInstance::GBool(data) => RhoExpr::ExprBool { data },
        ExprInstance::GInt(data) => RhoExpr::ExprInt { data },
        ExprInstance::GString(data) => RhoExpr::ExprString { data },
        ExprInstance::GUri(data) => RhoExpr::ExprUri { data },
        ExprInstance::GByteArray(data) => RhoExpr::ExprBytes {
            data: hex::encode(data),
        },
        ExprInstance::GDouble(bits) => RhoExpr::ExprFloat {
            data: f64::from_bits(bits),
        },
        ExprInstance::GBigInt(bytes) => RhoExpr::ExprBigInt {
            data: BigInt::from_signed_bytes_be(&bytes).to_string(),
        },
        ExprInstance::GBigRat(rational) => RhoExpr::ExprBigRat {
            numerator: BigInt::from_signed_bytes_be(&rational.numerator).to_string(),
            denominator: BigInt::from_signed_bytes_be(&rational.denominator).to_string(),
        },
        ExprInstance::GFixedPoint(fixed) => RhoExpr::ExprFixedPoint {
            value: BigInt::from_signed_bytes_be(&fixed.unscaled).to_string(),
            scale: fixed.scale,
        },
        ExprInstance::ETupleBody(tuple) => RhoExpr::ExprTuple {
            data: tuple.ps.into_iter().filter_map(from_par).collect(),
        },
        ExprInstance::EListBody(list) => RhoExpr::ExprList {
            data: list.ps.into_iter().filter_map(from_par).collect(),
        },
        ExprInstance::ESetBody(set) => RhoExpr::ExprSet {
            data: set.ps.into_iter().filter_map(from_par).collect(),
        },
        ExprInstance::EMapBody(map) => {
            let mut data = HashMap::new();
            for pair in map.kvs {
                if let (Some(key), Some(value)) = (pair.key, pair.value) {
                    if let (Some(key), Some(value)) = (from_par(key), from_par(value)) {
                        data.insert(extract_key_from_expr(&key), value);
                    }
                }
            }
            RhoExpr::ExprMap { data }
        }
        ExprInstance::EPathmapBody(pathmap) => from_pathmap(pathmap),
        ExprInstance::EZipperBody(zipper) => RhoExpr::ExprTuple {
            data: vec![
                zipper
                    .pathmap
                    .map(from_pathmap)
                    .unwrap_or(RhoExpr::ExprPathMap {
                        data: RhoPathMap::Empty,
                    }),
                RhoExpr::ExprList {
                    data: zipper
                        .current_path
                        .into_iter()
                        .map(|bytes| RhoExpr::ExprBytes {
                            data: hex::encode(bytes),
                        })
                        .collect(),
                },
            ],
        },
        ExprInstance::ENotBody(op) => RhoExpr::ExprNot {
            data: Box::new(nil_or(op.p)),
        },
        ExprInstance::ENegBody(op) => RhoExpr::ExprNeg {
            data: Box::new(nil_or(op.p)),
        },
        ExprInstance::EPlusBody(op) => binary!(ExprPlus, op),
        ExprInstance::EMinusBody(op) => binary!(ExprMinus, op),
        ExprInstance::EMultBody(op) => binary!(ExprMult, op),
        ExprInstance::EDivBody(op) => binary!(ExprDiv, op),
        ExprInstance::EModBody(op) => binary!(ExprMod, op),
        ExprInstance::ELtBody(op) => binary!(ExprLt, op),
        ExprInstance::ELteBody(op) => binary!(ExprLte, op),
        ExprInstance::EGtBody(op) => binary!(ExprGt, op),
        ExprInstance::EGteBody(op) => binary!(ExprGte, op),
        ExprInstance::EEqBody(op) => binary!(ExprEq, op),
        ExprInstance::ENeqBody(op) => binary!(ExprNeq, op),
        ExprInstance::EAndBody(op) => binary!(ExprAnd, op),
        ExprInstance::EOrBody(op) => binary!(ExprOr, op),
        ExprInstance::EPlusPlusBody(op) => binary!(ExprConcat, op),
        ExprInstance::EPercentPercentBody(op) => binary!(ExprInterpolate, op),
        ExprInstance::EMinusMinusBody(op) => binary!(ExprDiff, op),
        ExprInstance::EMatchesBody(op) => RhoExpr::ExprMatches {
            target: Box::new(nil_or(op.target)),
            pattern: Box::new(nil_or(op.pattern)),
        },
        ExprInstance::EMethodBody(method) => RhoExpr::ExprMethod {
            target: Box::new(nil_or(method.target)),
            name: method.method_name,
            args: method.arguments.into_iter().filter_map(from_par).collect(),
        },
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
            RhoExpr::ExprVar { index }
        }
    })
}
