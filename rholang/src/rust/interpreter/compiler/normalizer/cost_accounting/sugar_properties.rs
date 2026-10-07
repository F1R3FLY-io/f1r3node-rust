use models::rhoapi::Par;
use proptest::prelude::*;
use prost::Message;

use crate::rust::interpreter::compiler::compiler::Compiler;

fn signed(body: &str, authority: &str) -> String { format!("{{% {body} %}}[ {authority} ]") }

fn receive(body: &str, arrow: &str, count: usize) -> String {
    let binds = (0..count)
        .map(|index| format!("@value{index} {arrow} @\"input{index}\""))
        .collect::<Vec<_>>()
        .join("; ");
    format!("for({binds}){{ {body} }}")
}

fn compile(source: &str) -> Par {
    Compiler::source_to_adt(source)
        .unwrap_or_else(|error| panic!("generated valid source failed: {source}: {error:?}"))
}

fn authority() -> impl Strategy<Value = String> {
    prop::collection::vec(0_u16..32, 1..9).prop_map(|keys| {
        keys.iter()
            .map(|key| format!("payer{key}"))
            .collect::<Vec<_>>()
            .join(" (*) ")
    })
}

proptest! {
    #[test]
    fn transfer_equals_explicit_authority_layers(
        outer in authority(), inner in authority(),
        arrow in prop::sample::select(vec!["<-", "<=", "<<-"]),
        bind_count in 1_usize..9, payload in any::<i32>(),
        already_signed in any::<bool>(),
    ) {
        let bare_body = format!("@\"result\"!(value0, {payload})");
        let body = if already_signed { signed(&bare_body, "existing") } else { bare_body };
        let shorthand = signed(&receive(&body, arrow, bind_count), &format!("{outer} -o {inner}"));
        let explicit = signed(&receive(&signed(&body, &inner), arrow, bind_count), &outer);
        let shorthand = compile(&shorthand);
        let explicit = compile(&explicit);
        prop_assert_eq!(&shorthand, &explicit);
        prop_assert_eq!(shorthand.encode_to_vec(), explicit.encode_to_vec());
    }

    #[test]
    fn uniform_is_transfer_with_equal_authorities(
        payer in authority(), payload in any::<i32>(), count in 1_usize..9,
        arrow in prop::sample::select(vec!["<-", "<=", "<<-"]),
    ) {
        let body = receive(&format!("@\"result\"!(value0, {payload})"), arrow, count);
        let uniform = compile(&signed(&body, &payer));
        let transfer = compile(&signed(&body, &format!("{payer} -o {payer}")));
        prop_assert_eq!(uniform, transfer);
    }

    #[test]
    fn transfer_preserves_bound_authorities_and_payload_capture(
        payload in any::<i32>(), binder in 0_u16..1000,
    ) {
        let outer = format!("outer{binder}");
        let inner = format!("inner{binder}");
        let bare = format!("@\"result\"!(value0, *{outer}, *{inner}, {payload})");
        let shorthand = signed(&receive(&bare, "<-", 1), &format!("{outer} -o {inner}"));
        let explicit = signed(&receive(&signed(&bare, &inner), "<-", 1), &outer);
        let wrap = |body: &str| format!("new {outer}, {inner} in {{ {body} }}");
        prop_assert_eq!(compile(&wrap(&shorthand)), compile(&wrap(&explicit)));
    }

    #[test]
    fn transfer_chains_match_explicit_stages(
        payers in prop::collection::vec(0_u16..32, 2..10), payload in any::<i32>(),
    ) {
        let payers: Vec<_> = payers.iter().map(|key| format!("payer{key}")).collect();
        let mut bare = format!("@\"result\"!({payload})");
        let mut explicit = bare.clone();
        for (index, payer) in payers.iter().enumerate().rev() {
            bare = format!("for(_ <- @\"stage{index}\"){{ {bare} }}");
            explicit = signed(&format!("for(_ <- @\"stage{index}\"){{ {explicit} }}"), payer);
        }
        let shorthand = signed(&bare, &payers.join(" -o "));
        prop_assert_eq!(compile(&shorthand), compile(&explicit));
    }
}

#[test]
fn transfer_rejects_non_receive_bodies() {
    for body in ["Nil", "42", "@\"x\"!(0)", "new x in { x!(0) }"] {
        assert!(Compiler::source_to_adt(&signed(body, "a -o b")).is_err());
    }
}
