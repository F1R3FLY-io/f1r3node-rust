use std::collections::BTreeMap;

use models::create_bit_vector;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{Bundle, EPlus, Expr, GPrivate, GUnforgeable, MatchCase, Par, ReceiveBind};
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{
    new_boundvar_par, new_gint_par, new_match_par, new_new_par, new_receive_par, new_send_par,
};

use super::{gprivate_bytes_if_bare, is_scope_trapped};

fn bv(index: i32) -> Par { new_boundvar_par(index, create_bit_vector(&[index as usize]), false) }

fn int(i: i64) -> Par { new_gint_par(i, Vec::new(), false) }

/// `new x in { x!(5) }` — the fresh unforgeable at body index 0 is trapped.
#[test]
fn trapped_pure_send_channel() {
    let send = new_send_par(
        bv(0),
        vec![int(5)],
        false,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(is_scope_trapped(0, &send));
}

/// `new x in { for (y <- x) { Nil } }` — fresh unforgeable at body index 0
/// trapped (channel-only use).
#[test]
fn trapped_pure_receive_channel() {
    let bind = ReceiveBind {
        patterns: vec![Par::default()],
        source: Some(bv(0)),
        remainder: None,
        free_count: 1,
    };
    let body = Par::default();
    let recv = new_receive_par(
        vec![bind],
        body,
        false,
        false,
        1,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(is_scope_trapped(0, &recv));
}

/// `new x in { @1!(*x) }` — the fresh unforgeable at body index 0 is not
/// trapped; `*x` in Send.data leaks it.
#[test]
fn unsafe_leak_via_data() {
    let chan = int(1);
    let send = new_send_par(
        chan,
        vec![bv(0)],
        false,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(!is_scope_trapped(0, &send));
}

/// A send whose chan is `bundle+{x}` is still a channel use — trapped.
#[test]
fn trapped_under_bundle_chan() {
    let bundle = Par::default().with_bundles(vec![Bundle {
        body: Some(bv(0)),
        write_flag: true,
        read_flag: false,
    }]);
    let mut bundle_with_free = bundle;
    bundle_with_free.locally_free = create_bit_vector(&[0]);
    let send = new_send_par(
        bundle_with_free,
        vec![int(5)],
        false,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(is_scope_trapped(0, &send));
}

/// `for (@*x <- @"ch") { Nil }` — outer x appears in a Receive pattern,
/// so not trapped.
#[test]
fn unsafe_in_receive_pattern() {
    let pat = {
        let mut p = bv(0);
        p.locally_free = create_bit_vector(&[0]);
        p
    };
    let bind = ReceiveBind {
        patterns: vec![pat],
        source: Some(int(42)),
        remainder: None,
        free_count: 0,
    };
    let recv = new_receive_par(
        vec![bind],
        Par::default(),
        false,
        false,
        0,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(!is_scope_trapped(0, &recv));
}

/// `match *x { Nil => Nil }` — outer x in Match.target → not trapped.
#[test]
fn unsafe_in_match_target() {
    let case = MatchCase {
        pattern: Some(Par::default()),
        source: Some(Par::default()),
        free_count: 0,
        guard: None,
    };
    let par = new_match_par(
        bv(0),
        vec![case],
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(!is_scope_trapped(0, &par));
}

/// `match y { =*x => Nil }` — outer x in a Match case pattern → not trapped.
#[test]
fn unsafe_in_match_pattern() {
    let case = MatchCase {
        pattern: Some({
            let mut p = bv(0);
            p.locally_free = create_bit_vector(&[0]);
            p
        }),
        source: Some(Par::default()),
        free_count: 0,
        guard: None,
    };
    let par = new_match_par(
        int(7),
        vec![case],
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(!is_scope_trapped(0, &par));
}

/// Expression context: `@"u"!( *x + 1 )`. Outer x in an EPlus → not trapped.
#[test]
fn unsafe_in_expr() {
    let eplus = Expr {
        expr_instance: Some(ExprInstance::EPlusBody(EPlus {
            p1: Some(bv(0)),
            p2: Some(int(1)),
        })),
    };
    let payload = vector_par(create_bit_vector(&[0]), false).with_exprs(vec![eplus]);
    let send = new_send_par(
        int(42),
        vec![payload],
        false,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(!is_scope_trapped(0, &send));
}

/// `new x in { new y in { x!(6) | y!(7) } }` — outer x only in inner
/// send's chan. Trapped (depth shift handled).
#[test]
fn nested_new_shift() {
    let send_outer = new_send_par(
        bv(1),
        vec![int(6)],
        false,
        create_bit_vector(&[1]),
        false,
        create_bit_vector(&[1]),
        false,
    );
    let send_inner = new_send_par(
        bv(0),
        vec![int(7)],
        false,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    let mut combined = send_outer;
    combined.sends.extend(send_inner.sends);
    combined.locally_free = create_bit_vector(&[0, 1]);
    let new_body_par = new_new_par(
        1,
        combined,
        Vec::new(),
        BTreeMap::new(),
        create_bit_vector(&[0]),
        create_bit_vector(&[0]),
        false,
    );
    assert!(is_scope_trapped(0, &new_body_par));
}

/// `new x in { new y in { y!(*x) } }` — outer x leaks through inner send's
/// data. Not trapped.
#[test]
fn nested_new_leaks_x() {
    let send_inner = new_send_par(
        bv(0),
        vec![bv(1)],
        false,
        create_bit_vector(&[0, 1]),
        false,
        create_bit_vector(&[0, 1]),
        false,
    );
    let new_body_par = new_new_par(
        1,
        send_inner,
        Vec::new(),
        BTreeMap::new(),
        create_bit_vector(&[0]),
        create_bit_vector(&[0]),
        false,
    );
    assert!(!is_scope_trapped(0, &new_body_par));
}

/// Outer x inside a receive body (even when the receive source is x
/// itself) disqualifies trappedness under the strictest-safe rule: a
/// persistent receive on x could fire from an inside-closure send and
/// the body could emit a further-reaching reference via a nested
/// receive on a non-trapped channel.
#[test]
fn receive_body_mentioning_x_is_rejected() {
    // new x in { for (@v <- x) { x!(v) } }
    let inner_send = new_send_par(
        bv(1),
        vec![int(0)],
        false,
        create_bit_vector(&[1]),
        false,
        create_bit_vector(&[1]),
        false,
    );
    let bind = ReceiveBind {
        patterns: vec![Par::default()],
        source: Some(bv(0)),
        remainder: None,
        free_count: 1,
    };
    let recv = new_receive_par(
        vec![bind],
        inner_send,
        false,
        false,
        1,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(!is_scope_trapped(0, &recv));
}

/// Outer x inside a receive body whose SOURCE is a different channel —
/// the receive is parked on that other channel, so an external send on
/// it fires the body and leaks x. Must NOT be classified as trapped.
#[test]
fn receive_body_on_other_channel_leaks_x() {
    // new x in { for (@v <- y /* NOT x */) { x!(v) } }
    // In a one-binder scope x is BoundVar(0); `y` here is a forgeable
    // channel modeled as a GInt.
    let inner_send = new_send_par(
        bv(1),
        vec![int(0)],
        false,
        create_bit_vector(&[1]),
        false,
        create_bit_vector(&[1]),
        false,
    );
    let bind = ReceiveBind {
        patterns: vec![Par::default()],
        source: Some(int(42)),
        remainder: None,
        free_count: 1,
    };
    let recv = new_receive_par(
        vec![bind],
        inner_send,
        false,
        false,
        1,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(!is_scope_trapped(0, &recv));
}

/// Join: outer x used only as one source of a `for (a <- x; b <- y) { Nil }`
/// is trapped.
#[test]
fn trapped_in_join() {
    let bind_x = ReceiveBind {
        patterns: vec![Par::default()],
        source: Some(bv(0)),
        remainder: None,
        free_count: 1,
    };
    let bind_y = ReceiveBind {
        patterns: vec![Par::default()],
        source: Some(int(99)),
        remainder: None,
        free_count: 1,
    };
    let recv = new_receive_par(
        vec![bind_x, bind_y],
        Par::default(),
        false,
        false,
        2,
        create_bit_vector(&[0]),
        false,
        create_bit_vector(&[0]),
        false,
    );
    assert!(is_scope_trapped(0, &recv));
}

#[test]
fn gprivate_bytes_identifies_bare_gprivate() {
    let bare = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![1, 2, 3] })),
    }]);
    assert_eq!(gprivate_bytes_if_bare(&bare), Some(&[1u8, 2, 3][..]));
}

#[test]
fn gprivate_bytes_rejects_non_bare() {
    let not_bare = new_send_par(
        Par::default(),
        vec![],
        false,
        Vec::new(),
        false,
        Vec::new(),
        false,
    );
    assert_eq!(gprivate_bytes_if_bare(&not_bare), None);
}
