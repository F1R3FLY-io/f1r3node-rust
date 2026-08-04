use models::rhoapi::{
    Bundle, Connective, Expr, GUnforgeable, Match, New, Par, Receive, ReceiveBind, Send,
};

/*
 * This file is doing a custom comparison with two 'Pars'
 * It checks all the fields except 'locallyFree'
 * This is NOT handling every check of 'locallyFree' for each message in 'RhoTypes.proto'
 * Only the necessary for the 'Par, Par' trait implementation
 * This may be a problem later on. If so, probably should switch to BitSet like Scala side does
*/

pub fn compare_sends_without_locally_free(a: &Send, b: &Send) -> bool {
    a.chan == b.chan
        && a.data == b.data
        && a.persistent == b.persistent
        && a.connective_used == b.connective_used
}

pub fn compare_receives_without_locally_free(a: &Receive, b: &Receive) -> bool {
    match_pars_drive(MatchWork::Receive(a, b))
}

pub fn compare_news_without_locally_free(a: &New, b: &New) -> bool {
    match_pars_drive(MatchWork::New(a, b))
}

pub fn compare_exprs_without_locally_free(a: &Expr, b: &Expr) -> bool {
    a.expr_instance == b.expr_instance
}

pub fn compare_matches_without_locally_free(a: &Match, b: &Match) -> bool {
    a.target == b.target && a.cases == b.cases && a.connective_used == b.connective_used
}

pub fn compare_unforgeables_without_locally_free(a: &GUnforgeable, b: &GUnforgeable) -> bool {
    a.unf_instance == b.unf_instance
}

pub fn compare_bundles_without_locally_free(a: &Bundle, b: &Bundle) -> bool {
    a.body == b.body && a.write_flag == b.write_flag && a.read_flag == b.read_flag
}

pub fn compare_connectives_without_locally_free(a: &Connective, b: &Connective) -> bool {
    a.connective_instance == b.connective_instance
}

pub fn compare_receive_binds_without_locally_free(a: &ReceiveBind, b: &ReceiveBind) -> bool {
    match_pars_drive(MatchWork::ReceiveBind(a, b))
}

pub fn match_pars(target: &Par, pattern: &Par) -> bool {
    match_pars_drive(MatchWork::Par(target, pattern))
}

/// The recursive edges of the concrete fast path. All other fields are either
/// scalars or use the schema-generated stack-safe equality PDA.
enum MatchWork<'a> {
    Par(&'a Par, &'a Par),
    Receive(&'a Receive, &'a Receive),
    New(&'a New, &'a New),
    ReceiveBind(&'a ReceiveBind, &'a ReceiveBind),
}

fn match_pars_drive(root: MatchWork<'_>) -> bool {
    let mut work = vec![root];
    while let Some(task) = work.pop() {
        match task {
            MatchWork::Par(target, pattern) => {
                if target.sends.len() != pattern.sends.len()
                    || target.receives.len() != pattern.receives.len()
                    || target.news.len() != pattern.news.len()
                    || target.exprs.len() != pattern.exprs.len()
                    || target.matches.len() != pattern.matches.len()
                    || target.unforgeables.len() != pattern.unforgeables.len()
                    || target.bundles.len() != pattern.bundles.len()
                    || target.connective_used != pattern.connective_used
                    || !target
                        .sends
                        .iter()
                        .zip(&pattern.sends)
                        .all(|(target, pattern)| {
                            compare_sends_without_locally_free(target, pattern)
                        })
                    || !target
                        .exprs
                        .iter()
                        .zip(&pattern.exprs)
                        .all(|(target, pattern)| {
                            compare_exprs_without_locally_free(target, pattern)
                        })
                    || !target
                        .matches
                        .iter()
                        .zip(&pattern.matches)
                        .all(|(target, pattern)| {
                            compare_matches_without_locally_free(target, pattern)
                        })
                    || !target.unforgeables.iter().zip(&pattern.unforgeables).all(
                        |(target, pattern)| {
                            compare_unforgeables_without_locally_free(target, pattern)
                        },
                    )
                    || !target
                        .bundles
                        .iter()
                        .zip(&pattern.bundles)
                        .all(|(target, pattern)| {
                            compare_bundles_without_locally_free(target, pattern)
                        })
                    || !target.connectives.iter().zip(&pattern.connectives).all(
                        |(target, pattern)| {
                            compare_connectives_without_locally_free(target, pattern)
                        },
                    )
                {
                    return false;
                }
                work.extend(
                    target
                        .news
                        .iter()
                        .zip(&pattern.news)
                        .rev()
                        .map(|(target, pattern)| MatchWork::New(target, pattern)),
                );
                work.extend(
                    target
                        .receives
                        .iter()
                        .zip(&pattern.receives)
                        .rev()
                        .map(|(target, pattern)| MatchWork::Receive(target, pattern)),
                );
            }
            MatchWork::Receive(target, pattern) => {
                if target.persistent != pattern.persistent
                    || target.peek != pattern.peek
                    || target.bind_count != pattern.bind_count
                    || target.connective_used != pattern.connective_used
                {
                    return false;
                }
                work.push(MatchWork::Par(
                    target.body.as_ref().expect("Receive.body (target)"),
                    pattern.body.as_ref().expect("Receive.body (pattern)"),
                ));
                work.extend(
                    target
                        .binds
                        .iter()
                        .zip(&pattern.binds)
                        .rev()
                        .map(|(target, pattern)| MatchWork::ReceiveBind(target, pattern)),
                );
            }
            MatchWork::New(target, pattern) => {
                if target.bind_count != pattern.bind_count
                    || target.uri != pattern.uri
                    || target.injections != pattern.injections
                {
                    return false;
                }
                work.push(MatchWork::Par(
                    target.p.as_ref().expect("New.p (target)"),
                    pattern.p.as_ref().expect("New.p (pattern)"),
                ));
            }
            MatchWork::ReceiveBind(target, pattern) => {
                if target.remainder != pattern.remainder || target.free_count != pattern.free_count
                {
                    return false;
                }
                work.push(MatchWork::Par(
                    target.source.as_ref().expect("ReceiveBind.source (target)"),
                    pattern
                        .source
                        .as_ref()
                        .expect("ReceiveBind.source (pattern)"),
                ));
                work.extend(
                    target
                        .patterns
                        .iter()
                        .zip(&pattern.patterns)
                        .rev()
                        .map(|(target, pattern)| MatchWork::Par(target, pattern)),
                );
            }
        }
    }
    true
}
