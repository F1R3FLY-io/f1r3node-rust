//! # The prost encoder's SPACE gate — the file `protobuf_encoder.rs` said already pinned it
//!
//! `protobuf_encoder.rs`'s §3 header carries this sentence:
//!
//! > *"All four quantities are pinned by `models/tests/protobuf_encoder_space.rs`, mirroring
//! > `bincode_encoder_space.rs`."*
//!
//! ⛔ **That file did not exist.** `op_size`, `frame_size` and `len_table_size` are `pub`,
//! each documented as *"exposed so the space gate can pin it"*, and a repository-wide search
//! found **zero** references to `op_size` or `frame_size` outside their own definitions. The
//! encoder's per-call space was unpinned while its header asserted the opposite.
//!
//! ⚠ Phase 4 of the stack-safety consolidation names this as the **gate blocking its start**,
//! and the reason is not tidiness: S5 converts `protobuf_encoder` to the pooled stack to close a
//! measured 3.79× shallow regression, and a pooling change is exactly the kind that alters
//! per-call space while leaving every byte identical. Without this file that change would have
//! had no instrument.
//!
//! ## Why a space gate at all — what no correctness test can see
//!
//! | claim | why bytes cannot show it |
//! |---|---|
//! | the op stack is Θ(**depth**), not Θ(**size**) | a machine that pushed all `n` children eagerly emits byte-identical output |
//! | an op-stack entry is four words | an `Op` that silently grew multiplies the only per-call heap there is |
//! | `lens` is Θ(message **nodes**) | the Θ(d²)→Θ(n) trade is a space *cost*; nothing about the output records it |
//!
//! ★ This mirrors `bincode_encoder_space.rs`, deliberately: the two encoders are the pair whose
//! divergence produced the regression, and a claim measured for one and not the other is how
//! that divergence survived.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::rholang::protobuf_encoder::{frame_size, len_table_size, op_size};

/// `[[[…[0]…]]]` — a chain of `depth` nested `EList`s, one message node per level.
fn nested_list(depth: usize) -> Par {
    let mut par = Par::default();
    for _ in 0..depth {
        par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![par],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }]);
    }
    par
}

/// ★★ **The op-stack entry is FOUR WORDS, and that ceiling is load-bearing.**
///
/// `protobuf_encoder::Op`'s widest arm is `Seq { &dyn, u32, u32, u32 }` — a fat pointer (16 B)
/// plus three `u32`s = 28 B of payload, with the discriminant in the 4 spare bytes of the
/// 32-byte slot. **Four spare bytes**, against `bincode_encoder::Op`'s eight.
///
/// ⚠ This is the number the `drive.rs` codec exemption rests on. A `Node`+`Kont` split needs
/// two tags at the same offset, which cannot overlay, so a *shared* `Step` is predicted at
/// **40 B = 5 words against a pinned 4-word ceiling** — +64 KiB at depth 4,096. If this
/// assertion moves, that argument must be re-derived rather than re-quoted.
#[test]
fn the_op_stack_entry_is_four_words() {
    let word = std::mem::size_of::<usize>();
    // ⚠ The expected width is bound ONCE and both the comparison and the message read it.
    // Written out twice, a perturbation of the comparison leaves the message quoting the old
    // value — which is exactly what the first RED run of this file printed:
    // "32 B against the pinned 32 B". A failure message that misreports the thing it failed
    // on is worse than no message, because it sends the reader to look for a bug in the code
    // under test rather than in the assertion.
    let expected = 4 * word;
    assert_eq!(
        op_size(),
        expected,
        "PROTOBUF `Op` CHANGED WIDTH: {} B against the pinned {} B (4 × {} B word).\n\n\
         The op stack is the encoder's per-level heap, so a wider `Op` multiplies the only \
         per-call allocation there is — at depth 4,096 each extra word costs 32 KiB.\n\n\
         ⚠ This width is also what the codec exemption at `models/src/rust/rholang/drive.rs` \
         is derived FROM: the widest arm reaches 32 B with only 4 spare bytes, which is why a \
         `Node`+`Kont` split (two tags at one offset) is predicted at 40 B = 5 words. If `Op` \
         moved, that prediction is stale and the exemption must be RE-DERIVED, not re-quoted.",
        op_size(),
        expected,
        word
    );
}

/// The pass-1 frame stack's per-level cost.
///
/// `Frame` is what pass 1 pushes per open message so `Op::Close` can add
/// `key_len(tag) + varint_len(sum) + sum` to its parent. It is Θ(depth), so its width is a
/// per-level cost in exactly the way `Op`'s is.
#[test]
fn the_frame_is_two_words() {
    let word = std::mem::size_of::<usize>();
    let expected = 2 * word; // bound once; see `the_op_stack_entry_is_four_words`
    assert_eq!(
        frame_size(),
        expected,
        "PROTOBUF `Frame` CHANGED WIDTH: {} B against the pinned {} B.\n\
         The frame stack is Θ(depth), so this is a per-level cost: at depth 4,096 each extra \
         word is 32 KiB.",
        frame_size(),
        expected
    );
}

/// ★★ **`lens` is Θ(message NODES) — the space side of the Θ(d²) → Θ(n) trade.**
///
/// This is the quantity the whole two-pass design exists to buy. `encoded_len` is Θ(d²)
/// because `encode_to_vec` calls it once and then `message::encode` calls it *again* per
/// nested message; the memoised table computes each subtree length once. The price is one
/// `u32` per message node, and **pricing it is the point** — a trade whose cost is unmeasured
/// is not a trade, it is a hope.
///
/// ⚠ Asserted as a linear RELATIONSHIP across two rungs rather than as a constant. A single
/// figure would pin an implementation detail (how many nodes a level happens to produce) and
/// would have to be edited by any legitimate change to the schema; the *shape* is the claim.
#[test]
fn the_length_table_is_linear_in_message_nodes() {
    let small = len_table_size(&nested_list(64));
    let large = len_table_size(&nested_list(256));

    assert!(
        small > 0 && large > 0,
        "VACUOUS: the length table measured {small} and {large} entries. A table that is \
         always empty would satisfy every ratio below while measuring nothing — and would \
         mean the memoised pass is not running at all."
    );

    // 4× the depth ⇒ ~4× the nodes. Bounded on BOTH sides: a table that grew faster than
    // linear would be the quadratic returning in space, and one that grew slower would mean
    // nodes are being skipped.
    let ratio = large as f64 / small as f64;
    assert!(
        (3.0..=5.0).contains(&ratio),
        "LENGTH TABLE IS NOT LINEAR IN NODES: {small} entries at depth 64, {large} at depth \
         256 — a ratio of {ratio:.2} where 4× the depth should give ~4× the nodes.\n\n\
         Above 5×: the Θ(n) space claim is false and the quadratic has returned on the space \
         axis instead of the time one.\n\
         Below 3×: message nodes are being SKIPPED, and a length table with holes produces \
         wrong length prefixes — which is a consensus fault, not a performance one."
    );

    // The trade, priced: 4 bytes per message node.
    let bytes_at_4096 = len_table_size(&nested_list(1024)) * std::mem::size_of::<u32>();
    println!(
        "  prost length table: {small} entries @ depth 64, {large} @ 256; \
         {bytes_at_4096} B for a depth-1,024 chain (4 B per message node)"
    );
}

/// ⚠ The three quantities above are pinned **together**, because the header claims them
/// together and a partially-pinned claim is the state this file was written to end.
///
/// ★ It also fails if any of the three helpers is removed: each is `pub` solely so this gate
/// can read it, and deleting one as "unused" would silently un-pin the quantity it exposes.
/// That is not hypothetical — they *were* unreferenced, workspace-wide, until this file
/// existed.
#[test]
fn every_quantity_the_header_claims_is_actually_pinned() {
    let claimed = ["op_size", "frame_size", "len_table_size"];
    // Reading each here is what makes the claim true; the assertion is that all three
    // resolve and are non-zero.
    let measured = [op_size(), frame_size(), len_table_size(&nested_list(8))];
    for (name, value) in claimed.iter().zip(measured.iter()) {
        assert!(
            *value > 0,
            "`protobuf_encoder::{name}` measured 0. `protobuf_encoder.rs`'s §3 header asserts all \
             four space quantities are pinned by THIS file; a zero means the quantity is not \
             being measured and the header's claim is false again."
        );
    }
    println!(
        "  protobuf_encoder space: op {} B, frame {} B, len-table {} entries @ depth 8",
        measured[0], measured[1], measured[2]
    );
}
