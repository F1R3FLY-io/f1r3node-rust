// f1r3lang-with-monads Stage B.2 source-to-normalizer smoke test.
// Round-trips the minimal `foreach ( binder <- source ) yield ( r )
// body` surface through the grammar and p_foreach_normalizer into the
// `par.foreaches` proto field. Reducer-side native evaluation is a
// separate commit; this spec pins only the structural lowering.

use models::rhoapi::Par;
use rholang::rust::interpreter::compiler::compiler::Compiler;

fn compile(src: &str) -> Par {
    Compiler::source_to_adt(src).unwrap_or_else(|e| panic!("compile failed for {src:?}: {e:?}"))
}

#[test]
fn source_foreach_lowers_to_par_foreaches() {
    // yield-name must be in scope — wrap in `new r`.
    let par = compile("new r in { foreach ( x <- [1, 2, 3] ) yield ( r ) { r!(x) } }");

    // A new wraps the whole thing; drill into its body.
    assert_eq!(par.news.len(), 1, "outer new");
    let body = par.news[0]
        .p
        .as_ref()
        .expect("new body");

    assert_eq!(body.foreaches.len(), 1, "one foreach in body");
    let foreach = &body.foreaches[0];
    assert_eq!(foreach.bind_count, 1);

    let source = foreach.source.as_ref().expect("source");
    assert!(
        !source.exprs.is_empty(),
        "source has at least one expression (the list literal)"
    );

    let body_par = foreach.body.as_ref().expect("body");
    assert_eq!(
        body_par.sends.len(),
        1,
        "body has one send (r!(x))"
    );

    // yield_name was `r` from the outer `new`, so it resolves to a
    // BoundVar reference, not a GString.
    let yield_name = foreach.yield_name.as_ref().expect("yield_name");
    assert!(
        !yield_name.exprs.is_empty(),
        "yield_name is an EVar(BoundVar)"
    );
}

#[test]
fn source_foreach_unbound_yield_name_rejected() {
    // `rslt` is not bound in the surrounding scope — the normalizer
    // must reject so the yield has somewhere to land at reduce time.
    let src = "foreach ( x <- [1, 2, 3] ) yield ( rslt ) { rslt!(x) }";
    let result = Compiler::source_to_adt(src);
    assert!(result.is_err(), "expected unbound-yield-name error");
}
