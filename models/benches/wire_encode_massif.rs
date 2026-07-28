//! # The steady-state HEAP profile — one arm per run, for `massif`
//!
//! `models/tests/wire_encode_space.rs` counts allocations exactly, per call,
//! with a per-thread counting allocator. That answers *"does a warm encode
//! allocate?"*. This binary answers the other half — *"how much heap is live
//! while it runs, and does the high-water mark come back down?"* — because a
//! codec can allocate nothing per call and still hold a pathological peak.
//!
//! One arm per run so the profiles do not superimpose:
//!
//! ```text
//!   MASSIF_ARM=derived  valgrind --tool=massif --time-unit=B …   the oracle
//!   MASSIF_ARM=machine  valgrind --tool=massif --time-unit=B …   owned Vec
//!   MASSIF_ARM=reused   valgrind --tool=massif --time-unit=B …   reused buffer
//!   MASSIF_ARM=deep     valgrind --tool=massif --time-unit=B …   the op stack alone
//! ```
//!
//! `deep` is separated deliberately: the encoder's op stack and the DECODER's
//! eighteen value stacks are different mechanisms with different growth, and
//! averaging them would hide whichever is worse.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, ListParWithRandom, Par, Send};
use models::rust::rholang::wire_encode::{encode, with_encoded};

fn gint(n: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        }],
        ..Default::default()
    }
}

/// The production shape: depth 2, which is 95.43% of measured produces.
fn datum() -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![gint(1), gint(2), gint(3)],
                    locally_free: vec![],
                    connective_used: false,
                    remainder: None,
                })),
            }],
            sends: vec![Send {
                chan: Some(gint(9)),
                data: vec![gint(4)],
                persistent: false,
                locally_free: vec![],
                connective_used: false,
            }],
            ..Default::default()
        }],
        random_state: vec![0xa5; 32],
    }
}

fn deep(depth: usize) -> Par {
    let mut par = Par::default();
    for _ in 0..depth {
        par = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![par],
                    ..Default::default()
                })),
            }],
            ..Default::default()
        };
    }
    par
}

fn main() {
    let arm = std::env::var("MASSIF_ARM").unwrap_or_else(|_| "machine".into());
    // Enough iterations that the steady state dominates the profile, few
    // enough that massif stays fast under valgrind.
    const ITERS: usize = 20_000;
    let value = datum();
    let mut sink = 0usize;
    match arm.as_str() {
        "derived" => {
            for _ in 0..ITERS {
                sink += bincode::serialize(&value).expect("oracle").len();
            }
        }
        "machine" => {
            for _ in 0..ITERS {
                sink += encode(&value).len();
            }
        }
        "reused" => {
            for _ in 0..ITERS {
                sink += with_encoded(&value, <[u8]>::len);
            }
        }
        "deep" => {
            // The op stack ALONE: one 4,096-deep term, encoded repeatedly into
            // the reused buffer, so the only growing structure is the op stack.
            let term = deep(4096);
            for _ in 0..64 {
                sink += with_encoded(&term, <[u8]>::len);
            }
            std::mem::forget(term);
        }
        // ⚠ THE DECODER'S VALUE STACKS, measured SEPARATELY.
        //
        // The encoder's op stack and `par_codec`'s eighteen per-type value
        // stacks are different mechanisms with different growth: the encoder
        // walks top-down over borrows and builds nothing, while the decoder
        // reassembles bottom-up and must park every completed child until its
        // parent is ready. Averaging the two would hide whichever is worse, so
        // this arm exists to be read against `deep`.
        "decode" => {
            use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
            let term = deep(4096);
            let bytes = with_encoded(&term, <[u8]>::to_vec);
            std::mem::forget(term);
            for _ in 0..8 {
                let decoded = Par::cold_decode(&bytes).expect("decode");
                sink += decoded.exprs.len();
                std::mem::forget(decoded);
            }
        }
        other => panic!("unknown MASSIF_ARM `{other}`"),
    }
    println!("{arm}: {sink}");
}
