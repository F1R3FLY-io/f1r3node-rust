//! End-to-end regression tests for the rholang-rs parse-time
//! desugarings (method-call sugar from FIP 2025-08-20, agent block
//! sugar from FIP 2025-08-20 + 2026-01-28).
//!
//! Each case parses both a sugared form and its hand-written
//! equivalent through the same `Compiler::source_to_adt` pipeline
//! and asserts the resulting `Par`s are identical. This catches:
//!
//! - Any future parser change that breaks the equivalence the
//!   desugaring promises.
//! - Any normalizer change that introduces a divergence between
//!   the way `Proc::SendSync` is lowered when it came from `!?`
//!   versus when it came from the synthesized `x!y(...)` form
//!   (currently the same path; this test guards that).
//!
//! Upstream (rholang-rs) also has equivalence tests at the AST
//! level. These are Par-level: they exercise the full normalizer,
//! catching anything that goes wrong between parse and Par.

use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

// ----- Method-call sugar: x!y(args) -----------------------------------

#[test]
fn send_method_terminator_compiles_same_as_send_sync() {
    ParBuilderUtil::assert_compiled_equal(
        r#"new x in { x!y(1, 2). }"#,
        r#"new x in { x!?("y", 1, 2). }"#,
    );
}

#[test]
fn send_method_no_args_compiles_same_as_send_sync() {
    ParBuilderUtil::assert_compiled_equal(r#"new x in { x!y(). }"#, r#"new x in { x!?("y"). }"#);
}

#[test]
fn send_method_sequential_compiles_same_as_send_sync() {
    ParBuilderUtil::assert_compiled_equal(
        r#"new x in { x!set(42); Nil }"#,
        r#"new x in { x!?("set", 42); Nil }"#,
    );
}

#[test]
fn send_method_for_source_compiles_same_as_send_receive() {
    ParBuilderUtil::assert_compiled_equal(
        r#"new x in { for (@z <- x!get()) { Nil } }"#,
        r#"new x in { for (@z <- x!?("get")) { Nil } }"#,
    );
}

#[test]
fn send_method_for_source_with_args_compiles_same_as_send_receive() {
    ParBuilderUtil::assert_compiled_equal(
        r#"new x in { for (@z <- x!compute(1, 2, 3)) { Nil } }"#,
        r#"new x in { for (@z <- x!?("compute", 1, 2, 3)) { Nil } }"#,
    );
}

// Nested method call as argument. Inner argument is a literal so
// there's no name/proc context confusion at the resolver level (the
// upstream AST-level equivalence test used `b` here, which only
// surfaces a resolver error when run through the full normalizer).
#[test]
fn nested_send_method_compiles_same_as_nested_send_receive() {
    ParBuilderUtil::assert_compiled_equal(
        r#"new x, a in { x!y(a!z(1).). }"#,
        r#"new x, a in { x!?("y", a!?("z", 1).). }"#,
    );
}

// ----- Agent block sugar: agent fooCtor { ... } -----------------------

// Minimal: ctor + default only.
#[test]
fn agent_minimal_compiles_same_as_handwritten_desugaring() {
    let sugared = r#"
        new fooCtor in {
            agent fooCtor {
                constructor() { Nil } |
                default(...@args) { Nil }
            }
        }
    "#;
    let hand = r#"
        new fooCtor in {
            for (__r <= fooCtor) {
                new this, private in {
                    for (...@args <= this) {
                        match args {
                            _ => Nil
                        }
                    } |
                    Nil |
                    __r!(bundle+{*this})
                }
            }
        }
    "#;
    ParBuilderUtil::assert_compiled_equal(sugared, hand);
}

// Cell shape: ctor + 2 methods + default.
#[test]
fn agent_with_methods_compiles_same_as_handwritten_desugaring() {
    let sugared = r#"
        new Cell in {
            agent Cell {
                constructor(init) {
                    new state in { state!(*init) }
                } |
                method get() { Nil } |
                method set(newV) { Nil } |
                default(...@args) { Nil }
            }
        }
    "#;
    let hand = r#"
        new Cell in {
            for (__r, init <= Cell) {
                new this, private in {
                    for (...@args <= this) {
                        match args {
                            [*return, "get"] => Nil
                            [*return, "set", newV] => Nil
                            _ => Nil
                        }
                    } |
                    new state in { state!(*init) } |
                    __r!(bundle+{*this})
                }
            }
        }
    "#;
    ParBuilderUtil::assert_compiled_equal(sugared, hand);
}

// Mixed public + private: two parallel for-comprehensions inside
// `new this, private`.
#[test]
fn agent_with_private_compiles_same_as_handwritten_desugaring() {
    let sugared = r#"
        new Counter in {
            agent Counter {
                constructor(init) { Nil } |
                method get() { Nil } |
                private method incrInternal() { Nil } |
                default(...@args) { Nil } |
                private default(...@args) { Nil }
            }
        }
    "#;
    let hand = r#"
        new Counter in {
            for (__r, init <= Counter) {
                new this, private in {
                    for (...@args <= private) {
                        match args {
                            [*return, "incrInternal"] => Nil
                            _ => Nil
                        }
                    } |
                    for (...@args <= this) {
                        match args {
                            [*return, "get"] => Nil
                            _ => Nil
                        }
                    } |
                    Nil |
                    __r!(bundle+{*this})
                }
            }
        }
    "#;
    ParBuilderUtil::assert_compiled_equal(sugared, hand);
}

// Instance-private state using @[*private, *stateToken] as a
// compound address key. Guards the "private always bound" property.
#[test]
fn agent_private_state_compiles_same_as_handwritten_desugaring() {
    let sugared = r#"
        new Foo, stateToken in {
            agent Foo {
                constructor(@x) {
                    @[*private, *stateToken]!(x)
                } |
                method get() {
                    for (@state <<- @[*private, *stateToken]) {
                        return!(state)
                    }
                } |
                default(...@args) { Nil }
            }
        }
    "#;
    let hand = r#"
        new Foo, stateToken in {
            for (__r, @x <= Foo) {
                new this, private in {
                    for (...@args <= this) {
                        match args {
                            [*return, "get"] => for (@state <<- @[*private, *stateToken]) { return!(state) }
                            _ => Nil
                        }
                    } |
                    @[*private, *stateToken]!(x) |
                    __r!(bundle+{*this})
                }
            }
        }
    "#;
    ParBuilderUtil::assert_compiled_equal(sugared, hand);
}
