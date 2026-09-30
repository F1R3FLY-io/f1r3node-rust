use std::mem::size_of;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, CostSignedTerm, CostStack, EVar, Expr, If, Match,
    MatchCase, New, Par, Receive, ReceiveBind, Send,
};
use models::rust::rholang::sorter::metered::SorterMeter;
use shared::rust::clone_backing::{self, BackingError, BackingMeter, CloneBacking};

use super::native_cost_signature::substitute_cost_signature_metered_with;
use super::{Env, InterpreterError, Substitute};

fn rejected(_: BackingError) -> InterpreterError { InterpreterError::HostWorkRejected }

fn required<T: Clone + std::fmt::Debug>(
    value: Option<T>,
    backing: &dyn BackingMeter,
) -> Result<T, InterpreterError> {
    if value.is_none() {
        let count = std::any::type_name::<T>()
            .len()
            .checked_add(2)
            .ok_or(InterpreterError::HostWorkRejected)?;
        cleanup_meter(backing)
            .reserve(1, count, count)
            .map_err(rejected)?;
    }
    super::unwrap_option_safe(value)
}

fn cleanup_meter(
    backing: &dyn BackingMeter,
) -> impl Fn(usize, usize, usize) -> Result<(), BackingError> + '_ {
    move |operations, scanned, bytes| {
        backing.reserve(
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            scanned,
            bytes,
        )
    }
}

fn collect_metered<T, U>(
    source: impl Iterator<Item = T>,
    backing: &dyn BackingMeter,
    mut transform: impl FnMut(T) -> Result<U, InterpreterError>,
) -> Result<Vec<U>, InterpreterError> {
    let meter = cleanup_meter(backing);
    let count = source
        .size_hint()
        .1
        .ok_or(InterpreterError::HostWorkRejected)?;
    let bytes = count
        .checked_mul(size_of::<U>())
        .ok_or(InterpreterError::HostWorkRejected)?;
    meter
        .reserve(
            count
                .checked_add(1)
                .ok_or(InterpreterError::HostWorkRejected)?,
            bytes,
            bytes,
        )
        .map_err(rejected)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    for value in source {
        values.push(transform(value)?);
    }
    Ok(values)
}

fn copy_metered<T: Clone + CloneBacking>(
    value: &T,
    backing: &dyn BackingMeter,
) -> Result<T, InterpreterError> {
    clone_backing::reserve_copy_and_cleanup(value, backing).map_err(rejected)?;
    Ok(value.clone())
}

fn set_bits_until_metered(
    bits: Vec<u8>,
    until: i32,
    backing: &dyn BackingMeter,
) -> Result<Vec<u8>, InterpreterError> {
    let count = if until <= 0 {
        0
    } else {
        bits.len().min(until as usize)
    };
    let meter = cleanup_meter(backing);
    meter
        .reserve(
            count
                .checked_add(1)
                .ok_or(InterpreterError::HostWorkRejected)?,
            count,
            count,
        )
        .map_err(rejected)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    result.extend(bits.into_iter().take(count));
    Ok(result)
}

fn shifted_env(
    env: &Env<Par>,
    amount: i32,
    backing: &dyn BackingMeter,
) -> Result<Env<Par>, InterpreterError> {
    let shift = env
        .shift
        .checked_add(amount)
        .ok_or(InterpreterError::HostWorkRejected)?;
    Ok(Env {
        env_map: copy_metered(&env.env_map, backing)?,
        level: env.level,
        shift,
    })
}

fn joined_vec<T>(
    left: Vec<T>,
    right: Vec<T>,
    backing: &dyn BackingMeter,
) -> Result<Vec<T>, InterpreterError> {
    let count = left
        .len()
        .checked_add(right.len())
        .ok_or(InterpreterError::HostWorkRejected)?;
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(InterpreterError::HostWorkRejected)?;
    cleanup_meter(backing)
        .reserve(
            count
                .checked_add(1)
                .ok_or(InterpreterError::HostWorkRejected)?,
            bytes,
            bytes,
        )
        .map_err(rejected)?;
    let mut joined = Vec::new();
    joined
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    joined.extend(right);
    joined.extend(left);
    Ok(joined)
}

fn prepended_vec<T>(
    value: T,
    tail: Vec<T>,
    backing: &dyn BackingMeter,
) -> Result<Vec<T>, InterpreterError> {
    let count = tail
        .len()
        .checked_add(1)
        .ok_or(InterpreterError::HostWorkRejected)?;
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(InterpreterError::HostWorkRejected)?;
    cleanup_meter(backing)
        .reserve(count, bytes, bytes)
        .map_err(rejected)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    result.push(value);
    result.extend(tail);
    Ok(result)
}

fn union_bits_metered(
    left: &[u8],
    right: &[u8],
    backing: &dyn BackingMeter,
) -> Result<Vec<u8>, InterpreterError> {
    let count = left.len().max(right.len());
    cleanup_meter(backing)
        .reserve(
            count
                .checked_add(1)
                .ok_or(InterpreterError::HostWorkRejected)?,
            count
                .checked_mul(2)
                .ok_or(InterpreterError::HostWorkRejected)?,
            count,
        )
        .map_err(rejected)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    for index in 0..count {
        result.push(left.get(index).copied().unwrap_or(0) | right.get(index).copied().unwrap_or(0));
    }
    Ok(result)
}

fn bit_vector_metered(index: i32, backing: &dyn BackingMeter) -> Result<Vec<u8>, InterpreterError> {
    let count = usize::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or(InterpreterError::HostWorkRejected)?;
    cleanup_meter(backing)
        .reserve(
            count
                .checked_add(1)
                .ok_or(InterpreterError::HostWorkRejected)?,
            count,
            count,
        )
        .map_err(rejected)?;
    let mut bits = Vec::new();
    bits.try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    bits.resize(count, 0);
    bits[count - 1] = 1;
    Ok(bits)
}

fn expr_metadata(
    expr: &Expr,
    depth: i32,
    backing: &dyn BackingMeter,
) -> Result<(Vec<u8>, bool), InterpreterError> {
    fn unary(
        value: &Option<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<(Vec<u8>, bool), InterpreterError> {
        let par = value.as_ref().ok_or(InterpreterError::HostWorkRejected)?;
        Ok((
            copy_metered(&par.locally_free, backing)?,
            par.connective_used,
        ))
    }
    fn binary(
        left: &Option<Par>,
        right: &Option<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<(Vec<u8>, bool), InterpreterError> {
        let left = left.as_ref().ok_or(InterpreterError::HostWorkRejected)?;
        let right = right.as_ref().ok_or(InterpreterError::HostWorkRejected)?;
        Ok((
            union_bits_metered(&left.locally_free, &right.locally_free, backing)?,
            left.connective_used | right.connective_used,
        ))
    }
    macro_rules! cached {
        ($($variant:ident),+ $(,)?) => { $(
            if let Some(ExprInstance::$variant(value)) = &expr.expr_instance {
                return Ok((copy_metered(&value.locally_free, backing)?, value.connective_used));
            }
        )+ };
    }
    macro_rules! binary_arms {
        ($($variant:ident),+ $(,)?) => { $(
            if let Some(ExprInstance::$variant(value)) = &expr.expr_instance {
                return binary(&value.p1, &value.p2, backing);
            }
        )+ };
    }
    cached!(
        EListBody,
        ETupleBody,
        ESetBody,
        EMapBody,
        EPathmapBody,
        EZipperBody
    );
    binary_arms!(
        EMultBody,
        EDivBody,
        EModBody,
        EPlusBody,
        EMinusBody,
        ELtBody,
        ELteBody,
        EGtBody,
        EGteBody,
        EEqBody,
        ENeqBody,
        EAndBody,
        EOrBody,
        EPercentPercentBody,
        EPlusPlusBody,
        EMinusMinusBody
    );
    match &expr.expr_instance {
        Some(ExprInstance::EVarBody(EVar { v })) => {
            let var = v.as_ref().ok_or(InterpreterError::HostWorkRejected)?;
            match var
                .var_instance
                .as_ref()
                .ok_or(InterpreterError::HostWorkRejected)?
            {
                VarInstance::BoundVar(index) if depth == 0 => {
                    Ok((bit_vector_metered(*index, backing)?, false))
                }
                VarInstance::BoundVar(_) => Ok((Vec::new(), false)),
                VarInstance::FreeVar(_) | VarInstance::Wildcard(_) => Ok((Vec::new(), true)),
            }
        }
        Some(ExprInstance::ENotBody(value)) => unary(&value.p, backing),
        Some(ExprInstance::ENegBody(value)) => unary(&value.p, backing),
        Some(ExprInstance::EMethodBody(value)) => Ok((
            copy_metered(&value.locally_free, backing)?,
            value.connective_used,
        )),
        Some(ExprInstance::EMatchesBody(value)) => unary(&value.target, backing),
        _ => Ok((Vec::new(), false)),
    }
}

fn concat_metered(
    left: Par,
    right: Par,
    backing: &dyn BackingMeter,
) -> Result<Par, InterpreterError> {
    let locally_free = union_bits_metered(&right.locally_free, &left.locally_free, backing)?;
    let connective_used = right.connective_used || left.connective_used;
    Ok(Par {
        sends: joined_vec(left.sends, right.sends, backing)?,
        receives: joined_vec(left.receives, right.receives, backing)?,
        news: joined_vec(left.news, right.news, backing)?,
        exprs: joined_vec(left.exprs, right.exprs, backing)?,
        matches: joined_vec(left.matches, right.matches, backing)?,
        unforgeables: joined_vec(left.unforgeables, right.unforgeables, backing)?,
        bundles: joined_vec(left.bundles, right.bundles, backing)?,
        connectives: joined_vec(left.connectives, right.connectives, backing)?,
        conditionals: joined_vec(left.conditionals, right.conditionals, backing)?,
        cost_signed_terms: joined_vec(left.cost_signed_terms, right.cost_signed_terms, backing)?,
        cost_stacks: joined_vec(left.cost_stacks, right.cost_stacks, backing)?,
        locally_free,
        connective_used,
    })
}

fn prepend_expr_metered(
    par: Par,
    expr: Expr,
    depth: i32,
    backing: &dyn BackingMeter,
) -> Result<Par, InterpreterError> {
    let (expr_free, expr_connective) = expr_metadata(&expr, depth, backing)?;
    let locally_free = union_bits_metered(&par.locally_free, &expr_free, backing)?;
    let connective_used = par.connective_used || expr_connective;
    let exprs = prepended_vec(expr, par.exprs, backing)?;
    Ok(Par {
        exprs,
        locally_free,
        connective_used,
        ..par
    })
}

fn prepend_connective_metered(
    par: Par,
    connective: Connective,
    depth: i32,
    backing: &dyn BackingMeter,
) -> Result<Par, InterpreterError> {
    let locally_free = match &connective.connective_instance {
        Some(ConnectiveInstance::VarRefBody(value)) if value.depth == depth => {
            bit_vector_metered(value.index, backing)?
        }
        _ => Vec::new(),
    };
    let connective_used = par.connective_used
        || !matches!(
            &connective.connective_instance,
            Some(ConnectiveInstance::VarRefBody(_)) | None
        );
    let connectives = prepended_vec(connective, par.connectives, backing)?;
    Ok(Par {
        connectives,
        locally_free,
        connective_used,
        ..par
    })
}

impl Substitute {
    pub(crate) fn substitute_par_metered(
        &self,
        term: Par,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Par, InterpreterError> {
        let prepaid = cleanup_meter(backing);
        let meter = SorterMeter::new(&prepaid);
        let _depth = meter.enter().map_err(rejected)?;
        meter.reserve(1, size_of::<Par>(), 0).map_err(rejected)?;

        let exprs = self.sub_exprs_metered(term.exprs, depth, env, backing)?;
        let connectives = self.sub_connectives_metered(term.connectives, depth, env, backing)?;
        let sends = collect_metered(term.sends.into_iter(), backing, |value| {
            self.sub_send_metered(value, depth, env, backing)
        })?;
        let bundles = collect_metered(term.bundles.into_iter(), backing, |value| {
            self.sub_bundle_metered(value, depth, env, backing)
        })?;
        let receives = collect_metered(term.receives.into_iter(), backing, |value| {
            self.sub_receive_metered(value, depth, env, backing)
        })?;
        let news = collect_metered(term.news.into_iter(), backing, |value| {
            self.sub_new_metered(value, depth, env, backing)
        })?;
        let matches = collect_metered(term.matches.into_iter(), backing, |value| {
            self.sub_match_metered(value, depth, env, backing)
        })?;
        let conditionals = collect_metered(term.conditionals.into_iter(), backing, |value| {
            self.sub_if_metered(value, depth, env, backing)
        })?;
        let cost_signed_terms =
            collect_metered(term.cost_signed_terms.into_iter(), backing, |value| {
                self.sub_cost_signed_metered(value, depth, env, backing)
            })?;
        let cost_stacks = collect_metered(term.cost_stacks.into_iter(), backing, |value| {
            self.sub_cost_stack_metered(value, depth, env, backing)
        })?;
        let base = Par {
            sends,
            receives,
            news,
            exprs: Vec::new(),
            matches,
            unforgeables: term.unforgeables,
            bundles,
            connectives: Vec::new(),
            conditionals,
            locally_free: set_bits_until_metered(term.locally_free, env.shift, backing)?,
            connective_used: term.connective_used,
            cost_signed_terms,
            cost_stacks,
        };
        concat_metered(exprs, concat_metered(connectives, base, backing)?, backing)
    }

    fn sub_exprs_metered(
        &self,
        exprs: Vec<Expr>,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Par, InterpreterError> {
        let mut result = Par::default();
        for expr in exprs {
            if let Some(ExprInstance::EVarBody(EVar { v })) = &expr.expr_instance {
                let var = required(copy_metered(v, backing)?, backing)?;
                if depth == 0 {
                    let VarInstance::BoundVar(index) =
                        required(copy_metered(&var.var_instance, backing)?, backing)?
                    else {
                        cleanup_meter(backing)
                            .reserve(1, size_of::<models::rhoapi::Var>(), 256)
                            .map_err(rejected)?;
                        return Err(InterpreterError::SubstituteError(format!(
                            "Illegal Substitution [{:?}]",
                            var
                        )));
                    };
                    let position = env
                        .level
                        .checked_add(env.shift)
                        .and_then(|n| n.checked_sub(index))
                        .and_then(|n| n.checked_sub(1));
                    if position.is_some() {
                        let value = env
                            .get_metered(&index, &cleanup_meter(backing))
                            .map_err(rejected)?;
                        if let Some(value) = value {
                            result = concat_metered(value, result, backing)?;
                            continue;
                        }
                    }
                }
                result = prepend_expr_metered(result, expr, depth, backing)?;
            } else {
                let expr = self.substitute_expr_metered(expr, depth, env, backing)?;
                result = prepend_expr_metered(result, expr, depth, backing)?;
            }
        }
        Ok(result)
    }

    fn sub_connectives_metered(
        &self,
        connectives: Vec<Connective>,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Par, InterpreterError> {
        let mut result = Par::default();
        for connective in connectives {
            let Some(instance) = connective.connective_instance else {
                continue;
            };
            let mapped = match instance {
                ConnectiveInstance::VarRefBody(value) => {
                    if value.depth == depth {
                        let position = env
                            .level
                            .checked_add(env.shift)
                            .and_then(|n| n.checked_sub(value.index))
                            .and_then(|n| n.checked_sub(1));
                        if position.is_some() {
                            if let Some(replacement) = env
                                .get_metered(&value.index, &cleanup_meter(backing))
                                .map_err(rejected)?
                            {
                                result = concat_metered(replacement, result, backing)?;
                                continue;
                            }
                        }
                    }
                    ConnectiveInstance::VarRefBody(value)
                }
                ConnectiveInstance::ConnAndBody(ConnectiveBody { ps }) => {
                    ConnectiveInstance::ConnAndBody(ConnectiveBody {
                        ps: collect_metered(ps.into_iter(), backing, |par| {
                            self.substitute_par_metered(par, depth, env, backing)
                        })?,
                    })
                }
                ConnectiveInstance::ConnOrBody(ConnectiveBody { ps }) => {
                    ConnectiveInstance::ConnOrBody(ConnectiveBody {
                        ps: collect_metered(ps.into_iter(), backing, |par| {
                            self.substitute_par_metered(par, depth, env, backing)
                        })?,
                    })
                }
                ConnectiveInstance::ConnNotBody(par) => ConnectiveInstance::ConnNotBody(
                    self.substitute_par_metered(par, depth, env, backing)?,
                ),
                other => other,
            };
            result = prepend_connective_metered(
                result,
                Connective {
                    connective_instance: Some(mapped),
                },
                depth,
                backing,
            )?;
        }
        Ok(result)
    }

    fn sub_send_metered(
        &self,
        term: Send,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Send, InterpreterError> {
        let channel = required(term.chan, backing)?;
        let channel = self.substitute_par_metered(channel, depth, env, backing)?;
        let data = collect_metered(term.data.into_iter(), backing, |par| {
            self.substitute_par_metered(par, depth, env, backing)
        })?;
        Ok(Send {
            chan: Some(channel),
            data,
            persistent: term.persistent,
            locally_free: set_bits_until_metered(term.locally_free, env.shift, backing)?,
            connective_used: term.connective_used,
        })
    }

    fn sub_bundle_metered(
        &self,
        term: Bundle,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Bundle, InterpreterError> {
        let body = required(term.body, backing)?;
        let mut body = self.substitute_par_metered(body, depth, env, backing)?;
        if body.sends.is_empty()
            && body.receives.is_empty()
            && body.news.is_empty()
            && body.exprs.is_empty()
            && body.matches.is_empty()
            && body.unforgeables.is_empty()
            && body.connectives.is_empty()
            && body.bundles.len() == 1
        {
            let mut inner = body
                .bundles
                .pop()
                .ok_or(InterpreterError::HostWorkRejected)?;
            inner.read_flag &= term.read_flag;
            inner.write_flag &= term.write_flag;
            Ok(inner)
        } else {
            Ok(Bundle {
                body: Some(body),
                ..term
            })
        }
    }

    fn sub_receive_metered(
        &self,
        term: Receive,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Receive, InterpreterError> {
        let pattern_depth = depth
            .checked_add(1)
            .ok_or(InterpreterError::HostWorkRejected)?;
        let binds = collect_metered(term.binds.into_iter(), backing, |bind: ReceiveBind| {
            let source = required(bind.source, backing)?;
            let source = self.substitute_par_metered(source, depth, env, backing)?;
            let patterns = collect_metered(bind.patterns.into_iter(), backing, |par| {
                self.substitute_par_metered(par, pattern_depth, env, backing)
            })?;
            let cost_signature = bind
                .cost_signature
                .map(|signature| {
                    substitute_cost_signature_metered_with(
                        self,
                        signature,
                        depth,
                        env,
                        backing,
                        &Substitute::substitute_par_metered,
                    )
                })
                .transpose()?;
            Ok(ReceiveBind {
                patterns,
                source: Some(source),
                remainder: bind.remainder,
                free_count: bind.free_count,
                cost_signature,
            })
        })?;
        let shifted = shifted_env(env, term.bind_count, backing)?;
        let body = required(term.body, backing)?;
        let body = self.substitute_par_metered(body, depth, &shifted, backing)?;
        let condition = term
            .condition
            .map(|par| self.substitute_par_metered(par, depth, &shifted, backing))
            .transpose()?;
        Ok(Receive {
            binds,
            body: Some(body),
            persistent: term.persistent,
            peek: term.peek,
            bind_count: term.bind_count,
            locally_free: set_bits_until_metered(term.locally_free, env.shift, backing)?,
            connective_used: term.connective_used,
            condition,
        })
    }

    fn sub_new_metered(
        &self,
        term: New,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<New, InterpreterError> {
        let shifted = shifted_env(env, term.bind_count, backing)?;
        let body = required(term.p, backing)?;
        let body = self.substitute_par_metered(body, depth, &shifted, backing)?;
        Ok(New {
            bind_count: term.bind_count,
            p: Some(body),
            uri: term.uri,
            injections: term.injections,
            locally_free: set_bits_until_metered(term.locally_free, env.shift, backing)?,
        })
    }

    fn sub_match_metered(
        &self,
        term: Match,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Match, InterpreterError> {
        let target = required(term.target, backing)?;
        let target = self.substitute_par_metered(target, depth, env, backing)?;
        let cases = collect_metered(
            term.cases
                .into_iter()
                .filter(|case| case.pattern.is_some() && case.source.is_some()),
            backing,
            |case: MatchCase| {
                let shifted = shifted_env(env, case.free_count, backing)?;
                let source = self.substitute_par_metered(
                    case.source.ok_or(InterpreterError::HostWorkRejected)?,
                    depth,
                    &shifted,
                    backing,
                )?;
                let pattern_depth = depth
                    .checked_add(1)
                    .ok_or(InterpreterError::HostWorkRejected)?;
                let pattern = self.substitute_par_metered(
                    case.pattern.ok_or(InterpreterError::HostWorkRejected)?,
                    pattern_depth,
                    env,
                    backing,
                )?;
                let guard = case
                    .guard
                    .map(|par| self.substitute_par_metered(par, depth, &shifted, backing))
                    .transpose()?;
                Ok(MatchCase {
                    pattern: Some(pattern),
                    source: Some(source),
                    free_count: case.free_count,
                    guard,
                })
            },
        )?;
        Ok(Match {
            target: Some(target),
            cases,
            locally_free: set_bits_until_metered(term.locally_free, env.shift, backing)?,
            connective_used: term.connective_used,
        })
    }

    fn sub_if_metered(
        &self,
        term: If,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<If, InterpreterError> {
        let condition =
            self.substitute_par_metered(required(term.condition, backing)?, depth, env, backing)?;
        let if_true =
            self.substitute_par_metered(required(term.if_true, backing)?, depth, env, backing)?;
        let if_false =
            self.substitute_par_metered(required(term.if_false, backing)?, depth, env, backing)?;
        Ok(If {
            condition: Some(condition),
            if_true: Some(if_true),
            if_false: Some(if_false),
            locally_free: set_bits_until_metered(term.locally_free, env.shift, backing)?,
            connective_used: term.connective_used,
        })
    }

    fn sub_cost_signed_metered(
        &self,
        term: CostSignedTerm,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<CostSignedTerm, InterpreterError> {
        let body =
            self.substitute_par_metered(required(term.body, backing)?, depth, env, backing)?;
        let signature = substitute_cost_signature_metered_with(
            self,
            required(term.signature, backing)?,
            depth,
            env,
            backing,
            &Substitute::substitute_par_metered,
        )?;
        Ok(CostSignedTerm {
            body: Some(body),
            signature: Some(signature),
        })
    }

    fn sub_cost_stack_metered(
        &self,
        term: CostStack,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<CostStack, InterpreterError> {
        let cells = collect_metered(term.cells.into_iter(), backing, |signature| {
            substitute_cost_signature_metered_with(
                self,
                signature,
                depth,
                env,
                backing,
                &Substitute::substitute_par_metered,
            )
        })?;
        Ok(CostStack { cells })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::{CostSignature, EList, VarRef};
    use models::rust::utils::{new_boundvar_par, new_gint_par};

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::metering::MeteredMachine;
    use crate::rust::interpreter::substitute::SubstituteTrait;

    fn instance() -> Substitute {
        Substitute {
            metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
        }
    }

    fn nested_term() -> Par {
        let bound = new_boundvar_par(0, Vec::new(), false);
        let ground = new_gint_par(7, Vec::new(), false);
        let send = Send {
            chan: Some(bound.clone()),
            data: vec![ground.clone(), bound.clone()],
            ..Send::default()
        };
        let bind = ReceiveBind {
            patterns: vec![bound.clone()],
            source: Some(ground.clone()),
            ..ReceiveBind::default()
        };
        let receive = Receive {
            binds: vec![bind],
            body: Some(bound.clone()),
            condition: Some(ground.clone()),
            ..Receive::default()
        };
        let new = New {
            p: Some(bound.clone()),
            ..New::default()
        };
        let case = MatchCase {
            pattern: Some(bound.clone()),
            source: Some(ground.clone()),
            guard: Some(bound.clone()),
            ..MatchCase::default()
        };
        let matching = Match {
            target: Some(bound.clone()),
            cases: vec![case],
            ..Match::default()
        };
        let conditional = If {
            condition: Some(ground.clone()),
            if_true: Some(bound.clone()),
            if_false: Some(ground.clone()),
            ..If::default()
        };
        let bundle = Bundle {
            body: Some(ground.clone()),
            ..Bundle::default()
        };
        let list = Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![bound.clone(), ground],
                ..EList::default()
            })),
        };
        let connective = Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![bound],
            })),
        };
        let stack = CostStack {
            cells: vec![CostSignature {
                value: Some(CostSignatureValue::Ground(vec![2, 3])),
            }],
        };
        Par {
            sends: vec![send],
            receives: vec![receive],
            news: vec![new],
            matches: vec![matching],
            bundles: vec![bundle],
            conditionals: vec![conditional],
            exprs: vec![list],
            connectives: vec![connective],
            cost_stacks: vec![stack],
            ..Par::default()
        }
    }

    #[test]
    fn metered_par_substitution_matches_nested_legacy_path_and_rejects_at_cuts() {
        let substitute = instance();
        let term = nested_term();
        let mut env = Env::new();
        env.push(new_gint_par(42, Vec::new(), false)).unwrap();
        let expected = substitute
            .substitute_no_sort(term.clone(), 0, &env)
            .unwrap();
        let calls = Cell::new(0_usize);
        let unlimited = |_: usize, _: usize, _: usize| {
            calls.set(calls.get() + 1);
            Ok(())
        };
        let actual = substitute
            .substitute_par_metered(term.clone(), 0, &env, &unlimited)
            .unwrap();
        assert_eq!(actual, expected);
        assert!(calls.get() > 10);
        for cut in [0, 1, calls.get() / 2, calls.get() - 1] {
            let seen = Cell::new(0_usize);
            let reject = |_: usize, _: usize, _: usize| {
                let current = seen.get();
                seen.set(current + 1);
                if current == cut {
                    Err(BackingError::Rejected)
                } else {
                    Ok(())
                }
            };
            assert!(matches!(
                substitute.substitute_par_metered(term.clone(), 0, &env, &reject),
                Err(InterpreterError::HostWorkRejected)
            ));
            assert_eq!(term, nested_term());
        }
    }

    #[test]
    fn connective_bitvector_allocation_is_reserved_before_construction() {
        let substitute = instance();
        let term = Par {
            connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
                    index: 1024,
                    depth: 0,
                })),
            }],
            ..Par::default()
        };
        let env = Env::new();
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        let actual = substitute
            .substitute_par_metered(term.clone(), 0, &env, &unlimited)
            .unwrap();
        let expected = substitute
            .substitute_no_sort(term.clone(), 0, &env)
            .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual.locally_free.len(), 1025);

        let rejected = Cell::new(false);
        let reject_bitvector = |_: usize, _: usize, backing: usize| {
            if backing == 1025 {
                rejected.set(true);
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert!(matches!(
            substitute.substitute_par_metered(term, 0, &env, &reject_bitvector),
            Err(InterpreterError::HostWorkRejected)
        ));
        assert!(rejected.get());
    }
}
