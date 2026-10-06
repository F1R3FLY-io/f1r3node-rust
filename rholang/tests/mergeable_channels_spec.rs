use models::rhoapi::expr::ExprInstance;
use prost::Message;
use rholang::rust::interpreter::merging::mergeable_tags::bitmask_or_mergeable_tag_name;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::with_runtime;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_deploy_reports_only_its_own_mergeable_channels_under_the_uri_tag() {
    with_runtime("mergeable-channels-spec-", |mut runtime| async move {
        let tagged = runtime
            .evaluate_with_term(r#"new t(`rho:system:bitmaskMergeableTag`) in { @(*t, "x")!(1) }"#)
            .await
            .unwrap();
        assert!(tagged.errors.is_empty(), "{:?}", tagged.errors);
        assert_eq!(tagged.mergeable.len(), 1);

        let (channel, merge_type) = tagged.mergeable.iter().next().unwrap();
        assert_eq!(*merge_type, MergeType::BitmaskOr);
        let head = match &channel.exprs[0].expr_instance {
            Some(ExprInstance::ETupleBody(tuple)) => &tuple.ps[0],
            other => panic!("expected a tuple channel, got {other:?}"),
        };
        assert_eq!(
            head.encode_to_vec(),
            bitmask_or_mergeable_tag_name().encode_to_vec()
        );

        let next = runtime.evaluate_with_term("Nil").await.unwrap();
        assert!(next.errors.is_empty(), "{:?}", next.errors);
        assert!(next.mergeable.is_empty(), "{:?}", next.mergeable);
    })
    .await
}
