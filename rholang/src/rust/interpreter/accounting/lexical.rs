use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Par, Receive};

use super::costs::Cost;
use super::RuntimeBudget;
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::metering::MeteredMachine;
use crate::rust::interpreter::substitute::{Substitute, SubstituteTrait};
use crate::rust::interpreter::util::{allocate_new_bindings, evaluation_random, evaluation_terms};

pub fn resolve_lexical_names_for_funding(
    program: &Par,
    rand: Blake2b512Random,
    urn_map: &HashMap<String, Par>,
) -> Result<Par, InterpreterError> {
    let substitute = Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::unsafe_max())),
    };
    resolve_par(program.clone(), rand, urn_map, &substitute)
}

fn resolve_receive(
    mut receive: Receive,
    rand: Blake2b512Random,
    urn_map: &HashMap<String, Par>,
    substitute: &Substitute,
) -> Result<Receive, InterpreterError> {
    if let Some(body) = receive.body.take() {
        receive.body = Some(resolve_par(body, rand, urn_map, substitute)?);
    }
    Ok(receive)
}

fn resolve_par(
    mut par: Par,
    rand: Blake2b512Random,
    urn_map: &HashMap<String, Par>,
    substitute: &Substitute,
) -> Result<Par, InterpreterError> {
    let term_count = evaluation_terms(&par).len();
    if term_count > i16::MAX as usize {
        return Err(InterpreterError::ReduceError(format!(
            "The number of terms in the Par is {}, which exceeds the limit of {}",
            term_count,
            i16::MAX
        )));
    }
    let mut index = par.sends.len();

    for receive in &mut par.receives {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        *receive = resolve_receive(receive.clone(), term_rand, urn_map, substitute)?;
        index += 1;
    }

    for new in &mut par.news {
        let mut term_rand = evaluation_random(&rand, index, term_count)?;
        let env = allocate_new_bindings(new, &Env::new(), &mut term_rand, urn_map)?;
        let body = new.p.take().ok_or_else(|| {
            InterpreterError::UndefinedRequiredProtobufFieldError("New.p".to_string())
        })?;
        let substituted = substitute.substitute_no_sort(body, 0, &env)?;
        new.p = Some(resolve_par(substituted, term_rand, urn_map, substitute)?);
        index += 1;
    }

    for mat in &mut par.matches {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        for case in &mut mat.cases {
            if let Some(source) = case.source.take() {
                case.source = Some(resolve_par(source, term_rand.clone(), urn_map, substitute)?);
            }
        }
        index += 1;
    }

    for conditional in &mut par.conditionals {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        if let Some(if_true) = conditional.if_true.take() {
            conditional.if_true = Some(resolve_par(
                if_true,
                term_rand.clone(),
                urn_map,
                substitute,
            )?);
        }
        if let Some(if_false) = conditional.if_false.take() {
            conditional.if_false = Some(resolve_par(if_false, term_rand, urn_map, substitute)?);
        }
        index += 1;
    }

    for bundle in &mut par.bundles {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        if let Some(body) = bundle.body.take() {
            bundle.body = Some(resolve_par(body, term_rand, urn_map, substitute)?);
        }
        index += 1;
    }

    index += par
        .exprs
        .iter()
        .filter(|expr| {
            matches!(
                expr.expr_instance,
                Some(ExprInstance::EVarBody(_)) | Some(ExprInstance::EMethodBody(_))
            )
        })
        .count();

    for signed in &mut par.cost_signed_terms {
        let term_rand = evaluation_random(&rand, index, term_count)?;
        if let Some(body) = signed.body.take() {
            signed.body = Some(resolve_par(body, term_rand, urn_map, substitute)?);
        }
        index += 1;
    }

    index += par.cost_stacks.len();
    if index != term_count {
        return Err(InterpreterError::BugFoundError(
            "funding resolver term schedule disagrees with reducer schedule".to_string(),
        ));
    }
    Ok(par)
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{GPrivate, GUnforgeable};

    use super::*;
    use crate::rust::interpreter::accounting::authority::cost_signature_to_sig;
    use crate::rust::interpreter::accounting::delta_sigma::static_authority_plan;
    use crate::rust::interpreter::accounting::Sig;
    use crate::rust::interpreter::compiler::compiler::Compiler;

    fn private_name(id: Vec<u8>) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }])
    }

    #[test]
    fn resolves_new_bound_cost_authorities_to_the_runtime_name() {
        let program = Compiler::source_to_adt(
            r#"new slot in { {% for(_ <- @"x"){ Nil } %}[ slot ] | slot :: slot :: () | @"x"!(0) }"#,
        )
        .unwrap();
        let rand = Blake2b512Random::create_from_bytes(b"lexical slot");
        let mut expected_rand = rand.clone();
        let expected = private_name(
            expected_rand
                .next()
                .into_iter()
                .map(|byte| byte as u8)
                .collect(),
        );
        let resolved = resolve_lexical_names_for_funding(&program, rand, &HashMap::new()).unwrap();
        let body = resolved.news[0].p.as_ref().unwrap();
        assert_eq!(
            body.cost_signed_terms[0].signature.as_ref().unwrap().value,
            Some(Value::Name(expected.clone()))
        );
        assert_eq!(
            body.cost_stacks[0].cells[0].value,
            Some(Value::Name(expected.clone()))
        );
        assert_eq!(
            body.cost_stacks[0].cells[1].value,
            Some(Value::Name(expected))
        );
    }

    #[test]
    fn resolved_slot_supply_reduces_external_reservation_without_erasing_demand() {
        let program = Compiler::source_to_adt(
            r#"new slot in { {% for(_ <- @"x"){ new y in { y!(0) | for(@0 <- y){ Nil } } } %}[ a -o slot ] | a :: () | slot :: slot :: () | @"slot-registry"!(*slot) }"#,
        )
        .unwrap();
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"funding plan"),
            &HashMap::new(),
        )
        .unwrap();
        let deploy = Sig::Ground(b"deployer".to_vec());
        let plan = static_authority_plan(&resolved, &deploy).unwrap();
        let body = resolved.news[0].p.as_ref().unwrap();
        let slot_stack = body
            .cost_stacks
            .iter()
            .find(|stack| stack.cells.len() == 2)
            .expect("two-cell slot stack");
        let slot =
            cost_signature_to_sig(slot_stack.cells.first().expect("slot stack cell")).unwrap();
        assert_eq!(
            plan.guaranteed_program_supply.get(&slot.lane_hash()),
            2,
            "plan={plan:?}, cells={:?}",
            slot_stack.cells
        );
        assert_eq!(plan.demand.get(&slot.lane_hash()), 2);
        assert_eq!(plan.external_reservation.get(&slot.lane_hash()), 0);
    }

    #[test]
    fn uri_bound_authority_uses_the_same_runtime_binding() {
        let program =
            Compiler::source_to_adt(r#"new slot(`rho:test:slot`) in { {% @"x"!(0) %}[ slot ] }"#)
                .unwrap();
        let expected = private_name(vec![9; 32]);
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"uri slot"),
            &HashMap::from([("rho:test:slot".to_string(), expected.clone())]),
        )
        .unwrap();
        let signature = resolved.news[0].p.as_ref().unwrap().cost_signed_terms[0]
            .signature
            .as_ref()
            .unwrap();
        assert_eq!(signature.value, Some(Value::Name(expected)));
    }

    #[test]
    fn receive_bound_authority_remains_dynamic() {
        let program =
            Compiler::source_to_adt(r#"for(slot <- @"slots"){ {% @"x"!(0) %}[ slot ] }"#).unwrap();
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"dynamic slot"),
            &HashMap::new(),
        )
        .unwrap();
        let signature = resolved.receives[0]
            .body
            .as_ref()
            .unwrap()
            .cost_signed_terms[0]
            .signature
            .as_ref()
            .unwrap();
        assert!(matches!(signature.value, Some(Value::BoundLevel(_))));
    }

    #[test]
    fn resolution_is_idempotent() {
        let program = Compiler::source_to_adt(r#"new slot in { {% @"x"!(0) %}[ slot ] }"#).unwrap();
        let rand = Blake2b512Random::create_from_bytes(b"idempotent slot");
        let once =
            resolve_lexical_names_for_funding(&program, rand.clone(), &HashMap::new()).unwrap();
        let twice = resolve_lexical_names_for_funding(&once, rand, &HashMap::new()).unwrap();
        assert_eq!(once, twice);
    }
}
