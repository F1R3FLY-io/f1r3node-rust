mod par_corpus;

use models::rhoapi::Par;
use models::rust::rhoapi_ext::EPathMap;
use models::rust::rholang::match_path::{
    encode_match_path, encode_par_match_path, epathmap_match_digest, RHO_MATCH_PATH_VERSION,
};
use models::rust::rholang::match_schema_tables::ParCursor;

#[test]
fn structural_paths_are_deterministic_and_schema_sensitive() {
    let corpus = par_corpus::par_corpus();
    for (name, value) in &corpus {
        let first = encode_par_match_path(value);
        let second = encode_par_match_path(value);
        assert_eq!(first, second, "{name}");
        assert!(first.starts_with(&RHO_MATCH_PATH_VERSION), "{name}");
    }

    for pair in corpus.windows(2) {
        assert_ne!(
            encode_par_match_path(&pair[0].1),
            encode_par_match_path(&pair[1].1),
            "adjacent exhaustive corpus terms {} and {} must remain distinguishable",
            pair[0].0,
            pair[1].0
        );
    }
}

#[test]
fn epathmap_token_uses_the_native_logical_trie_without_a_snapshot() {
    let set_forward = EPathMap::from_set_iter(
        [par_corpus::gint(1), par_corpus::gint(2)],
        Vec::new(),
        false,
        None,
    );
    assert!(format!("{set_forward:?}").contains("snapshot_cached: false"));
    assert_eq!(
        hex::encode(epathmap_match_digest(&set_forward)),
        "9a637685a61e33171ea5e35a90b60d82e92697517a4fc56c02d7fbbeddd5befd"
    );
    assert!(format!("{set_forward:?}").contains("snapshot_cached: false"));

    // Moving a clone into ParBag construction is allowed to materialize its
    // canonical endpoint snapshot. Keep that independent from the native
    // digest subject above: the property under test is that the EPathMap
    // digest itself does not force a snapshot.
    let wrapped_forward = EPathMap::from_set_iter(
        [par_corpus::gint(1), par_corpus::gint(2)],
        Vec::new(),
        false,
        None,
    );
    let wrapped_reverse = EPathMap::from_set_iter(
        [par_corpus::gint(2), par_corpus::gint(1)],
        Vec::new(),
        false,
        None,
    );
    assert_eq!(
        encode_par_match_path(&wrap(wrapped_forward)),
        encode_par_match_path(&wrap(wrapped_reverse))
    );

    let map_a = EPathMap::new_map(
        [(par_corpus::gint(1), par_corpus::gint(10))],
        Vec::new(),
        false,
        None,
    );
    let map_b = EPathMap::new_map(
        [(par_corpus::gint(1), par_corpus::gint(20))],
        Vec::new(),
        false,
        None,
    );
    let set = EPathMap::from_set_iter([par_corpus::gint(1)], Vec::new(), false, None);

    // Map values are intentionally delegated to the native EPathMap matcher.
    // Their shared structural token is a conservative false-positive lane,
    // never an identity or acceptance decision.
    assert_eq!(
        encode_match_path(ParCursor::from(&map_a)),
        encode_match_path(ParCursor::from(&map_b))
    );
    assert_ne!(
        encode_match_path(ParCursor::from(&set)),
        encode_match_path(ParCursor::from(&set_forward))
    );
}

#[test]
fn deep_and_wide_structural_encoding_fits_a_small_native_stack() {
    std::thread::Builder::new()
        .name("rho-match-path".to_string())
        .stack_size(256 * 1024)
        .spawn(|| {
            let deep = encode_par_match_path(&par_corpus::deep_par(20_000));
            assert!(
                deep.len() < 256,
                "the native ParBag token must remain compact regardless of AST depth"
            );

            let mut wide = par_corpus::all_par_fields();
            let mut exprs = wide.decode_occurrences::<models::rhoapi::Expr>();
            exprs.reserve(20_000);
            let seed = exprs[0].clone();
            exprs.extend(std::iter::repeat_n(seed, 20_000));
            wide.replace_exprs(exprs);
            let wide = encode_par_match_path(&wide);
            assert!(
                wide.len() < 256,
                "the native ParBag token must remain compact regardless of sibling width"
            );
        })
        .expect("spawn small-stack structural encoder")
        .join()
        .expect("small-stack structural encoder must complete");
}

fn wrap(map: EPathMap) -> Par {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::Expr;

    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map)),
    }])
}
