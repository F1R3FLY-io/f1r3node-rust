(* D-O6 (epic 8946, Phase D item D-B2; decision record DR-93): the work of
   a nested prost encode.

   A message is a tree: Message own children, where own is the bytes that
   one traversal reads for the message itself (its scalar fields, the
   headers of its nested fields and the payloads of its byte and string
   fields) and children are its nested messages (map entries included).

   Prost computes the encoded length of a message by reading its own bytes
   and computing the lengths of its nested messages, one traversal:
     len_work m = own m + sum of len_work c.
   It encodes a message by writing its own fields and, for each nested
   message, computing that message's length for the length prefix and
   encoding it:
     encode_work m = own m + sum of (len_work c + encode_work c).
   A site that encodes a message first computes its length to size the
   output buffer: site_work m = len_work m + encode_work m. The writes of
   the output are its encoded length, which the site reserves separately.

   Results:
   - encode_work_le_height: encode_work m <= (1 + height m) * len_work m;
   - nested_encode_work_le_depth_times_visited_blocks:
     site_work m <= (2 + height m) * len_work m;
   - depth_weighted_charge_covers_encode: if a block inspection charge C
     covers one traversal (len_work m <= C) and the walker's depth d of the
     message is at least 1 + height m (every nested message is at least one
     worklist level below its parent), then the inspection C and the extra
     reservation d * C cover site_work m;
   - linear_charge_undercounts_deep_chain (negative control): for a chain of
     n + 1 messages of s bytes each, site_work is quadratic in n, so a
     charge of two traversals does not cover it.

   Rust correspondence: shared/src/rust/clone_backing.rs
   (inspect_blocks_depth returns the walker depth d and the scanned charge
   C of a block-mode inspection; reserve_nested_encode reserves the depth
   walk, then d * C and the output bytes); SorterMeter::nested_encode in
   models/src/rust/rholang/sorter/metered.rs; the sites that encode a
   nested message after a walker inspection, in
   rholang/src/rust/interpreter/accounting/authority.rs and
   authority/fallback_metered.rs (inventory in DR-93). The premise
   d >= 1 + height holds because every nested message is a worklist entry
   at least one level below its parent's entry; the Rust test
   nested_encode_charge_covers_counted_reads checks it on generated terms,
   with deep_chain_charge_grows_with_depth and
   nested_encode_sites_charge_quadratically_in_depth. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Inductive message := Message (own : nat) (children : list message).

Fixpoint len_work (m : message) : nat :=
  match m with
  | Message own cs => own + fold_right (fun c total => len_work c + total) 0 cs
  end.

Fixpoint encode_work (m : message) : nat :=
  match m with
  | Message own cs => own + fold_right (fun c total => len_work c + encode_work c + total) 0 cs
  end.

Definition site_work (m : message) : nat := len_work m + encode_work m.

Fixpoint height (m : message) : nat :=
  match m with
  | Message _ cs => fold_right (fun c deepest => Nat.max (1 + height c) deepest) 0 cs
  end.

Lemma child_height_lt : forall cs c, In c cs ->
  1 + height c <= fold_right (fun c' deepest => Nat.max (1 + height c') deepest) 0 cs.
Proof.
  induction cs as [| head rest IH]; intros c member; [contradiction |].
  cbn [fold_right In] in member |- *. destruct member as [-> | member]; [lia |].
  specialize (IH c member). lia.
Qed.

Theorem encode_work_le_height : forall m, encode_work m <= (1 + height m) * len_work m.
Proof.
  fix IH 1. intros [own cs]. cbn [encode_work len_work height].
  remember (fold_right (fun c deepest => Nat.max (1 + height c) deepest) 0 cs) as h eqn:h_def.
  assert (all_bounded : forall c, In c cs -> 1 + height c <= h).
  { intros c member. rewrite h_def. exact (child_height_lt cs c member). }
  clear h_def.
  assert (children : forall cs',
    (forall c, In c cs' -> 1 + height c <= h) ->
    fold_right (fun c total => len_work c + encode_work c + total) 0 cs'
    <= (1 + h) * fold_right (fun c total => len_work c + total) 0 cs').
  { induction cs' as [| c rest IHrest]; intros bounded; cbn [fold_right]; [lia |].
    pose proof (IH c) as child.
    assert (child_bound : 1 + height c <= h) by (apply bounded; left; reflexivity).
    assert (rest_bounded : forall c', In c' rest -> 1 + height c' <= h)
      by (intros c' member; apply bounded; right; exact member).
    specialize (IHrest rest_bounded).
    assert (scaled : (1 + height c) * len_work c <= h * len_work c)
      by (apply Nat.mul_le_mono_r; exact child_bound).
    assert (step : len_work c + encode_work c <= (1 + h) * len_work c) by lia.
    lia. }
  specialize (children cs all_bounded).
  nia.
Qed.

Theorem nested_encode_work_le_depth_times_visited_blocks : forall m,
  site_work m <= (2 + height m) * len_work m.
Proof.
  intros m. unfold site_work. pose proof (encode_work_le_height m). nia.
Qed.

Theorem depth_weighted_charge_covers_encode : forall m charge depth,
  len_work m <= charge -> 1 + height m <= depth ->
  site_work m <= charge + depth * charge.
Proof.
  intros m charge depth one_traversal deep_enough.
  pose proof (nested_encode_work_le_depth_times_visited_blocks m). nia.
Qed.

(* Negative control: a chain of n + 1 messages of s bytes. *)
Fixpoint chain (n s : nat) : message :=
  match n with
  | 0 => Message s []
  | S n' => Message s [chain n' s]
  end.

Lemma chain_len_work : forall n s, len_work (chain n s) = (n + 1) * s.
Proof. induction n as [| n IH]; intros s; cbn [chain len_work fold_right]; [lia |]. rewrite IH. lia. Qed.

Lemma chain_encode_work : forall n s, 2 * encode_work (chain n s) = (n + 1) * (n + 2) * s.
Proof.
  induction n as [| n IH]; intros s; cbn [chain encode_work fold_right]; [lia |].
  rewrite chain_len_work. pose proof (IH s). nia.
Qed.

Theorem linear_charge_undercounts_deep_chain : forall n s,
  3 <= n -> 1 <= s -> 2 * len_work (chain n s) < site_work (chain n s).
Proof.
  intros n s deep positive. unfold site_work.
  rewrite chain_len_work. pose proof (chain_encode_work n s). nia.
Qed.

Example deep_chain_example :
  2 * len_work (chain 3 100) = 800 /\ site_work (chain 3 100) = 1400.
Proof. split; vm_compute; reflexivity. Qed.

Print Assumptions encode_work_le_height.
Print Assumptions nested_encode_work_le_depth_times_visited_blocks.
Print Assumptions depth_weighted_charge_covers_encode.
Print Assumptions linear_charge_undercounts_deep_chain.
Print Assumptions deep_chain_example.
