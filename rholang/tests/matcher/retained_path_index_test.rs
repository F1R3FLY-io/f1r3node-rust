use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par};
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rspace_plus_plus::rspace::r#match::Match;

fn integer(value: i64) -> Par {
    let mut par = Par::default();
    par.exprs.push(Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    });
    par
}

fn datum(par: Par) -> ListParWithRandom {
    let mut datum = ListParWithRandom::default();
    datum.pars.push(par);
    datum
}

#[test]
fn exact_ground_pattern_and_datum_have_the_same_retained_path() {
    let matcher = Matcher;
    let value = integer(7);
    let pattern = BindPattern {
        patterns: vec![value.clone()],
        remainder: None,
        free_count: 0,
    };
    let datum = datum(value);

    assert_eq!(
        matcher.retained_pattern_path(&pattern),
        matcher.retained_datum_path(&datum)
    );
    assert!(matcher.get(&pattern, &datum).is_some());
}

#[test]
fn retained_paths_reject_binding_and_connective_patterns() {
    let matcher = Matcher;
    let mut binding = BindPattern {
        patterns: vec![integer(7)],
        remainder: None,
        free_count: 1,
    };
    assert!(matcher.retained_pattern_path(&binding).is_none());

    binding.free_count = 0;
    binding.patterns[0].connective_used = true;
    assert!(matcher.retained_pattern_path(&binding).is_none());
}

#[test]
fn distinct_ground_values_do_not_share_an_exact_retained_path() {
    let matcher = Matcher;
    let pattern = BindPattern {
        patterns: vec![integer(7)],
        remainder: None,
        free_count: 0,
    };
    let datum = datum(integer(8));

    assert_ne!(
        matcher.retained_pattern_path(&pattern),
        matcher.retained_datum_path(&datum)
    );
    assert!(matcher.get(&pattern, &datum).is_none());
}
