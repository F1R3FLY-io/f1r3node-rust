// Sanity check: the .rho files in /examples/where_*.rho parse and
// normalize cleanly. These are user-facing demos; if they break, users
// will hit the same break.

use std::collections::HashMap;
use std::fs;

use rholang::rust::interpreter::compiler::compiler::Compiler;

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

#[test]
fn where_match_as_expression_example_compiles() {
    assert_compiles("../examples/where_match_as_expression.rho");
}
