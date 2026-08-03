use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPathMap, EZipper, Expr, Par};

use super::*;

fn int_par(value: i64) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }],
        ..Default::default()
    }
}

fn string_par(value: &str) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(value.to_owned())),
        }],
        ..Default::default()
    }
}

#[test]
fn empty_epathmap_preserves_neutral_mode() {
    let result = expr_from_expr_proto(Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::default())),
    });

    assert!(matches!(
        result,
        Some(RhoExpr::ExprPathMap {
            data: RhoPathMap::Empty
        })
    ));
}

#[test]
fn set_epathmap_preserves_set_mode_and_canonical_members() {
    let mut pathmap = EPathMap::default();
    pathmap.insert_entry(string_par("beta"));
    pathmap.insert_entry(string_par("alpha"));

    let result = expr_from_expr_proto(Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
    });

    let Some(RhoExpr::ExprPathMap {
        data: RhoPathMap::Set { entries },
    }) = result.as_ref()
    else {
        panic!("expected set-mode ExprPathMap");
    };
    assert_eq!(entries.len(), 2);
    assert!(entries
        .iter()
        .any(|entry| matches!(entry, RhoExpr::ExprString { data } if data == "alpha")));
    assert!(entries
        .iter()
        .any(|entry| matches!(entry, RhoExpr::ExprString { data } if data == "beta")));
}

#[test]
fn map_epathmap_retains_typed_keys_that_collide_as_strings() {
    let mut pathmap = EPathMap::default();
    pathmap
        .insert_map_entry(int_par(1), string_par("integer"))
        .expect("first insertion selects map mode");
    pathmap
        .insert_map_entry(string_par("1"), string_par("string"))
        .expect("second insertion remains map mode");

    let result = expr_from_expr_proto(Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
    });

    let Some(RhoExpr::ExprPathMap {
        data: RhoPathMap::Map { entries },
    }) = result.as_ref()
    else {
        panic!("expected map-mode ExprPathMap");
    };
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| {
        matches!(entry.key, RhoExpr::ExprInt { data: 1 })
            && matches!(entry.value, RhoExpr::ExprString { ref data } if data == "integer")
    }));
    assert!(entries.iter().any(|entry| {
        matches!(entry.key, RhoExpr::ExprString { ref data } if data == "1")
            && matches!(entry.value, RhoExpr::ExprString { ref data } if data == "string")
    }));
}

#[test]
fn zipper_preserves_map_mode_and_cursor_bytes() {
    let mut pathmap = EPathMap::default();
    pathmap
        .insert_map_entry(string_par("key"), int_par(7))
        .expect("first insertion selects map mode");

    let result = expr_from_expr_proto(Expr {
        expr_instance: Some(ExprInstance::EZipperBody(EZipper {
            pathmap: Some(pathmap),
            current_path: vec![vec![0x12, 0x34]],
            ..Default::default()
        })),
    });

    let Some(RhoExpr::ExprTuple { data }) = result.as_ref() else {
        panic!("expected zipper tuple");
    };
    assert!(matches!(
        data.first(),
        Some(RhoExpr::ExprPathMap {
            data: RhoPathMap::Map { entries }
        }) if entries.len() == 1
    ));
    assert!(matches!(
        data.get(1),
        Some(RhoExpr::ExprList { data })
            if matches!(data.first(), Some(RhoExpr::ExprBytes { data }) if data == "1234")
    ));
}
