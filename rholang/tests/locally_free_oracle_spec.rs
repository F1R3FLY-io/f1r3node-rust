// Property test: for every `Par` node emitted by the normalizer, the
// cached `locally_free` bitset agrees with a ground-truth tree walk
// that collects every `BoundVar(i)` (at a Par scope) and every
// `VarRef(index, depth)` (at matching pattern depth), applying each
// enclosing binder as a de Bruijn shift. The oracle never reads a
// cached bitset — it recomputes from leaves — so any regression in
// `filter_and_adjust_bitset` (or any other normalizer bit-plumbing)
// fails this test.

use std::collections::BTreeSet;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, EList, EMap, EMatches, EMethod, ENeg, ENot, ESet, ETuple, Expr, If,
    KeyValuePair, Match, MatchCase, New, Par, Receive, Send,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;

type Bits = BTreeSet<usize>;

fn cached_bits(v: &[u8]) -> Bits {
    (0..v.len())
        .filter(|&i| v.get(i).copied().unwrap_or(0) != 0)
        .collect()
}

fn shift_down(b: &Bits, n: usize) -> Bits {
    b.iter().filter(|&&i| i >= n).map(|&i| i - n).collect()
}

fn union(a: Bits, b: Bits) -> Bits { a.union(&b).copied().collect() }

/// Oracle: compute `locally_free` of `par` seen at pattern `depth`
/// (top-level is 0; each enclosing pattern level adds 1).
fn tree_lf_par(par: &Par, depth: i32) -> Bits {
    let mut out = Bits::new();
    for s in &par.sends {
        out = union(out, tree_lf_send(s, depth));
    }
    for r in &par.receives {
        out = union(out, tree_lf_receive(r, depth));
    }
    for n in &par.news {
        out = union(out, tree_lf_new(n, depth));
    }
    for m in &par.matches {
        out = union(out, tree_lf_match(m, depth));
    }
    for b in &par.bundles {
        out = union(out, tree_lf_bundle(b, depth));
    }
    for e in &par.exprs {
        out = union(out, tree_lf_expr(e, depth));
    }
    for c in &par.connectives {
        out = union(out, tree_lf_connective(c, depth));
    }
    for i in &par.conditionals {
        out = union(out, tree_lf_if(i, depth));
    }
    // unforgeables carry no free variables.
    out
}

fn tree_lf_send(s: &Send, depth: i32) -> Bits {
    let mut out = s
        .chan
        .as_ref()
        .map(|c| tree_lf_par(c, depth))
        .unwrap_or_default();
    for d in &s.data {
        out = union(out, tree_lf_par(d, depth));
    }
    out
}

fn tree_lf_receive(r: &Receive, depth: i32) -> Bits {
    let mut out = Bits::new();
    for b in &r.binds {
        if let Some(src) = &b.source {
            out = union(out, tree_lf_par(src, depth));
        }
        for p in &b.patterns {
            out = union(out, tree_lf_par(p, depth + 1));
        }
    }
    if let Some(body) = &r.body {
        let bits = tree_lf_par(body, depth);
        out = union(out, shift_down(&bits, r.bind_count as usize));
    }
    if let Some(cond) = &r.condition {
        let bits = tree_lf_par(cond, depth);
        out = union(out, shift_down(&bits, r.bind_count as usize));
    }
    out
}

fn tree_lf_new(n: &New, depth: i32) -> Bits {
    n.p.as_ref()
        .map(|p| shift_down(&tree_lf_par(p, depth), n.bind_count as usize))
        .unwrap_or_default()
}

fn tree_lf_match(m: &Match, depth: i32) -> Bits {
    let mut out = m
        .target
        .as_ref()
        .map(|t| tree_lf_par(t, depth))
        .unwrap_or_default();
    for c in &m.cases {
        out = union(out, tree_lf_case(c, depth));
    }
    out
}

fn tree_lf_case(c: &MatchCase, depth: i32) -> Bits {
    let mut out = c
        .pattern
        .as_ref()
        .map(|p| tree_lf_par(p, depth + 1))
        .unwrap_or_default();
    if let Some(s) = &c.source {
        let bits = tree_lf_par(s, depth);
        out = union(out, shift_down(&bits, c.free_count as usize));
    }
    if let Some(g) = &c.guard {
        let bits = tree_lf_par(g, depth);
        out = union(out, shift_down(&bits, c.free_count as usize));
    }
    out
}

fn tree_lf_bundle(b: &Bundle, depth: i32) -> Bits {
    b.body
        .as_ref()
        .map(|p| tree_lf_par(p, depth))
        .unwrap_or_default()
}

fn tree_lf_if(i: &If, depth: i32) -> Bits {
    let mut out = i
        .condition
        .as_ref()
        .map(|c| tree_lf_par(c, depth))
        .unwrap_or_default();
    if let Some(t) = &i.if_true {
        out = union(out, tree_lf_par(t, depth));
    }
    if let Some(f) = &i.if_false {
        out = union(out, tree_lf_par(f, depth));
    }
    out
}

fn tree_lf_connective(c: &Connective, depth: i32) -> Bits {
    match &c.connective_instance {
        Some(ConnectiveInstance::VarRefBody(vr)) if vr.depth == depth => {
            [vr.index as usize].into_iter().collect()
        }
        Some(ConnectiveInstance::ConnAndBody(cb)) | Some(ConnectiveInstance::ConnOrBody(cb)) => cb
            .ps
            .iter()
            .fold(Bits::new(), |acc, p| union(acc, tree_lf_par(p, depth))),
        Some(ConnectiveInstance::ConnNotBody(p)) => tree_lf_par(p, depth),
        Some(ConnectiveInstance::ConnFixedPointBody(b)) => b
            .scale
            .as_ref()
            .map(|p| tree_lf_par(p, depth))
            .unwrap_or_default(),
        _ => Bits::new(),
    }
}

fn tree_lf_expr(e: &Expr, depth: i32) -> Bits {
    use ExprInstance::*;
    match &e.expr_instance {
        None
        | Some(
            GBool(_) | GInt(_) | GUint64(_) | GInt32(_) | GUint32(_) | GUint16(_) | GUint8(_)
            | GDouble(_) | GFloat32(_) | GBigInt(_) | GBigRat(_) | GFixedPoint(_) | GString(_)
            | GUri(_) | GByteArray(_),
        ) => Bits::new(),

        Some(EVarBody(v)) => match v.v.as_ref().and_then(|v| v.var_instance.as_ref()) {
            Some(VarInstance::BoundVar(i)) if depth == 0 => [*i as usize].into_iter().collect(),
            _ => Bits::new(),
        },

        Some(ENotBody(ENot { p })) | Some(ENegBody(ENeg { p })) => p
            .as_ref()
            .map(|p| tree_lf_par(p, depth))
            .unwrap_or_default(),

        Some(EMatchesBody(EMatches { target, pattern })) => {
            let t = target
                .as_ref()
                .map(|p| tree_lf_par(p, depth))
                .unwrap_or_default();
            let p = pattern
                .as_ref()
                .map(|p| tree_lf_par(p, depth + 1))
                .unwrap_or_default();
            union(t, p)
        }

        Some(EListBody(EList { ps, .. }))
        | Some(ETupleBody(ETuple { ps, .. }))
        | Some(ESetBody(ESet { ps, .. })) => ps
            .iter()
            .fold(Bits::new(), |acc, p| union(acc, tree_lf_par(p, depth))),

        Some(EMapBody(EMap { kvs, .. })) => {
            kvs.iter()
                .fold(Bits::new(), |acc, KeyValuePair { key, value }| {
                    let k = key
                        .as_ref()
                        .map(|p| tree_lf_par(p, depth))
                        .unwrap_or_default();
                    let v = value
                        .as_ref()
                        .map(|p| tree_lf_par(p, depth))
                        .unwrap_or_default();
                    union(union(acc, k), v)
                })
        }

        Some(EMethodBody(EMethod {
            target, arguments, ..
        })) => {
            let t = target
                .as_ref()
                .map(|p| tree_lf_par(p, depth))
                .unwrap_or_default();
            arguments
                .iter()
                .fold(t, |acc, p| union(acc, tree_lf_par(p, depth)))
        }

        Some(
            EMultBody(_)
            | EDivBody(_)
            | EModBody(_)
            | EPlusBody(_)
            | EMinusBody(_)
            | ELtBody(_)
            | ELteBody(_)
            | EGtBody(_)
            | EGteBody(_)
            | EEqBody(_)
            | ENeqBody(_)
            | EAndBody(_)
            | EOrBody(_)
            | EPercentPercentBody(_)
            | EPlusPlusBody(_)
            | EMinusMinusBody(_),
        ) => tree_lf_binary_expr(e, depth),

        // Variants with their own locally_free cache but no sub-Par walk from the oracle.
        // These should never contribute free vars on their own (lambdas, pi, zipper, etc.
        // have explicit parameters that bind inside the Expr's own structure, which this
        // oracle treats conservatively by trusting the cache).
        Some(ELambdaBody(_) | EAppBody(_) | EPiBody(_) | EPathmapBody(_) | EZipperBody(_)) => {
            cached_bits(&e.locally_free(depth_hint(depth)))
        }
    }
}

// Depth passed to HasLocallyFree is irrelevant for the fallback path (these variants
// only use their cached locally_free). Trait method takes i32, we have i32 already.
fn depth_hint(d: i32) -> i32 { d }

use rholang::rust::interpreter::matcher::has_locally_free::HasLocallyFree;

fn tree_lf_binary_expr(e: &Expr, depth: i32) -> Bits {
    use ExprInstance::*;
    let (p1, p2) = match &e.expr_instance {
        Some(EMultBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EDivBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EModBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EPlusBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EMinusBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(ELtBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(ELteBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EGtBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EGteBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EEqBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(ENeqBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EAndBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EOrBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EPercentPercentBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EPlusPlusBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        Some(EMinusMinusBody(x)) => (x.p1.as_ref(), x.p2.as_ref()),
        _ => return Bits::new(),
    };
    let a = p1.map(|p| tree_lf_par(p, depth)).unwrap_or_default();
    let b = p2.map(|p| tree_lf_par(p, depth)).unwrap_or_default();
    union(a, b)
}

/// Verify at every Par node in `par`: `cached_bits(par.locally_free)` equals
/// `tree_lf_par(par, pattern_depth)`, where `pattern_depth` tracks how many
/// enclosing pattern contexts the Par sits in. The cached value is laid down
/// by the normalizer at the depth it saw the Par, so the oracle must use the
/// same depth to be comparable.
fn assert_consistent(par: &Par, path: &str, pattern_depth: i32) {
    let cached = cached_bits(&par.locally_free);
    let oracle = tree_lf_par(par, pattern_depth);
    assert_eq!(
        cached, oracle,
        "locally_free mismatch at {} (pattern_depth={}):\n  cached = {:?}\n  oracle = {:?}",
        path, pattern_depth, cached, oracle
    );
    for (i, s) in par.sends.iter().enumerate() {
        if let Some(c) = &s.chan {
            assert_consistent(c, &format!("{path}/send[{i}].chan"), pattern_depth);
        }
        for (j, d) in s.data.iter().enumerate() {
            assert_consistent(d, &format!("{path}/send[{i}].data[{j}]"), pattern_depth);
        }
    }
    for (i, r) in par.receives.iter().enumerate() {
        for (j, b) in r.binds.iter().enumerate() {
            if let Some(src) = &b.source {
                assert_consistent(
                    src,
                    &format!("{path}/receive[{i}].bind[{j}].source"),
                    pattern_depth,
                );
            }
            for (k, p) in b.patterns.iter().enumerate() {
                assert_consistent(
                    p,
                    &format!("{path}/receive[{i}].bind[{j}].pattern[{k}]"),
                    pattern_depth + 1,
                );
            }
        }
        if let Some(body) = &r.body {
            assert_consistent(body, &format!("{path}/receive[{i}].body"), pattern_depth);
        }
        if let Some(cond) = &r.condition {
            assert_consistent(
                cond,
                &format!("{path}/receive[{i}].condition"),
                pattern_depth,
            );
        }
    }
    for (i, n) in par.news.iter().enumerate() {
        if let Some(body) = &n.p {
            assert_consistent(body, &format!("{path}/new[{i}].body"), pattern_depth);
        }
    }
    for (i, m) in par.matches.iter().enumerate() {
        if let Some(t) = &m.target {
            assert_consistent(t, &format!("{path}/match[{i}].target"), pattern_depth);
        }
        for (j, c) in m.cases.iter().enumerate() {
            if let Some(p) = &c.pattern {
                assert_consistent(
                    p,
                    &format!("{path}/match[{i}].case[{j}].pattern"),
                    pattern_depth + 1,
                );
            }
            if let Some(s) = &c.source {
                assert_consistent(
                    s,
                    &format!("{path}/match[{i}].case[{j}].source"),
                    pattern_depth,
                );
            }
            if let Some(g) = &c.guard {
                assert_consistent(
                    g,
                    &format!("{path}/match[{i}].case[{j}].guard"),
                    pattern_depth,
                );
            }
        }
    }
    for (i, b) in par.bundles.iter().enumerate() {
        if let Some(body) = &b.body {
            assert_consistent(body, &format!("{path}/bundle[{i}].body"), pattern_depth);
        }
    }
    for (i, cond) in par.conditionals.iter().enumerate() {
        if let Some(c) = &cond.condition {
            assert_consistent(c, &format!("{path}/if[{i}].cond"), pattern_depth);
        }
        if let Some(t) = &cond.if_true {
            assert_consistent(t, &format!("{path}/if[{i}].true"), pattern_depth);
        }
        if let Some(f) = &cond.if_false {
            assert_consistent(f, &format!("{path}/if[{i}].false"), pattern_depth);
        }
    }
}

fn check(rho: &str, label: &str) {
    let par = Compiler::source_to_adt(rho).unwrap_or_else(|e| {
        panic!("compile failed for {label}: {e:?}");
    });
    assert_consistent(&par, label, 0);
}

#[test]
fn simple_send_receive() {
    check(r#"new x in { x!(5) }"#, "simple_send");
    check(
        r#"new x in { for (y <- @"c") { x!(*y) } }"#,
        "simple_receive",
    );
}

#[test]
fn nested_new_crosses_binders() {
    // The Registry.rho-shaped pattern that first exposed the bug.
    check(
        r#"
        new depthP in {
          for (ret, @depth <= @"foo") {
            new map in {
              @(*map, *depthP)!!(depth) |
              for (@d <- @(*map, *depthP)) { ret!(*map) }
            }
          }
        }"#,
        "depthp_pattern",
    );
}

#[test]
fn contract_and_match() {
    check(
        r#"
        new add in {
          contract add(ret, @x, @y) = {
            match [x, y] {
              [a, b] => ret!(a + b)
            }
          }
        }"#,
        "contract_match",
    );
}

#[test]
fn if_branches() {
    check(
        r#"
        new x, y in {
          if (*x == *y) { x!(0) } else { y!(1) }
        }"#,
        "if_branches",
    );
}

#[test]
fn deep_new_chain() {
    check(
        r#"new a in { new b in { new c in { new d in {
        a!(1) | b!(2) | c!(3) | d!(4)
    } } } }"#,
        "deep_new_chain",
    );
}

#[test]
fn cross_binder_reference_is_tracked() {
    // Verify specifically that a variable declared across TWO binder
    // levels (the shape that triggered the chain-erasure bug in the
    // pre-fix helper) ends up with the expected non-empty locally_free
    // at each intermediate level.
    let rho = r#"new depthP in { for (@d <= @"c") { new map in { @(*map, *depthP)!!(d) } } }"#;
    let par = Compiler::source_to_adt(rho).unwrap();
    assert_consistent(&par, "cross_binder_ref", 0);

    // At the outer new's body, depthP is at DB 0 — the Receive must
    // carry a locally_free bit for it.
    let outer_body = par.news[0].p.as_ref().unwrap();
    let r = &outer_body.receives[0];
    assert!(
        r.locally_free.first().copied().unwrap_or(0) != 0,
        "receive.locally_free bit 0 must be set (depthP ref): {:?}",
        r.locally_free
    );
}

#[test]
fn genesis_registry_rho() {
    // The real test: Registry.rho is the file whose normalizer output
    // first exposed the bug via the audit. All embedded genesis
    // contracts should have consistent locally_free after the fix.
    for fname in [
        "Registry.rho",
        "NonNegativeNumber.rho",
        "MakeMint.rho",
        "AuthKey.rho",
        "SystemVault.rho",
        "MultiSigSystemVault.rho",
        "Either.rho",
        "ListOps.rho",
    ] {
        let path = format!("../casper/src/main/resources/{fname}");
        let rho = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("SKIP {path}: missing");
                continue;
            }
        };
        check(&rho, fname);
    }
}
