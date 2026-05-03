// Sanity check: the .rho files in /examples/where_*.rho parse and
// normalize cleanly. These are user-facing demos; if they break, users
// will hit the same break.

use std::collections::HashMap;
use std::fs;

use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::with_runtime;

fn assert_compiles(path: &str) {
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    match Compiler::source_to_adt_with_normalizer_env(&src, HashMap::new()) {
        Ok(_) => {}
        Err(e) => panic!("failed to compile {path}: {e:?}"),
    }
}

#[test]
fn where_receive_guard_example_compiles() {
    assert_compiles("../examples/where_receive_guard.rho");
}

#[test]
fn where_match_fallthrough_example_compiles() {
    assert_compiles("../examples/where_match_fallthrough.rho");
}

// Inline source: verifies the multi-bind cross-channel `where` syntax
// `for (@x <- a & @y <- b where x + y > 10)` parses and normalizes
// (Phase 9). The `where` clause attaches to the receipt, not to a
// single bind; binds are atomic-joined with `&`.
#[test]
fn where_multi_bind_atomic_join_compiles() {
    let src = r#"
        new a, b, stdout(`rho:io:stdout`) in {
            a!(3) | b!(15) |
            for (@x <- a & @y <- b where x + y > 10) {
                stdout!(("ok", x, y))
            }
        }
    "#;
    Compiler::source_to_adt_with_normalizer_env(src, HashMap::new())
        .expect("multi-bind where should compile");
}

// End-to-end: the cross-channel guard sees both bound vars at commit
// time. Evaluates without errors (guard 3+15=18 > 10, so the for fires
// and stdout!(("ok", 3, 15)) runs).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn where_multi_bind_atomic_join_evaluates() {
    with_runtime("where-multibind-eval-", |mut runtime| async move {
        let src = r#"
            new a, b, stdout(`rho:io:stdout`) in {
                a!(3) | b!(15) |
                for (@x <- a & @y <- b where x + y > 10) {
                    stdout!(("ok", x, y))
                }
            }
        "#;
        let result = runtime.evaluate_with_term(src).await.unwrap();
        assert!(
            result.errors.is_empty(),
            "no eval errors expected, got: {:?}",
            result.errors
        );
    })
    .await;
}

// Non-commutative end-to-end: `x - y > 0` is order-sensitive, so it would
// fire on the wrong data if the parser/normalizer ever swapped which bind
// contributes which de Bruijn index. With `x` from channel `a` and `y`
// from channel `b` and data `a!(200), b!(50)` the guard yields
// 200 - 50 = 150 > 0 → fires. Index swap would give 50 - 200 = -150 → no
// fire. This catches a regression the commutative `x + y > 10` test can't.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn where_multi_bind_non_commutative_guard_fires_with_correct_index_assignment() {
    with_runtime("where-multibind-noncomm-", |mut runtime| async move {
        let src = r#"
            new a, b, stdout(`rho:io:stdout`) in {
                a!(200) | b!(50) |
                for (@x <- a & @y <- b where x - y > 0) {
                    stdout!(("ok", x, y))
                }
            }
        "#;
        let result = runtime.evaluate_with_term(src).await.unwrap();
        assert!(
            result.errors.is_empty(),
            "no eval errors expected, got: {:?}",
            result.errors
        );
    })
    .await;
}
