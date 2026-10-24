use super::cost_accounting_sorter::{combine_signature, combine_signed_term, combine_stack};
use super::score_tree::ScoredTerm;
use super::sort_combine::{
    combine_bind, combine_bundle, combine_case, combine_connective, combine_expr, combine_if,
    combine_match, combine_new, combine_par, combine_receive, combine_send, connective_child_pars,
    empty_par, expr_child_pars, sort_unforgeable, ParParts,
};
use crate::rhoapi::cost_signature::Value;
use crate::rhoapi::{
    Bundle, Connective, CostSignature, CostSignedTerm, CostStack, Expr, GUnforgeable, If, Match,
    MatchCase, New, Par, Receive, ReceiveBind, Send,
};

pub(crate) enum SortVal {
    Par(ScoredTerm<Par>),
    Signature(ScoredTerm<CostSignature>),
    Signed(ScoredTerm<CostSignedTerm>),
    Stack(ScoredTerm<CostStack>),
    Expr(ScoredTerm<Expr>),
    Send(ScoredTerm<Send>),
    Receive(ScoredTerm<Receive>),
    Bind(ScoredTerm<ReceiveBind>),
    New(ScoredTerm<New>),
    Match(ScoredTerm<Match>),
    Case(ScoredTerm<MatchCase>),
    If(ScoredTerm<If>),
    Bundle(ScoredTerm<Bundle>),
    Connective(ScoredTerm<Connective>),
    Unforgeable(ScoredTerm<GUnforgeable>),
}

pub(crate) struct ValItem {
    pub idx: u32,
    pub val: SortVal,
}

pub(crate) enum SortWork<'t> {
    Par(&'t Par),
    Signature(&'t CostSignature),
    Signed(&'t CostSignedTerm),
    Stack(&'t CostStack),
    Expr(&'t Expr),
    Send(&'t Send),
    Receive(&'t Receive),
    Bind(&'t ReceiveBind),
    New(&'t New),
    Match(&'t Match),
    Case(&'t MatchCase),
    If(&'t If),
    Bundle(&'t Bundle),
    Connective(&'t Connective),
    Unforgeable(&'t GUnforgeable),
    Combine(SortKont<'t>),
}

pub(crate) struct WorkItem<'t> {
    pub idx: u32,
    pub work: SortWork<'t>,
}

pub(crate) enum SortKont<'t> {
    SignatureK {
        signature: &'t CostSignature,
        n: usize,
    },
    SignedK {
        term: &'t CostSignedTerm,
    },
    StackK {
        stack: &'t CostStack,
    },
    ParK {
        par: &'t Par,
        n_sends: usize,
        n_receives: usize,
        n_exprs: usize,
        n_news: usize,
        n_matches: usize,
        n_bundles: usize,
        n_connectives: usize,
        n_unforgeables: usize,
        n_conditionals: usize,
        n_cost_signed: usize,
        n_cost_stacks: usize,
    },
    SendK {
        send: &'t Send,
        n_data: usize,
    },
    BindK {
        bind: &'t ReceiveBind,
        n_patterns: usize,
    },
    ReceiveK {
        recv: &'t Receive,
        n_binds: usize,
    },
    NewK {
        new: &'t New,
        n_injections: usize,
    },
    CaseK {
        case: &'t MatchCase,
    },
    MatchK {
        m: &'t Match,
        n_cases: usize,
    },
    IfK {
        cond: &'t If,
    },
    BundleK {
        bundle: &'t Bundle,
    },
    ConnK {
        conn: &'t Connective,
        n: usize,
    },
    ExprK {
        expr: &'t Expr,
        n: usize,
    },
}

impl SortKont<'_> {
    fn arity(&self) -> usize {
        match self {
            SortKont::ParK {
                n_sends,
                n_receives,
                n_exprs,
                n_news,
                n_matches,
                n_bundles,
                n_connectives,
                n_unforgeables,
                n_conditionals,
                n_cost_signed,
                n_cost_stacks,
                ..
            } => {
                n_sends
                    + n_receives
                    + n_exprs
                    + n_news
                    + n_matches
                    + n_bundles
                    + n_connectives
                    + n_unforgeables
                    + n_conditionals
                    + n_cost_signed
                    + n_cost_stacks
            }
            SortKont::SendK { n_data, .. } => 1 + n_data,
            SortKont::BindK { bind, n_patterns } => {
                n_patterns + 1 + usize::from(bind.cost_signature.is_some())
            }
            SortKont::ReceiveK { n_binds, .. } => n_binds + 2,
            SortKont::NewK { n_injections, .. } => 1 + n_injections,
            SortKont::CaseK { .. } => 3,
            SortKont::MatchK { n_cases, .. } => 1 + n_cases,
            SortKont::IfK { .. } => 3,
            SortKont::BundleK { .. } => 1,
            SortKont::ConnK { n, .. } => *n,
            SortKont::ExprK { n, .. } => *n,
            SortKont::SignatureK { n, .. } => *n,
            SortKont::SignedK { term } => {
                usize::from(term.body.is_some()) + usize::from(term.signature.is_some())
            }
            SortKont::StackK { stack } => stack.cells.len(),
        }
    }
}

macro_rules! sort_pop_ix {
    ($name:ident, $variant:ident, $ty:ty) => {
        #[inline]
        fn $name(vals: &mut Vec<ValItem>) -> (u32, ScoredTerm<$ty>) {
            match vals.pop() {
                Some(ValItem {
                    idx,
                    val: SortVal::$variant(v),
                }) => (idx, v),
                _ => unreachable!(concat!(
                    "sort_drive: expected ",
                    stringify!($variant),
                    " on the value stack"
                )),
            }
        }
    };
}

sort_pop_ix!(pop_par_ix, Par, Par);
sort_pop_ix!(pop_expr_ix, Expr, Expr);
sort_pop_ix!(pop_send_ix, Send, Send);
sort_pop_ix!(pop_receive_ix, Receive, Receive);
sort_pop_ix!(pop_bind_ix, Bind, ReceiveBind);
sort_pop_ix!(pop_new_ix, New, New);
sort_pop_ix!(pop_match_ix, Match, Match);
sort_pop_ix!(pop_case_ix, Case, MatchCase);
sort_pop_ix!(pop_if_ix, If, If);
sort_pop_ix!(pop_bundle_ix, Bundle, Bundle);
sort_pop_ix!(pop_connective_ix, Connective, Connective);
sort_pop_ix!(pop_unforgeable_ix, Unforgeable, GUnforgeable);

#[inline]
fn pop_par(vals: &mut Vec<ValItem>) -> ScoredTerm<Par> { pop_par_ix(vals).1 }

macro_rules! sort_pop_n {
    ($name:ident, $one:ident, $ty:ty) => {
        #[inline]
        fn $name(vals: &mut Vec<ValItem>, n: usize) -> Vec<ScoredTerm<$ty>> {
            let mut out = Vec::with_capacity(n);
            let mut previous: Option<u32> = None;
            for _ in 0..n {
                let (idx, v) = $one(vals);
                if let Some(p) = previous {
                    debug_assert_eq!(
                        idx + 1,
                        p,
                        concat!(
                            "sort_drive: ",
                            stringify!($name),
                            " popped children out of source order — a `descend_*` did not \
                             push in reverse. That changes SIBLING ORDER, which is the \
                             canonical form, which is what cost_accounting/sig.rs signs."
                        )
                    );
                }
                previous = Some(idx);
                out.push(v);
            }
            out.reverse();
            out
        }
    };
}

sort_pop_n!(pop_n_par, pop_par_ix, Par);
sort_pop_n!(pop_n_expr, pop_expr_ix, Expr);
sort_pop_n!(pop_n_send, pop_send_ix, Send);
sort_pop_n!(pop_n_receive, pop_receive_ix, Receive);
sort_pop_n!(pop_n_bind, pop_bind_ix, ReceiveBind);
sort_pop_n!(pop_n_new, pop_new_ix, New);
sort_pop_n!(pop_n_match, pop_match_ix, Match);
sort_pop_n!(pop_n_case, pop_case_ix, MatchCase);
sort_pop_n!(pop_n_if, pop_if_ix, If);
sort_pop_n!(pop_n_bundle, pop_bundle_ix, Bundle);
sort_pop_n!(pop_n_connective, pop_connective_ix, Connective);
sort_pop_n!(pop_n_unforgeable, pop_unforgeable_ix, GUnforgeable);

#[inline]
fn assert_children_are_in_source_order(vals: &[ValItem], arity: usize) {
    debug_assert!(
        vals.len() >= arity,
        "sort_drive: a Combine of arity {} found only {} values on the stack",
        arity,
        vals.len()
    );
    let base = vals.len() - arity;
    for (offset, item) in vals[base..].iter().enumerate() {
        debug_assert_eq!(
            item.idx as usize, offset,
            "sort_drive: child values are not a contiguous ascending source-index run \
             (arity {}, offset {}, found index {}). Either a `descend_*` pushed children \
             in the wrong order, or `SortKont::arity` disagrees with the slot list.",
            arity, offset, item.idx
        );
    }
}

#[inline]
fn push_reversed<'t, T, F>(work: &mut Vec<WorkItem<'t>>, next: &mut u32, items: &'t [T], mut f: F)
where F: FnMut(&'t T) -> SortWork<'t> {
    for item in items.iter().rev() {
        *next -= 1;
        work.push(WorkItem {
            idx: *next,
            work: f(item),
        });
    }
}

#[inline]
fn push_one<'t>(work: &mut Vec<WorkItem<'t>>, next: &mut u32, w: SortWork<'t>) {
    *next -= 1;
    work.push(WorkItem {
        idx: *next,
        work: w,
    });
}

fn descend_par<'t>(par: &'t Par, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let kont = SortKont::ParK {
        par,
        n_sends: par.sends.len(),
        n_receives: par.receives.len(),
        n_exprs: par.exprs.len(),
        n_news: par.news.len(),
        n_matches: par.matches.len(),
        n_bundles: par.bundles.len(),
        n_connectives: par.connectives.len(),
        n_unforgeables: par.unforgeables.len(),
        n_conditionals: par.conditionals.len(),
        n_cost_signed: par.cost_signed_terms.len(),
        n_cost_stacks: par.cost_stacks.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    push_reversed(work, &mut next, &par.cost_stacks, SortWork::Stack);
    push_reversed(work, &mut next, &par.cost_signed_terms, SortWork::Signed);
    push_reversed(work, &mut next, &par.conditionals, SortWork::If);
    push_reversed(work, &mut next, &par.unforgeables, SortWork::Unforgeable);
    push_reversed(work, &mut next, &par.connectives, SortWork::Connective);
    push_reversed(work, &mut next, &par.bundles, SortWork::Bundle);
    push_reversed(work, &mut next, &par.matches, SortWork::Match);
    push_reversed(work, &mut next, &par.news, SortWork::New);
    push_reversed(work, &mut next, &par.exprs, SortWork::Expr);
    push_reversed(work, &mut next, &par.receives, SortWork::Receive);
    push_reversed(work, &mut next, &par.sends, SortWork::Send);
    debug_assert_eq!(next, 0, "descend_par: slot list and arity disagree");
}

fn descend_send<'t>(send: &'t Send, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let chan = send
        .chan
        .as_ref()
        .expect("channel field on Send was None, should be Some");
    let kont = SortKont::SendK {
        send,
        n_data: send.data.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    push_reversed(work, &mut next, &send.data, SortWork::Par);
    push_one(work, &mut next, SortWork::Par(chan));
    debug_assert_eq!(next, 0, "descend_send: slot list and arity disagree");
}

fn descend_bind<'t>(bind: &'t ReceiveBind, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let source = bind
        .source
        .as_ref()
        .expect("source field on Bind was None, should be Some");
    let kont = SortKont::BindK {
        bind,
        n_patterns: bind.patterns.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    if let Some(signature) = &bind.cost_signature {
        push_one(work, &mut next, SortWork::Signature(signature));
    }
    push_one(work, &mut next, SortWork::Par(source));
    push_reversed(work, &mut next, &bind.patterns, SortWork::Par);
    debug_assert_eq!(next, 0, "descend_bind: slot list and arity disagree");
}

fn descend_receive<'t>(recv: &'t Receive, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let body = recv
        .body
        .as_ref()
        .expect("body field on Receive was None, should be Some");
    let condition = match recv.condition.as_ref() {
        Some(p) => p,
        None => empty_par(),
    };
    let kont = SortKont::ReceiveK {
        recv,
        n_binds: recv.binds.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    push_one(work, &mut next, SortWork::Par(condition));
    push_one(work, &mut next, SortWork::Par(body));
    push_reversed(work, &mut next, &recv.binds, SortWork::Bind);
    debug_assert_eq!(next, 0, "descend_receive: slot list and arity disagree");
}

fn descend_new<'t>(new: &'t New, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let p = new
        .p
        .as_ref()
        .expect("p field on New was None, should be Some");
    let kont = SortKont::NewK {
        new,
        n_injections: new.injections.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    for v in new.injections.values().rev() {
        push_one(work, &mut next, SortWork::Par(v));
    }
    push_one(work, &mut next, SortWork::Par(p));
    debug_assert_eq!(next, 0, "descend_new: slot list and arity disagree");
}

fn descend_case<'t>(case: &'t MatchCase, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let pattern = case
        .pattern
        .as_ref()
        .expect("pattern field on MatchCase was None, should be Some");
    let source = case
        .source
        .as_ref()
        .expect("source field on MatchCase was None, should be Some");
    let guard = match case.guard.as_ref() {
        Some(p) => p,
        None => empty_par(),
    };
    let kont = SortKont::CaseK { case };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    push_one(work, &mut next, SortWork::Par(guard));
    push_one(work, &mut next, SortWork::Par(source));
    push_one(work, &mut next, SortWork::Par(pattern));
    debug_assert_eq!(next, 0, "descend_case: slot list and arity disagree");
}

fn descend_match<'t>(m: &'t Match, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let target = m
        .target
        .as_ref()
        .expect("target field on Match was None, should be Some");
    let kont = SortKont::MatchK {
        m,
        n_cases: m.cases.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    push_reversed(work, &mut next, &m.cases, SortWork::Case);
    push_one(work, &mut next, SortWork::Par(target));
    debug_assert_eq!(next, 0, "descend_match: slot list and arity disagree");
}

fn descend_if<'t>(cond: &'t If, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let condition = cond
        .condition
        .as_ref()
        .expect("condition field on If was None, should be Some");
    let if_true = cond
        .if_true
        .as_ref()
        .expect("if_true field on If was None, should be Some");
    let if_false = cond
        .if_false
        .as_ref()
        .expect("if_false field on If was None, should be Some");
    let kont = SortKont::IfK { cond };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    push_one(work, &mut next, SortWork::Par(if_false));
    push_one(work, &mut next, SortWork::Par(if_true));
    push_one(work, &mut next, SortWork::Par(condition));
    debug_assert_eq!(next, 0, "descend_if: slot list and arity disagree");
}

fn descend_bundle<'t>(bundle: &'t Bundle, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let body = bundle
        .body
        .as_ref()
        .expect("body was None, should be Some(Par)");
    let kont = SortKont::BundleK { bundle };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    push_one(work, &mut next, SortWork::Par(body));
    debug_assert_eq!(next, 0, "descend_bundle: slot list and arity disagree");
}

fn descend_connective<'t>(conn: &'t Connective, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let mut kids: Vec<&'t Par> = Vec::new();
    connective_child_pars(conn, &mut kids);
    let kont = SortKont::ConnK {
        conn,
        n: kids.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    for p in kids.into_iter().rev() {
        push_one(work, &mut next, SortWork::Par(p));
    }
    debug_assert_eq!(next, 0, "descend_connective: slot list and arity disagree");
}

fn descend_expr<'t>(expr: &'t Expr, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let mut kids: Vec<&'t Par> = Vec::new();
    expr_child_pars(expr, &mut kids);
    let kont = SortKont::ExprK {
        expr,
        n: kids.len(),
    };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    for p in kids.into_iter().rev() {
        push_one(work, &mut next, SortWork::Par(p));
    }
    debug_assert_eq!(next, 0, "descend_expr: slot list and arity disagree");
}

pub(crate) fn sort_drive(root: SortWork<'_>) -> SortVal {
    let mut work: Vec<WorkItem<'_>> = Vec::with_capacity(32);
    let mut vals: Vec<ValItem> = Vec::with_capacity(32);
    work.push(WorkItem { idx: 0, work: root });

    let mut descends: usize = 1;
    let mut combines: usize = 0;
    let mut sum_arity: usize = 0;

    loop {
        debug_assert_eq!(
            vals.len() + descends + combines,
            1 + sum_arity,
            "sort_drive: DEFICIT INVARIANT VIOLATED (|vals|={}, descends={}, combines={}, \
             Σarity={}). `SortKont::arity` disagrees with what a `descend_*` pushed or a \
             `Combine` popped — the bug class that silently forks the canonical form.",
            vals.len(),
            descends,
            combines,
            sum_arity
        );

        let Some(WorkItem { idx, work: w }) = work.pop() else {
            break;
        };

        match w {
            SortWork::Combine(k) => {
                combines -= 1;
                let arity = k.arity();
                sum_arity -= arity;
                assert_children_are_in_source_order(&vals, arity);
                let before = work.len();
                let val = run_combine(k, &mut vals);
                debug_assert_eq!(before, work.len(), "a Combine must not push work");
                vals.push(ValItem { idx, val });
            }
            descend => {
                descends -= 1;
                let before_len = work.len();
                match descend {
                    SortWork::Signature(s) => descend_signature(s, idx, &mut work),
                    SortWork::Signed(t) => descend_signed(t, idx, &mut work),
                    SortWork::Stack(s) => descend_stack(s, idx, &mut work),
                    SortWork::Par(p) => descend_par(p, idx, &mut work),
                    SortWork::Expr(e) => descend_expr(e, idx, &mut work),
                    SortWork::Send(s) => descend_send(s, idx, &mut work),
                    SortWork::Receive(r) => descend_receive(r, idx, &mut work),
                    SortWork::Bind(b) => descend_bind(b, idx, &mut work),
                    SortWork::New(n) => descend_new(n, idx, &mut work),
                    SortWork::Match(m) => descend_match(m, idx, &mut work),
                    SortWork::Case(c) => descend_case(c, idx, &mut work),
                    SortWork::If(i) => descend_if(i, idx, &mut work),
                    SortWork::Bundle(b) => descend_bundle(b, idx, &mut work),
                    SortWork::Connective(c) => descend_connective(c, idx, &mut work),
                    SortWork::Unforgeable(u) => vals.push(ValItem {
                        idx,
                        val: SortVal::Unforgeable(sort_unforgeable(u)),
                    }),
                    SortWork::Combine(_) => unreachable!("handled by the arm above"),
                }
                let pushed = work.len() - before_len;
                if pushed > 0 {
                    let kont_arity = match &work[before_len].work {
                        SortWork::Combine(k) => k.arity(),
                        _ => unreachable!("a `descend_*` must push its Combine first"),
                    };
                    debug_assert_eq!(
                        pushed - 1,
                        kont_arity,
                        "sort_drive: a `descend_*` pushed {} children but its continuation \
                         declares arity {}",
                        pushed - 1,
                        kont_arity
                    );
                    combines += 1;
                    sum_arity += kont_arity;
                    descends += pushed - 1;
                }
            }
        }
    }

    debug_assert_eq!(vals.len(), 1, "sort_drive: exactly one value must remain");
    vals.pop()
        .expect("sort_drive: exactly one value must remain")
        .val
}

#[inline]
fn run_combine(k: SortKont<'_>, vals: &mut Vec<ValItem>) -> SortVal {
    match k {
        SortKont::ParK {
            par,
            n_sends,
            n_receives,
            n_exprs,
            n_news,
            n_matches,
            n_bundles,
            n_connectives,
            n_unforgeables,
            n_conditionals,
            n_cost_signed,
            n_cost_stacks,
        } => combine_par_k(
            par,
            [
                n_sends,
                n_receives,
                n_exprs,
                n_news,
                n_matches,
                n_bundles,
                n_connectives,
                n_unforgeables,
                n_conditionals,
                n_cost_signed,
                n_cost_stacks,
            ],
            vals,
        ),
        SortKont::SendK { send, n_data } => combine_send_k(send, n_data, vals),
        SortKont::BindK { bind, n_patterns } => combine_bind_k(bind, n_patterns, vals),
        SortKont::ReceiveK { recv, n_binds } => combine_receive_k(recv, n_binds, vals),
        SortKont::NewK { new, n_injections } => combine_new_k(new, n_injections, vals),
        SortKont::CaseK { case } => combine_case_k(case, vals),
        SortKont::MatchK { m, n_cases } => combine_match_k(m, n_cases, vals),
        SortKont::IfK { cond } => combine_if_k(cond, vals),
        SortKont::BundleK { bundle } => combine_bundle_k(bundle, vals),
        SortKont::ConnK { conn, n } => combine_conn_k(conn, n, vals),
        SortKont::ExprK { expr, n } => combine_expr_k(expr, n, vals),
        SortKont::SignatureK { signature, n } => combine_signature_k(signature, n, vals),
        SortKont::SignedK { term } => combine_signed_k(term, vals),
        SortKont::StackK { stack } => combine_stack_k(stack, vals),
    }
}

const N_SENDS: usize = 0;
const N_RECEIVES: usize = 1;
const N_EXPRS: usize = 2;
const N_NEWS: usize = 3;
const N_MATCHES: usize = 4;
const N_BUNDLES: usize = 5;
const N_CONNECTIVES: usize = 6;
const N_UNFORGEABLES: usize = 7;
const N_CONDITIONALS: usize = 8;

#[inline(never)]
fn combine_par_k(par: &Par, counts: [usize; 11], vals: &mut Vec<ValItem>) -> SortVal {
    let cost_stacks = pop_n_stack(vals, counts[10]);
    let cost_signed_terms = pop_n_signed(vals, counts[9]);
    let conditionals = pop_n_if(vals, counts[N_CONDITIONALS]);
    let unforgeables = pop_n_unforgeable(vals, counts[N_UNFORGEABLES]);
    let connectives = pop_n_connective(vals, counts[N_CONNECTIVES]);
    let bundles = pop_n_bundle(vals, counts[N_BUNDLES]);
    let matches = pop_n_match(vals, counts[N_MATCHES]);
    let news = pop_n_new(vals, counts[N_NEWS]);
    let exprs = pop_n_expr(vals, counts[N_EXPRS]);
    let receives = pop_n_receive(vals, counts[N_RECEIVES]);
    let sends = pop_n_send(vals, counts[N_SENDS]);
    SortVal::Par(combine_par(par, ParParts {
        sends,
        receives,
        exprs,
        news,
        matches,
        bundles,
        connectives,
        unforgeables,
        conditionals,
        cost_signed_terms,
        cost_stacks,
    }))
}

#[inline(never)]
fn combine_send_k(send: &Send, n_data: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let data = pop_n_par(vals, n_data);
    let chan = pop_par(vals);
    SortVal::Send(combine_send(send, chan, data))
}

#[inline(never)]
fn combine_bind_k(bind: &ReceiveBind, n_patterns: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let cost_signature = bind
        .cost_signature
        .as_ref()
        .map(|_| pop_signature_ix(vals).1);
    let channel = pop_par(vals);
    let patterns = pop_n_par(vals, n_patterns);
    SortVal::Bind(combine_bind(bind, patterns, channel, cost_signature))
}

#[inline(never)]
fn combine_receive_k(recv: &Receive, n_binds: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let condition = pop_par(vals);
    let body = pop_par(vals);
    let binds = pop_n_bind(vals, n_binds);
    SortVal::Receive(combine_receive(recv, binds, body, condition))
}

#[inline(never)]
fn combine_new_k(new: &New, n_injections: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let injections = pop_n_par(vals, n_injections);
    let p = pop_par(vals);
    SortVal::New(combine_new(new, p, injections))
}

#[inline(never)]
fn combine_case_k(case: &MatchCase, vals: &mut Vec<ValItem>) -> SortVal {
    let guard = pop_par(vals);
    let source = pop_par(vals);
    let pattern = pop_par(vals);
    SortVal::Case(combine_case(case, pattern, source, guard))
}

#[inline(never)]
fn combine_match_k(m: &Match, n_cases: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let cases = pop_n_case(vals, n_cases);
    let target = pop_par(vals);
    SortVal::Match(combine_match(m, target, cases))
}

#[inline(never)]
fn combine_if_k(cond: &If, vals: &mut Vec<ValItem>) -> SortVal {
    let if_false = pop_par(vals);
    let if_true = pop_par(vals);
    let condition = pop_par(vals);
    SortVal::If(combine_if(cond, condition, if_true, if_false))
}

#[inline(never)]
fn combine_bundle_k(bundle: &Bundle, vals: &mut Vec<ValItem>) -> SortVal {
    let body = pop_par(vals);
    SortVal::Bundle(combine_bundle(bundle, body))
}

#[inline(never)]
fn combine_conn_k(conn: &Connective, n: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let kids = pop_n_par(vals, n);
    SortVal::Connective(combine_connective(conn, kids))
}

#[inline(never)]
fn combine_expr_k(expr: &Expr, n: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let kids = pop_n_par(vals, n);
    SortVal::Expr(combine_expr(expr, kids))
}

macro_rules! sort_entry {
    ($name:ident, $work:ident, $variant:ident, $ty:ty) => {
        pub(crate) fn $name(term: &$ty) -> ScoredTerm<$ty> {
            match sort_drive(SortWork::$work(term)) {
                SortVal::$variant(v) => v,
                _ => unreachable!(concat!(
                    "sort_drive: ",
                    stringify!($work),
                    " must produce ",
                    stringify!($variant)
                )),
            }
        }
    };
}

sort_entry!(sort_par, Par, Par, Par);
sort_entry!(sort_expr, Expr, Expr, Expr);
sort_entry!(sort_send, Send, Send, Send);
sort_entry!(sort_receive, Receive, Receive, Receive);
sort_entry!(sort_bind_ref, Bind, Bind, ReceiveBind);
sort_entry!(sort_new, New, New, New);
sort_entry!(sort_match_node, Match, Match, Match);
sort_entry!(sort_if, If, If, If);
sort_entry!(sort_bundle, Bundle, Bundle, Bundle);
sort_entry!(sort_connective, Connective, Connective, Connective);

sort_pop_ix!(pop_signature_ix, Signature, CostSignature);
sort_pop_ix!(pop_signed_ix, Signed, CostSignedTerm);
sort_pop_ix!(pop_stack_ix, Stack, CostStack);
sort_pop_n!(pop_n_signature, pop_signature_ix, CostSignature);
sort_pop_n!(pop_n_signed, pop_signed_ix, CostSignedTerm);
sort_pop_n!(pop_n_stack, pop_stack_ix, CostStack);
sort_entry!(sort_signature, Signature, Signature, CostSignature);
sort_entry!(sort_signed_term, Signed, Signed, CostSignedTerm);
sort_entry!(sort_stack, Stack, Stack, CostStack);

fn descend_signature<'t>(signature: &'t CostSignature, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let mut children = Vec::new();
    match &signature.value {
        Some(Value::Quote(par) | Value::Name(par)) => children.push(SortWork::Par(par)),
        Some(Value::Compound(compound)) => {
            let mut pending: Vec<_> = compound.elements.iter().rev().collect();
            while let Some(child) = pending.pop() {
                match &child.value {
                    Some(Value::Compound(compound)) => {
                        pending.extend(compound.elements.iter().rev())
                    }
                    _ => children.push(SortWork::Signature(child)),
                }
            }
        }
        _ => {}
    }
    let n = children.len();
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(SortKont::SignatureK { signature, n }),
    });
    let mut next = n as u32;
    for child in children.into_iter().rev() {
        push_one(work, &mut next, child);
    }
}

fn descend_signed<'t>(term: &'t CostSignedTerm, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let kont = SortKont::SignedK { term };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    if let Some(signature) = &term.signature {
        push_one(work, &mut next, SortWork::Signature(signature));
    }
    if let Some(body) = &term.body {
        push_one(work, &mut next, SortWork::Par(body));
    }
}

fn descend_stack<'t>(stack: &'t CostStack, idx: u32, work: &mut Vec<WorkItem<'t>>) {
    let kont = SortKont::StackK { stack };
    let mut next = kont.arity() as u32;
    work.push(WorkItem {
        idx,
        work: SortWork::Combine(kont),
    });
    push_reversed(work, &mut next, &stack.cells, SortWork::Signature);
}

#[inline(never)]
fn combine_signature_k(signature: &CostSignature, n: usize, vals: &mut Vec<ValItem>) -> SortVal {
    let mut par = None;
    let mut elements = Vec::new();
    match &signature.value {
        Some(Value::Quote(_) | Value::Name(_)) => par = Some(pop_par(vals)),
        Some(Value::Compound(_)) => elements = pop_n_signature(vals, n),
        _ => {}
    }
    SortVal::Signature(combine_signature(signature, par, elements))
}

#[inline(never)]
fn combine_signed_k(term: &CostSignedTerm, vals: &mut Vec<ValItem>) -> SortVal {
    let signature = term.signature.as_ref().map(|_| pop_signature_ix(vals).1);
    let body = term.body.as_ref().map(|_| pop_par(vals));
    SortVal::Signed(combine_signed_term(term, body, signature))
}

#[inline(never)]
fn combine_stack_k(stack: &CostStack, vals: &mut Vec<ValItem>) -> SortVal {
    SortVal::Stack(combine_stack(pop_n_signature(vals, stack.cells.len())))
}
