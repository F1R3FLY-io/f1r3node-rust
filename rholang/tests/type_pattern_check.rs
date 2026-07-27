//! Verifies the Rholang type-conjunction pattern syntax used by
//! Buffer.rho to guard caller-supplied args before touching the
//! metadata-token critical section.
//!
//! `x /\ Type` binds `x` and requires it to be an inhabitant of
//! `Type`.  Pattern matching is total, so `match arg { x /\ Int => ok
//! _ => reject }` never raises — even on non-Int input.

use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;

#[test]
fn type_conjunction_pattern_compiles() {
    ParBuilderUtil::mk_term("match 42 { n /\\ Int => Nil _ => Nil }").expect("Int guard");
    ParBuilderUtil::mk_term(r#"match "hi".toUtf8Bytes() { b /\ ByteArray => Nil _ => Nil }"#)
        .expect("ByteArray guard");
    ParBuilderUtil::mk_term(r#"match "hi" { s /\ String => Nil _ => Nil }"#).expect("String guard");
    ParBuilderUtil::mk_term("match true { b /\\ Bool => Nil _ => Nil }").expect("Bool guard");
}
