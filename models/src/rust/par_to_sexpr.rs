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
enum Job {
    Par(Par),
    Expr(Expr),
    Send(Send),
    Receive(Receive),
    Bind(ReceiveBind),
    New(New),
    Bundle(Bundle),
    Static(&'static str),
    Raw(String),
    Quoted(String),
    Uri(String),
    Bytes(Vec<u8>),
    Bool(bool),
    Int(i64),
    Binder(i32),
    Nil,
}

#[inline]
fn push_par_or_nil(work: &mut Vec<Job>, par: Option<Par>) {
    work.push(par.map_or(Job::Nil, Job::Par));
}

#[inline]
fn push_pars(work: &mut Vec<Job>, pars: Vec<Par>) {
    for (index, par) in pars.into_iter().enumerate().rev() {
        work.push(Job::Par(par));
        if index != 0 {
            work.push(Job::Static(" "));
        }
    }
}

fn push_par_components(work: &mut Vec<Job>, par: &Par) {
    let part_count = par.family_cardinality::<Send>()
        + par.family_cardinality::<Receive>()
        + par.family_cardinality::<New>()
        + par.family_cardinality::<Expr>()
        + par.family_cardinality::<Bundle>();
    if part_count == 0 {
        work.push(Job::Nil);
        return;
    }
    if part_count > 1 {
        work.push(Job::Static(")"));
    }

    let mut parts = Vec::new();
    par.try_visit_occurrences::<Send, std::convert::Infallible>(|value| {
        parts.push(Job::Send(value.clone()));
        Ok(())
    })
    .expect("resident Send components decode");
    par.try_visit_occurrences::<Receive, std::convert::Infallible>(|value| {
        parts.push(Job::Receive(value.clone()));
        Ok(())
    })
    .expect("resident Receive components decode");
    par.try_visit_occurrences::<New, std::convert::Infallible>(|value| {
        parts.push(Job::New(value.clone()));
        Ok(())
    })
    .expect("resident New components decode");
    par.try_visit_occurrences::<Expr, std::convert::Infallible>(|value| {
        parts.push(Job::Expr(value.clone()));
        Ok(())
    })
    .expect("resident Expr components decode");
    par.try_visit_occurrences::<Bundle, std::convert::Infallible>(|value| {
        parts.push(Job::Bundle(value.clone()));
        Ok(())
    })
    .expect("resident Bundle components decode");

    for (index, part) in parts.into_iter().enumerate().rev() {
        work.push(part);
        if index != 0 {
            work.push(Job::Static(" "));
        }
    }
    if part_count > 1 {
        work.push(Job::Static("(par "));
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
        let mut work = vec![Job::Par(par.clone())];

        while let Some(job) = work.pop() {
            match job {
                Job::Static(text) => out.push_str(text),
                Job::Raw(text) => out.push_str(&text),
                Job::Nil => out.push_str("Nil"),
                Job::Bool(value) => write!(out, "{value}").expect("String writes are infallible"),
                Job::Int(value) => write!(out, "{value}").expect("String writes are infallible"),
                Job::Binder(index) => {
                    write!(out, "x{index}").expect("String writes are infallible")
                }
                Job::Quoted(value) => {
                    out.push('"');
                    out.push_str(&value);
                    out.push('"');
                }
                Job::Uri(value) => {
                    out.push('`');
                    out.push_str(&value);
                    out.push('`');
                }
                Job::Bytes(value) => {
                    out.push_str("0x");
                    out.push_str(&hex::encode(value));
                }
                Job::Par(par) => push_par_components(&mut work, &par),
                Job::Send(mut send) => {
                    work.push(Job::Static(")"));
                    push_pars(&mut work, std::mem::take(&mut send.data));
                    work.push(Job::Static(" "));
                    push_par_or_nil(&mut work, send.chan.take());
                    work.push(Job::Static("(! "));
                }
                Job::Receive(mut receive) => {
                    work.push(Job::Static(")"));
                    push_par_or_nil(&mut work, receive.body.take());
                    work.push(Job::Static(" "));
                    work.push(Job::Static(")"));
                    for (index, bind) in std::mem::take(&mut receive.binds)
                        .into_iter()
                        .enumerate()
                        .rev()
                    {
                        work.push(Job::Bind(bind));
                        if index != 0 {
                            work.push(Job::Static(" "));
                        }
                    }
                    work.push(Job::Static("(for ("));
                }
                Job::Bind(mut bind) => {
                    work.push(Job::Static(")"));
                    push_par_or_nil(&mut work, bind.source.take());
                    work.push(Job::Static("(bind <- "));
                }
                Job::New(mut new) => {
                    work.push(Job::Static(")"));
                    push_par_or_nil(&mut work, new.p.take());
                    work.push(Job::Static(" "));
                    for index in (0..new.bind_count).rev() {
                        work.push(Job::Binder(index));
                        if index != 0 {
                            work.push(Job::Static(" "));
                        }
                    }
                    work.push(Job::Static("(new "));
                }
                Job::Bundle(mut bundle) => {
                    work.push(Job::Static(")"));
                    push_par_or_nil(&mut work, bundle.body.take());
                    work.push(Job::Static("(bundle "));
                }
                Job::Expr(expr) => match expr.expr_instance {
                    None => work.push(Job::Nil),
                    Some(ExprInstance::GBool(value)) => work.push(Job::Bool(value)),
                    Some(ExprInstance::GInt(value)) => work.push(Job::Int(value)),
                    Some(ExprInstance::GString(value)) => work.push(Job::Quoted(value)),
                    Some(ExprInstance::GUri(value)) => work.push(Job::Uri(value)),
                    Some(ExprInstance::GByteArray(value)) => work.push(Job::Bytes(value)),
                    Some(ExprInstance::EListBody(list)) => {
                        work.push(Job::Static("]"));
                        push_pars(&mut work, list.ps);
                        work.push(Job::Static("["));
                    }
                    Some(ExprInstance::ETupleBody(tuple)) => {
                        work.push(Job::Static(")"));
                        push_pars(&mut work, tuple.ps);
                        work.push(Job::Static("(tuple "));
                    }
                    Some(ExprInstance::ESetBody(set)) => {
                        work.push(Job::Static(")"));
                        push_pars(&mut work, set.ps);
                        work.push(Job::Static("(set "));
                    }
                    Some(ExprInstance::EMapBody(map)) => {
                        work.push(Job::Static(")"));
                        for (index, mut pair) in map.kvs.into_iter().enumerate().rev() {
                            work.push(Job::Static(")"));
                            push_par_or_nil(&mut work, pair.value.take());
                            work.push(Job::Static(" : "));
                            push_par_or_nil(&mut work, pair.key.take());
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
                        push_par_or_nil(&mut work, unary.p);
                        work.push(Job::Static("(- "));
                    }
                    Some(ExprInstance::ENotBody(unary)) => {
                        work.push(Job::Static(")"));
                        push_par_or_nil(&mut work, unary.p);
                        work.push(Job::Static("(not "));
                    }
                    Some(ExprInstance::EMultBody(binary)) => {
                        push_binary(&mut work, "(* ", binary.p1, binary.p2);
                    }
                    Some(ExprInstance::EDivBody(binary)) => {
                        push_binary(&mut work, "(/ ", binary.p1, binary.p2);
                    }
                    Some(ExprInstance::EPlusBody(binary)) => {
                        push_binary(&mut work, "(+ ", binary.p1, binary.p2);
                    }
                    Some(ExprInstance::EMinusBody(binary)) => {
                        push_binary(&mut work, "(- ", binary.p1, binary.p2);
                    }
                    Some(ExprInstance::EMethodBody(method)) => {
                        work.push(Job::Static(")"));
                        push_pars(&mut work, method.arguments);
                        work.push(Job::Static(" "));
                        work.push(Job::Raw(method.method_name));
                        work.push(Job::Static("."));
                        push_par_or_nil(&mut work, method.target);
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
fn push_binary(work: &mut Vec<Job>, open: &'static str, left: Option<Par>, right: Option<Par>) {
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
    fn depth_256_uses_a_fixed_128_kib_native_stack() {
        std::thread::Builder::new()
            .name("par-to-sexpr-pda".to_owned())
            .stack_size(128 * 1024)
            .spawn(|| {
                // A recursive formatter exhausts 128 KiB well before this
                // depth. The dedicated protobuf and bincode gates carry the
                // 4,096-level corpus; repeating that rung here mostly measures
                // construction of every immutable encoded prefix in an
                // unoptimized unit-test build.
                let depth = 256usize;
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
