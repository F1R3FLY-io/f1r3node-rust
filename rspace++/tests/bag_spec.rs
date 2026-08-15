use pathmap::ring::{AlgebraicResult, DistributiveLattice, Lattice, SELF_IDENT};
use rspace_plus_plus::rspace::bag::{Bag, BagError, Multiplicity};

fn bag(entries: &[(&[u8], u64)]) -> Bag {
    let mut result = Bag::new();
    for (path, count) in entries {
        result
            .insert_many(path, Multiplicity::new(*count).unwrap())
            .unwrap();
    }
    result
}

#[test]
fn multiplicity_rejects_zero_and_detects_overflow() {
    assert_eq!(Multiplicity::new(0), Err(BagError::ZeroMultiplicity));
    let mut value = Bag::new();
    value
        .insert_many(b"x", Multiplicity::new(u64::MAX).unwrap())
        .unwrap();
    assert_eq!(
        value.insert(b"x"),
        Err(BagError::MultiplicityOverflow {
            current: u64::MAX,
            added: 1,
        })
    );
    assert_eq!(value.count(b"x"), u64::MAX);
}

#[test]
fn multiplicity_bincode_rejects_zero() {
    let encoded_zero = bincode::serialize(&0u64).unwrap();
    assert!(bincode::deserialize::<Multiplicity>(&encoded_zero).is_err());
    let original = Multiplicity::new(17).unwrap();
    let encoded = bincode::serialize(&original).unwrap();
    assert_eq!(bincode::deserialize::<Multiplicity>(&encoded).unwrap(), original);
}

#[test]
fn insert_remove_and_metadata_preserve_the_absent_zero_invariant() {
    let mut value = Bag::new();
    value.insert(b"alpha").unwrap();
    value
        .insert_many(b"alpha", Multiplicity::new(2).unwrap())
        .unwrap();
    value.insert(b"beta").unwrap();
    assert_eq!(value.count(b"alpha"), 3);
    assert_eq!(value.distinct_len(), 2);
    assert_eq!(value.cardinality(), 4);
    assert_eq!(value.remove(b"alpha").unwrap().get(), 2);
    assert_eq!(value.remove_many(b"alpha", Multiplicity::new(9).unwrap()), None);
    assert!(!value.contains(b"alpha"));
    assert_eq!(value.distinct_len(), 1);
    assert_eq!(value.cardinality(), 1);
}

#[test]
fn sum_is_additive_while_join_and_meet_are_lattice_operations() {
    let left = bag(&[(b"a", 2), (b"b", 5)]);
    let right = bag(&[(b"a", 3), (b"c", 7)]);
    let sum = left.sum(&right).unwrap();
    let join = left.join(&right);
    let meet = left.meet(&right);
    assert_eq!(sum.count(b"a"), 5);
    assert_eq!(join.count(b"a"), 3);
    assert_eq!(join.count(b"b"), 5);
    assert_eq!(join.count(b"c"), 7);
    assert_eq!(meet, bag(&[(b"a", 2)]));
    assert_eq!(left.join(&left), left);
    assert_ne!(left.sum(&left).unwrap(), left);
}

#[test]
fn bag_lattice_satisfies_commutativity_associativity_and_absorption() {
    let a = bag(&[(b"shared", 2), (b"a", 8)]);
    let b = bag(&[(b"shared", 5), (b"b", 3)]);
    let c = bag(&[(b"shared", 4), (b"c", 9)]);
    assert_eq!(a.join(&b), b.join(&a));
    assert_eq!(a.meet(&b), b.meet(&a));
    assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));
    assert_eq!(a.meet(&b).meet(&c), a.meet(&b.meet(&c)));
    assert_eq!(a.join(&a.meet(&b)), a);
    assert_eq!(a.meet(&a.join(&b)), a);
}

#[test]
fn subtraction_is_truncated_and_reports_the_identity_case() {
    let left = bag(&[(b"a", 8), (b"b", 2)]);
    let right = bag(&[(b"a", 3), (b"b", 9)]);
    assert_eq!(left.subtract(&right), bag(&[(b"a", 5)]));
    let disjoint = bag(&[(b"z", 1)]);
    assert!(matches!(left.psubtract(&disjoint), AlgebraicResult::Identity(SELF_IDENT)));
}

#[test]
fn prefix_queries_remain_in_trie_order() {
    let value =
        bag(&[(b"prefix/a", 2), (b"prefix/a/deeper", 3), (b"prefix/b", 4), (b"unrelated", 5)]);
    assert_eq!(value.entries_with_prefix(b"prefix/"), vec![
        (b"prefix/a".to_vec(), Multiplicity::new(2).unwrap()),
        (b"prefix/a/deeper".to_vec(), Multiplicity::new(3).unwrap()),
        (b"prefix/b".to_vec(), Multiplicity::new(4).unwrap()),
    ]);
}

#[test]
fn clone_is_copy_on_write_and_merkleization_preserves_semantics() {
    let original = bag(&[(b"shared/left", 2), (b"shared/right", 3)]);
    let mut fork = original.clone();
    fork.insert(b"shared/new").unwrap();
    assert_eq!(original.count(b"shared/new"), 0);
    let before = fork.clone();
    fork.merkleize();
    assert_eq!(fork, before);
}

#[test]
fn deep_keys_clone_and_drop_on_a_small_native_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let key = vec![0xA5; 1_000_000];
            let mut value = Bag::new();
            value.insert(&key).unwrap();
            let fork = value.clone();
            drop(value);
            assert_eq!(fork.count(&key), 1);
            drop(fork);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn trait_algebra_agrees_with_inherent_algebra() {
    let left = bag(&[(b"a", 2), (b"b", 1)]);
    let right = bag(&[(b"a", 3), (b"c", 1)]);
    assert!(
        matches!(left.pjoin(&right), AlgebraicResult::Element(value) if value == left.join(&right))
    );
    assert!(
        matches!(left.pmeet(&right), AlgebraicResult::Element(value) if value == left.meet(&right))
    );
}
