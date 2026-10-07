use loom::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use loom::sync::Arc;
use loom::thread;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValidationResult {
    Invalid,
    Deferred,
    Accepted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ValidationObservation {
    result: ValidationResult,
    lookup_count: usize,
}

fn canonical(vector: &[u8]) -> bool { vector.windows(2).all(|pair| pair[0] < pair[1]) }

fn effect_mask(vector: &[u8]) -> u8 {
    vector
        .iter()
        .fold(0, |mask, effect| mask | 1 << (effect - 1))
}

fn user_projection(vector: &[u8]) -> u8 { effect_mask(vector) & 0b011 }

fn validate(
    claimed: &[u8],
    computed: &[u8],
    held: &AtomicU8,
    declared_scope: u8,
    lookup_count: &AtomicUsize,
) -> ValidationObservation {
    if !canonical(claimed) || claimed != computed {
        return ValidationObservation {
            result: ValidationResult::Invalid,
            lookup_count: lookup_count.load(Ordering::SeqCst),
        };
    }
    for effect in computed {
        lookup_count.fetch_add(1, Ordering::SeqCst);
        if held.load(Ordering::SeqCst) & (1 << (effect - 1)) == 0 {
            return ValidationObservation {
                result: ValidationResult::Deferred,
                lookup_count: lookup_count.load(Ordering::SeqCst),
            };
        }
    }
    ValidationObservation {
        result: if declared_scope == user_projection(computed) {
            ValidationResult::Accepted
        } else {
            ValidationResult::Invalid
        },
        lookup_count: lookup_count.load(Ordering::SeqCst),
    }
}

fn validate_claims_first(
    claimed: &[u8],
    computed: &[u8],
    held: u8,
    declared_scope: u8,
) -> ValidationResult {
    if effect_mask(claimed) & !held != 0 {
        return ValidationResult::Deferred;
    }
    let held = AtomicU8::new(held);
    let lookups = AtomicUsize::new(0);
    validate(claimed, computed, &held, declared_scope, &lookups).result
}

#[test]
fn absent_attacker_extra_never_causes_dependency_resolution() {
    loom::model(|| {
        let held = Arc::new(AtomicU8::new(0b001));
        let lookups = Arc::new(AtomicUsize::new(0));
        let publish = {
            let held = held.clone();
            thread::spawn(move || held.fetch_or(0b100, Ordering::SeqCst))
        };
        let validate_claim = {
            let held = held.clone();
            let lookups = lookups.clone();
            thread::spawn(move || validate(&[1, 3], &[1, 2], &held, 0b011, &lookups))
        };

        publish.join().unwrap();
        let observation = validate_claim.join().unwrap();
        assert_eq!(observation.result, ValidationResult::Invalid);
        assert_eq!(observation.lookup_count, 0);
        assert_eq!(lookups.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn exact_vector_defers_only_until_its_genuine_dependency_arrives() {
    loom::model(|| {
        let held = Arc::new(AtomicU8::new(0b001));
        let lookups = Arc::new(AtomicUsize::new(0));
        let publish = {
            let held = held.clone();
            thread::spawn(move || held.fetch_or(0b010, Ordering::SeqCst))
        };
        let first = {
            let held = held.clone();
            let lookups = lookups.clone();
            thread::spawn(move || validate(&[1, 2], &[1, 2], &held, 0b011, &lookups))
        };

        publish.join().unwrap();
        let first = first.join().unwrap();
        assert!(matches!(
            first.result,
            ValidationResult::Deferred | ValidationResult::Accepted
        ));
        let retry = validate(&[1, 2], &[1, 2], &held, 0b011, &lookups);
        assert_eq!(retry.result, ValidationResult::Accepted);
    });
}

#[test]
fn held_inherited_non_applied_effect_is_invalid_without_resolution() {
    loom::model(|| {
        let held = AtomicU8::new(0b111);
        let lookups = AtomicUsize::new(0);
        let observation = validate(&[1, 3], &[1], &held, 0b001, &lookups);

        assert_eq!(observation.result, ValidationResult::Invalid);
        assert_eq!(observation.lookup_count, 0);
    });
}

#[test]
fn duplicate_and_out_of_order_vectors_are_invalid_without_resolution() {
    loom::model(|| {
        let held = AtomicU8::new(0b111);
        for claimed in [&[1, 1][..], &[2, 1][..]] {
            let lookups = AtomicUsize::new(0);
            let observation = validate(claimed, &[1, 2], &held, 0b011, &lookups);
            assert_eq!(observation.result, ValidationResult::Invalid);
            assert_eq!(observation.lookup_count, 0);
        }
    });
}

#[test]
fn accepted_vector_has_exact_user_projection() {
    loom::model(|| {
        let held = AtomicU8::new(0b111);
        let lookups = AtomicUsize::new(0);
        let accepted = validate(&[1, 2, 3], &[1, 2, 3], &held, 0b011, &lookups);
        let wrong_projection = validate(&[1, 2, 3], &[1, 2, 3], &held, 0b001, &lookups);

        assert_eq!(accepted.result, ValidationResult::Accepted);
        assert_eq!(wrong_projection.result, ValidationResult::Invalid);
    });
}

#[test]
fn claims_first_control_amplifies_an_attacker_only_dependency() {
    loom::model(|| {
        let safe_held = AtomicU8::new(0b011);
        let safe_lookups = AtomicUsize::new(0);
        let safe = validate(&[1, 3], &[1, 2], &safe_held, 0b011, &safe_lookups);
        let unsafe_result = validate_claims_first(&[1, 3], &[1, 2], 0b011, 0b011);

        assert_eq!(safe.result, ValidationResult::Invalid);
        assert_eq!(safe.lookup_count, 0);
        assert_eq!(unsafe_result, ValidationResult::Deferred);
    });
}

fn vectors() -> Vec<Vec<u8>> {
    let mut vectors = vec![Vec::new()];
    for length in 1..=3 {
        let count = 3usize.pow(length);
        for encoded in 0..count {
            let mut value = encoded;
            let mut vector = Vec::with_capacity(length as usize);
            for _ in 0..length {
                vector.push((value % 3 + 1) as u8);
                value /= 3;
            }
            vectors.push(vector);
        }
    }
    vectors
}

#[test]
fn finite_domain_property_matches_the_validation_contract() {
    let mut builder = loom::model::Builder::new();
    builder.max_branches = 250_000;
    builder.check(|| {
        for claimed in vectors() {
            for computed in vectors().into_iter().filter(|vector| canonical(vector)) {
                for held in 0..=0b111 {
                    for declared_scope in 0..=0b011 {
                        let lookups = AtomicUsize::new(0);
                        let observation = validate(
                            &claimed,
                            &computed,
                            &AtomicU8::new(held),
                            declared_scope,
                            &lookups,
                        );
                        if !canonical(&claimed) || claimed != computed {
                            assert_eq!(observation.result, ValidationResult::Invalid);
                            assert_eq!(observation.lookup_count, 0);
                        } else if effect_mask(&computed) & !held != 0 {
                            assert_eq!(observation.result, ValidationResult::Deferred);
                        } else if declared_scope == user_projection(&computed) {
                            assert_eq!(observation.result, ValidationResult::Accepted);
                        } else {
                            assert_eq!(observation.result, ValidationResult::Invalid);
                        }
                    }
                }
            }
        }
    });
}
