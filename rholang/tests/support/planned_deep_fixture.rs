//! Final-root-only builders and byte-native witnesses for deeply nested test terms.
//!
//! A PathMap-backed [`Par`] stores the canonical score and protobuf body in
//! each component key.  Repeatedly wrapping an already-resident child therefore
//! recopies every completed suffix and makes an otherwise iterative fixture
//! quadratic.  These builders retain [`PlannedPar`] handles in one flat
//! [`ParConstruction`] arena and call `finish` exactly once.
//!
//! The matching witnesses stay over borrowed canonical protobuf regions.  An
//! iterative loop that repeatedly calls `Par::first` would still decode and
//! materialize every remaining suffix, reintroducing the same defect after
//! construction had been repaired.

use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    EList, EMap, ENot, ESet, Expr, KeyValuePair, New, Par, Receive, ReceiveBind, Send,
};
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::par_bag::{
    canonical_exact_free_var_par, canonical_exact_scalar_par, canonical_expr_oneof_tag,
    canonical_expr_parts, canonical_new_parts, canonical_receive_parts, canonical_send_parts,
    canonical_single_expr_from_par, canonical_single_new_from_par,
    canonical_single_receive_from_par, canonical_single_send_from_par, CanonicalExprParts,
};
use models::rust::par_construction::{ParConstruction, PlannedPar};
use models::rust::utils::{new_freevar_par, new_gbool_par, new_gint_par};

fn import(arena: &mut ParConstruction, value: Par, what: &'static str) -> PlannedPar {
    arena
        .import_par(&value)
        .unwrap_or_else(|error| panic!("{what}: {error}"))
}

fn singleton_expr(
    arena: &mut ParConstruction,
    shell: &Expr,
    children: &[PlannedPar],
    connective_used: bool,
    what: &'static str,
) -> PlannedPar {
    let component = arena
        .expr_component(shell, children, false)
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    arena
        .singleton_component(component, Vec::new(), connective_used)
        .unwrap_or_else(|error| panic!("{what}: {error}"))
}

fn finish(arena: ParConstruction, root: PlannedPar, what: &'static str) -> Par {
    arena
        .finish(root)
        .unwrap_or_else(|error| panic!("{what}: {error}"))
}

pub(crate) fn nested_list_pattern(depth: usize) -> Par {
    let mut arena = ParConstruction::new();
    let mut planned = import(
        &mut arena,
        new_freevar_par(0, Vec::new()),
        "plan free-variable terminal",
    );
    let shell = Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![Par::default()],
            locally_free: Vec::new(),
            connective_used: true,
            remainder: None,
        })),
    };
    for _ in 0..depth {
        planned = singleton_expr(
            &mut arena,
            &shell,
            &[planned],
            true,
            "plan list-pattern level",
        );
    }
    finish(arena, planned, "finish list-pattern fixture")
}

pub(crate) fn nested_sets_leaf(depth: usize, leaf: i64) -> Par {
    let mut arena = ParConstruction::new();
    let mut planned = import(
        &mut arena,
        new_gint_par(leaf, Vec::new(), false),
        "plan ESet terminal",
    );
    let shell = Expr {
        expr_instance: Some(ExprInstance::ESetBody(ESet {
            ps: vec![Par::default()],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    };
    for _ in 0..depth {
        planned = singleton_expr(&mut arena, &shell, &[planned], false, "plan ESet level");
    }
    finish(arena, planned, "finish ESet fixture")
}

pub(crate) fn nested_maps(depth: usize) -> Par {
    let mut arena = ParConstruction::new();
    let mut planned = import(
        &mut arena,
        new_gint_par(0, Vec::new(), false),
        "plan EMap terminal",
    );
    let one = import(
        &mut arena,
        new_gint_par(1, Vec::new(), false),
        "plan EMap value",
    );
    let shell = Expr {
        expr_instance: Some(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(Par::default()),
                value: Some(Par::default()),
            }],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    };
    for _ in 0..depth {
        planned = singleton_expr(
            &mut arena,
            &shell,
            &[planned, one],
            false,
            "plan EMap level",
        );
    }
    finish(arena, planned, "finish EMap fixture")
}

pub(crate) fn nested_nots(depth: usize) -> Par {
    let mut arena = ParConstruction::new();
    let mut planned = import(
        &mut arena,
        new_gbool_par(true, Vec::new(), false),
        "plan ENot terminal",
    );
    let shell = Expr {
        expr_instance: Some(ExprInstance::ENotBody(ENot {
            p: Some(Par::default()),
        })),
    };
    for _ in 0..depth {
        planned = singleton_expr(&mut arena, &shell, &[planned], false, "plan ENot level");
    }
    finish(arena, planned, "finish ENot fixture")
}

pub(crate) fn nested_binders(depth: usize) -> Par {
    nested_binders_with_component_metadata(depth, Vec::new())
}

/// The same alternating binder chain with deliberately different ignored
/// component metadata. This keeps the semantic witness equal while preventing
/// authoritative `Par::eq` from proving it before the byte-region matcher is
/// exercised.
pub(crate) fn nested_binders_with_component_metadata(depth: usize, locally_free: Vec<u8>) -> Par {
    let mut arena = ParConstruction::new();
    let empty = arena.empty_par().expect("plan empty binder child");
    let zero = import(
        &mut arena,
        new_gint_par(0, Vec::new(), false),
        "plan binder scalar",
    );
    let mut planned = zero;
    let receive_shell = Receive {
        binds: vec![ReceiveBind {
            patterns: vec![Par::default()],
            source: Some(Par::default()),
            remainder: None,
            free_count: 0,
            cost_region: None,
        }],
        body: Some(Par::default()),
        persistent: false,
        peek: false,
        bind_count: 1,
        locally_free: locally_free.clone(),
        connective_used: false,
        condition: None,
        cost_region: None,
    };
    let new_shell = New {
        bind_count: 1,
        p: Some(Par::default()),
        uri: Vec::new(),
        injections: BTreeMap::new(),
        locally_free,
    };
    for _ in 0..depth {
        let receive = arena
            .receive_component(&receive_shell, &[empty, zero, planned, empty])
            .expect("plan Receive level");
        let receive = arena
            .singleton_component(receive, Vec::new(), false)
            .expect("plan Receive carrier");
        let new = arena
            .new_component(&new_shell, &[receive])
            .expect("plan New level");
        planned = arena
            .singleton_component(new, Vec::new(), false)
            .expect("plan New carrier");
    }
    finish(arena, planned, "finish binder fixture")
}

pub(crate) fn nested_send_chain(depth: usize) -> Par {
    let mut arena = ParConstruction::new();
    let mut planned = import(
        &mut arena,
        new_gint_par(0, Vec::new(), false),
        "plan Send terminal",
    );
    for level in 0..depth {
        let shell = Send {
            chan: Some(Par::default()),
            data: Vec::new(),
            persistent: false,
            locally_free: vec![(level % 251) as u8],
            connective_used: false,
            cost_region: None,
        };
        let send = arena
            .send_component(&shell, &[planned])
            .expect("plan Send level");
        planned = arena
            .singleton_component(send, Vec::new(), false)
            .expect("plan Send carrier");
    }
    finish(arena, planned, "finish Send fixture")
}

fn planned_gint(arena: &mut ParConstruction, value: i64) -> PlannedPar {
    import(
        arena,
        new_gint_par(value, Vec::new(), false),
        "plan EPathMap key",
    )
}

pub(crate) fn nested_pathmap_chain_leaf(depth: usize, leaf: i64) -> Par {
    let mut arena = ParConstruction::new();
    let mut planned = planned_gint(&mut arena, leaf);
    for level in 0..depth {
        let key = planned_gint(&mut arena, level as i64);
        let map = arena
            .epathmap_expr_component(
                EPathMapMode::Map,
                &[key, planned],
                vec![(level % 251) as u8],
                false,
                None,
            )
            .expect("plan EPathMap map level");
        planned = arena
            .singleton_component(map, Vec::new(), false)
            .expect("plan EPathMap carrier");
    }
    finish(arena, planned, "finish EPathMap map fixture")
}

pub(crate) fn nested_pathmap_match_pattern(depth: usize) -> Par {
    let mut arena = ParConstruction::new();
    let mut planned = import(
        &mut arena,
        new_freevar_par(0, Vec::new()),
        "plan EPathMap free-variable terminal",
    );
    for level in 0..depth {
        let key = planned_gint(&mut arena, level as i64);
        let map = arena
            .epathmap_expr_component(EPathMapMode::Map, &[key, planned], Vec::new(), true, None)
            .expect("plan EPathMap pattern level");
        planned = arena
            .singleton_component(map, Vec::new(), true)
            .expect("plan EPathMap pattern carrier");
    }
    finish(arena, planned, "finish EPathMap pattern fixture")
}

pub(crate) fn list_pattern_depth(par: &Par) -> usize {
    let mut observed = None;
    par.visit_encoded::<Expr>(|entry| {
        if observed.is_some() || entry.multiplicity().get() != 1 {
            observed = Some(0);
            return;
        }
        let mut depth = 0usize;
        let mut expr = entry.payload();
        loop {
            let Some(CanonicalExprParts::Sequence {
                tag: 20,
                children,
                remainder: None,
            }) = canonical_expr_parts(expr)
            else {
                observed = Some(0);
                return;
            };
            let [child] = children.as_slice() else {
                observed = Some(0);
                return;
            };
            depth += 1;
            if canonical_exact_free_var_par(child).is_some_and(|free| free.level() == 0) {
                observed = Some(depth);
                return;
            }
            let Some(next) = canonical_single_expr_from_par(child) else {
                observed = Some(0);
                return;
            };
            expr = next;
        }
    });
    observed.unwrap_or(0)
}

pub(crate) fn enot_depth(par: &Par) -> usize {
    let mut observed = None;
    par.visit_encoded::<Expr>(|entry| {
        let Some((depth, terminal)) = entry.canonical_unary_enot_parts() else {
            observed = Some(0);
            return;
        };
        let valid_terminal = canonical_exact_scalar_par(terminal)
            .and_then(|scalar| canonical_expr_oneof_tag(scalar.expr()))
            == Some(1);
        observed = Some(if valid_terminal { depth } else { 0 });
    });
    observed.unwrap_or(0)
}

pub(crate) fn eset_depth(par: &Par) -> usize {
    let mut observed = None;
    par.visit_encoded::<Expr>(|entry| {
        let mut depth = 0usize;
        let mut expr = entry.payload();
        loop {
            let Some(CanonicalExprParts::Sequence {
                tag: 22,
                children,
                remainder: None,
            }) = canonical_expr_parts(expr)
            else {
                observed = Some(0);
                return;
            };
            let [child] = children.as_slice() else {
                observed = Some(0);
                return;
            };
            depth += 1;
            if canonical_exact_scalar_par(child)
                .and_then(|scalar| canonical_expr_oneof_tag(scalar.expr()))
                == Some(2)
            {
                observed = Some(depth);
                return;
            }
            let Some(next) = canonical_single_expr_from_par(child) else {
                observed = Some(0);
                return;
            };
            expr = next;
        }
    });
    observed.unwrap_or(0)
}

pub(crate) fn emap_depth(par: &Par) -> usize {
    let one = new_gint_par(1, Vec::new(), false)
        .protobuf_bytes()
        .expect("encode EMap witness value");
    let mut observed = None;
    par.visit_encoded::<Expr>(|entry| {
        let mut depth = 0usize;
        let mut expr = entry.payload();
        loop {
            let Some(CanonicalExprParts::Map {
                entries,
                remainder: None,
            }) = canonical_expr_parts(expr)
            else {
                observed = Some(0);
                return;
            };
            let [(key, value)] = entries.as_slice() else {
                observed = Some(0);
                return;
            };
            if *value != one.as_slice() {
                observed = Some(0);
                return;
            }
            depth += 1;
            if canonical_exact_scalar_par(key)
                .and_then(|scalar| canonical_expr_oneof_tag(scalar.expr()))
                == Some(2)
            {
                observed = Some(depth);
                return;
            }
            let Some(next) = canonical_single_expr_from_par(key) else {
                observed = Some(0);
                return;
            };
            expr = next;
        }
    });
    observed.unwrap_or(0)
}

pub(crate) fn binder_depth(par: &Par) -> usize {
    let mut observed = None;
    par.visit_encoded::<New>(|entry| {
        let mut depth = 0usize;
        let mut new_body = entry.payload();
        loop {
            let Some(new) = canonical_new_parts(new_body) else {
                observed = Some(0);
                return;
            };
            let Some(receive_body) = canonical_single_receive_from_par(new.body) else {
                observed = Some(0);
                return;
            };
            let Some(receive) = canonical_receive_parts(receive_body) else {
                observed = Some(0);
                return;
            };
            if receive.binds.len() != 1 || receive.persistent {
                observed = Some(0);
                return;
            }
            depth += 1;
            if canonical_exact_scalar_par(receive.body).is_some() {
                observed = Some(depth);
                return;
            }
            let Some(next) = canonical_single_new_from_par(receive.body) else {
                observed = Some(0);
                return;
            };
            new_body = next;
        }
    });
    observed.unwrap_or(0)
}

pub(crate) fn send_chain_depth(par: &Par) -> usize {
    let mut observed = None;
    par.visit_encoded::<Send>(|entry| {
        let mut depth = 0usize;
        let mut send_body = entry.payload();
        loop {
            let Some(send) = canonical_send_parts(send_body) else {
                observed = Some(0);
                return;
            };
            if !send.data.is_empty() || send.persistent {
                observed = Some(0);
                return;
            }
            depth += 1;
            if canonical_exact_scalar_par(send.channel).is_some() {
                observed = Some(depth);
                return;
            }
            let Some(next) = canonical_single_send_from_par(send.channel) else {
                observed = Some(0);
                return;
            };
            send_body = next;
        }
    });
    observed.unwrap_or(0)
}

pub(crate) fn pathmap_chain_depth(par: &Par) -> usize {
    let mut observed = None;
    par.visit_encoded::<Expr>(|entry| {
        observed = entry
            .canonical_singleton_epathmap_map_parts()
            .map(|(depth, _terminal)| depth);
    });
    observed.unwrap_or(0)
}
