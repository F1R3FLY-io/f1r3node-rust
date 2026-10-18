use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par, Send};
use models::rust::pathmap_crate_type_mapper::substitution_identity;
use models::rust::utils::{new_boundvar_par, new_gint_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::substitute::{Substitute, SubstituteTrait};

fn subject() -> Substitute {
    Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::create(
            i64::MAX / 4,
            "substitution identity fast path",
        ))),
    }
}

fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

#[test]
fn a_deep_ground_par_preserves_its_cow_root_with_and_without_sorting() {
    let value = Par::wrap_unary_elist(8_192, gint(7)).expect("deep unary list is representable");
    assert!(substitution_identity(&value));

    let no_sort_witness = value.clone();
    let no_sort = subject()
        .substitute_no_sort(value, 41, &Env::new().put(gint(99)))
        .expect("ground substitution succeeds");
    assert_eq!(no_sort, no_sort_witness);
    assert!(
        no_sort
            .components()
            .shares_storage_with(no_sort_witness.components()),
        "the certified no-sort path must return the owned CoW root"
    );

    let sorted_input = no_sort;
    let sorted_witness = sorted_input.clone();
    let sorted = subject()
        .substitute(sorted_input, -17, &Env::new().put(gint(-1)))
        .expect("ground sorted substitution succeeds");
    assert_eq!(sorted, sorted_witness);
    assert!(
        sorted
            .components()
            .shares_storage_with(sorted_witness.components()),
        "the certified sorted path must skip both the PDA and root sort"
    );
}

#[test]
fn a_bound_variable_inside_a_list_is_not_certified_and_is_substituted() {
    let open = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![new_boundvar_par(0, Vec::new(), false)],
            locally_free: vec![1],
            connective_used: false,
            remainder: None,
        })),
    }]);
    assert!(!substitution_identity(&open));

    let result = subject()
        .substitute_no_sort(open, 0, &Env::new().put(gint(123)))
        .expect("the bound variable is resolved");
    let expr = result
        .first::<Expr>()
        .expect("resident Expr key is canonical")
        .expect("one list expression")
        .0;
    let Some(ExprInstance::EListBody(list)) = expr.expr_instance else {
        panic!("substitution changed the list constructor")
    };
    assert_eq!(list.ps, vec![gint(123)]);
}

#[test]
fn the_broader_reducer_identity_domain_is_rejected_for_substitution() {
    let process = Par::default().with_sends(vec![Send {
        chan: Some(new_boundvar_par(0, Vec::new(), false)),
        data: Vec::new(),
        persistent: false,
        locally_free: vec![1],
        connective_used: false,
        cost_region: None,
    }]);
    assert_eq!(process.family_cardinality::<Expr>(), 0);
    assert!(models::rust::pathmap_crate_type_mapper::reducer_eval_identity(&process));
    assert!(!substitution_identity(&process));

    let result = subject()
        .substitute_no_sort(process, 0, &Env::new().put(gint(5)))
        .expect("the process channel is substituted");
    let send = result
        .first::<Send>()
        .expect("resident Send key is canonical")
        .expect("one Send")
        .0;
    assert_eq!(send.chan, Some(gint(5)));
}
