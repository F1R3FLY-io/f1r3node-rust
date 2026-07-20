//! EPathMap fix P4.3 — SPLICED EVENT-HASH SERIALIZATION (amendment PM-1).
//!
//! Event hashes cover `bincode(vec![channel_hash_bytes, bincode(datum),
//! bincode(persist)])` (produce) and the per-pattern / continuation bincode
//! legs (consume) — see `rspace_plus_plus::…::stable_hash_provider`. The
//! datum leg re-serializes the WHOLE `ListParWithRandom` per produce, which
//! is size-proportional in the (potentially huge) EPathMap payload. A manual
//! `Serialize` impl cannot raw-splice cached bytes into a serde stream
//! (`serialize_bytes` length-prefixes; serde has no raw-bytes primitive), so
//! this module is the PM-1 HAND-ROLLED emitter for exactly the container
//! spine `datum → Vec<Par> → Par → Expr → Option<ExprInstance> → variant →
//! EPathMap`: it reproduces the bincode 1.3.3 legacy-default layout
//! byte-for-byte, SPLICES `InternedEPathMap.serde_bytes` at filled-cell
//! `EPathMap` nodes (populating that P1 `OnceLock` lazily, here), and uses
//! black-box `bincode::serialize` for every interned-map-free subtree —
//! sound because bincode 1.3.3's encoding is COMPOSITIONAL:
//! `serialize(&vec) = u64-LE len ++ concat(elem encodings)` and a struct is
//! the plain concatenation of its field encodings.
//!
//! FULL LAYOUT RULE SET (bincode 1.3.3 `bincode::serialize` defaults —
//! fixint, little-endian, the exact config the direct path uses):
//!   * `Vec<T>` / `BTreeMap<K, V>` / bytes / `String`: u64-LE length, then
//!     the elements (map entries as key ++ value, in order; `String` as its
//!     UTF-8 bytes);
//!   * `Option<T>`: ONE tag byte (0 = `None`, 1 = `Some`), then `T`;
//!   * enums: u32-LE variant index in DECLARATION order, then the payload;
//!   * structs: fields concatenated in DECLARATION order (names never hit
//!     the wire);
//!   * integers: fixint LE (`i32`/`u32` 4 bytes, `i64`/`u64` 8 bytes);
//!     `bool`: one byte;
//!   * `locally_free`: EVERY `.rhoapi` message serializes it as EMPTY bytes
//!     (`serialize_as_empty_bytes` → `serialize_bytes(&[])` → u64-LE 0) —
//!     the serialize-only normalization models/build.rs injects; the
//!     emitter honors it at EVERY hand-walked level, and the black-box
//!     fallback honors it via the derives.
//!
//! DISPATCH: the intern-aware path is taken only when the value CONTAINS a
//! filled-cell `EPathMap` (`EPathMap::interned_handle()` — a read-only cell
//! peek that never forces an intern); otherwise the existing direct
//! `bincode::serialize` runs, byte-identically and with zero new work. The
//! contains-scan short-circuits at filled cells and descends unfilled maps
//! (a filled inner map inside an unfilled outer map still splices).
//!
//! SPLICE UNIT: `InternedEPathMap.serde_bytes` = `bincode::serialize` of the
//! source `EPathMap` — the wrapper's derived serde impl (P3) already skips
//! the cell and blanks `locally_free` at every level, so the cached bytes
//! ARE the direct path's bytes for that subtree. The cache is sound to
//! share across the intern family: the store is content-addressed by
//! canonical prost bytes, which determine every field value, which
//! determine the serde bytes.
//!
//! GATES (risk R7): the P0 event-hash goldens re-asserted with filled cells
//! (`epathmap_canonical_fixtures`), the spliced-vs-direct proptest
//! (`epathmap_spliced_event_bytes` — arbitrary map-bearing trees, cells
//! force-filled), and the replay-equivalence test over an EPathMap-heavy
//! program (rholang).

use serde::Serialize;

use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::tagged_continuation::TaggedCont;
use crate::rhoapi::{
    BindPattern, Bundle, Connective, EList, EMap, EMatches, EMethod, EPathMap, ETuple, ESet,
    EZipper, Expr, If, KeyValuePair, ListParWithRandom, Match, MatchCase, New, Par, ParWithRandom,
    Receive, ReceiveBind, Send, TaggedContinuation,
};

// ─────────────────────────────────────────────────────────────────────────────
// Public entry points (the three event-hash legs)
// ─────────────────────────────────────────────────────────────────────────────

/// bincode-of-`ListParWithRandom` for `hash_produce`'s datum leg —
/// intern-aware when the datum contains a filled-cell EPathMap, the direct
/// path otherwise.
pub fn event_hash_bytes_list_par_with_random(datum: &ListParWithRandom) -> Vec<u8> {
    if !datum.pars.iter().any(contains_par) {
        return direct(datum);
    }
    let mut out = Vec::new();
    emit_vec(&datum.pars, contains_par, emit_par, &mut out);
    emit_bytes(&datum.random_state, &mut out);
    out
}

/// bincode-of-`BindPattern` for `hash_consume`'s per-pattern leg.
pub fn event_hash_bytes_bind_pattern(pattern: &BindPattern) -> Vec<u8> {
    if !pattern.patterns.iter().any(contains_par) {
        return direct(pattern);
    }
    let mut out = Vec::new();
    emit_vec(&pattern.patterns, contains_par, emit_par, &mut out);
    emit_black_box_option(&pattern.remainder, &mut out); // Var: map-free
    emit_i32(pattern.free_count, &mut out);
    out
}

/// bincode-of-`TaggedContinuation` for `hash_consume`'s continuation leg.
pub fn event_hash_bytes_tagged_continuation(continuation: &TaggedContinuation) -> Vec<u8> {
    if !contains_tagged_continuation(continuation) {
        return direct(continuation);
    }
    let mut out = Vec::new();
    // Declaration order: `guard` (Option<Par>), then `tagged_cont`.
    emit_option(&continuation.guard, contains_par, emit_par, &mut out);
    match &continuation.tagged_cont {
        None => out.push(0),
        Some(tc) => {
            out.push(1);
            match tc {
                TaggedCont::ParBody(pwr) => {
                    emit_u32(0, &mut out); // variant 0 in declaration order
                    emit_par_with_random(pwr, &mut out);
                }
                TaggedCont::ScalaBodyRef(body_ref) => {
                    emit_u32(1, &mut out);
                    out.extend_from_slice(&body_ref.to_le_bytes());
                }
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Primitive emitters (the layout rule set)
// ─────────────────────────────────────────────────────────────────────────────

/// The direct path: black-box bincode of a whole (interned-map-free) value.
fn direct<T: Serialize>(value: &T) -> Vec<u8> {
    bincode::serialize(value).expect("spliced_event_bytes: bincode serialization must not fail")
}

/// Append the black-box bincode of a subtree (compositionality: a field's
/// encoding inside a struct/seq IS its standalone encoding).
fn emit_black_box<T: Serialize>(value: &T, out: &mut Vec<u8>) {
    out.extend_from_slice(&direct(value));
}

fn emit_u64(value: u64, out: &mut Vec<u8>) { out.extend_from_slice(&value.to_le_bytes()); }

fn emit_u32(value: u32, out: &mut Vec<u8>) { out.extend_from_slice(&value.to_le_bytes()); }

fn emit_i32(value: i32, out: &mut Vec<u8>) { out.extend_from_slice(&value.to_le_bytes()); }

fn emit_i64(value: i64, out: &mut Vec<u8>) { out.extend_from_slice(&value.to_le_bytes()); }

fn emit_bool(value: bool, out: &mut Vec<u8>) { out.push(value as u8); }

/// Length-prefixed raw bytes (`Vec<u8>` fields and `String` payloads share
/// this shape; a `String` is its UTF-8 bytes).
fn emit_bytes(value: &[u8], out: &mut Vec<u8>) {
    emit_u64(value.len() as u64, out);
    out.extend_from_slice(value);
}

fn emit_string(value: &str, out: &mut Vec<u8>) { emit_bytes(value.as_bytes(), out); }

/// The serialize-only `locally_free` normalization: EMPTY bytes at every
/// hand-walked level (u64-LE 0), regardless of content.
fn emit_locally_free_as_empty(out: &mut Vec<u8>) { emit_u64(0, out); }

/// A `Vec<T>` whose elements MAY contain interned maps: u64-LE length, then
/// per element either the hand walk (contains) or the black box.
fn emit_vec<T: Serialize>(
    items: &[T],
    contains: fn(&T) -> bool,
    emit: fn(&T, &mut Vec<u8>),
    out: &mut Vec<u8>,
) {
    emit_u64(items.len() as u64, out);
    for item in items {
        if contains(item) {
            emit(item, out);
        } else {
            emit_black_box(item, out);
        }
    }
}

/// An `Option<T>` whose payload MAY contain interned maps: one tag byte,
/// then the payload (hand walk or black box).
fn emit_option<T: Serialize>(
    value: &Option<T>,
    contains: fn(&T) -> bool,
    emit: fn(&T, &mut Vec<u8>),
    out: &mut Vec<u8>,
) {
    match value {
        None => out.push(0),
        Some(inner) => {
            out.push(1);
            if contains(inner) {
                emit(inner, out);
            } else {
                emit_black_box(inner, out);
            }
        }
    }
}

/// An `Option<T>` that can NEVER contain a map (e.g. `Var`): tag byte +
/// black box.
fn emit_black_box_option<T: Serialize>(value: &Option<T>, out: &mut Vec<u8>) {
    match value {
        None => out.push(0),
        Some(inner) => {
            out.push(1);
            emit_black_box(inner, out);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The contains-scan (short-circuits at filled cells; descends unfilled maps)
// ─────────────────────────────────────────────────────────────────────────────

fn contains_epathmap(map: &EPathMap) -> bool {
    map.interned_handle().is_some() || map.ps.iter().any(contains_par)
}

fn contains_par(par: &Par) -> bool {
    par.sends.iter().any(contains_send)
        || par.receives.iter().any(contains_receive)
        || par.news.iter().any(contains_new)
        || par.exprs.iter().any(contains_expr)
        || par.matches.iter().any(contains_match)
        || par.bundles.iter().any(contains_bundle)
        || par.connectives.iter().any(contains_connective)
        || par.conditionals.iter().any(contains_if)
    // unforgeables carry no Par (GPrivate/GDeployId/GDeployerId/GSysAuthToken
    // are byte/unit payloads) — never map-bearing.
}

fn contains_opt_par(par: &Option<Par>) -> bool { par.as_ref().is_some_and(contains_par) }

fn contains_send(send: &Send) -> bool {
    contains_opt_par(&send.chan) || send.data.iter().any(contains_par)
}

fn contains_receive_bind(bind: &ReceiveBind) -> bool {
    bind.patterns.iter().any(contains_par) || contains_opt_par(&bind.source)
}

fn contains_receive(receive: &Receive) -> bool {
    receive.binds.iter().any(contains_receive_bind)
        || contains_opt_par(&receive.body)
        || contains_opt_par(&receive.condition)
}

fn contains_new(new: &New) -> bool {
    contains_opt_par(&new.p) || new.injections.values().any(contains_par)
}

fn contains_match_case(case: &MatchCase) -> bool {
    contains_opt_par(&case.pattern) || contains_opt_par(&case.source) || contains_opt_par(&case.guard)
}

fn contains_match(match_proc: &Match) -> bool {
    contains_opt_par(&match_proc.target) || match_proc.cases.iter().any(contains_match_case)
}

fn contains_if(conditional: &If) -> bool {
    contains_opt_par(&conditional.condition)
        || contains_opt_par(&conditional.if_true)
        || contains_opt_par(&conditional.if_false)
}

fn contains_bundle(bundle: &Bundle) -> bool { contains_opt_par(&bundle.body) }

fn contains_connective(connective: &Connective) -> bool {
    match &connective.connective_instance {
        Some(ConnectiveInstance::ConnAndBody(body))
        | Some(ConnectiveInstance::ConnOrBody(body)) => body.ps.iter().any(contains_par),
        Some(ConnectiveInstance::ConnNotBody(par)) => contains_par(par),
        _ => false,
    }
}

fn contains_key_value_pair(kv: &KeyValuePair) -> bool {
    contains_opt_par(&kv.key) || contains_opt_par(&kv.value)
}

fn contains_expr(expr: &Expr) -> bool {
    match &expr.expr_instance {
        Some(ExprInstance::EPathmapBody(map)) => contains_epathmap(map),
        Some(ExprInstance::EZipperBody(zipper)) => {
            zipper.pathmap.as_ref().is_some_and(contains_epathmap)
        }
        Some(ExprInstance::EListBody(list)) => list.ps.iter().any(contains_par),
        Some(ExprInstance::ETupleBody(tuple)) => tuple.ps.iter().any(contains_par),
        Some(ExprInstance::ESetBody(set)) => set.ps.iter().any(contains_par),
        Some(ExprInstance::EMapBody(map)) => map.kvs.iter().any(contains_key_value_pair),
        Some(ExprInstance::EMethodBody(method)) => {
            contains_opt_par(&method.target) || method.arguments.iter().any(contains_par)
        }
        Some(ExprInstance::EMatchesBody(matches)) => {
            contains_opt_par(&matches.target) || contains_opt_par(&matches.pattern)
        }
        Some(ExprInstance::ENotBody(e)) => contains_opt_par(&e.p),
        Some(ExprInstance::ENegBody(e)) => contains_opt_par(&e.p),
        Some(ExprInstance::EMultBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EDivBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EModBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EPlusBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EMinusBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::ELtBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::ELteBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EGtBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EGteBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EEqBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::ENeqBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EAndBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EOrBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EPercentPercentBody(e)) => {
            contains_opt_par(&e.p1) || contains_opt_par(&e.p2)
        }
        Some(ExprInstance::EPlusPlusBody(e)) => contains_opt_par(&e.p1) || contains_opt_par(&e.p2),
        Some(ExprInstance::EMinusMinusBody(e)) => {
            contains_opt_par(&e.p1) || contains_opt_par(&e.p2)
        }
        // Ground scalars and EVar: map-free.
        _ => false,
    }
}

fn contains_par_with_random(pwr: &ParWithRandom) -> bool { contains_opt_par(&pwr.body) }

fn contains_tagged_continuation(continuation: &TaggedContinuation) -> bool {
    contains_opt_par(&continuation.guard)
        || matches!(&continuation.tagged_cont,
                    Some(TaggedCont::ParBody(pwr)) if contains_par_with_random(pwr))
}

// ─────────────────────────────────────────────────────────────────────────────
// Hand emitters for the container spine (declaration-order field walks)
// ─────────────────────────────────────────────────────────────────────────────

/// The SPLICE point. Filled cell ⇒ append the cached serde bytes (computing
/// them ONCE per interned entry via the P1 `OnceLock` — the lazy population
/// PM-1 assigns to P4); unfilled ⇒ hand-walk the fields (a nested map may
/// still splice).
fn emit_epathmap(map: &EPathMap, out: &mut Vec<u8>) {
    if let Some(interned) = map.interned_handle() {
        let bytes = interned.serde_bytes.get_or_init(|| direct(map));
        out.extend_from_slice(bytes);
        return;
    }
    // Declaration order: ps, locally_free (EMPTY), connective_used, remainder.
    emit_vec(&map.ps, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(map.connective_used, out);
    emit_black_box_option(&map.remainder, out);
}

fn emit_par(par: &Par, out: &mut Vec<u8>) {
    // Declaration order: sends, receives, news, exprs, matches, unforgeables,
    // bundles, connectives, conditionals, locally_free (EMPTY),
    // connective_used. (NOTE: declaration order differs from proto tag
    // order — serde follows declaration.)
    emit_vec(&par.sends, contains_send, emit_send, out);
    emit_vec(&par.receives, contains_receive, emit_receive, out);
    emit_vec(&par.news, contains_new, emit_new, out);
    emit_vec(&par.exprs, contains_expr, emit_expr, out);
    emit_vec(&par.matches, contains_match, emit_match, out);
    emit_black_box(&par.unforgeables, out); // never map-bearing
    emit_vec(&par.bundles, contains_bundle, emit_bundle, out);
    emit_vec(&par.connectives, contains_connective, emit_connective, out);
    emit_vec(&par.conditionals, contains_if, emit_if, out);
    emit_locally_free_as_empty(out);
    emit_bool(par.connective_used, out);
}

fn emit_send(send: &Send, out: &mut Vec<u8>) {
    // chan, data, persistent, locally_free (EMPTY), connective_used.
    emit_option(&send.chan, contains_par, emit_par, out);
    emit_vec(&send.data, contains_par, emit_par, out);
    emit_bool(send.persistent, out);
    emit_locally_free_as_empty(out);
    emit_bool(send.connective_used, out);
}

fn emit_receive_bind(bind: &ReceiveBind, out: &mut Vec<u8>) {
    // patterns, source, remainder, free_count.
    emit_vec(&bind.patterns, contains_par, emit_par, out);
    emit_option(&bind.source, contains_par, emit_par, out);
    emit_black_box_option(&bind.remainder, out);
    emit_i32(bind.free_count, out);
}

fn emit_receive(receive: &Receive, out: &mut Vec<u8>) {
    // binds, body, persistent, peek, bind_count, locally_free (EMPTY),
    // connective_used, condition.
    emit_vec(&receive.binds, contains_receive_bind, emit_receive_bind, out);
    emit_option(&receive.body, contains_par, emit_par, out);
    emit_bool(receive.persistent, out);
    emit_bool(receive.peek, out);
    emit_i32(receive.bind_count, out);
    emit_locally_free_as_empty(out);
    emit_bool(receive.connective_used, out);
    emit_option(&receive.condition, contains_par, emit_par, out);
}

fn emit_new(new: &New, out: &mut Vec<u8>) {
    // bind_count, p, uri, injections, locally_free (EMPTY).
    emit_i32(new.bind_count, out);
    emit_option(&new.p, contains_par, emit_par, out);
    emit_black_box(&new.uri, out);
    emit_u64(new.injections.len() as u64, out);
    for (key, value) in &new.injections {
        emit_string(key, out);
        if contains_par(value) {
            emit_par(value, out);
        } else {
            emit_black_box(value, out);
        }
    }
    emit_locally_free_as_empty(out);
}

fn emit_match_case(case: &MatchCase, out: &mut Vec<u8>) {
    // pattern, source, free_count, guard.
    emit_option(&case.pattern, contains_par, emit_par, out);
    emit_option(&case.source, contains_par, emit_par, out);
    emit_i32(case.free_count, out);
    emit_option(&case.guard, contains_par, emit_par, out);
}

fn emit_match(match_proc: &Match, out: &mut Vec<u8>) {
    // target, cases, locally_free (EMPTY), connective_used.
    emit_option(&match_proc.target, contains_par, emit_par, out);
    emit_vec(&match_proc.cases, contains_match_case, emit_match_case, out);
    emit_locally_free_as_empty(out);
    emit_bool(match_proc.connective_used, out);
}

fn emit_if(conditional: &If, out: &mut Vec<u8>) {
    // condition, if_true, if_false, locally_free (EMPTY), connective_used.
    emit_option(&conditional.condition, contains_par, emit_par, out);
    emit_option(&conditional.if_true, contains_par, emit_par, out);
    emit_option(&conditional.if_false, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(conditional.connective_used, out);
}

fn emit_bundle(bundle: &Bundle, out: &mut Vec<u8>) {
    // body, write_flag, read_flag.
    emit_option(&bundle.body, contains_par, emit_par, out);
    emit_bool(bundle.write_flag, out);
    emit_bool(bundle.read_flag, out);
}

fn emit_connective(connective: &Connective, out: &mut Vec<u8>) {
    // connective_instance: Option<ConnectiveInstance> (9 variants,
    // declaration order 0..=8).
    match &connective.connective_instance {
        None => out.push(0),
        Some(instance) => {
            out.push(1);
            match instance {
                ConnectiveInstance::ConnAndBody(body) => {
                    emit_u32(0, out);
                    emit_vec(&body.ps, contains_par, emit_par, out);
                }
                ConnectiveInstance::ConnOrBody(body) => {
                    emit_u32(1, out);
                    emit_vec(&body.ps, contains_par, emit_par, out);
                }
                ConnectiveInstance::ConnNotBody(par) => {
                    emit_u32(2, out);
                    if contains_par(par) {
                        emit_par(par, out);
                    } else {
                        emit_black_box(par, out);
                    }
                }
                ConnectiveInstance::VarRefBody(var_ref) => {
                    emit_u32(3, out);
                    emit_black_box(var_ref, out);
                }
                ConnectiveInstance::ConnBool(b) => {
                    emit_u32(4, out);
                    emit_bool(*b, out);
                }
                ConnectiveInstance::ConnInt(b) => {
                    emit_u32(5, out);
                    emit_bool(*b, out);
                }
                ConnectiveInstance::ConnString(b) => {
                    emit_u32(6, out);
                    emit_bool(*b, out);
                }
                ConnectiveInstance::ConnUri(b) => {
                    emit_u32(7, out);
                    emit_bool(*b, out);
                }
                ConnectiveInstance::ConnByteArray(b) => {
                    emit_u32(8, out);
                    emit_bool(*b, out);
                }
            }
        }
    }
}

fn emit_key_value_pair(kv: &KeyValuePair, out: &mut Vec<u8>) {
    emit_option(&kv.key, contains_par, emit_par, out);
    emit_option(&kv.value, contains_par, emit_par, out);
}

fn emit_elist(list: &EList, out: &mut Vec<u8>) {
    // ps, locally_free (EMPTY), connective_used, remainder.
    emit_vec(&list.ps, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(list.connective_used, out);
    emit_black_box_option(&list.remainder, out);
}

fn emit_etuple(tuple: &ETuple, out: &mut Vec<u8>) {
    // ps, locally_free (EMPTY), connective_used.
    emit_vec(&tuple.ps, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(tuple.connective_used, out);
}

fn emit_eset(set: &ESet, out: &mut Vec<u8>) {
    // ps, locally_free (EMPTY), connective_used, remainder.
    emit_vec(&set.ps, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(set.connective_used, out);
    emit_black_box_option(&set.remainder, out);
}

fn emit_emap(map: &EMap, out: &mut Vec<u8>) {
    // kvs, locally_free (EMPTY), connective_used, remainder.
    emit_vec(&map.kvs, contains_key_value_pair, emit_key_value_pair, out);
    emit_locally_free_as_empty(out);
    emit_bool(map.connective_used, out);
    emit_black_box_option(&map.remainder, out);
}

fn emit_emethod(method: &EMethod, out: &mut Vec<u8>) {
    // method_name, target, arguments, locally_free (EMPTY), connective_used.
    emit_string(&method.method_name, out);
    emit_option(&method.target, contains_par, emit_par, out);
    emit_vec(&method.arguments, contains_par, emit_par, out);
    emit_locally_free_as_empty(out);
    emit_bool(method.connective_used, out);
}

fn emit_ematches(matches: &EMatches, out: &mut Vec<u8>) {
    emit_option(&matches.target, contains_par, emit_par, out);
    emit_option(&matches.pattern, contains_par, emit_par, out);
}

fn emit_ezipper(zipper: &EZipper, out: &mut Vec<u8>) {
    // pathmap, current_path, is_write_zipper, locally_free (EMPTY),
    // connective_used.
    emit_option(&zipper.pathmap, contains_epathmap, emit_epathmap, out);
    emit_black_box(&zipper.current_path, out);
    emit_bool(zipper.is_write_zipper, out);
    emit_locally_free_as_empty(out);
    emit_bool(zipper.connective_used, out);
}

/// Two-Par operator payload (`p1`, `p2`) — shared shape of the arithmetic /
/// comparison / logic / concatenation expr messages.
fn emit_binary_op(p1: &Option<Par>, p2: &Option<Par>, out: &mut Vec<u8>) {
    emit_option(p1, contains_par, emit_par, out);
    emit_option(p2, contains_par, emit_par, out);
}

fn emit_par_with_random(pwr: &ParWithRandom, out: &mut Vec<u8>) {
    // body, random_state.
    emit_option(&pwr.body, contains_par, emit_par, out);
    emit_bytes(&pwr.random_state, out);
}

fn emit_expr(expr: &Expr, out: &mut Vec<u8>) {
    // Expr { expr_instance: Option<ExprInstance> } — the 36 variants in
    // DECLARATION order (0-based indexes; the proto tags are NOT the serde
    // indexes).
    match &expr.expr_instance {
        None => out.push(0),
        Some(instance) => {
            out.push(1);
            match instance {
                ExprInstance::GBool(b) => {
                    emit_u32(0, out);
                    emit_bool(*b, out);
                }
                ExprInstance::GInt(i) => {
                    emit_u32(1, out);
                    emit_i64(*i, out);
                }
                ExprInstance::GString(s) => {
                    emit_u32(2, out);
                    emit_string(s, out);
                }
                ExprInstance::GUri(s) => {
                    emit_u32(3, out);
                    emit_string(s, out);
                }
                ExprInstance::GByteArray(bytes) => {
                    emit_u32(4, out);
                    emit_bytes(bytes, out);
                }
                ExprInstance::ENotBody(e) => {
                    emit_u32(5, out);
                    emit_option(&e.p, contains_par, emit_par, out);
                }
                ExprInstance::ENegBody(e) => {
                    emit_u32(6, out);
                    emit_option(&e.p, contains_par, emit_par, out);
                }
                ExprInstance::EMultBody(e) => {
                    emit_u32(7, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EDivBody(e) => {
                    emit_u32(8, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EPlusBody(e) => {
                    emit_u32(9, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EMinusBody(e) => {
                    emit_u32(10, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::ELtBody(e) => {
                    emit_u32(11, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::ELteBody(e) => {
                    emit_u32(12, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EGtBody(e) => {
                    emit_u32(13, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EGteBody(e) => {
                    emit_u32(14, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EEqBody(e) => {
                    emit_u32(15, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::ENeqBody(e) => {
                    emit_u32(16, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EAndBody(e) => {
                    emit_u32(17, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EOrBody(e) => {
                    emit_u32(18, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EVarBody(e) => {
                    emit_u32(19, out);
                    emit_black_box(e, out); // Var payload: map-free
                }
                ExprInstance::EListBody(list) => {
                    emit_u32(20, out);
                    emit_elist(list, out);
                }
                ExprInstance::ETupleBody(tuple) => {
                    emit_u32(21, out);
                    emit_etuple(tuple, out);
                }
                ExprInstance::ESetBody(set) => {
                    emit_u32(22, out);
                    emit_eset(set, out);
                }
                ExprInstance::EMapBody(map) => {
                    emit_u32(23, out);
                    emit_emap(map, out);
                }
                ExprInstance::EMethodBody(method) => {
                    emit_u32(24, out);
                    emit_emethod(method, out);
                }
                ExprInstance::EPathmapBody(map) => {
                    emit_u32(25, out);
                    emit_epathmap(map, out); // THE SPLICE
                }
                ExprInstance::EZipperBody(zipper) => {
                    emit_u32(26, out);
                    emit_ezipper(zipper, out);
                }
                ExprInstance::EMatchesBody(matches) => {
                    emit_u32(27, out);
                    emit_ematches(matches, out);
                }
                ExprInstance::EPercentPercentBody(e) => {
                    emit_u32(28, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EPlusPlusBody(e) => {
                    emit_u32(29, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EMinusMinusBody(e) => {
                    emit_u32(30, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::EModBody(e) => {
                    emit_u32(31, out);
                    emit_binary_op(&e.p1, &e.p2, out);
                }
                ExprInstance::GDouble(bits) => {
                    emit_u32(32, out);
                    out.extend_from_slice(&bits.to_le_bytes());
                }
                ExprInstance::GBigInt(bytes) => {
                    emit_u32(33, out);
                    emit_bytes(bytes, out);
                }
                ExprInstance::GBigRat(rational) => {
                    emit_u32(34, out);
                    emit_bytes(&rational.numerator, out);
                    emit_bytes(&rational.denominator, out);
                }
                ExprInstance::GFixedPoint(fixed_point) => {
                    emit_u32(35, out);
                    emit_bytes(&fixed_point.unscaled, out);
                    emit_u32(fixed_point.scale, out);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The StableHashSerialize overrides (the ONLY event-hash entry points)
// ─────────────────────────────────────────────────────────────────────────────

use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;

impl StableHashSerialize for ListParWithRandom {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_list_par_with_random(self) }
}

impl StableHashSerialize for BindPattern {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_bind_pattern(self) }
}

impl StableHashSerialize for TaggedContinuation {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_tagged_continuation(self) }
}

// Default-body impls (direct bincode) for rhoapi types used as space type
// parameters in tests/tools without an EPathMap-bearing hot path.
impl StableHashSerialize for Par {}
impl StableHashSerialize for ParWithRandom {}
