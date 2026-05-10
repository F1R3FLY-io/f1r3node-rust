use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn multiple_equivocations_distinct_records() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // First equivocation at seq=5.
    let _b1 = harness.sign_block("v0", 5);
    let b1_prime = harness.sign_block_distinct("v0", 5);
    let s1 = harness.dispatch(b1_prime);
    assert_eq!(s1, Status::IgnorableEquivocation);
    assert!(
        harness.has_record("v0", 4),
        "first equivocation recorded at base=4"
    );

    // Second equivocation by the same validator at seq=10.
    let _b2 = harness.sign_block("v0", 10);
    let b2_prime = harness.sign_block_distinct("v0", 10);
    let s2 = harness.dispatch(b2_prime);
    // The second equivocation's base_seq=9 has no prior record, so the
    // dispatcher classifies it as Ignorable too — and mints a fresh
    // record under (v0, 9).
    assert_eq!(s2, Status::IgnorableEquivocation);
    assert!(
        harness.has_record("v0", 9),
        "second equivocation recorded at base=9"
    );

    // Both records coexist.
    assert!(harness.has_record("v0", 4));
    assert!(harness.has_record("v0", 9));

    // Witness sets are partitioned by base seq.
    assert!(harness.record_witnesses("v0", 4).contains(&b1_prime));
    assert!(!harness.record_witnesses("v0", 4).contains(&b2_prime));
    assert!(harness.record_witnesses("v0", 9).contains(&b2_prime));
    assert!(!harness.record_witnesses("v0", 9).contains(&b1_prime));
}
