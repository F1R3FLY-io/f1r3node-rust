(* DR-74: native evidence encodes each causal path as a delta from the
   previous path.

   Each path is encoded as the length of the prefix that it shares with the
   previous path, the suffix length, and the suffix segments. The encoder
   always emits the maximal shared prefix, and the decoder rejects a
   non-maximal prefix, so each recording has exactly one encoding.

   Rust correspondence (rholang native_runtime/wire.rs):
   - [lcp], [encode_one]: [write_path_delta] (chunked maximal-prefix scan);
   - [decode_one]: [read_path_delta], including its bounds
     (prefix <= min(previous length, maximum), suffix <= maximum - prefix) and
     its canonicality check (a nonempty suffix whose head equals the previous
     path at the prefix position is malformed);
   - [encode_all], [decode_all]: the attempt-then-retry chain of
     [encode_native_budget_recording] and [decode_native_budget_recording]. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool Lia.
Import ListNotations.

Section DeltaCodec.

Context {A : Type} (eq_dec : forall x y : A, {x = y} + {x <> y}).

Fixpoint lcp (xs ys : list A) : nat :=
  match xs, ys with
  | x :: xs', y :: ys' => if eq_dec x y then S (lcp xs' ys') else 0
  | _, _ => 0
  end.

Lemma lcp_le_l : forall xs ys, lcp xs ys <= length xs.
Proof.
  induction xs as [|x xs IH]; intros [|y ys]; simpl; try lia.
  destruct (eq_dec x y); simpl; [specialize (IH ys); lia | lia].
Qed.

Lemma lcp_le_r : forall xs ys, lcp xs ys <= length ys.
Proof.
  induction xs as [|x xs IH]; intros [|y ys]; simpl; try lia.
  destruct (eq_dec x y); simpl; [specialize (IH ys); lia | lia].
Qed.

Lemma lcp_firstn : forall xs ys, firstn (lcp xs ys) xs = firstn (lcp xs ys) ys.
Proof.
  induction xs as [|x xs IH]; intros [|y ys]; simpl; try reflexivity.
  destruct (eq_dec x y) as [same | different]; simpl.
  - subst y. f_equal. apply IH.
  - reflexivity.
Qed.

Lemma lcp_app : forall prefix xs ys,
  lcp (prefix ++ xs) (prefix ++ ys) = length prefix + lcp xs ys.
Proof.
  induction prefix as [|segment prefix IH]; intros xs ys; simpl; [reflexivity |].
  destruct (eq_dec segment segment) as [_ | different];
    [rewrite IH; reflexivity | contradiction].
Qed.

(* At the position [lcp xs ys] the two paths differ, or one of them ends. *)
Lemma lcp_maximal : forall xs ys x y,
  nth_error xs (lcp xs ys) = Some x ->
  nth_error ys (lcp xs ys) = Some y ->
  x <> y.
Proof.
  induction xs as [|a xs IH]; intros [|b ys] x y left right; simpl in *;
    try discriminate.
  destruct (eq_dec a b) as [same | different]; simpl in *.
  - exact (IH ys x y left right).
  - injection left as <-. injection right as <-. exact different.
Qed.

Lemma nth_error_of_skipn_head : forall (l : list A) n x rest,
  skipn n l = x :: rest -> nth_error l n = Some x.
Proof.
  induction l as [|a l IH]; intros [|n] x rest shifted; simpl in *.
  - discriminate.
  - discriminate.
  - injection shifted as <- _. reflexivity.
  - exact (IH n x rest shifted).
Qed.

Lemma skipn_length_app : forall (l m : list A), skipn (length l) (l ++ m) = m.
Proof.
  induction l as [|a l IH]; intros m; simpl; [reflexivity | apply IH].
Qed.

Definition encode_one (previous path : list A) : nat * list A :=
  (lcp path previous, skipn (lcp path previous) path).

Definition prefix_canonical (previous : list A) (p : nat) (suffix : list A) : bool :=
  match suffix, nth_error previous p with
  | s :: _, Some q => if eq_dec s q then false else true
  | _, _ => true
  end.

Definition decode_one (maximum : nat) (previous : list A) (entry : nat * list A)
    : option (list A) :=
  let (p, suffix) := entry in
  if Nat.leb p (Nat.min (length previous) maximum)
     && Nat.leb (length suffix) (maximum - p)
     && prefix_canonical previous p suffix
  then Some (firstn p previous ++ suffix)
  else None.

(* The encoder's output always decodes to the original path. *)
Theorem encode_one_decodes : forall maximum previous path,
  length path <= maximum ->
  decode_one maximum previous (encode_one previous path) = Some path.
Proof.
  intros maximum previous path bounded.
  unfold encode_one, decode_one.
  set (p := lcp path previous).
  assert (in_previous : p <= length previous) by apply lcp_le_r.
  assert (in_path : p <= length path) by apply lcp_le_l.
  assert (prefix_ok : Nat.leb p (Nat.min (length previous) maximum) = true).
  { apply Nat.leb_le. lia. }
  assert (suffix_ok : Nat.leb (length (skipn p path)) (maximum - p) = true).
  { apply Nat.leb_le. rewrite length_skipn. lia. }
  assert (canonical : prefix_canonical previous p (skipn p path) = true).
  { unfold prefix_canonical.
    destruct (skipn p path) as [|s rest] eqn:shifted; [reflexivity |].
    destruct (nth_error previous p) as [q |] eqn:at_previous; [| reflexivity].
    destruct (eq_dec s q) as [same | different]; [| reflexivity].
    exfalso.
    apply (lcp_maximal path previous s q);
      [ exact (nth_error_of_skipn_head path p s rest shifted)
      | exact at_previous
      | exact same ]. }
  rewrite prefix_ok, suffix_ok, canonical. simpl.
  f_equal.
  unfold p. rewrite <- lcp_firstn. apply firstn_skipn.
Qed.

(* Every accepted encoding is the encoder's output for the decoded path. *)
Theorem decode_one_canonical : forall maximum previous entry path,
  decode_one maximum previous entry = Some path ->
  encode_one previous path = entry.
Proof.
  intros maximum previous [p suffix] path decoded.
  unfold decode_one in decoded.
  destruct (Nat.leb p (Nat.min (length previous) maximum)) eqn:prefix_ok;
    [| discriminate].
  destruct (Nat.leb (length suffix) (maximum - p)) eqn:suffix_ok; [| discriminate].
  destruct (prefix_canonical previous p suffix) eqn:canonical; [| discriminate].
  simpl in decoded. injection decoded as <-.
  apply Nat.leb_le in prefix_ok.
  assert (in_previous : p <= length previous) by lia.
  assert (prefix_length : length (firstn p previous) = p)
    by (apply firstn_length_le; exact in_previous).
  assert (shared : lcp (firstn p previous ++ suffix) previous = p).
  { rewrite <- (firstn_skipn p previous) at 2.
    rewrite lcp_app, prefix_length.
    enough (lcp suffix (skipn p previous) = 0) by lia.
    unfold prefix_canonical in canonical.
    destruct suffix as [|s rest]; [reflexivity |].
    destruct (skipn p previous) as [|q later] eqn:shifted; [reflexivity |].
    rewrite (nth_error_of_skipn_head previous p q later shifted) in canonical.
    simpl. destruct (eq_dec s q) as [same | different]; [discriminate | reflexivity]. }
  unfold encode_one. rewrite shared. f_equal.
  rewrite <- prefix_length at 1. apply skipn_length_app.
Qed.

Fixpoint encode_all (previous : list A) (paths : list (list A)) : list (nat * list A) :=
  match paths with
  | [] => []
  | path :: rest => encode_one previous path :: encode_all path rest
  end.

Fixpoint decode_all (maximum : nat) (previous : list A)
    (entries : list (nat * list A)) : option (list (list A)) :=
  match entries with
  | [] => Some []
  | entry :: rest =>
      match decode_one maximum previous entry with
      | Some path =>
          match decode_all maximum path rest with
          | Some paths => Some (path :: paths)
          | None => None
          end
      | None => None
      end
  end.

(* Round trip: decoding the encoding of any bounded path chain returns it. *)
Theorem delta_codec_round_trip : forall maximum paths previous,
  Forall (fun path => length path <= maximum) paths ->
  decode_all maximum previous (encode_all previous paths) = Some paths.
Proof.
  intros maximum paths.
  induction paths as [|path rest IH]; intros previous bounded;
    cbn [encode_all decode_all]; [reflexivity |].
  inversion bounded as [| ? ? path_bounded rest_bounded]; subst.
  rewrite (encode_one_decodes maximum previous path path_bounded).
  rewrite (IH path rest_bounded).
  reflexivity.
Qed.

(* Canonicity: an accepted encoding is the encoding of its decoded chain. *)
Theorem delta_codec_canonical : forall maximum entries previous paths,
  decode_all maximum previous entries = Some paths ->
  encode_all previous paths = entries.
Proof.
  intros maximum entries.
  induction entries as [|entry rest IH]; intros previous paths decoded;
    cbn [decode_all] in decoded.
  - injection decoded as <-. reflexivity.
  - destruct (decode_one maximum previous entry) as [path |] eqn:first;
      [| discriminate].
    destruct (decode_all maximum path rest) as [later |] eqn:remaining;
      [| discriminate].
    injection decoded as <-. cbn [encode_all].
    rewrite (decode_one_canonical maximum previous entry path first).
    rewrite (IH path later remaining).
    reflexivity.
Qed.

(* Each path chain has exactly one accepted encoding. *)
Corollary delta_encoding_unique : forall maximum previous paths first second,
  decode_all maximum previous first = Some paths ->
  decode_all maximum previous second = Some paths ->
  first = second.
Proof.
  intros maximum previous paths first second first_decoded second_decoded.
  rewrite <- (delta_codec_canonical maximum first previous paths first_decoded).
  exact (delta_codec_canonical maximum second previous paths second_decoded).
Qed.

End DeltaCodec.

Print Assumptions encode_one_decodes.
Print Assumptions decode_one_canonical.
Print Assumptions delta_codec_round_trip.
Print Assumptions delta_codec_canonical.
Print Assumptions delta_encoding_unique.
