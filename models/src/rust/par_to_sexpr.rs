// Converts Rholang Par AST to S-expression string representation
// This is used to generate deterministic path keys for PathMap

use std::fmt::Write as _;

use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::var::VarInstance;
use crate::rhoapi::{Bundle, Expr, New, Par, Receive, ReceiveBind, Send};

/// One pending emission in the stack-safe `Par` → S-expression PDA.
///
/// Structural jobs borrow the input. Punctuation jobs are static, so the work
/// stack grows with AST depth/width while the native call stack stays constant.
enum Job<'a> {
    Par(&'a Par),
    Expr(&'a Expr),
    Send(&'a Send),
    Receive(&'a Receive),
    Bind(&'a ReceiveBind),
    New(&'a New),
    Bundle(&'a Bundle),
    Static(&'static str),
    Raw(&'a str),
    Quoted(&'a str),
    Uri(&'a str),
    Bytes(&'a [u8]),
    Bool(bool),
    Int(i64),
    Binder(i32),
    Nil,
}

#[inline]
fn push_par_or_nil<'a>(work: &mut Vec<Job<'a>>, par: Option<&'a Par>) {
    work.push(par.map_or(Job::Nil, Job::Par));
}

#[inline]
fn push_par_slice<'a>(work: &mut Vec<Job<'a>>, pars: &'a [Par]) {
    for index in (0..pars.len()).rev() {
        work.push(Job::Par(&pars[index]));
        if index != 0 {
            work.push(Job::Static(" "));
        }
    }
}

pub struct ParToSExpr;

impl ParToSExpr {
    /// Convert a `Par` to the historical S-expression diagnostic format.
    ///
    /// This is a deterministic pushdown traversal: no structural job calls
    /// another structural function. Native stack use is O(1), heap work is
    /// O(depth + pending siblings), and output is written once rather than
    /// assembled from one temporary `String` per node.
    pub fn par_to_sexpr(par: &Par) -> String {
        let mut out = String::new();
        let mut work = vec![Job::Par(par)];

        while let Some(job) = work.pop() {
            match job {
                Job::Static(text) | Job::Raw(text) => out.push_str(text),
                Job::Nil => out.push_str("Nil"),
                Job::Bool(value) => write!(out, "{value}").expect("String writes are infallible"),
                Job::Int(value) => write!(out, "{value}").expect("String writes are infallible"),
                Job::Binder(index) => {
                    write!(out, "x{index}").expect("String writes are infallible")
                }
                Job::Quoted(value) => {
                    out.push('"');
                    out.push_str(value);
                    out.push('"');
                }
                Job::Uri(value) => {
                    out.push('`');
                    out.push_str(value);
                    out.push('`');
                }
                Job::Bytes(value) => {
                    out.push_str("0x");
                    out.push_str(&hex::encode(value));
                }
                Job::Par(par) => {
                    let part_count = par.sends.len()
                        + par.receives.len()
                        + par.news.len()
                        + par.exprs.len()
                        + par.bundles.len();
                    if part_count == 0 {
                        work.push(Job::Nil);
                        continue;
                    }
                    if part_count > 1 {
                        work.push(Job::Static(")"));
                    }
                    let mut parts = Vec::with_capacity(part_count);
                    parts.extend(par.sends.iter().map(Job::Send));
                    parts.extend(par.receives.iter().map(Job::Receive));
                    parts.extend(par.news.iter().map(Job::New));
                    parts.extend(par.exprs.iter().map(Job::Expr));
                    parts.extend(par.bundles.iter().map(Job::Bundle));
                    for index in (0..parts.len()).rev() {
                        work.push(parts.swap_remove(index));
                        if index != 0 {
                            work.push(Job::Static(" "));
                        }
                    }
                    if part_count > 1 {
                        work.push(Job::Static("(par "));
                    }
                }
                Job::Send(send) => {
                    work.push(Job::Static(")"));
                    push_par_slice(&mut work, &send.data);
                    work.push(Job::Static(" "));
                    push_par_or_nil(&mut work, send.chan.as_ref());
                    work.push(Job::Static("(! "));
                }
                Job::Receive(receive) => {
                    work.push(Job::Static(")"));
                    push_par_or_nil(&mut work, receive.body.as_ref());
                    work.push(Job::Static(" "));
                    work.push(Job::Static(")"));
                    for index in (0..receive.binds.len()).rev() {
                        work.push(Job::Bind(&receive.binds[index]));
                        if index != 0 {
                            work.push(Job::Static(" "));
                        }
                    }
                    work.push(Job::Static("(for ("));
                }
                Job::Bind(bind) => {
                    work.push(Job::Static(")"));
                    push_par_or_nil(&mut work, bind.source.as_ref());
                    work.push(Job::Static("(bind <- "));
                }
                Job::New(new) => {
                    work.push(Job::Static(")"));
                    push_par_or_nil(&mut work, new.p.as_ref());
                    work.push(Job::Static(" "));
                    for index in (0..new.bind_count).rev() {
                        work.push(Job::Binder(index));
                        if index != 0 {
                            work.push(Job::Static(" "));
                        }
                    }
                    work.push(Job::Static("(new "));
                }
                Job::Bundle(bundle) => {
                    work.push(Job::Static(")"));
                    push_par_or_nil(&mut work, bundle.body.as_ref());
                    work.push(Job::Static("(bundle "));
                }
                Job::Expr(expr) => match expr.expr_instance.as_ref() {
                    None => work.push(Job::Nil),
                    Some(ExprInstance::GBool(value)) => work.push(Job::Bool(*value)),
                    Some(ExprInstance::GInt(value)) => work.push(Job::Int(*value)),
                    Some(ExprInstance::GString(value)) => work.push(Job::Quoted(value)),
                    Some(ExprInstance::GUri(value)) => work.push(Job::Uri(value)),
                    Some(ExprInstance::GByteArray(value)) => work.push(Job::Bytes(value)),
                    Some(ExprInstance::EListBody(list)) => {
                        work.push(Job::Static("]"));
                        push_par_slice(&mut work, &list.ps);
                        work.push(Job::Static("["));
                    }
                    Some(ExprInstance::ETupleBody(tuple)) => {
                        work.push(Job::Static(")"));
                        push_par_slice(&mut work, &tuple.ps);
                        work.push(Job::Static("(tuple "));
                    }
                    Some(ExprInstance::ESetBody(set)) => {
                        work.push(Job::Static(")"));
                        push_par_slice(&mut work, &set.ps);
                        work.push(Job::Static("(set "));
                    }
                    Some(ExprInstance::EMapBody(map)) => {
                        work.push(Job::Static(")"));
                        for index in (0..map.kvs.len()).rev() {
                            let pair = &map.kvs[index];
                            work.push(Job::Static(")"));
                            push_par_or_nil(&mut work, pair.value.as_ref());
                            work.push(Job::Static(" : "));
                            push_par_or_nil(&mut work, pair.key.as_ref());
                            work.push(Job::Static("("));
                            if index != 0 {
                                work.push(Job::Static(" "));
                            }
                        }
                        work.push(Job::Static("(map "));
                    }
                    Some(ExprInstance::EVarBody(evar)) => {
                        match evar.v.as_ref().and_then(|var| var.var_instance.as_ref()) {
                            Some(VarInstance::BoundVar(index)) => {
                                write!(out, "_{index}").expect("String writes are infallible")
                            }
                            Some(VarInstance::FreeVar(index)) => {
                                write!(out, "${index}").expect("String writes are infallible")
                            }
                            Some(VarInstance::Wildcard(_)) => out.push('_'),
                            None => out.push_str("var"),
                        }
                    }
                    Some(ExprInstance::ENegBody(unary)) => {
                        work.push(Job::Static(")"));
                        push_par_or_nil(&mut work, unary.p.as_ref());
                        work.push(Job::Static("(- "));
                    }
                    Some(ExprInstance::ENotBody(unary)) => {
                        work.push(Job::Static(")"));
                        push_par_or_nil(&mut work, unary.p.as_ref());
                        work.push(Job::Static("(not "));
                    }
                    Some(ExprInstance::EMultBody(binary)) => {
                        push_binary(&mut work, "(* ", binary.p1.as_ref(), binary.p2.as_ref());
                    }
                    Some(ExprInstance::EDivBody(binary)) => {
                        push_binary(&mut work, "(/ ", binary.p1.as_ref(), binary.p2.as_ref());
                    }
                    Some(ExprInstance::EPlusBody(binary)) => {
                        push_binary(&mut work, "(+ ", binary.p1.as_ref(), binary.p2.as_ref());
                    }
                    Some(ExprInstance::EMinusBody(binary)) => {
                        push_binary(&mut work, "(- ", binary.p1.as_ref(), binary.p2.as_ref());
                    }
                    Some(ExprInstance::EMethodBody(method)) => {
                        work.push(Job::Static(")"));
                        push_par_slice(&mut work, &method.arguments);
                        work.push(Job::Static(" "));
                        work.push(Job::Raw(&method.method_name));
                        work.push(Job::Static("."));
                        push_par_or_nil(&mut work, method.target.as_ref());
                        work.push(Job::Static("("));
                    }
                    Some(_) => work.push(Job::Static("(expr)")),
                },
            }
        }
        out
    }
}

#[inline]
fn push_binary<'a>(
    work: &mut Vec<Job<'a>>,
    open: &'static str,
    left: Option<&'a Par>,
    right: Option<&'a Par>,
) {
    work.push(Job::Static(")"));
    push_par_or_nil(work, right);
    work.push(Job::Static(" "));
    push_par_or_nil(work, left);
    work.push(Job::Static(open));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_int() {
        let par = par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GInt(42)),
            }],
            ..Default::default()
        };
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "42");
    }

    #[test]
    fn test_simple_string() {
        let par = par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GString("hello".to_string())),
            }],
            ..Default::default()
        };
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "\"hello\"");
    }

    #[test]
    fn test_list() {
        let par = par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(crate::rhoapi::EList {
                    ps: vec![
                        par_from_default! {
                            exprs: vec![Expr {
                                expr_instance: Some(ExprInstance::GString("a".to_string())),
                            }],
                            ..Default::default()
                        },
                        par_from_default! {
                            exprs: vec![Expr {
                                expr_instance: Some(ExprInstance::GString("b".to_string())),
                            }],
                            ..Default::default()
                        },
                    ],
                    locally_free: vec![],
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
        assert_eq!(ParToSExpr::par_to_sexpr(&par), "[\"a\" \"b\"]");
    }

    #[test]
    fn depth_4096_uses_a_fixed_small_native_stack() {
        std::thread::Builder::new()
            .name("par-to-sexpr-pda".to_owned())
            .stack_size(256 * 1024)
            .spawn(|| {
                let depth = 4_096usize;
                let mut par = par_from_default! {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GInt(1)),
                    }],
                    ..Default::default()
                };
                for _ in 0..depth {
                    par = par_from_default! {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::EListBody(crate::rhoapi::EList {
                                ps: vec![par],
                                locally_free: Vec::new(),
                                connective_used: false,
                                remainder: None,
                            })),
                        }],
                        ..Default::default()
                    };
                }

                let rendered = ParToSExpr::par_to_sexpr(&par);
                assert_eq!(rendered.len(), depth * 2 + 1);
                assert!(rendered.starts_with("[[[["));
                assert!(rendered.ends_with("]]]]"));
            })
            .expect("spawn fixed-stack ParToSExpr probe")
            .join()
            .expect("ParToSExpr must not grow the native stack with AST depth");
    }
}
