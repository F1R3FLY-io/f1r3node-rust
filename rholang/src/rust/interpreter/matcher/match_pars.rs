use models::rhoapi::{
    Bundle, Connective, Expr, GUnforgeable, If, Match, New, Par, Receive, ReceiveBind, Send,
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
    match_pars_drive(MatchWork::Receive(a.clone(), b.clone()))
}

pub fn compare_news_without_locally_free(a: &New, b: &New) -> bool {
    match_pars_drive(MatchWork::New(a.clone(), b.clone()))
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

pub fn compare_ifs_without_locally_free(a: &If, b: &If) -> bool {
    a.condition == b.condition
        && a.if_true == b.if_true
        && a.if_false == b.if_false
        && a.connective_used == b.connective_used
}

pub fn compare_receive_binds_without_locally_free(a: &ReceiveBind, b: &ReceiveBind) -> bool {
    match_pars_drive(MatchWork::ReceiveBind(a.clone(), b.clone()))
}

pub fn match_pars(target: &Par, pattern: &Par) -> bool {
    match_pars_drive(MatchWork::Par(target.clone(), pattern.clone()))
}

/// The recursive edges of the concrete fast path. All other fields are either
/// scalars or use the schema-generated stack-safe equality PDA.
enum MatchWork {
    Par(Par, Par),
    Receive(Receive, Receive),
    New(New, New),
    ReceiveBind(ReceiveBind, ReceiveBind),
}

fn match_pars_drive(root: MatchWork) -> bool {
    let mut work = vec![root];
    while let Some(task) = work.pop() {
        match task {
            MatchWork::Par(target, pattern) => {
                if target.family_cardinality::<Send>() != pattern.family_cardinality::<Send>()
                    || target.family_cardinality::<Receive>()
                        != pattern.family_cardinality::<Receive>()
                    || target.family_cardinality::<New>() != pattern.family_cardinality::<New>()
                    || target.family_cardinality::<Expr>() != pattern.family_cardinality::<Expr>()
                    || target.family_cardinality::<Match>() != pattern.family_cardinality::<Match>()
                    || target.family_cardinality::<GUnforgeable>()
                        != pattern.family_cardinality::<GUnforgeable>()
                    || target.family_cardinality::<Bundle>()
                        != pattern.family_cardinality::<Bundle>()
                    || target.family_cardinality::<Connective>()
                        != pattern.family_cardinality::<Connective>()
                    || target.family_cardinality::<If>() != pattern.family_cardinality::<If>()
                    || target.connective_used() != pattern.connective_used()
                {
                    return false;
                }

                let target_sends = target.decode_occurrences::<Send>();
                let pattern_sends = pattern.decode_occurrences::<Send>();
                let target_exprs = target.decode_occurrences::<Expr>();
                let pattern_exprs = pattern.decode_occurrences::<Expr>();
                let target_matches = target.decode_occurrences::<Match>();
                let pattern_matches = pattern.decode_occurrences::<Match>();
                let target_unforgeables = target.decode_occurrences::<GUnforgeable>();
                let pattern_unforgeables = pattern.decode_occurrences::<GUnforgeable>();
                let target_bundles = target.decode_occurrences::<Bundle>();
                let pattern_bundles = pattern.decode_occurrences::<Bundle>();
                let target_connectives = target.decode_occurrences::<Connective>();
                let pattern_connectives = pattern.decode_occurrences::<Connective>();
                let target_conditionals = target.decode_occurrences::<If>();
                let pattern_conditionals = pattern.decode_occurrences::<If>();

                if !target_sends
                    .iter()
                    .zip(&pattern_sends)
                    .all(|(target, pattern)| compare_sends_without_locally_free(target, pattern))
                    || !target_exprs
                        .iter()
                        .zip(&pattern_exprs)
                        .all(|(target, pattern)| {
                            compare_exprs_without_locally_free(target, pattern)
                        })
                    || !target_matches
                        .iter()
                        .zip(&pattern_matches)
                        .all(|(target, pattern)| {
                            compare_matches_without_locally_free(target, pattern)
                        })
                    || !target_unforgeables.iter().zip(&pattern_unforgeables).all(
                        |(target, pattern)| {
                            compare_unforgeables_without_locally_free(target, pattern)
                        },
                    )
                    || !target_bundles
                        .iter()
                        .zip(&pattern_bundles)
                        .all(|(target, pattern)| {
                            compare_bundles_without_locally_free(target, pattern)
                        })
                    || !target_connectives.iter().zip(&pattern_connectives).all(
                        |(target, pattern)| {
                            compare_connectives_without_locally_free(target, pattern)
                        },
                    )
                    || !target_conditionals
                        .iter()
                        .zip(&pattern_conditionals)
                        .all(|(target, pattern)| compare_ifs_without_locally_free(target, pattern))
                {
                    return false;
                }
                work.extend(
                    target
                        .decode_occurrences::<New>()
                        .into_iter()
                        .zip(pattern.decode_occurrences::<New>())
                        .rev()
                        .map(|(target, pattern)| MatchWork::New(target, pattern)),
                );
                work.extend(
                    target
                        .decode_occurrences::<Receive>()
                        .into_iter()
                        .zip(pattern.decode_occurrences::<Receive>())
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
                    target.body.expect("Receive.body (target)"),
                    pattern.body.expect("Receive.body (pattern)"),
                ));
                work.extend(
                    target
                        .binds
                        .into_iter()
                        .zip(pattern.binds)
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
                    target.p.expect("New.p (target)"),
                    pattern.p.expect("New.p (pattern)"),
                ));
            }
            MatchWork::ReceiveBind(target, pattern) => {
                if target.remainder != pattern.remainder || target.free_count != pattern.free_count
                {
                    return false;
                }
                work.push(MatchWork::Par(
                    target.source.expect("ReceiveBind.source (target)"),
                    pattern.source.expect("ReceiveBind.source (pattern)"),
                ));
                work.extend(
                    target
                        .patterns
                        .into_iter()
                        .zip(pattern.patterns)
                        .rev()
                        .map(|(target, pattern)| MatchWork::Par(target, pattern)),
                );
            }
        }
    }
    true
}
