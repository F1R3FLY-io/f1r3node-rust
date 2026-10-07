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
    mut signature: CostSignature,
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
    let value = match signature.value.take() {
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
                if let Some(name) = env.env_map.get(&position) {
                    meter.inspect(name).map_err(host_rejected)?;
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
    meter.inspect(&value).map_err(host_rejected)?;
    sort_signature_metered(&value, meter)
        .map(|sorted| sorted.term)
        .map_err(host_rejected)
}

fn host_rejected(_: BackingError) -> InterpreterError { InterpreterError::HostWorkRejected }

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{
        Connective, CostSignature, CostSignatureCompound, EMatches, Expr, KeyValuePair, VarRef,
    };
    use models::rust::utils::{
        new_boundvar_par, new_elist_par, new_emap_par, new_eset_par, new_gint_par,
    };
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
    fn signature_worklist_preserves_source_reservation_trace_on_non_par_shapes() {
        let substitute = substitute();
        let mut empty = Env::<Par>::new();
        let bound_env = empty.put(Par::default());
        let shapes = vec![
            (
                CostSignature {
                    value: Some(Value::Unit(false)),
                },
                &bound_env,
            ),
            (
                CostSignature {
                    value: Some(Value::Ground(vec![1, 2, 3])),
                },
                &bound_env,
            ),
            (
                CostSignature {
                    value: Some(Value::BoundLevel(0)),
                },
                &bound_env,
            ),
            (
                CostSignature {
                    value: Some(Value::BoundLevel(8)),
                },
                &bound_env,
            ),
            (
                CostSignature {
                    value: Some(Value::Compound(CostSignatureCompound {
                        elements: vec![
                            CostSignature {
                                value: Some(Value::Ground(vec![9])),
                            },
                            CostSignature {
                                value: Some(Value::Unit(true)),
                            },
                            CostSignature {
                                value: Some(Value::Ground(vec![4])),
                            },
                        ],
                    })),
                },
                &bound_env,
            ),
        ];
        for (shape, env) in shapes {
            let shape_for_prefix = shape.clone();
            let source_trace = RefCell::new(Vec::new());
            let source_meter = |operations, scanned, backing| {
                source_trace
                    .borrow_mut()
                    .push((operations, scanned, backing));
                Ok(())
            };
            let source = substitute_cost_signature_metered_with(
                &substitute,
                shape.clone(),
                0,
                env,
                &source_meter,
                &identity_par,
            )
            .unwrap();
            let driver_trace = RefCell::new(Vec::new());
            let driver_meter = |operations, scanned, backing| {
                driver_trace
                    .borrow_mut()
                    .push((operations, scanned, backing));
                Ok(())
            };
            let driver = substitute
                .substitute_cost_signature_metered(shape, 0, env, &driver_meter)
                .unwrap();
            assert_eq!(driver, source);
            assert_eq!(*driver_trace.borrow(), *source_trace.borrow());
            let full = source_trace.into_inner();
            for allowed in [0, full.len() / 2, full.len().saturating_sub(1)] {
                let source_prefix = RefCell::new(Vec::new());
                let source_reject = |operations, scanned, backing| {
                    if source_prefix.borrow().len() == allowed {
                        return Err(BackingError::Rejected);
                    }
                    source_prefix
                        .borrow_mut()
                        .push((operations, scanned, backing));
                    Ok(())
                };
                let source_result = substitute_cost_signature_metered_with(
                    &substitute,
                    shape_for_prefix.clone(),
                    0,
                    env,
                    &source_reject,
                    &identity_par,
                );
                let driver_prefix = RefCell::new(Vec::new());
                let driver_reject = |operations, scanned, backing| {
                    if driver_prefix.borrow().len() == allowed {
                        return Err(BackingError::Rejected);
                    }
                    driver_prefix
                        .borrow_mut()
                        .push((operations, scanned, backing));
                    Ok(())
                };
                let driver_result = substitute.substitute_cost_signature_metered(
                    shape_for_prefix.clone(),
                    0,
                    env,
                    &driver_reject,
                );
                assert!(matches!(
                    source_result,
                    Err(InterpreterError::HostWorkRejected)
                ));
                assert!(matches!(
                    driver_result,
                    Err(InterpreterError::HostWorkRejected)
                ));
                assert_eq!(*source_prefix.borrow(), full[..allowed]);
                assert_eq!(*driver_prefix.borrow(), *source_prefix.borrow());
            }
        }
    }

    #[test]
    fn signature_worklist_preserves_source_reservation_trace_on_shallow_quote_and_name() {
        let substitute = substitute();
        let env = Env::<Par>::new();
        let values = vec![
            Value::Quote(Par::default()),
            Value::Name(Par::default()),
            Value::Quote(new_gint_par(7, Vec::new(), false)),
            Value::Quote(new_elist_par(
                vec![
                    new_gint_par(2, Vec::new(), false),
                    new_gint_par(1, Vec::new(), false),
                ],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            )),
            Value::Quote(new_eset_par(
                vec![
                    new_gint_par(2, Vec::new(), false),
                    new_gint_par(1, Vec::new(), false),
                ],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            )),
            Value::Quote(new_emap_par(
                vec![KeyValuePair {
                    key: Some(new_gint_par(1, Vec::new(), false)),
                    value: Some(new_gint_par(2, Vec::new(), false)),
                }],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            )),
        ];
        for value in values {
            let source_trace = RefCell::new(Vec::new());
            let source_meter = |operations, scanned, backing| {
                source_trace
                    .borrow_mut()
                    .push((operations, scanned, backing));
                Ok(())
            };
            let source = substitute_cost_signature_metered_with(
                &substitute,
                CostSignature {
                    value: Some(value.clone()),
                },
                0,
                &env,
                &source_meter,
                &Substitute::substitute_par_metered,
            )
            .unwrap();
            let driver_trace = RefCell::new(Vec::new());
            let driver_meter = |operations, scanned, backing| {
                driver_trace
                    .borrow_mut()
                    .push((operations, scanned, backing));
                Ok(())
            };
            let driver = substitute
                .substitute_cost_signature_metered(
                    CostSignature { value: Some(value) },
                    0,
                    &env,
                    &driver_meter,
                )
                .unwrap();
            assert_eq!(driver, source);
            assert_eq!(*driver_trace.borrow(), *source_trace.borrow());
        }
    }

    /// The source native helper predates the target frontend's proved
    /// `EMatches` pattern scope. Its same-depth operand traversal is an
    /// intentional behavioral *exception* to the shallow source oracle:
    /// MeTTaIL/target normalization places the pattern one binder level deeper
    /// (`substitute_combine::expr_arm_pattern_slots`, `substitute_test`).
    #[test]
    fn metered_quote_keeps_target_ematches_pattern_binder_depth() {
        let substitute = substitute();
        let pattern = models::par_from_default! {
            connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
                    index: 1,
                    depth: 2,
                })),
            }],
            ..Default::default()
        };
        let body = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMatchesBody(EMatches {
                    target: Some(new_boundvar_par(0, Vec::new(), false)),
                    pattern: Some(pattern),
                })),
            }],
            ..Default::default()
        };
        let mut env = Env::<Par>::new();
        let env = env.put(new_gint_par(42, Vec::new(), false)).shift(1);
        let signature = CostSignature {
            value: Some(Value::Quote(body)),
        };
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        let expected = substitute
            .substitute_cost_signature(signature.clone(), 1, &env)
            .unwrap();
        let source = substitute_cost_signature_metered_with(
            &substitute,
            signature.clone(),
            1,
            &env,
            &unlimited,
            &Substitute::substitute_par_metered,
        )
        .unwrap();
        let actual = substitute
            .substitute_cost_signature_metered(signature, 1, &env, &unlimited)
            .unwrap();
        assert_eq!(actual, expected);
        assert_ne!(
            actual, source,
            "source same-depth EMatches is the known scope exception"
        );
    }

    /// The source native helper leaves EPathMap opaque, but the target's
    /// recursive oracle proves entry substitution is consensus-visible.
    #[test]
    fn metered_quote_keeps_target_epathmap_entry_substitution() {
        use models::rust::canonical_path::encode_trie_path;
        use models::rust::rhoapi_ext::EPathMap;

        let substitute = substitute();
        let original_key = new_boundvar_par(0, Vec::new(), false);
        let original_path_bytes = encode_trie_path(&original_key).len();
        let body = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new(
                    vec![original_key],
                    Vec::new(),
                    false,
                    None,
                ))),
            }],
            ..Default::default()
        };
        let mut env = Env::<Par>::new();
        let env = env.put(new_gint_par(42, Vec::new(), false));
        let signature = CostSignature {
            value: Some(Value::Quote(body)),
        };
        let expected = substitute
            .substitute_cost_signature(signature.clone(), 0, &env)
            .unwrap();
        let source_unlimited = |_: usize, _: usize, _: usize| Ok(());
        let source = substitute_cost_signature_metered_with(
            &substitute,
            signature.clone(),
            0,
            &env,
            &source_unlimited,
            &Substitute::substitute_par_metered,
        )
        .unwrap();
        let trace = RefCell::new(Vec::new());
        let unlimited = |operations, scanned, backing| {
            trace.borrow_mut().push((operations, scanned, backing));
            Ok(())
        };
        let actual = substitute
            .substitute_cost_signature_metered(signature, 0, &env, &unlimited)
            .unwrap();
        assert_eq!(actual, expected);
        assert_ne!(actual, source, "source leaves EPathMap entries opaque");
        assert_eq!(substitute.metering.budget().total_cost().value, 0);

        // Source's native helper leaves EPathMap opaque. Target semantics
        // substitute its entries, so the extra host-work policy explicitly
        // prices each canonical key decode and trie insertion; these events
        // never pass through the consensus BillableKind ledger.
        let decode_backing =
            original_path_bytes * (std::mem::size_of::<Par>() + std::mem::size_of::<Vec<Par>>());
        let decode_event = (
            2 * (original_path_bytes + 1),
            original_path_bytes,
            decode_backing,
        );
        assert!(trace.borrow().contains(&decode_event));
        let substituted_path_bytes = encode_trie_path(&new_gint_par(42, Vec::new(), false)).len();
        let insertion_event = (
            2 * (2 * substituted_path_bytes + 1),
            3 * substituted_path_bytes,
            substituted_path_bytes * std::mem::size_of::<usize>() * 4 + std::mem::size_of::<Par>(),
        );
        assert!(trace.borrow().contains(&insertion_event));

        let deny_decode = |operations, scanned, backing| {
            if (operations, scanned, backing) == decode_event {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        let signature = CostSignature {
            value: Some(Value::Quote(models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new(
                        vec![new_boundvar_par(0, Vec::new(), false)],
                        Vec::new(),
                        false,
                        None,
                    ))),
                }],
                ..Default::default()
            })),
        };
        assert!(matches!(
            substitute.substitute_cost_signature_metered(signature, 0, &env, &deny_decode),
            Err(InterpreterError::HostWorkRejected)
        ));
    }

    #[test]
    fn metered_quote_substitutes_epathmap_map_keys_and_values() {
        use models::rust::canonical_path::encode_trie_path;
        use models::rust::rhoapi_ext::{EPathMap, EntryTrie};

        let substitute = substitute();
        let mut map = EPathMap::new(EntryTrie::default(), Vec::new(), false, None);
        map.insert_map_entry(
            new_boundvar_par(0, Vec::new(), false),
            new_boundvar_par(0, Vec::new(), false),
        )
        .unwrap();
        let signature = CostSignature {
            value: Some(Value::Quote(models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(map)),
                }],
                ..Default::default()
            })),
        };
        let env = Env::<Par>::new().put(new_gint_par(42, Vec::new(), false));
        let expected = substitute
            .substitute_cost_signature(signature.clone(), 0, &env)
            .unwrap();
        let trace = RefCell::new(Vec::new());
        let meter = |operations, scanned, backing| {
            trace.borrow_mut().push((operations, scanned, backing));
            Ok(())
        };
        let actual = substitute
            .substitute_cost_signature_metered(signature, 0, &env, &meter)
            .unwrap();
        assert_eq!(actual, expected);
        let key_bytes = encode_trie_path(&new_gint_par(42, Vec::new(), false)).len();
        assert!(trace.borrow().contains(&(
            2 * (2 * key_bytes + 2),
            3 * key_bytes,
            key_bytes * std::mem::size_of::<usize>() * 4 + 2 * std::mem::size_of::<Par>(),
        )));
    }

    #[test]
    fn deep_compound_rejects_on_a_small_stack_without_recursive_descent() {
        let mut signature = CostSignature {
            value: Some(Value::Ground(vec![7])),
        };
        for _ in 0..1024 {
            signature = CostSignature {
                value: Some(Value::Compound(CostSignatureCompound {
                    elements: vec![signature],
                })),
            };
        }
        let worker = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let substitute = substitute();
                let env = Env::<Par>::new();
                let unlimited = |_: usize, _: usize, _: usize| Ok(());
                substitute.substitute_cost_signature_metered(signature, 0, &env, &unlimited)
            })
            .expect("spawn bounded-stack substitution worker");
        assert!(matches!(
            worker.join().expect("worker must not overflow"),
            Err(InterpreterError::HostWorkRejected)
        ));
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
