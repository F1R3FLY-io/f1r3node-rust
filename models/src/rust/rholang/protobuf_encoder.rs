//! # `protobuf_encoder` — the Θ(depth)-native-stack, Θ(n)-work protobuf encoder
//!
//! The protobuf twin of [`crate::rust::rholang::bincode_encoder`]. Same op-stack
//! discipline, same generated-table split, same tail-call rule — and one
//! structural difference that changes the *complexity class*, not merely the
//! stack shape.
//!
//! ⚠ **Nothing in production calls this yet.** It is byte-for-byte equivalent to
//! the derived path (`models/tests/protobuf_encoder_differential.rs`) and is not
//! wired into any call site; migrating them is a later stage's deliverable.
//!
//! ---
//!
//! ## 1. ★★ Why protobuf needs TWO passes where bincode needed one
//!
//! bincode legacy has **no length prefixes**: a struct is its fields,
//! positionally, and a nested value is written in place. So emission order is
//! exactly pre-order and a single walk suffices — which is what makes
//! `bincode_encoder` one pass.
//!
//! Protobuf prefixes **every** nested message with its own length. Writing a
//! child's key therefore requires knowing the child's encoded size *before*
//! writing the child. There are only two ways to arrange that, and prost picks
//! the expensive one:
//!
//! ```text
//!                              work on a depth-d chain
//!   ──────────────────────────  ───────────────────────
//!   prost's derived encoder      Θ(d²)   ← re-measures every subtree
//!   this module                  Θ(n)    ← measures each node ONCE
//! ```
//!
//! `Message::encode_to_vec` (`prost-0.14.3/src/message.rs:61-69`) calls
//! `self.encoded_len()` — a complete recursive walk — and then `encode_raw`.
//! And `encoding::message::encode` (`encoding.rs:788-795`) calls
//! `msg.encoded_len()` **again, for every nested message it writes**:
//!
//! ```text
//!   pub fn encode<M>(tag: u32, msg: &M, buf: &mut impl BufMut) where M: Message {
//!       encode_key(tag, WireType::LengthDelimited, buf);
//!       encode_varint(msg.encoded_len() as u64, buf);   ← a FULL walk of the subtree
//!       msg.encode_raw(buf);
//!   }
//! ```
//!
//! So a node at depth `` $k$ `` of a `` $d$ ``-deep chain has its subtree
//! measured once by its parent, once by *its* parent, and so on: the total is
//!
//! ```math
//! \sum_{v} \big|\mathrm{subtree}(v)\big| \;=\; \Theta(d^2)
//! ```
//!
//! for a chain, and `` $\Theta(n \cdot d)$ `` in general. A memoized bottom-up
//! length pass makes it `` $\Theta(n)$ ``, which is why
//! [`encoded_len`] is worth having *on its own*, separately from
//! [`encode_to_vec`].
//!
//! ## 2. The machine
//!
//! ```text
//!   PASS 1 — LENGTHS, bottom-up, memoized
//!   ┌──────────────────────────────────────────────────────────────────┐
//!   │ lens : Vec<u32>       pre-order id → that node's encoded_len     │  Θ(nodes)
//!   │ frames : Vec<Frame>   one per OPEN ancestor  { id, tag, sum }    │  Θ(depth)
//!   │ ops : Vec<Op>         the obligation stack                       │  Θ(depth)
//!   └──────────────────────────────────────────────────────────────────┘
//!                    │  Op::Close pops a frame and adds
//!                    │  key_len(tag) + varint_len(sum) + sum  to its PARENT
//!                    ▼
//!   PASS 2 — EMIT, top-down, ONE monotonic cursor into `lens`
//!   ┌──────────────────────────────────────────────────────────────────┐
//!   │ cursor : usize        starts at 1 (id 0 is the root)             │
//!   │ ops : Vec<Op>         the same discipline, no frames needed      │  Θ(depth)
//!   └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ### ★★ Why a single cursor is sufficient, and why that is not a coincidence
//!
//! Pass 1 assigns ids **in descent order**, which is pre-order. Pass 2 descends
//! in the same order, through the same generated field walk, the same counted
//! repeat and the same `BTreeMap` iterator. So the sequence of nodes at which
//! pass 2 must write a length prefix is *exactly* the sequence in which pass 1
//! allocated ids, and a monotonically increasing index into `lens` resolves
//! every one of them. No child table, no id on the op stack, no map.
//!
//! ⚠ That makes agreement between the passes **structural rather than
//! asserted**: any divergence in visit order — a field reordered in one pass, a
//! sequence walked backwards, a map iterated differently — shifts the cursor and
//! shows up immediately as a wrong length prefix, which the differential catches
//! on its first case. [`Machine::finish_emit`] additionally requires the cursor
//! to have consumed the table *exactly*, so a divergence that happened to
//! cancel out is caught too.
//!
//! ## 3. Space
//!
//! | structure | bound | at depth 4,096 |
//! |---|---|---|
//! | `ops` (both passes) | Θ(depth) | ~2 entries/level × 24 B |
//! | `frames` (pass 1) | Θ(depth) | 16 B/level |
//! | `lens` | Θ(message nodes) × 4 B | ~48 KiB for a 12,288-node chain |
//!
//! `lens` is the price of the `` $\Theta(d^2) \to \Theta(n)$ `` trade and it is
//! `` $\Theta(n)$ `` **space** where prost's is `` $O(1)$ ``. That is the whole
//! trade: 4 bytes per message node buys the elimination of a quadratic. All four
//! quantities are pinned by `models/tests/protobuf_encoder_space.rs`, mirroring
//! `bincode_encoder_space.rs`.
//!
//! ## 4. ⚠ `EPathMap` is an OPAQUE LEAF, and that is a NAMED RESIDUAL
//!
//! `EPathMap::encode_raw` has three arms — a `memcpy` of interned canonical
//! bytes, the ground field-8 `` $U(m)$ `` form, and the ordinary field walk — of
//! which only the last is a field walk at all, and which one fires depends on a
//! `OnceLock` another thread may fill. Decomposing it into field descents would
//! silently drop two of the three arms, so both passes intercept it through
//! [`crate::rust::rholang::prost_wire::ProstNode::prost_opaque`] and treat it as
//! ONE node: `opaque_encoded_len()` for the prefix, `opaque_encode_raw()` for
//! the body — exact parity with what `prost::encoding::message::encode` does at
//! that position.
//!
//! ★ **Correct, and not depth-independent, are two separate statements.** A term
//! nested through an `EPathMap` recurses inside `EPathMap::encode_raw` exactly as
//! the derived path does. The bytes are identical (the differential's
//! `deep_mixed_par` cycles through an `EPathMap` every eighth level); the native
//! stack is not bounded through that one shape. It is named here rather than
//! left for a stack trace to report.

use std::collections::{btree_map, BTreeMap};

use prost::encoding::{encode_key, encode_varint, encoded_len_varint, key_len, WireType};

use crate::rhoapi::Par;
use crate::rust::rholang::prost_wire::{ProstDescent, ProstNode, ProstSeq, NO_RESUME};

// ===========================================================================
// §A  The opcode alphabet
// ===========================================================================

/// One suspended obligation. Shared by both passes — the traversal is the same
/// traversal, and giving each pass its own opcode set would be two places for
/// the visit order to live.
#[derive(Clone, Copy)]
enum Op<'a> {
    /// Run fields `[field..]` of `node`.
    Node { node: &'a dyn ProstNode, field: u16 },
    /// Run elements `[index..]` of `seq`, all under `tag`.
    Seq {
        seq: &'a dyn ProstSeq,
        /// ⚠ `u32`, not `usize`. A sequence with more than 4,294,967,295
        /// elements cannot exist in memory (each element is far more than one
        /// byte), and [`Machine::open_seq`] refuses one rather than truncating
        /// the cursor — a truncated cursor emits fewer elements than the length
        /// prefix promises, which is a corrupt encoding rather than a slow one.
        index: u32,
        len: u32,
        tag: u32,
    },
    /// Run the remaining entries of the `BTreeMap` iterator on top of
    /// `map_iters`, all under `tag`.
    MapEntries { tag: u32 },
    /// ⚠ PASS 1 ONLY. Close the top frame: its length is final, so record it and
    /// hand its contribution to its parent.
    Close,
}

/// One OPEN node in pass 1.
///
/// `sum` accumulates this node's own encoded length: the bounded fields the
/// generated walk measured, plus one contribution per child as that child
/// closes.
struct Frame {
    /// The node's pre-order id — its slot in `lens`.
    id: u32,
    /// The tag this node sits under in its PARENT, so [`Machine::close`] can
    /// compute the contribution without asking anyone. `0` marks the root,
    /// which sits under no tag at all.
    tag: u32,
    sum: u64,
}

/// The sentinel `tag` of the root frame.
///
/// Protobuf tags start at 1, so 0 cannot collide with a real one — and the
/// generator refuses a zero tag outright, so this is a guarantee rather than a
/// convention.
const ROOT_TAG: u32 = 0;

/// Preallocated op-stack capacity, matching `bincode_encoder`'s.
const OP_STACK_CAPACITY: usize = 64;

/// Preallocated `lens` capacity. ⚠ A *starting point*, not a bound: the table is
/// Θ(message nodes) and grows geometrically past this.
const LEN_TABLE_CAPACITY: usize = 256;

/// Initial capacity of a fresh output buffer.
const OUT_CAPACITY: usize = 4096;

// ===========================================================================
// §B  Pass 1 — lengths, bottom-up, memoized
// ===========================================================================

struct LenMachine<'a> {
    ops: Vec<Op<'a>>,
    frames: Vec<Frame>,
    lens: Vec<u32>,
    map_iters: Vec<btree_map::Iter<'a, String, Par>>,
    /// The value every map entry's value is compared against for skip-at-default.
    ///
    /// ⚠★ Constructed ONCE and compared with `==`, which goes through the
    /// **hand-written** `PartialEq` in `models/src/lib.rs` — and that impl
    /// deliberately ignores `locally_free`. So a `Par` carrying only
    /// `locally_free` IS skipped, exactly as prost's
    /// `encode_with_default` skips it (`encoding.rs:1046`). Restating the
    /// predicate as "is it structurally empty?" would be a second opinion and
    /// would differ on precisely that value.
    default_par: Par,
    high_water: usize,
}

impl<'a> LenMachine<'a> {
    fn new() -> Self {
        LenMachine {
            ops: Vec::with_capacity(OP_STACK_CAPACITY),
            frames: Vec::with_capacity(OP_STACK_CAPACITY),
            lens: Vec::with_capacity(LEN_TABLE_CAPACITY),
            map_iters: Vec::new(),
            default_par: Par::default(),
            high_water: 0,
        }
    }

    /// Allocate `node`'s pre-order id, open its frame, and schedule its walk.
    ///
    /// ★ The id is allocated HERE, at the moment of descent, which is what makes
    /// the id order pre-order and lets pass 2 resolve every length with one
    /// monotonic cursor. See the module header §2.
    fn open(&mut self, node: &'a dyn ProstNode, tag: u32) {
        let id = self.lens.len();
        assert!(
            id <= u32::MAX as usize,
            "protobuf_encoder: more than {} message nodes in one term. The length table is indexed \
             by a u32; refusing is correct because a truncated index would silently attach one \
             node's length to another.",
            u32::MAX
        );
        self.lens.push(0);
        self.frames.push(Frame {
            id: id as u32,
            tag,
            sum: 0,
        });
        // `Close` is pushed FIRST so it pops LAST — beneath every op the node's
        // subtree will push. That is what makes "the subtree is complete" an
        // observable event without a virtual call to ask.
        self.ops.push(Op::Close);
        self.ops.push(Op::Node { node, field: 0 });
    }

    /// Open a synthetic protobuf MAP ENTRY.
    ///
    /// ⚠ A protobuf map is **not** a repeated value. Each pair is a
    /// length-delimited entry message with the key at tag 1 and the value at
    /// tag 2, and BOTH are skipped at their defaults — `prost-0.14.3/
    /// src/encoding.rs:1044-1059`, verbatim:
    ///
    /// ```text
    ///   let skip_key = key == &K::default();
    ///   let skip_val = val == val_default;
    ///   let len = (if skip_key { 0 } else { key_encoded_len(1, key) })
    ///           + (if skip_val { 0 } else { val_encoded_len(2, val) });
    ///   encode_key(tag, WireType::LengthDelimited, buf);
    ///   encode_varint(len as u64, buf);
    /// ```
    ///
    /// The entry is therefore a NODE with an id of its own, whose bounded part
    /// is the key and whose single child is the value. Its contribution to the
    /// map's owner is computed by [`Self::close`] like any other child's.
    fn open_map_entry(&mut self, key: &'a String, value: &'a Par, tag: u32) {
        let id = self.lens.len();
        self.lens.push(0);
        let key_part = if key.is_empty() {
            0
        } else {
            prost::encoding::string::encoded_len(1u32, key) as u64
        };
        self.frames.push(Frame {
            id: id as u32,
            tag,
            sum: key_part,
        });
        self.ops.push(Op::Close);
        // ⚠ `!=` against ONE `Par::default()`, through the HAND-WRITTEN
        // `PartialEq`. See [`Self::default_par`].
        if value != &self.default_par {
            // The value is a message child at tag 2, so it gets its own id and
            // its own frame — `close` then contributes
            // `key_len(2) + varint_len(v) + v`, which is exactly
            // `message::encoded_len(2, val)`.
            self.open(value, 2u32);
        }
    }

    /// Finalize the top frame.
    fn close(&mut self) {
        let frame = self
            .frames
            .pop()
            .expect("protobuf_encoder: Op::Close with no open frame");
        assert!(
            frame.sum <= u32::MAX as u64,
            "protobuf_encoder: a message node encodes to {} bytes, which exceeds the u32 length \
             table. Refusing is correct: a truncated length would be written as a varint \
             prefix that does not match the body.",
            frame.sum
        );
        self.lens[frame.id as usize] = frame.sum as u32;
        if let Some(parent) = self.frames.last_mut() {
            // Exactly `encoding::message::encoded_len` (`encoding.rs:845-852`),
            // read from the table instead of recursed:
            //   key_len(tag) + encoded_len_varint(len) + len
            parent.sum +=
                (key_len(frame.tag) + encoded_len_varint(frame.sum)) as u64 + frame.sum;
        } else {
            // The ROOT sits under no tag: its `encoded_len()` is its body, with
            // no key and no length prefix. `encode_to_vec` writes exactly that.
            debug_assert_eq!(
                frame.tag, ROOT_TAG,
                "protobuf_encoder: the outermost frame must carry the root sentinel tag"
            );
        }
    }

    fn open_seq(&mut self, seq: &'a dyn ProstSeq, len: usize, tag: u32) {
        assert!(
            len <= u32::MAX as usize,
            "protobuf_encoder: a repeated field of {len} elements exceeds the u32 cursor. Each \
             element carries its own key and length prefix, so a truncated cursor emits fewer \
             elements than the term contains — a corrupt encoding, not a slow one."
        );
        self.ops.push(Op::Seq {
            seq,
            index: 0,
            len: len as u32,
            tag,
        });
    }

    /// Push a resume point unless the program is spent — the TAIL CALL.
    #[inline(always)]
    fn suspend(&mut self, node: &'a dyn ProstNode, resume: u16) {
        if resume != NO_RESUME {
            self.ops.push(Op::Node {
                node,
                field: resume,
            });
        }
    }

    fn run<const TRACK: bool>(&mut self) {
        loop {
            if TRACK && self.ops.len() > self.high_water {
                self.high_water = self.ops.len();
            }
            let Some(op) = self.ops.pop() else { break };
            match op {
                Op::Node { node, field } => {
                    // ⚠ `EPathMap`: one opaque node, never decomposed. §4.
                    if field == 0 {
                        if let Some(opaque) = node.prost_opaque() {
                            self.frames
                                .last_mut()
                                .expect("protobuf_encoder: an opaque node with no open frame")
                                .sum += opaque.opaque_encoded_len() as u64;
                            continue;
                        }
                    }
                    let (bounded, descent) = node.prost_len_step(field as usize);
                    self.frames
                        .last_mut()
                        .expect("protobuf_encoder: a measured node with no open frame")
                        .sum += bounded;
                    match descent {
                        ProstDescent::Done => {}
                        ProstDescent::Node {
                            resume,
                            tag,
                            node: child,
                        } => {
                            self.suspend(node, resume);
                            self.open(child, tag);
                        }
                        ProstDescent::Seq {
                            resume,
                            tag,
                            len,
                            seq,
                        } => {
                            self.suspend(node, resume);
                            self.open_seq(seq, len, tag);
                        }
                        ProstDescent::Map { resume, tag, map } => {
                            self.suspend(node, resume);
                            self.map_iters.push(map.iter());
                            self.ops.push(Op::MapEntries { tag });
                        }
                    }
                }
                Op::Seq {
                    seq,
                    index,
                    len,
                    tag,
                } => {
                    if index < len {
                        // ★ Re-push SELF, not `n` children: the op stack stays
                        // Θ(depth) however WIDE the sequence is. ★★ …and not
                        // even self when this is the last element, which is the
                        // sequence's tail call.
                        if index + 1 < len {
                            self.ops.push(Op::Seq {
                                seq,
                                index: index + 1,
                                len,
                                tag,
                            });
                        }
                        self.open(seq.prost_get(index as usize), tag);
                    }
                }
                Op::MapEntries { tag } => {
                    let next = self
                        .map_iters
                        .last_mut()
                        .expect("protobuf_encoder: MapEntries with no live iterator")
                        .next();
                    match next {
                        Some((key, value)) => {
                            self.ops.push(Op::MapEntries { tag });
                            self.open_map_entry(key, value, tag);
                        }
                        None => {
                            self.map_iters.pop();
                        }
                    }
                }
                Op::Close => self.close(),
            }
        }
    }

    /// Measure `root` and hand back the finished table.
    fn measure<const TRACK: bool>(mut self, root: &'a dyn ProstNode) -> LenTable {
        // ⚠ An opaque ROOT still needs an id, so pass 2's cursor arithmetic is
        // uniform. `open` gives it one; its walk adds the opaque length.
        self.open(root, ROOT_TAG);
        self.run::<TRACK>();
        assert!(
            self.frames.is_empty(),
            "protobuf_encoder: {} frame(s) outlived the length pass — a node was opened and never \
             closed, so its length was never written and pass 2 would read a zero",
            self.frames.len()
        );
        assert!(
            self.map_iters.is_empty(),
            "protobuf_encoder: a map iterator outlived its field"
        );
        LenTable {
            lens: self.lens,
            high_water: self.high_water,
        }
    }
}

/// The memoized length table: `lens[id]` is the `encoded_len` of the node with
/// pre-order id `id`. `lens[0]` is the root's.
struct LenTable {
    lens: Vec<u32>,
    high_water: usize,
}

// ===========================================================================
// §C  Pass 2 — emit, top-down, one monotonic cursor
// ===========================================================================

struct EmitMachine<'a, 'l> {
    ops: Vec<Op<'a>>,
    lens: &'l [u32],
    /// The next unconsumed length. Starts at **1**: id 0 is the root, whose
    /// length nothing writes (`encode_to_vec` emits the body with no prefix).
    cursor: usize,
    map_iters: Vec<btree_map::Iter<'a, String, Par>>,
    default_par: Par,
    high_water: usize,
}

impl<'a, 'l> EmitMachine<'a, 'l> {
    fn new(lens: &'l [u32]) -> Self {
        EmitMachine {
            ops: Vec::with_capacity(OP_STACK_CAPACITY),
            lens,
            cursor: 1,
            map_iters: Vec::new(),
            default_par: Par::default(),
            high_water: 0,
        }
    }

    /// Write one child's `key ++ varint(len)` header and take its length slot.
    ///
    /// ★ THE CURSOR STEP, and the only one. Every length prefix in the output
    /// passes through here, in the order pass 1 allocated ids, which is why one
    /// monotonic index resolves them all.
    #[inline]
    fn open_child(&mut self, out: &mut Vec<u8>, tag: u32) {
        let len = *self.lens.get(self.cursor).unwrap_or_else(|| {
            panic!(
                "protobuf_encoder: the emit pass asked for length slot {} of {}. The two passes \
                 have visited nodes in DIFFERENT ORDERS, which means one of them walked a \
                 field, a sequence or a map differently from the other — and every length \
                 prefix from here on would belong to the wrong node.",
                self.cursor,
                self.lens.len()
            )
        });
        self.cursor += 1;
        encode_key(tag, WireType::LengthDelimited, out);
        encode_varint(len as u64, out);
    }

    #[inline(always)]
    fn suspend(&mut self, node: &'a dyn ProstNode, resume: u16) {
        if resume != NO_RESUME {
            self.ops.push(Op::Node {
                node,
                field: resume,
            });
        }
    }

    fn run<const TRACK: bool>(&mut self, out: &mut Vec<u8>) {
        loop {
            if TRACK && self.ops.len() > self.high_water {
                self.high_water = self.ops.len();
            }
            let Some(op) = self.ops.pop() else { break };
            match op {
                Op::Node { node, field } => {
                    if field == 0 {
                        if let Some(opaque) = node.prost_opaque() {
                            opaque.opaque_encode_raw(out);
                            continue;
                        }
                    }
                    match node.prost_emit(field as usize, out) {
                        ProstDescent::Done => {}
                        ProstDescent::Node {
                            resume,
                            tag,
                            node: child,
                        } => {
                            self.suspend(node, resume);
                            self.open_child(out, tag);
                            self.ops.push(Op::Node {
                                node: child,
                                field: 0,
                            });
                        }
                        ProstDescent::Seq {
                            resume,
                            tag,
                            len,
                            seq,
                        } => {
                            self.suspend(node, resume);
                            // The u32 bound was already refused by pass 1's
                            // `open_seq` over this same sequence; asserting
                            // again would be a second opinion about a value
                            // neither pass can have changed.
                            debug_assert!(len <= u32::MAX as usize);
                            self.ops.push(Op::Seq {
                                seq,
                                index: 0,
                                len: len as u32,
                                tag,
                            });
                        }
                        ProstDescent::Map { resume, tag, map } => {
                            self.suspend(node, resume);
                            self.map_iters.push(map.iter());
                            self.ops.push(Op::MapEntries { tag });
                        }
                    }
                }
                Op::Seq {
                    seq,
                    index,
                    len,
                    tag,
                } => {
                    if index < len {
                        if index + 1 < len {
                            self.ops.push(Op::Seq {
                                seq,
                                index: index + 1,
                                len,
                                tag,
                            });
                        }
                        // Each element carries its OWN key and length prefix.
                        self.open_child(out, tag);
                        self.ops.push(Op::Node {
                            node: seq.prost_get(index as usize),
                            field: 0,
                        });
                    }
                }
                Op::MapEntries { tag } => {
                    let next = self
                        .map_iters
                        .last_mut()
                        .expect("protobuf_encoder: MapEntries with no live iterator")
                        .next();
                    match next {
                        Some((key, value)) => {
                            self.ops.push(Op::MapEntries { tag });
                            // The ENTRY is a node: key ++ varint(entry_len).
                            self.open_child(out, tag);
                            if !key.is_empty() {
                                prost::encoding::string::encode(1u32, key, out);
                            }
                            if value != &self.default_par {
                                // …and its VALUE is a nested message at tag 2.
                                self.open_child(out, 2u32);
                                self.ops.push(Op::Node {
                                    node: value,
                                    field: 0,
                                });
                            }
                        }
                        None => {
                            self.map_iters.pop();
                        }
                    }
                }
                Op::Close => unreachable!(
                    "protobuf_encoder: `Op::Close` is a PASS-1 opcode. The emit pass needs no \
                     frame stack — it reads finished lengths from the table."
                ),
            }
        }
    }

    /// Assert the cursor consumed the table EXACTLY.
    ///
    /// ★ This is the second half of the structural agreement between the passes.
    /// [`Self::open_child`] catches a cursor that ran PAST the table; this
    /// catches one that stopped short — which is what a pass-2 walk that skipped
    /// a child would do, and which would otherwise produce a perfectly
    /// well-formed protobuf message that is simply missing a field.
    fn finish_emit(&self) {
        assert_eq!(
            self.cursor,
            self.lens.len(),
            "protobuf_encoder: the emit pass consumed {} of {} length slots. The length pass \
             visited nodes the emit pass did not, so the output is a well-formed protobuf \
             message with a field missing — which every decoder accepts.",
            self.cursor,
            self.lens.len()
        );
        assert!(
            self.map_iters.is_empty(),
            "protobuf_encoder: a map iterator outlived its field"
        );
    }
}

// ===========================================================================
// §D  Entry points
// ===========================================================================

/// `value`'s protobuf `encoded_len`, computed in **one** memoized pass.
///
/// ★ Separately valuable, and not merely a by-product: it is the
/// `` $\Theta(n)$ `` twin of `prost::Message::encoded_len`, which is
/// `` $\Theta(d^2)$ `` on a depth-`` $d$ `` chain because
/// `encoding::message::encoded_len` re-measures every subtree
/// (`prost-0.14.3/src/encoding.rs:845-852`). Anything that needs a size without
/// the bytes — a capacity hint, a fee estimate, a bound check — should prefer
/// this.
pub fn encoded_len<T: ProstNode>(value: &T) -> usize {
    let table = LenMachine::new().measure::<false>(value);
    table.lens[0] as usize
}

/// Encode `value`, **appending** to `out`.
///
/// Byte-identical to `prost::Message::encode_raw`, in Θ(depth) native stack and
/// Θ(n) work.
pub fn encode_into<T: ProstNode>(value: &T, out: &mut Vec<u8>) {
    let table = LenMachine::new().measure::<false>(value);
    out.reserve(table.lens[0] as usize);
    let mut machine = EmitMachine::new(&table.lens);
    machine.ops.push(Op::Node {
        node: value,
        field: 0,
    });
    machine.run::<false>(out);
    machine.finish_emit();
}

/// Encode `value` into a fresh `Vec<u8>`.
///
/// Byte-identical to `prost::Message::encode_to_vec`.
///
/// ★ Exactly-sized, and for free: the length pass already knows the answer, so
/// this allocates once and never grows — where `encode_to_vec` pays a full
/// recursive `encoded_len()` walk for the same information.
pub fn encode_to_vec<T: ProstNode>(value: &T) -> Vec<u8> {
    let table = LenMachine::new().measure::<false>(value);
    let mut out = Vec::with_capacity(table.lens[0] as usize);
    let mut machine = EmitMachine::new(&table.lens);
    machine.ops.push(Op::Node {
        node: value,
        field: 0,
    });
    machine.run::<false>(&mut out);
    machine.finish_emit();
    out
}

// ===========================================================================
// §E  Introspection for the space gate
// ===========================================================================

/// Op-stack high-water marks, `(length pass, emit pass)`.
///
/// ★ Exists because "the op stack is Θ(term DEPTH), not Θ(term SIZE)" is a claim
/// about a *mechanism*: a bug that pushed children eagerly would turn a 10-deep,
/// 1,000,000-node term into a 1,000,000-entry stack while every correctness test
/// still passed.
///
/// The measurement runs the production loops with `TRACK = true`, so it cannot
/// drift into describing a different machine.
pub fn op_stack_high_water<T: ProstNode>(value: &T) -> (usize, usize) {
    let table = LenMachine::new().measure::<true>(value);
    let mut out = Vec::with_capacity(OUT_CAPACITY);
    let mut machine = EmitMachine::new(&table.lens);
    machine.ops.push(Op::Node {
        node: value,
        field: 0,
    });
    machine.run::<true>(&mut out);
    machine.finish_emit();
    (table.high_water, machine.high_water)
}

/// How many message nodes the length table held — the Θ(n) space this encoder
/// trades for prost's Θ(d²) time.
pub fn len_table_size<T: ProstNode>(value: &T) -> usize {
    LenMachine::new().measure::<false>(value).lens.len()
}

/// `size_of::<Op>()`, exposed so the space gate can pin it.
pub const fn op_size() -> usize {
    std::mem::size_of::<Op<'static>>()
}

/// `size_of::<Frame>()` — the pass-1 frame stack's per-level cost.
pub const fn frame_size() -> usize {
    std::mem::size_of::<Frame>()
}

// A `BTreeMap<String, Par>` is named in `ProstDescent::Map`; this alias keeps
// the import above load-bearing and documents the one map shape in the schema.
#[allow(dead_code)]
type InjectionsMap = BTreeMap<String, Par>;
