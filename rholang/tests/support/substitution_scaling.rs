use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::par_construction::ParConstruction;
use models::rust::pathmap_crate_type_mapper::substitution_identity;
use models::rust::utils::{new_boundvar_par, new_gint_par};
use prost::Message;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::substitute::SubstituteTrait;

use super::{counting_alloc, substitute_instance};

#[derive(Clone, Copy, Debug)]
pub(super) enum Entry {
    NoSort,
    Sorted,
    Charged,
}

impl Entry {
    fn label(self) -> &'static str {
        match self {
            Self::NoSort => "no_sort",
            Self::Sorted => "sorted",
            Self::Charged => "charged",
        }
    }
}

fn bound_list(depth: usize) -> Par {
    let mut arena = ParConstruction::new();
    let mut root = arena
        .import_par(&new_boundvar_par(0, vec![1], false))
        .unwrap();
    let shell = Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![Par::default()],
            locally_free: vec![1],
            ..EList::default()
        })),
    };
    for _ in 0..depth {
        let component = arena.expr_component(&shell, &[root], false).unwrap();
        root = arena
            .singleton_component(component, vec![1], false)
            .unwrap();
    }
    arena.finish(root).unwrap()
}

pub(super) fn run(depth: usize, entry: Entry) {
    let input = bound_list(depth);
    assert!(
        !substitution_identity(&input),
        "measurement must enter substitution"
    );
    let expected = Par::wrap_unary_elist(depth, new_gint_par(123, Vec::new(), false)).unwrap();
    let expected_bytes = expected.encode_to_vec();
    let input_bytes = input.encode_to_vec();
    assert_ne!(
        input_bytes, expected_bytes,
        "measurement must change the leaf"
    );
    let env = Env::new().put(new_gint_par(123, Vec::new(), false));
    let subject = substitute_instance();
    let ((output, elapsed), allocations, requested_bytes) = counting_alloc::measure(|| {
        let start = std::time::Instant::now();
        let output = match entry {
            Entry::NoSort => subject.substitute_no_sort(input, 0, &env),
            Entry::Sorted => subject.substitute(input, 0, &env),
            Entry::Charged => subject.substitute_and_charge(input, 0, &env),
        };
        (output, start.elapsed())
    });
    let output = output.expect("non-identity substitution succeeds");
    assert_eq!(
        output.canonical_unary_elist_gint_parts(),
        Some((depth, 123))
    );
    assert_eq!(output.encode_to_vec(), expected_bytes);
    assert_eq!(
        output.components().compact_bytes(),
        expected.components().compact_bytes()
    );
    println!(
        "SUBSTITUTION_SAMPLE\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        entry.label(),
        depth,
        elapsed.as_nanos(),
        allocations,
        requested_bytes,
        input_bytes.len(),
        expected_bytes.len(),
    );
}

#[test]
fn nonidentity_measurement_fixtures_have_exact_changed_outputs() {
    for depth in [0, 1, 7, 64] {
        for entry in [Entry::NoSort, Entry::Sorted, Entry::Charged] {
            run(depth, entry);
        }
    }
}
