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

#[cfg(feature = "protobuf-projection-experiment")]
mod costs {
    use models::rust::par_construction::substitution_cost::{capture, Metric};

    use super::*;

    #[test]
    fn nonidentity_costs_track_distinct_endpoints_and_single_finish() {
        let mut previous = None;
        for depth in [1, 7, 64, 512, 4096] {
            let input = bound_list(depth);
            let mut payload_bytes = 0;
            input.visit_encoded::<Expr>(|entry| payload_bytes += entry.payload().len());
            let expected =
                Par::wrap_unary_elist(depth, new_gint_par(123, Vec::new(), false)).unwrap();
            let env = Env::new().put(new_gint_par(123, Vec::new(), false));
            let subject = substitute_instance();
            let (output, totals) = capture(|| subject.substitute_no_sort(input, 0, &env));
            let output = output.unwrap();
            assert_eq!(output.encode_to_vec(), expected.encode_to_vec());
            assert_eq!(
                output.components().compact_bytes(),
                expected.components().compact_bytes()
            );
            assert_eq!(totals.get(Metric::RootCopies), 1, "{totals:?}");
            assert_eq!(totals.get(Metric::RootBytes), payload_bytes, "{totals:?}");
            assert_eq!(totals.get(Metric::ArenaCount), 1, "{totals:?}");
            assert_eq!(totals.get(Metric::FinishAttempts), 1, "{totals:?}");
            assert_eq!(totals.get(Metric::FinishSuccesses), 1, "{totals:?}");
            assert_eq!(totals.get(Metric::RootKeys), 1, "{totals:?}");
            let mut expected_key_bytes = 0;
            output.visit_encoded::<Expr>(|entry| {
                expected_key_bytes += models::rust::par_bag::PAR_BAG_KEY_DOMAIN.len()
                    + 1
                    + entry.score().len()
                    + entry.payload().len()
                    + std::mem::size_of::<u64>();
            });
            assert_eq!(
                totals.get(Metric::RootKeyBytes),
                expected_key_bytes,
                "{totals:?}"
            );
            assert_eq!(totals.get(Metric::OriginalBytes), 0, "{totals:?}");
            assert_eq!(totals.get(Metric::ResidentBytes), 0, "{totals:?}");
            assert_eq!(totals.get(Metric::ParCuts), depth);
            assert_eq!(totals.get(Metric::MessageNodes), 2 * depth + 3);
            assert_eq!(totals.get(Metric::WorkSteps), 6 * depth + 5);
            assert_eq!(totals.get(Metric::ShallowBytes), 8 * depth + 7);
            for metric in [
                Metric::BorrowedRegions,
                Metric::FieldVisits,
                Metric::MessageNodes,
                Metric::ParCuts,
                Metric::WorkSteps,
                Metric::ShallowBytes,
            ] {
                assert!(totals.get(metric) > 0, "{metric:?}: {totals:?}");
            }
            if let Some((last_depth, last_shallow)) = previous {
                assert!(totals.get(Metric::ShallowBytes) * last_depth <= last_shallow * depth * 2);
            }
            previous = Some((depth, totals.get(Metric::ShallowBytes)));
            println!("SUBSTITUTION_COST depth={depth} {totals:?}");
        }
    }

    #[test]
    fn root_multiplicity_does_not_repeat_region_traversal() {
        let mut baseline = None;
        for count in [1, 2, 1_000_000] {
            let input = bound_list(7).scale_occurrences(count).unwrap();
            let expected = Par::wrap_unary_elist(7, new_gint_par(123, Vec::new(), false))
                .unwrap()
                .scale_occurrences(count)
                .unwrap();
            let env = Env::new().put(new_gint_par(123, Vec::new(), false));
            let subject = substitute_instance();
            let (output, totals) = capture(|| subject.substitute_no_sort(input, 0, &env));
            assert_eq!(
                output.unwrap().components().compact_bytes(),
                expected.components().compact_bytes()
            );
            if let Some(first) = baseline {
                assert_eq!(totals, first);
            }
            baseline = Some(totals);
        }
    }

    #[test]
    fn substitution_error_records_partial_work_without_finish() {
        let input = Par::default().with_exprs(vec![Expr::default()]);
        assert!(!substitution_identity(&input));
        let subject = substitute_instance();
        let (result, totals) = capture(|| subject.substitute_no_sort(input, 0, &Env::new()));
        assert!(matches!(result, Err(rholang::rust::interpreter::errors::InterpreterError::UndefinedRequiredProtobufFieldError(_))));
        assert!(totals.get(Metric::WorkSteps) > 0);
        assert_eq!(totals.get(Metric::ArenaCount), 1);
        assert_eq!(totals.get(Metric::FinishAttempts), 0);
        assert_eq!(totals.get(Metric::FinishSuccesses), 0);
    }

    #[test]
    fn identity_substitution_has_no_construction_events() {
        let input = Par::wrap_unary_elist(64, new_gint_par(123, Vec::new(), false)).unwrap();
        assert!(substitution_identity(&input));
        let expected = input.components().compact_bytes();
        let subject = substitute_instance();
        let (output, totals) = capture(|| subject.substitute_no_sort(input, 0, &Env::new()));
        assert_eq!(output.unwrap().components().compact_bytes(), expected);
        assert_eq!(totals.get(Metric::ArenaCount), 0);
        assert_eq!(totals.get(Metric::FinishAttempts), 0);
        assert_eq!(totals.get(Metric::FinishSuccesses), 0);
        assert_eq!(totals.get(Metric::RootCopies), 0);
        assert_eq!(totals.get(Metric::WorkSteps), 0);
    }
}
