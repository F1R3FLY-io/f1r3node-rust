use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::{CostSignature, CostSignatureCompound, Par};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature_metered;
use models::rust::rholang::sorter::metered::SorterMeter;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use shared::rust::clone_backing::{BackingError, BackingMeter};

use super::{Env, InterpreterError, Substitute};

pub(crate) fn substitute_cost_signature_metered_with<F>(
    substitute: &Substitute,
    signature: CostSignature,
    depth: i32,
    env: &Env<Par>,
    backing: &dyn BackingMeter,
    substitute_par: &F,
) -> Result<CostSignature, InterpreterError>
where
    F: Fn(&Substitute, Par, i32, &Env<Par>, &dyn BackingMeter) -> Result<Par, InterpreterError>,
{
    let cleanup = |operations: usize, scanned: usize, bytes: usize| {
        backing.reserve(
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            scanned,
            bytes,
        )
    };
    let meter = SorterMeter::new(&cleanup);
    substitute_cost_signature_inner(substitute, signature, depth, env, &meter, substitute_par)
}

fn substitute_cost_signature_inner<F>(
    substitute: &Substitute,
    signature: CostSignature,
    depth: i32,
    env: &Env<Par>,
    meter: &SorterMeter<'_>,
    substitute_par: &F,
) -> Result<CostSignature, InterpreterError>
where
    F: Fn(&Substitute, Par, i32, &Env<Par>, &dyn BackingMeter) -> Result<Par, InterpreterError>,
{
    let _depth = meter.enter().map_err(host_rejected)?;
    meter
        .reserve(1, std::mem::size_of::<CostSignature>(), 0)
        .map_err(host_rejected)?;
    let value = match signature.value {
        Some(CostSignatureValue::Unit(value)) => CostSignatureValue::Unit(value),
        Some(CostSignatureValue::Ground(bytes)) => CostSignatureValue::Ground(bytes),
        Some(CostSignatureValue::BoundLevel(index)) if depth == 0 => {
            let position = env
                .level
                .checked_add(env.shift)
                .and_then(|level| level.checked_sub(index))
                .and_then(|level| level.checked_sub(1));
            if let Some(position) = position {
                let probes = env.env_map.capacity().max(1);
                let scanned = probes
                    .checked_mul(std::mem::size_of::<i32>())
                    .ok_or(InterpreterError::HostWorkRejected)?;
                meter.reserve(probes, scanned, 0).map_err(host_rejected)?;
                // Changed by D-O1 (DR-110): the name is no longer inspected here.
                // if let Some(name) = env.env_map.get(&position) {
                if let Some(_name) = env.env_map.get(&position) {
                    // Disabled by D-O1 (DR-110): redundant. `get_metered` below
                    // prepays the copy of the name and its release, and the sort
                    // charges its own reads (the precedent of DR-94, decision 4).
                    // meter.inspect(name).map_err(host_rejected)?;
                    let name = env
                        .get_metered(&index, meter)
                        .map_err(host_rejected)?
                        .ok_or(InterpreterError::HostWorkRejected)?;
                    let sorted =
                        ParSortMatcher::sort_match_metered(&name, meter).map_err(host_rejected)?;
                    CostSignatureValue::Name(sorted.term)
                } else {
                    CostSignatureValue::BoundLevel(index)
                }
            } else {
                CostSignatureValue::BoundLevel(index)
            }
        }
        Some(CostSignatureValue::BoundLevel(index)) => CostSignatureValue::BoundLevel(index),
        Some(CostSignatureValue::Quote(par)) => {
            let par = substitute_par(substitute, par, depth, env, meter)?;
            let sorted = ParSortMatcher::sort_match_metered(&par, meter).map_err(host_rejected)?;
            CostSignatureValue::Quote(sorted.term)
        }
        Some(CostSignatureValue::Name(par)) => {
            let par = substitute_par(substitute, par, depth, env, meter)?;
            let sorted = ParSortMatcher::sort_match_metered(&par, meter).map_err(host_rejected)?;
            CostSignatureValue::Name(sorted.term)
        }
        Some(CostSignatureValue::Compound(compound)) => {
            let mut elements = meter
                .vec::<CostSignature>(compound.elements.len())
                .map_err(host_rejected)?;
            for element in compound.elements {
                elements.push(substitute_cost_signature_inner(
                    substitute,
                    element,
                    depth,
                    env,
                    meter,
                    substitute_par,
                )?);
            }
            CostSignatureValue::Compound(CostSignatureCompound { elements })
        }
        None => {
            return Err(InterpreterError::UndefinedRequiredProtobufFieldError(
                "CostSignature.value".to_string(),
            ));
        }
    };
    let value = CostSignature { value: Some(value) };
    // Changed by D-O1 (DR-110): the owned `value` is dropped after the sort,
    // which charges its own reads. A block inspection prepays that release.
    // meter.inspect(&value).map_err(host_rejected)?;
    meter.inspect_blocks(&value).map_err(host_rejected)?;
    sort_signature_metered(&value, meter)
        .map(|sorted| sorted.term)
        .map_err(host_rejected)
}

fn host_rejected(_: BackingError) -> InterpreterError { InterpreterError::HostWorkRejected }

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::{CostSignature, CostSignatureCompound};
    use shared::rust::clone_backing::BackingError;

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

    fn legacy_par(
        substitute: &Substitute,
        par: Par,
        depth: i32,
        env: &Env<Par>,
        _: &dyn BackingMeter,
    ) -> Result<Par, InterpreterError> {
        substitute.substitute_no_sort(par, depth, env)
    }

    fn identity_par(
        _: &Substitute,
        par: Par,
        _: i32,
        _: &Env<Par>,
        _: &dyn BackingMeter,
    ) -> Result<Par, InterpreterError> {
        Ok(par)
    }

    #[test]
    fn metered_outer_cost_signature_substitution_matches_legacy() {
        let substitute = substitute();
        let mut env = Env::<Par>::new();
        let env = env.put(Par::default());
        let values = [
            CostSignature {
                value: Some(Value::Unit(false)),
            },
            CostSignature {
                value: Some(Value::Ground(vec![1, 2, 3])),
            },
            CostSignature {
                value: Some(Value::BoundLevel(0)),
            },
            CostSignature {
                value: Some(Value::BoundLevel(8)),
            },
            CostSignature {
                value: Some(Value::Quote(Par::default())),
            },
            CostSignature {
                value: Some(Value::Name(Par::default())),
            },
            CostSignature {
                value: Some(Value::Compound(CostSignatureCompound {
                    elements: vec![
                        CostSignature {
                            value: Some(Value::Ground(vec![9])),
                        },
                        CostSignature {
                            value: Some(Value::Ground(vec![4])),
                        },
                        CostSignature {
                            value: Some(Value::Unit(true)),
                        },
                    ],
                })),
            },
        ];
        let full = |_: usize, _: usize, _: usize| Ok(());
        for value in values {
            assert_eq!(
                substitute_cost_signature_metered_with(
                    &substitute,
                    value.clone(),
                    0,
                    &env,
                    &full,
                    &legacy_par,
                )
                .unwrap(),
                substitute
                    .substitute_cost_signature(value, 0, &env)
                    .unwrap()
            );
        }
    }

    #[test]
    fn metered_outer_cost_signature_substitution_rejects_before_copy() {
        let substitute = substitute();
        let env = Env::<Par>::new();
        let signature = CostSignature {
            value: Some(Value::Ground(vec![3; 4096])),
        };
        let reject = |_: usize, _: usize, bytes: usize| {
            if bytes > 1024 {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert!(matches!(
            substitute_cost_signature_metered_with(
                &substitute,
                signature,
                0,
                &env,
                &reject,
                &identity_par,
            ),
            Err(InterpreterError::HostWorkRejected)
        ));
    }
}
