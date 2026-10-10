//! G1-3 (DR-121): the funding resolver's names against native allocation.
//!
//! The reducer runs one program, and each `new` sends its name to an
//! observation channel. The funding resolver resolves the same program with
//! the same randomness. For every name that the resolver claims, the claim
//! must equal the name that the reducer allocated. A name that a receive body
//! creates depends on the matched datum, so the resolver must leave it
//! unresolved, and the analyzer must treat it as dynamic authority.

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::new_gstring_par;

use super::*;
use crate::rust::interpreter::accounting::authority::UnprovableDemand;
use crate::rust::interpreter::accounting::delta_sigma::static_authority_plan;
use crate::rust::interpreter::accounting::Sig;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::rho_runtime::RhoRuntime;
use crate::rust::interpreter::test_utils::resources::with_runtime;

/// Each `new` sends its name to `observe-<label>` and signs a send on
/// `label-<label>` with the name. The program covers a URN binding, a nested
/// `new`, a `new` in a match case, a `new` in a receive body (a delayed
/// activation), and an outer name inside the receive body.
const PROGRAM: &str = r#"
new outer, stdout(`rho:io:stdout`) in {
  @"observe-outer"!(*outer) | {% @"label-outer"!(0) %}[ outer ] |
  @"observe-stdout"!(*stdout) | {% @"label-stdout"!(0) %}[ stdout ] |
  new inner in {
    @"observe-inner"!(*inner) | {% @"label-inner"!(0) %}[ inner ]
  } |
  match 7 {
    _ => { new matched in { @"observe-matched"!(*matched) | {% @"label-matched"!(0) %}[ matched ] } }
  } |
  for (_ <- @"trigger") {
    new received in { @"observe-received"!(*received) | {% @"label-received"!(0) %}[ received ] } |
    {% @"label-outer-in-body"!(0) %}[ outer ]
  } |
  @"trigger"!(Nil)
}
"#;

/// The channel of a signed send: the label of its signed term.
fn label_of(body: &Par) -> Option<String> {
    let send = body.sends.first()?;
    let chan = send.chan.as_ref()?;
    match chan.exprs.first()?.expr_instance.as_ref()? {
        ExprInstance::GString(label) => label.strip_prefix("label-").map(str::to_string),
        _ => None,
    }
}

/// The signature of every signed term of a resolved program, by label. The
/// walk visits every position that the funding analysis reads.
fn signatures_by_label(resolved: &Par) -> HashMap<String, Option<CostSignatureValue>> {
    let mut signatures = HashMap::new();
    let mut pending = vec![resolved];
    while let Some(par) = pending.pop() {
        for send in &par.sends {
            pending.extend(send.data.iter());
        }
        for receive in &par.receives {
            pending.extend(receive.body.iter());
        }
        for new in &par.news {
            pending.extend(new.p.iter());
        }
        for matched in &par.matches {
            pending.extend(matched.cases.iter().filter_map(|case| case.source.as_ref()));
        }
        for conditional in &par.conditionals {
            pending.extend(conditional.if_true.iter());
            pending.extend(conditional.if_false.iter());
        }
        for bundle in &par.bundles {
            pending.extend(bundle.body.iter());
        }
        for signed in &par.cost_signed_terms {
            if let Some(label) = signed.body.as_ref().and_then(label_of) {
                signatures.insert(
                    label,
                    signed
                        .signature
                        .as_ref()
                        .and_then(|signature| signature.value.clone()),
                );
            }
            pending.extend(signed.body.iter());
        }
    }
    signatures
}

fn native_name(observed: &HashMap<String, Par>, label: &str) -> Option<CostSignatureValue> {
    observed
        .get(label)
        .map(|name| CostSignatureValue::Name(ParSortMatcher::sort_match(name).term))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn funding_resolver_names_match_native_allocation() {
    with_runtime("funding-names-", |runtime| async move {
        let rand = Blake2b512Random::create_from_bytes(b"native funding names");
        let result = runtime
            .evaluate(PROGRAM, Cost::unsafe_max(), HashMap::new(), rand.clone())
            .await
            .expect("the program evaluates");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let mut observed = HashMap::new();
        for label in ["outer", "stdout", "inner", "matched", "received"] {
            let channel = new_gstring_par(format!("observe-{label}"), Vec::new(), false);
            let data = runtime.get_data(&channel).await;
            assert_eq!(data.len(), 1, "one observation of {label}");
            let name = data[0].a.pars.first().cloned().expect("an observed name");
            observed.insert(label.to_string(), name);
        }

        let program = Compiler::source_to_adt(PROGRAM).expect("the program normalizes");
        let urn_map = runtime.reducer.urn_map.as_ref().clone();
        let resolved = resolve_lexical_names_for_funding(&program, rand, &urn_map)
            .expect("the resolver accepts the program");
        let claimed = signatures_by_label(&resolved);

        for label in ["outer", "stdout", "inner", "matched"] {
            assert_eq!(
                claimed.get(label).cloned().flatten(),
                native_name(&observed, label),
                "the resolver claims the native name of {label}"
            );
        }
        assert_eq!(
            claimed.get("outer-in-body").cloned().flatten(),
            native_name(&observed, "outer"),
            "an outer name keeps its native value inside a receive body"
        );
        assert!(
            matches!(
                claimed.get("received").cloned().flatten(),
                Some(CostSignatureValue::BoundLevel(_))
            ),
            "a name that a receive body creates stays unresolved: {:?}",
            claimed.get("received")
        );
        assert_eq!(
            static_authority_plan(&resolved, &Sig::Ground(b"deployer".to_vec())),
            Err(UnprovableDemand::DynamicAuthority),
            "the analyzer treats the receive-body name as dynamic authority"
        );
    })
    .await;
}

/// HEAD's recursive resolver gave a receive body the split of the receive's
/// own randomness. The reducer runs the body with the merge of that
/// randomness and the matched datum's randomness, so HEAD claimed a name that
/// the reducer never allocates.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn head_resolver_claimed_another_name_for_a_receive_body() {
    with_runtime("funding-names-head-", |runtime| async move {
        let rand = Blake2b512Random::create_from_bytes(b"native funding names");
        let result = runtime
            .evaluate(PROGRAM, Cost::unsafe_max(), HashMap::new(), rand.clone())
            .await
            .expect("the program evaluates");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let channel = new_gstring_par("observe-received".to_string(), Vec::new(), false);
        let data = runtime.get_data(&channel).await;
        let received = data[0].a.pars.first().cloned().expect("an observed name");
        let observed = HashMap::from([("received".to_string(), received)]);

        let program = Compiler::source_to_adt(PROGRAM).expect("the program normalizes");
        let urn_map = runtime.reducer.urn_map.as_ref().clone();
        let head = resolve_lexical_names_for_funding_recursive(&program, rand, &urn_map, true)
            .expect("HEAD's resolver accepts the program");
        let claimed = signatures_by_label(&head);
        let head_claim = claimed.get("received").cloned().flatten();
        assert!(matches!(head_claim, Some(CostSignatureValue::Name(_))));
        assert_ne!(
            head_claim,
            native_name(&observed, "received"),
            "HEAD claimed a name that the reducer does not allocate"
        );
    })
    .await;
}
