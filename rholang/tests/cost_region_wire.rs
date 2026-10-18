use models::rhoapi::cost_signature_atom::Form;
use models::rhoapi::{New, Receive, Send};
use prost::Message;
use rholang::rust::interpreter::compiler::compiler::Compiler;

#[test]
fn signed_send_and_receive_keep_region_on_wire() {
    let send = Compiler::source_to_adt(r#"{% @"r"!(42) %}[ s ]"#).unwrap();
    let plain_send = Compiler::source_to_adt(r#"@"r"!(42)"#).unwrap();
    assert_ne!(send, plain_send);
    let sends = send.decode_occurrences::<Send>();
    assert_eq!(sends.len(), 1);
    let send_region = sends[0].cost_region.as_ref().unwrap();
    assert_eq!(send_region.version, 1);
    assert_eq!(send_region.signer.as_ref().unwrap().atoms.len(), 1);
    let round_trip = models::rhoapi::Par::decode(send.encode_to_vec().as_slice()).unwrap();
    assert_eq!(round_trip, send);

    let receive = Compiler::source_to_adt(r#"{% for(x <- @"ch"){ Nil } %}[ s ]"#).unwrap();
    let plain_receive = Compiler::source_to_adt(r#"for(x <- @"ch"){ Nil }"#).unwrap();
    assert_ne!(receive, plain_receive);
    let receives = receive.decode_occurrences::<Receive>();
    assert_eq!(receives.len(), 1);
    assert_eq!(receives[0].cost_region.as_ref().unwrap().version, 1);
    assert_eq!(receives[0].binds.len(), 1);
    let round_trip = models::rhoapi::Par::decode(receive.encode_to_vec().as_slice()).unwrap();
    assert_eq!(round_trip, receive);
}

#[test]
fn inner_region_overrides_outer_region() {
    let term = Compiler::source_to_adt(r#"{% {% @"r"!(42) %}[ b ] %}[ a ]"#).unwrap();
    let sends = term.decode_occurrences::<Send>();
    let region = sends[0].cost_region.as_ref().unwrap();
    let inner = Compiler::source_to_adt(r#"{% @"r"!(42) %}[ b ]"#).unwrap();
    let inner_sends = inner.decode_occurrences::<Send>();
    assert_eq!(
        region.signer,
        inner_sends[0].cost_region.as_ref().unwrap().signer
    );
}

#[test]
fn lollipop_keeps_outer_payer_and_inner_continuation_signer() {
    let term =
        Compiler::source_to_adt(r#"{% for(x <- @"ch"){ @"out"!(42) } %}[ a -o b ]"#).unwrap();
    let receives = term.decode_occurrences::<Receive>();
    assert_eq!(receives.len(), 1);
    let receive_region = receives[0].cost_region.as_ref().unwrap();
    let body = receives[0].body.as_ref().unwrap();
    let sends = body.decode_occurrences::<Send>();
    assert_eq!(sends.len(), 1);
    let send_region = sends[0].cost_region.as_ref().unwrap();
    assert_ne!(receive_region.signer, send_region.signer);

    let uniform_a =
        Compiler::source_to_adt(r#"{% for(x <- @"ch"){ @"out"!(42) } %}[ a ]"#).unwrap();
    let uniform_receive = uniform_a.decode_occurrences::<Receive>();
    let uniform_body = uniform_receive[0].body.as_ref().unwrap();
    let uniform_send = uniform_body.decode_occurrences::<Send>();
    assert_eq!(
        receive_region.signer,
        uniform_receive[0].cost_region.as_ref().unwrap().signer
    );
    assert_ne!(
        send_region.signer,
        uniform_send[0].cost_region.as_ref().unwrap().signer
    );
}

#[test]
fn new_bound_signer_stays_a_runtime_variable() {
    let term = Compiler::source_to_adt(r#"new x in { {% x!(42) %}[ x ] }"#).unwrap();
    let news = term.decode_occurrences::<New>();
    let body = news[0].p.as_ref().unwrap();
    let sends = body.decode_occurrences::<Send>();
    let signer = sends[0]
        .cost_region
        .as_ref()
        .unwrap()
        .signer
        .as_ref()
        .unwrap();
    assert!(matches!(signer.atoms[0].form, Some(Form::Bound(_))));
}

#[test]
fn signed_join_keeps_clause_authority_at_data_arity() {
    let term =
        Compiler::source_to_adt(r#"new x, w in { for( {% y <- x %}[ s ] & @z <- w ){ Nil } }"#)
            .unwrap();
    let news = term.decode_occurrences::<New>();
    let body = news[0].p.as_ref().unwrap();
    let receives = body.decode_occurrences::<Receive>();
    assert_eq!(receives.len(), 1);
    assert_eq!(receives[0].binds.len(), 2);
    assert_eq!(
        receives[0]
            .binds
            .iter()
            .filter(|bind| bind.cost_region.is_some())
            .count(),
        1
    );
    let clause = receives[0]
        .binds
        .iter()
        .find_map(|bind| bind.cost_region.as_ref())
        .unwrap();
    assert_eq!(clause.version, 1);
    assert_eq!(clause.ordinal, 1);
}

#[test]
fn signed_join_sort_keeps_each_signer_with_its_source() {
    let compile_binds = |source: &str| {
        let term = Compiler::source_to_adt(source).unwrap();
        let news = term.decode_occurrences::<New>();
        let body = news[0].p.as_ref().unwrap();
        body.decode_occurrences::<Receive>()[0].binds.clone()
    };
    let a_only = compile_binds(r#"new x, w in { for( {% y <- x %}[ a ] & @z <- w ){ Nil } }"#);
    let b_only = compile_binds(r#"new x, w in { for( y <- x & {% @z <- w %}[ b ] ){ Nil } }"#);
    let both =
        compile_binds(r#"new x, w in { for( {% y <- x %}[ a ] & {% @z <- w %}[ b ] ){ Nil } }"#);
    let a_bind = a_only
        .iter()
        .find(|bind| bind.cost_region.is_some())
        .unwrap();
    let b_bind = b_only
        .iter()
        .find(|bind| bind.cost_region.is_some())
        .unwrap();
    assert_ne!(a_bind.source, b_bind.source);
    for bind in &both {
        if bind.source == a_bind.source {
            assert_eq!(
                bind.cost_region.as_ref().unwrap().signer,
                a_bind.cost_region.as_ref().unwrap().signer
            );
        } else if bind.source == b_bind.source {
            assert_eq!(
                bind.cost_region.as_ref().unwrap().signer,
                b_bind.cost_region.as_ref().unwrap().signer
            );
        } else {
            panic!("unexpected join source");
        }
    }
}

#[test]
fn signed_receipt_groups_keep_their_own_clause_regions() {
    let term = Compiler::source_to_adt(
        r#"new x, w in { for( {% y <- x %}[ a ]; {% @z <- w %}[ b ] ){ Nil } }"#,
    )
    .unwrap();
    let news = term.decode_occurrences::<New>();
    let outer = news[0].p.as_ref().unwrap().decode_occurrences::<Receive>();
    assert_eq!(outer.len(), 1);
    assert!(outer[0].binds[0].cost_region.is_some());
    let inner = outer[0]
        .body
        .as_ref()
        .unwrap()
        .decode_occurrences::<Receive>();
    assert_eq!(inner.len(), 1);
    assert!(inner[0].binds[0].cost_region.is_some());
    assert_ne!(
        outer[0].binds[0].cost_region.as_ref().unwrap().signer,
        inner[0].binds[0].cost_region.as_ref().unwrap().signer
    );
}
