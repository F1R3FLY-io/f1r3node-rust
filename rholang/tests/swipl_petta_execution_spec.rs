use prost::Message;
use rholang::rust::interpreter::rho_type::{RhoList, RhoMap, RhoNumber, RhoString};
use rholang::rust::interpreter::swi_prolog_service::{petta_execute, petta_execute_with_blocks};
use rholang::rust::interpreter::test_utils::utils::should_skip_petta_test;

/// Tests for PeTTa execution service. This service is experimental.
/// This API is not finalized. Specifically, the interface between Rholang and MeTTa
/// is not complete. Here we test simple success and failure cases.
///
/// These tests require PeTTa to be installed. Set PETTA_PATH environment variable
/// to point to the PeTTa installation directory.
/// Example: PETTA_PATH=/path/to/PeTTa cargo test

#[tokio::test]
async fn test_petta_execute_simple_swap() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = "(= (swap (Pair $x $y)) (Pair $y $x)) !(swap (Pair 1 3))";
    let result = petta_execute(metta_code).await;

    assert!(
        result.is_ok(),
        "PeTTa execution should succeed: {:?}",
        result.err()
    );
    let par = result.unwrap();

    let expected_map = RhoMap::create_par(
        vec![(
            RhoString::create_par("results".into()),
            RhoList::create_par(vec![RhoList::create_par(vec![
                RhoString::create_par("Pair".into()),
                RhoNumber::create_par(3),
                RhoNumber::create_par(1),
            ])]),
        )]
        .into_iter()
        .collect(),
    );

    assert_eq!(
        par, expected_map,
        "Result should be {{\"results\": [\"(Pair 3 1)\"]}}"
    );
    assert!(
        !par.encode_to_vec().is_empty(),
        "Par structure should not be empty"
    );
}

#[tokio::test]
async fn test_petta_execute_fibonacci() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = r#"
        (= (fib-tr $n $a $b) (if (== $n 0) $a (fib-tr (- $n 1) $b (+ $a $b))))
        (= (fib $n) (fib-tr $n 0 1))
        !(fib 10)
    "#;
    let result = petta_execute(metta_code).await;

    assert!(
        result.is_ok(),
        "Fibonacci execution should succeed: {:?}",
        result.err()
    );
    let par = result.unwrap();

    let expected_map = RhoMap::create_par(
        vec![(
            RhoString::create_par("results".into()),
            RhoList::create_par(vec![RhoNumber::create_par(55)]),
        )]
        .into_iter()
        .collect(),
    );

    assert_eq!(
        par, expected_map,
        "fib(10) should return {{\"results\": [55]}}"
    );
    assert!(
        !par.encode_to_vec().is_empty(),
        "Par structure should not be empty"
    );
}

#[tokio::test]
async fn test_petta_execute_simple_arithmetic() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = "!(+ 1 2)";
    let result = petta_execute(metta_code).await;

    assert!(
        result.is_ok(),
        "Simple arithmetic should succeed: {:?}",
        result.err()
    );
    let par = result.unwrap();

    let expected_map = RhoMap::create_par(
        vec![(
            RhoString::create_par("results".into()),
            RhoList::create_par(vec![RhoNumber::create_par(3)]),
        )]
        .into_iter()
        .collect(),
    );

    assert_eq!(
        par, expected_map,
        "1 + 2 should return {{\"results\": [3]}}"
    );
    assert!(
        !par.encode_to_vec().is_empty(),
        "Par structure should not be empty"
    );
}

#[tokio::test]
async fn test_petta_execute_invalid_syntax() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = "(= incomplete";
    let result = petta_execute(metta_code).await;

    assert!(
        result.is_err(),
        "Invalid MeTTa syntax should fail: got {:?}",
        result.ok()
    );
}

#[tokio::test]
async fn test_petta_execute_empty_code() {
    if should_skip_petta_test() {
        return;
    }

    let metta_code = "";
    let result = petta_execute(metta_code).await;

    // Empty code might succeed or fail depending on PeTTa behavior
    println!("Empty code result: {:?}", result);
}

#[tokio::test]
async fn test_petta_execute_timeout_large_fibonacci() {
    if should_skip_petta_test() {
        return;
    }

    // Fibonacci of a very large number should timeout (10 second limit)
    // fib(10000000) will take much longer than 10 seconds
    let metta_code = r#"
        (= (fib-tr $n $a $b) (if (== $n 0) $a (fib-tr (- $n 1) $b (+ $a $b))))
        (= (fib $n) (fib-tr $n 0 1))
        !(fib 10000000)
    "#;

    let result = petta_execute(metta_code).await;

    // Should fail with a timeout error
    assert!(
        result.is_err(),
        "Large fibonacci computation should timeout after 10 seconds"
    );

    let err = result.unwrap_err();
    let err_msg = format!("{:?}", err);

    println!("Timeout error (expected): {}", err_msg);

    // Error should mention timeout
    assert!(
        err_msg.contains("timed out") || err_msg.contains("timeout"),
        "Error should be a timeout error, got: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_petta_readln_blocked_by_default() {
    if should_skip_petta_test() {
        return;
    }

    // By default petta.sh blocks readln!.  The predicate should resolve
    // to unevaluated data rather than reading from stdin.
    let metta_code = "!(readln!)";
    let result = petta_execute(metta_code).await;

    assert!(
        result.is_ok(),
        "Blocked readln! should resolve to data: {:?}",
        result.err()
    );
    let par = result.unwrap();

    // The unevaluated form is the literal list ["readln!"] inside results.
    let expected = RhoMap::create_par(
        vec![(
            RhoString::create_par("results".into()),
            RhoList::create_par(vec![RhoList::create_par(vec![RhoString::create_par(
                "readln!".into(),
            )])]),
        )]
        .into_iter()
        .collect(),
    );

    assert_eq!(
        par, expected,
        "Blocked readln! should yield {{\"results\": [[\"readln!\"]]}}, got {:?}",
        par
    );
}

#[tokio::test]
async fn test_petta_custom_blocked_pred() {
    if should_skip_petta_test() {
        return;
    }

    // Use the explicit override API so we don't touch the process-global env.
    let metta_code = r#"!(println! "hi")"#;
    let result = petta_execute_with_blocks(metta_code, Some("println!")).await;

    assert!(
        result.is_ok(),
        "Blocked println! should resolve to data: {:?}",
        result.err()
    );
    let par = result.unwrap();

    // The unevaluated form is the literal list ["println!", "hi"].
    let expected = RhoMap::create_par(
        vec![(
            RhoString::create_par("results".into()),
            RhoList::create_par(vec![RhoList::create_par(vec![
                RhoString::create_par("println!".into()),
                RhoString::create_par("hi".into()),
            ])]),
        )]
        .into_iter()
        .collect(),
    );

    assert_eq!(
        par, expected,
        "Blocked println! should yield {{\"results\": [[\"println!\", \"hi\"]]}}, got {:?}",
        par
    );
}
