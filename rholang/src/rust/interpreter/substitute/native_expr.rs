use std::mem::size_of;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set::ParSet;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::sorter::metered::SorterMeter;
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::sorted_par_map::SortedParMap;
use shared::rust::clone_backing::{self, BackingError, BackingMeter};

use super::Substitute;
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::unwrap_option_safe;

fn rejected(_: BackingError) -> InterpreterError { InterpreterError::HostWorkRejected }

struct OwnedMeter<'a>(&'a dyn BackingMeter);

impl BackingMeter for OwnedMeter<'_> {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError> {
        self.0.reserve(
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            scanned,
            backing,
        )
    }
}

fn set_bits_until_metered(
    bits: Vec<u8>,
    shift: i32,
    backing: &dyn BackingMeter,
) -> Result<Vec<u8>, InterpreterError> {
    let count = bits.len().min(shift.max(0) as usize);
    let owned = OwnedMeter(backing);
    owned
        .reserve(
            count
                .checked_add(1)
                .ok_or(InterpreterError::HostWorkRejected)?,
            count,
            count,
        )
        .map_err(rejected)?;
    let mut truncated = Vec::new();
    truncated
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    truncated.extend(bits.into_iter().take(count));
    Ok(truncated)
}

impl Substitute {
    fn substitute_pair_metered(
        &self,
        first: Option<Par>,
        second: Option<Par>,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<(Option<Par>, Option<Par>), InterpreterError> {
        let first = self.substitute_par_metered(unwrap_option_safe(first)?, depth, env, backing)?;
        let second =
            self.substitute_par_metered(unwrap_option_safe(second)?, depth, env, backing)?;
        Ok((Some(first), Some(second)))
    }

    fn substitute_many_metered(
        &self,
        input: Vec<Par>,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Vec<Par>, InterpreterError> {
        let count = input.len();
        let scanned = count
            .checked_mul(size_of::<Par>())
            .ok_or(InterpreterError::HostWorkRejected)?;
        let owned = OwnedMeter(backing);
        let meter = SorterMeter::new(&owned);
        meter.reserve(count, scanned, 0).map_err(rejected)?;
        let mut output = meter.vec(count).map_err(rejected)?;
        for par in input {
            output.push(self.substitute_par_metered(par, depth, env, backing)?);
        }
        Ok(output)
    }

    fn substitute_entries_metered(
        &self,
        input: Vec<(Par, Par)>,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Vec<(Par, Par)>, InterpreterError> {
        let count = input.len();
        let scanned = count
            .checked_mul(size_of::<(Par, Par)>())
            .ok_or(InterpreterError::HostWorkRejected)?;
        let owned = OwnedMeter(backing);
        let meter = SorterMeter::new(&owned);
        meter
            .reserve(
                count
                    .checked_mul(2)
                    .ok_or(InterpreterError::HostWorkRejected)?,
                scanned,
                0,
            )
            .map_err(rejected)?;
        let mut output = meter.vec(count).map_err(rejected)?;
        for (key, value) in input {
            let key = self.substitute_par_metered(key, depth, env, backing)?;
            let value = self.substitute_par_metered(value, depth, env, backing)?;
            output.push((key, value));
        }
        Ok(output)
    }

    pub fn substitute_expr_metered(
        &self,
        term: Expr,
        depth: i32,
        env: &Env<Par>,
        backing: &dyn BackingMeter,
    ) -> Result<Expr, InterpreterError> {
        let owned = OwnedMeter(backing);
        clone_backing::reserve(&term, &owned).map_err(rejected)?;
        let instance = unwrap_option_safe(term.expr_instance)?;
        let instance = match instance {
            ExprInstance::ENotBody(mut op) => {
                op.p = Some(self.substitute_par_metered(
                    unwrap_option_safe(op.p)?,
                    depth,
                    env,
                    backing,
                )?);
                ExprInstance::ENotBody(op)
            }
            ExprInstance::ENegBody(mut op) => {
                op.p = Some(self.substitute_par_metered(
                    unwrap_option_safe(op.p)?,
                    depth,
                    env,
                    backing,
                )?);
                ExprInstance::ENegBody(op)
            }
            ExprInstance::EMultBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EMultBody(op)
            }
            ExprInstance::EDivBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EDivBody(op)
            }
            ExprInstance::EModBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EModBody(op)
            }
            ExprInstance::EPercentPercentBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EPercentPercentBody(op)
            }
            ExprInstance::EPlusBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EPlusBody(op)
            }
            ExprInstance::EMinusBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EMinusBody(op)
            }
            ExprInstance::EPlusPlusBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EPlusPlusBody(op)
            }
            ExprInstance::EMinusMinusBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EMinusMinusBody(op)
            }
            ExprInstance::ELtBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::ELtBody(op)
            }
            ExprInstance::ELteBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::ELteBody(op)
            }
            ExprInstance::EGtBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EGtBody(op)
            }
            ExprInstance::EGteBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EGteBody(op)
            }
            ExprInstance::EEqBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EEqBody(op)
            }
            ExprInstance::ENeqBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::ENeqBody(op)
            }
            ExprInstance::EAndBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EAndBody(op)
            }
            ExprInstance::EOrBody(mut op) => {
                (op.p1, op.p2) = self.substitute_pair_metered(op.p1, op.p2, depth, env, backing)?;
                ExprInstance::EOrBody(op)
            }
            ExprInstance::EMatchesBody(mut op) => {
                (op.target, op.pattern) =
                    self.substitute_pair_metered(op.target, op.pattern, depth, env, backing)?;
                ExprInstance::EMatchesBody(op)
            }
            ExprInstance::EListBody(mut list) => {
                list.ps = self.substitute_many_metered(list.ps, depth, env, backing)?;
                list.locally_free = set_bits_until_metered(list.locally_free, env.shift, backing)?;
                ExprInstance::EListBody(list)
            }
            ExprInstance::ETupleBody(mut tuple) => {
                tuple.ps = self.substitute_many_metered(tuple.ps, depth, env, backing)?;
                tuple.locally_free =
                    set_bits_until_metered(tuple.locally_free, env.shift, backing)?;
                ExprInstance::ETupleBody(tuple)
            }
            ExprInstance::ESetBody(set) => {
                let par_set =
                    ParSetTypeMapper::eset_to_par_set_metered(set, &owned).map_err(rejected)?;
                let ParSet {
                    ps,
                    connective_used,
                    locally_free,
                    remainder,
                } = par_set;
                let substituted =
                    self.substitute_many_metered(ps.sorted_pars, depth, env, backing)?;
                let sorted = SortedParHashSet::create_from_vec_metered(substituted, &owned)
                    .map_err(rejected)?;
                let set = ParSetTypeMapper::par_set_to_eset_prepaid(ParSet {
                    ps: sorted,
                    connective_used,
                    locally_free: set_bits_until_metered(locally_free, env.shift, backing)?,
                    remainder,
                });
                ExprInstance::ESetBody(set)
            }
            ExprInstance::EMapBody(map) => {
                let par_map =
                    ParMapTypeMapper::emap_to_par_map_metered(map, &owned).map_err(rejected)?;
                let ParMap {
                    ps,
                    connective_used,
                    locally_free,
                    remainder,
                } = par_map;
                let substituted =
                    self.substitute_entries_metered(ps.sorted_list, depth, env, backing)?;
                let sorted =
                    SortedParMap::create_from_vec_metered(substituted, &owned).map_err(rejected)?;
                let map = ParMapTypeMapper::par_map_to_emap_metered(
                    ParMap {
                        ps: sorted,
                        connective_used,
                        locally_free: set_bits_until_metered(locally_free, env.shift, backing)?,
                        remainder,
                    },
                    &owned,
                )
                .map_err(rejected)?;
                ExprInstance::EMapBody(map)
            }
            ExprInstance::EMethodBody(mut method) => {
                method.target = Some(self.substitute_par_metered(
                    unwrap_option_safe(method.target)?,
                    depth,
                    env,
                    backing,
                )?);
                method.arguments =
                    self.substitute_many_metered(method.arguments, depth, env, backing)?;
                method.locally_free =
                    set_bits_until_metered(method.locally_free, env.shift, backing)?;
                ExprInstance::EMethodBody(method)
            }
            other => other,
        };
        Ok(Expr {
            expr_instance: Some(instance),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use models::rhoapi::{EList, EMap, EMinus, ESet, KeyValuePair};
    use models::rust::utils::new_gint_par;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::metering::MeteredMachine;
    use crate::rust::interpreter::substitute::SubstituteTrait;

    fn substitute() -> Substitute {
        Substitute {
            metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
        }
    }

    fn par(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

    #[test]
    fn metered_expr_substitution_matches_legacy_no_sort_for_nested_collections() {
        let env: Env<Par> = Env::new();
        let nested = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMinusBody(EMinus {
                    p1: Some(par(9)),
                    p2: Some(par(4)),
                })),
            }],
            ..Par::default()
        };
        let expressions = [
            Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![nested.clone(), par(1)],
                    locally_free: vec![1, 2, 3],
                    connective_used: false,
                    remainder: None,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::ESetBody(ESet {
                    ps: vec![nested.clone(), nested.clone(), par(2)],
                    locally_free: vec![1, 2, 3],
                    connective_used: false,
                    remainder: None,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: vec![KeyValuePair {
                        key: Some(par(3)),
                        value: Some(nested),
                    }],
                    locally_free: vec![1, 2, 3],
                    connective_used: false,
                    remainder: None,
                })),
            },
        ];
        let substitute = substitute();
        let meter = |_: usize, _: usize, _: usize| Ok::<(), BackingError>(());
        for expr in expressions {
            let expected = substitute
                .substitute_no_sort(expr.clone(), 0, &env)
                .unwrap();
            let actual = substitute
                .substitute_expr_metered(expr, 0, &env, &meter)
                .unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn metered_expr_substitution_rejects_late_collection_work() {
        let env: Env<Par> = Env::new();
        let expr = Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![par(3), par(1), par(2)],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let substitute = substitute();
        let calls = Cell::new(0usize);
        let count = |_: usize, _: usize, _: usize| {
            calls.set(calls.get() + 1);
            Ok::<(), BackingError>(())
        };
        substitute
            .substitute_expr_metered(expr.clone(), 0, &env, &count)
            .unwrap();
        let last = calls.get();
        assert!(last > 1);
        let seen = Cell::new(0usize);
        let reject = |_: usize, _: usize, _: usize| {
            seen.set(seen.get() + 1);
            if seen.get() == last {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            substitute.substitute_expr_metered(expr, 0, &env, &reject),
            Err(InterpreterError::HostWorkRejected)
        );
    }
}
