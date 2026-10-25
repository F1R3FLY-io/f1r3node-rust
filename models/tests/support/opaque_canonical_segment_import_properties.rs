use proptest::prelude::*;
use proptest::test_runner::TestCaseResult;

fn check_opaque_expr_import(expr: Expr) -> TestCaseResult {
    prop_assert!(<Expr as ParBagComponent>::canonical_path_stable(&expr));
    let source = Par::default().with_exprs(vec![expr.clone()]);
    let key = CanonicalParBagKey::compile(&expr);
    let (expected_score, expected_component_protobuf) = key.parts();
    let mut arena = ParConstruction::new();
    let component = arena.import_component(&expr).unwrap();
    let mut imported_score = Vec::new();
    arena.emit_score(arena.components[component.0].score, &mut imported_score);
    prop_assert_eq!(imported_score.as_slice(), expected_score);
    let imported_component_protobuf = WireByteCursor::new(
        &arena,
        WireRoot::Proto(arena.components[component.0].protobuf),
    )
    .collect::<Vec<_>>();
    prop_assert_eq!(
        imported_component_protobuf.as_slice(),
        expected_component_protobuf
    );
    let planned = arena
        .singleton_component(component, Vec::new(), false)
        .unwrap();

    prop_assert_eq!(
        arena.canonical_path_bytes(planned).unwrap(),
        crate::rust::canonical_path::encode_trie_path(&source)
    );
    prop_assert_eq!(
        arena.canonical_segment_bytes(planned).unwrap(),
        crate::rust::canonical_path::encode_trie_segment(&source)
    );
    prop_assert_eq!(
        arena.protobuf_bytes(planned),
        source.protobuf_bytes().unwrap()
    );

    let stable_rewrap = arena.with_metadata(planned, Vec::new(), false).unwrap();
    prop_assert_eq!(
        arena.canonical_path_bytes(stable_rewrap).unwrap(),
        crate::rust::canonical_path::encode_trie_path(&source)
    );
    let metadata_rewrap = arena.with_metadata(planned, vec![1], true).unwrap();
    let reconstructed = arena.finish(metadata_rewrap).unwrap();
    prop_assert_eq!(
        arena.canonical_path_bytes(metadata_rewrap).unwrap(),
        crate::rust::canonical_path::encode_trie_path(&reconstructed)
    );
    prop_assert_eq!(
        arena.canonical_segment_bytes(metadata_rewrap).unwrap(),
        crate::rust::canonical_path::encode_trie_segment(&reconstructed)
    );
    Ok(())
}

proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(128))]

    #[test]
    fn generated_opaque_scalar_and_private_imports_retain_exact_keys_and_wire(
        value in any::<i64>(),
        bytes in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        for instance in [
            ExprInstance::GBool(value & 1 != 0),
            ExprInstance::GInt(value),
            ExprInstance::GString(text.clone()),
            ExprInstance::GUri(text),
            ExprInstance::GByteArray(bytes.clone()),
            ExprInstance::GDouble(value as u64),
            ExprInstance::GBigInt(bytes.clone()),
            ExprInstance::GBigRat(crate::rhoapi::GBigRational {
                numerator: bytes.clone(),
                denominator: value.to_be_bytes().to_vec(),
            }),
            ExprInstance::GFixedPoint(crate::rhoapi::GFixedPoint {
                unscaled: bytes.clone(),
                scale: value as u32,
            }),
        ] {
            check_opaque_expr_import(Expr {
                expr_instance: Some(instance),
            })?;
        }

        let private = GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: bytes,
            })),
        };
        prop_assert!(<GUnforgeable as ParBagComponent>::canonical_path_stable(&private));
        let source = Par::default().with_unforgeables(vec![private.clone()]);
        let key = CanonicalParBagKey::compile(&private);
        let (expected_score, expected_component_protobuf) = key.parts();
        let mut arena = ParConstruction::new();
        let component = arena.import_component(&private).unwrap();
        let mut imported_score = Vec::new();
        arena.emit_score(arena.components[component.0].score, &mut imported_score);
        prop_assert_eq!(imported_score.as_slice(), expected_score);
        let imported_component_protobuf = WireByteCursor::new(
            &arena,
            WireRoot::Proto(arena.components[component.0].protobuf),
        )
        .collect::<Vec<_>>();
        prop_assert_eq!(imported_component_protobuf.as_slice(), expected_component_protobuf);
        let planned = arena.singleton_component(component, Vec::new(), false).unwrap();
        prop_assert_eq!(
            arena.canonical_path_bytes(planned).unwrap(),
            crate::rust::canonical_path::encode_trie_path(&source)
        );
        prop_assert_eq!(
            arena.canonical_segment_bytes(planned).unwrap(),
            crate::rust::canonical_path::encode_trie_segment(&source)
        );
        prop_assert_eq!(arena.protobuf_bytes(planned), source.protobuf_bytes().unwrap());
    }

    #[test]
    fn generated_opaque_lists_and_tuples_preserve_distinct_segment_and_split_path(
        values in proptest::collection::vec(any::<i64>(), 0..12),
    ) {
        let children = values.into_iter().map(resident_gint).collect::<Vec<_>>();
        let list = Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: children.clone(),
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let source = Par::default().with_exprs(vec![list.clone()]);
        prop_assert_ne!(
            crate::rust::canonical_path::encode_trie_path(&source),
            crate::rust::canonical_path::encode_trie_segment(&source)
        );
        check_opaque_expr_import(list)?;
        check_opaque_expr_import(Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: children,
                locally_free: Vec::new(),
                connective_used: false,
            })),
        })?;
    }

    #[test]
    fn generated_opaque_pathmap_imports_retain_trie_keys_after_clone_on_write(
        values in proptest::collection::vec(any::<i64>(), 0..12),
        extra in any::<i64>(),
    ) {
        let set = EPathMap::from_set_iter(
            values.iter().copied().map(resident_gint),
            Vec::new(),
            false,
            None,
        );
        let original_snapshot = set.trie_snapshot().to_vec();
        let mut changed = set.clone();
        changed.insert_entry(resident_gint(extra));
        prop_assert_eq!(set.trie_snapshot(), original_snapshot.as_slice());
        for map in [set, changed] {
            check_opaque_expr_import(Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(map)),
            })?;
        }
        let map = EPathMap::new_map(
            values.into_iter().map(|value| (resident_gint(value), resident_gint(value.wrapping_add(1)))),
            Vec::new(),
            false,
            None,
        );
        check_opaque_expr_import(Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        })?;
    }
}
